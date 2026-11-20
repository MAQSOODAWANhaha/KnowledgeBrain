"""Select source lines by short refs; the host retains every physical coordinate."""

from copy import deepcopy
import hashlib
import json

from docreader.llm.request import ModelContractError
from docreader.tender.directory_compile import _Source


def _rejected(detail):
    return ModelContractError([{'path': [], 'code': 'directory_source_ref_invalid', 'detail': detail}])


_DERIVED_FIELDS = frozenset({'layout_decoration', 'continuations', 'body_projection'})


class SourceRefs:
    def __init__(self, documents, identity=None):
        _Source(documents)
        self.documents = deepcopy(documents)
        self.identity = {key: identity.get(key) for key in ('source_version', 'domain_revision')} if identity else {}
        self.lines, self.by_point = [], {}
        for document in self.documents:
            for page, text in enumerate(document['pages']):
                for line, content in enumerate(text.splitlines()):
                    point = document['document_id'], page, line
                    self.by_point[point] = len(self.lines)
                    self.lines.append((*point, content))
        # Derived presentation fields are not source identity; keep saved projections and receipts valid.
        projection = [{key: value for key, value in document.items() if key not in _DERIVED_FIELDS}
                      for document in self.documents]
        encoded = json.dumps(projection, ensure_ascii=False, sort_keys=True, separators=(',', ':'))
        self.digest = hashlib.sha256(encoded.encode()).hexdigest()

    def reading(self, pages=None):
        documents = []
        pages = frozenset(pages) if pages is not None else None
        known = {(document['document_id'], page) for document in self.documents
                 for page in range(len(document['pages']))}
        if pages is not None and not set(pages) <= known:
            raise ValueError('source reading contains an unknown physical page')
        for document in self.documents:
            selected = []
            for page, text in enumerate(document['pages']):
                if pages is None or (document['document_id'], page) in pages:
                    selected.append({'page': page, 'lines': [
                        [self.by_point[(document['document_id'], page, line)], content]
                        for line, content in enumerate(text.splitlines())]})
            if selected:
                documents.append({'document_id': document['document_id'], 'role': document['role'], 'pages': selected})
        return {'documents': documents, 'source_ref_format': 'host_bound_line_refs_v1',
                'source_projection_digest': self.digest, **self.identity}

    def reference(self, point):
        return self.by_point[(point['document_id'], point['page'], point['line'])]

    def _line(self, reference, visible=None):
        if type(reference) is not int or not 0 <= reference < len(self.lines):
            raise _rejected('source ref must be a known integer')
        if visible is not None and reference not in visible:
            raise _rejected('source ref is outside the supplied reading')
        return self.lines[reference]

    def point(self, reference, *, visible=None, nonempty=False):
        document_id, page, line, text = self._line(reference, visible)
        if nonempty and not text.strip():
            raise _rejected('heading ref must select nonempty source text')
        return {'document_id': document_id, 'page': page, 'line': line}

    def text(self, reference):
        return self._line(reference)[3]

    def windows(self, start, end, *, visible=None):
        first, last = self._line(start, visible), self._line(end, visible)
        if end < start or first[0] != last[0]:
            raise _rejected('source range must run forward inside one document')
        result = []
        for reference in range(start, end + 1):
            document_id, page, line, _ = self._line(reference, visible)
            if result and result[-1]['page'] == page and result[-1]['end_line'] + 1 == line:
                result[-1]['end_line'] = line
            else:
                result.append({'document_id': document_id, 'page': page, 'start_line': line, 'end_line': line})
        return result
