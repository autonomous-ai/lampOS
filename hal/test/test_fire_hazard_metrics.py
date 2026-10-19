"""Fire-hazard telemetry: hourly aggregate row and one row per sent alert."""

import pytest

from hal.telemetry import client
from hal.telemetry import fire_hazard_metrics as fm
from hal.telemetry.fire_hazard_metrics import FireHazardStats


@pytest.fixture
def reports(monkeypatch):
    rows: list[tuple[str, dict]] = []
    monkeypatch.setattr(client, "report", lambda name, params, event_id="": rows.append((name, params)))
    return rows


def test_hourly_row_with_exact_counts(reports):
    s = FireHazardStats(start=0.0)
    for outcome, ms in [("ok", 300.0), ("ok", 500.0), ("ok", 400.0), ("timeout", 15000.0),
                        ("http_error", 120.0), ("rate_limited", 80.0), ("encryption_unavailable", None),
                        ("error", None), ("backoff", None)]:
        s.record_owlv2(outcome, ms)
    s.record_owlv2("preset_fallback", None)
    for verdict, ms in [("miss", 150.0), ("miss", 170.0), ("hit", 190.0), ("unavailable", None)]:
        s.record_gate(verdict, ms)
    s.record_escalation()

    s.maybe_report(3599.0, gate_state="ready")
    assert reports == []
    s.maybe_report(3600.0, gate_state="ready")
    assert len(reports) == 1
    name, p = reports[0]
    assert name == "fire_hazard_stats"
    assert p["window_s"] == 3600.0
    assert p["observation_complete"] is True
    assert p["owlv2_calls"] == 8  # backoff and preset_fallback are not separate calls
    assert (p["owlv2_ok"], p["owlv2_timeout"], p["owlv2_error"], p["owlv2_http_error"]) == (3, 1, 1, 1)
    assert (p["owlv2_rate_limited"], p["owlv2_encryption_unavailable"], p["owlv2_preset_fallback"]) == (1, 1, 1)
    assert p["owlv2_backoff_skipped"] == 1
    assert p["owlv2_ms_p50"] == 400.0  # successful calls only
    assert p["owlv2_ms_max"] == 500.0
    assert (p["gate_runs"], p["gate_hits"], p["gate_unavailable"]) == (4, 1, 1)
    assert p["gate_ms_p50"] == 170.0
    assert p["escalations"] == 1
    assert p["gate_state"] == "ready"

    # The window resets after a report.
    s.maybe_report(7200.0, gate_state="ready")
    assert reports[1][1]["owlv2_calls"] == 0 and reports[1][1]["gate_runs"] == 0


def test_final_row_is_incomplete(reports):
    s = FireHazardStats(start=100.0)
    s.record_gate("miss", 160.0)
    s.maybe_report(400.0, final=True, gate_state="loading")
    (name, p), = reports
    assert name == "fire_hazard_stats"
    assert p["observation_complete"] is False
    assert p["window_s"] == 300.0
    assert p["gate_state"] == "loading"


def test_latency_null_without_samples(reports):
    s = FireHazardStats(start=0.0)
    s.record_owlv2("timeout", 15000.0)
    s.maybe_report(3600.0)
    p = reports[0][1]
    assert p["owlv2_ms_p50"] is None and p["owlv2_ms_max"] is None and p["gate_ms_p50"] is None
    assert p["gate_state"] == "unknown"


def test_samples_are_bounded(reports, monkeypatch):
    monkeypatch.setattr(fm, "_SAMPLE_CAP", 3)
    s = FireHazardStats(start=0.0)
    for ms in (1.0, 2.0, 3.0, 4.0, 5.0):
        s.record_owlv2("ok", ms)
    s.maybe_report(3600.0)
    p = reports[0][1]
    assert p["owlv2_ok"] == 5
    assert p["samples_dropped"] == 2
    assert p["owlv2_ms_max"] == 3.0


def test_record_alert_row(reports):
    s = FireHazardStats(start=0.0)
    s.record_alert(["smoke", "hazard_fire"], 12000, "ready")
    assert reports == [("fire_hazard_alert", {
        "types": ["hazard_fire", "smoke"], "first_seen_age_ms": 12000, "gate_state": "ready",
    })]


