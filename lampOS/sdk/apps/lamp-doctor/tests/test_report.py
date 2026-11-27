"""The report is the agreed schema (Ten and Backend, 26 Nov 16:06) and holds nothing a person said."""

import pytest

from fake_ops import BadReport, FAILURES, REPORT_FIELDS, strict_check, turn
from lamp_doctor.report import build_report
from lamp_doctor.score import score_day

THREE = [turn(turn=n) for n in range(3)]


def failed(**over):
    call = {"kind": "llm", "outcome": "timeout", "fault": "response_timeout", "waited_ms": 4500, "request": 41}
    call.update(over)
    return call


def test_the_report_is_exactly_the_agreed_fields_and_passes_the_strict_check():
    report = build_report(score_day(THREE), diagnosis=[])
    assert set(report) == REPORT_FIELDS and "build" not in report and "day" not in report
    strict_check(report)
    assert report["tier"] == 1 and report["route"] == "first" and report["diagnosis_version"] == 1
    assert isinstance(report["answered_within_5s_pct"], int) and report["band"] == "green"


def test_under_three_questions_there_is_no_report():
    assert build_report(score_day([turn()]), diagnosis=[]) is None


def test_failures_are_the_v6_kinds_counted_once_each():
    day = score_day(THREE + [turn(turn=9, outcome="stuck"), turn(turn=10, outcome="no_answer", heard_to_first_sound_ms=None),
                             turn(turn=11, outcome="error", heard_to_first_sound_ms=None), turn(turn=12, outcome="lost_answer"),
                             turn(turn=13, outcome="asked_again"), turn(turn=14, heard_to_first_sound_ms=9000)])
    report = build_report(day, diagnosis=[])
    counts = {f["category"]: f["count"] for f in report["failures"]}
    assert counts == {"stuck": 1, "lost_answer": 1, "error": 1, "no_answer": 1, "misheard": 1, "very_slow": 1}
    strict_check(report)


def test_a_diagnosis_adds_only_listed_categories_with_count_one():
    report = build_report(score_day(THREE), diagnosis=["misheard", "the user said turn on the lamp", "Anna", "issue"])
    assert {f["category"] for f in report["failures"]} == {"misheard", "issue"}
    assert "Anna" not in repr(report) and "lamp" not in repr(report)
    strict_check(report)


def test_cases_come_from_the_failed_calls_with_listed_words_only():
    bad = turn(turn=7, at_min=29_000_100, failed=[failed(), failed(kind="stt", outcome="error", http=503, fault=None, request=42),
                                                    failed(kind="the user said hello", request=43),
                                                    failed(fault="Anna said so", request=44)])
    report = build_report(score_day(THREE + [bad]), diagnosis=[])
    assert [(c["kind"], c["outcome"], c["request"]) for c in report["cases"]] == [("llm", "timeout", 1), ("stt", "error", 2), ("llm", "timeout", 3)]  # numbered 1..N, not the voice's ids
    assert report["cases"][1]["http"] == 503 and "fault" not in report["cases"][1] and "fault" not in report["cases"][2]
    assert len(report["cases"][0]["at"]) == 5 and report["cases_dropped"] == 0
    assert "Anna" not in repr(report) and "hello" not in repr(report)
    strict_check(report)


def test_past_50_cases_only_the_count_goes_on():
    many = turn(turn=7, failed=[failed(request=n) for n in range(53)])
    report = build_report(score_day(THREE + [many]), diagnosis=[])
    assert len(report["cases"]) == 50 and report["cases_dropped"] == 3
    strict_check(report)


def test_the_route_is_first_second_or_unknown():
    for route, want in (("first", "first"), ("second", "second"), ("live", "unknown")):
        assert build_report(score_day([turn(route=route, turn=n) for n in range(3)]), diagnosis=[])["route"] == want


def test_the_strict_check_refuses_what_the_app_must_never_send():
    good = build_report(score_day(THREE), diagnosis=[])
    for broken in (
        {**good, "note": "she said hello"},
        {**good, "failures": [{"category": "Anna asked for the light", "count": 1}]},
        {**good, "failures": [{"category": "stuck", "count": 1}, {"category": "stuck", "count": 2}]},
        {**good, "route": "turn the light on"},
        {**good, "tier": 5},
        {**good, "score": 9.55},
        {**good, "band": "red"},
        {**good, "cases": [{"at": "9:5", "kind": "llm", "outcome": "error", "waited_ms": 1, "request": 1}]},
        {**good, "cases": [{"at": "09:05", "kind": "llm", "outcome": "error", "waited_ms": 1, "request": 1, "text": "hi"}]},
    ):
        with pytest.raises(BadReport):
            strict_check(broken)


def test_every_number_stays_inside_the_bounds_the_lamp_checks():
    huge = [turn(turn=n, heard_to_first_sound_ms=5_000_000) for n in range(3)]
    odd = turn(turn=9, failed=[failed(waited_ms=9_999_999, request=7), failed(request=10**12), failed(request=-4)])
    report = build_report(score_day(huge + [odd]), diagnosis=[])
    assert report["heard_to_first_sound_ms_p90"] <= 600_000
    # A request id past 100,000 (the voice's counter, in about 11 days) or below 0 never costs a case: numbered 1..N.
    assert [(c["request"], c["waited_ms"]) for c in report["cases"]] == [(1, 120_000), (2, 4500), (3, 4500)]
    assert report["cases_dropped"] == 0
    strict_check(report)


def test_cases_dropped_is_at_most_the_turns():
    many = turn(turn=7, failed=[failed(request=n) for n in range(60)])
    report = build_report(score_day([turn(turn=1), turn(turn=2), many]), diagnosis=[])
    assert len(report["cases"]) == 50 and report["cases_dropped"] == 3 == report["turns"]
    strict_check(report)
