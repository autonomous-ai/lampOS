"""A curious look of your own, from three poses in degrees: up a little, then a
little left and a little right, and back. The SDK eases it under the body's caps;
a pose the body can't make is refused here, before anything is sent."""

import asyncio

from lampos import Lamp
from lampos.errors import LampError

# Degrees from the standing pose: pitch up, yaw right. The body holds each joint
# to its own bounds (`await lamp.get("body")`).
CURIOUS = [{"pitch": 3}, {"pitch": 2, "yaw": -3}, {"pitch": 2, "yaw": 3}]


async def main():
    async with await Lamp.connect() as lamp:
        try:
            async with lamp.lease("body", why="a curious look"):
                done = await lamp.move_poses(CURIOUS, hold_ms=400, wait=True)
                print(f"looked: {done}")
        except LampError as refused:  # busy, private, not standing...
            print(f"not now: {refused}")


asyncio.run(main())
