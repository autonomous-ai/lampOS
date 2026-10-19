"""Voice service environment-variable configuration."""

import os
from pathlib import Path

from hal import config as _hal_config


OS_SENSING_URL = "http://127.0.0.1:5000/api/sensing/event"
OS_HARNESS_FOLLOWUP_URL = "http://127.0.0.1:5000/api/harness/voice-followup"
OS_FILLER_URL = "http://127.0.0.1:5000/api/sensing/filler"


STT_RATE = 16000
CHANNELS = 1
FRAME_DURATION_MS = 64


RMS_THRESHOLD = int(os.environ.get("HAL_VAD_THRESHOLD", "3500"))
SILENCE_TIMEOUT_S = float(os.environ.get("HAL_SILENCE_TIMEOUT", "2.5"))
SPEECH_HOLDOFF_S = float(os.environ.get("HAL_SPEECH_HOLDOFF", "0.2"))
# Pre-roll lookback — 8 × 64ms = 512ms of audio history before VAD trigger so
# quiet first syllables ("b", "k", "t", "p") reach STT instead of getting clipped.
PRE_ROLL_FRAMES = int(os.environ.get("HAL_PRE_ROLL_FRAMES", "8"))
SESSION_COOLDOWN_S = float(os.environ.get("HAL_SESSION_COOLDOWN_S", "0.3"))


SILERO_VAD_ENABLED = os.environ.get("HAL_SILERO_ENABLED", "false").lower() == "true"
SILERO_VAD_THRESHOLD = float(os.environ.get("HAL_SILERO_THRESHOLD", "0.3"))
SILERO_CHUNK_SIZE = int(os.environ.get("HAL_SILERO_CHUNK_SIZE", "512"))
SILERO_MODEL_PATH = Path(__file__).resolve().parent.parent / "resources" / "silero_vad.onnx"

# Silero on the SILENCE clock (end of turn), not just the entry gate. In a noisy room
# the noise floor sits above the threshold, so the timer never expires.
SILENCE_VAD_ENABLED = os.environ.get("HAL_SILENCE_VAD_ENABLED", "true").lower() == "true"
SILENCE_VAD_WINDOW_FRAMES = int(os.environ.get("HAL_SILENCE_VAD_WINDOW_FRAMES", "3"))


WEBRTCVAD_ENABLED = os.environ.get("HAL_WEBRTCVAD_ENABLED", "false").lower() == "true"
WEBRTCVAD_AGGRESSIVENESS = int(os.environ.get("HAL_WEBRTCVAD_AGGRESSIVENESS", "2"))
WEBRTCVAD_FRAME_MS = int(os.environ.get("HAL_WEBRTCVAD_FRAME_MS", "30"))


ECHO_RMS_FLOOR = int(os.environ.get("HAL_ECHO_RMS_FLOOR", "200"))
ECHO_GATE_MAX_WAIT_S = float(os.environ.get("HAL_ECHO_GATE_MAX_WAIT_S", "1.5"))
ECHO_GATE_WINDOW_S = float(os.environ.get("HAL_ECHO_GATE_WINDOW_S", "0.05"))
ECHO_SIMILARITY_THRESHOLD = float(os.environ.get("HAL_ECHO_SIMILARITY_THRESHOLD", "0.55"))
ECHO_RELEVANCE_WINDOW_S = float(os.environ.get("HAL_ECHO_RELEVANCE_WINDOW_S", "15.0"))
MAX_SESSION_DURATION_S = float(os.environ.get("HAL_MAX_SESSION_DURATION_S", "30"))
# Hands-free capture with recognized words can outlive the short noise/manual
# capture ceiling. A hard limit aborts; it is never permission to execute a
# possibly unfinished request.
STREAM_NOISE_EVIDENCE_ENABLED = os.environ.get("HAL_STREAM_NOISE_EVIDENCE", "false").lower() == "true"
TURN_END_ENABLED = os.environ.get("HAL_TURN_END_ENABLED", "true").lower() == "true"
TURN_END_FALLBACK_S = float(os.environ.get("HAL_TURN_END_FALLBACK_S", "2.5"))
TURN_END_MAX_PAUSE_S = float(os.environ.get("HAL_TURN_END_MAX_PAUSE_S", "6.0"))
TURN_END_MAX_DURATION_S = float(os.environ.get("HAL_TURN_END_MAX_DURATION_S", "180"))

