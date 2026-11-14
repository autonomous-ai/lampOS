"""The speaker stream (`speaker.stream`, the `speaker` grant): the SDK's
`speaker_stream`, the fake lamp's own, and `lamp speaker`. The fake follows
lamp-control's speaker.rs; nothing here stands in for a lamp check."""

from __future__ import annotations

import argparse
import asyncio
import json
import math
import struct
import time
import wave
from pathlib import Path

import pytest

from lampos import Keyring, Lamp, LampError, debug
from lampos.errors import NoGrantError, PrivateError
from lampos.testing import FakeLamp


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, 30))


async def connected(fake: FakeLamp, folder: Path) -> Lamp:
    keyring = Keyring(folder)
    await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, keyring=keyring)
    return await Lamp.connect(keyring=keyring)


def pcm_of(samples: int, rate: int = 16_000, hz: float = 440.0) -> bytes:
    return b"".join(struct.pack("<h", int(2000 * math.sin(2 * math.pi * hz * n / rate))) for n in range(samples))


def wav(path: Path, pcm: bytes, rate: int = 16_000, channels: int = 1) -> Path:
    with wave.open(str(path), "wb") as out:
        out.setnchannels(channels)
        out.setsampwidth(2)
        out.setframerate(rate)
        out.writeframes(pcm)
    return path


async def until(check, within=5.0):
    deadline = asyncio.get_running_loop().time() + within
    while not check():
        assert asyncio.get_running_loop().time() < deadline, "never happened"
        await asyncio.sleep(0.01)


def test_a_stream_opens_with_its_answer_and_ends_on_leaving(tmp_path):
    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            async with await connected(fake, tmp_path) as lamp:
                async with lamp.speaker_stream() as out:
                    assert fake.speaker is not None
                    assert (out.sample_rate, out.turn) == (16_000, 1)
                    assert out.info["format"] == "s16le" and out.info["source"] == "speaker"
                    assert out.info["frame_ms"] == [10, 100] and out.info["buffer_ms"] == 300
                assert fake.speaker is None
                assert fake.asked("speaker.stream") == [
                    {"on": True, "sample_rate": 16_000}, {"on": False},
                ]
                async with lamp.speaker_stream(24_000) as out:
                    assert out.sample_rate == 24_000 and out.turn == 2
                with pytest.raises(LampError) as bad:
                    async with lamp.speaker_stream(8_000):
                        pass
                assert bad.value.code == "bad_request"
                # The plain request, as the microphone's.
                assert (await lamp.speaker_stream_request(True))["turn"] == 3
                assert (await lamp.speaker_stream_request(False))["stream"] == "off"

    run(go())


def test_frames_carry_a_header_a_running_sequence_and_at_most_100_ms(tmp_path):
    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            async with await connected(fake, tmp_path) as lamp:
                pcm = pcm_of(5_000)  # 10,000 bytes
                async with lamp.speaker_stream() as out:
                    await out.speaker_write(pcm)
                    # An odd last byte waits for the next write's first.
                    await out.speaker_write(pcm[:3])
                    await out.speaker_write(pcm[3:4])
                    await until(lambda: len(fake.speaker_frames) == 6)
                    assert [seq for seq, _ in fake.speaker_frames] == [0, 1, 2, 3, 4, 5]
                    sizes = [size for _, size in fake.speaker_frames]
                    assert sizes == [3_200, 3_200, 3_200, 400, 2, 2]
                    assert max(sizes) <= 9_600
                    assert fake.played_samples() == pcm + pcm[:4]
                    assert fake.speaker_stray_total() == 0
                    assert out.sequence == 6

    run(go())


def test_the_header_is_the_lamps_four_byte_length_then_json(tmp_path):
    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            async with await connected(fake, tmp_path) as lamp:
                sent = []
                original = lamp._ws.send

                async def spy(message):
                    sent.append(message)
                    await original(message)

                lamp._ws.send = spy
                async with lamp.speaker_stream() as out:
                    await out.speaker_write(pcm_of(320))
                binary = [m for m in sent if isinstance(m, bytes)]
                assert len(binary) == 1
                length = int.from_bytes(binary[0][:4], "big")
                assert json.loads(binary[0][4 : 4 + length]) == {"ev": "speaker.audio", "sequence": 0}
                assert len(binary[0]) - 4 - length == 640

    run(go())


