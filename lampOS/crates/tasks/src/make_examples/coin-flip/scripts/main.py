import asyncio
import random

from lampos import Lamp


async def main():
    lamp = await Lamp.local()
    await lamp.say(random.choice(["Heads.", "Tails."]))
    await lamp.close()


asyncio.run(main())