def test_tracker_exception_does_not_reach_check(monkeypatch):
    def boom(*_a, **_k):
        raise RuntimeError("telemetry down")

    monkeypatch.setattr(client, "report", boom)
    s = FireHazardStats(start=0.0)
    s.record_alert(["smoke"], 1, "ready")
    s.maybe_report(3600.0)
    s.maybe_report(1.0, final=True)


def test_perception_records_gate_owlv2_and_alert(reports):
    """Wiring: a gate hit, three OWLv2 sightings, one alert row, then the hourly row."""
    from test.test_fire_hazard import FIRE_HZ, STEP, T0, _check, _FakeDetector, _FakeGate, _perception
    from hal.drivers.sensing.perceptions.processors.fire_gate import GateVerdict

    det = _FakeDetector(FIRE_HZ)
    det.last_outcome = "ok"
    det.last_preset_fallback = False
    p, clock, events = _perception(_FakeGate(GateVerdict.HIT, GateVerdict.MISS), det)
    for i in range(3):
        if i:
            clock.t += STEP
        _check(p)
    alerts = [r for r in reports if r[0] == "fire_hazard_alert"]
    assert alerts == [("fire_hazard_alert", {
        "types": ["unsure_fire"], "first_seen_age_ms": 12000, "gate_state": "ready",
    })]
    assert len(events.sent) == 1

    clock.t = T0 + 3600.0
    _check(p)
    stats = [r[1] for r in reports if r[0] == "fire_hazard_stats"]
    assert len(stats) == 1
    assert stats[0]["gate_runs"] == 1 and stats[0]["gate_hits"] == 1 and stats[0]["escalations"] == 1
    assert stats[0]["owlv2_calls"] == 3 and stats[0]["owlv2_ok"] == 3
    assert stats[0]["owlv2_ms_p50"] is not None


def test_perception_survives_raising_tracker(monkeypatch):
    from test.test_fire_hazard import FIRE_HZ, _FakeDetector, _FakeGate, _perception, _run_checks
    from hal.drivers.sensing.perceptions.processors.fire_gate import GateVerdict

    def boom(*_a, **_k):
        raise RuntimeError("telemetry down")

    monkeypatch.setattr(client, "report", boom)
    p, clock, events = _perception(_FakeGate(GateVerdict.UNAVAILABLE), _FakeDetector(FIRE_HZ))
    _run_checks(p, clock, 3)
    assert len(events.sent) == 1
    p.cleanup()



def test_detector_reports_outcome(monkeypatch):
    import requests

    from hal import config
    from test.test_fire_hazard import FRAME, _detector, _FakePost, _Resp
    from hal.drivers.sensing.crypto import DLCrypto
    from hal.drivers.sensing.perceptions.processors import fire_hazard

    monkeypatch.setattr(DLCrypto, "http_session", lambda self: None)
    monkeypatch.setattr(config, "FIRE_HAZARD_PRESET", "fire_hazard_v1")
    cases = [
        ([_Resp(200)], "ok", False),
        ([_Resp(400), _Resp(200)], "ok", True),
        ([requests.Timeout("slow")], "timeout", False),
        ([requests.ConnectionError("down")], "error", False),
        ([_Resp(503)], "busy", False),
        ([_Resp(500)], "http_error", False),
        ([_Resp(429)], "rate_limited", False),
    ]
    for responses, outcome, fallback in cases:
        fake = _FakePost(*responses)
        monkeypatch.setattr(fire_hazard.requests.Session, "post", lambda self, *a, fake=fake, **kw: fake(*a, **kw))
        det = _detector()
        det.detect(FRAME)
        assert (det.last_outcome, det.last_preset_fallback) == (outcome, fallback), responses
    det.detect(FRAME)  # inside the 429 window
    assert det.last_outcome == "backoff"


def test_busy_503_is_its_own_counted_call(reports):
    s = FireHazardStats(start=0.0)
    s.record_owlv2("busy", None)
    s.record_owlv2("ok", 200.0)
    s.maybe_report(3600.0, gate_state="ready")
    p = reports[0][1]
    assert p["owlv2_busy"] == 1 and p["owlv2_calls"] == 2
