"""Phonon holds a sentence back until it has a head start, so playback never runs dry early."""

import threading

from hal.drivers.voice.tts import phonon
from hal.drivers.voice.tts.phonon import PhononTTSBackend

RATE = 24000


def _pcm(ms: int) -> bytes:
    return b"\x00\x00" * (RATE * ms // 1000)


class FakeWorker:
    """Yields (arrival time, PCM) like _Worker.synth and records what was consumed."""

    sample_rate = RATE
    voices = ["Freya"]

    def __init__(self, chunks):
        self._chunks = chunks
        self.consumed = 0
        self.cancelled = None

    def synth(self, text, voice, cancelled=None):
        self.cancelled = cancelled
        for arrived, ms in self._chunks:
            self.consumed += 1
            yield arrived, _pcm(ms)


def _backend(worker, prebuffer_s=1.2):
    backend = object.__new__(PhononTTSBackend)
    backend._model_dir = "/nonexistent"
    backend._worker = worker
    backend._prebuffer_s = prebuffer_s
    backend._warned_speed = False
    return backend


def _stream(backend, monkeypatch, **kwargs):
    monkeypatch.setattr(PhononTTSBackend, "available", property(lambda self: True))
    return backend.stream_pcm("Hello there.", "Freya", "", 1.0, **kwargs)


def test_first_chunk_waits_for_the_head_start(monkeypatch):
    # Timeline measured on a lamp: small slow chunks first, then large fast ones.
    chunks = [(0.42, 80), (0.55, 80), (0.64, 80), (0.83, 80), (1.16, 240), (1.70, 560), (2.65, 1040)]
    worker = FakeWorker(chunks)
    out = _stream(_backend(worker), monkeypatch)

    first = next(out)

    # Nothing plays until a chunk arrives 1.2 s after the first (1.70 s - 0.42 s).
    assert worker.consumed == 6
    assert first == _pcm(80)
    rest = list(out)
    assert b"".join([first] + rest) == b"".join(_pcm(ms) for _, ms in chunks)


def test_short_sentence_plays_whole_after_it_finishes(monkeypatch):
    chunks = [(0.30, 80), (0.40, 80), (0.55, 240)]
    worker = FakeWorker(chunks)

    out = list(_stream(_backend(worker), monkeypatch))

    assert worker.consumed == 3
    assert b"".join(out) == b"".join(_pcm(ms) for _, ms in chunks)


def test_zero_head_start_streams_at_once(monkeypatch):
    chunks = [(0.42, 80), (0.55, 80), (0.64, 80)]
    worker = FakeWorker(chunks)
    out = _stream(_backend(worker, prebuffer_s=0.0), monkeypatch)

    next(out)

    assert worker.consumed == 1


def test_cancel_reaches_the_worker(monkeypatch):
    worker = FakeWorker([(0.1, 80)])
    stop = threading.Event()

    list(_stream(_backend(worker), monkeypatch, cancelled=stop.is_set))

    assert worker.cancelled is not None
    assert worker.cancelled() is False
    stop.set()
    assert worker.cancelled() is True
    assert PhononTTSBackend.supports_synthesis_cancellation is True


def test_prebuffer_env_default_is_200_ms():
    assert phonon.PHONON_PREBUFFER_S == 0.2


def test_worker_pinned_to_a76_cores_by_default():
    worker = phonon._Worker("/m", "en", "q8", phonon.PHONON_THREADS, phonon.PHONON_CPUS)

    assert worker._argv[-2:] == ["--cpus", "6-7"]
    assert phonon.PHONON_THREADS == 1


def test_worker_parses_cpu_lists():
    from hal.drivers.voice.tts import phonon_worker

    assert phonon_worker._parse_cpus("6-7") == {6, 7}
    assert phonon_worker._parse_cpus("0, 6-7") == {0, 6, 7}
    assert phonon_worker._parse_cpus("x,3") == {3}
    assert phonon_worker._parse_cpus("") == set()


def test_every_audio_tag_is_stripped():
    strip = phonon._strip_audio_tags

    assert strip("[curious, warm] I see you, chin on hand.") == "I see you, chin on hand."
    assert strip("Oh! [laughs softly] That tickles. [pause] Again?") == "Oh! That tickles. Again?"
    assert strip("[dramatic tone]") == ""
    assert strip("No tags here.") == "No tags here."
