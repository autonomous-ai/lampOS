"""Fire-hazard detection: request shape, gate/escalation, confirm and alert logic."""

import json
import logging

import numpy as np
import pytest

from hal import config
from hal.drivers.sensing.crypto import DLCrypto
from hal.drivers.sensing.perceptions.processors import fire_hazard
from hal.drivers.sensing.perceptions.processors.fire_hazard import RemoteFireHazardDetector

FRAME = np.zeros((64, 64, 3), dtype=np.uint8)
FIRE = "a single flame"
SMOKE = "a single cloud of smoke"
FLAMMABLE = "a single piece of wooden furniture"
SAFE = "a single fireplace"


class _Resp:
    def __init__(self, status: int, body=None, headers=None):
        self.status_code = status
        self._body = body if body is not None else {"detections": []}
        self.text = json.dumps(self._body)
        self.content = self.text.encode()
        self.headers = headers or {}

    def json(self):
        return self._body


class _FakePost:
    """Scripted stand-in for Session.post; records each posted JSON body and session."""

    def __init__(self, *responses):
        self.responses = list(responses)
        self.bodies: list[dict] = []
        self.sessions: list[int] = []

    def __call__(self, url, data=None, headers=None, timeout=None):
        self.bodies.append(json.loads(data))
        item = self.responses.pop(0) if len(self.responses) > 1 else self.responses[0]
        if isinstance(item, BaseException):
            raise item
        return item


def _det(name: str, xywh=(0.5, 0.5, 0.2, 0.2), conf=0.9) -> dict:
    return {"class_name": name, "confidence": conf, "xywh": list(xywh)}


@pytest.fixture
def plaintext(monkeypatch):
    monkeypatch.setattr(DLCrypto, "http_session", lambda self: None)


@pytest.fixture
def post(monkeypatch, plaintext):
    fake = _FakePost(_Resp(200))

    def session_post(self, *args, **kwargs):
        fake.sessions.append(id(self))
        return fake(*args, **kwargs)

    monkeypatch.setattr(fire_hazard.requests.Session, "post", session_post)
    return fake


def _detector() -> RemoteFireHazardDetector:
    return RemoteFireHazardDetector(base_url="https://dl.example/detect/owlv2", api_key="k")


# --- request shape (Task 3.2) -------------------------------------------------

def test_sends_preset_not_classes(post, monkeypatch):
    monkeypatch.setattr(config, "FIRE_HAZARD_PRESET", "fire_hazard_v1")
    _detector().detect(FRAME)
    assert len(post.bodies) == 1
    assert post.bodies[0]["preset"] == "fire_hazard_v1"
    assert "classes" not in post.bodies[0]
    assert post.bodies[0]["image_b64"]


def test_400_falls_back_to_classes_same_call(post, monkeypatch):
    monkeypatch.setattr(config, "FIRE_HAZARD_PRESET", "fire_hazard_v1")
    post.responses = [_Resp(400, {"detail": "unknown preset"}), _Resp(200, {"detections": [_det(FIRE)]})]
    hazards = _detector().detect(FRAME)
    assert len(post.bodies) == 2
    assert "preset" in post.bodies[0]
    assert "preset" not in post.bodies[1]
    assert len(post.bodies[1]["classes"]) == 46
    assert post.bodies[1]["image_b64"] == post.bodies[0]["image_b64"]
    assert [h.type.value for h in hazards] == ["unsure_fire"]


def test_empty_preset_config_sends_classes(post, monkeypatch):
    monkeypatch.setattr(config, "FIRE_HAZARD_PRESET", "")
    _detector().detect(FRAME)
    assert len(post.bodies) == 1
    assert "preset" not in post.bodies[0]
    assert len(post.bodies[0]["classes"]) == 46


def test_unknown_classes_warning_logged_once(post, monkeypatch, caplog):
    monkeypatch.setattr(config, "FIRE_HAZARD_PRESET", "fire_hazard_v1")
    monkeypatch.setattr(fire_hazard, "_warned_unknown_preset_classes", False)
    post.responses = [_Resp(200, {"detections": [_det("person"), _det("chair")]})]
    det = _detector()
    with caplog.at_level(logging.WARNING):
        det.detect(FRAME)
        det.detect(FRAME)
    hits = [r for r in caplog.records if "preset response has no known classes" in r.getMessage()]
    assert len(hits) == 1


