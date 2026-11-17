"""Deepgram STT follows the device stt_language instead of always using flux-general-en."""

from hal.drivers.voice.stt.deepgram import DEFAULT_MODEL, NON_ENGLISH_MODEL, deepgram_options


def test_english_or_unset_keeps_default_flux_model():
    assert deepgram_options(None) == {}
    assert deepgram_options("") == {}
    assert deepgram_options("en") == {}
    assert deepgram_options("en-US") == {}


def test_non_english_uses_nova3_with_language():
    for lang in ("vi", "ja", "ko", "ar", "zh-CN", "zh-TW"):
        assert deepgram_options(lang) == {"model": NON_ENGLISH_MODEL, "language": lang}


def test_regional_codes_normalize():
    assert deepgram_options("ko-KR") == {"model": NON_ENGLISH_MODEL, "language": "ko"}
    assert deepgram_options("ar-SA") == {"model": NON_ENGLISH_MODEL, "language": "ar"}


def test_configured_deepgram_model_is_kept():
    assert deepgram_options("ko", "nova-2-general") == {"model": "nova-2-general", "language": "ko"}
    assert deepgram_options("en", "nova-3-general") == {"model": "nova-3-general"}


def test_english_only_flux_is_not_used_for_other_languages():
    assert deepgram_options("ko", DEFAULT_MODEL)["model"] == NON_ENGLISH_MODEL


def test_non_deepgram_model_names_are_ignored():
    # stt_model also feeds Autonomous STT; a non-Deepgram name must not reach Deepgram.
    assert deepgram_options("ko", "whisper-1") == {"model": NON_ENGLISH_MODEL, "language": "ko"}
    assert deepgram_options("en", "whisper-1") == {}
