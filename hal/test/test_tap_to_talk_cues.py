"""Tap to talk has its own ready chime and listening light, separate from the wake cues."""

import json
from unittest.mock import Mock, patch

import pytest

from hal import config as hal_config
from hal import wake_light
from hal.drivers.voice._internal.device_capture import DeviceCapture
from hal.drivers.voice._internal.harness_capture import Capture
from hal.drivers.voice._internal.input_policy import InputPolicy

TAP_LOOK = {"color": [40, 0, 20], "effect": "blink", "speed": 2.0}
WAKE_LOOK = {"color": [0, 20, 40], "effect": "spin", "speed": 1.5}


def _write_config(tmp_path, monkeypatch, data):
    path = tmp_path / "config.json"
    path.write_text(json.dumps(data))
    monkeypatch.setattr(hal_config, "OS_CONFIG_PATH", str(path))


def test_tap_chime_defaults_on(tmp_path, monkeypatch):
    _write_config(tmp_path, monkeypatch, {})
    assert hal_config.tap_chime_enabled()
    _write_config(tmp_path, monkeypatch, {"tap_chime": False, "wake_chime": True})
    assert not hal_config.tap_chime_enabled()


def test_tap_light_is_forced_on_when_its_chime_is_off(tmp_path, monkeypatch):
    _write_config(tmp_path, monkeypatch, {"tap_light": {"enabled": False}})
    assert not wake_light.tap_enabled()
    _write_config(tmp_path, monkeypatch, {"tap_light": {"enabled": False}, "tap_chime": False})
    assert wake_light.tap_enabled()
    _write_config(tmp_path, monkeypatch, {"wake_light": {"enabled": False}})
    assert wake_light.tap_enabled()


def test_listening_look_follows_the_input_mode(tmp_path, monkeypatch):
    _write_config(tmp_path, monkeypatch, {"tap_light": TAP_LOOK, "wake_light": WAKE_LOOK})
    monkeypatch.setattr(hal_config, "VOICE_INPUT_MODE", "tap_to_talk")
    assert wake_light.listening_preset()["color"] == TAP_LOOK["color"]
    monkeypatch.setattr(hal_config, "VOICE_INPUT_MODE", "automatic")
    assert wake_light.listening_preset()["color"] == WAKE_LOOK["color"]


def test_unset_tap_light_keeps_the_board_look_not_the_wake_look(tmp_path, monkeypatch):
    _write_config(tmp_path, monkeypatch, {"wake_light": WAKE_LOOK})
    monkeypatch.setattr(hal_config, "VOICE_INPUT_MODE", "tap_to_talk")
    assert wake_light.listening_preset() == wake_light.look_preset()


def _device_policy():
    return InputPolicy(explicit=True, device_capture=True, harness_capture=False, realtime_allowed=True)


@pytest.mark.parametrize("light_on", [True, False])
def test_tap_light_toggle_gates_the_listening_visual(tmp_path, monkeypatch, light_on):
    _write_config(tmp_path, monkeypatch, {"tap_light": {"enabled": light_on}})
    emotion = Mock()
    with patch("hal.app_state.clear_listening_cue") as clear:
        _device_policy().set_capturing(True, emotion)
        _device_policy().set_capturing(False, emotion)
    assert emotion.called is light_on
    clear.assert_called_once_with()


@pytest.mark.parametrize("chime_on", [True, False])
def test_tap_chime_toggle_gates_both_capture_tones(tmp_path, monkeypatch, chime_on):
    _write_config(tmp_path, monkeypatch, {"tap_chime": chime_on})
    tts = Mock()
    controller = DeviceCapture(Capture({}), tts, Mock(), lambda: True)
    controller._cue()
    controller._cue(finished=True)
    assert tts.play_device_capture_chime.call_count == (2 if chime_on else 0)