def test_known_classes_do_not_warn(post, monkeypatch, caplog):
    monkeypatch.setattr(config, "FIRE_HAZARD_PRESET", "fire_hazard_v1")
    monkeypatch.setattr(fire_hazard, "_warned_unknown_preset_classes", False)
    post.responses = [_Resp(200, {"detections": [_det(FIRE), _det("person")]})]
    with caplog.at_level(logging.WARNING):
        _detector().detect(FRAME)
    assert not [r for r in caplog.records if "no known classes" in r.getMessage()]


# --- gate + escalation (Task 3.3) ----------------------------------------------

from hal.drivers.sensing.perceptions.processors.fire_gate import GateResult, GateVerdict  # noqa: E402
from hal.drivers.sensing.perceptions.processors.fire_hazard import FireHazardPerception  # noqa: E402
from hal.drivers.sensing.perceptions.processors.models import FireHazard, FireHazardEnum  # noqa: E402

T0 = 1000.0
STEP = 6.0  # effective check cadence: 2 s sensing tick + 5 s interval gate (D6)


def _hz(kind: FireHazardEnum) -> FireHazard:
    return FireHazard(type=kind, bbox=np.array([10, 10, 30, 30], dtype=np.float32))


FIRE_HZ = [_hz(FireHazardEnum.UNSURE_FIRE)]


class _Clock:
    def __init__(self, t: float = T0):
        self.t = t

    def __call__(self) -> float:
        return self.t


class _FakeGate:
    def __init__(self, *verdicts: GateVerdict):
        self.verdicts = list(verdicts) or [GateVerdict.MISS]
        self.runs: list[object] = []
        self.closed = False

    @property
    def state(self) -> str:
        return "ready"

    def run(self, frame) -> GateResult:
        self.runs.append(frame)
        v = self.verdicts.pop(0) if len(self.verdicts) > 1 else self.verdicts[0]
        return GateResult(v, 0.5 if v is GateVerdict.HIT else 0.0, "fire" if v is GateVerdict.HIT else None, 1.0)

    def close(self) -> None:
        self.closed = True


class _FakeDetector:
    def __init__(self, *results):
        self.results = list(results) or [[]]
        self.frames: list[object] = []

    def detect(self, frame):
        self.frames.append(frame)
        return self.results.pop(0) if len(self.results) > 1 else self.results[0]


class _Events:
    def __init__(self, clock: _Clock):
        self.clock = clock
        self.sent: list[tuple[float, str]] = []

    def __call__(self, event, message, kind, snapshots, extra):
        self.sent.append((self.clock.t, message))


def _perception(gate, detector, clock=None):
    clock = clock or _Clock()
    events = _Events(clock)
    p = FireHazardPerception(None, events, gate=gate, detector=detector, clock=clock)
    return p, clock, events


def _check(p, frame=FRAME):
    """One check on the caller's thread, then wait for its OWLv2 round (if any)."""
    p._check_impl(frame)
    fut = p._owlv2_future
    if fut is not None:
        fut.result(timeout=5)


def _run_checks(p, clock, n, frame=FRAME):
    """n due checks, STEP seconds apart, starting at the clock's current time."""
    for i in range(n):
        if i:
            clock.t += STEP
        _check(p, frame)


def test_gate_miss_skips_owlv2():
    gate, det = _FakeGate(GateVerdict.MISS), _FakeDetector()
    p, clock, _ = _perception(gate, det)
    _run_checks(p, clock, 5)
    assert det.frames == []
    assert len(gate.runs) == 5


def test_gate_hit_calls_owlv2_same_frame():
    gate, det = _FakeGate(GateVerdict.HIT), _FakeDetector([])
    p, clock, _ = _perception(gate, det)
    frame = np.ones((32, 32, 3), dtype=np.uint8)
    _check(p, frame)
    assert det.frames == [frame] and det.frames[0] is frame


def test_escalation_holds_while_owlv2_sees_fire():
    gate, det = _FakeGate(GateVerdict.HIT, GateVerdict.MISS), _FakeDetector(FIRE_HZ)
    p, clock, _ = _perception(gate, det)
    _run_checks(p, clock, 5)
    assert len(det.frames) == 5
    assert len(gate.runs) == 1


