# The skill folder

```
<name>/
  SKILL.md      the standard (Agent Skills): name, description, how the lamp talks about it
  skill.toml    what it needs, its trigger, its conditions, the Policy's question, its steps
  tests/        a folder for each case: recordings and expect.toml
  lights/       optional: named light steps (lights/NAME.toml)
  sounds/       optional: short sounds (sounds/NAME.wav)
  moves/        optional: the skill's own moves (moves/NAME.json)
```

The runtime (`lamp-skills`) reads this; its schema is in the lamp's
`lampOS/docs/skills.md`, "The schema, as `lamp-skills` reads it", and that is
the truth. **A key the runtime does not know refuses the whole skill**, and
the runtime says why in its journal: write only the keys below.

Refused for now, so never write them: `run.py`, a `tools` key (the key is
`tool`, below), `when = "leaves"` or `"changes"`.

## `SKILL.md`

```markdown
---
name: stretch-break
description: When someone has been at the desk a long time without getting up, the lamp stretches once, as an invitation to take a break; never a lecture.
---

# Stretch break

What the lamp does and why, in two or three sentences, as the owner would
say it. How the lamp talks about it if asked ("I stretch when you've been
sitting a long time; tell me to stop if you'd rather I didn't").
```

`name` is the folder's name: lowercase letters, digits and hyphens. The
description is always in the lamp's instructions; keep it to one line.

## `skill.toml`, a whole example

```toml
needs = ["eyes", "body"]            # the parts it uses: eyes, ears, air, voice, body, light, ...
kind = "installable"                # an owner's skill is always installable; core ones come with the lamp
version = "1.0.0"                   # MAJOR.MINOR.PATCH; a package needs it

[permissions]                       # what it may read and use, and nothing else
topics = ["humans/faces/tracked", "voice/state"]   # every topic its trigger and conditions read
body = true                         # the look at the face and the stretch

[trigger]                           # what starts it
topic = "humans/faces/tracked"      # on faces, no field: it holds while a face is in view
when = "present"                    # present (the default) or appears
for = "50m"                         # it has held this long
gap = "2m"                          # a dropout shorter than this does not break it
rearm = "leaves"                    # once each time it holds; again only after it stopped
cooldown = "90m"                    # never again within this long

[[while]]                           # conditions that must also hold, all of them
topic = "voice/state"
field = "state"
equals = "resting"                  # not in the middle of a conversation

[ask]                               # required: the Policy's question before anything is done
policy = "nudge"                    # hello | news | nudge

[[do]]                              # steps, in order
body = { look_at = "face" }

[[do]]
body = { gesture = "stretching" }
```

### What it may use (`[permissions]`)

**A skill without `[permissions]` may do nothing**, and the runtime refuses
one that reads or does anything its permissions do not give, saying what it
lacks in its journal. Give exactly what the skill uses, no more: the owner
sees the list (`lamp-skills list`), and so will the Store.

| Key | Gives |
|---|---|
| `topics` | The topics it reads: each one a trigger or a `[[while]]` names, `humans/faces/tracked` for `body = { look_at = "face" }`, and `voice/heard` for a tool's `[again]` |
| `speak` | `say` (a note or `{ from = ... }`) and `sound` |
| `light` | `light` steps |
| `body` | `look_at`, `gesture`, `move`, `mood` |
| `look` | `look = true`: the camera's picture goes to the model |
| `actions` | The actions part's functions called by name. No step calls one yet: leave it out |

`kind` is `installable` for every skill an owner makes (leave it out, or
write it). `core` is for the skills that come with the lamp; the runtime's
`lamp-skills install` refuses one.

### The trigger (`[trigger]`)

