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
`ci-light.js`, and `push-skill.js ../../examples/relaxing-music` (a pairing with `skills`).

It answers as a lamp does today: `gesture` and `move` with `{"to": "body"}`,
`express` as the feelings skill's move and light,
and `play`, `sound`, `look` and `joints` refused (`refused`,
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

## Push a skill to your own lamp

With the `skills` grant, which the owner ticks at pairing, a computer can put
a skill of its own on its own lamp, with no Store
([control.md](../../docs/control.md), "Skills pushed from a computer"):
`await lamp.pushSkill(folder)` packs the folder and sends it in parts, and
resolves with the lamp's answer (on, or held for the owner's yes);
`await lamp.skills()` and `await lamp.removeSkill(name)`; on the command line,
`lamp skill push FOLDER`, `lamp skill list` and `lamp skill remove NAME`. It refuses a
folder without a `version`, or a `core` or add-on skill. The fuller check is
the Python SDK's `lamp skill check`, and the lamp checks everything again.

To share a skill, sign it with a key of your own, kept apart from the
pairing key: `lamp skill keygen` once (`author.pem` in `LAMPOS_HOME`), then
`lamp skill pack FOLDER --sign` writes `NAME-VERSION.tar`, the shape
`lamp-skills pack --key-file` makes. `lamp skill check NAME-VERSION.tar`
checks its signature and every file. In code, `pack(folder, { author })` and
`openPackage(bytes)` from `lampos/push`. A signature says the bytes are that
key's, not who is behind the key, so `check` says "signed by key FPR16, not
the Store".

`lamp skill install FILE|URL` puts a shared package on your own paired lamp
(the `skills` grant): a file, or an `https://` URL read to 1 MiB at most. It
shows the package's name, version and signer, and what it asks in the
owner's app's words (`camera`: "uses the camera when it runs", which the lamp
keeps off until the owner says yes in the app), asks you yes once (`--yes` skips it), and sends
it as `skill.push` with `asks` on part 0. In code, `lamp.installPackage(bytes)`
and `asksOf(rules)`. The skill is held to your pairing's grants whatever it
asks.

### Indexes: lists of shared skills

An index is a signed list of signed packages that anyone can host: a gist, a
folder, a LAN share. It grants nothing, and nothing of it runs on the lamp:

```sh
lamp skill index make desk-things *.tar --url-base https://example.org/desk/ --seq 1   # publish: lamp-index.json, signed
lamp skill index add https://example.org/desk/lamp-index.json   # keep it; pins its publisher's key, asking once
lamp skill browse wave                                            # what your indexes list now
lamp skill install desk-wave                                      # by name, from the index that lists it
```

The index is fetched as a package is: https only, 1 MiB at most. It must be
signed by the key pinned when you added it, and never with a lower `seq` than
the last one seen. `install NAME` fetches the entry's package and refuses it
unless its SHA-256, size, signer and asks are the entry's. Then it asks you
yes as for any shared package. A publisher is shown as "published by key
FPR16, not the Store", never "verified".

- What you're shown at install comes from the package itself, never from its
  entry: an entry can only make the install refuse.
- `index add` and `install` name the host they fetch from. A URL with a name
  or password in it is refused.
- An index that can't be fetched is named by `browse`, and the others go on.
  `install NAME` then needs `--from INDEX`.
- An older version than the one on your lamp needs `--force`.
- Nothing is fetched or installed in the background: an index is read when you
  run `browse` or `install`, and a skill goes on only on your yes.
- `seq` stops an older copy of the index, but not someone serving an old one
  frozen. A publisher can't withdraw a version yet. A format 1 listing refuses
  any field it doesn't know, so a later `withdrawn` list comes as format 2,
  which an older SDK refuses rather than reads past.
- A fetch that is redirected to another host says so: "fetched from B,
  redirected from A".
