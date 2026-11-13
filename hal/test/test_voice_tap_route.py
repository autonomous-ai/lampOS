"""POST /voice/tap mirrors one physical short tap."""

from unittest import mock


def test_voice_tap_runs_the_short_tap_and_reports_listening():
    import hal.app_state as state
    from hal.routes import voice

    fake = mock.Mock()
    fake.device_input.active = True
    with mock.patch("hal.drivers.device_tap_actions.physical_short_tap") as tap, \
            mock.patch.object(state, "voice_service", fake):
        assert voice.voice_tap() == {"status": "ok", "listening": True}
    tap.assert_called_once_with(source="api", announce=False)


def test_voice_tap_without_voice_service_is_not_listening():
    import hal.app_state as state
    from hal.routes import voice

    with mock.patch("hal.drivers.device_tap_actions.physical_short_tap"), \
            mock.patch.object(state, "voice_service", None):
        assert voice.voice_tap() == {"status": "ok", "listening": False}
