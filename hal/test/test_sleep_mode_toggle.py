"""Sleep mode is opt-in: off by default, it blocks every sleep path and wakes a stuck device."""

from unittest import mock

import pytest

import hal.app_state as state
from hal import config
from hal.drivers import button_actions
from hal.models import EmotionRequest
from hal.routes import emotion


@pytest.fixture
def awake(monkeypatch):
    monkeypatch.setattr(state, "_sleeping", False)
    monkeypatch.setattr(state, "_current_emotion", "idle")
    monkeypatch.setattr(emotion, "harness_blocks_sleep", lambda: False)
    monkeypatch.setattr(state, "_finalize_sleepy_peripherals", mock.Mock())
    monkeypatch.setattr(state, "_persist_sleep_state", mock.Mock())
    monkeypatch.setattr(state, "_log_sleep_transition", mock.Mock())


def test_sleep_mode_defaults_off(monkeypatch):
    monkeypatch.setattr(config, "_os_cfg_get", lambda key, default=None: default)
    assert config.sleep_mode_enabled() is False


def test_sleep_mode_on_only_when_owner_sets_true(monkeypatch):
    for value, expected in ((True, True), (False, False), ("true", False), (None, False)):
        monkeypatch.setattr(config, "_os_cfg_get", lambda key, default=None, v=value: v)
        assert config.sleep_mode_enabled() is expected


def test_sleepy_emotion_is_ignored_while_sleep_mode_is_off(awake, monkeypatch):
    monkeypatch.setattr(config, "sleep_mode_enabled", lambda: False)
    result = emotion.express_emotion(EmotionRequest(emotion="sleepy"), source="voice")
    assert result["status"] == "ignored"
    assert state._sleeping is False
    state._finalize_sleepy_peripherals.assert_not_called()


def test_button_sleep_hold_is_ignored_while_sleep_mode_is_off(awake, monkeypatch):
    monkeypatch.setattr(config, "sleep_mode_enabled", lambda: False)
    with mock.patch.object(button_actions, "_tts_available", return_value=True), \
            mock.patch.object(state, "tts_service") as tts, \
            mock.patch.object(emotion, "express_emotion") as express:
        button_actions.sleep_action("GPIO button")
    tts.speak_cached.assert_not_called()
    express.assert_not_called()


def test_disabled_sleep_is_released_quietly(monkeypatch):
    monkeypatch.setattr(config, "sleep_mode_enabled", lambda: False)
    monkeypatch.setattr(state, "_sleeping", True)
    wake = mock.Mock()
    monkeypatch.setattr(state, "_wake_sleepy_peripherals", wake)
    monkeypatch.setattr(state, "_log_sleep_transition", mock.Mock())
    monkeypatch.setattr(state, "note_presence_wake", mock.Mock())
    express = mock.Mock()
    monkeypatch.setattr(emotion, "express_emotion", express)
    with mock.patch("hal.routes.led.restore_led") as restore:
        assert emotion.release_disabled_sleep("boot_sleep_mode_off") is True
    assert state._sleeping is False
    wake.assert_called_once()
    state._log_sleep_transition.assert_called_once_with("wake", "idle", "boot_sleep_mode_off")
    # Idle, not the stretching wake performance.
    assert express.call_args.args[0].emotion == "idle"
    restore.assert_called_once()


def test_enabled_sleep_is_kept(monkeypatch):
    monkeypatch.setattr(config, "sleep_mode_enabled", lambda: True)
    monkeypatch.setattr(state, "_sleeping", True)
    wake = mock.Mock()
    monkeypatch.setattr(state, "_wake_sleepy_peripherals", wake)
    assert emotion.release_disabled_sleep() is False
    assert state._sleeping is True
    wake.assert_not_called()


def test_sync_route_reports_and_wakes(monkeypatch):
    from hal.routes import system

    monkeypatch.setattr(config, "sleep_mode_enabled", lambda: False)
    monkeypatch.setattr(emotion, "release_disabled_sleep", lambda source: True)
    assert system.sleep_mode_sync() == {"sleep_mode": False, "woke": True}
