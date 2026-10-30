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

## React to presence (look for the events, body for the gesture)

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
        await asyncio.sleep(20)     # closing gives the light back at once: stay while it shows

asyncio.run(main(sys.argv[1] if len(sys.argv) > 1 else "pass"))
```

```js
import { Lamp } from 'lampos';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const lamp = await Lamp.connect();
if ((process.argv[2] ?? 'pass') === 'pass') {
  await lamp.light('green', 3, 20_000, 'breathe');
} else {
  await lamp.light('red', 3, 20_000, 'pulse');
  await lamp.say('The build failed.');
}
await sleep(20_000);      // closing gives the light back at once: stay while it shows
await lamp.close();
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

```js
import { Lamp, LampError } from 'lampos';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function hold(lamp, colour, seconds) {
  for (let i = 0; i < Math.floor(seconds / 25); i++) {
    await lamp.light(colour, 2, 30_000);
    await sleep(25_000);
  }
}
const lamp = await Lamp.connect();
for (;;) {
  await hold(lamp, 'warm', 25 * 60);
  try {
    await lamp.say('Time for a short break.');
    await lamp.gesture('stretching');
  } catch (e) {
    if (!(e instanceof LampError)) throw e;
    console.log('break, quietly:', e.code);
  }
  await hold(lamp, 'green', 5 * 60);
}
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

```js
import { Lamp } from 'lampos';
const LOOK = {
  working: ['blue', 2, 'breathe'],
  waiting: ['yellow', 3, undefined],    // undefined: steady (no pattern is sent)
  done: ['green', 3, undefined],
  failed: ['red', 3, undefined],
};
class Progress {
  constructor(lamp) { this.lamp = lamp; this.timer = null; }
  async show(state) {
    clearInterval(this.timer);
    const [colour, level, pattern] = LOOK[state];
    const light = () => this.lamp.light(colour, level, 30_000, pattern).catch((e) => console.log('light:', e.code));
    await light();
    this.timer = setInterval(light, 25_000);
  }
  stop() { clearInterval(this.timer); }
}
const lamp = await Lamp.connect();
const progress = new Progress(lamp);
await progress.show('working');
// ... your agent's work ...
await progress.show('done');
await lamp.express('glad');
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
        await asyncio.sleep(20)     # the light lasts while this stays connected

asyncio.run(main())
```

```js
import { Lamp, LampError } from 'lampos';
async function alert(lamp, text, urgent = false) {
  await lamp.light('purple', 3, 20_000, urgent ? 'pulse' : undefined);
  if (!urgent) return;
  try { await lamp.say(text.slice(0, 300)); } catch (e) { if (e instanceof LampError) console.log('said nothing:', e.code); else throw e; }
}
```

Hook `alert` to your webhook handler (an `aiohttp` or Express route), and
connect once at start rather than per message.

## Hear a phrase (hear)

```python
await lamp.on("phrase.heard", handler, phrases=["good night lamp"])
```

Up to 16 phrases per computer, whole words, 8 letters at least; the event
carries only which of yours it was, never the words around it.

## A program that runs for days

The lamp restarts on every update, and a reconnect is a new session: it
loses its subscriptions, leases and camera stream. Loop on the connection,
set up again each time, and back off.

```python
import asyncio
from lampos import Lamp
from lampos.errors import LampError

async def run_once(lamp):
    # Everything a session needs, again on every connection.
    await lamp.on("person.arrived", lambda e: print("arrived", e["count"]))
    await lamp.on("touch", lambda e: print("touched", e["kind"]))
    await lamp.wait_closed()          # returns when the lamp goes away

async def main():
    wait = 1
    while True:
        try:
            async with await Lamp.connect() as lamp:
                wait = 1              # connected: reset the backoff
                await run_once(lamp)
        except (LampError, OSError) as gone:
            print("lamp away:", gone)
        await asyncio.sleep(wait)
        wait = min(wait * 2, 60)

asyncio.run(main())
```

```js
import { Lamp, LampError } from 'lampos';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let wait = 1000;
for (;;) {
  let lamp;
  try {
    lamp = await Lamp.connect();
    wait = 1000;
    await lamp.on('person.arrived', (e) => console.log('arrived', e.count));
    await lamp.on('touch', (e) => console.log('touched', e.kind));
    await lamp.closed();
  } catch (e) {
    if (!(e instanceof LampError) && !e.code) throw e;
    console.log('lamp away:', e.message);
  } finally {
    await lamp?.close().catch(() => {});
  }
  await sleep(wait);
  wait = Math.min(wait * 2, 60_000);
}
```

Take a body lease only around what needs it, and let it go after.

## A web app on the SDK

A browser page can't hold the lamp's pairing key, so put a small local
server between them:
- **The server holds the key and the one connection** to the lamp (the SDK),
  and offers the page your own small API and an event stream (server-sent
  events or a WebSocket).
- **Bind `127.0.0.1`**, never `0.0.0.0`: other computers on the network
  mustn't drive your lamp through it.
- **A random token on every API call and the event stream** (made at start,
  put in the page's URL or a cookie), so another page in the same browser
  can't call it.
- **Check `Origin` against `Host`** on every call and the event stream; behind
  a tunnel the host changes, so compare the two, don't hard-code a name.
- **Serve the page and its assets (HTML, CSS, JS) without the token**, and
  require it only on the API and the stream; asked for on the assets, the
  CSS and JS come back 403 and the page is blank.
- Reconnect to the lamp as in "A program that runs for days", and tell the
  page when the lamp is away.
