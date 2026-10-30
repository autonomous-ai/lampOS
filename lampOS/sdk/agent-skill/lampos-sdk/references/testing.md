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
another terminal with the same `LAMPOS_HOME`.

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

What the fake gives a test:
- `fake.asked(op)`: the fields of every `op` request it took, oldest first.
- `await fake.emit(event, **fields)`: say an event to subscribers.
- `fake.refuse_next(op, code, why, retry_ms=None)`: the next `op` is refused.
- `await fake.done(re, how)`, `await fake.lose_lease(lease, why)`.
- `fake.clock`: set it to test the one-pulse-a-minute rule.
- `hand_track(...)`, `await fake.emit_hand_track(track)`: a synthetic hand.
- `refuses=LAMP_CONTROL_REFUSES`: answer as a lamp does today.

## In JavaScript tests (node --test)

```js
import { Keyring, Lamp } from 'lampos';
import { FakeLamp } from 'lampos/testing';

const fake = await FakeLamp.start({ grants: ['light', 'speak'] });
const ring = new Keyring(dir);
await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring: ring });
const lamp = await Lamp.connect(undefined, { keyring: ring });
await myCiLight(lamp, 'fail');
// fake.asked('light'), fake.emit('person.arrived', { count: 1, near: 0.3 }), fake.refuseNext(...)
await lamp.close(); await fake.stop();
```

## lamp-mcp

`lamp-mcp` connects to the paired lamp; pair it with `lamp fake --everything`
to try an agent's tools without a lamp.
