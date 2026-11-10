"""A visitor count: the lamp keeps how many times someone arrived today in a
note, quietly, and says the number when asked "how many visitors".

An arrival is the lamp's own (a face after the room was empty for a while); the
lamp tells this computer only that someone arrived, never who. Nothing is said
or lit when someone arrives: a lamp that announces every comer is a doorbell.
The count is kept per day (this program's own clock, since an event's `at` is
UTC), so a restart does not forget it.

Needs the `look` (arrivals), `hear` (the phrase), `speak` and `notes` grants.
Try it on the virtual lamp: `lamp-sim people 0 DIR`, wait about 5 s, then
`lamp-sim people 1 DIR` for an arrival, then `lamp-sim hear "how many visitors" DIR`.
"""

import asyncio
from datetime import datetime

from lampos import Lamp
from lampos.errors import LampError


def today() -> str:
    """The day key by this program's own clock."""
    return datetime.now().strftime("%Y-%m-%d")


async def main():
    async with await Lamp.connect() as lamp:
        counting = asyncio.Lock()  # one note write at a time: a second one is refused `busy`

        async def arrived(event):
            async with counting:
                key = f"visits-{today()}"
                try:
                    count = await lamp.notes_get(key, 0) + 1
                    await lamp.notes_set(key, count)
                    print(f"visit {count} today")
                except LampError as refused:
                    print(f"not now: {refused}")  # notes paused, over its limits...

        async def asked(event):
            async with counting:
                count = await lamp.notes_get(f"visits-{today()}", 0)
            line = "Nobody yet today." if count == 0 else f"{count} so far today."
            try:
                await lamp.say(line)
            except LampError as refused:
                print(f"not now: {refused}")  # private, paused, asleep, quiet hours...

        await lamp.on("person.arrived", arrived)
        await lamp.on("phrase.heard", asked, phrases=["how many visitors"])
        print("counting arrivals; Ctrl-C to stop")
        await lamp.wait_closed()


try:
    asyncio.run(main())
except KeyboardInterrupt:
    pass
