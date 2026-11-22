# #7901 ROW leg K fixture: never expected to start (its key was never added in the app).
import asyncio
import os

from lampos import Lamp


async def main():
    lamp = await Lamp.local()
    await lamp.say("The key reached me." if os.environ.get("ROW_PROBE_API_KEY") else "No key here.", wait=True)
    await lamp.close()


asyncio.run(main())
