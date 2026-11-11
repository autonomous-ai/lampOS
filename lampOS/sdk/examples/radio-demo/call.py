"""Lamp call demo: a Mac calls the lamp, through the lamp SDK.

The lamp rings (a slow "beep-beep" on its own speaker, its ring pulsing green,
its head up), says who is calling and asks for a raised hand. A raised hand
answers: the lamp's microphone goes to the Mac's speakers, and the Mac's
microphone to the lamp's speaker. A hand raised again during the call (a
wave goodbye), or Enter on the Mac, hangs up; the lamp bows and says so.

Needs a lamp-control built for the demo (binary frames to the speaker) and
LAMP_CONTROL_MICROPHONE=on, LAMP_BUS_AUDIO=on on the lamp. Use headphones on
the Mac, or keep it a little away from the lamp: there is no echo canceller,
only a gate that drops the lamp's sound while the Mac is talking.

    python call.py --caller Elvis
"""

from __future__ import annotations

import argparse
import asyncio
import contextlib
import math
import queue
import sys
import time

import numpy as np
import sounddevice as sd
from lampos import Lamp

RATE = 16_000
BLOCK = 320  # 20 ms
RING_SECONDS = 30
MAC_TALKING = 0.04  # RMS: the Mac's side is talking
LAMP_TALKING = 0.03  # RMS: the lamp's side is talking


def _wav_pcm(path: str) -> bytes:
    import wave
    with wave.open(path, "rb") as wav:
        return wav.readframes(wav.getnframes())
ANSWER_HOLD = 0.8  # seconds a hand must be seen to answer
HANGUP_HOLD = 1.2  # and to hang up during the call


def tone(seconds: float, hz=(440.0, 480.0), level=0.25) -> bytes:
    t = np.arange(int(RATE * seconds)) / RATE
    wave = sum(np.sin(2 * np.pi * f * t) for f in hz) / len(hz)
    fade = np.minimum(1.0, np.minimum(t, seconds - t) / 0.02)
    return (wave * fade * level * 32767).astype("<i2").tobytes()


# One ring: beep, gap, beep, then a long pause (as a phone).
BEEP = tone(0.35)
GAP = bytes(int(RATE * 0.2) * 2)


class Hands:
    """A raised hand in the lamp's camera picture (MediaPipe on the Mac)."""

    def __init__(self):
        import cv2
        import mediapipe as mp

        self.cv2 = cv2
        self.hands = mp.solutions.hands.Hands(
            static_image_mode=True, max_num_hands=2, min_detection_confidence=0.5
        )

    def raised(self, jpeg: bytes) -> bool:
        image = self.cv2.imdecode(np.frombuffer(jpeg, np.uint8), self.cv2.IMREAD_COLOR)
        if image is None:
            return False
        found = self.hands.process(self.cv2.cvtColor(image, self.cv2.COLOR_BGR2RGB))
        for hand in found.multi_hand_landmarks or []:
            wrist, middle_tip = hand.landmark[0], hand.landmark[12]
            # Fingers above the wrist: a hand held up, open.
            if middle_tip.y < wrist.y - 0.08:
                return True
        return False


async def hand_held(lamp: Lamp, hands: Hands, hold: float, until: float | None = None) -> bool:
    """True once a raised hand is seen for `hold` seconds; False at `until`."""
    since = None
    while until is None or time.monotonic() < until:
        try:
            jpeg = await lamp.camera_snapshot()
            up = await asyncio.to_thread(hands.raised, jpeg)
        except Exception:
            up = False
        now = time.monotonic()
        if up:
            since = since or now
            if now - since >= hold:
                return True
        else:
            since = None
        await asyncio.sleep(0.25)
    return False


async def stream_tone(lamp: Lamp, pcm: bytes):
    """Sent at its own pace, so the lamp's buffer stays short."""
    for at in range(0, len(pcm), BLOCK * 2):
        await lamp.speaker_write(pcm[at : at + BLOCK * 2])
        await asyncio.sleep(BLOCK / RATE * 0.95)