def test_stray_frames_are_counted_never_answered_and_never_keep_a_stream_alive(tmp_path):
    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            fake.speaker_stats_every = 0.1
            async with await connected(fake, tmp_path) as lamp:
                # No stream: dropped, no error, nothing counted.
                header = json.dumps({"ev": "speaker.audio", "sequence": 0}).encode()
                await lamp._ws.send(len(header).to_bytes(4, "big") + header + pcm_of(100))
                async with lamp.speaker_stream() as out:
                    def frame(samples: bytes) -> bytes:
                        return len(header).to_bytes(4, "big") + header + samples

                    for bad in (b"", b"\x01", b"\x00" * 3, b"\x00" * 9_602):
                        await lamp._ws.send(frame(bad))
                    await lamp._ws.send(b"junk")  # no header at all
                    await lamp._ws.send(frame(b"\x00" * 9_600))  # the most: taken
                    await until(lambda: out.stats is not None and out.stats["stray"] == 5)
                    assert out.stats["turn"] == out.turn
                    assert set(out.stats) >= {"buffered_ms", "lost", "underruns", "limited", "stray"}
                    assert len(fake.played_samples()) == 9_600
                    assert out.ended is None

    run(go())


def test_junk_alone_does_not_hold_a_stream_open(tmp_path):
    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            fake.speaker_idle_after = 0.3
            async with await connected(fake, tmp_path) as lamp:
                async with lamp.speaker_stream() as out:
                    deadline = asyncio.get_running_loop().time() + 5
                    while out.ended is None:
                        assert asyncio.get_running_loop().time() < deadline
                        await lamp._ws.send(b"\x00\x00\x00\x00")  # empty header, no samples
                        await asyncio.sleep(0.05)
                    assert out.ended == "idle"
                    with pytest.raises(LampError) as closed:
                        await out.speaker_write(pcm_of(10))
                    assert closed.value.code == "closed" and "idle" in closed.value.why

    run(go())


def test_without_the_grant_it_is_refused_in_the_lamps_words(tmp_path):
    async def go():
        async with FakeLamp(grants=("body", "camera", "microphone")) as fake:
            async with await connected(fake, tmp_path) as lamp:
                with pytest.raises(NoGrantError) as error:
                    async with lamp.speaker_stream():
                        pytest.fail("opened")
                assert error.value.why == "not granted: speaker"
                assert fake.speaker is None
                # Never in the grants a fake gets by default.
        async with FakeLamp() as fake:
            assert "speaker" not in fake.grants

    run(go())


def test_private_ends_the_stream_with_its_turn_and_refuses_a_new_one(tmp_path):
    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            async with await connected(fake, tmp_path) as lamp:
                async with lamp.speaker_stream() as out:
                    await fake.set_private(True)
                    await until(lambda: out.ended is not None)
                    assert out.ended == "the lamp is private"
                    with pytest.raises(LampError):
                        await out.speaker_write(pcm_of(10))
                with pytest.raises(PrivateError) as refused:
                    async with lamp.speaker_stream():
                        pytest.fail("opened")
                assert refused.value.why == "the lamp is private"
                await fake.set_private(False)
                # Muted: the volume at 0.
                async with lamp.speaker_stream() as out:
                    await fake.set_volume(0)
                    await until(lambda: out.ended is not None)
                    assert out.ended == "the lamp is muted"
                with pytest.raises(LampError) as muted:
                    async with lamp.speaker_stream():
                        pytest.fail("opened")
                assert muted.value.why == "the lamp is muted"

    run(go())


def test_a_stream_that_replaces_another_is_told_by_turn_and_an_old_end_is_not_the_new_ones(tmp_path):
    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            async with await connected(fake, tmp_path) as lamp:
                async with lamp.speaker_stream() as first:
                    async with lamp.speaker_stream() as second:
                        await until(lambda: first.ended is not None)
                        assert first.ended == "replaced"
                        assert second.turn == first.turn + 1
                        assert second.ended is None
                        # An end of some other turn is not this stream's.
                        second._event({"ev": "speaker.ended", "turn": 99, "why": "limit"})
                        second._event({"ev": "speaker.stats", "turn": 99})
                        assert second.ended is None and second.stats is None
                        await fake.end_speaker("limit")
                        await until(lambda: second.ended is not None)
                        assert second.ended == "limit"
                # Leaving a block of an ended stream asks nothing more.
                assert fake.asked("speaker.stream")[-1]["on"] is True

    run(go())


def test_another_computers_stream_replaces_it_and_its_frames_are_stray(tmp_path):
    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            async with await connected(fake, tmp_path / "a") as one, await connected(fake, tmp_path / "b") as two:
                async with one.speaker_stream() as first:
                    async with two.speaker_stream() as second:
                        await until(lambda: first.ended == "replaced")
                        # The old connection's frames are not the stream's: stray.
                        header = json.dumps({"ev": "speaker.audio", "sequence": 0}).encode()
                        await one._ws.send(len(header).to_bytes(4, "big") + header + pcm_of(100))
                        await second.speaker_write(pcm_of(100))
                        await until(lambda: len(fake.played_samples()) == 200)
                        assert fake.speaker_stray_total() == 1

    run(go())


