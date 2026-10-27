"""A build's result on the ring: green breathing when it passes, a red pulse
when it fails. Run it at the end of a CI job on your own network:

    python ci_light.py pass
"""

import asyncio
import sys

from lampos import Lamp


async def main(result: str):
    async with await Lamp.connect() as lamp:
        if result == "pass":
            await lamp.light("green", 3, 20_000, pattern="breathe")
        else:
            await lamp.light("red", 3, 20_000, pattern="pulse")
            await lamp.say("The build failed.")


asyncio.run(main(sys.argv[1] if len(sys.argv) > 1 else "pass"))
