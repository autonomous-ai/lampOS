"""Fast voice (default on for Intern v2 only): off changes nothing; on shortens the waits before Gemini."""

import json
import os
import subprocess
import sys
import threading
from pathlib import Path
from types import SimpleNamespace

import numpy as np
import pytest

from hal.test.test_turn_endpoint_capture import capture

_REPO = Path(__file__).resolve().parents[2]

_PROBE = """
import json
import hal.config as c
import hal.drivers.voice._internal.config as v
import hal.drivers.voice.stt.autonomous as a
import hal.drivers.voice.stt.deepgram as d
print(json.dumps({
    "fast": c.FAST_VOICE, "native": c.REALTIME_NATIVE_AUDIO, "model": c.REALTIME_GEMINI_MODEL,
    "silence": v.SILENCE_TIMEOUT_S, "prepass": v.SPEAKER_PREPASS_COMMIT_JOIN_S,
    "gap": v.LANG_GATE_UTTERANCE_GAP_S, "endpointing": [a.DEFAULT_ENDPOINTING_MS, d.DEFAULT_ENDPOINTING_MS],
    "pause": [v.TURN_END_MAX_PAUSE_S, v.TURN_END_FALLBACK_S],
    "silent": c.REALTIME_RECV_QUEUE_TIMEOUT_S,
}))
"""


def _knobs(**env) -> dict:
    clean = {k: v for k, v in os.environ.items() if not k.startswith("HAL_") and k != "DEVICE_TYPE"}
    # No config.json: only env and code defaults decide.
    clean["OS_CONFIG_PATH"] = "/nonexistent/config.json"
    clean.setdefault("DEVICE_TYPE", "intern-v2")
    clean.update(env)
    out = subprocess.run(
        [sys.executable, "-c", _PROBE], cwd=_REPO, env=clean,
        capture_output=True, text=True, check=True,
    ).stdout
    return json.loads(out.strip().splitlines()[-1])


def test_intern_v2_is_on_by_default():
    assert _knobs(HAL_SILENCE_TIMEOUT="1.2")["fast"] is True


@pytest.mark.parametrize("device", ["lamp", "", "intern"])
def test_other_devices_are_off_by_default_but_can_opt_in(device):
    knobs = _knobs(DEVICE_TYPE=device)
    assert knobs["fast"] is False and knobs["native"] is False
    assert knobs["silence"] == 2.5 and knobs["endpointing"] == [1500, 1500]
    assert _knobs(DEVICE_TYPE=device, HAL_FAST_VOICE="1")["fast"] is True


def test_flag_off_keeps_every_existing_wait():
    assert _knobs(HAL_FAST_VOICE="0") == {
        "fast": False, "native": False, "model": "gemini-3.8-live",
        "silence": 2.5, "prepass": 0.2, "gap": 1.0, "endpointing": [1500, 1500],
        "pause": [6.0, 2.5], "silent": 8.0,
    }


def test_flag_off_keeps_explicit_settings():
    knobs = _knobs(HAL_FAST_VOICE="0", HAL_SILENCE_TIMEOUT="1.2",
                   HAL_GEMINI_LIVE_MODEL="gemini-3.8-live-extended-thinking")
    assert knobs["silence"] == 1.2
    assert knobs["model"] == "gemini-3.8-live-extended-thinking"
    assert knobs["native"] is False


def test_flag_on_shortens_waits_and_keeps_the_model():
    knobs = _knobs(
        HAL_FAST_VOICE="1", HAL_SILENCE_TIMEOUT="1.2",
        HAL_GEMINI_LIVE_MODEL="gemini-3.8-live-extended-thinking",
    )
    assert knobs == {
        "fast": True, "native": True, "model": "gemini-3.8-live-extended-thinking",
        "silence": 0.65, "prepass": 0.0, "gap": 0.65, "endpointing": [400, 400],
        "pause": [2.0, 2.0], "silent": 5.0,
    }


def test_flag_on_waits_stay_tunable():
    knobs = _knobs(
        HAL_FAST_VOICE="true", HAL_FAST_VOICE_SILENCE_S="0.8",
        HAL_FAST_VOICE_PREPASS_JOIN_S="0.1", HAL_FAST_VOICE_STT_ENDPOINTING_MS="500",
    )
    assert (knobs["silence"], knobs["prepass"], knobs["gap"]) == (0.8, 0.1, 0.8)
    assert knobs["endpointing"] == [500, 500]