def test_escalation_ends_10s_after_last_sighting():
    # Fire seen at t=0 only: escalated until t=10, so t=6 still calls OWLv2 and
    # t=12 is back to the gate (MISS, no OWLv2 call).
    gate = _FakeGate(GateVerdict.HIT, GateVerdict.MISS)
    det = _FakeDetector(FIRE_HZ, [], [])
    p, clock, _ = _perception(gate, det)
    _run_checks(p, clock, 4)  # t = 0, 6, 12, 18
    assert len(det.frames) == 2
    assert len(gate.runs) == 3


def test_escalation_ends_10s_after_start_without_sighting():
    gate = _FakeGate(GateVerdict.HIT, GateVerdict.MISS)
    det = _FakeDetector([])
    p, clock, _ = _perception(gate, det)
    _run_checks(p, clock, 3)  # t = 0 (hit), 6 (escalated), 12 (gate miss)
    assert len(det.frames) == 2
    assert len(gate.runs) == 2


def test_gate_unavailable_is_fail_open():
    gate, det = _FakeGate(GateVerdict.UNAVAILABLE), _FakeDetector([])
    p, clock, _ = _perception(gate, det)
    _run_checks(p, clock, 5)
    assert len(det.frames) == 5


def test_need_owlv2_reasons():
    p, clock, _ = _perception(_FakeGate(GateVerdict.MISS, GateVerdict.UNAVAILABLE, GateVerdict.HIT), _FakeDetector())
    assert p._need_owlv2(T0, FRAME) == (False, "gate_miss")
    assert p._need_owlv2(T0, FRAME) == (True, "gate_unavailable")
    assert p._need_owlv2(T0, FRAME) == (True, "gate_hit")
    assert p._need_owlv2(T0 + 9.9, FRAME) == (True, "escalated")


def test_timeline_alert_latency_matches_today():
    sent_at = {}
    for verdict in (GateVerdict.HIT, GateVerdict.UNAVAILABLE):
        p, clock, events = _perception(_FakeGate(verdict, GateVerdict.MISS), _FakeDetector(FIRE_HZ))
        _run_checks(p, clock, 3)  # fire at 0 / 6 / 12
        assert len(events.sent) == 1
        sent_at[verdict] = events.sent[0][0] - T0
    assert sent_at[GateVerdict.HIT] == sent_at[GateVerdict.UNAVAILABLE] == 12.0


def test_to_dict_and_cleanup_report_gate():
    gate = _FakeGate(GateVerdict.HIT)
    p, clock, _ = _perception(gate, _FakeDetector([]))
    _check(p, FRAME)
    d = p.to_dict()
    assert d["gate_state"] == "ready" and d["escalated"] is True
    p.cleanup()
    assert gate.closed


# --- error = no observation; 429 backoff (Task 4.1) ----------------------------

import requests  # noqa: E402

from hal.drivers.sensing.crypto import EncryptionUnavailable  # noqa: E402


def test_timeout_returns_none(post):
    post.responses = [requests.Timeout("slow")]
    assert _detector().detect(FRAME) is None


def test_connection_error_returns_none(post):
    post.responses = [requests.ConnectionError("down")]
    assert _detector().detect(FRAME) is None


def test_http_500_returns_none(post):
    post.responses = [_Resp(500, {"detail": "boom"})]
    assert _detector().detect(FRAME) is None


def test_bad_json_returns_none(post):
    post.responses = [_Resp(200, "not-a-dict-or-list")]
    assert _detector().detect(FRAME) is None


def test_no_url_returns_none(post):
    assert RemoteFireHazardDetector(base_url="", api_key="k").detect(FRAME) is None
    assert post.bodies == []


def test_encryption_unavailable_returns_none(post, monkeypatch):
    def unavailable(self):
        raise EncryptionUnavailable("no key")

    monkeypatch.setattr(DLCrypto, "http_session", unavailable)
    assert _detector().detect(FRAME) is None
    assert post.bodies == []


def test_ok_empty_returns_empty_list(post):
    post.responses = [_Resp(200, {"detections": []})]
    assert _detector().detect(FRAME) == []


