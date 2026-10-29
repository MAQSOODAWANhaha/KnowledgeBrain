"""Shared outline budget helper for document execution tests."""

from docreader.llm.budget import RequestBudget


def budget():
    return RequestBudget(context_tokens=524288, model="synthetic-outline", prompts={}, output_reserve=32768)
