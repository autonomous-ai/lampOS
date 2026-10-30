# A person's own keys (#2094, built, off unless set)

Product lamps answer through the Autonomous relay and hold no provider key of
the company's (the lead on #1711, 20 Oct 07:34). A person may bring their
own instead, as in V1: a **DeepSeek** endpoint and key for the text model,
and a **Deepgram** key for speech to text. Each is optional. With none, the
lamp uses the relay, as before. The design was agreed on #2094 (El 09:42,
the first builder 09:44, Third's privacy read 09:50).

**Off unless `LAMP_OWN_KEYS=on`**, on three units: `lamp-setup` and
`lamp-cloud` (`Environment=LAMP_OWN_KEYS=on` in a drop-in), and the voice,
through `deploy/product/own-keys-voice.conf` placed as
`/etc/systemd/system/lamp-voice.service.d/own-keys.conf`. `lamp-cloud` also
needs `LAMP_CLOUD=on` and `LAMP_SETTINGS_CHANGE=on`. The helper's units
(`lamp-setup-keys.socket`, `lamp-setup-keys@.service`) are enabled by hand
with the switch, as they aren't in `install`'s list yet. The helper itself
doesn't read the switch (its unit passes `--enable`): with the switch off it
is closed because its units aren't installed, `lamp-cloud` answers "not on
this lamp" and only `lamp-cloud` may ask. Off, nothing changes:
setup checks and keeps nothing and says nothing new, the app's kinds are
"not on this lamp", and the voice reads no person's key even if one is
there.

## How a key arrives

- **At setup** (`crates/setup/src/serve.rs`): the setup request's optional
  `deepseek_api_key`, `deepseek_base_url` (an https address; absent:
  `https://api.deepseek.com`) and `deepgram_api_key`, hidden in `Debug`.
  Setup runs as root. It checks each after the home's Wi-Fi is joined and
  before the marker, so the voice's first start takes them. A key that's
  refused, can't be checked or doesn't fit never fails setup: the lamp uses
  the relay, and `GET /api/device/setup/status` says why in `own_keys`, by
  service (absent when none was given). The status's `takes_own_keys` says
  whether this switch is on, true or false, so the app asks for own keys
  only on a lamp that will use them.
- **From the app, after setup** (`docs/cloud.md`, "A person's own keys"):
  `keys.set`, `keys.remove` ("use the lamp's own again") and `keys.get`.
  `lamp-cloud` writes nothing: it hands one line to root's helper,
  `lamp-setup keys`, on `lamp-setup-keys.socket`.
