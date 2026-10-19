"""Explicit silence metadata must not escape as audible native PCM."""

import asyncio
import queue
import threading
from types import SimpleNamespace
from unittest.mock import Mock

import numpy as np
import pytest

from hal.realtime.models import AudioOutput, OutputEvent, TextOutput, TurnDoneEvent
from hal.realtime.voice_agent import gemini_live
from hal.realtime.voice_agent.gemini_live import GeminiLiveAgent, _SilenceMarkerAudioGate
from hal.drivers.voice._internal import realtime_turn


def packet(text=None, audio=False, terminal=False):
    return SimpleNamespace(
        usage_metadata=None, tool_call=None, session_resumption_update=None, go_away=None,
        server_content=SimpleNamespace(
            grounding_metadata=None, interrupted=False, generation_complete=False,
            turn_complete=terminal, input_transcription=None,
            output_transcription=SimpleNamespace(text=text) if text is not None else None,
            model_turn=SimpleNamespace(parts=[SimpleNamespace(
                thought=False, text=None, inline_data=SimpleNamespace(data=b'\x01\x00' * 160),
            )]) if audio else None,
        ),
    )


def receive(monkeypatch, packets, *, live=False):
    monkeypatch.setattr(gemini_live.app_config, 'LIVE_MODE', live)
    class Session:
        async def receive(self):
            for message in packets:
                yield message
    agent = object.__new__(GeminiLiveAgent)
    agent._session = Session()
    agent._recv_queue = queue.Queue()
    agent._turn_done = threading.Event()
    agent._first_audio_received = False
    agent._last_audio_sent_at = None
    agent._activity_end_sent_at = None
    agent._pending_tool_calls = set()
    asyncio.run(asyncio.wait_for(agent._async_receive_turn(), 1))
    return list(agent._recv_queue.queue)


# The last case is a device turn (2026-10-16): Gemini repeated the marker in square brackets.
@pytest.mark.parametrize('parts', [['NO_REPLY'], ['NO_', 'REPLY'], ['<no ', 'speech>'], ['<no speech>'],
                                   ['[no speech]'], ['<no ', 'speech>', '', '[no speech]']])
def test_complete_marker_drops_same_packet_and_split_audio_before_delivery(monkeypatch, parts):
    events = receive(monkeypatch, [packet(text, True, i == len(parts) - 1) for i, text in enumerate(parts)])
    assert not any(isinstance(event, OutputEvent) and isinstance(event.output, AudioOutput) for event in events)
    assert ''.join(event.output.text for event in events if isinstance(event, OutputEvent)
                   and isinstance(event.output, TextOutput)) == ''.join(parts)
    assert sum(isinstance(event, TurnDoneEvent) for event in events) == 1


@pytest.mark.parametrize('parts', [['No, the answer is five.'], ['No', ', the answer is five.'],
                                  ['NO_REPLY', ' The answer is five.'], ['NO_']])
def test_normal_answer_or_unfinished_marker_retains_all_audio(monkeypatch, parts):
    events = receive(monkeypatch, [packet(text, True, i == len(parts) - 1) for i, text in enumerate(parts)])
    assert sum(isinstance(event, OutputEvent) and isinstance(event.output, AudioOutput)
               for event in events) == len(parts)


def test_audio_only_and_prior_spoken_audio_cannot_be_retroactively_silenced(monkeypatch):
    for packets in ([packet(audio=True, terminal=True)],
                    [packet(audio=True), packet('NO_REPLY', True, True)]):
        events = receive(monkeypatch, packets)
        assert sum(isinstance(event, OutputEvent) and isinstance(event.output, AudioOutput)
                   for event in events) == len(packets)


def test_live_mode_audio_contract_is_unchanged(monkeypatch):
    events = receive(monkeypatch, [packet('NO_REPLY', True, True)], live=True)
    assert any(isinstance(event, OutputEvent) and isinstance(event.output, AudioOutput) for event in events)


def test_candidate_buffer_is_bounded_and_flushes_in_order_on_disambiguation():
    emitted = []
    gate = _SilenceMarkerAudioGate(emitted.append, enabled=True, max_bytes=8)
    first = OutputEvent(output=AudioOutput(audio=np.array([1.0, 2.0], dtype=np.float32)))
    second = OutputEvent(output=AudioOutput(audio=np.array([3.0], dtype=np.float32)))
    gate.observe('NO_')
    gate.publish(first)
    assert not emitted
    gate.publish(second)
    assert emitted == [first, second]
    assert gate._bytes == 0
    gate.observe('REPLY')
    gate.finish()
    assert len(emitted) == 2


@pytest.mark.parametrize('prior_audio', [False, True])
def test_native_marker_decision_never_reclassifies_already_spoken_output(monkeypatch, prior_audio):
    for key, value in [('REALTIME_ENABLED', True), ('REALTIME_NATIVE_AUDIO', True), ('REALTIME_PROVIDER', 'gemini')]:
        monkeypatch.setattr(realtime_turn.hal_config, key, value)
    monkeypatch.setattr(realtime_turn, 'gemini_needs_idle_workaround', lambda: False)
    monkeypatch.setattr(realtime_turn, 'pending_main_question', lambda: '')
    monkeypatch.setattr(realtime_turn, '_reply_language_name', lambda: 'English')
    monkeypatch.setattr(realtime_turn, '_thinking_cue_start', Mock())
    monkeypatch.setattr(realtime_turn, '_thinking_cue_clear', Mock())
    rt = Mock(available=True, execution_completed=True, intentional_silence=True, output_sample_rate=24000)
    outputs = ([AudioOutput(audio=np.zeros(160, dtype=np.float32))] if prior_audio else [])
    rt.stream_output.return_value = iter(outputs + [TextOutput(text='NO_REPLY')])
    tts = Mock()
    tts.native_play_begin.return_value = True
    result = realtime_turn.run_realtime_turn(
        rt, tts, lambda text: text, 'room chatter', [b'audio'], 1.0,
        wait_filler=Mock(), harness_followup=False,
    )
    assert result.rejected is (not prior_audio)
    assert result.handled is prior_audio
    assert tts.native_play_frame.called is prior_audio


def test_audio_without_candidate_text_is_not_buffered_awaiting_terminal():
    emitted = []
    gate = _SilenceMarkerAudioGate(emitted.append, enabled=True)
    frame = OutputEvent(output=AudioOutput(audio=np.zeros(160, dtype=np.float32)))
    gate.publish(frame)
    assert emitted == [frame]
    assert gate._bytes == 0