def test_the_idle_end_and_stats_come_as_the_lamp_says_them(tmp_path):
    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            fake.speaker_idle_after = 0.4
            fake.speaker_stats_every = 0.1
            async with await connected(fake, tmp_path) as lamp:
                async with lamp.speaker_stream() as out:
                    await until(lambda: out.stats is not None)
                    await until(lambda: out.ended == "idle")
                    # Ended by the lamp: leaving sends no `off`.
                assert fake.asked("speaker.stream") == [{"on": True, "sample_rate": 16_000}]

    run(go())


def test_play_wav_writes_in_real_time_and_the_whole_clip_arrives(tmp_path):
    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            async with await connected(fake, tmp_path) as lamp:
                pcm = pcm_of(8_000)  # 0.5 s
                clip = wav(tmp_path / "half.wav", pcm)
                async with lamp.speaker_stream() as out:
                    start = time.monotonic()
                    await out.play_wav(clip)
                    took = time.monotonic() - start
                    # 0.5 s of sound and the lamp's 0.3 s buffer, not written at once.
                    assert 0.75 <= took < 3.0, took
                    assert fake.played_samples() == pcm
                    # Frames of 20 ms, as paced.
                    assert {size for _, size in fake.speaker_frames} == {640}
                # 24 kHz needs its own stream.
                clip24 = wav(tmp_path / "c24.wav", pcm_of(2_400, 24_000), 24_000)
                async with lamp.speaker_stream() as out:
                    with pytest.raises(LampError) as other:
                        await out.play_wav(clip24)
                    assert other.value.code == "bad_request" and "sample_rate=24000" in other.value.why
                async with lamp.speaker_stream(24_000) as out:
                    await out.play_wav(clip24)
                assert len(fake.played_samples(None)) == len(pcm) + 4_800

    run(go())


def test_play_wav_says_clearly_what_it_cannot_play(tmp_path):
    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            async with await connected(fake, tmp_path) as lamp:
                async with lamp.speaker_stream() as out:
                    cases = {
                        "stereo": wav(tmp_path / "s.wav", pcm_of(320) * 2, channels=2),
                        "44k": wav(tmp_path / "k.wav", pcm_of(441, 44_100), 44_100),
                        "text": tmp_path / "notes.txt",
                    }
                    cases["text"].write_text("not a wav")
                    for name, path in cases.items():
                        with pytest.raises(LampError) as error:
                            await out.play_wav(path)
                        assert error.value.code == "bad_request", name
                    assert "16000 or 24000" in (await _why(out, cases["44k"]))
                    assert fake.played_samples() == b""

    async def _why(out, path):
        try:
            await out.play_wav(path)
        except LampError as error:
            return error.why

    run(go())


def test_the_debug_log_shows_a_streams_numbers_and_never_the_sound():
    for name in ("buffer_ms", "buffered_ms", "frame_ms", "limited", "stray", "underruns", "turn", "sequence"):
        assert name in debug.SHOWN, name
    stats = {"ev": "speaker.stats", "turn": 2, "buffered_ms": 120, "lost": 0, "underruns": 1, "limited": 0, "stray": 3}
    assert debug.redact(stats) == stats
    assert "<not shown>" not in json.dumps(debug.redact({"re": 1, "ok": True, "stream": "on", "frame_ms": [10, 100], "buffer_ms": 300}))


def test_lamp_speaker_plays_a_wav_and_stdin_through_the_stream(tmp_path, monkeypatch):
    from lampos import cli

    monkeypatch.setenv("LAMPOS_HOME", str(tmp_path / "home"))

    async def go():
        async with FakeLamp(grants=("speaker",)) as fake:
            await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, keyring=Keyring())
            pcm = pcm_of(3_200)  # 0.2 s
            clip = wav(tmp_path / "a.wav", pcm)
            assert await cli._speaker(argparse.Namespace(file=str(clip), lamp=None, rate=None)) == 0
            assert fake.played_samples() == pcm
            fake.speaker_played_all.clear()
            # 24 kHz raw samples from stdin.
            raw = pcm_of(2_400, 24_000)

            class Stdin:
                buffer = type("B", (), {"read": staticmethod(lambda: raw)})

            monkeypatch.setattr(cli.sys, "stdin", Stdin)
            assert await cli._speaker(argparse.Namespace(file="-", lamp=None, rate=24_000)) == 0
            assert fake.played_samples() == raw
            assert fake.asked("speaker.stream")[-2]["sample_rate"] == 24_000
            # A WAV at a rate other than the one named, and one the lamp takes not.
            assert await cli._speaker(argparse.Namespace(file=str(clip), lamp=None, rate=24_000)) == 2
            odd = wav(tmp_path / "o.wav", pcm_of(441, 44_100), 44_100)
            assert await cli._speaker(argparse.Namespace(file=str(odd), lamp=None, rate=None)) == 2

    run(go())
    args = cli.parser().parse_args(["speaker", "x.wav", "--rate", "24000"])
    assert (args.file, args.rate, args.lamp) == ("x.wav", 24_000, None)
