# Patterns

Each is short on purpose: adapt it. The SDK's own examples
(`lampOS/sdk/python/examples`, `lampOS/sdk/js/examples`) are complete,
runnable versions of most; `tour.py` / `tour.js` take every kind of call.

Rules that make any of them feel right:
- React to something that happened; don't loop gestures or lights.
- A light lasts at most 30 s: renew it every ~25 s while it should stay, and
  the lamp's own light comes back by itself if your program stops.
- Wrap every action in a refusal handler: the person, quiet hours or another
  motion can say no at any time.

## React to presence (look grant)

```python
import asyncio, time
from lampos import Lamp
from lampos.errors import LampError

async def main():
    async with await Lamp.connect() as lamp:
        last = 0.0
        async def arrived(event):
            nonlocal last
            if time.monotonic() - last < 60:      # at most once a minute
                return
            last = time.monotonic()
            try:
                await lamp.gesture("greeting")
            except LampError as refused:
                print("not now:", refused)
        await lamp.on("person.arrived", arrived)
        await lamp.wait_closed()

asyncio.run(main())
```

```js
import { Lamp, LampError } from 'lampos';
const lamp = await Lamp.connect();
let last = 0;
await lamp.on('person.arrived', async () => {
  if (Date.now() - last < 60_000) return;
  last = Date.now();
  try { await lamp.gesture('greeting'); } catch (e) { if (e instanceof LampError) console.log('not now:', e.message); else throw e; }
});
await lamp.closed();
```

## A CI light (light, speak)

Run at the end of a CI job that can reach the lamp:

```python
import asyncio, sys
from lampos import Lamp

async def main(result):
    async with await Lamp.connect() as lamp:
        if result == "pass":
            await lamp.light("green", 3, 20_000, pattern="breathe")
        else:
            await lamp.light("red", 3, 20_000, pattern="pulse")   # one pulse a minute: see gotchas
            await lamp.say("The build failed.")

asyncio.run(main(sys.argv[1] if len(sys.argv) > 1 else "pass"))
```

## A pomodoro (light, speak, body)

```python
import asyncio
from lampos import Lamp
from lampos.errors import LampError

async def hold(lamp, colour, seconds):
    for _ in range(int(seconds // 25)):
        await lamp.light(colour, 2, 30_000)
        await asyncio.sleep(25)

async def main():
    async with await Lamp.connect() as lamp:
        while True:
            await hold(lamp, "warm", 25 * 60)
            try:
                await lamp.say("Time for a short break.")
                await lamp.gesture("stretching")
            except LampError as refused:
                print("break, quietly:", refused)
            await hold(lamp, "green", 5 * 60)

asyncio.run(main())
```

## An agent's progress on the lamp (light)

A coding agent or long job shows where it is: working, waiting for the
person, done or failed. One light per state, renewed while it lasts.

```python
import asyncio
from lampos import Lamp

LOOK = {
    "working": ("blue", 2, "breathe"),
    "waiting": ("yellow", 3, None),       # needs the person: steady, brighter
    "done": ("green", 3, None),
    "failed": ("red", 3, None),
}

class Progress:
    def __init__(self, lamp):
        self.lamp, self.state, self._task = lamp, None, None

    async def show(self, state):
        self.state = state
        if self._task:
            self._task.cancel()
        self._task = asyncio.create_task(self._hold())

    async def _hold(self):
        colour, level, pattern = LOOK[self.state]
        while True:
            await self.lamp.light(colour, level, 30_000, pattern=pattern)
            await asyncio.sleep(25)

async def main():
    async with await Lamp.connect() as lamp:
        progress = Progress(lamp)
        await progress.show("working")
        await asyncio.sleep(60)            # your agent's work
        await progress.show("done")
        await lamp.express("glad")         # a little joy, once, at the end
        await asyncio.sleep(30)

asyncio.run(main())
```

With lamp-mcp an agent can do this itself: `lamp_light` while it works,
`lamp_express("glad")` at the end, `lamp_say` only when the person must act.

## A chat alert (light, speak)

Something your computer already receives (a Slack or Teams webhook, a mail
filter) shown on the lamp. Keep it calm: a light first, words only for what
matters.

```python
import asyncio
from lampos import Lamp
from lampos.errors import LampError

async def alert(lamp, text, urgent=False):
    await lamp.light("purple", 3, 20_000, pattern="pulse" if urgent else None)
    if urgent:
        try:
            await lamp.say(text[:300])
        except LampError as refused:      # quiet hours, private: the light is enough
            print("said nothing:", refused.code)

async def main():
    async with await Lamp.connect() as lamp:
        await alert(lamp, "Message from the on-call channel.", urgent=True)
        await asyncio.sleep(5)

asyncio.run(main())
```

Hook `alert` to your webhook handler (an `aiohttp` or Express route), and
connect once at start rather than per message.

## Hear a phrase (hear)

```python
await lamp.on("phrase.heard", handler, phrases=["good night lamp"])
```

Up to 16 phrases per computer, whole words, 8 letters at least; the event
carries only which of yours it was, never the words around it.
