"""Start a turn with a question: the lamp asks it, the person answers the lamp.

The person's words stay on the lamp; this computer hears only how the turn
ended. Needs the speak and hear grants.
"""

import asyncio

from lampos import Lamp
from lampos.errors import LampError


async def main():
    async with await Lamp.connect() as lamp:
        try:
            done = await lamp.start_turn("Shall we go over today's list?", wait=True)
            print(f"the turn ended: {done['how']} {done.get('why', '')}".strip())
        except LampError as refused:  # quiet hours, private, no grant...
            print(f"not now: {refused}")


asyncio.run(main())