| Key | Meaning |
|---|---|
| `topic` | The topic it listens to (only those in [senses-and-actions.md](senses-and-actions.md)) |
| `field` with `equals`, `above` or `below` | What a message must show: `field = "co2_ppm"`, `above = 1200`; `field = "state"`, `equals = "resting"`. A number is compared as a number, anything else exactly. **Required on every topic but `humans/faces/tracked`**. Only a field at the top of the message: one inside another (`schedule/due`'s `oldest.due_unix_ms`) cannot be named |
| `when` | `present` (the default): it holds, and has held `for`. `appears`: it holds where it did not just before |
| `for` | It has held this long |
| `gap` | A dropout shorter than this does not break `for` (a face lost as the person looks away) |
| `absent_for` | With `appears`: it did not hold for at least this long before |
| `rearm` | `leaves` or `ends` (the same): once each time it holds, again only after it stopped. This is "don't nag": once per sitting, once per stuffy spell. A chance held back by a condition (unless it has a `wait`), the cooldown or the Policy is spent |
| `cooldown` | Not again within this long. Always set one |
| `wait` | With `rearm` or `appears`: if a `[[while]]` holds it back (someone talking), it waits this long while the trigger still holds and acts once the conditions hold, rather than giving up its turn. A creature waits for a pause: `wait = "10m"` |

Durations: a whole number and `ms`, `s`, `m` or `h` (`"10m"`).

### Conditions (`[[while]]`)

Each is a `topic` with `field` and `equals`/`above`/`below`, or
`topic = "humans/faces/tracked"` with `present = true` ("someone is at the
desk"). All must hold, on each topic's newest value while that value lasts
(a face lasts 2 s). The runtime adds no conditions of its own: the pause and
a person speaking are the Policy's gate (#939).

### Asking first (`[ask]`)

Required with a trigger. `policy` names the kind of thing: `hello` (greeting
someone), `news` (telling them something), `nudge` (an unprompted suggestion,
spoken or not). A skill started by its `tool` has none: a person asked.

### Started when a person asks (`tool`)

When the owner wants the lamp to do something **when they ask for it**
("check my work", "do a little dance"), the skill has a `tool` in place of a
trigger: the model calls it when what the person says matches the skill's
description.

```toml
needs = ["eyes", "voice", "light"]
tool = "check_my_work"              # a lowercase letter, then lowercase letters, digits and _, at most 48

[permissions]
speak = true
light = true
look = true

[[do]]
light = { colour = "white", for = "20s" }
look = true                         # the voice's full look at what is in front of the lamp
say = "Tutor: a hint or a question, never the answer."
```

- The `SKILL.md` description is what the model reads to decide when to call
  it: say **when to call**, in the person's words, at most 1,024 bytes.
- No trigger, no `[ask]`. Its `[[while]]` conditions still hold it back.
- **A tool takes no arguments.** "Focus for 25 minutes" cannot pass the 25:
  a number the person says is for the voice's own tools (a timer is the
  voice's `set_timer`), not a skill's.
- One `[[do]]` list: a skill cannot do one thing when asked and another when
  something later happens. That is two skills, and the second needs a
  trigger that can tell its moment apart (see `schedule/due` in
  [senses-and-actions.md](senses-and-actions.md)).
- `say` in a tool's run is not spoken by the say service: it goes back to
  the model with the call's answer, as what to do next. `look = true` (only
  in a skill with a tool and no trigger) has the model say "Let me take a look." and the
  voice hand it the picture.
- While its last run is still under way (a light's `for`), a call again
  gets the look and the say again and holds the run's poses again, but
  nothing more is made: the light already on stays and is given back at its
  `for`. Only a skill whose steps are all body and light refuses it then
  ("I'm still doing that.").

### Steps (`[[do]]`)

In order. Within one table: body, then light, then say, then sound.

| Key | Meaning |
|---|---|
| `body = { look_at = "face" }` | Turn to the largest face in view |
| `body = { gesture = "..." }` | One of `nod`, `headshake`, `greeting`, `laugh`, `shy`, `stretching` |
| `body = { move = "NAME" }` | The skill's own move, `moves/NAME.json` (below), played by the body as a gesture |
| `body = { mood = "reading", for = "20s" }` | A pose held for `for` (more than 0, at most `2m`): `reading` is the only one, the head down over the desk and the arm leaning forward (#1011). The run lasts while it is held. Both keys or neither |
| `light = { colour = "...", level = 1..5, for = "4s" }` | The colour, then stepped to `level` from the level shown; `for` holds it in a run with no say. Given back when the run ends |
| `light = { dim = 1, until = "ends" }` | Dimmer by N steps. Held until the run ends (the say ends, or its longest `for`), then given back; with `until = "ends"`, only in a skill with a trigger and no `tool`, held instead until the trigger stops holding, then given back if the light is still as it was left (#1021) |
| `light = "NAME"` | The same keys from `lights/NAME.toml` |
| `say = "..."` | A note for the voice to say in its own words through its say service (#954), never a script or a number to compare. One a skill. The other steps are made when the voice begins the say, so the look and the light come with the words, and none if it never begins. In a tool's run, the note goes to the model instead |
| `sound = "NAME"` | Plays `sounds/NAME.wav`, through the voice (its say service, `LAMP_SAY=on`), so it never overlaps speech. The file must be a regular file (not a link), a WAV of 16-bit mono PCM at 24 kHz, at most 3 s and 148,140 bytes, and no 10 ms of it louder than -30 dBFS RMS and no sample past -12 dBFS (well under the lamp's speech), or the skill is refused with why. A lamp not at rest (paused, muted, speaking, listening, owing an answer, a reminder due, within 15 s of its last turn) does not play it, nor a second sound within 30 s, and never plays it later: `skills/events` says why. Keep it rare, short and soft, a chime or a tone, as a living thing would make. One say or sound a skill; the other steps are made when it begins. Not in a skill with a `tool` |
| `look = true` | Only in a skill with a `tool` and no trigger: the voice's full look (above) |

**The light is always given back.** Before a run the light shown is read
from `light/state`; at the end of the run (the say ends, or the run's
longest `for`) it is set back. If nothing said the light in the last 3 s,
the light step is left out: a skill never leaves someone's light changed. A
light lasts at most 10 minutes, so "a calm light while I focus for 25
minutes" cannot be a skill's. Two skills, or a person, changing the light
meanwhile are not arbitrated yet (#940).

## `moves/`

One move a file, `moves/NAME.json`, in the body's own format, its `name` the
file's:

```json
{"name": "wiggle", "step_ms": 750, "frames": [[0, 0, 0, 0, 0], [4, 0, 2, 6, 6], [0, 0, 0, 0, 0]]}
```

Each frame is offsets in encoder counts from the pose the standing lamp is
in: head pitch, head roll, base yaw, shoulder, elbow, `step_ms` apart. The
runtime refuses the skill, and says why, unless the move is what the body
takes: a name of 1 to 32 of `a-z`, `0-9`, `-`, `_` that is not one of the
body's gestures (nor `called`); `step_ms` 40 to 1000 and 1.5 to 10 s in all;
the first and last frame all zeros; no joint farther than 200, 200, 150,
350, 350 counts, nor faster than 900, 1,300, 1,000, 800, 800 counts a
second. The body plays it only on a standing lamp, never while it is paused,
held or slumped, and refuses it whole if a frame would tip the lamp past V1's
22 mm from the pose it is in: keep moves small. The file is a regular file
of at most 256 KiB. The body keeps 64 move names a run; an edited move of the
same name replaces the old one. A test expects it as
`call = "body.move"`, `args = { move = "NAME" }`.

## `tests/`

See [testing.md](testing.md).
