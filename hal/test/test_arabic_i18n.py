"""Arabic speech pools and voices must never silently fall back to English."""

import re
from string import Formatter

from hal.i18n import (
    DEFAULT_FILLERS_BY_LANG,
    HEAD_PAT_PHRASES_BY_LANG,
    MIC_MUTED_PHRASES_BY_LANG,
    MIC_UNMUTED_PHRASES_BY_LANG,
    MUSIC_BACKCHANNEL_POOLS,
    PHRASES_BY_LANG,
    localized_phrase,
)
from hal.presets import LANG_AR, LANG_EN, SUPPORTED_LANGS, normalize_language

ARABIC = re.compile(r"[؀-ۿ]")
EXPECTED_VOICES = ["Salma", "Sara", "Farah", "Mona", "Haytham", "Anas"]


def test_arabic_regional_codes_share_one_language():
    for code in ("ar", "AR", "ar-SA", "ar_EG", " ar-ae "):
        assert normalize_language(code) == LANG_AR
    # A code that merely starts with "ar" letters is not Arabic.
    assert normalize_language("arn") == "arn"


def test_arabic_phrases_preserve_format_fields():
    assert LANG_AR in SUPPORTED_LANGS
    fields = lambda text: {name for _, name, _, _ in Formatter().parse(text) if name}
    for key, translations in PHRASES_BY_LANG.items():
        arabic = translations[LANG_AR]
        assert localized_phrase(key, LANG_AR) == arabic
        assert ARABIC.search(arabic), key
        assert fields(arabic) == fields(translations[LANG_EN]), key


def test_arabic_interaction_pools_are_complete():
    for pools in (HEAD_PAT_PHRASES_BY_LANG, MIC_MUTED_PHRASES_BY_LANG, MIC_UNMUTED_PHRASES_BY_LANG):
        assert len(pools[LANG_AR]) == len(pools[LANG_EN])
        assert all(ARABIC.search(line) for line in pools[LANG_AR])
    assert all(ARABIC.search(f) for f in DEFAULT_FILLERS_BY_LANG[LANG_AR].split(","))


def test_arabic_music_tags_match_plain_pool():
    plain = MUSIC_BACKCHANNEL_POOLS[(LANG_AR, False)]
    tagged = MUSIC_BACKCHANNEL_POOLS[(LANG_AR, True)]
    assert len(plain) == len(MUSIC_BACKCHANNEL_POOLS[(LANG_EN, False)])
    assert [re.sub(r"^\[[a-z]+\] ", "", line) for line in tagged] == plain


def test_elevenlabs_arabic_catalog_and_websocket_mapping():
    from hal.drivers.voice.tts.elevenlabs import ElevenLabsTTSBackend
    from hal.drivers.voice.tts.elevenlabs_ws import ElevenLabsWSTTSBackend

    for language in ("ar", "ar-SA", "AR"):
        assert ElevenLabsTTSBackend.voices_for_language(language) == EXPECTED_VOICES
    pool = ElevenLabsTTSBackend.VOICE_IDS_BY_LANG[LANG_AR]
    assert len(set(pool.values())) == 6
    for name in EXPECTED_VOICES:
        assert name in ElevenLabsTTSBackend.voices_for_language("")
        assert ElevenLabsWSTTSBackend.VOICE_IDS[name] == pool[name]


def test_arabic_locale_aliases_reach_all_phrase_consumers(monkeypatch):
    import hal.config
    from hal.drivers import button_actions, os_shutdown
    from hal.drivers.voice import backchannel
    from hal.drivers.voice._internal.realtime_turn import _reply_language_name
    from hal.drivers.voice.lang_id.gate import resolve_language
    from hal.drivers.voice.lang_id.identifier import _APP_TO_MODEL
    from hal.drivers.voice.tts.elevenlabs import ElevenLabsTTSBackend
    from hal.i18n import PHRASE_REBOOT
    from hal.realtime.config import _load_language
    from hal.routes import music

    assert _APP_TO_MODEL[LANG_AR] == "ar"
    for alias in ("ar", "ar-SA", "ar_EG", " AR-ae "):
        monkeypatch.setattr(hal.config, "_os_cfg_get", lambda *args: alias)
        assert localized_phrase(PHRASE_REBOOT, alias) == "جارٍ إعادة التشغيل الآن."
        assert button_actions._current_lang() == LANG_AR
        assert button_actions._factory_reset_phrase() == "جارٍ استعادة إعدادات المصنع. سأعيد التشغيل الآن."
        assert os_shutdown._phrase(PHRASE_REBOOT) == "جارٍ إعادة التشغيل الآن."
        assert backchannel._default_fillers_for_active_lang() == DEFAULT_FILLERS_BY_LANG[LANG_AR]
        assert music._active_stt_language() == LANG_AR
        assert _load_language() == LANG_AR
        assert _reply_language_name() == "Arabic"
        assert resolve_language(alias) == LANG_AR
        assert ElevenLabsTTSBackend.voices_for_language(alias)[0] == "Salma"
