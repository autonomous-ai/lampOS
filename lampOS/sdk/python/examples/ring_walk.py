"""The ring pixel by pixel, as a check you can watch (or film): pixel 0 white,
then one green pixel walking all the way around, then the whole ring blue at
half brightness, then the ring given back.

    python ring_walk.py            # 2 pixels a second
    python ring_walk.py 4          # 4 a second

It shows where pixel 0 is and which way the ring counts. Each step is a new
`light`, which replaces the last under the same claim; the person's own light,
a turn's face and the privacy marks stay over it.
"""

import asyncio
import sys

from lampos import Lamp


async def main(per_second: float):
    async with await Lamp.connect() as lamp:
        count = lamp.ring_pixels
        if count is None:
            sys.exit("this lamp's lamp-control draws no pixels yet (no ring in hello's limits)")
        dark = ["#000000"] * count

        first = list(dark)
        # White, not red: the camera mark's pixel never shows a computer's red.
        first[0] = "#ffffff"
        print("pixel 0 white, 3 s")
        await lamp.ring(first, for_ms=4_000)
        await asyncio.sleep(3)

        print(f"one green pixel walking around, {per_second:g} a second")
        for at in range(count):
            step = list(dark)
            step[at] = "#00ff00"
            await lamp.ring(step, for_ms=2_000)
            await asyncio.sleep(1 / per_second)

        print("all blue at brightness 50, 3 s")
        await lamp.light_rgb("#0000ff", brightness=50, for_ms=4_000)
        await asyncio.sleep(3)

        print("off: the ring given back")
        await lamp.stop()


asyncio.run(main(float(sys.argv[1]) if len(sys.argv) > 1 else 2.0))
