"""lamp_play_audio against the SDK's fake lamp: the speaker grant, real time,
the 60 s bound, and a lamp that ends the sound."""

from __future__ import annotations

import asyncio
import struct
import time
import wave
from pathlib import Path

from test_server import connected, run, text


def make_wav(path: Path, seconds: float, rate: int = 16_000, channels: int = 1) -> bytes:
    pcm = b"".join(struct.pack("<h", (n % 50) * 40) for n in range(int(rate * seconds)))
    with wave.open(str(path), "wb") as out:
        out.setnchannels(channels)
        out.setsampwidth(2)
        out.setframerate(rate)
        out.writeframes(pcm * channels)
    return pcm


def test_the_tool_is_offered_only_with_the_speaker_grant(tmp_path):
    async def go():
        async with connected(tmp_path / "a", grants=("body", "light", "speak")) as (_, _, session):
            assert "lamp_play_audio" not in {tool.name for tool in (await session.list_tools()).tools}
        async with connected(tmp_path / "b", grants=("speak", "speaker")) as (_, _, session):
            assert "lamp_play_audio" in {tool.name for tool in (await session.list_tools()).tools}

    run(go())


def test_it_plays_the_wav_in_real_time_and_returns_when_done(tmp_path):
    async def go():
        clip = tmp_path / "clip.wav"
        pcm = make_wav(clip, 0.4)
        async with connected(tmp_path / "home", grants=("speaker",)) as (fake, _, session):
            start = time.monotonic()
            result = await session.call_tool("lamp_play_audio", {"wav_path": str(clip)})
            took = time.monotonic() - start
            assert not result.isError, text(result)
            assert text(result) == "played 0.4 s"
            assert 0.65 <= took < 3.0, took
            assert fake.played_samples() == pcm
            assert [r["on"] for r in fake.asked("speaker.stream")] == [True, False]

    run(go())


def test_a_clip_it_cannot_play_is_refused_before_any_stream(tmp_path):
    async def go():
        async with connected(tmp_path / "home", grants=("speaker",)) as (fake, _, session):
            cases = {
                "long": (tmp_path / "long.wav", dict(seconds=61, rate=16_000), "at most 60 s"),
                "stereo": (tmp_path / "st.wav", dict(seconds=0.1, channels=2), "16-bit mono"),
                "44k": (tmp_path / "k.wav", dict(seconds=0.1, rate=44_100), "16000 or 24000"),
            }
            for name, (path, how, said) in cases.items():
                make_wav(path, **how)
                result = await session.call_tool("lamp_play_audio", {"wav_path": str(path)})
                assert result.isError and said in text(result), (name, text(result))
            missing = await session.call_tool("lamp_play_audio", {"wav_path": str(tmp_path / "none.wav")})
            assert missing.isError and "bad_request" in text(missing)
            assert fake.asked("speaker.stream") == []

    run(go())


def test_the_lamps_refusal_and_early_end_are_the_tools_error(tmp_path):
    async def go():
        clip = tmp_path / "clip.wav"
        make_wav(clip, 1.0)
        async with connected(tmp_path / "home", grants=("speaker",)) as (fake, _, session):
            await fake.set_private(True)
            result = await session.call_tool("lamp_play_audio", {"wav_path": str(clip)})
            assert result.isError and "private" in text(result)
            await fake.set_private(False)
            playing = asyncio.ensure_future(session.call_tool("lamp_play_audio", {"wav_path": str(clip)}))
            while not fake.speaker_frames:
                await asyncio.sleep(0.02)
            await fake.end_speaker("the person is talking")
            result = await playing
            assert result.isError and "the person is talking" in text(result)

    run(go())
