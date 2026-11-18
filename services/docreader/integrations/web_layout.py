"""Short-lived layout conversion bound to a live authoring execution lease."""

from __future__ import annotations

import base64
import asyncio
import hashlib
import hmac
import json
import os
import re
import time
from urllib.parse import quote

import httpx


MAX_DOCUMENT_BYTES = 50 * 1024 * 1024
RENDER_CONTRACT = 'knowledgebrain_onlyoffice_candidate_v1'
ENGINE = 'onlyoffice-converter-v1'
TOTAL_SECONDS = 195


class LayoutRenderError(RuntimeError):
    """Safe conversion failure; transport diagnostics and credentials stay private."""


def _segment(value):
    return base64.urlsafe_b64encode(value).rstrip(b'=').decode('ascii')


class WebLayoutRenderer:
    def __init__(self, *, execution_id, base_url, service_secret, transport=None):
        self.execution_id, self.service_secret, self.transport = execution_id, service_secret, transport
        self.base_url = None
        try:
            url = httpx.URL(base_url or '')
            if (url.scheme in {'http', 'https'} and url.host and not (url.userinfo or url.query or url.fragment)
                    and url.path in {'', '/'}):
                self.base_url = str(url).rstrip('/')
        except httpx.InvalidURL:
            pass

    def spec(self):
        return {'contract': RENDER_CONTRACT, 'api_origin': self.base_url,
                'max_document_bytes': MAX_DOCUMENT_BYTES, 'timeout_seconds': TOTAL_SECONDS,
                'page_policy': 'single_section_continuous_arabic_v1'}

    def _token(self, docx_sha256, epoch):
        header = _segment(b'{"alg":"HS256","typ":"JWT"}')
        claims = _segment(json.dumps({'purpose': 'tender_docx_render_v1', 'execution_id': self.execution_id,
            'docx_sha256': docx_sha256, 'epoch': epoch, 'exp': int(time.time()) + 60}, separators=(',', ':')).encode())
        signed = header + '.' + claims
        signature = hmac.new(self.service_secret.encode(), signed.encode(), hashlib.sha256).digest()
        return signed + '.' + _segment(signature)

    def render(self, docx_bytes, *, epoch):
        if (not self.base_url or not isinstance(self.service_secret, str) or not self.service_secret.strip()
                or not isinstance(self.execution_id, str) or not self.execution_id.strip()
                or type(epoch) is not int or epoch < 1):
            raise LayoutRenderError('LAYOUT_RENDER_UNAVAILABLE')
        if not isinstance(docx_bytes, bytes) or not 0 < len(docx_bytes) <= MAX_DOCUMENT_BYTES:
            raise LayoutRenderError('LAYOUT_DOCUMENT_LIMIT')
        digest = hashlib.sha256(docx_bytes).hexdigest()
        try:
            return asyncio.run(self._render(docx_bytes, digest, epoch))
        except (httpx.HTTPError, TimeoutError):
            raise LayoutRenderError('LAYOUT_RENDER_UNAVAILABLE') from None

    async def _render(self, docx_bytes, digest, epoch):
        endpoint = f'{self.base_url}/api/internal/tender-render/{quote(self.execution_id, safe="")}'
        async with asyncio.timeout(TOTAL_SECONDS):
            async with httpx.AsyncClient(transport=self.transport,
                    timeout=httpx.Timeout(TOTAL_SECONDS, connect=min(10, TOTAL_SECONDS)),
                    follow_redirects=False) as client:
                async with client.stream('POST', endpoint, content=docx_bytes, headers={
                    'authorization': 'Bearer ' + self._token(digest, epoch),
                    'content-type': 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
                }) as response:
                    if response.status_code != 200:
                        raise LayoutRenderError('LAYOUT_RENDER_REJECTED')
                    service = response.headers.get('x-kb-render-service-sha256', '')
                    if (response.headers.get('content-type', '').split(';')[0].strip().lower() != 'application/pdf'
                            or response.headers.get('x-kb-render-source-sha256') != digest
                            or response.headers.get('x-kb-render-engine') != ENGINE
                            or not re.fullmatch(r'[0-9a-f]{64}', service)):
                        raise LayoutRenderError('LAYOUT_RENDER_BINDING_INVALID')
                    declared = response.headers.get('content-length')
                    if declared is not None and (not declared.isdigit() or int(declared) > MAX_DOCUMENT_BYTES):
                        raise LayoutRenderError('LAYOUT_PDF_LIMIT')
                    chunks, size = [], 0
                    async for chunk in response.aiter_bytes():
                        size += len(chunk)
                        if size > MAX_DOCUMENT_BYTES:
                            raise LayoutRenderError('LAYOUT_PDF_LIMIT')
                        chunks.append(chunk)
                    pdf = b''.join(chunks)
                    if (not pdf.startswith(b'%PDF-') or response.headers.get('x-kb-render-pdf-sha256')
                            != hashlib.sha256(pdf).hexdigest()):
                        raise LayoutRenderError('LAYOUT_PDF_BINDING_INVALID')
            return pdf, ENGINE + ':' + service


def build_web_layout(*, execution_id):
    return WebLayoutRenderer(execution_id=execution_id,
        base_url=os.environ.get('KNOWLEDGEBRAIN_INTERNAL_API_URL'),
        service_secret=os.environ.get('GRPC_AUTH_TOKEN'))
