"""Korean speech pools and voices must never silently fall back to English."""

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
from hal.presets import LANG_EN, LANG_KO, SUPPORTED_LANGS

HANGUL = re.compile(r"[가-힣]")
EXPECTED_VOICES = ["Anna Kim", "Selly Han", "Rosa Oh", "Stella", "Hyunbin", "Minho"]


def test_korean_phrases_preserve_format_fields():
    assert LANG_KO in SUPPORTED_LANGS
    fields = lambda text: {name for _, name, _, _ in Formatter().parse(text) if name}
    for key, translations in PHRASES_BY_LANG.items():
        korean = translations[LANG_KO]
        assert localized_phrase(key, LANG_KO) == korean
        assert HANGUL.search(korean), key
        assert fields(korean) == fields(translations[LANG_EN]), key


def test_korean_interaction_pools_are_complete():
    for pools in (HEAD_PAT_PHRASES_BY_LANG, MIC_MUTED_PHRASES_BY_LANG, MIC_UNMUTED_PHRASES_BY_LANG):
        assert len(pools[LANG_KO]) == len(pools[LANG_EN])
        assert all(HANGUL.search(line) for line in pools[LANG_KO])
    assert DEFAULT_FILLERS_BY_LANG[LANG_KO].split(",")


def test_korean_music_tags_match_plain_pool():
    plain = MUSIC_BACKCHANNEL_POOLS[(LANG_KO, False)]
    tagged = MUSIC_BACKCHANNEL_POOLS[(LANG_KO, True)]
    assert len(plain) == len(MUSIC_BACKCHANNEL_POOLS[(LANG_EN, False)])
    assert [re.sub(r"^\[[a-z]+\] ", "", line) for line in tagged] == plain


def test_elevenlabs_korean_catalog_and_websocket_mapping():
    from hal.drivers.voice.tts.elevenlabs import ElevenLabsTTSBackend
    from hal.drivers.voice.tts.elevenlabs_ws import ElevenLabsWSTTSBackend

    for language in ("ko", "ko-KR", "KO"):
        assert ElevenLabsTTSBackend.voices_for_language(language) == EXPECTED_VOICES
    pool = ElevenLabsTTSBackend.VOICE_IDS_BY_LANG[LANG_KO]
    assert len(set(pool.values())) == 6
    for name in EXPECTED_VOICES:
        assert name in ElevenLabsTTSBackend.voices_for_language("")
        assert ElevenLabsWSTTSBackend.VOICE_IDS[name] == pool[name]


def test_voice_names_stay_unique_across_languages():
    # VOICE_IDS flattens every bucket by name, so a repeated name would shadow another voice.
    from hal.drivers.voice.tts.elevenlabs import ElevenLabsTTSBackend

    names = [n for voices in ElevenLabsTTSBackend.VOICE_IDS_BY_LANG.values() for n in voices]
    assert len(names) == len(set(names))


def test_korean_locale_aliases_reach_all_phrase_consumers(monkeypatch):
    import hal.config
    from hal.drivers import button_actions, os_shutdown
    from hal.drivers.voice import backchannel
    from hal.drivers.voice._internal.realtime_turn import _reply_language_name
    from hal.drivers.voice.lang_id.gate import resolve_language
    from hal.drivers.voice.tts.elevenlabs import ElevenLabsTTSBackend
    from hal.i18n import PHRASE_REBOOT
    from hal.realtime.config import _load_language
    from hal.routes import music

    for alias in ("ko", "ko-KR", "ko_KR", " KO-kr "):
        monkeypatch.setattr(hal.config, "_os_cfg_get", lambda *args: alias)
        assert localized_phrase(PHRASE_REBOOT, alias) == "지금 재부팅할게요."
        assert button_actions._current_lang() == LANG_KO
        assert button_actions._factory_reset_phrase() == "공장 초기화를 시작할게요. 지금 재부팅할게요."
        assert os_shutdown._phrase(PHRASE_REBOOT) == "지금 재부팅할게요."
        assert backchannel._default_fillers_for_active_lang() == DEFAULT_FILLERS_BY_LANG[LANG_KO]
        assert music._active_stt_language() == LANG_KO
        assert _load_language() == LANG_KO
        assert _reply_language_name() == "Korean"
        assert resolve_language(alias) == LANG_KO
        assert ElevenLabsTTSBackend.voices_for_language(alias)[0] == "Anna Kim"
