"""A Pomodoro on the lamp: 25 minutes of focus under a steady warm light, then
a word and a stretch, then 5 minutes of rest. Ctrl-C ends it.

The light is renewed every 25 s (a client's light lasts 30 s at most), so if
this stops, the lamp's own light comes back by itself.
"""

import asyncio

from lampos import Lamp
from lampos.errors import LampError

FOCUS_S, REST_S = 25 * 60, 5 * 60


async def hold_light(lamp, colour, seconds):
    for _ in range(int(seconds // 25)):
        await lamp.light(colour, 2, 30_000)
        await asyncio.sleep(25)


async def main():
    async with await Lamp.connect() as lamp:
        while True:
            await hold_light(lamp, "warm", FOCUS_S)
            try:
                await lamp.say("Time for a short break.")
                await lamp.gesture("stretching")
            except LampError as refused:  # quiet hours, private, someone talking...
                print(f"break, quietly: {refused}")
            await hold_light(lamp, "green", REST_S)


asyncio.run(main())
