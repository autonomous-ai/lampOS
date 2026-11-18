# Apps: the contract (#7415)

An **app** is a program a developer writes with the SDK, publishes to the
Skills Store, and an owner installs from the app on their phone; it then runs
on the lamp as its own unprivileged systemd unit (`lampapp@<name>.service`),
talking to the lamp through the door's vocabulary on a socket of its own. This
page is the contract between the developer, the Store, the phone's app and the
lamp. It is in English only ([the docs rule](../AGENTS.md)).

**Status, honestly.** Built: the package format and its reader (piece 1,
`lamp_skills::app_package`, with tests) and the yes ops in lamp-control (piece
3a, `app_yes.rs`) and the app socket without phrases and triggers (piece 3b-1, `app_socket.rs`, [control.md](control.md)). Designed and not yet built: the
app socket's phrases and triggers (piece 3b-2), lamp-cloud's `apps.*` kinds
(piece 4), the installer (`lamp-apps-manage`, eighth's). Everything behind
the one switch **`LAMP_STORE_APPS`**, off unless set. A lamp that does not
know format 2 refuses an app package in words ("format 2 is not one this lamp
reads (1)") and installs nothing.

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
| `runtime` | `name` is `python3` (the only one in v1); `entry` is a dotted module path (`a.b.c`), run as `python3 -I -m <entry>`, and `src/a/b/c.py` or `src/a/b/c/__init__.py` must be in the package |
| `grants` | names from the door's 14 only (`body`, `light`, `speak`, `look`, `hear`, `sound`, `skills`, `air`, `device`, `camera`, `microphone`, `speaker`, `notes`, `speech`), no duplicates. No service's name, and not `net` or `logs`. What the owner says yes to is this list as the app showed it |
| `net`, `logs` | top-level booleans, default `false`, never inside `grants` |
| `phrases` | at most 16; each at most 64 bytes with at least 8 letters, no repeats; needs `hear` |
| `triggers` | required (may be `[]`); at most 8, the same per-phrase rule; needs `hear`; matched whenever the app runs and **not** part of the 16 phrases. A trigger may also be a phrase: a heard line is delivered once, as the trigger |
| `settings` | at most 16, below |
| `skills` | the companion skills: exactly the folders under `skills/`, each holding a `skill.toml`; installed, updated and removed with the app, never listed on their own |
| `assets` | exactly the files under `assets/`, each ending `.wav`, at most 20; each a WAV of 16-bit mono PCM at 24 kHz, 1 to 240,000 frames (10 s), within the lamp's loudness limits (no sample past −12 dBFS, no 10 ms louder than −30 dBFS RMS: the voice's own `decode_sound_with_limit`) |
| `files` | as above |

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
`current` link; its data under `/var/lib/lampOS/apps/<name>/`, which survives
an update and is deleted by a remove and by forget-everything; the unit
`lampapp@<name>.service` from a template (`DynamicUser`, no new privileges,
`Slice=lampos-apps.slice`). Companion skills are placed into the skills folder
by the same helper with the skill checks.

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
internet"), never yes'd as "no new grants". Two writers: the voice
(a spoken yes) and lamp-cloud (the owner's tap in the phone's app, from the
owner's signed-in account on the lamp's own channel, after the same words the
voice would say: what it is and the grants it asks), told apart by `by`. The
helper takes it once, by its own cgroup. Ops on lamp-control's admin socket:
`app_yes_record`, `app_yes_take`, answered `{"ok":true}` or `{"refused":WHY}`
(piece 3a, built: [control.md](control.md)).

### The app socket

(Built in piece 3b-1 except `phrases.set`, `phrase.heard` and `app.trigger`, which are 3b-2; the SDK's fake lamp speaks it all. The socket's side is [control.md](control.md), "An installed app's socket".) One socket,
`LAMP_APP_SOCKET`, default `/run/lampos-apps/apps.sock`, mode 0666 in a root
0755 folder: the guard is the peer's cgroup (`/lampos.slice/lampos-apps.slice/lampapp@<name>.service`),
read from `SO_PEERCRED`'s pid, never a name the app says. It speaks the door's
WebSocket at `/v1` with no TLS and the same frames; `hello` takes any
well-formed `client` and `sig` and answers with `app` and `version` beside
`lamp`, `grants` (the manifest's) and `limits`. The app may send the door's ops
its grants allow, each refused `no_grant` without its grant, plus:

| op | fields | answer |
|---|---|---|
| `phrases.set` | `phrases`: replace-whole, at most 16, each 1 to 64 characters with at least 8 letters, no repeats; needs `hear` | `{"ok":true,"phrases":N}` |
| `app.took` | `turn`: the id of an `app.trigger`, 1 to 64 characters | `{"ok":true}` |
| `app.settings.get` | none | `{"ok":true,"settings":{key: value,…}}`, every key with its current value |
| `app.panel.set` | `title` 1 to 40, `lines` at most 6 of at most 80, no control characters; one a minute | `{"ok":true,"rev":N}`; over the rate `{"ok":false,"code":"rate","retry_ms":N}` |

Events (`{"ev":…}`): `phrase.heard` `{phrase}` (never what was said),
`app.trigger` `{trigger, phrase, turn}` (a manifest trigger; the words around
it are not carried), `app.settings` `{changed:[keys], settings:{…}}`. With no
`app.took` or say from the app within 3 s of a trigger, the voice answers the
line itself. When the owner stops, updates or removes the app, systemd sends
`SIGTERM` and the connection closes with a close frame that says why; no event
comes first. Errors are the door's codes; with `LAMP_STORE_APPS` off every op
is refused `off`.
