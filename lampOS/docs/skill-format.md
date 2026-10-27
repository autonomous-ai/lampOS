# The skill package format, version 1

A skill is a folder the lamp runs without writing code: rules that say
**when** (triggers and conditions) and **what** (steps). This page is that
folder's public format, version 1, for people who write skills outside
lampOS and for the Store, the SDKs' `lamp skill check` and the app's
install screen. It describes today's format as `lamp-skills` reads it. Its
shape is [`skill-format.schema.json`](skill-format.schema.json), and a test
keeps every skill in the repo valid against it
(`crates/skills/tests/it/format.rs`).

It speaks the words of the [programming protocol](control.md): its
**events**, **actions** and **grants**. A rule in a skill and a call from a
paired computer name the same things and need the same grants.

[skills.md](skills.md) is the design and every rule's reason. This page is
the reference. Where they differ, the code wins and both are fixed.

## The folder

```
my-skill/
  SKILL.md          # name and description (front matter), and any notes
  skill.toml        # the rules: format 1
  moves/NAME.json   # the skill's own recorded moves (optional)
  sounds/NAME.wav   # short sounds, 3 s at most (optional)
  lights/NAME.toml  # named lights (optional)
  shows/NAME.toml   # a light show of 2 to 30 frames (optional)
```

A packaged skill adds `lamp-skill.json`, with `format: 1`, the skill's
`version` and the SHA-256 of every file, and the Store's signature
`lamp-skill.sig` ([skills.md](skills.md), "Core and installable skills",
`crates/skills/src/package.rs`). A folder with a `run.py`, or any script,
is refused: script skills are a later format.

**`SKILL.md`** starts with front matter between two `---` lines. Two keys
are read and both are required:
- `name`: the folder's own name, 1 to 64 characters of `a-z`, `0-9` and `-`;
- `description`: one line, what the skill does.

Other front-matter lines and the body are for people.

**`moves/NAME.json`** is a recorded move, held to the body's own check
(`lamp_body::moves`). The joints, the bounds and the timing are in
[body.md](body.md), "Moves from files". It is at most 256 KiB, and its
inner `name` is `NAME`.

**`sounds/NAME.wav`** is 16-bit mono PCM at 24 kHz, 3 s at most, within the
voice's loudness bounds (`lamp_voice::say::read_sound`).

## Versions

- **The format:** a `skill.toml` names no format. Every `skill.toml` today
  is format 1, and so is every one a lamp of this build accepts. A later
  format will add a top-level `format` key, and a build will refuse a
  format it doesn't know rather than guess. A package's `lamp-skill.json`
  already says `format: 1`.
- **The skill:** `version`, as `MAJOR.MINOR.PATCH` (no leading zeros, at
  most 9 digits a part). Required in a package and for an add-on.

## `skill.toml`

TOML. Every table refuses a key it doesn't know: a misspelt key is an
error at load, never ignored. Durations are a whole number and a unit
(`500ms`, `20s`, `5m`, `1h`); times of day are `"HH:MM"` in the lamp's own
zone, and a window `{ from, to }` may cross midnight.

The schema is the published form, a little stricter than the parser in
three places, which a package shouldn't rely on: the parser also takes a
leading `+` on a duration's number and an empty `[[trigger]]` list, and it
counts every length in bytes where the schema counts characters, so words
outside ASCII can pass the schema and still be too long on the lamp.

### Top level

