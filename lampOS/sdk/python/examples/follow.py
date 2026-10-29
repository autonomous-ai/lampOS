"""A desk camera's helper: while someone sits facing the lamp, it looks at them;
when they leave, it lets go and the lamp's own life comes back.

Needs the `look` grant (faces), and takes a body lease only while someone is
there. A hand on the lamp's head or a conversation takes the body back, and
this waits for the next arrival.
"""

import asyncio

from lampos import Lamp
from lampos.errors import LampError


async def main():
    async with await Lamp.connect() as lamp:
        here = asyncio.Event()
        await lamp.on("person.arrived", lambda event: here.set())
        await lamp.on("person.left", lambda event: here.clear() if event.get("count", 0) == 0 else None)
        while True:
            await here.wait()
            try:
                async with lamp.lease("body", why="looking at the person at the desk") as lease:
                    while here.is_set() and lease.held:
                        await lamp.look("face")
                        await asyncio.sleep(1)  # motion: 1 a second
            except LampError as refused:
                print(f"not now: {refused}")
                await asyncio.sleep(10)


asyncio.run(main())
