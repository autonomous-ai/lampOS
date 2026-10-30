"""Stuffy air: the lamp's light turns a soft orange while the air is stuffy, and
the program says what its sensor and its own computer read.

Needs the `air`, `device` and `light` grants. A lamp with no air sensor answers
`known: False`, and this says so and waits for one.
"""

import asyncio

from lampos import Lamp
from lampos.errors import LampError


async def main():
    async with await Lamp.connect() as lamp:
        device = await lamp.device()
        if device["known"]:
            hottest = max(device["temperature_c"].values(), default=None)
            print(f"lamp: load {device['cpu_load']:.2f}, memory {device['memory_used']:.0%}, hottest {hottest} °C")
        air = await lamp.air()
        print("air:", air if air["known"] else "no reading (no sensor, or not yet)")

        async def changed(event):
            # The lamp tells the air by change: a value past its step, at most every 5 s.
            print(f"air: {event.get('co2_ppm', '?')} ppm CO2, {event.get('pm2_5', '?')} µg/m³ PM2.5, {event['feel']}")
            if event["feel"] in ("stuffy", "very_stuffy"):
                try:
                    await lamp.light("orange", 2, 30_000, pattern="breathe")
                except LampError as refused:
                    print(f"not now: {refused}")  # private, paused, asleep...

        await lamp.on("air.changed", changed)
        await lamp.wait_closed()


asyncio.run(main())