| key | type | |
|---|---|---|
| `kind` | `"installable"` (default), `"addon"` or `"core"` | `core` comes with the lamp and only it may use the core grants. An `addon` only proposes (see "Add-ons") |
| `version` | string | `MAJOR.MINOR.PATCH` |
| `enabled` | bool, default `true` | |
| `needs` | list of strings | what the skill needs of the lamp (for the Store's listing) |
| `tool` | `[a-z][a-z0-9_]{0,47}` | a function the model may call to run the steps. Its description is `SKILL.md`'s, at most 1024 bytes |
| `still` | 1 to 256 bytes | with `tool`: what the model is told while a run that holds no pose is under way |
| `most_a_day` | 1 to 24 | the most runs it starts itself in a local day |
| `[permissions]` | table | what it may read and do: see "Permissions" |
| `[trigger]`, `[[trigger]]` | table, or a list of them | when it starts itself |
| `[[while]]` | list | conditions that must hold for a trigger to start it |
| `[ask]` | `{ policy }` | the ask policy a trigger's run is held to. Required with a trigger, unless an add-on or a core `sleep` skill |
| `[choice]`, `[flag]` | | the tool's one argument: a named value, or yes or no |
| `[again]` | `{ within, if_heard, told }` | when the tool is called again soon after it ran |
| `[quiet]`, `[active]` | windows | local hours when it never starts itself, or the only ones it may |
| `[budget]` | `{ group, per_hour 1–6 }` | runs an hour shared by every skill of the group |
| `[rest]` | `{ after_ignored 1–5, within }` | rests until the next morning after runs nobody noticed |
| `[stop]` | `{ tool, about }` | a function the model calls to snooze it or turn it off |
| `[search]` | `{ looks 1–8, give_up, found, note, walk, looked }` | a search a person asked for, its looks and what the model is told past the last (look-for-things) |
| `[history]` | `{ topic = "room/presence", within, words }` | the room's presence over the last hours, for the model (who-was-here) |
| `[watch]` | `{ every, asked_within, if_heard, most, mark, words }` | a watch the skill's tool turns on and off, with a faint `mark` light while on (guard-mode). Needs `light`, `messages` and `known_faces`; no trigger and no steps |
| `[[do]]` | list | the steps |

A skill needs a trigger or a tool, and at least one step. It says, sounds
or cues at most once a run.

**The lamp's own tables.** `[unplug]`, `[onboarding]` and `[startle]`
need a core grant (`prepare_unplug`, `onboarding`, `startle`), so only a
`kind = "core"` skill may have them. `[limits]` (needs `output_limits`),
`[news]` and `[reminder]` (each with its `say = { from = … }`) are used
only by the lamp's own skills today. Each is checked by the part that reads
it, and the schema doesn't look inside them. They aren't part of the public
format.

### Triggers: the events

A trigger names a bus topic and a test of it. The topics a public skill
uses are the protocol's events, read where they come from:

| protocol event | the trigger | grant |
|---|---|---|
| `person.arrived` | `topic = "humans/faces/tracked"`, `when = "appears"` | `look` (today: the topic) |
| `person.left` | the same, `absent_for = "…"` | `look` (today: the topic) |
| `person.stood_up` | `topic = "room/stood_up"` | `look` (today: the topic) |
| `person.facing` | (no trigger yet) | `look` |
| `phrase.heard` | (no trigger yet: a tool's `[again].if_heard` reads `voice/heard`) | `hear` (gap) |
| `name.heard` | `topic = "name/heard"` | `hear` (today: the topic) |
| `sound.heard` | `topic = "ears/classes"`, a `field` test on the class | `sound` (today: the topic) |
| `time` | `topic = "clock/local"` with `field` and `during` | none |
| `lamp.changed` | `topic = "voice/state"`, `lamp/pause`, … | none |
| `touch` | `topic = "controls/touch"` (night-sleep's wake, and a `[[while]]`'s `broken_by`) | none |
| `button` | `topic = "controls/gesture"` | none |

Keys of a trigger:
- `topic` (required) and a test:
  - `field` (a dot path into the message) with one of `equals`, `above` or
    `below`;
  - or, on `humans/faces/tracked` alone, `present` or `near` (0 to 1, the
    face's share of the picture's height).
- `when`: `appears` (the test turns true), `present` (it is true;
  default), or `changes` (with `changes`, 1 to 8 field paths).
- `name` (`[a-z0-9_]{1,64}`), for a step's `on` and for `yields_to`.
- `each` and `key`, together: one start per value of `key`.
- Timing: `for` (the test holds this long), `absent_for`, `gap`,
  `cooldown`, `wait` (at most 1 h), `rise = { by, within }`,
  `rearm` (`"leaves"`, `"ends"`, or `{ above | below, for }`).
- `during` (a window), `once_per_day` (with a `name` and `during`),
  `unless_heard`.
- `yields_to = { skill, trigger }`: it gives way to another skill's
  trigger.
- `sooner = { topic, field, above, for }`: a shorter `for` while that
  topic's field is above the line, on a trigger with a `for`. It is kept
  only with `LAMP_YAWN_SOONER=on`; off, it is read, checked and ignored
  (#3709).

A `[[while]]` condition takes `topic`, `field`, `equals`, `not_equals`,
`above`, `below`, `for`, `present = true` or `faces_at_most` (1 to 8) on
`humans/faces/tracked`, `unknown = "holds"`, and `through` with
`broken_by`.

### Steps: the actions

Each `[[do]]` is one step: one of the kinds below, and the modifiers
`on` (a trigger's `name`), `after = "say"` (it waits for the say before
it) or `if_choice` (the tool's values it runs for).

| protocol action | the step | grant |
|---|---|---|
| `gesture` | `body = { gesture = "nod" }`, or a map from the choice's values to gestures | `body` |
| `play` | `body = { move = "NAME" }`: `moves/NAME.json` | `body` |
| `look` (`at = "face"`) | `body = { look_at = "face" }` | `body`, and `humans/faces/tracked` in `topics` (the protocol asks `body` and `look`) |
| (none) | `body = { mood = "reading", for, toward }`: the reading pose | `body` |
| `light` | `light = "NAME"` (`lights/NAME.toml`) or `light = { colour, level 1–5, for ≤ 10m, pattern, … }` | `light` |
| (none) | `show = "NAME"`: `shows/NAME.toml`, 2 to 30 frames, 60 s at most | `light` |
| `express` | (none yet: a feeling is the `feelings` skill's, #3691) | `body` and `light` |
| `say` | `say = "a note the model speaks from"`, `say = { note = "…" }`, or `say = { text = "said as it is" }`, each with `priority` (0 to 8) and `expires` (1 s to 1 h) | `speak` |
| `sound` | `sound = "NAME"`: `sounds/NAME.wav`; not with a tool | `speak` |
| (none) | `cue = "hum"`: the lamp's own hum; not with a tool | `speak` |
| (none) | `look = true`: the full look's picture to the model; tool only | today's `look` (see the gaps) |
| (none) | `rest = { op = "rest"\|"wake"\|"cancel", during, resettle }` | `body` |

Light values are the protocol's: colours `yellow`, `red`, `green`,
`blue`, `cyan`, `purple`, `orange`, `pink`, `white`, `warm`, levels 1 to 5,
patterns `breathe`, `candle`, `pulse`. A light never blinks or flashes.
A skill's light may last up to 10 minutes, while the protocol's `light` is
at most 30 s (`for_ms`) and is kept by renewing it. Both bounds are right
for what they are: a skill's light is the lamp's own rule, and a computer's
light must lapse if the computer goes. So the 10 minutes is no bound of
the protocol's.
Gestures are the body's clips that a skill may name ([control.md](control.md),
"Gestures"): the schema holds a gesture to those 19, so a misspelt one is
caught before install.

`say = { from = "schedule" | "tasks" | "onboarding" }` and
`requested_audio` are core.

### Add-ons

A skill of `kind = "addon"` never acts: each of its steps is a `propose`,
which the lamp's judge takes or drops ([addons.md](addons.md)). It needs a
`version`, a trigger and `[permissions] propose`, may read only the
add-on topics (summaries, never what a person said), and has no tool and
no output permission.

```toml
[[do]]
propose = { act = "light", why = "evening", sure = 0.7, holds = "20s",
            light = { colour = "warm", level = 2 } }
```

- `act`: `say`, `light`, `move`, `mood` or `note`;
- `why`: one of `music`, `laughter`, `evening`, `morning`, `arrived`,
  `left`, `quiet`, `long_sitting`, `mood`;
- `sure`: 0 to 1;
- `holds`: 1 to 30 s;
- the act's own part: `light`, `gesture` (`nod`, `curious`, `stretching`),
  `mood` (`energy`, `warmth` each ±0.1, and `for`) or `note`.

## Permissions

`[permissions]` is the skill's manifest. A skill without it may do
nothing. It is read at load, where a skill that reads or does more than
it declares is not loaded and the journal says what it lacks, and again
at every call where the call leaves `lamp-skills`
([skills.md](skills.md), "Permissions"). Before install, the app and the
Store show it as plain words.

```toml
[permissions]
body  = true                      # gestures, moves, the face look, the reading pose, rest
light = true                      # light steps and shows
speak = true                      # say, sound, the hum
topics = ["humans/faces/tracked"] # every topic its triggers and conditions read
```

**The protocol's grants, and what format 1 has for each:**

| grant (control.md) | format 1 | shown before install | enforced on the lamp |
|---|---|---|---|
| `body` | `body = true` | "moves the lamp's body" | yes: at load for every body and rest step, and at the body's sockets |
| `light` | `light = true` | "changes the light" | yes: at load, at `light/request` and the actions part's light functions |
| `speak` | `speak = true` | "speaks" | yes: at load for say, sound and cue, and at the say service |
| `look` | presence topics in `topics` (`humans/faces/tracked`, `room/stood_up`) | "knows when someone arrives" | yes, as topics: a trigger or condition on a topic not listed doesn't load |
| `hear` | `voice/heard` or `name/heard` in `topics` | "hears its phrases" | partly: the topic is checked, the phrases aren't (see the gaps) |
| `sound` | `ears/classes` in `topics` | "hears knocks, music…" | partly: the topic is checked, the classes aren't (see the gaps) |

Format 1 also has, beyond the protocol's six:
- `topics` and `actions`: raw topics and the actions part's functions;
- `messages` and `known_faces`: a watch's;
- `who`: Welcome back's note told who is at the lamp;
- `output_limits`: private light and volume ceilings;
- `say = "quoted"`: an add-on's grant only, with `propose` naming `say`, for a proposed say's words;
- `propose`: an add-on's acts;
- `core`: `onboarding`, `prepare_unplug`, `sleep` and `startle`, for core skills only.

### The gaps

What the protocol's grants say that format 1 doesn't yet enforce. Each
gap's code change is its own follow-up PR.

1. **`look` means two things.** Here, `look = true` gives the model the
   full look's picture (`do.look`). The protocol's `look` is presence
   only, "positions, never who". A public format must not name a camera
   picture with the word the protocol uses for presence. Format 2 should
   rename today's key (to `picture`, say) and make `look` the presence
   grant over the presence topics.
2. **`hear` is a topic, not phrases.** A skill granted `voice/heard`
   could match any phrase. The protocol's `hear` grants only the
   package's own listed phrases (`hear = ["good night lamp"]`), matched on
   the lamp, never the words around them.
3. **`sound` is a topic, not classes.** `ears/classes` gives every class.
   The protocol's `sound = ["knock"]` gives only those listed.
4. **The grant names aren't the keys.** `look`, `hear` and `sound` have no
   `[permissions]` key of their own. Format 2 adds them, each mapped onto
   its topics, so the install screen shows the protocol's words and not
   raw topic names.
5. **`night`** (may speak in quiet hours, Nine's §7) isn't a permission.
   Today `[quiet]` and `[active]` are the skill's own choice, and the
   voice's quiet hours hold every say.
6. **`net`** is for script skills, a later format.
7. **No `format` key.** A build can't refuse a newer format by name until
   format 2 adds it, so format 2's first change is the key, and a build
   that reads it refuses any `format` it doesn't know.
8. **No check without installing.** `lamp-skills install` loads a skill
   before it installs it. A `lamp-skills check DIR` that loads and says
   why, and changes nothing, is what the SDKs' `lamp skill check` would
   call on a lamp.

## Checking a skill

`lamp skill check` (the SDKs, phase 2) and the Store hold a package to the
schema for its shape, and a lamp holds it to `lamp-skills`' load check for
the rest:
- the rules between keys;
- that the permissions cover every read and call;
- the moves' bounds and the sounds' form.

On a lamp, `lamp-skills install DIR` runs the same load before it
installs, and refuses with why a skill wouldn't load. There's no check
that doesn't install yet: that's gap 8.
