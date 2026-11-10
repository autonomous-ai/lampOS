"""Answer a tap on the lamp's head with a warm light for a moment, and a pet
with a glad feeling. `touch` needs no grant; the light needs `light`, the
feeling `body` and `light`:

    python touch.py
"""

import asyncio

from lampos import Lamp
from lampos.errors import LampError


async def main():
    async with await Lamp.connect() as lamp:

        async def touched(event):
            try:
                if event["kind"] == "tap":
                    await lamp.light("warm", 3, 3_000)
                elif event["kind"] == "pet":
                    await lamp.express("glad")
                print(f"{event['kind']}")
            except LampError as refused:  # private, paused, asleep, rate...
                print(f"not now: {refused}")

        await lamp.on("touch", touched)
        await lamp.wait_closed()


asyncio.run(main())