- **Who may ask the helper:** every part runs as `lampos`, so the socket's
  group can't tell `lamp-cloud` from a skill. The helper reads its peer by
  `SO_PEERCRED` and answers only a process whose cgroup
  (`/proc/<pid>/cgroup`) is the system's place for `lamp-cloud.service`,
  `/lampos.slice/lampos-parts.slice/lamp-cloud.service` or one under it,
  which only root can move a process into. A `lamp-cloud.service` anywhere
  else, such as one a user's own manager made under `user@<uid>.service`, is
  refused (the reviewer's P3 1 on #2167). Any other peer is refused before
  its line is read (Third's P2 on #2094). Peer credentials are read on
  Linux only.
- **What the check can't stop:** a `lampos` process that can attach to
  `lamp-cloud` (same user, with `kernel.yama.ptrace_scope` at 0) can make
  it ask. The check holds against a part that connects itself, not against
  one that controls `lamp-cloud`.

## The messages

The contract posted on #2094 (11:13, with its additions after), which
`lamp-cloud` and the helper hold to:

| Kind | Asked | Success `data` |
|---|---|---|
| `keys.set` | `{"service": "deepseek" \| "deepgram", "key", "base_url"?, "request_id"?}` | `{"service", "uses": "own", "code": "works", "answer"}` |
| `keys.remove` | `{"service", "request_id"?}` | `{"service", "uses": "lamp"}` |
| `keys.get` | `{"request_id"?}` | `{"deepseek": "own" \| "lamp", "deepgram": "own" \| "lamp", "since": {"deepseek": RFC 3339 \| null, "deepgram": …}, "deepseek_host": host \| null}` |

Each is `starting` at once, then one `success` or `failure` of the same kind
within 15 s; `request_id` comes back in `data` on each. A `failure` has the
person's words in `error` and `data.code`: `refused`, `unreached`,
`not_kept` (the key worked but couldn't be written: the lamp uses its own),
`bad_field` (the field named), `busy` (one key request at a time),
`not_allowed` (a peer other than `lamp-cloud`) or `unavailable` (the helper
not there or not answering). Without the switch: V1's "not on this lamp" at
once, with no `starting`. Setup's status says `own_keys` by service as
`{"code", "answer"}`.

## The check

One request to the service before a key is kept, at most 5 s, no redirect
followed, and the only request setup or the helper makes with the key:

| Service | Request | Key |
|---|---|---|
| DeepSeek | `GET <endpoint>/models` | `Authorization: Bearer <key>` |
| Deepgram | `GET https://api.deepgram.com/v1/auth/token` | `Authorization: Token <key>` |

- **2xx:** kept. The answer names where the person's data goes now:
  "that key works: your conversations now go to api.deepseek.com with your
  key: your words, what the lamp remembers that's sent with them, and what
  it sees when asked to look", or for Deepgram, "that key works: your speech
  now goes to Deepgram under your account and its retention terms".
- **401 or 403:** "that key was refused". Not kept, and a key kept before
  stays as it was.
- **Anything else, a redirect, a timeout or no network:** "the lamp couldn't
  reach DeepSeek to check that key; nothing changed". Not kept.
- **2xx but the key can't be written** (a full or failing disk): "the key
  could not be kept; the lamp uses its own now", code `not_kept`. Keeping
  DeepSeek's takes the old key away first, so the person's key for that
  service is removed as far as it can be, and the voice is restarted to
  take the lamp's own (the reviewer's P3 2 on #2167).

## Where it's kept

`/etc/lampOS/voice/own/`: `deepseek.key`, `deepseek.url` (the endpoint,
the old key removed first, so a key never sits beside another key's endpoint) and
`deepgram.key`. The folder is root's, 0700, each file 0600, written whole and
renamed into place. The voice's drop-in loads the folder whole
(`LoadCredential=own:/etc/lampOS/voice/own`), so each file is the credential
`own_<file>` in its `CREDENTIALS_DIRECTORY`; `ConfigurationDirectory=` makes
the folder if it's missing, so an empty one is no credential, never a failed
start.

A key is never in `provider.json` or `second.json`, a journal line, an
error, a reply, a session file or the bus. The 5 s reset wipes the folder as
one plain path (`reset-plan-product.json`, the rule on #2060), and
`lamp-selftest`'s voice route takes a credential's folder as present.

## The route at its next start

`SecondConfig::with_own_keys` (`crates/voice/src/second_config.rs`), with
`LAMP_OWN_KEYS=on`:
- `own_deepseek.key` there: DeepSeek's chat, its search and the memory's
  observation pass go to the person's endpoint with their key, over
  `LAMP_SECOND_DEEPSEEK_BASE_URL`, `deepseek_base_url` or the voice's own
  default for the company relay; whichever of those it was is kept as the
  relay a refused key falls back to.
- `own_deepgram.key` there: speech to text goes straight to Deepgram.
- Otherwise the relay. One journal line each, naming whose key and the
  host, never the key or a path: `DeepSeek: the person's own key, at
  api.deepseek.com (LAMP_OWN_KEYS)`, `Deepgram: the relay, no key of the
  person's (LAMP_OWN_KEYS)`. A key that can't be read (not private, an
  endpoint that isn't https) is passed over for the relay, said by its
  error's kind.

After a change the helper restarts the voice (`systemctl try-restart`) once
the lamp is quiet, an update's rule (45 s with nobody heard and nothing
said), and no update is at work: it takes lamp-update's lock and holds it
through the restart (`change::quiet_and_unlocked`, as a zone's restart), so
a round's watch never sees the voice change process and puts its build back
(Tests' P3 on #4490). It waits at most 30 minutes in all; otherwise the
voice takes it at its next start. Each helper that waits holds one of the socket's 4 connections
(`MaxConnections=4`) while it does: in a busy room, a fifth `keys.*` within
that half hour, `keys.get` included, gets `unavailable`, "the lamp's keys
are not running", until one restart is done (the reviewer's P3 4 on
#2167). Left so: four changes in half an hour is not a person's use, and
handing the restart to one waiter would need a unit of its own.

The voice's start lines name the host each time, never the path: DeepSeek's
chat, search and observation pass say "with the person's own key, at
api.deepseek.com (LAMP_OWN_KEYS)", so a changed endpoint shows in every
session's record, not only the day it was set (Third's P3 on #2167).

**`voice-keys`:** with the switch on, the second route's worker reports the
part to `lamp-health` once a second while it runs, `ok` with `uses=own` or
`uses=lamp`, whether or not the person's key is kept. So a key kept and
then removed (the voice restarted without it) leaves a part still reporting,
never one quiet for good, which would keep the lamp `stale` and, after 5
minutes, droop "Not feeling well" (the reviewer's P2 on #2167). Its bus publisher is named `voice_keys`, as the bus takes no `-` in a
publisher's name; before #2167's fix it was `voice-keys`, which the bus
refused, so the part never reported at all. Turning the
switch off, or dropping the second worker (#960), says once that
`voice-keys` is stopping (`state` = `stopping`, #2415's P3): the second
worker says it at its start with the switch off, and the voice says it when
it drops the second worker with the switch on. `lamp-health` forgets the
part instead of counting it quiet, so the lamp doesn't droop "Not feeling
well" for it, and tracks it again when it next reports
([health.md](health.md), "Stopping on purpose").

**A key that later fails** (a rotation): a request the person's endpoint
refuses with 401 or 403 before any answer goes through the relay it went
over, with the relay's key, so the person never gets a failed answer. One
journal line names the status, and the `voice-keys` part tells
`lamp-health` "your DeepSeek key was refused, using the lamp's own" from
then until the voice restarts. That covers the spoken chat; the search and
the observation pass don't fall back yet. **The person's answer is then out
of date:** they were told "your conversations now go to <host>", while each
refused request goes through Autonomous's relay again. Only `lamp-health`'s
`voice-keys` warning says so; the app could show it beside the keys screen
(the reviewer's P3 5 on #2167).

## Updates

Read one version ahead (#1680): `serve.json`'s `own_keys` field (the
folder, default `/etc/lampOS/voice/own`) is dropped and named by an older
setup. A build rolled back to before #2094 has no reader for `LAMP_OWN_KEYS`
or the `own_*` credentials and uses the relay; the drop-in's
`LoadCredential=` is systemd's alone and changes nothing it reads. An older
`lamp-selftest`, though, says a credential's folder "is not a file".

## What the app's repo must do (not built here)

- Send the three setup fields when the person gives them, and show
  `own_keys`' answers from the setup status.
- A keys screen with `keys.set`, `keys.remove` and `keys.get`, showing the
  `starting` ack at once and the answer after, word for word, including
  where the person's data goes.
- Never store or log a key; the backend passes it through to the lamp and
  never stores or logs it.

## Tests

`cargo test -p lamp-setup` (`tests/it/keys.rs`, `tests/it/serve.rs`,
`tests/it/reset.rs`, `src/keys.rs`), `-p lamp-voice` (`second_config`,
`second_provider::worker`), `-p lamp-cloud` (`settings`, `online`) and
`-p lamp-selftest`: a key absent, present and removed (relay, own, relay);
a refused or unreachable key not kept; a key that worked but couldn't be
written said as `not_kept`, the old one gone and the voice restarted;
`voice-keys` still reporting after a key is removed; the check one request, the key in
its header alone, no redirect followed; only `lamp-cloud`'s cgroup, at the system's place for it, asks; a
planted `sk-…` key in no reply, error, status or journal line; the reset
wiping the folder; a later 401 answered through the relay with the warning;
the switch off changing nothing. Not yet run on a lamp: systemd 252's
folder credential names (`own_<file>`), the cgroup check under the real
socket, and the checks against DeepSeek's and Deepgram's own services.
