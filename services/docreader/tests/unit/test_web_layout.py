"""Layout conversion reads only a signed, execution-scoped candidate."""

import base64
import asyncio
import hashlib
import hmac
import json

import httpx
import pytest

from docreader.integrations.web_layout import LayoutRenderError, WebLayoutRenderer


def renderer(handler, **options):
    return WebLayoutRenderer(execution_id='document-job', base_url='http://internal.test',
        service_secret='synthetic-service-secret', transport=httpx.MockTransport(handler), **options)


def response(request, blob=b'%PDF-synthetic-layout', **headers):
    values = {'content-type': 'application/pdf', 'x-kb-render-engine': 'onlyoffice-converter-v1',
        'x-kb-render-service-sha256': hashlib.sha256(b'http://office.test/').hexdigest(),
        'x-kb-render-source-sha256': hashlib.sha256(request.content).hexdigest(),
        'x-kb-render-pdf-sha256': hashlib.sha256(blob).hexdigest(), **headers}
    return httpx.Response(200, content=blob, headers=values)


def test_render_token_binds_epoch_and_exact_bytes_without_persisting_credentials():
    calls = []

    def handle(request):
        calls.append(request)
        assert request.url == 'http://internal.test/api/internal/tender-render/document-job'
        assert request.headers['content-type'].endswith('wordprocessingml.document')
        token = request.headers['authorization'].removeprefix('Bearer ')
        header, payload, signed = token.split('.')
        claims = json.loads(base64.urlsafe_b64decode(payload + '=' * (-len(payload) % 4)))
        assert claims['purpose'] == 'tender_docx_render_v1'
        assert claims['execution_id'] == 'document-job' and claims['epoch'] == 7
        assert claims['docx_sha256'] == hashlib.sha256(b'original candidate').hexdigest()
        expected = hmac.new(b'synthetic-service-secret', (header + '.' + payload).encode(), hashlib.sha256).digest()
        assert base64.urlsafe_b64decode(signed + '=' * (-len(signed) % 4)) == expected
        return response(request)

    client = renderer(handle)
    pdf, engine = client.render(b'original candidate', epoch=7)
    assert pdf == b'%PDF-synthetic-layout'
    assert engine == 'onlyoffice-converter-v1:' + hashlib.sha256(b'http://office.test/').hexdigest()
    assert len(calls) == 1
    assert 'synthetic-service-secret' not in json.dumps(client.spec())
    assert not {'epoch', 'token', 'authorization'} & client.spec().keys()


@pytest.mark.parametrize('headers', [
    {'x-kb-render-source-sha256': '0' * 64},
    {'x-kb-render-pdf-sha256': '0' * 64},
    {'x-kb-render-engine': ''},
    {'x-kb-render-service-sha256': 'not-a-digest'},
    {'content-type': 'text/html'},
])
def test_unbound_or_non_pdf_conversion_is_not_accepted(headers):
    with pytest.raises(LayoutRenderError):
        renderer(lambda request: response(request, **headers)).render(b'candidate', epoch=1)


def test_conversion_redirect_is_never_followed():
    calls = []

    def handle(request):
        calls.append(request)
        return httpx.Response(302, headers={'location': 'http://untrusted.test/steal'})

    with pytest.raises(LayoutRenderError):
        renderer(handle).render(b'candidate', epoch=1)
    assert len(calls) == 1


@pytest.mark.parametrize('origin', [None, 'http://user:password@internal.test',
    'http://internal.test/path', 'http://internal.test/?token=secret'])
def test_missing_or_ambiguous_service_configuration_has_a_local_failure(origin):
    def forbidden(request):
        pytest.fail('Invalid renderer configuration must not send a request.')

    client = WebLayoutRenderer(execution_id='job', base_url=origin, service_secret='synthetic',
                               transport=httpx.MockTransport(forbidden))
    with pytest.raises(LayoutRenderError):
        client.render(b'candidate', epoch=1)


def test_limits_apply_before_send_and_during_bounded_download(monkeypatch):
    from docreader.integrations import web_layout
    monkeypatch.setattr(web_layout, 'MAX_DOCUMENT_BYTES', 8)
    with pytest.raises(LayoutRenderError):
        renderer(lambda request: pytest.fail('Oversized upload must not be sent.')).render(b'123456789', epoch=1)
    with pytest.raises(LayoutRenderError):
        renderer(lambda request: response(request, b'%PDF-123456789')).render(b'input', epoch=1)


def test_renderer_exceptions_do_not_expose_tokens_or_endpoint_details():
    def fail(request):
        raise httpx.ReadTimeout('secret diagnostic from http://private.test/?token=hidden', request=request)

    with pytest.raises(LayoutRenderError) as error:
        renderer(fail).render(b'candidate', epoch=1)
    assert 'hidden' not in str(error.value) and 'private.test' not in str(error.value)


@pytest.mark.parametrize('phase', ['headers', 'body'])
def test_total_deadline_cancels_stalled_transport_and_closes_the_stream(monkeypatch, phase):
    from docreader.integrations import web_layout
    monkeypatch.setattr(web_layout, 'TOTAL_SECONDS', 0.01)
    cancelled, closed = [], []

    class Stalled(httpx.AsyncByteStream):
        async def __aiter__(self):
            yield b'%PDF-'
            try:
                await asyncio.sleep(1)
            finally:
                cancelled.append(True)
            yield b'fixture'

        async def aclose(self):
            closed.append(True)

    async def handle(request):
        if phase == 'headers':
            try:
                await asyncio.sleep(1)
            finally:
                cancelled.append(True)
            return response(request)
        ready = response(request, b'%PDF-fixture')
        return httpx.Response(200, headers=ready.headers, stream=Stalled())

    with pytest.raises(LayoutRenderError):
        renderer(handle).render(b'candidate', epoch=1)
    assert cancelled
    assert bool(closed) is (phase == 'body')
