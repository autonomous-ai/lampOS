"""A posture nudge: when someone has sat at the desk for a long stretch, the lamp
makes one slow small tilt of its head, once, and says nothing.

The stretch starts at an arrival and ends when the person leaves; after
`SIT_AFTER_S` seconds (default 50 minutes) of it, the lamp tilts and counts the
nudge in today's note (`posture-<day>`, by this program's own clock). One nudge
per stretch: a lamp that fidgets every few minutes is a metronome. A refused
tilt (the lamp is busy with a conversation, private, paused) is simply let go.

Needs the `look` (arrivals), `body` (the tilt) and `notes` grants. Try it on the
virtual lamp: `SIT_AFTER_S=20 python posture.py`, then `lamp-sim people 0 DIR`,
wait about 5 s, `lamp-sim people 1 DIR` (an arrival), and wait 20 s.
"""

import asyncio
import os
from datetime import datetime

from lampos import Lamp
from lampos.errors import LampError

SIT_AFTER_S = float(os.environ.get("SIT_AFTER_S", 50 * 60))
# Degrees from the standing pose: a small slow tip of the head and back.
TILT = [{"roll": 3, "pitch": 1}, {"roll": -2}]


def today() -> str:
    """The day key by this program's own clock: an event's `at` is UTC."""
    return datetime.now().strftime("%Y-%m-%d")


async def main():
    async with await Lamp.connect() as lamp:
        sitting = None  # the timer for this stretch, None while nobody is there

        async def nudge():
            await asyncio.sleep(SIT_AFTER_S)
            try:
                async with lamp.lease("body", why="a posture nudge"):
                    await lamp.move_poses(TILT, hold_ms=600, wait=True)
                key = f"posture-{today()}"
                count = await lamp.notes_get(key, 0) + 1
                await lamp.notes_set(key, count)
                print(f"nudge {count} today")
            except LampError as refused:
                print(f"not now: {refused}")  # busy, private, paused, not standing...

        async def arrived(event):
            nonlocal sitting
            if sitting is None or sitting.done():
                sitting = asyncio.ensure_future(nudge())

        async def left(event):
            nonlocal sitting
            if sitting is not None:
                sitting.cancel()
                sitting = None

        await lamp.on("person.arrived", arrived)
        await lamp.on("person.left", left)
        print(f"waiting for a stretch of {SIT_AFTER_S / 60:.0f} min; Ctrl-C to stop")
        await lamp.wait_closed()


try:
    asyncio.run(main())
except KeyboardInterrupt:
    pass
