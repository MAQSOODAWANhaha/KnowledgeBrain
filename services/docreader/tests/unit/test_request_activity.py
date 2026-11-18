"""Only bounded, numeric timing observations reach request metadata."""

import pytest

from docreader.integrations.store import _recorded_first_data


TIMINGS = {
    "first_data_seconds": 1.2,
    "first_raw_seconds": 0.8,
    "first_content_seconds": 2.4,
    "max_raw_idle_seconds": 3,
    "semaphore_wait_seconds": 0,
}


def test_request_metadata_preserves_distinct_timing_observations_only():
    activity = {**TIMINGS, "raw_body": "private source", "api_key": "private key",
                "http_headers": {"Authorization": "private"}, "received_bytes": 42}
    assert _recorded_first_data(activity) == TIMINGS


@pytest.mark.parametrize("invalid", [None, True, False, "1.2", -1, float("nan"),
                                     float("inf"), -float("inf")])
@pytest.mark.parametrize("field", TIMINGS)
def test_invalid_timing_does_not_hide_other_valid_observations(field, invalid):
    activity = {**TIMINGS, field: invalid}
    assert _recorded_first_data(activity) == {
        key: value for key, value in TIMINGS.items() if key != field
    }


@pytest.mark.parametrize("activity", [None, {}, {"raw_body": "private"},
                                      {"first_data_seconds": float("nan")}])
def test_missing_or_invalid_timings_do_not_invent_measurements(activity):
    assert _recorded_first_data(activity) is None
