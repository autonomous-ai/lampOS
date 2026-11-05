"""First program: read what the lamp is doing, light it warm, say hello, nod."""

import asyncio

from lampos import Lamp
from lampos.errors import LampError


async def main():
    async with await Lamp.connect() as lamp:
        print("connected to", lamp.info.get("id"), "with", lamp.grants)
        print("lamp state:", await lamp.get("lamp"))
        try:
            await lamp.light("warm", 3, 5_000, pattern="breathe")
            await lamp.say("Hello from my computer.")
            await lamp.gesture("nod")
        except LampError as refused:  # private, paused, asleep, busy, quiet_hours...
            print("not now:", refused)
        await asyncio.sleep(1)  # stay connected a moment: closing hands the light back


asyncio.run(main())
