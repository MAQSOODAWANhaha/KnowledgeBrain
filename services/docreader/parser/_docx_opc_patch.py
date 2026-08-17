"""Apply python-docx OPC patch before any `docx` Document import.

See https://github.com/python-openxml/python-docx/issues/1105

Import this module (or call `ensure_patched`) before `from docx import Document`.
The patch lives here so `docx_parser` can keep all its own imports at the top
of that file after the patch is applied.
"""

from docx.opc.oxml import parse_xml
from docx.opc.pkgreader import _SerializedRelationship, _SerializedRelationships

_APPLIED = False


def load_from_xml_v2(baseURI, rels_item_xml):
    """Load relationships, skipping broken NULL targets."""
    srels = _SerializedRelationships()
    if rels_item_xml is not None:
        rels_elm = parse_xml(rels_item_xml)
        for rel_elm in rels_elm.Relationship_lst:
            if rel_elm.target_ref in ("../NULL", "NULL"):
                continue
            srels._srels.append(_SerializedRelationship(baseURI, rel_elm))
    return srels


def ensure_patched() -> bool:
    """Install the NULL-target skip. Idempotent; safe to call more than once."""
    global _APPLIED
    if not _APPLIED:
        _SerializedRelationships.load_from_xml = load_from_xml_v2
        _APPLIED = True
    return _APPLIED


ensure_patched()
