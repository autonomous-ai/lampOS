"""Check this computer's pairing with a lamp: hello, grants, limits, and what
each grant lets a program do. Exit 0 when the lamp answered hello.

    python check_pairing.py [LAMP_ID] [--host HOST]

Needs the lampos SDK and a pairing (`lamp pair`, or `lamp fake`).
"""

import argparse
import asyncio
import sys

from lampos import Lamp
from lampos.errors import LampError

GRANTS = {
    "body": "move: gestures, feelings, looks, moves (looks and moves under a body lease)",
    "light": "light the ring",
    "speak": "say lines, play sounds",
    "look": "hear presence and hands; look at a face or follow a hand",
    "hear": "hear registered phrases and the lamp's name",
    "sound": "hear sound classes (music, laughter, door, footsteps)",
    "skills": "push skills of your own to the lamp",
}


async def check(lamp_id, host):
    try:
        lamp = await Lamp.connect(lamp_id, host=host)
    except LampError as refused:
        print(f"no hello: {refused}")
        return 1
    async with lamp:
        print(f"lamp: {lamp.info.get('name')} ({lamp.info.get('id')})")
        print("grants:")
        for grant in lamp.grants:
            print(f"  {grant}: {GRANTS.get(grant, 'see the SDK README')}")
        missing = sorted(set(GRANTS) - set(lamp.grants))
        if missing:
            print(f"not granted (the owner ticks these in the app): {', '.join(missing)}")
        limits = lamp.limits
        rates = "off: only refused actions and phrase changes are bounded" if limits.get("rates") == "off" else "on: a window per kind"
        print(f"rates: {rates}")
        print(f"quiet hours: {limits.get('quiet_hours', 'not said')}")
        try:
            state = await lamp.get("lamp")
            print(f"now: {state.get('lamp')}")
        except LampError as refused:
            print(f"state: {refused}")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("lamp", nargs="?", help="the lamp's id, when paired with more than one")
    parser.add_argument("--host", help="an address to use instead of the kept one")
    args = parser.parse_args()
    sys.exit(asyncio.run(check(args.lamp, args.host)))


if __name__ == "__main__":
    main()