def test_429_backoff_is_short_and_jittered(post, monkeypatch):
    now = [5000.0]
    monkeypatch.setattr(fire_hazard.time, "time", lambda: now[0])
    det = _detector()

    def delay_after(resp) -> float:
        post.responses = [resp]
        det.detect(FRAME)
        d = det._backoff_until - now[0]
        now[0] = det._backoff_until + 0.001  # step past the window
        return d

    assert 4.0 <= delay_after(_Resp(429)) <= 6.0
    assert 8.0 <= delay_after(_Resp(429)) <= 12.0
    for _ in range(6):
        assert delay_after(_Resp(429)) <= 72.0
    assert delay_after(_Resp(429)) >= 48.0  # capped at 60 s, jittered ±20 %
    delay_after(_Resp(200))
    assert 4.0 <= delay_after(_Resp(429)) <= 6.0


def test_inside_backoff_returns_none_without_posting(post, monkeypatch):
    now = [5000.0]
    monkeypatch.setattr(fire_hazard.time, "time", lambda: now[0])
    det = _detector()
    post.responses = [_Resp(429)]
    assert det.detect(FRAME) is None
    n = len(post.bodies)
    now[0] += 1.0
    assert det.detect(FRAME) is None
    assert len(post.bodies) == n


# --- upload size and connection reuse (#612) ------------------------------------

from hal.drivers.sensing.perceptions.processors.utils import img2b64  # noqa: E402


def test_img2b64_quality_shrinks_the_jpeg():
    frame = np.random.default_rng(0).integers(0, 255, (120, 160, 3), dtype=np.uint8)
    assert len(img2b64(frame, 80)) < len(img2b64(frame))


def test_upload_uses_configured_jpeg_quality(post, monkeypatch):
    seen: list[int | None] = []
    monkeypatch.setattr(
        fire_hazard, "img2b64", lambda frame, quality=None: seen.append(quality) or "eA=="
    )
    monkeypatch.setattr(config, "FIRE_HAZARD_JPEG_QUALITY", 80)
    _detector().detect(FRAME)
    assert seen == [80]


def test_consecutive_calls_reuse_one_session(post):
    det = _detector()
    det.detect(FRAME)
    det.detect(FRAME)
    assert len(post.sessions) == 2 and len(set(post.sessions)) == 1


def test_dropped_connection_is_retried_once(post):
    post.responses = [requests.ConnectionError("reset by peer"), _Resp(200, {"detections": []})]
    assert _detector().detect(FRAME) == []
    assert len(post.bodies) == 2 and post.bodies[0] == post.bodies[1]


def test_second_dropped_connection_is_no_observation(post):
    post.responses = [requests.ConnectionError("reset"), requests.ConnectionError("reset"), _Resp(200)]
    assert _detector().detect(FRAME) is None
    assert len(post.bodies) == 2


def test_connect_timeout_is_not_retried(post):
    post.responses = [requests.ConnectTimeout("slow"), _Resp(200)]
    assert _detector().detect(FRAME) is None
    assert len(post.bodies) == 1


def test_cleanup_closes_the_detector_session(plaintext, monkeypatch):
    det = _detector()
    closed: list[bool] = []
    monkeypatch.setattr(det._http, "close", lambda: closed.append(True))
    p, _, _ = _perception(_FakeGate(), det)
    p.cleanup()
    assert closed == [True]


def test_failed_call_keeps_streak():
    det = _FakeDetector(FIRE_HZ, None, FIRE_HZ)
    p, clock, events = _perception(_FakeGate(GateVerdict.UNAVAILABLE), det)
    _run_checks(p, clock, 3)  # fire at 0, failed call at 6, fire at 12
    assert [t - T0 for t, _ in events.sent] == [12.0]


# --- flame/smoke streaks; classify at confirm time (Task 4.2) -------------------

def _obj(xywh) -> dict:
    return {"xywh": list(xywh)}


_BOX = (0.5, 0.5, 0.2, 0.2)


def _assess(fire=(), smoke=(), flammable=(), safe=()):
    det = _detector()
    hz = det._assess_hazard([_obj(b) for b in fire], [_obj(b) for b in smoke],
                            [_obj(b) for b in flammable], [_obj(b) for b in safe])
    return sorted(h.type.value for h in hz)


def test_assess_fire_without_context_is_unsure():
    assert _assess(fire=[_BOX]) == ["unsure_fire"]


def test_assess_fire_on_flammable_is_hazard():
    assert _assess(fire=[_BOX], flammable=[(0.5, 0.6, 0.4, 0.4)]) == ["hazard_fire"]


