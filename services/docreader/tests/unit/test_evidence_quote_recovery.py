"""Recover layout-only quote edits without widening evidence visibility."""

from copy import deepcopy

import pytest

from docreader.tender.evidence.view import resolve_evidence_quote, supplied_evidence_texts


def resolve(text, quote, visible=None, **extra):
    return resolve_evidence_quote("r1", quote, {"r1": {"text": text, **extra}},
                                  {"r1": [text] if visible is None else visible})


def test_exact_quote_keeps_original_protocol_and_precedes_recovery():
    assert resolve("甲\n乙，甲乙，甲乙", "甲乙") == {"quote": "甲乙", "start": 4, "end": 6}


@pytest.mark.parametrize("text,quote,expected", [
    ("前言：交付\n期限\n30天。", "交付期限30天", "交付\n期限\n30天"),
    ("前言：交付\n\n期限\n30天。", "交付\n期限30天", "交付\n\n期限\n30天"),
    ("人员😀\n证书３０天", "😀证书３０天", "😀\n证书３０天"),
    ("甲\n\n乙\n\n丙", "甲\n乙丙", "甲\n\n乙\n\n丙"),
    ("前\n甲\n乙\n后", "\n甲乙\n", "\n甲\n乙\n"),
])
def test_deleted_lfs_restore_the_precise_original_interval(text, quote, expected):
    start = text.index(expected)
    assert resolve(text, quote, start=90) == {"quote": expected, "start": start,
                                             "end": start + len(expected)}


@pytest.mark.parametrize("text,quote", [
    ("甲乙", "甲\n乙"),  # The model cannot insert a newline.
    ("甲\n乙丙", "甲乙\n丙"),  # Nor move one to another character boundary.
    ("甲\n乙", "甲\n\n乙"),
    ("甲\r乙", "甲乙"),
    ("甲\t乙", "甲乙"),
    ("甲 乙", "甲乙"),
    ("甲\n乙", "甲 乙"),
    ("３０\n天", "30天"),
    ("甲\n，乙", "甲,乙"),
    ("甲\n乙", "甲丙"),
    ("甲\n乙", " \n "),
    ("甲\n乙", ""),
])
def test_changes_beyond_deleted_source_lf_are_rejected(text, quote):
    assert resolve(text, quote) is None


def test_recovery_rejects_repeated_original_intervals():
    assert resolve("甲\n乙；甲\n\n乙", "甲乙") is None
    assert resolve("甲\n甲\n甲", "甲甲") is None


def test_duplicate_candidates_for_one_original_interval_are_not_ambiguous():
    assert resolve("名称 | 交付\n30天", "交付30天", ["名称 | 交付\n30天", "交付\n30天"]) == {
        "quote": "交付\n30天", "start": 5, "end": 11,
    }


def test_one_visible_cell_can_restore_its_own_deleted_lf():
    assert resolve("名称 | 交付\n30天", "交付30天", ["名称", "交付\n30天"]) == {
        "quote": "交付\n30天", "start": 5, "end": 11,
    }


def test_recovery_requires_the_whole_original_quote_in_one_visible_candidate():
    assert resolve("甲\n乙", "甲乙", ["甲\n", "乙"]) is None
    assert resolve("甲\n | 乙", "甲 | 乙", ["甲\n", "乙"]) is None
    assert resolve("甲\n乙", "甲乙", ["甲乙"]) is None


def test_hidden_original_text_and_unknown_range_cannot_be_recovered():
    assert resolve("前言：甲\n乙", "甲乙", ["前言："]) is None
    assert resolve_evidence_quote("missing", "甲乙", {}, {"missing": ["甲\n乙"]}) is None


def test_contiguous_text_alias_recovery_uses_the_actually_sent_union():
    original = {"range_id": "alias", "text": "交付\n期限30天", "start": 10}
    first = {"range_id": "f1", "text": "交付\n", "start": 10}
    second = {"range_id": "f2", "text": "期限30天", "start": 13}
    views = [{"body": [first, second], "supporting": [], "source_aliases": [{
        "range_id": "alias", "segments": [{"range_id": "f1", "start": 10, "end": 13},
                                            {"range_id": "f2", "start": 13, "end": 18}],
    }]}]
    sources = {"alias": original}
    sent = supplied_evidence_texts(views, sources)
    assert resolve_evidence_quote("alias", "交付期限30天", sources, sent) == {
        "quote": "交付\n期限30天", "start": 0, "end": 8,
    }
    views[0]["source_aliases"][0]["segments"].pop()
    assert resolve_evidence_quote("alias", "交付期限30天", sources,
                                  supplied_evidence_texts(views, sources)) is None


def test_cells_only_alias_cannot_recover_hidden_row_or_content_outside_alias():
    row = {"range_id": "row", "cells": [{"column": 0, "text": "甲\n"},
                                           {"column": 1, "text": "乙\n丙"}]}
    sources = {"alias": {"text": "甲\n | 乙\n"}}
    views = [{"body": [row], "supporting": [], "source_aliases": [{"range_id": "alias",
        "segments": [{"range_id": "row", "start": 0, "end": 7}]}]}]
    sent = supplied_evidence_texts(views, sources)
    assert resolve_evidence_quote("alias", "甲 | 乙", sources, sent) is None
    assert resolve_evidence_quote("alias", "乙丙", sources, sent) is None


def test_resolution_does_not_mutate_frozen_sources_or_sent_candidates():
    sources = {"r1": {"text": "交付\n30天", "start": 90}}
    sent = {"r1": [sources["r1"]["text"]]}
    before = deepcopy((sources, sent))
    assert resolve_evidence_quote("r1", "交付30天", sources, sent)
    assert (sources, sent) == before
