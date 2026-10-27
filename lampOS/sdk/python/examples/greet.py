"""Greet a person who arrives, at most once a minute."""

import asyncio
import time

from lampos import Lamp
from lampos.errors import LampError


async def main():
    async with await Lamp.connect() as lamp:
        last = 0.0

        async def arrived(event):
            nonlocal last
            if time.monotonic() - last < 60:
                return
            last = time.monotonic()
            try:
                await lamp.gesture("greeting")
            except LampError as refused:  # private, paused, asleep, rate...
                print(f"not now: {refused}")

        await lamp.on("person.arrived", arrived)
        await lamp.wait_closed()


asyncio.run(main())
