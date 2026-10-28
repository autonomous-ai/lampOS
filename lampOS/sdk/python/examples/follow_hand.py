"""Hand signs: hold an open hand up to the lamp and its head follows it; take
the hand away, or close it, and the lamp lets go and its own life comes back.

Needs the `body` and `look` grants. The lamp follows only while it is shown
an open hand, and a hand on its head or a conversation takes the body back
at once; this then waits for the next open hand.
"""

import asyncio

from lampos import Lamp
from lampos.errors import LampError


async def main():
    async with await Lamp.connect() as lamp:
        shown = asyncio.Event()
        dropped = asyncio.Event()

        def hand(event):
            # The lamp tells a hand by change, and `seen: false` once it's gone.
            if event["seen"] and event["open"]:
                shown.set()
                dropped.clear()
            else:
                shown.clear()
                dropped.set()

        await lamp.on("hand", hand)
        while True:
            await shown.wait()
            try:
                async with lamp.follow_hand(why="following the hand it is shown") as following:
                    gone = asyncio.ensure_future(dropped.wait())
                    await asyncio.wait({gone, asyncio.ensure_future(following.wait())}, return_when=asyncio.FIRST_COMPLETED)
                    gone.cancel()
                    print("let go: " + ("the hand dropped" if dropped.is_set() else "the lamp took its body back"))
            except LampError as refused:
                print(f"not now: {refused}")
                await asyncio.sleep(10)


asyncio.run(main())
