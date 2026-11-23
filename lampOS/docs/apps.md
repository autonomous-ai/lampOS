# Apps: the contract (#7415)

An **app** is a program a developer writes with the SDK, publishes to the
Skills Store, and an owner installs from the app on their phone; it then runs
on the lamp as its own unprivileged systemd unit (`lamp-app@<name>.service`),
talking to the lamp through the door's vocabulary on a socket of its own. This
page is the contract between the developer, the Store, the phone's app and the
lamp. It is in English only ([the docs rule](../AGENTS.md)).

**Status, honestly.** Built: the package format and its reader (piece 1,
`lamp_skills::app_package`, with tests) and the yes ops in lamp-control (piece
3a, `app_yes.rs`) and the app socket with `phrases.set` (pieces 3b-1 and 3b-2a, `app_socket.rs`, [control.md](control.md)). The app socket's `app.trigger` and its 3 s hold are built too
(piece 3b-2b, `app_triggers.rs`). `apps.remember` and the `remember` grant are built behind
`LAMP_APP_REMEMBER` (C166, `app_remember.rs`; below). Designed and not yet built: lamp-cloud's `apps.*` kinds
(piece 4), the installer (`lamp-apps-manage`, eighth's). Everything behind
the one switch **`LAMP_STORE_APPS`**, off unless set and `on` in the product's control, cloud and apps units (#7956). A lamp that has the
format probe refuses an app package in words ("format 2 is not one this lamp
reads (1)"); an older lamp refuses it with its parser's "unknown field". Either
way nothing is installed.

**App tools: designed, not built** ([app-tools.md](app-tools.md), #8386). An
app's actions become functions Lamp's model calls, as it calls a skill's:
a `tools` file in the skills' format in the manifest, offered by the voice
in a section of its own while the app runs, and called through lamp-control's
`app.call`. It retires `phrases.set` for answers and replaces `apps.ask`;
phrases stay for a session's fast words.

## The package (format 2)

A package is a tar, as a skill's (format 1, [skills.md](skills.md)): the
manifest `lamp-skill.json` and its signature `lamp-skill.sig` **beside** one
folder named for the app, which holds every other file.

```
lamp-skill.json
lamp-skill.sig
english-lessons/src/lamp_learn/app.py
english-lessons/assets/ok.wav
english-lessons/skills/english-tutor/skill.toml
```

- `lamp-skill.sig` is `{"key": <fingerprint>, "signature": <base64>}`:
  Ed25519 over `lamp-skill.json`'s exact bytes, with the **Store's key**. It is
  checked before the manifest is parsed.
- Every file is listed in the manifest with its size and SHA-256; a file not
  listed, listed twice or missing is refused, and so is a path of more than 8
  parts, one with a part beginning with a dot, or a link or device in the tar.
  Files live under `src/` (the app's Python), `assets/` (WAV clips) and
  `skills/<name>/` (companion skills): nothing else.
- Limits: at most 512 files, 16 MiB of files, 18 MiB the package.
- Unknown fields are refused at every level of the manifest.

## The manifest

```json
{
  "format": 2,
  "kind": "app",
  "name": "english-lessons",
  "title": "English lessons",
  "version": "1.2.0",
  "min_build": 143,
  "runtime": { "name": "python3", "entry": "lamp_learn.app" },
  "grants": ["speak", "hear", "light", "body", "sound", "skills"],
  "net": false,
  "logs": true,
  "phrases": ["start my english lesson"],
  "triggers": ["start my english lesson", "begin the english lesson"],
  "settings": [
    { "key": "level", "type": "choice", "label": "Level",
      "options": [{ "value": "a1", "label": "A1" }, { "value": "a2", "label": "A2" }],
      "default": "a1", "hint": "optional" },
    { "key": "answer_s", "type": "number", "label": "Seconds to answer",
      "min": 3, "max": 60, "unit": "s", "default": 15 },
    { "key": "name", "type": "text", "label": "Learner's name", "max": 40, "default": "" },
    { "key": "hints", "type": "toggle", "label": "Show hints", "default": true }
  ],
  "skills": ["english-tutor"],
  "assets": ["assets/ok.wav"],
  "files": [{ "path": "src/lamp_learn/app.py", "size": 123, "sha256": "…64 hex…" }]
}
```

| field | rule |
|---|---|
| `format`, `kind` | `2`, `"app"` |
| `name` | `^[a-z][a-z0-9-]{0,23}$`; the tar's folder has this name |
| `title` | 1 to 40 characters on one line; the lamp says "stop the *title* app" |
| `version` | `MAJOR.MINOR.PATCH` |
| `min_build` | the lamp build's sequence number (the part before the `+` in `lamp.build`), 1 or more; a lamp below it refuses to install |
| `runtime` | `name` is `python3` (the only one in v1); `entry` is a dotted module path (`a.b.c`), and `src/a/b/c.py` or `src/a/b/c/__init__.py` must be in the package. It is run isolated (`-I`: no `PYTHONPATH`, no working directory on the path), which a bare `python3 -I -m <entry>` cannot do (it never finds `<app>/src`; run on 18 Nov), so the unit runs `python3 -I -c 'import runpy, sys; sys.path[:0] = sys.argv[1:3]; runpy.run_module(sys.argv[3], run_name="__main__", alter_sys=True)' <app>/src <the lamp's SDK folder> <entry>`: the app's `src` first, then the folder the lamp keeps `lampos` in, then the module as `__main__` (checked 18 Nov with a module that imports a package from the second entry). Where the lamp keeps the SDK is the installer's (`lamp-apps-manage`), not the manifest's |
| `grants` | names from the door's 14 (`body`, `light`, `speak`, `look`, `hear`, `sound`, `skills`, `air`, `device`, `camera`, `microphone`, `speaker`, `notes`, `speech`) and the app's own `remember` (`apps.remember`, below), no duplicates. No service's name, and not `net` or `logs`. What the owner says yes to is this list as the app showed it |
| `net`, `logs` | top-level booleans, default `false`, never inside `grants` |
| `npu` | optional: the lamp's NPU ops the app uses, from `pose` only today (`lamp_skills::app_package::NPU_OPS`), no repeats; each needs the grant it rides (`pose`: `camera`). With it, the lamp's NPU service fetches that op's model when the app is installed, not at its first ask (npu.md, "When the model is fetched"). A lamp built before this field refuses a manifest that has it, so an app that names it sets `min_build` to a build that reads it |
| `phrases` | at most 16; each at most 64 bytes (the socket's `phrases.set` counts characters) with at least 8 letters, no repeats; needs `hear` |
| `triggers` | required (may be `[]`); at most 8, the same per-phrase rule; needs `hear`; matched whenever the app runs and **not** part of the 16 phrases. A trigger may also be a phrase: a heard line is delivered once, as the trigger |
| `settings` | at most 16, below |
| `skills` | the companion skills: exactly the folders under `skills/`, each holding a `skill.toml`; installed, updated and removed with the app, never listed on their own |
| `assets` | exactly the files under `assets/`, each ending `.wav`, at most 20; each a WAV of 16-bit mono PCM at 24 kHz, 1 to 240,000 frames (10 s), within the lamp's loudness limits (no sample past −12 dBFS, no 10 ms louder than −30 dBFS RMS: the voice's own `decode_sound_with_limit`) |
| `files` | as above |

Also enforced, as the reader reads them: a path in the tar is at most 1024
bytes, each of its parts ASCII `[A-Za-z0-9._-]` of at most 100 bytes and none
beginning with `.`; `entry` is lowercase, at most 100 bytes; a label, title or
hint has no leading or trailing space. A tar holds plain files only (no link,
device or sparse entry), no second manifest, signature or file of one path,
and no gzip.

### Settings

The app shows the owner these in its own page; lamp-control checks a value
against the manifest **before** the app sees it, so a bad value never reaches
the app. Every field: `key` `^[a-z0-9_]{1,32}$` and unique, `label` 1 to 40,
optional `hint` up to 120, and a `default` valid for its type.

| type | fields | default is |
|---|---|---|
| `text` | `max`, a whole number 1 to 200 (required) | text of at most `max` |
| `number` | `min` below `max`, both finite; optional `unit` up to 16 | a number from `min` to `max` |
| `toggle` | none | true or false |
| `choice` | `options`, 2 to 16 of `{value, label}`; a `value` is `[a-z0-9][a-z0-9_-]` up to 32 and unique, a `label` 1 to 40 | one of the values |

A field with a key its type does not have (`max` on a toggle) is refused.

## From the developer's computer

The Python SDK checks and packs an app, and submits it to the Store
(`lamp app check`, `lamp app pack`, `lamp login`, `lamp app submit`,
`lamp app status`, `lamp app request-public`, `lamp app studio`; the JS SDK's
`submitApp`, `submissions`, `requestPublic`). The developer's folder holds
`app.json` (the manifest above without `format`, `kind` and `files`) beside
`src/`, `assets/` and `skills/`; `pack` adds those three and writes the tar
unsigned, and the Store signs it at approval. The SDK READMEs' "Publish your
app" has the commands and the Store's refusals. Staged: tested against a fake
Store only so far.

## An app from your own computer (`app.push`, #8351)

A developer installs their own app on their own lamp from a computer paired
with it, with no Store, the way a skill is pushed (control.md, "Skills
pushed from a computer"). The first builder's ruling on #7570: **no yes is
asked**, at the lamp or in the app (the owner's 18 Nov rule: installs are
not asked about); the consent is the pairing.

- **Who may push.** Only a computer this lamp's owner paired, with the
  `skills` grant, the one the pairing screen says "can put skills on the lamp
  that run on their own". A computer never paired here (another owner's) is
  refused at `hello` (`unpaired`), before any op. The app may ask only
  grants that pairing holds now (`no_grant`, "not granted: camera"), never
  `remember`, an app's own grant no computer holds. `net` and `logs` are
  the manifest's, as for a Store app.
- **Unsigned, on this path only.** The package is format 2, as `lamp app
  pack` makes it, and must **not** carry `lamp-skill.sig` ("it carries
  lamp-skill.sig: an app pushed from a computer is unsigned"). It is read
  with every other check of "What the reader checks, in order"
  (`lamp_skills::app_package::read_pushed_app_package_bytes`), by
  lamp-control first and by the apps' helper again. The helper takes its
  `install_pushed`, `list_pushed_by`, `remove_pushed`, `off_pushed_by` and
  `grants_pushed_by` from lamp-control's
  cgroup alone (`lamp_apps::ask::answer_door`), and takes none of the
  Store's verbs from it; lamp-cloud and the voice can't ask the pushed
  verbs. So an unsigned package reaches the lamp only from a paired
  computer.
- **On the lamp** it is installed as a Store app is (the same folders,
  unit, sandbox and launcher, started at once unless the owner stopped
  it), with no yes taken, and its record (`lamp-apps.json`) says `pushed`:
  the computer's pairing id and name. The phone's `apps.list` row says
  `"from": "computer"` (a Store app's `"store"`), for "From your computer",
  and `"pushed_by": {"computer", "client"}` (the name at pairing, at most 64
  bytes, and the pairing id, from that record), the shape a pushed skill's
  `skills.list` row has; a Store app's row has no `pushed_by`. Like a pushed
  skill's row, it carries no push time. A pushed app's row always carries
  `net`, from that record. Once it is on, the lamp says one line with its
  undo: "Gym Coach is on, from your computer. To undo, remove it in the
  app."; for an app whose manifest says `net: true`, "Gym Coach is on, from
  your computer, and can use the internet. To undo, remove it in the app."
  (no yes named the internet, as a Store app's does).
- **Stopped when the computer is revoked or loses the grant.** On the
  owner's revoke, lamp-control asks the apps' helper `{"off_pushed_by":
  CLIENT}`: each app that computer pushed is stopped (not removed) with the
  owner's stop mark, so the boot's `start-all` leaves it stopped, and the
  owner can still remove or start it from the phone; another computer's
  pushed apps and Store apps are not touched. When the owner narrows a
  pairing (`set_grants`, or a re-pair with other grants), it asks
  `{"grants_pushed_by": {"client", "grants"}}`: a pushed app that asks a
  grant the pairing no longer holds is stopped the same way, and the rest
  run on. Each answers `{"ok": {"stopped": [NAME], "not_stopped": ["NAME:
  why"]}}`. Each change is kept in lamp-control's `--state-dir` as
  `apps-helper-owed.json` (0600) before it is sent, and let go once the
  helper answered (asked three times, as the skills' helper is), so one the
  helper never answered, a restart in between, is told again at
  lamp-control's next start. An install answered after the pairing changed
  is followed by the change again, as for a pushed skill.
- **Replaced only by its computer.** A push of the same name from the same
  computer replaces it, any version, older too. A push never takes a Store
  app's name ("a Store app has that name") nor another computer's ("another
  computer's app has that name"), and the Store never updates a pushed app
  ("this app was pushed from a computer: remove it before installing it
  from the Store").
- **Removed** by its computer (`app.remove`, no yes) or by the owner in the
  phone's app (`apps.remove`, the owner's yes as for any app), with the same
  remove as any app's, its data and what lamp-control kept of it going too.
  `app.list` shows the computer's own pushed apps only.
- **The wire** (lamp-control, `--apps-manage-socket`; with `LAMP_STORE_APPS`
  off every op is refused "apps are off on this lamp", and without the
  socket "app push is not on this lamp"): `app.push`
  `{name, sha256, size, part, parts, data}` as `skill.push`'s parts, the
  package at most 4 MiB in 1 to 128 parts of at most 48 KiB of base64, all
  within 60 s, one push in flight on the lamp (`busy`), one every 10 s and 20
  an hour a computer; each part but the last answers `{"got": N}`, the last
  `{"app": {"name", "title", "version", "sha256", "grants", "net", "logs",
  "updated", "started", "from": "computer", "pushed_by"}}` once installed
  (`updated` a bool: it replaced one already there). `app.list` answers
  `{"apps": [{"name", "title", "version", "sha256", "grants", "net",
  "logs", "stopped", "pushed_by"}]}`; `app.remove` `{name}` answers `{"removed": NAME}`.
  Both 6 a minute. lamp-control stages the whole package, checked, as
  `/var/lib/lampos-apps-staging/pushed-<sha256>.tar` (0600, the unit's
  `ReadWritePaths=`), asks the helper `{"install_pushed": {"path",
  "client", "client_name"}}`, and removes the file once answered.
- **The SDKs:** `lamp app push FOLDER|TAR` (checked and packed by `lamp app
  check`/`pack` first), `lamp app list`, `lamp app remove NAME`;
  `push_app`, `apps`, `remove_app` (Python), `pushApp`, `apps`, `removeApp`
  (JS); the fake lamp serves all three.
- **Not yet:** a spoken "remove it" as the undo (the voice has no such
  intent; the line points at the app instead). Tested with a fake helper
  and the helper's own tests; not yet run on a lamp.

## What the reader checks, in order

`lamp_skills::app_package::read_app_package_bytes(bytes, store_keys)` reads in
memory and writes nothing: the package's size; the tar (plain files only);
**the signature**; the manifest's own rules (all of the above); the folder's
name; each listed file's size and SHA-256 and no file unlisted; the entry
module; the assets and their clips; the companion skills. It returns the
manifest and its bytes, the app's files, each companion skill's files, the key
fingerprint and the package's SHA-256 (the Store's catalogue names it so).
The installer checks, beyond it, `min_build` against the lamp and the
companion skills against the existing skill checks, and takes the owner's yes
before it writes anything.

## On the lamp

Installed by `lamp-apps-manage` (root, its own sandbox; eighth's): the app's
files read-only to it under `/opt/lampOS/apps/<name>/<version>/` with a
`current` link; its data in the unit's state folder
`/var/lib/private/lampapp-<name>/` (systemd's `DynamicUser`, the units'
`StateDirectory=lampapp-%i`, one path component), behind the link
`/var/lib/lampapp-<name>`, which survives an update and is deleted by
a remove and by forget-everything; the unit `lamp-app@<name>.service` from a
template (`DynamicUser`, no new privileges, `Slice=lampos-apps.slice`), or
`lamp-app-net@<name>.service` for an app whose manifest says `net: true` (the
same template with the network). A failed or cut install is cleaned (the
half-written version and temporary folders go, the old `current` stays). A
remove runs in one order, every step safe to run again: mark it, stop both
unit variants, wait until neither has a process, clear their failed state (a unit that is not loaded, which `systemctl` says
with exit 5 or with exit 1 and "not loaded", counts as cleared), 
delete the data, the code and the companion skills, then, only with both units
empty, ask lamp-control `{"app_forget": NAME}` so that what it kept of the app
(settings, panel, notes, clips, moves) goes too, and clear the mark. The boot
start (`lamp-apps-start.service`, `lamp-apps-manage start-all`) finishes a cut
remove the same way and may send `app_forget` as well: lamp-control accepts it
from that unit for this op alone. (The helper's `app_forget` step is piece 2
part C, #7813; until that is in, a remove does not send it.) Companion skills
are placed into the skills folder by the same helper with the skill checks.

A factory reset ("forget everything", `lamp-setup`'s reset) leaves no app of the previous owner: its barrier stops
every `lamp-app@*` and `lamp-app-net@*` unit with the other writers, and a step after the plan's wipes empties
`/opt/lampOS/apps` (the apps' code and the helper's launcher), `/var/lib/lampos-apps-state` (the marks) and
`/var/lib/lampos-apps-staging` (staged packages), keeping the folders the units name, and removes every
`/var/lib/private/lampapp-*` and its `/var/lib/lampapp-*` link (`reset::remove_installed_apps`, #7956). The next owner
starts with no app, no app data and nothing to start at boot; the apps' facts in memory go with the person's
forget-everything.

The first round that adds the apps' part (#8131's build, done by the lamp's stable lamp-update) can start
`lamp-apps-folders.service` before it makes `/opt/lampOS/apps-manage/current`, which fails it 203/EXEC, and it enables
`lamp-apps-manage.socket` into `sockets.target`, which no round starts (lamp-9adf, 23 Nov). So the folders unit has
`Restart=on-failure`, `RestartSec=2s` and at most five starts a minute, and `lamp-control.service`, which that round
restarts after the links, has `Wants=lamp-apps-manage.socket lamp-apps-folders.service` and
`After=lamp-apps-manage.socket`: after the round the folders are made and the socket listens, with no reboot and no
restart by hand.

### The owner's yes

An install, an update or a remove needs the owner's yes, recorded in
lamp-control (memory only, ten minutes, used once) as the seven values (`op`,
`name`, `version`, `sha256`, the sorted grants, `net`, `logs`, all seven
required and no other; for an update only the grants newly asked, and `net` the
manifest's and not the installed one's, `logs` likewise, so only what is newly
on; for a remove the installed version and sha256, no grants, `net` and `logs`
false) and matched
exactly: a changed byte, grant, `net` or `logs` never matches. `net` and `logs`
stand outside the grants in the manifest, so they are in the key and in the
words said back like grants: an update that turns the internet or the logs on
is a different key and is said back as such (the owner hears "and the
internet"), never yes'd as "no new grants". One writer: lamp-cloud
(the owner's tap in the phone's app, from the owner's signed-in account on the
lamp's own channel; the tap is the yes, with no separate spoken one: the
voice's cgroup is refused). The
helper takes it once, by its own cgroup. Ops on lamp-control's admin socket:
`app_yes_record`, `app_yes_take`, answered `{"ok":true}` or `{"refused":WHY}`
(piece 3a, built: [control.md](control.md)).

### The app socket

(Built in pieces 3b-1, 3b-2a and 3b-2b; the SDK's fake lamp speaks the first two. The socket's side is [control.md](control.md), "An installed app's socket".) One socket,
`LAMP_APP_SOCKET`, default `/run/lampos-apps/apps.sock`, mode 0666 in the folder
`/run/lampos-apps` that the product's `tmpfiles.d` line makes (`d /run/lampos-apps 0755 lampos lampos -`: the shared `lampos` user's, which is also lamp-cloud's and lamp-skills' user, so the folder keeps an app out, not another lampOS part, except that the three `lampos` units with no `ProtectSystem=strict` (lamp-body, lamp-eyes, lamp-tasks, where the agents' shells run) have it hidden, `InaccessiblePaths=-/run/lampos-apps`, and an install test holds every other `lampos` unit to that or to `strict`; lamp-control's unit names it `ReadWritePaths=-/run/lampos-apps` under `ProtectSystem=strict`, and it never makes the folder: its bind fails quietly without it): the guard is the peer's cgroup (`/lampos.slice/lampos-apps.slice/lamp-app@<name>.service`, or `lamp-app-net@<name>.service` for an app whose manifest says `net: true`, and the unit must be the manifest's: a plain unit for a `net` app, or the `net` unit for any other, is refused), and the process must still have the uid the kernel named on connect (a pid that came back as another process is no peer),
read from `SO_PEERCRED`'s pid, never a name the app says. It speaks the door's
WebSocket at `/v1` with no TLS and the same frames; `hello` takes any
well-formed `client` and `sig` and answers with `app` and `version` beside
`lamp`, `grants` (the manifest's) and `limits`. The app may send the door's ops
its grants allow, each refused `no_grant` without its grant, plus:

| op | fields | answer |
|---|---|---|
| `phrases.set` | `phrases`: replace-whole, at most 16, each 1 to 64 characters with at least 8 letters (the door's `subscribe` rules, its rate on changing them), no phrase twice (`bad_request`, as the schema's `uniqueItems`), no field but `phrases`; an empty list stops listening; needs `hear` | `{"ok":true,"phrases":N}` |
| `app.took` | `turn`: the id of an `app.trigger`, 1 to 64 characters; ten a second at most | `{"ok":true,"taken":true}` when the turn was offered this app and is within 3 s; `{"ok":true,"taken":false}` when it was not counted (late, never offered, or already taken): the voice has let the model answer, so the app must not speak for that line; over the rate `{"ok":false,"error":{"code":"rate","why":"ten a second","retry_ms":N}}` |
| `app.settings.get` | none; ten a second at most | `{"ok":true,"settings":{key: value,…}}`, every key with its current value; over the rate `{"ok":false,"error":{"code":"rate","why":"ten reads a second","retry_ms":N}}` |
| `app.panel.set` | `title` 1 to 40, `lines` at most 6 of at most 80, no control characters; one a minute | `{"ok":true,"rev":N}`; over the rate `{"ok":false,"error":{"code":"rate","why":"one panel a minute","retry_ms":N}}`, and the minute holds across the app's reconnects |
| `apps.remember` | `text`: one fact for the lamp's known person, at most 280 characters once tidied, no other field; needs `remember` and `LAMP_APP_REMEMBER`; never while private; three a minute | `{"ok":true,"kept":true}` once memory kept it; refusals and the voice's side below, "What an app tells Lamp's memory" |
| `apps.context` | `line`: one live line for this session, at most 120 characters once tidied, with `for_ms` 1 to 600000 (10 minutes; the app renews it); `line` `""` alone (no `for_ms`) clears it; no other field; needs `remember` and `LAMP_APP_REMEMBER`; a line never while private; one a second (a clear always goes) | `{"ok":true}` once told to the voice; refusals below, "What an app tells Lamp's memory" |

Events (`{"ev":…}`): `phrase.heard` `{phrase}` (never what was said),
`app.trigger` `{trigger, phrase, turn}` (a manifest trigger; the words around
it are not carried), `app.settings` `{changed:[keys], settings:{…}}`. With no
`app.took` from the app within 3 s of a trigger (`turn` is the trigger's
turn), or a `say` from that app within the same 3 s (only a `say`, only an app
with the `speak` grant, only the app that was offered the trigger; light, body
and sound do not count), the voice answers the line itself. lamp-control (`app_triggers.rs`, behind `LAMP_STORE_APPS`)
offers a trigger only to an app running now (a socket up) whose manifest has
exactly those words, tells the voice the running apps' triggers on
`apps/triggers` at each change and every 5 s, and an `app.took` it offered
within the 3 s, or its `say` in them, on `apps/took` (once a turn; one not
offered, offered twice or late is answered `taken: false` and told to nobody).
An app that is hung with its socket open stays listed, so each line that spells
its trigger waits the 3 s before the model answers. A trigger is the app's own
choice and may be a broad phrase ("turn off the lights"): while the app runs it
takes that line from the model, which is the owner's install to weigh. A trigger
offered while 16 are already waiting is not sent. When the owner stops, updates or removes the app, systemd sends
`SIGTERM` and the connection closes with a close frame that says why; no event
comes first. Errors are the door's codes; with `LAMP_STORE_APPS` off every op
is refused `off`.

### What an app tells Lamp's memory (`apps.remember` and `apps.context` built)

Agreed on #7415 (the first builder's yes of 19 Nov 16:45): an app with the
`remember` grant keeps a fact for the lamp's known person, marked with the app
as its source, and sets a live context line that is never kept. The memory's
side is built ([memory-days.md](memory-days.md), "An app's facts and context
lines"; `lamp_msgs::app_fact`). **`apps.remember` is built** (C166:
lamp-control's `app_remember.rs`, the voice's `app_remember.rs`), behind its
own switch `LAMP_APP_REMEMBER` (lamp-control; off, in no unit, until the first
builder's privacy read and the ROW) inside `LAMP_STORE_APPS`. **`apps.context`
is built** too (lamp-control's `app_context.rs`, the voice's `app_context.rs`),
behind the same two switches and, at the voice, `LAMP_APP_MEMORY`.

- **The grant.** `remember` is an app's own grant, beside the door's 14 and
  never a computer's: a manifest may ask it (`lamp_skills::app_package::GRANTS`,
  `REMEMBER_GRANT`; the apps helper's `APP_GRANTS`), and it is granted at
  install with nothing more asked, as every grant is (the owner's 18 Nov rule).
  `hello` lists it after the door's grants. A manifest without it reads as
  before; a lamp built before it refuses a manifest that asks it, so such an
  app sets `min_build` to a build that reads it.
- **The op**, on the app socket only, after `hello`:
  `{"id", "op": "apps.remember", "text"}`, no other field (the app never names
  whose it is). Answered `{"ok":true,"kept":true}` once memory kept it.
  Refused, in this order: `off` (`LAMP_STORE_APPS`, then `LAMP_APP_REMEMBER`
  off: "remembering is off on this lamp"); `no_grant` ("apps.remember needs the
  remember grant"); `bad_request` for another field, no text, or words that
  `app_fact::tidy` refuses (links, markup and double quotes left out, runs of
  space made one; nothing left, or over 280 characters: refused, never cut);
  `private` while the privacy switch is on (or its lease lapsed, or lamp-control
  has no reading of it); `unavailable` ("Lamp is still forgetting this app's
  earlier facts") while a forget from the app's remove is still pending, so a
  reinstall's new facts are never forgotten with the old; `rate` past three a
  minute an app (`retry_ms`; the minute holds across the app's reconnects; only
  a remove forgets it, and a refused ask is not counted). Then memory's own, as
  `refused`: "Lamp did not keep it" for `not_known` and `forgotten` alike, so an
  app learns nothing of the person's own forgets (the journals keep the exact
  code), "at most 20 facts an app a day (too_many)", or `bad_time`;
  `unavailable` when the voice or memory gave no word within 2.5 s (it may not
  be kept).
- **Whose and how.** The door tells the voice on `apps/remember`
  (`AppRemember`: the door's ask number, the app's checked name, its manifest's
  title as `app_fact::name`, the tidied words, the wall time). The voice picks
  the person from the owner's memory ids alone (each passed through
  `app_fact::whose`): never another person and never the room; with none,
  `not_known`. The owner may go by two ids (a typed "Minh Nguyen" whose face was
  kept as "Minh"), so the face's (`known_faces::owner_now`) is tried first, as
  the person's own memory is keyed by it, then each other in turn while memory
  answers `not_known` (`app_remember::owners_face_first`, `keep`). It sends
  memory's `remember_app_fact` with source `app:<name>` and says how it went on
  `apps/remembered`, which is the app's answer. The journals say the
  app and the outcome, never a word of the fact.
- **A live context line (`apps.context`).** On the app socket only, after
  `hello`: `{"id", "op": "apps.context", "line", "for_ms"}`, no other field.
  `line` is one live line for the app's session ("set 2 of 3, 7 squats"),
  told to the model beside the person's line until `for_ms` has passed (1 to
  600000 ms; the app renews it by sending it again) and never kept. `{"id",
  "op": "apps.context", "line": ""}` with no `for_ms` clears it. Answered
  `{"ok":true}` once lamp-control published it to the voice (no memory round
  trip: the line lives in the voice's RAM only), or `unavailable` ("Lamp's
  voice can't be told now") when it could not. Refused, in `apps.remember`'s
  order: `off` (`LAMP_STORE_APPS`, then `LAMP_APP_REMEMBER`); `no_grant`
  ("apps.context needs the remember grant"); `bad_request` for another field,
  no `line`, a `for_ms` missing, not a whole number or outside 1 to 600000, a
  `for_ms` with a clear, or words `app_fact::tidy` refuses (nothing left, or
  over 120 characters: refused, never cut); `private` ("the lamp is private:
  nothing is told now") while the privacy switch is on (or unread); `rate`
  ("one line a second", `retry_ms`) within 1 s of the app's last line told,
  held across reconnects, forgotten by a remove. One a second because a
  set's counter changes a few times a minute and a line is told only at the
  person's next turn, so nothing an app means is lost while a loop can't flood
  the bus. A clear is never refused for privacy or the rate, and is not
  counted; a refused line or one not published is not counted either. The
  door's pending forget of a removed app's facts does not hold a new live
  line (the remove already cleared the old one); a person's own "forget what
  <app> said" clears it at the voice, below.
  - **To the voice in one hop.** The door publishes `AppContext` on
    `apps/context` (`{"app", "name", "line", "for_ms"}`: the app's checked
    name, its manifest title as `app_fact::name`, the tidied line; a clear is
    `{"app", "name": "", "line": "", "for_ms": 0}`). The voice's writer
    (subscriber `voice`, started once a process with `LAMP_APP_MEMORY` on)
    calls `app_context::set` or `clear` with its own privacy reading (none yet
    counts as private) and the journal says the app and what was done, never a
    word. The line is told only on the owner's turn: any spoken turn in the
    private room (`LAMP_PRIVATE_ROOM=on`, whoever speaks is the owner), and
    outside it only when the known person in view is the owner, never a
    guest's or the room's. App send to the voice holding the line is one bus hop: about 0.3 ms
    in the door's bus test, under 100 ms asserted in the voice's.
  - **Gone with the session.** When the app's last connection closes (it
    stopped, was updated, crashed or was removed), lamp-control publishes the
    clear for it, once; `app_forget` publishes it before closing the
    connections. If that publish fails, or lamp-control restarts, the voice
    drops the line at its own `for_ms`, 10 minutes at most. The voice also
    clears every line when Lamp goes private and on a forget everything, and
    one app's line on a heard `forget` whose `about` names that app holding
    a line ("forget what Gym Coach said", by its title or id as whole words;
    `app_context::forget_about`), unless the forget erred or forgot nothing
    (`forgotten: false`). The app's next line is then a new one.
- **A remove forgets, and the forget holds.** `app_forget` (the helper's
  remove) first marks the app in `--state-dir`'s `app-facts-forget.json` (0600;
  a mark that can't be written refuses the op, so the helper asks again), then
  lamp-control's own thread sends memory's `forget_app_facts` for `app:<name>`
  on memory's forget socket (`/run/lampos-memory/forget.sock`, which takes it
  and `forget_person` only), at once and every 30 s, until memory answers `ok`;
  only then is the mark let go (`app_facts_forget.rs`). The voice and the bus
  are no part of it, and a mark left by a restart is tried at the start. A
  memory with no Days store (`unknown_op`) keeps no app facts and counts as
  done. Every fact of the app goes, with what was made from them, whatever
  `LAMP_APP_REMEMBER` says now. The person's own forgets take them as any
  kept thing. An update whose manifest drops `remember` forgets nothing: the
  app's facts stay until its remove, the person's forget or their 90 days.
