"""Put a skill of your own on your lamp, then list what this computer has
put there. The pairing needs the `skills` grant; the skill keeps running
after this program ends, never beyond the computer's other grants:

    python push_skill.py path/to/my-skill
"""

import asyncio
import sys

from lampos import Lamp


async def main(folder: str):
    async with await Lamp.connect() as lamp:
        skill = await lamp.push_skill(folder)
        if skill["held"]:
            print(f"{skill['name']} {skill['version']} waits for the owner's yes: {skill['held']}")
        else:
            print(f"{skill['name']} {skill['version']} is on, asking {', '.join(skill['shown']) or 'nothing'}")
        for kept in await lamp.skills():
            print(f"  {kept['name']} {kept['version']}{'' if kept['on'] else ' (off)'}")


asyncio.run(main(sys.argv[1]))
