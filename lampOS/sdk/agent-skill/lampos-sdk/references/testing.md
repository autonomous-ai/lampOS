# Testing with the fake lamp

The SDKs ship a fake lamp that speaks the real protocol over TLS and
answers as lamp-control does: the same refusals, in the same words, for
light levels, pulses, moves, leases and grants.

## By hand

```sh
lamp fake --everything          # pairs this computer with it; Ctrl-C ends it
lamp fake --hands               # also waves a hand after each arrival
```

Without `--everything` the fake refuses what real lamps don't route yet
(`lampos.testing.LAMP_CONTROL_REFUSES`), as they do. Run your program in
another terminal with the same `LAMPOS_HOME`. The fake also plays a small
day on its own: every `--every` seconds (10) it says `person.arrived`, then
`phrase.heard` ("good night lamp"), then `person.left`, for `--cycles` rounds
(until Ctrl-C), so a program that listens has something to hear. It prints
`asked` lines only between those events (subscriptions included, and one
event late); for every frame, run your program
with `LAMPOS_LOG=debug`. This skill's `scripts/try_fake.sh` starts a fresh
fake, runs your program, and prints the fake's log after.

## In Python tests (pytest)

```python
import asyncio
from lampos import Keyring, Lamp
from lampos.testing import FakeLamp

def test_my_ci_light(tmp_path):
    async def go():
        async with FakeLamp(grants=("light", "speak")) as fake:
            ring = Keyring(tmp_path)
            await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, keyring=ring)
            async with await Lamp.connect(keyring=ring) as lamp:
                await my_ci_light(lamp, "fail")          # your code
            assert fake.asked("light")[0]["colour"] == "red"
            assert fake.asked("say")
    asyncio.run(go())
```

`FakeLamp(...)` answers every op by default, as `--everything` does; pass
`refuses=LAMP_CONTROL_REFUSES` to answer as a lamp does today.

What the fake gives a test:
- `fake.asked(op)`: the fields of every `op` request it took, oldest first,
  without `id` and `op`, as the program sent them: `light` →
  `{"colour", "level", "for_ms"}` plus `"pattern"` only when one was given;
  `say` → `{"text"}`; `express` → `{"feeling", "strength"}`; `gesture` →
  `{"name"}`; `look` → `{"at"}` (a name, or `{"yaw", "pitch"}`); `joints` →
  `{"lease", "targets", "within_ms"}`. A refused request isn't in it.
- `await fake.emit(event, **fields)`: say an event to subscribers.
- `fake.refuse_next(op, code, why, retry_ms=None)`: the next `op` is refused
  with that code (one of the schema's) and `why` (any text).
- `await fake.done(re, how)`, `await fake.lose_lease(lease, why)`.
- `fake.clock`: a function giving seconds (JS: milliseconds); set it to step time. With it,
  `fake.pulses_played` and `fake.pulses_dropped` (JS `pulsesPlayed`,
  `pulsesDropped`) show the one-pulse-a-minute rule: a second pulse within a
  minute is in `asked("light")` but lands in `pulses_dropped`.
- `hand_track(...)`, `await fake.emit_hand_track(track)`: a synthetic hand.
- Grants: a call without its grant is refused `no_grant`, as on a lamp
  (`express` needs both `body` and `light`), so test with the grants your
  program will have.

## In JavaScript tests (node --test)

```js
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Keyring, Lamp } from 'lampos';
import { FakeLamp } from 'lampos/testing';

const fake = await FakeLamp.start({ grants: ['light', 'speak'] });
const ring = new Keyring(mkdtempSync(join(tmpdir(), 'lamp-')));
await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring: ring });
const lamp = await Lamp.connect(undefined, { keyring: ring });
await myCiLight(lamp, 'fail');
// fake.asked('light'), fake.emit('person.arrived', { count: 1, near: 0.3 }), fake.refuseNext(...)
await lamp.close(); await fake.stop();
```

The JS fake's clock is in milliseconds, `fake.emit` doesn't wait, and
`FakeLamp.start({ refuses })` takes the same refusals as Python's.

## Waiting for an event to reach your program

`fake.emit` returns once the event is sent, not once your handler has run.
Wait for what your handler does, with a small helper, never a fixed sleep:

```python
async def until(check, within=5.0):
    deadline = asyncio.get_running_loop().time() + within
    while not check():
        assert asyncio.get_running_loop().time() < deadline, "never happened"
        await asyncio.sleep(0.01)

await fake.emit("person.arrived", count=1, near=0.3)
await until(lambda: fake.asked("gesture"))
```

```js
async function until(check, within = 5000) {
  const end = Date.now() + within;
  while (!check()) {
    if (Date.now() > end) throw new Error('never happened');
    await new Promise((r) => setTimeout(r, 10));
  }
}
fake.emit('person.arrived', { count: 1, near: 0.3 });
await until(() => fake.asked('gesture').length > 0);
```

Subscribe before you emit: `await lamp.on(...)` returns once the fake has
the subscription.

## lamp-mcp

`lamp-mcp` connects to the paired lamp; pair it with `lamp fake --everything`
to try an agent's tools without a lamp.
