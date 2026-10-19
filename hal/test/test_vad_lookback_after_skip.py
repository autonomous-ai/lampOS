import threading
"""A trigger the live noise-guard rejects must not erase the pre-roll."""

from unittest.mock import Mock, patch

import numpy as np
import pytest


def _frame(value):
    return np.full((320, 1), value, dtype=np.int16)


@pytest.mark.parametrize("first_decision", ["skip", "live"])
def test_pre_roll_after_skipped_trigger_keeps_the_rejected_frames(monkeypatch, first_decision):
    from hal.drivers.voice import voice_service as module

    frames = iter([_frame(1000), _frame(2000), _frame(3000)])
    mic = Mock()
    mic.read.side_effect = lambda _: (next(frames), False)

    service = Mock()
    service._automatic_reply_lock = threading.Lock()
    service._automatic_reply_stop = None
    service._automatic_reply_cancelled_at = None
    service._running = True
    service._stt_drain_worker = None
    service._stt_drain_future = None
    service._np = np
    service._tts_is_speaking.return_value = False
    service._music_is_playing.return_value = False
    service._backchannel.self_audio_active = False
    service._webrtcvad_is_speech.return_value = True
    # Exercise the real entry gates. A bare Mock makes the newly introduced
    # hardware-AEC predicate truthy and incorrectly selects its 160 ms holdoff.
    service._live_gate = None
    service._hardware_aec_live_entry = module.VoiceService._hardware_aec_live_entry.__get__(service)
    service._vad_entry_is_speech = module.VoiceService._vad_entry_is_speech.__get__(service)
    service._silero_vad = None  # energy + webrtc gate only; Silero is opt-in
    service._live_decision.side_effect = [first_decision, "live"]
    service._live_session.return_value = True

    monkeypatch.setattr(module.voice_cfg, "STT_KEEPALIVE", False)
    monkeypatch.setattr(module.voice_cfg, "WARM_MIC", True)
    monkeypatch.setattr(module.voice_cfg, "LIVE_MODE", True)
    monkeypatch.setattr(module.voice_cfg, "RMS_THRESHOLD", 0)
    monkeypatch.setattr(module.voice_cfg, "SPEECH_HOLDOFF_S", 0.0)
    monkeypatch.setattr(module.voice_cfg, "SESSION_COOLDOWN_S", 0.0)
    monkeypatch.setattr(module.voice_cfg, "PRE_ROLL_FRAMES", 12)
    with patch.object(module, "read_voice_mode", return_value={"enabled": False, "generation": 0}), \
         patch.object(module, "resample_to_stt", side_effect=lambda f, *a: f):
        module.VoiceService._vad_loop(service, mic, 320, 16000)

    pre_roll = service._live_session.call_args.args[3]
    values = [int(f[0, 0]) for f in pre_roll]
    if first_decision == "skip":
        # The rejected first trigger (frame 1000) rides along as pre-roll.
        assert values == [1000, 2000]
    else:
        # A session that actually opened ends the loop on the first trigger.
        assert values == [1000]
    expected_triggers = 2 if first_decision == "skip" else 1
    assert service._webrtcvad_is_speech.call_count == expected_triggers
    assert service._live_decision.call_count == expected_triggers


@pytest.mark.parametrize("first_decision", ["skip", "live"])
def test_a_capture_that_opens_no_turn_closes_the_preconnected_stt(monkeypatch, first_decision):
    """A gate reject used to drop the pre-connected STT without closing it; the
    abandoned socket idled until the server killed it (Flux INACTIVE_CLIENT, 60 s)."""
    from hal.drivers.voice import voice_service as module

    frames = iter([_frame(1000 * (n + 1)) for n in range(12)])
    mic = Mock()
    mic.read.side_effect = lambda _: (next(frames), False)
    service = Mock()
    service._automatic_reply_lock = threading.Lock()
    service._automatic_reply_stop = None
    service._automatic_reply_cancelled_at = None
    service._running = True
    service._np = np
    service._tts_is_speaking.return_value = False
    service._music_is_playing.return_value = False
    service._backchannel.self_audio_active = False
    service._webrtcvad_is_speech.return_value = True
    service._live_gate = None
    service._hardware_aec_live_entry = module.VoiceService._hardware_aec_live_entry.__get__(service)
    service._vad_entry_is_speech = module.VoiceService._vad_entry_is_speech.__get__(service)
    service._silero_vad = None
    sessions = [Mock(name=f"stt{i}") for i in range(4)]
    service._stt.create_session.side_effect = sessions
    service._live_decision.side_effect = [first_decision, "live"]
    # A live capture that does not keep the mic falls through, like a skip.
    service._live_session.side_effect = [False, True] if first_decision == "live" else [True]

    monkeypatch.setattr(module.voice_cfg, "STT_KEEPALIVE", True)
    monkeypatch.setattr(module.voice_cfg, "WARM_MIC", True)
    monkeypatch.setattr(module.voice_cfg, "LIVE_MODE", True)
    monkeypatch.setattr(module.voice_cfg, "RMS_THRESHOLD", 0)
    monkeypatch.setattr(module.voice_cfg, "SPEECH_HOLDOFF_S", 0.0)
    monkeypatch.setattr(module.voice_cfg, "SESSION_COOLDOWN_S", 0.0)
    monkeypatch.setattr(module.voice_cfg, "PRE_ROLL_FRAMES", 12)
    retired = []
    monkeypatch.setattr(module.VoiceService, "_retire_keepalive_session", staticmethod(retired.append))
    with patch.object(module, "read_voice_mode", return_value={"enabled": False, "generation": 0}), \
         patch.object(module, "resample_to_stt", side_effect=lambda f, *a: f):
        module.VoiceService._vad_loop(service, mic, 320, 16000)

    # Every pre-connected session that no turn took over is closed.
    assert retired == sessions[: service._stt.create_session.call_count]


def test_stale_mic_backlog_is_dropped_until_reads_wait_for_the_clock(monkeypatch):
    """Native replies play while nobody reads the mic; the queued audio (starting with
    the device's own words) must not come back as the next speech."""
    from hal.drivers.voice import voice_service as module

    clock = [0.0]
    reads = []
    # Three queued frames come back at once, then the fourth waits a full frame.
    def read(_):
        reads.append(clock[0])
        if len(reads) > 3:
            clock[0] += 0.02
        return _frame(9000), False

    monkeypatch.setattr(module.time, "monotonic", lambda: clock[0])
    service = Mock(_running=True)
    mic = Mock(read=read)
    dropped = module.VoiceService._discard_stale_mic_backlog(service, mic, 320, 16000)
    assert len(reads) == 4
    assert dropped == pytest.approx(3 * 0.02)


def test_stale_mic_backlog_drop_is_bounded(monkeypatch):
    from hal.drivers.voice import voice_service as module

    monkeypatch.setattr(module.time, "monotonic", lambda: 0.0)
    mic = Mock(read=lambda _: (_frame(9000), False))
    dropped = module.VoiceService._discard_stale_mic_backlog(Mock(_running=True), mic, 320, 16000)
    assert dropped == pytest.approx(module.VoiceService.STALE_MIC_BACKLOG_MAX_S, abs=0.02)
