"""A tour of everything a computer can do with a lamp, one step at a time.

Each step runs only with the grant it needs, and a refusal is printed with
the lamp's own words, then the tour goes on. Try it against `lamp fake
--everything` first, then your own lamp.
"""

import asyncio

from lampos import Lamp
from lampos.errors import LampError


async def step(name, action):
    try:
        print(f"{name}: {await action}")
    except LampError as refused:
        print(f"{name}: refused, {refused}")


async def main():
    async with await Lamp.connect() as lamp:
        print(f"connected to {lamp.info.get('name')} ({lamp.info.get('id')})")
        print(f"grants: {', '.join(lamp.grants)}")
        print(f"limits: {lamp.limits}")

        # What the lamp can tell this computer now.
        states = ["light", "mood", "lamp"] + (["presence"] if "look" in lamp.grants else [])
        await step("state", lamp.get(*states))

        # Events, by edge: printed for the rest of the tour.
        events = ["touch", "button", "lamp.changed"]
        if "look" in lamp.grants:
            events += ["person.arrived", "person.left", "hand"]
        for event in events:
            await lamp.on(event, lambda said: print(f"  heard {said}"))

        if "light" in lamp.grants:
            await step("light", lamp.light("warm", 3, 5_000))
            await asyncio.sleep(1)
            await step("light, breathing", lamp.light("blue", 3, 5_000, pattern="breathe"))

        if "speak" in lamp.grants:
            await step("say", lamp.say("Hello from the tour."))

        if "body" in lamp.grants:
            await step("gesture", lamp.gesture("nod"))
            await asyncio.sleep(3)
            if "light" in lamp.grants:
                await step("express", lamp.express("glad"))
                await asyncio.sleep(3)
            try:
                async with lamp.lease("body", why="the SDK tour") as lease:
                    await step("look ahead", lamp.look("ahead"))
                    await step("look to the side", lamp.look(yaw=20, pitch=5))
                    await step("joints", lamp.joints(lease, 1_000, pitch=5))
            except LampError as refused:
                print(f"body lease: refused, {refused}")

        await step("stop", lamp.stop())
        await asyncio.sleep(2)


asyncio.run(main())