WARM_MIC = os.environ.get("HAL_WARM_MIC", "true").lower() == "true"
# Experimental automatic ALSA capture isolation; manual and live paths are unchanged.
CONTINUOUS_MIC_CAPTURE = os.environ.get("HAL_CONTINUOUS_MIC_CAPTURE", "false").lower() == "true"
# Bound input retained while the consumer waits for a response or STT drain.
CONTINUOUS_MIC_BACKLOG_S = max(1.024, min(10.0, float(
    os.environ.get("HAL_CONTINUOUS_MIC_BACKLOG_S", "1.024")
)))
# Max echo-skip after TTS/music ends before resuming VAD (warm mic only).
WARM_MIC_ECHO_SKIP_MAX_S = float(os.environ.get("HAL_WARM_MIC_ECHO_SKIP_MAX_S", "0.1"))


# Acoustic echo cancellation (WebRTC AEC3) — see drivers/voice/aec.py. On by default.
AEC_ENABLED = os.environ.get("HAL_AEC_ENABLED", "true").lower() == "true"
# Speaker→mic delay hint. Re-measure with HAL_AEC_DUMP_DIR after any audio hardware
# change; a wrong hint costs convergence, not correctness.
AEC_DELAY_MS = int(os.environ.get("HAL_AEC_DELAY_MS", "205"))
AEC_NOISE_SUPPRESSION = os.environ.get("HAL_AEC_NS", "true").lower() == "true"
# Keep cancelling for this long after the last speaker write, then bypass the APM until
# playback resumes.
AEC_TAIL_S = float(os.environ.get("HAL_AEC_TAIL_S", "2.0"))
# Depth of the echo-reference FIFO.
AEC_REF_MS = int(os.environ.get("HAL_AEC_REF_MS", "500"))
AEC_DUMP_DIR = os.environ.get("HAL_AEC_DUMP_DIR", "")


STT_KEEPALIVE = os.environ.get("HAL_STT_KEEPALIVE", "false").lower() == "true"
# Send a KeepAlive every N seconds while pre-connected and idle, so the server doesn't
# idle-close the WS (~10s) and force a slow cold-reconnect at speech start (the cause of
# empty transcripts on short/quiet utterances).
STT_KEEPALIVE_PING_S = float(os.environ.get("HAL_STT_KEEPALIVE_PING_S", "3"))

