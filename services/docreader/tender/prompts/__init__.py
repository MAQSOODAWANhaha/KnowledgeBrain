"""Versioned, packaged domain prompts; source documents are untrusted data."""

from functools import lru_cache
from importlib.resources import files
from typing import get_args

from docreader.tender.schema import Task

PROMPT_VERSION = "tender-phase1-v1"


@lru_cache(maxsize=len(get_args(Task)))
def get_prompt(task: Task) -> str:
    if task not in get_args(Task):
        raise ValueError(f"Unknown tender model task: {task}")
    return files(__package__).joinpath(f"{task}.md").read_text(encoding="utf-8")


__all__ = ["get_prompt", "PROMPT_VERSION"]
