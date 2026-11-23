import asyncio

from lampos import Lamp


async def main():
    lamp = await Lamp.local()
    try:
        await lamp.camera_snapshot()
        line = "The camera was not refused."
    except Exception:
        line = "The camera was refused."
    await lamp.say(line, wait=True)
    await lamp.close()


asyncio.run(main())
