# lampos (JavaScript): program a lampOS lamp from your computer

The Node client for the lamp's v1 control protocol
([docs/control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md),
messages in
[control.schema.json](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.schema.json)).
Same names as the Python SDK, in camelCase, with promises. It runs on your
computer, never on a lamp. Node 20.11 or later.

**New here?** [docs/develop.md](../../docs/develop.md) is the 10-minute quick-start: the fake lamp, a first program, pairing, lamp-mcp and skills.

**Not usable on a lamp yet:** the lamp's side, `lamp-control`, is being built
(#3504). Until then, try it with the fake lamp below.

## Install

It isn't on npm yet. From a checkout of this repository, in your project:

```sh
npm i /path/to/lampOS/sdk/js               # add bonjour-service to find lamps by mDNS
```

## Try it with the fake lamp (no lamp needed)

`lampos fake` (also `lamp fake`) starts a fake lamp on this computer with every grant, pairs this
computer with it at once, and every 10 seconds says a person arrived, the
phrase "good night lamp", and the person left, printing what it is asked.
Keep the trial apart from real pairings with its own `LAMPOS_HOME`
(the fake makes its certificate with `openssl`):

```sh
# terminal 1 (from lampOS/sdk/js, after npm install)
LAMPOS_HOME=/tmp/lampos-try npx --no-install lampos fake
# terminal 2 (the same folder)
LAMPOS_HOME=/tmp/lampos-try node examples/greet.js
```

Terminal 1 then shows `asked {"op":"gesture","name":"greeting"}`. Every
example runs this way: `greet.js`, `follow.js`, `phrase.js`, `pomodoro.js`,
`ci-light.js`.

It answers as a lamp does today: `gesture` and `move` with `{"to": "body"}`,
and `express`, `play`, `sound`, `look` and `joints` refused (`refused`,
"not routed yet: waits for …"), in lamp-control's own words, so a program
built against it meets the same refusals on a lamp. `--everything` answers
every op instead.

## Pair with a real lamp

Run the command as `npx --no-install lampos …` (or from `node_modules/.bin`),
in a project with `lampos` installed: npm has an unrelated package called
`lamp`, and a plain `npx lamp` outside such a project would offer to install
it.

```sh
npx --no-install lampos pair 12345678 --pin 3f9a…   # the code and the lamp's pin (64 hex), both shown in the app
```

The lamp's certificate is checked against the pin before any frame is sent
(only the TLS handshake and the WebSocket's upgrade request, which carry no
secret, go first), so the code's proof never reaches another host, and the pin is inside the
proof too. There is no pairing without it. The pairing is kept in
`~/.config/lampos/` (or `$LAMPOS_HOME`, which is made private too, so point it at a folder of its own), the same folder as the Python
SDK's, so a computer paired by one is paired for both: the folder is yours
alone (0700), the key and `lamps.json` 0600, and the pin is checked again on
every connection.

## Program it

```js
import { Lamp } from 'lampos';

const lamp = await Lamp.connect();
await lamp.on('person.arrived', () => lamp.gesture('greeting'));
await lamp.closed();
```

- **Events:** `await lamp.on(name, handler)` resolves once subscribed, and
  rejects with the lamp's refusal, `NoGrantError` without the event's grant:
  `person.*` needs `look`, `phrase.heard` needs `hear`. `lamp.off(name,
  handler)`. Each needs its grant:

  | event | fields | grant |
  |---|---|---|
  | `person.arrived`, `person.left` | `count`, `near` (0–1, share of the picture's height) | `look` |
  | `person.stood_up` | `side` | `look` |
  | `person.facing` | `facing` | `look` |
  | `phrase.heard` | `phrase`: one you registered (`on(..., { phrases })`), never what else was said | `hear` |
  | `name.heard` | | `hear` |
  | `sound.heard` | `class`, `score` | `sound` |
  | `touch` | `kind` (`tap`, `pet`, `hold`), `where` | none |
  | `button` | `press` (`press`, `long_press`) | none |
  | `time` | your schedule | none |
  | `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep`, `lease`, and `quiet_hours` (planned: not produced yet; the door already refuses `say` and `sound` in quiet hours, or while the lamp's local time isn't known, with the error `quiet_hours`) | none |

- **Actions:** `express`, `gesture`, `look`, `move`, `play`, `joints`,
  `light`, `say`, `sound`, `stop`. Each resolves when the lamp has taken it;
  `{ wait: true }` on a gesture, move or play resolves when it has finished,
  or, where the lamp hands it on and says no more (today `gesture` and
  `move`, answered `{"to": "body"}`), at that answer.
- **Leases:** `await lamp.withLease(['body'], async (lease) => ...)` holds
  the body for looks, moves and joints, renewed for you, until the callback
  ends or the lamp takes it back: `lease.state`, `lease.lostWhy`.
- **Refusals** reject with typed errors: `byCode.rate` (with `retryMs`),
  `byCode.no_grant`, `byCode.private`, ...
- **In your tests:** `FakeLamp` from `lampos/testing` is the same fake lamp:
  `emit`, `refuseNext`, `done`, `leaseEvent`, `requests`, `received`.

**When the lamp restarts** (every update), the connection closes and
`lamp.closed()` resolves: a long-running program connects again, for
example in a loop with a few seconds' pause.

**Dependencies:** `ws` and the optional `bonjour-service` are caret ranges
with no lock file, as a library's are: the application that uses it locks
them.

Tests: `npm test`.
