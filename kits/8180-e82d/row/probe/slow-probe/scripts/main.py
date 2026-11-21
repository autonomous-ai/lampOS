import asyncio

from lampos import Lamp


async def main():
    lamp = await Lamp.local()
    await asyncio.sleep(20)
    await lamp.say("The slow probe is done.", wait=True)
    await lamp.close()


asyncio.run(main())
