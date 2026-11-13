"""The camera: save its newest picture, then watch a short stream and save
the last frame. Needs a pairing with the `camera` grant; the lamp refuses
both while it is private or its camera is closed.

    python examples/camera.py [seconds]    # 3 by default
"""

import asyncio
import base64
import sys
from pathlib import Path

from lampos import Lamp
from lampos.errors import LampError


def jpeg_of(frame: dict) -> bytes:
    # A fast stream's frame carries the JPEG as `data` (bytes); otherwise
    # it is the eyes' newest frame, as `jpeg` in base64.
    return frame["data"] if "data" in frame else base64.b64decode(frame["jpeg"])


async def main(seconds: float):
    async with await Lamp.connect() as lamp:
        try:
            picture = await lamp.camera_snapshot()
        except LampError as refused:  # no `camera` grant, private, camera closed...
            print(f"snapshot: refused, {refused}")
            return 1
        Path("snapshot.jpg").write_bytes(picture)
        print(f"snapshot: {len(picture)} bytes, saved as snapshot.jpg")

        frames: list[dict] = []
        ended = asyncio.Event()
        await lamp.on("camera.frame", frames.append)
        await lamp.on("camera.ended", lambda event: ended.set())
        try:
            answer = await lamp.camera_stream(fps=5)
        except LampError as refused:
            print(f"stream: refused, {refused}")
            return 1
        print(f"stream: on, fast={answer.get('fast', False)}")
        try:
            await asyncio.wait_for(ended.wait(), seconds)
            print("stream: the lamp ended it (private, or the camera closed)")
        except asyncio.TimeoutError:
            await lamp.camera_stream(False)
        if not frames:
            print("stream: no frame came")
            return 1
        Path("last-frame.jpg").write_bytes(jpeg_of(frames[-1]))
        print(f"stream: {len(frames)} frames in {seconds:g} s, the last saved as last-frame.jpg")
        return 0


sys.exit(asyncio.run(main(float(sys.argv[1]) if len(sys.argv) > 1 else 3)))
