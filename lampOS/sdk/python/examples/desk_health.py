"""Desk health: keeps count of how long the air at the desk was stuffy today, and
asks for a window once, not every few seconds.

When the air turns stuffy this waits two minutes (a breath of a room is not a
problem), then glows a soft orange and says one short line; when the air clears
it puts its own light out with `light_off` and says nothing. The minutes of
stuffy air are kept in a note per day, so a restart does not forget them.

Needs the `air`, `light`, `speak` and `notes` grants. Try it on the virtual lamp:
`lamp-sim air co2_ppm=1500`, then `lamp-sim air co2_ppm=600`.
"""

import asyncio
import os
import time
from datetime import datetime

from lampos import Lamp
from lampos.errors import LampError

STUFFY = ("stuffy", "very_stuffy")
WAIT_S = float(os.environ.get("DESK_STUFFY_AFTER_S", 120))  # stuffy this long before a word
RENEW_S = 25  # a light is held 30 s at most, so it is sent again before that


def today() -> str:
    """The day key, by this program's own clock: an event's `at` is UTC."""
    return datetime.now().strftime("%Y-%m-%d")


async def main():
    async with await Lamp.connect() as lamp:
        since = None  # when the air turned stuffy (monotonic), None while it is not
        nudged = False  # said once per stuffy spell
        glowing = False
        backoff = False  # the last light was refused: wait a renew, not 0.1 s
        wake = asyncio.Event()

        async def keep(seconds: float):
            """Add a spell of stuffy air to today's note."""
            key = f"stuffy-{today()}"
            total = await lamp.notes_get(key, 0)
            await lamp.notes_set(key, round(total + seconds / 60, 1))
            print(f"today the air was stuffy {round(total + seconds / 60, 1)} min")

        async def changed(event):
            nonlocal since
            if event["feel"] in STUFFY:
                if since is None:
                    since = time.monotonic()
                    print(f"air: {event.get('co2_ppm', '?')} ppm, stuffy, waiting {WAIT_S:.0f} s")
            elif since is not None:
                spell = time.monotonic() - since
                since = None
                try:
                    await keep(spell)
                except LampError as refused:
                    print(f"not now: {refused}")  # notes paused, over its limits...
            wake.set()  # an event wakes the loop, so a clear air puts the light out at once

        await lamp.on("air.changed", changed)
        print("watching the air; Ctrl-C to stop")
        while True:
            # Wake for an event, for the end of the wait, or to renew the light.
            left = WAIT_S - (time.monotonic() - since) if since is not None else RENEW_S
            if backoff:
                left, backoff = RENEW_S, False
            try:
                await asyncio.wait_for(wake.wait(), timeout=max(0.1, min(RENEW_S, left)))
            except asyncio.TimeoutError:
                pass
            wake.clear()
            try:
                if since is not None and time.monotonic() - since >= WAIT_S:
                    if not nudged:
                        nudged = True
                        await lamp.say("It is getting stuffy in here. A window would help.")
                    await lamp.light("orange", 2, 30_000, pattern="breathe")
                    glowing = True
                elif since is None and glowing:
                    await lamp.light_off()  # only the light this program put on
                    glowing = False
                    nudged = False
            except LampError as refused:
                print(f"not now: {refused}")  # private, paused, asleep...
                backoff = True


try:
    asyncio.run(main())
except KeyboardInterrupt:
    pass