def test_assess_fire_more_on_safe_is_safe():
    assert _assess(fire=[_BOX], flammable=[(0.65, 0.5, 0.2, 0.2)], safe=[_BOX]) == ["safe_fire"]


def test_assess_equal_overlap_is_hazard():
    assert _assess(fire=[_BOX], flammable=[_BOX], safe=[_BOX]) == ["hazard_fire"]


def test_assess_smoke_only():
    assert _assess(smoke=[_BOX]) == ["smoke"]


def test_assess_zero_area_fire_dropped():
    assert _assess(fire=[(0.5, 0.5, 0.0, 0.0)]) == []


def test_assess_fire_and_smoke():
    assert _assess(fire=[_BOX], smoke=[_BOX]) == ["smoke", "unsure_fire"]


# --- flame words count as fire, not as a safe container (#717) -------------------

BONFIRE = "a single bonfire"
CAMPFIRE = "a single campfire"
TORCH = "a single torch"
COUCH = "a single couch"
FIRE_BOWL = "a single fire bowl"


def _detect_types(post, dets) -> list[str]:
    post.responses = [_Resp(200, {"detections": dets})]
    return sorted(h.type.value for h in _detector().detect(FRAME))


def test_flame_words_are_fire_classes():
    det = _detector()
    for name in (BONFIRE, CAMPFIRE, TORCH):
        assert name in det._fire_set
        assert name not in det._safe_set


@pytest.mark.parametrize("name", [BONFIRE, CAMPFIRE, TORCH])
def test_flame_word_alone_is_unsure_fire(post, name):
    assert _detect_types(post, [_det(name)]) == ["unsure_fire"]


def test_bonfire_on_couch_is_hazard(post):
    # fire-3.jpg (sofa on fire): OWLv2 boxes, 640x480.
    dets = [_det(BONFIRE, (413.1, 207.8, 146.9, 271.3), 0.67), _det(COUCH, (320.0, 312.8, 382.5, 285.6), 0.68)]
    assert _detect_types(post, dets) == ["hazard_fire"]


def test_torch_on_couch_is_hazard(post):
    dets = [_det(TORCH, (300, 300, 60, 120)), _det(COUCH, (320, 320, 380, 280))]
    assert _detect_types(post, dets) == ["hazard_fire"]


def test_bonfire_in_fire_bowl_is_safe(post):
    # fire-2.jpg (garden fire bowl): bonfire above the rim, bowl, wooden furniture to the left.
    dets = [
        _det(BONFIRE, (303.4, 265.3, 124.4, 73.4), 0.61),
        _det(FIRE_BOWL, (308.1, 311.9, 297.5, 78.1), 0.87),
        _det(FLAMMABLE, (132.5, 280.2, 263.1, 61.3), 0.43),
    ]
    assert _detect_types(post, dets) == ["safe_fire"]


def test_flame_inside_bonfire_is_not_safe(post):
    # Dataset case: a flame box inside a bonfire box used to excuse itself as safe_fire.
    dets = [_det(FIRE, (300, 260, 60, 80), 0.64), _det(BONFIRE, (300, 250, 200, 160), 0.33)]
    assert "safe_fire" not in _detect_types(post, dets)


# --- containment: how much of the fire sits inside the container (#717) ----------

CANDLE = "a single candle"
BLANKET = "a single blanket"
FIREPLACE = "a single fireplace"


def test_assess_small_safe_box_inside_big_fire_does_not_excuse():
    # A candle box on one flame column of a big fire: shared ÷ candle = 1.0, shared ÷ fire ≈ 0.02.
    assert _assess(fire=[(0.5, 0.5, 0.4, 0.6)], safe=[(0.4, 0.7, 0.05, 0.1)]) == ["unsure_fire"]


def test_assess_fire_fully_inside_container_is_safe():
    assert _assess(fire=[(0.5, 0.4, 0.2, 0.2)], safe=[(0.5, 0.5, 0.4, 0.4)]) == ["safe_fire"]


