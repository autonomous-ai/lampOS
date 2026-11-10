"""Watch the joints' temperatures (`body.state`, about 12 a second) and tell
when one runs hot: a pulsing orange light and a line on the screen, once, and
again only after every joint has cooled. `body.state` needs the `body` grant,
the light needs `light`:

    python heat_watch.py [limit in degrees C, default 60]
"""

import asyncio
import sys

from lampos import Lamp
from lampos.errors import LampError


async def main(limit: float):
    async with await Lamp.connect() as lamp:
        warned = False

        async def state(event):
            nonlocal warned
            hot = {name: joint["temp_c"] for name, joint in event["joints"].items() if (joint.get("temp_c") or 0) >= limit}
            if hot and not warned:
                warned = True
                worst = max(hot, key=hot.get)
                print(f"hot: {worst} {hot[worst]} C", flush=True)
                try:
                    await lamp.light("orange", 2, 5_000, pattern="pulse")
                except LampError as refused:  # private, paused, asleep, rate...
                    print(f"not now: {refused}", flush=True)
            elif not hot and warned:
                warned = False
                print("cool again", flush=True)

        await lamp.on("body.state", state)
        await lamp.wait_closed()


asyncio.run(main(float(sys.argv[1]) if len(sys.argv) > 1 else 60.0))