@pytest.mark.parametrize("value,want", [("1.5", [1.5, 1.5]), ("4", [4.0, 2.5]), ("0.1", [0.8, 0.8]), ("99", [6.0, 2.5]), ("x", [2.0, 2.0])])
def test_fast_voice_pause_is_tunable_and_bounded(value, want):
    assert _knobs(HAL_FAST_VOICE="1", HAL_FAST_VOICE_MAX_PAUSE_S=value)["pause"] == want


@pytest.mark.parametrize("value,want", [("4", 4.0), ("1", 3.0), ("20", 8.0), ("x", 5.0)])
def test_fast_voice_silent_timeout_is_tunable_and_bounded(value, want):
    assert _knobs(HAL_FAST_VOICE="1", HAL_FAST_VOICE_SILENT_TIMEOUT_S=value)["silent"] == want
    # Off keeps the existing timeout whatever the setting says.
    assert _knobs(HAL_FAST_VOICE="0", HAL_FAST_VOICE_SILENT_TIMEOUT_S=value)["silent"] == 8.0


@pytest.mark.parametrize("env", [
    {"HAL_REALTIME_PROVIDER": "openai"},
    {"HAL_REALTIME_ENABLED": "false"},
    {"HAL_LIVE_MODE": "true"},
])
def test_flag_is_inert_outside_gemini_turn_mode(env):
    knobs = _knobs(HAL_FAST_VOICE="1", **env)
    assert knobs["fast"] is False
    assert knobs["silence"] == 2.5 and knobs["endpointing"] == [1500, 1500]


class _SyncThread:
    def __init__(self, target, **_):
        self.target = target

    def start(self):
        self.target()

    def is_alive(self):
        return False

    def join(self, *_):
        return None


def _wake_capture(monkeypatch, fast: bool, first=("Sam, please check my memory", True), wake=None):
    """A closed wake window: "Sam, ..." heard by STT on the first frame.

    Returns the capture context and the frame being read when the turn was bound.
    """
    from hal.drivers.voice import voice_service as module

    monkeypatch.setattr(module.voice_cfg, "FAST_VOICE", fast)
    monkeypatch.setattr(module, "threading", SimpleNamespace(
        Thread=_SyncThread, Event=threading.Event, Lock=threading.Lock,
        Timer=lambda *_a, **_k: SimpleNamespace(start=lambda: None),
    ))
    reading = [0]
    bound_at = []

    def configure(service):
        if wake is None:
            service._decorator.starts_with_wake_word.return_value = True
        else:
            service._decorator.starts_with_wake_word.side_effect = wake
        service._realtime.bind_audio_turn.side_effect = lambda: bound_at.append(reading[0]) or object()

    def on_read(elapsed):
        reading[0] = elapsed

    frames = [
        (1, True, first),
        (2, True, None),
        (3, True, None),
        (6, False, None),
    ]
    return capture(
        monkeypatch, frames, realtime=True, wake_enabled=True, focus=lambda: False,
        configure_service=configure, on_read=on_read,
    ), bound_at


def test_fast_voice_streams_a_confirmed_wake_turn_during_capture(monkeypatch):
    ctx, bound_at = _wake_capture(monkeypatch, fast=True)
    with ctx as result:
        # Bound while the person was still speaking, not after the last frame.
        assert bound_at and bound_at[0] < 6
        result.realtime.assert_called_once()


def test_without_fast_voice_a_wake_turn_still_waits_for_the_capture(monkeypatch):
    ctx, bound_at = _wake_capture(monkeypatch, fast=False)
    with ctx as result:
        assert bound_at == [6]
        result.realtime.assert_called_once()


def test_fast_voice_streams_on_a_wake_partial_and_commits_once_confirmed(monkeypatch):
    frames_text = ("Sam, please", False)
    ctx, bound_at = _wake_capture(monkeypatch, fast=True, first=frames_text)
    with ctx as result:
        # Bound on the partial, while the person was still speaking; the assembled
        # transcript confirms the wake at the end, so the turn still runs.
        assert bound_at and bound_at[0] < 6
        result.realtime.assert_called_once()
        result.service._realtime.discard_open_activity.assert_not_called()


def test_fast_voice_discards_a_partial_stream_the_final_does_not_confirm(monkeypatch):
    # The partial heard "Sam"; nothing after it confirms the name.
    ctx, bound_at = _wake_capture(
        monkeypatch, fast=True, first=("Sam, please", False),
        wake=lambda text, seen=[]: not seen and not seen.append(text),
    )
    with ctx as result:
        assert bound_at and bound_at[0] < 6
        result.service._realtime.discard_open_activity.assert_called_once_with("wake-unconfirmed")
        result.realtime.assert_not_called()
        result.service._realtime.commit_audio.assert_not_called()