def test_candles_inside_room_fire_do_not_excuse_it(post):
    # s072 (room fire next to a blanket) without the frame-sized fireplace box:
    # two candle boxes on the flame columns sit inside the bonfire box.
    dets = [
        _det(BONFIRE, (117.0, 236.0, 176.0, 364.0), 0.59),
        _det(CANDLE, (57.5, 372.0, 47.0, 110.0), 0.39),
        _det(CANDLE, (101.5, 382.0, 47.0, 120.0), 0.38),
        _det(BLANKET, (474.0, 340.0, 314.0, 278.0), 0.52),
    ]
    assert _detect_types(post, dets) == ["unsure_fire"]


def test_frame_sized_fireplace_holding_couch_is_ignored(post):
    # s052 (room flashover): OWLv2 calls the whole doorway view a fireplace; it holds a couch.
    dets = [
        _det(BONFIRE, (295.0, 267.0, 422.0, 232.0), 0.34),
        _det(FIREPLACE, (315.0, 235.5, 624.0, 469.0), 0.51),
        _det(COUCH, (150.5, 366.5, 131.0, 165.0), 0.55),
    ]
    assert _detect_types(post, dets) == ["hazard_fire"]


def test_frame_sized_fireplace_and_candles_room_fire(post):
    # s072 full detections: frame-sized fireplace holding a blanket, plus candle boxes on the flames.
    dets = [
        _det(BONFIRE, (117.0, 236.0, 176.0, 364.0), 0.59),
        _det(FIREPLACE, (316.5, 239.0, 629.0, 472.0), 0.44),
        _det(CANDLE, (57.5, 372.0, 47.0, 110.0), 0.39),
        _det(CANDLE, (101.5, 382.0, 47.0, 120.0), 0.38),
        _det(BLANKET, (474.0, 340.0, 314.0, 278.0), 0.52),
    ]
    assert _detect_types(post, dets) == ["unsure_fire"]


def test_assess_container_with_rug_partly_inside_stays_safe():
    # Real fireplace with a rug in front: ~17 % of the rug is inside the fireplace box.
    assert _assess(
        fire=[(0.5, 0.4, 0.2, 0.2)],
        safe=[(0.5, 0.45, 0.4, 0.4)],
        flammable=[(0.5, 0.7, 0.6, 0.2)],
    ) == ["safe_fire"]


def test_assess_small_cushion_on_hearth_keeps_fireplace():
    # A cushion fully inside the fireplace box but ~3 % of its area is an object on the hearth,
    # not a sign that the "fireplace" is a whole room.
    assert _assess(
        fire=[(0.5, 0.4, 0.2, 0.2)],
        safe=[(0.5, 0.45, 0.4, 0.4)],
        flammable=[(0.6, 0.6, 0.08, 0.06)],
    ) == ["safe_fire"]


def test_assess_zero_area_flammable_does_not_crash():
    assert _assess(
        fire=[(0.5, 0.4, 0.2, 0.2)],
        safe=[(0.5, 0.45, 0.4, 0.4)],
        flammable=[(0.5, 0.45, 0.0, 0.0)],
    ) == ["safe_fire"]


def _always_owlv2(*results):
    return _perception(_FakeGate(GateVerdict.UNAVAILABLE), _FakeDetector(*results))


def test_label_flip_does_not_reset_streak():
    p, clock, events = _always_owlv2(
        [_hz(FireHazardEnum.HAZARD_FIRE)], [_hz(FireHazardEnum.UNSURE_FIRE)], [_hz(FireHazardEnum.HAZARD_FIRE)],
    )
    _run_checks(p, clock, 3)
    assert [(t - T0, m) for t, m in events.sent] == [(12.0, "Fire hazard detected: hazard_fire")]


def test_confirm_type_comes_from_confirming_frame():
    p, clock, events = _always_owlv2(
        [_hz(FireHazardEnum.UNSURE_FIRE)], [_hz(FireHazardEnum.UNSURE_FIRE)], [_hz(FireHazardEnum.SAFE_FIRE)],
    )
    _run_checks(p, clock, 3)
    assert [m for _, m in events.sent] == ["Fire hazard detected: safe_fire"]


def test_absent_observation_breaks_streak():
    p, clock, events = _always_owlv2(FIRE_HZ, [], FIRE_HZ)
    _run_checks(p, clock, 3)
    assert events.sent == []


def test_stale_streak_restarts_after_gap():
    p, clock, events = _always_owlv2(FIRE_HZ)
    _check(p, FRAME)  # t=0
    for t in (40.0, 46.0):
        clock.t = T0 + t
        _check(p, FRAME)
    assert events.sent == []
    clock.t = T0 + 52.0
    _check(p, FRAME)
    assert [t - T0 for t, _ in events.sent] == [52.0]