# Language gate (see lang_id/gate.py): before a live session opens, AmberNet
# classifies the first LANG_GATE_SPEECH_S of speech and drops captures whose
# selected language (`stt_language`) holds less than LANG_GATE_MIN_PROB of the full
# 107-language softmax. off | shadow (log only) | enforce.
_VOICE_RESOURCES = Path(__file__).resolve().parent.parent / "resources"
LANG_ID_MODEL_PATH = Path(os.environ.get("HAL_LANG_ID_MODEL_PATH", str(_VOICE_RESOURCES / "ambernet.onnx")))
LANG_ID_LABELS_PATH = Path(os.environ.get("HAL_LANG_ID_LABELS_PATH", str(_VOICE_RESOURCES / "ambernet.labels.json")))
# Without a local model file, the release asset pinned here (url + sha256) is
# downloaded in the background into LANG_ID_CACHE_DIR. The cache sits beside the
# HAL tree (/opt/.models-hal on a device) because OTA replaces /opt/hal.
LANG_ID_MODEL_MANIFEST = Path(os.environ.get("HAL_LANG_ID_MODEL_MANIFEST", str(_VOICE_RESOURCES / "ambernet.model.json")))
LANG_ID_CACHE_DIR = Path(os.environ.get(
    "HAL_LANG_ID_CACHE_DIR", str(Path(__file__).resolve().parents[4] / ".models-hal"),
))
# Pin identification to these CPU indices (e.g. "0,1,2,3" = four little A55 cores on
# the lamp) so it never competes with the voice pipeline elsewhere; empty = no pin.
LANG_ID_CPUS = [int(c) for c in os.environ.get("HAL_LANG_ID_CPUS", "").split(",") if c.strip()]
# Inference threads; defaults to one per pinned CPU, else 2.
LANG_ID_THREADS = int(os.environ.get("HAL_LANG_ID_THREADS", "") or len(LANG_ID_CPUS) or 2)
LANG_GATE_MODE = os.environ.get("HAL_LANG_GATE", "off").strip().lower()
LANG_GATE_MIN_PROB = float(os.environ.get("HAL_LANG_GATE_MIN_PROB", "0.5"))
# Speech collected before judging; also the classification window.
LANG_GATE_SPEECH_S = float(os.environ.get("HAL_LANG_GATE_SPEECH_S", "2.0"))
# An utterance that ends with less speech than LANG_GATE_SPEECH_S is still judged
# on what it has, down to this minimum (below 0.5 s acceptance collapses).
LANG_GATE_MIN_SPEECH_S = float(os.environ.get("HAL_LANG_GATE_MIN_SPEECH_S", "0.7"))
# Upper bound on how long the gate holds a capture while collecting speech.
LANG_GATE_MAX_WAIT_S = float(os.environ.get("HAL_LANG_GATE_MAX_WAIT_S", "3.5"))
# Captures with less than LANG_GATE_MIN_SPEECH_S of speech: "reject" or "accept".
LANG_GATE_SHORT = os.environ.get("HAL_LANG_GATE_SHORT", "reject").strip().lower()
# Only 16 kHz frames at or above this int16 RMS count as speech (post-AEC uplink:
# silence ~5, speech in the thousands).
LANG_GATE_SPEECH_RMS = float(os.environ.get("HAL_LANG_GATE_SPEECH_RMS", "300"))
# A non-speech gap this long ends the utterance and stops the hold early.
LANG_GATE_UTTERANCE_GAP_S = float(os.environ.get("HAL_LANG_GATE_UTTERANCE_GAP_S", "1.0"))
# Debug: when set, every gate decision writes the scored speech and the whole held
# capture (16 kHz WAVs, verdict and in_set in the name) to this directory.
LANG_GATE_DUMP_DIR = os.environ.get("HAL_LANG_GATE_DUMP_DIR", "")

SPEAKER_PREPASS_JOIN_S = float(os.environ.get("HAL_SPEAKER_PREPASS_JOIN_S", "2.0"))
SPEAKER_PREPASS_COMMIT_JOIN_S = float(os.environ.get("HAL_SPEAKER_PREPASS_COMMIT_JOIN_S", "0.2"))

# How long a resolved speaker identity is reused instead of re-running the recognizer.
# Voices do not change mid-conversation; the cache is what a face identity already gets
# by aging out rather than being re-derived per frame.
SPEAKER_ID_CACHE_S = float(os.environ.get("HAL_SPEAKER_ID_CACHE_S", "90"))
SPEAKER_ID_CACHE_FOLLOWUP_S = float(
    os.environ.get("HAL_SPEAKER_ID_CACHE_FOLLOWUP_S", "300")
)


SPEAKER_MIN_AUDIO_S = _hal_config.SPEAKER_MIN_AUDIO_S


_wake_name = _hal_config.resolve_device_type("friend")
WAKE_WORD_PREFIXES = ("hello", "hey", "hi", "alo", "okay", "ok", "wake up")
# Intern listens for a NAME anywhere in a sentence ("hey intern", "what time is it,
# autonomous"); other devices keep greeting + name phrases at a sentence edge.
WAKE_NAME_ANYWHERE = _wake_name.startswith("intern")
if WAKE_NAME_ANYWHERE:
    # "intern-v2" is the device class, not what people call it.
    DEFAULT_WAKE_WORDS = ["autonomous", "intern"]
