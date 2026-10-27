"""Say "good night lamp" and a script of yours runs: here, a dim warm light and
a nod. The lamp tells this computer only which phrase it heard, never what
else was said.

Needs the `hear` grant.
"""

import asyncio

from lampos import Lamp


async def main():
    async with await Lamp.connect() as lamp:

        async def good_night(event):
            await lamp.light("warm", 1, 30_000, pattern="breathe")
            await lamp.gesture("nod")

        await lamp.on("phrase.heard", good_night, phrases=["good night lamp"])
        await lamp.wait_closed()


asyncio.run(main())
