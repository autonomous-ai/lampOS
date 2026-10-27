"""The realtime prompt names what people call the device, so a spoken name is not taken as the request."""

import hal.config as app_config
from hal.realtime.context_manager import CONTEXT_MANAGERS
from hal.realtime.context_manager import base


def test_owner_wake_phrases_reach_the_prompt(tmp_path, monkeypatch):
    monkeypatch.setattr(app_config, "custom_wake_phrases", lambda: ["sam", "lam", "sam"])
    manager = CONTEXT_MANAGERS["openclaw"](workspace_dir=str(tmp_path), provider="gemini")
    instructions = manager.build_instructions()
    assert "# HOW PEOPLE CALL YOU" in instructions
    assert 'saying one of these: "sam", "lam".' in instructions


def test_built_in_names_when_the_owner_kept_defaults(monkeypatch):
    monkeypatch.setattr(app_config, "custom_wake_phrases", lambda: [])
    names = base._wake_names()
    from hal.drivers.voice._internal.config import DEFAULT_WAKE_WORDS
    assert names == [w.casefold() for w in dict.fromkeys(DEFAULT_WAKE_WORDS)]


def test_no_names_adds_no_section():
    assert base.wake_names_section([]) == ""
