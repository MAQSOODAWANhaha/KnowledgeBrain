"""Independent review and deterministic domain validation."""

from docreader.tender.checks.aggregate import apply_applicability_decisions, verify_all
from docreader.tender.checks.verify import verify_pack

__all__ = ["verify_pack", "verify_all", "apply_applicability_decisions"]
