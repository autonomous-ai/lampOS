import asyncio

from lampos import Lamp


async def main():
    lamp = await Lamp.local()
    await lamp.say("The rest probe is done.", wait=True)
    await lamp.close()


asyncio.run(main())