def test_smoke_and_flame_streaks_independent():
    p, _, _ = _always_owlv2()
    smoke, fire = _hz(FireHazardEnum.SMOKE), _hz(FireHazardEnum.UNSURE_FIRE)
    assert p._update_streaks(T0, [smoke]) == set()
    assert p._update_streaks(T0 + 6, [smoke, fire]) == set()
    assert p._update_streaks(T0 + 12, [smoke, fire]) == {"smoke"}
    assert p._update_streaks(T0 + 18, [fire]) == {"flame"}
    assert p.to_dict()["streak_kinds"] == ["flame"]


# --- dedup: safe_fire 30 min, others re-alert every 60 s (Task 4.3) -------------

def _continuous(kind: FireHazardEnum, seconds: float):
    p, clock, events = _always_owlv2([_hz(kind)])
    _run_checks(p, clock, int(seconds // STEP) + 1)
    return [t - T0 for t, _ in events.sent], [m for _, m in events.sent]


def test_hazard_fire_realerts_every_60s():
    times, msgs = _continuous(FireHazardEnum.HAZARD_FIRE, 132)
    assert times == [12.0, 72.0, 132.0]
    assert all(b - a >= 60 for a, b in zip(times, times[1:]))
    assert set(msgs) == {"Fire hazard detected: hazard_fire"}


def test_unsure_fire_realerts_every_60s():
    times, _ = _continuous(FireHazardEnum.UNSURE_FIRE, 132)
    assert times == [12.0, 72.0, 132.0]


def test_smoke_realerts_every_60s():
    times, msgs = _continuous(FireHazardEnum.SMOKE, 132)
    assert times == [12.0, 72.0, 132.0]
    assert set(msgs) == {"Fire hazard detected: smoke"}


def test_safe_fire_dedup_30min():
    p, clock, events = _always_owlv2([_hz(FireHazardEnum.SAFE_FIRE)])
    _run_checks(p, clock, int(1800 // STEP) + 1)  # t = 0 .. 1800
    assert [t - T0 for t, _ in events.sent] == [12.0]
    for _ in range(3):
        clock.t += STEP
        _check(p, FRAME)
    assert len(events.sent) == 2 and events.sent[1][0] - events.sent[0][0] >= 1800


def test_new_type_alerts_immediately_within_window():
    fire, smoke = _hz(FireHazardEnum.HAZARD_FIRE), _hz(FireHazardEnum.SMOKE)
    p, clock, events = _always_owlv2([fire], [fire], [fire, smoke], [fire, smoke], [fire, smoke])
    _run_checks(p, clock, 5)  # smoke from t=12, confirmed at t=24
    assert [(t - T0, m) for t, m in events.sent] == [
        (12.0, "Fire hazard detected: hazard_fire"),
        (24.0, "Fire hazard detected: smoke"),
    ]


def test_dedup_window_per_type():
    p, _, _ = _always_owlv2()
    assert p._dedup_window(FireHazardEnum.SAFE_FIRE.value) == config.FIRE_HAZARD_DEDUP_WINDOW_S
    for t in (FireHazardEnum.HAZARD_FIRE, FireHazardEnum.UNSURE_FIRE, FireHazardEnum.SMOKE):
        assert p._dedup_window(t.value) == config.FIRE_HAZARD_REALERT_S


# --- OWLv2 off the shared camera pool (Task 4.4) ---------------------------------

import threading  # noqa: E402

from hal.drivers.sensing.perceptions.processors.base import Perception  # noqa: E402


class _BlockingDetector:
    def __init__(self, result=None):
        self.result = result if result is not None else []
        self.entered = threading.Event()
        self.release = threading.Event()
        self.calls = 0

    def detect(self, frame):
        self.calls += 1
        self.entered.set()
        self.release.wait(5)
        return self.result


class _BarrierPerception(Perception):
    def __init__(self, barrier):
        super().__init__(None, lambda *a: None)
        self.barrier = barrier
        self.ran = threading.Event()

    def _check_impl(self, data):
        self.barrier.wait(timeout=0.5)
        self.ran.set()

    def cleanup(self):
        pass


def test_slow_owlv2_does_not_hold_shared_pool():
    det = _BlockingDetector()
    p, clock, _ = _perception(_FakeGate(GateVerdict.UNAVAILABLE), det)
    try:
        p.check(FRAME)  # real shared-pool path
        assert det.entered.wait(2), "OWLv2 call never started"
        # Both pool workers must be free: two checks that only finish together.
        barrier = threading.Barrier(2)
        a, b = _BarrierPerception(barrier), _BarrierPerception(barrier)
        a.check(FRAME)
        b.check(FRAME)
        assert a.ran.wait(1) and b.ran.wait(1)
        assert not barrier.broken
        assert det.calls == 1 and not det.release.is_set()
    finally:
        det.release.set()


def test_one_request_in_flight_max():
    det = _BlockingDetector()
    p, clock, _ = _perception(_FakeGate(GateVerdict.UNAVAILABLE), det)
    try:
        p._check_impl(FRAME)
        assert det.entered.wait(2)
        for _ in range(3):
            clock.t += STEP
            p._check_impl(FRAME)
        assert det.calls == 1
    finally:
        det.release.set()
    p._owlv2_future.result(timeout=5)
    clock.t += STEP
    _check(p)
    assert det.calls == 2


def test_retire_before_post_sends_nothing():
    det = _FakeDetector(FIRE_HZ)
    p, clock, events = _perception(_FakeGate(GateVerdict.UNAVAILABLE), det)
    p.retire()
    p._owlv2_round(FRAME, clock.t)
    assert det.frames == []
    assert events.sent == []


def test_retire_during_post_drops_event():
    det = _BlockingDetector(FIRE_HZ)
    p, clock, events = _perception(_FakeGate(GateVerdict.UNAVAILABLE), det)
    # A flame streak that this observation will confirm.
    p._streaks["flame"] = (clock.t - 12.0, clock.t - 6.0)
    p._check_impl(FRAME)
    assert det.entered.wait(2)
    p.retire()
    det.release.set()
    p._owlv2_future.result(timeout=5)
    assert events.sent == []


def test_retire_cleans_up_gate_and_worker():
    gate = _FakeGate(GateVerdict.UNAVAILABLE)
    p, clock, _ = _perception(gate, _FakeDetector([]))
    p.retire()
    assert gate.closed
    clock.t += STEP
    p._check_impl(FRAME)  # a late call after shutdown must not raise or post
    assert p.to_dict()["owlv2_inflight"] is False


def test_results_applied_in_order():
    p, clock, _ = _always_owlv2(FIRE_HZ)
    seen = []
    for _ in range(5):
        _check(p)
        seen.append(p._streaks["flame"][1])
        clock.t += STEP
    assert seen == sorted(seen) and len(set(seen)) == 5
    assert p._streaks["flame"][0] == T0


# --- 503 = server busy (#612) ---------------------------------------------------

from hal.drivers.sensing.perceptions.processors.utils import retry_after_s  # noqa: E402


@pytest.mark.parametrize(
    ("headers", "lo", "hi"),
    [
        ({"Retry-After": "2"}, 1.6, 2.4),
        ({"Retry-After": "100"}, 24.0, 36.0),   # clamped to 30 s, then jittered
        ({"Retry-After": "0"}, 1.6, 2.4),       # clamped up to 2 s
        ({}, 4.0, 6.0),                         # default 5 s
        ({"Retry-After": "Wed, 21 Oct 2015 07:28:00 GMT"}, 4.0, 6.0),  # date form -> default
    ],
)
def test_retry_after_is_clamped_and_jittered(headers, lo, hi):
    for _ in range(50):
        assert lo <= retry_after_s(headers) <= hi


def test_503_pauses_detection_for_retry_after(post, monkeypatch):
    now = [5000.0]
    monkeypatch.setattr(fire_hazard.time, "time", lambda: now[0])
    det = _detector()
    post.responses = [_Resp(503, {"detail": "busy"}, {"Retry-After": "2"})]
    assert det.detect(FRAME) is None
    assert det.last_outcome == "busy"
    assert 1.6 <= det._backoff_until - now[0] <= 2.4
    n = len(post.bodies)
    now[0] += 1.0
    assert det.detect(FRAME) is None
    assert len(post.bodies) == n  # still inside the pause: no request sent
