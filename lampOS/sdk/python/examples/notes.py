"""Count how many times this program has run, in a note the lamp keeps for
this computer, and show the count as the light's level for a moment. The
note stays across this program's restarts and the lamp's. `notes` and `light`
are the grants it needs:

    python notes.py
"""

import asyncio

from lampos import Lamp


async def main():
    async with await Lamp.connect() as lamp:
        runs = await lamp.notes.get("runs", 0) + 1
        await lamp.notes.set("runs", runs)
        print(f"run {runs}")
        await lamp.light("warm", min(runs, 5), 3_000)
        # Closing gives the light back at once: stay while it shows.
        await asyncio.sleep(3)


asyncio.run(main())
