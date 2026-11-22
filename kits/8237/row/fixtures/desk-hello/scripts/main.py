# #8237 ROW fixture: one short say, then done (its words are 20 chars, never the getting-ready line's 30).
import asyncio

from lampos import Lamp


async def main():
    lamp = await Lamp.local()
    await lamp.say("Hello from the desk.", wait=True)
    await lamp.close()


asyncio.run(main())
