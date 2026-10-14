"""Versioned, packaged domain prompts; source documents are untrusted data."""

from functools import lru_cache
from importlib.resources import files

PROMPT_VERSION = "tender-phase1-v1"


@lru_cache(maxsize=4)
def get_prompt(task: str) -> str:
    if task not in {"extract", "verify", "plan", "ocr"}:
        raise ValueError(f"Unknown tender model task: {task}")
    return files(__package__).joinpath(f"{task}.md").read_text(encoding="utf-8")


__all__ = ["get_prompt", "PROMPT_VERSION"]
