import asyncio

from lampos import Lamp


async def main():
    lamp = await Lamp.local()
    await lamp.say("Let's take three slow breaths.", wait=True)
    for breath in range(3):
        await lamp.light("blue", 4, 4000)
        await lamp.say("Breathe in.", wait=True)
        await asyncio.sleep(3)
        await lamp.light("blue", 1, 4000)
        await lamp.say("And out.", wait=True)
        await asyncio.sleep(3)
    await lamp.say("Well done.")
    await lamp.close()


asyncio.run(main())