else:
    DEFAULT_WAKE_WORDS = [
        *(f"{prefix} autonomous" for prefix in WAKE_WORD_PREFIXES),
        *(f"{prefix} {_wake_name}" for prefix in WAKE_WORD_PREFIXES),
    ]


ENROLL_NUDGE_COOLDOWN_S = float(os.environ.get("HAL_ENROLL_NUDGE_COOLDOWN_S", str(30 * 60)))


ENDPOINT_SILENCE_S = float(os.environ.get("HAL_ENDPOINT_SILENCE_S", "0.8"))


LIVE_MODE = _hal_config.LIVE_MODE

# Linear PCM gain while hardware-AEC live playback is temporarily ducked.
# Keep the demo default until a device-specific value has been measured.
def _live_duck_gain(value):
    try:
        gain = float(value)
    except (TypeError, ValueError):
        return 0.12
    return gain if 0.0 < gain <= 1.0 else 0.12


LIVE_DUCK_GAIN = _live_duck_gain(os.environ.get("HAL_LIVE_DUCK_GAIN", "0.12"))

# What goes on the uplink while our own speaker is playing.
LIVE_UPLINK_DURING_PLAYBACK = os.environ.get(
    "HAL_LIVE_UPLINK_DURING_PLAYBACK", "mute"
).strip().lower()

# How long after the last reference write or observed TTS end the room still counts as
# "playing", including when AEC is unavailable.
LIVE_PLAYBACK_TAIL_S = float(os.environ.get("HAL_LIVE_PLAYBACK_TAIL_S", "0.35"))

# Hang up after this long with no action from the USER, then hand the mic back to the
# VAD, which opens a new session on the next real speech.
LIVE_IDLE_HANGUP_S = float(os.environ.get("HAL_LIVE_IDLE_HANGUP_S", "15"))
# What counts as "the user said something" for that window.
LIVE_IDLE_REQUIRES_TRANSCRIPT = os.environ.get(
    "HAL_LIVE_IDLE_REQUIRES_TRANSCRIPT", "true"
).strip().lower() in ("1", "true", "yes")

LIVE_MAX_UNPROMPTED_REPLIES = int(
    os.environ.get("HAL_LIVE_MAX_UNPROMPTED_REPLIES", "3")
)

LIVE_HANGUP_GRACE_S = float(os.environ.get("HAL_LIVE_HANGUP_GRACE_S", "3"))
LIVE_UPLINK_DUMP_DIR = os.environ.get("HAL_LIVE_UPLINK_DUMP_DIR", "")

# Absolute ceiling on one session, whatever is happening. Backstop against a
# session that never goes idle because the room is noisy.
LIVE_MAX_S = float(os.environ.get("HAL_LIVE_MAX_S", "600"))

# Fast voice (hal.config.FAST_VOICE; default on for Intern v2 only): shorter waits
# between the person stopping and the question reaching Gemini. Each wait stays tunable.
FAST_VOICE = _hal_config.FAST_VOICE
if FAST_VOICE:
    # Smart Turn still runs at this point and holds an unfinished sentence.
    SILENCE_TIMEOUT_S = float(os.environ.get("HAL_FAST_VOICE_SILENCE_S", "0.65"))
    # Commit without waiting on identity; a late speaker is corrected downstream.
    SPEAKER_PREPASS_COMMIT_JOIN_S = float(os.environ.get("HAL_FAST_VOICE_PREPASS_JOIN_S", "0"))
    # The language gate's own end-of-utterance wait never outlasts the silence clock.
    LANG_GATE_UTTERANCE_GAP_S = min(LANG_GATE_UTTERANCE_GAP_S, SILENCE_TIMEOUT_S)
    # Smart Turn calls most Vietnamese questions unfinished; cap that wait (owner setting).
    TURN_END_MAX_PAUSE_S = _hal_config.REALTIME_FAST_VOICE_PAUSE_S
    TURN_END_FALLBACK_S = min(TURN_END_FALLBACK_S, TURN_END_MAX_PAUSE_S)
