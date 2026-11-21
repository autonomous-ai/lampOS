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
(piece 3b-2b, `app_triggers.rs`). Designed and not yet built: lamp-cloud's `apps.*` kinds
(piece 4), the installer (`lamp-apps-manage`, eighth's), and the views an
app gives the phone ("App views" below, behind `LAMP_APP_VIEWS` as well). Everything behind
the one switch **`LAMP_STORE_APPS`**, off unless set. A lamp that has the
format probe refuses an app package in words ("format 2 is not one this lamp
reads (1)"); an older lamp refuses it with its parser's "unknown field". Either
way nothing is installed.

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
| `grants` | names from the door's 14 only (`body`, `light`, `speak`, `look`, `hear`, `sound`, `skills`, `air`, `device`, `camera`, `microphone`, `speaker`, `notes`, `speech`), no duplicates. No service's name, and not `net` or `logs`. What the owner says yes to is this list as the app showed it |
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

### What an app tells Lamp's memory (`apps.remember`, `apps.context`, designed)

Agreed on #7415 (the first builder's yes of 19 Nov 16:45): an app with the
`remember` grant keeps a fact for the lamp's known person, marked with the app
as its source, and sets a live context line that is never kept. The memory's
side is built ([memory-days.md](memory-days.md), "An app's facts and context
lines"; `lamp_msgs::app_fact`); the two ops at this socket, the grant and
their writer are not built yet, and come behind their own switch, refused
while the physical privacy switch is on.

## App views: screens the phone draws (`views`, designed, not built)

**Designed, not built** (#7415; the first builder's direction of 21 Nov, 09:37,
the Store's three asks beside it). Nothing below is in the code yet: no
`views` or `ops` field in `AppManifest`, no op on the app socket, no kind in
lamp-cloud and no screen in the phone's app. A lamp today refuses a manifest
with either field ("unknown field"). The design waits for the first builder's
read before any code.

An app may give the phone up to 8 **views**: typed screens the phone's app
draws itself on the app's page, from small JSON the app's own op returns. Three
kinds: `list`, `chart` over time and `form`. No downloaded code runs on the
phone, so the Store checks a data shape, not a page. A plan editor is a `form`
with a `rows` field. A real app that needs a fourth kind brings its case.

### In the manifest

Two optional top-level fields; a lamp built before them refuses them, so an app
that names them sets `min_build` to a build that reads them (as `npu`).

| field | rule |
|---|---|
| `ops` | the names the app answers on its socket: at most 16, each `^[a-z][a-z0-9_]{0,31}$`, no repeats. Required (non-empty) when `views` is there |
| `views` | at most **8**, each `{id, kind, title, op, …}`: `id` `^[a-z][a-z0-9_]{0,31}$` and unique; `kind` `list`, `chart` or `form`; `title` 1 to 40 on one line; `op` one of the manifest's `ops`. The kind's own fields below, and no other |

**The op rule (the Store's check, and the reader's).** A view's `op` must be
one of the app's own `ops`; a package naming an undeclared op, an unknown kind,
a field its kind does not have, or more than 8 views is refused, by the Store's
review and again by `read_app_package_bytes`. Two views may share an op; the
op is told which view asks.

A view's own fields in the manifest:

- `list`: optional `empty`, up to 80 characters (shown when there are no items).
- `chart`: `style` `line` or `bar` (required); optional `unit` up to 16; optional
  `goal_label` up to 24.
- `form`: `fields`, 1 to 16 (below), and `submit`, the button's words, 1 to 24.

### Each kind's answer

The op answers one JSON object, at most **32 KiB** as sent (the app's
serialised `data`). lamp-control checks every answer against its view's kind
and these limits before it leaves the lamp: an answer too big, of the wrong
shape or late is refused whole, and the phone shows one plain line, "*Title*
couldn't be shown: the app's answer wasn't one Lamp can read." (too big, bad)
or "*Title* didn't answer in time." (late), with Try again. Never part of an
answer, never one cut short. Text everywhere is one line with no control
characters, U+2028, U+2029 or bidi marks (the panel's rule).

**`list`**: `{"sections": [{"title"?, "items": [...]}]}`, or `{"items": [...]}`
for one section with no title.

| field | type | rule |
|---|---|---|
| `sections` | array | 1 to 12; a section's `title` optional, 1 to 40 |
| `items` | array | at most **200** in the whole answer, across sections |
| item `title` | text | required, 1 to 60 |
| item `subtitle` | text | optional, up to 80 |
| item `value` | text | optional, up to 24 (shown at the right: "42 kg", "3 × 10") |
| item `badge` | text | optional, up to 16 ("New", "PR") |

**`chart`** (over time only): `{"series": [...], "goal"?: number, "range"?: text}`.

| field | type | rule |
|---|---|---|
| `series` | array | 1 to **4**; each `{label, points}`, `label` 1 to 24 and unique |
| `points` | array | 0 to **92** a series (a quarter of days), each `{"x": "YYYY-MM-DD", "y": number}`; `x` a real date, strictly increasing; `y` finite |
| `goal` | number | optional, finite: a flat line, named by the view's `goal_label` |
| `range` | text | optional, up to 40, said under the title ("18 to 24 Nov") |

The `style` and `unit` are the manifest's, never the answer's, so the Store
reviews what the chart means.

**`form`**: its fields are in the manifest, so the Store reviews them and
lamp-control checks values against them **before** the app sees them, as it
does settings. A field: `key` `^[a-z0-9_]{1,32}$` and unique, `label` 1 to 40,
optional `hint` up to 120, and its type:

| type | fields | a value is |
|---|---|---|
| `text` | `max` 1 to 200 (required) | text of at most `max` |
| `number` | `min` below `max`, finite; optional `unit` up to 16; optional `step` above 0 | a number from `min` to `max` |
| `toggle` | none | true or false |
| `choice` | `options`, 2 to 16 of `{value, label}` (the settings' rule) | one of the values |
| `rows` | `columns`, 1 to 4 fields of `text`, `number` or `choice` (no `rows` in `rows`); `max_rows` 1 to 50 | an array of at most `max_rows` objects, each with every column's key |

On open the op answers `{"values": {key: value, …}}` (keys it leaves out start
empty, or at `min`, or off; any value checked as on submit). On the submit
button lamp-control sends the op the checked values; the op answers
`{"ok": true, "said"?: text up to 80}` or `{"refused": text 1 to 120}`, and the
phone shows the words as written. A value that fails the manifest is refused by
lamp-control with the key and why, as `apps.settings.set` is, and the app never
sees it.

### The flow

The phone reaches the lamp one way, at home or away: lamp-cloud's kinds on the
lamp's own account channel over the broker (cloud.md, "The device API"; there
is no separate local path for apps). So a view opened at the gym takes the same
path as one opened at the desk.

1. **Phone → broker → lamp-cloud** (phone: App Only; carrying the kinds and the
   grant check: **Backend**, with Backend3's check that the asker holds the lamp
   and that the app and view are the installed ones). New kinds, only with
   `LAMP_APP_VIEWS=on`: `apps.view` `{"id", "view"}` and `apps.view.submit`
   `{"id", "view", "values"}`; `apps.list`'s rows carry `views` (from the
   installed manifest) and `can` names both kinds only then. Who may ask: the
   lamp's own account channel only (`online::commanded`), as every `apps.*`.
2. **lamp-cloud → lamp-control** (lamp side: Eighth, or App Only): the admin
   op `{"app_view": NAME, "view": ID}` or with `"values"`, lamp-cloud's cgroup
   alone, beside `app_state`. lamp-control checks the app is installed and
   running (a socket up), the view is in its manifest, and a submit's values.
3. **lamp-control → the app** on its socket: the event
   `{"ev": "app.view", "ask": "<id>", "view", "op", "values"?}` (`values` only
   on a submit); the app answers with the op `app.answer`
   `{"ask", "data"}`, or `{"ask", "refused": text}`. An `ask` not offered,
   answered twice or late is refused `bad_request` and dropped. The SDK's side
   (`lamp.on_view(op, handler)`, its fake lamp) is Migrate's.
4. lamp-control checks the answer (kind, limits, 32 KiB) and returns it to
   lamp-cloud, which sends `success` `{"id", "view", "kind", "data"}` or
   `failure` with the plain words, adding no check of its own.

A stopped app answers "*Title* isn't running." with Start; the lamp offline,
the phone's usual "Lamp can't be reached".

**Time (CLAUDE.md rule 7).** Tap to feedback: the view's skeleton (title and
the kind's empty frame) at once, under 100 ms, with no wait on the lamp. Tap to
data: under **1 s** at home, under **2 s** away, from the phone's send to its
draw, measured on the row. The app has **2 s** to answer an `app.view`
(lamp-control's bound; a fixed answer is expected in under 100 ms), lamp-cloud
waits at most its `CONTROL_WAIT` (5 s), and the phone gives up at 8 s with the
"didn't answer in time" line. At most 4 asks wait per app and 10 a second
(`rate` with `retry_ms`), a submit at most one a second; a second open of a
view while one waits joins it, not a second ask.

**Fresh, not pushed.** The phone asks on open, on pull-to-refresh and after a
submit (the form and every view of the same op); nothing is pushed in v1, and
the phone keeps no copy once the page closes. The last answer asked wins on
screen; an older one landing later is dropped.

### Privacy and grants

- A view's data is whatever the app's op returns: what the app already holds
  within its grants and its own data folder. A view asks for no grant and adds
  none; the owner's yes is the install's, unchanged.
- An app removed takes its views with it: they leave `apps.list`, `app_forget`
  drops any ask in flight, and the phone shows nothing kept.
- No log carries a view's data or a form's values: lamp-control and lamp-cloud
  say the app, the view, the outcome, the bytes and the milliseconds only, and
  so does Backend's step.

### The switch

`LAMP_APP_VIEWS`, `on` exactly, off unless set, with `LAMP_STORE_APPS`; read
by lamp-cloud (the kinds, `can`, `views` in `apps.list`) and lamp-control
(`app_view`, `app.view`, `app.answer`), set alike in both. In no unit. Off: the
kinds are "not on this lamp", `app.answer` is `off`, and an app with `views`
still installs and runs, its views not shown. Its row in
[switches.md](switches.md), added with the code, not before (that page is a
census of the code), reads: "the views an installed app gives the phone (list,
chart over time, form) … off until the views row passes".

### Owners

- The phone's views: App Only.
- The lamp side (the `ops` and `views` fields, the op check, `app_view`, the
  socket's `app.view` and `app.answer`): Eighth, or App Only.
- The Store's schema check, with a test package for each refusal: Backend3.
- The cloud step (carrying `apps.view` and `apps.view.submit` between the phone
  and the lamp, with Backend3's grant check): Backend.
- The SDK's docs, `views` template and `lamp app check`: Migrate, after #8198.

### The row, and what is not in v1

**The row** (lamp-0c89 and a test iPhone, both switches on, gym-coach with its
views): each view opened at home and away (the phone on mobile data), first
open and again, with the times; the plan saved and read back; a refused value
and an app's own refusal shown as words; a fixture app's too-big, bad and late
answers each shown as one plain line; the app stopped, then removed, its views
gone; the switch off, no views; the journal and Backend's logs holding no view
data.

**Not in v1:** web views (#7415's proposal of 21 Nov, declined), images, push
or live updates, charts other than over time (no pie, no scatter), and views
on the lamp's own screen.

### Worked example: gym-coach

```json
{
  "ops": ["week", "sessions", "plan"],
  "views": [
    { "id": "this_week", "kind": "chart", "title": "This week", "op": "week",
      "style": "bar", "unit": "min", "goal_label": "Daily goal" },
    { "id": "sessions", "kind": "list", "title": "Sessions", "op": "sessions",
      "empty": "No sessions yet. Say \"start my workout\"." },
    { "id": "my_plan", "kind": "form", "title": "My plan", "op": "plan",
      "submit": "Save plan",
      "fields": [
        { "key": "days", "type": "number", "label": "Days a week", "min": 1, "max": 7, "step": 1 },
        { "key": "routine", "type": "rows", "label": "Routine", "max_rows": 20,
          "columns": [
            { "key": "exercise", "type": "text", "label": "Exercise", "max": 40 },
            { "key": "sets", "type": "number", "label": "Sets", "min": 1, "max": 10, "step": 1 },
            { "key": "reps", "type": "number", "label": "Reps", "min": 1, "max": 50, "step": 1 }
          ] }
      ] }
  ]
}
```

The phone opens "This week"; the app is sent
`{"ev": "app.view", "ask": "a17", "view": "this_week", "op": "week"}` and answers:

```json
{ "op": "app.answer", "ask": "a17", "data": {
  "range": "18 to 24 Nov",
  "goal": 20,
  "series": [{ "label": "Workout", "points": [
    { "x": "2026-11-18", "y": 25 }, { "x": "2026-11-19", "y": 0 },
    { "x": "2026-11-20", "y": 18 }, { "x": "2026-11-21", "y": 30 } ] }]
} }
```

"Sessions" answers `{"sections": [{"title": "This week", "items": [{"title":
"Legs", "subtitle": "Fri 21 Nov, 07:10", "value": "30 min", "badge": "PR"}]}]}`;
"My plan" opens with `{"values": {"days": 3, "routine": [{"exercise": "Squat",
"sets": 3, "reps": 10}]}}`, and a save answers `{"ok": true, "said": "Plan
saved: 3 days a week."}`.
