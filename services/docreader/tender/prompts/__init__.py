"""Versioned, packaged domain prompts; source documents are untrusted data."""

from functools import lru_cache
from importlib.resources import files
from typing import get_args

from docreader.tender.schema import Task

PROMPT_VERSION = "tender-phase1-v1"
# Live analysis identity. The unused plan prompt stays loadable for its own tests.
ANALYSIS_PROMPT_TASKS = ("extract", "verify", "ocr")


@lru_cache(maxsize=len(get_args(Task)))
def get_prompt(task: Task) -> str:
    if task not in get_args(Task):
        raise ValueError(f"Unknown tender model task: {task}")
    return (
        files("docreader.tender.prompts")
        .joinpath(f"{task}.md")
        .read_text(encoding="utf-8")
    )


__all__ = ["get_prompt", "PROMPT_VERSION", "ANALYSIS_PROMPT_TASKS"]