# The lamp's own lines, in the owner's language (hello's `lamp.language`).
LINES = {
    "en": {
        "calling": "{caller} is calling. Raise your hand to answer.",
        "still": "{caller} is still calling. Raise your hand to answer.",
        "missed": "Missed call from {caller}.",
        "ended": "Call ended. Bye bye!",
    },
    "vi": {
        "calling": "{caller} đang gọi cho bạn. Giơ tay lên để nghe máy nhé.",
        "still": "{caller} vẫn đang gọi. Giơ tay lên để nghe máy nhé.",
        "missed": "Bạn có một cuộc gọi nhỡ từ {caller}.",
        "ended": "Đã kết thúc cuộc gọi. Tạm biệt nhé!",
    },
}


def line(lamp: Lamp, key: str, caller: str) -> str:
    language = (lamp.info.get("language") or "en").split("-")[0].lower()
    return LINES.get(language, LINES["en"])[key].format(caller=caller)


ANNOUNCE_EVERY = 3  # rings: the caller is said again, about every 10 s
SAY_SECONDS = 3.2  # how long the lamp takes to say a short line


async def announce(lamp: Lamp, line: str):
    with contextlib.suppress(Exception):
        await lamp.say(line)
        await asyncio.sleep(SAY_SECONDS)


async def ring(lamp: Lamp, stop: asyncio.Event, caller: str, until: float):
    rings = 0
    said_still = False
    while not stop.is_set():
        with contextlib.suppress(Exception):
            await lamp.light_rgb("#00c853", for_ms=1200, brightness=90)
        await stream_tone(lamp, BEEP + GAP + BEEP)
        rings += 1
        with contextlib.suppress(Exception):
            await lamp.light_rgb("#00c853", for_ms=1500, brightness=15)
        with contextlib.suppress(asyncio.TimeoutError):
            await asyncio.wait_for(stop.wait(), 1.0)
        if stop.is_set():
            break
        left = until - time.monotonic()
        if left < 9 and not said_still:
            said_still = True
            await announce(lamp, line(lamp, "still", caller))
        elif rings % ANNOUNCE_EVERY == 0 and left > 14:
            await announce(lamp, line(lamp, "calling", caller))
        else:
            with contextlib.suppress(asyncio.TimeoutError):
                await asyncio.wait_for(stop.wait(), 1.0)


async def call(lamp: Lamp, caller: str, hands: Hands, max_seconds: float = 600, test: bool = False):
    print(f"Calling the lamp as {caller}...")
    with contextlib.suppress(Exception):
        await lamp.look("ahead")
        await lamp.gesture("curious")
    await announce(lamp, line(lamp, "calling", caller))
    until = time.monotonic() + RING_SECONDS
    stop_ring = asyncio.Event()
    ringing = asyncio.create_task(ring(lamp, stop_ring, caller, until))
    answered = await hand_held(lamp, hands, ANSWER_HOLD, until)
    stop_ring.set()
    await ringing
    if not answered:
        print("No answer.")
        await lamp.say(line(lamp, "missed", caller))
        return
    print("Answered. Talk now; Enter or a raised hand on the lamp hangs up.")
    with contextlib.suppress(Exception):
        await lamp.gesture("nod")
        await lamp.light_rgb("#2979ff", for_ms=30_000, brightness=35)
    await talk(lamp, hands, max_seconds, test)
    print("Call ended.")
    with contextlib.suppress(Exception):
        await lamp.gesture("acknowledge")
    await lamp.say(line(lamp, "ended", caller))


async def talk(lamp: Lamp, hands: Hands, max_seconds: float, test: bool):
    recording = bytearray() if test else None
    loop = asyncio.get_running_loop()
    to_mac: queue.Queue[bytes] = queue.Queue(maxsize=50)
    from_mac: asyncio.Queue[bytes] = asyncio.Queue(maxsize=50)
    mac_talking_until = [0.0]
    lamp_talking_until = [0.0]

    def heard(event: dict):
        # The lamp's microphone: dropped while the Mac talks (its echo).
        if time.monotonic() < mac_talking_until[0]:
            return
        data = event.get("data")
        if data and recording is not None:
            recording.extend(bytes(data))
        if data:
            block = np.frombuffer(bytes(data), dtype="<i2").astype(np.float32) / 32768
            if float(np.sqrt(np.mean(block**2))) > LAMP_TALKING:
                lamp_talking_until[0] = time.monotonic() + 0.3
            with contextlib.suppress(queue.Full):
                to_mac.put_nowait(bytes(data))

    restart = asyncio.Event()

    def ended(event: dict):
        print("microphone ended:", event.get("why"))
        loop.call_soon_threadsafe(restart.set)

    await lamp.on("microphone.audio", heard)
    await lamp.on("microphone.ended", ended)
    await lamp.microphone_stream(True)

    def mic_in(indata, frames, at, status):
        level = float(np.sqrt(np.mean((indata[:, 0].astype(np.float32) / 32768) ** 2)))
        if level > MAC_TALKING:
            mac_talking_until[0] = time.monotonic() + 0.3
        if time.monotonic() < lamp_talking_until[0] and level < 2 * MAC_TALKING:
            return  # half duplex: the lamp's side is talking
        pcm = indata[:, 0].astype("<i2").tobytes()
        loop.call_soon_threadsafe(lambda: from_mac.full() or from_mac.put_nowait(pcm))

    pending = bytearray()

    def speaker_out(outdata, frames, at, status):
        while len(pending) < frames * 2:
            try:
                pending.extend(to_mac.get_nowait())
            except queue.Empty:
                break
        take = min(len(pending), frames * 2)
        out = np.zeros(frames, dtype="<i2")
        out[: take // 2] = np.frombuffer(bytes(pending[:take]), dtype="<i2")
        del pending[:take]
        outdata[:, 0] = out

    hangup = asyncio.Event()
    if sys.stdin.isatty():
        loop.add_reader(sys.stdin, hangup.set)
    loop.call_later(max_seconds, hangup.set)

    async def send():
        while not hangup.is_set():
            pcm = await from_mac.get()
            with contextlib.suppress(Exception):
                await lamp.speaker_write(pcm)

    async def keep_mic():
        while not hangup.is_set():
            await restart.wait()
            restart.clear()
            await asyncio.sleep(0.3)
            with contextlib.suppress(Exception):
                await lamp.microphone_stream(True)

    async def watch_hand():
        await asyncio.sleep(3)  # not the answering hand still up
        if await hand_held(lamp, hands, HANGUP_HOLD):
            hangup.set()

    async def greet():
        # Test: a spoken line from the Mac to the lamp's speaker.
        import subprocess, tempfile, os
        path = tempfile.mktemp(suffix=".wav")
        subprocess.run(["say", "-o", path, "--data-format=LEI16@16000",
                        "Hello! This is Elvis, calling you from my Mac. Can you hear me?"], check=True)
        pcm = open(path, "rb").read()[4096:] if False else _wav_pcm(path)
        os.remove(path)
        await stream_tone(lamp, pcm)

    with sd.InputStream(samplerate=RATE, channels=1, dtype="int16", blocksize=BLOCK, callback=mic_in), \
         sd.OutputStream(samplerate=RATE, channels=1, dtype="int16", blocksize=BLOCK, callback=speaker_out):
        jobs = [keep_mic, watch_hand] + ([greet] if test else [send])
        tasks = [asyncio.create_task(job()) for job in jobs]
        await hangup.wait()
        for task in tasks:
            task.cancel()
    if sys.stdin.isatty():
        loop.remove_reader(sys.stdin)
    if recording is not None:
        import wave
        with wave.open("lamp-mic.wav", "wb") as out:
            out.setnchannels(1); out.setsampwidth(2); out.setframerate(RATE); out.writeframes(bytes(recording))
        print(f"the lamp's microphone, {len(recording) / 2 / RATE:.1f} s: lamp-mic.wav")
    with contextlib.suppress(Exception):
        await lamp.microphone_stream(False)


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--caller", default="Elvis")
    parser.add_argument("--lamp", default=None, help="the lamp's id, when more than one is paired")
    parser.add_argument("--host", default=None, help="the lamp's address (127.0.0.1 through an SSH tunnel)")
    parser.add_argument("--now", action="store_true", help="call at once, without waiting for Enter")
    parser.add_argument("--max-seconds", type=float, default=600)
    parser.add_argument("--test", action="store_true", help="the Mac says a line; the lamp's microphone is saved")
    args = parser.parse_args()
    hands = Hands()
    async with await Lamp.connect(args.lamp, host=args.host) as lamp:
        print("Connected:", lamp.info.get("name"), "grants:", sorted(lamp.grants))
        if not args.now:
            input("Press Enter to call the lamp...")
        async with lamp.lease("body", "light", for_ms=60_000, why="a call"):
            await call(lamp, args.caller, hands, args.max_seconds, args.test)


if __name__ == "__main__":
    asyncio.run(main())
