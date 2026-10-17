# Physical skills

A physical skill is something the lamp can do in the room: it notices
something with its senses and answers with its body, its light and its
voice. A skill made for Claude Code or Codex acts in software; a lamp skill
acts in the physical world. This document is the design, settled with the
owner on 2026-10-16 between 06:45 and 07:40.

The owner's direction, in order:
- Build the most capable skill first; safety and security for skills from
  other people come later, before anything is published.
- Owners build skills for their own lamp first, by talking to it. Publishing
  to the world comes much later.
- Prove the framework with a first set of skills that work well. Many
  behaviours built into V2 should become skills: "greeting users when they
  are back at their desk as a physical skill rather than an ad hoc
  implementation".
- Use the existing standards (Agent Skills' `SKILL.md`, MCP for tools)
  rather than inventing a format.

Decided the same morning: developers may publish skills from launch, listed
only after our review, and skills are free at launch. Neither happens before
the first skills below work.

## What V1 did

V1 had 30 skills in `autonomous-os/skills/`, each a `SKILL.md` with an
optional `skill.json` naming the capabilities it needs (`presence`,
`motion`, `light`, `expression`, `audio`). Read on 2026-10-16:

| V1 | Verdict |
|---|---|
| `SKILL.md`, the same format as OpenClaw and Claude | Keep |
| `skill.json` capabilities: one skill runs on any body that has them | Keep |
| The model reached the hardware by writing `[HW:/led/off]` into its reply, or `curl` to the hardware server; regexes repaired the markdown the model mangled | Drop: skills call services and read topics |
| Every sensing event, schedule and skill update went into the one agent conversation as text (`[sensing:presence.enter] …`); prose in the lamp's character decided which skill answered | Drop: a skill's trigger is a subscription to a topic, decided in code |
| Thresholds and cooldowns written in prose for the model to compare (wellbeing's 45-minute hydration, a music cooldown left at its development value) | Drop: numbers live in the skill's data and the runtime applies them |
| The OS owns polling, thresholds and cooldowns; context is computed before the model sees it (the environment skill, `[wellbeing_context]`) | Keep: V1's cleanest contract |
| The ambient loop and the agent both wrote the LEDs; a lock was pushed through monitor events | Fix: requests carry who and how important, and the part that owns the light decides |
| `skill-creator`, with runs with and without the skill and a grader | Keep, taught the lamp's topics and services |
| `make push-skill`: live on the next conversation, no reboot | Keep |
| A store (catalogue, browse, one-tap install, publish by pull request) | Later |
| No physical skill had a test | Fix: every skill carries tests on the lamp's own recordings |

## What V2 has

Senses are **topics** on the bus ([bus.md](bus.md), [msgs.md](msgs.md));
actions are **services**, one JSON line in and one out on a socket.

Topics a skill can read today: `humans/faces/tracked` (faces, a passing id,
whether turned to the lamp), `voice/heard`, `voice/said`, `voice/state`,
`room/state`, `name/heard`, `controls/touch`, `controls/gesture`,
`ears/level`, `ears/sudden`, `air/state`, `schedule/due`, `tasks/status`,
`body/joint_states`, `body/hand`, `light/state`, `policy/decisions`,
`lamp/pause`, `mood/state`.

Services: the actions part (light and volume, and the schedule tools;
`/run/lampos-actions/call.sock`), Schedule, Tasks (work handed to Claude
Code or Codex), Memory, the eyes (`permit.sock` for the camera, `who.sock`
for faces it knows), the body (`mood.sock`: its clips and a mood;
`gaze.sock`).

What is missing for skills:
- **No way to ask the voice to say something.** Each unprompted line (the
  hello, reminders, task news) is wired inside the voice by hand.
- **No arbitration.** The ring shows the newest light asked for, whoever
  asked; the body merges its inputs by fixed rules. bus.md's table says so
  ("Several requesters, one actuator: No").
- **`camera/image` is declared and published by nobody.**
- **The body plays its built-in clips by name (a skill may name its six
  gestures and, since #979, V1's kept moves) and a skill's own moves
  (`moves/`, below)**; nothing weighs them against Expression and the eyes.
- Expression's catalogue is data, but its triggers are a fixed list in code.

## The design

### A skill is a folder

```
welcome-back/
  SKILL.md        the standard: name, description, and how the lamp talks about it
  skill.toml      what it needs, its triggers, its steps, its tools
  moves/          body clips (data, played within the body's limits)
  lights/         ring patterns
  sounds/         short sounds
  tests/          recordings of the lamp's bus and what the skill should do in each
  run.py          optional: code for logic the data cannot say, given the bus
```

`skill.toml` for the first skill:

```toml
needs = ["eyes", "voice", "body", "light"]

[[trigger]]
topic = "humans/faces/tracked"
when = "appears"            # a face where there was none
absent_for = "5m"           # after at least five minutes with nobody
cooldown = "20m"

[ask]                       # anything the lamp starts itself asks the Policy first
policy = "hello"

[[do]]
body = { look_at = "face" }
light = "warm_glow"
say = "They are back at their desk. Greet them in one short line, by name if you know it."
```

The numbers are in the data, never in prose. The runtime applies them.

### The schema, as `lamp-skills` reads it

The seventh builder wrote this on 2026-10-16 (#938) from the code in
`crates/skills`. It includes the fields the lamp skill creator proposed (Ten,
#947, `skill-creator/references/skill-folder.md`) that the runtime took.
Where the design above says nothing, the runtime does the smallest thing, and
each such choice is marked *(chosen)*. A skill that breaks any rule here is
not loaded and the program says why in its journal. The other skills still
load.

**The folder.** The folder's name is the skill's name: 1 to 64 lowercase
letters, digits and hyphens. A folder whose name begins with a dot is passed
over, and so is any file or case folder whose name begins with one: a folder
copied from a Mac carries a binary `._NAME` beside each file (lamp-4ace,
2026-10-16), which must not be read as a recording or reload the skill.

**`SKILL.md`** begins with Agent Skills' front matter between two `---`
lines. It has two required fields: `name:`, which is the folder's name, and
`description:`. The rest is for the model and for people; `lamp-skills` does
not read it.

**`skill.toml`.** A key not listed here refuses the skill *(chosen: a typo
must not leave a step silently undone)*.

| Key | Required | What it is |
|---|---|---|
| `needs` | no | The parts it uses (`eyes`, `ears`, `air`, `voice`, `body`, `light`, ...). Kept, not checked yet |
| `[trigger]` or `[[trigger]]` | yes, unless it has a `tool` | When the skill runs: one table, or several, each checked on its own |
| `tool` | no | A function the model may call, which runs the steps (below, "Tools for the model"). A lowercase letter, then at most 47 lowercase letters, digits and `_`. It is described to the model with `SKILL.md`'s description, which is then at most 1,024 bytes and says when to call it |
| `[choice]` | no | The tool's one argument the model picks from named values: `name` (a lowercase letter, then at most 31 lowercase letters, digits and `_`), `about` (what it means, told to the model, at most 256 bytes) and `one_of` (1 to 16 different plain names). The model may leave it out. Only with a `tool` |
| `[flag]` | no | The tool's one argument that is true or false: `name` and `about` as for the choice, a different name. Left out, it is false. Only with a `tool` |
| `[again]` | no | When the tool runs again soon after it ran: `within` (a duration) and `if_heard` (at least one entry, each one or more whole words; any case). Within `within` of the tool's last run, counted from when that run began, a call runs only if the newest thing heard on `voice/heard`, finished or not, its pieces joined as the voice joins them, began after that run began and has one of the entries among its whole words (an entry of two words, as "one more", in a row); otherwise nothing is done and the model is answered with the error `told` (at most 256 bytes; a general "you just did this" without it). A word is what is between characters that are not letters or digits, so "dance" is not in "guidance" nor "again" in "against". Nothing heard since counts as not asked. A refused call does not count as a run. With it, `lamp-skills` subscribes to `voice/heard`. Only with a `tool` |
| `still` | no | What the model is told, as an error, of a call while the tool's last run is under way, in a skill with no pose (`do.body.mood`) and no `do.look`: nothing is done, and it does not count as a run (at most 256 bytes; "I'm still doing that." without it). Only with a `tool` |
| `trigger.topic` | yes | The topic it listens to |
| `trigger.field` with one of `equals`, `above`, `below` | yes, except on `humans/faces/tracked` | What a message must show for the trigger to hold. A number is compared as a number, anything else exactly. On `humans/faces/tracked` without them, it holds while a face is in view |
| `trigger.when` | no | `present` (the default): it holds, and has held for `for`. `appears`: it holds where it did not just before, once each time. `leaves` and `changes` are refused for now |
| `trigger.near` | no | On `humans/faces/tracked` alone: only a face at least this share of the picture's height counts (Welcome back: 0.14, the voice's Arrival's line for someone at the desk) |
| `trigger.for` | no | It has held this long |
| `trigger.gap` | no | A dropout shorter than this does not break `for` |
| `trigger.absent_for` | no | `appears` only: at least this long before, it did not hold. At the program's start nothing has been away: someone there at the first message is not greeted |
| `trigger.rearm` | no | `leaves` or `ends`, which mean the same: once each time it holds, and again only after it stopped. Without it, `present` runs at every message once `for` has passed, as often as `cooldown` lets it. Or a line, on a trigger with a `field` (#1021): `{ below = N, for = D }` (or `above`): once each time it holds, and again only after the trigger's field has been below N, by the messages of its topic, for D since the last chance; a dip that does not stay past the line does not count, however often the trigger stops and holds again (Stuffy air: `{ below = 1000, for = "10m" }` on a trigger above 1,200). A chance held back by a condition, the cooldown or the Policy is spent as one that ran, unless `wait` keeps one held back by a condition; one that comes in the quiet hours is kept (`[quiet]`): the line waits for the chance taken |
| `trigger.cooldown` | no | Not again within this long of the last run. A run the Policy refused does not count |
| `trigger.wait` | no | With `rearm` or `when = "appears"`, at most `1h`: a chance held back by a `[[while]]` condition (someone talking) is kept while the trigger still holds, for at most this long, and taken at the trigger's first message after every condition holds again: the lamp acts once the talk ends, within the same sitting, instead of giving up its turn (#1021). It began waiting when the chance came, or for a chance kept through the `[quiet]` hours, when they end; the trigger stopping (past its `gap`) ends it. Without it, a chance held back by a condition is spent. A chance held back by `absent_for` or the cooldown is spent either way |
| `trigger.name` | no | What a step's `on` calls it: lowercase letters, digits and `_`, a different one for each trigger (#1021) |
| `trigger.each` with `trigger.key` | no | `each` names a field that is a list, and `key` the field that names one of its items (`each = "settled"`, `key = "task"` on `tasks/status`). The `field` test is then of each item on its own, and `when` must be `appears`: a run for each item that holds where, in the trigger's last message, it did not or was not in the list. So an item new to the list that holds counts (every item of #1048's `settled` is first seen already settled), and so does one that leaves and comes back. What is there at the first message since the program started is not news: it only says what is there *(chosen: a restart must not tell everything again)*. At a reload the items begin again from the topic's value the program keeps, so what was there then is not news and the next message, the next change, is compared with it (the reviewer on #1042); with no value kept yet, the topic's first message only says what is there, as at the start. A missing list is an empty one. Each item's run is asked about on its own, and two in one message are two runs, in the items' order across the skill's triggers: by number when the key is a number (task 9 before task 10), else by text. Not with `near`, `absent_for`, `for`, `gap`, `rearm`, `cooldown` or `wait`, which are timed over the whole topic *(chosen)*. Nor in a skill with `[quiet]` or a dim `until = "ends"`: an item's run has no quiet hours and no end of its own, so quiet would be passed over and a held dim would keep the skill busy *(chosen, the reviewer on #1042)*. With the Policy asked (`LAMP_SKILLS_ASK=policy`), its answer starts the item's run only if that item is still in the list and holds |
| `[[while]]` | no | Conditions that must also hold, all of them, on the newest value of each topic while that value lasts: a `topic` with `field` and one of `equals`, `not_equals` (anything else, #1140), `above` or `below`, or `humans/faces/tracked` with `present = true`; with `for`, it has also held that long, by the messages of its topic since this program started. It begins again after a value that expired, and when a reload changes the condition |
| `[quiet]` | no | `from` and `to`, two different local times as `"HH:MM"` (`from = "22:00"`, `to = "08:00"`, past midnight when `to` comes first): from `from` up to `to`, the skill never starts itself (#1021). A chance that comes then is kept, as `wait` keeps one through a talk, and taken at the trigger's first message after `to`, while the trigger still holds; the trigger stopping (past its `gap`) spends it (Stuffy air, stuffy from 23:00 to 10:00: one droop at 08:00, the reviewer on #1066). A trigger with `when = "appears"` spends it: someone who came at 22:30 is not greeted at 08:00. The local time is the program's wall clock and its `--zone` or `--utc-offset-minutes`; without either the time of day is not known, and a skill with quiet hours never starts itself *(chosen: "never at night" holds when the night cannot be told)*. Only with a trigger |
| `[ask] policy` | yes, with a trigger | The Policy's question asked before anything is done *(chosen: required, since every trigger is something the lamp starts itself)*. A tool's call is not asked about: a person asked for it |
| `[[do]]` | at least one | The steps, in order. Within one table: body, then light, then say, then sound |
| `do.body.look_at` | no | Only `"face"`. The largest face in view goes to the body's `gaze.sock` as the eyes send one (`{"face":{"x","y","size","turned":0}}`) |
| `do.body.gesture` | no | One of the body's clips, sent to `mood.sock` as `{"gesture":NAME}`: its six gestures (`nod`, `headshake`, `greeting`, `laugh`, `shy`, `stretching`) and the V1 moves kept as clips (#979: `acknowledge`, `curious`, `happy`, `caring`, `surprised`, `confused`, `sleepy`, `excited`, `scanning`, and the dances `dance_chill`, `dance_groove`, `dance_hype`, `dance_waltz`, `dance_jazz`), checked against the body's own table (`lamp_body::clips::ALL`); not `idle`, the body's own. Or a table with clips for each value of the `[choice]`, and only those (`{ calm = ["dance_chill", "dance_waltz"], jazz = ["dance_jazz"] }`): one is picked by the call's choice, from all of them when the call gives none. A clip (or a `move`) after another in the same run is sent when the body says on `body/gesture` that the one before is `over` ([body.md](body.md), "On the bus"): the body takes a gesture only while none plays, so one sent while the first plays is dropped (#999, lamp-06c1's ask 8), and when the body begins a clip is not known from the send (a standing lamp began one 280–350 ms after it, a rising one later) nor from `body/joint_states` (the lean as the lamp speaks moves them too): lamp-06c1's asks 19, 22 and 23 (#1199). `body/gesture` is subscribed to before each clip is sent. If the body says the clip before was `dropped`, nothing more is sent; if it says no `over` within that clip's length, 100 ms and 3 s more (a gesture waits up to 3 s for a rise), the next is not sent either, and the run's `done` says why: so without `body/gesture` (lamp-body without `LAMP_BUS=on`) a run plays only its first clip. A clip the body let go because it was pushed, or that ended as the run stopped, is said `over` too; a push or a pause still sends nothing more. Every step after it waits too; it is not sent, nor any after it, if meanwhile `body/hand` said a push or `lamp/pause` a pause. A folded lamp is stood up for it at any hour: the body has no idea of night or of Expression's sleep. Only the Policy's unprompted question ("not late at night", under `LAMP_SKILLS_ASK=policy` with the hour known) keeps a sleeping lamp down; with `skip`, nothing does ([body.md](body.md)) |
| `do.body.move` | no | A move of the skill's own: the file `moves/NAME.json`, in the body's move format ([body.md](body.md#moves-from-files)), whose `name` is `NAME`. Checked at load by the body's own check (`lamp_body::moves::check`): not one of the body's gestures, 1.5 to 10 s, `step_ms` 40 to 1000, beginning and ending at nothing, each joint inside the clips' size and speed. Sent whole to `mood.sock` as `{"move":{"name","step_ms","frames"}}`, where the body checks it again and plays it as a gesture (below) |
| `do.body.again_if` | no | The `[flag]`'s name: the gesture is played twice when the call sets the flag |
| `do.body.mood` + `do.body.for` | no | A pose held: only `"reading"` (#1011: the head looks down at the desk and the arm leans forward). Sent to `mood.sock` as `{"mood":"reading","level":0}` when the step is made and again every second for `for` (more than 0, at most `2m`); the body lets it go 1.5 s after the last, and comes back. One without the other is refused |
| `do.light` | no | A file in `lights/`, written as its name, or a table in place. A file `lights/NAME.toml` holds the same keys. `{ colour, level, for }` asks the actions part for `light_colour`, then steps the light to `level` (1 to 5) with `light_brighter` or `light_dimmer` calls, counted from the level `light/state` shows; with that level not known it is left and the run says so. `for` holds the light in a run with no say. `{ dim = N }` asks for `light_dimmer` N times. A light is held until the run ends (the say ends, the run's longest `for`, or its last clip ends, whichever is last) and is then given back. With `until = "ends"` (#1021), only in a skill with a trigger and no `tool` (a tool's run has no trigger to end it), a dim is held instead until the trigger that ran it stops holding (its test fails past its `gap`), or its topic's newest value expires (its header's `expires_ms`), or, when that value names no lifespan, its topic says nothing for 15 minutes, or a reload (the folder is looked at every 2 s) finds its skill removed, no longer dimming `until = "ends"`, or with no trigger at that place in the skill on the same topic; then it is given back, but only if the light is still as the run left it (on, the same colour, N steps under, or at the lowest): a light someone changed meanwhile is left as it is. The run stays under way meanwhile. A skill reloaded with the same trigger (its place in the skill and its topic) and still dimming `until = "ends"` keeps its hold, whatever else was edited: its end comes as before *(chosen: an edit is not an end, and a removed skill or trigger has nothing left to end its hold, the reviewer on #1066)*. With the Policy asked (`LAMP_SKILLS_ASK=policy`), the end is told whether or not it said `say_now`, and is let pass when no run holds anything; a `say_now` that comes after that end (the trigger's own, or a reload's) does nothing. With `LAMP_LIGHT_REQUESTS=on` it is asked of the ring instead, and the person's light is never touched (#940, below); a dim held `until = "ends"` is asked of the ring until its trigger ends, and then given back the same way |
| `do.say` | no | A note for the voice to say in its own words, through the voice's say service (#954). One a skill. The run's other steps are made, in their order, when the voice begins the say, so the look and the light come with the words, and none if it is never begun. A light changed is given back when the say ends. In a tool's run the note goes to the model in the call's answer instead. Or `{ from = "schedule" }` or `{ from = "tasks" }`: words the voice keeps itself and says when asked, the reminder due or the news of work handed over (#939, (c), the first builder's 12:32). The voice keeps the words and their delivery (a reminder's acknowledgement, news told once); the skill says when, and moves and lights with them as with a note. Sent to the say service as `from` in place of `note`, at priority 8 for `schedule` and 5 for `tasks`. Any other `from`, another key beside it, or a `from` in a skill with a `tool` refuses the skill: what the voice keeps is said when a trigger starts the skill, never from the model's call (Fourth on #1033) |
| `do.sound` | no | A file in `sounds/`, written as its name: `sound = "ding"` plays `sounds/ding.wav` (#938). Checked at load by the voice's own check (`lamp_voice::say::read_sound`), and the skill refused with why: a regular file, not a link, of at most 148,140 bytes; a WAV of 16-bit mono PCM at 24 kHz (the speaker path's rate), 1 frame to 3 s; no 10 ms of it louder than -30 dBFS RMS (well under the lamp's speech) and no sample past -12 dBFS. Played by the voice, through its say service (`LAMP_SAY=on`) as `{"sound": PATH}`, the file's absolute path; the voice reads it again, only from under the skills folder, and plays it as a turn of its own, so it never overlaps speech ([speaking-first.md](speaking-first.md), the say service). A lamp not at rest as a say needs it (paused, muted, speaking, listening, owing an answer, a reminder due, within 15 s of its last turn) does not play it, nor a second sound within 30 s, and the voice refuses it at once, never later; the run's `done` says why (`sound: refused (the lamp is speaking)`). One say or sound a skill: the run's other steps are made when the voice begins it, as with a say. Refused in a skill with a `tool`: a tool is called while the voice answers |
| `do.after` | no | `"say"`: this step and every one after it are made when the say ends, not as it begins: the skill's own say in an earlier step, which ends with its outcome, or in a skill with a `tool`, the line the model says after the call, which ends when `voice/state` is `resting` or `listening` again. A line that ends in `listening` was cut, the person speaking over it: then none of the steps after it is made, and the run's `done` says "the line was cut". `thinking` after `speaking` is still the line, since its sound plays on (lamp-06c1, 2026-10-16: 20 ms of `speaking`, then 15 s of `thinking`, through a line of about 2 s), so that wait ends at most 4 s after the speaking began. The run waits at most 5 s for the speaking to begin, and goes on at once if `voice/state` was not said within 3 s. Not after a say never begun. A light changed is then given back when the last clip ends |
| `do.on` | no | A trigger's `name`: the step is made only in a run that trigger started (Task news: `happy` for done, `caring` and a dimmer light for failed). A step without it is made in every run. One say for each run: one among the steps without `on`, and one for each trigger with its own. Not with `after` in the same skill *(chosen, until a skill needs both)* (#1021) |
| `do.look` | no | `true`: the voice's full look (#836). Only in a skill with a `tool` and no trigger: the call's answer carries `"look": true` and tells the model to say "Let me take a look."; lamp-skills makes the run's body and light before it answers, and the voice takes its full look for that turn itself as the answer's first audio plays, whatever the model says, its still in the model's next turn, or a note that it could not see if none comes (#941) |

A duration is a whole number and one of `ms`, `s`, `m` or `h` (`"5m"`).

The ask is the only gate. The runtime adds no conditions of its own, such as
the pause or someone speaking: per the design above, the Policy is the one
gate for everything the lamp starts itself (#939). Until that gate exists, a
skill that must keep quiet in those cases says so with a `[[while]]`.

**`moves/`** holds the skill's own moves, read only as `do.body.move` names
them. The body plays one as it plays its own gestures: on a standing lamp
(a folded one stands up for it first), with the same smoothing and speed
limits, refused whole by V1's anti-tip rule when a frame would take the
centre of gravity past 22 mm off the base's axis (the journal says
`anti-tip: REFUSED gesture NAME`), and dropped, with the reason in the
journal, while the lamp is paused, held by a hand or slumped. A skill never
writes an arm goal of its own. Whether a move may play from the pose the
lamp is in is known only to the body, so a skill that loads may still have
its move refused there. **`sounds/`** holds the files `do.sound` names;
the others are not read. A **`run.py`** refuses the skill, because nothing
runs it yet.

**`tests/`** has one folder for each case. A case holds the `.jsonl` files
`lamp-bus record` wrote and an `expect.toml`. The expect file lists, as
`[[expect]]`, every call the case may make, in order, or says `none = true`:

    [[expect]]
    at = "600s"                # optional: when, from the case's first message
    within = "1s"              # optional; without it, exactly
    call = "body.gaze"         # body.gaze, body.gesture, body.move, body.mood, light, say, sound, look or ends
    args = { look_at = "face" }

A call matches when its name is the same and each argument named in `args`
is the same. Arguments it leaves out are not compared. An argument written
as a list matches any one of its values, for a clip picked at random
(`args = { gesture = ["dance_chill", "dance_waltz"] }`). The arguments the
runtime gives are:

- `body.gaze`: `look_at`, `x`, `y`, `size`
- `body.gesture`: `gesture`
- `body.move`: `move` (its name)
- `body.mood`: `mood`, `for_ms`
- `light`: `colour`, `level`, `for`, or `dim`, `until`
- `say`: `note`, or `from` for what the voice keeps (`say = { from = … }`), and `task` when an item's run names its task (#1237)
- `sound`: `sound`, the name in `sounds/`
- `look`: none
- `ends`: none; the trigger of a run with `until = "ends"` stopped holding,
  or its topic's newest value expired or went silent, or a reload found its
  skill or trigger gone, so what it held is given back. In a replay, an expiry or a silence is seen at the first
  message of any topic after it

A case's local time is not known unless its expect file sets the time of
its first message, as `clock = "10:00"`; a skill with `[quiet]` hours makes
no call in a case without it.

A skill a person starts by its `tool` has the model's call in its case:

    [[call_tool]]
    name = "dance"
    at = "0s"                  # from the case's first message
    args = { style = "calm" }  # optional: the tool's [choice] and [flag]

`[[call_tool]]` with `name`, `at` and, if the tool takes them, `args`
makes the call after the messages recorded at or before that time, its
arguments, its conditions and its `[again]` judged on them as the program
would; a call refused makes nothing. A case that is only a call needs no
recording. Its calls are listed like any others, the say's note among
them; whether a step waits for the say is the program's, tested on its
own. Each call is a fresh run: what the program does with a call while the
last run is still under way (the look and the note again, the pose held
again) is not replayed. Desk tutor's `a-page-held-too-far-is-asked-closer`
and Dance's four `dance-*` cases are such cases.

A case is replayed in the program, not on a bus. Every message of every file
goes in the order of its `at_us` (the publisher's clock), with the ask
skipped. Any call not listed fails the case. A case with no recording yet,
and no `[[call_tool]]`, is counted and not run.

`lamp-skills test SKILL_DIR` loads a skill and runs its cases.
`cargo test -p lamp-skills` loads every folder in `lampOS/skills/` and runs
its recorded cases, so CI checks every skill. To make a case, record the
topics while the thing happens, or publish it by hand with
`lamp-bus pub ... --at-us`. Welcome back's first two cases were made that way
(its third was recorded on lamp-06c1, #995):

    lamp-bus record skills/welcome-back/tests/CASE humans/faces/tracked --seconds 40 --root R &
    lamp-bus pub humans/faces/tracked latest '{"faces":[]}' --type lamp_msgs/Faces --at-us 0 --root R

### `lamp-skills`, a new part

Its own process, its own owner, like every part
([build as components](architecture.md)):
- loads every skill folder and listens to the topics its triggers name;
- applies the conditions, timers and cooldowns;
- before anything unprompted, asks the Policy "may the lamp do this now?",
  so Jev is the one gate for everything the lamp starts itself;
- runs the steps by calling the services;
- publishes `skills/events`: triggered, asked, answered, done, with why.

As built on 2026-10-16 (#938, `crates/skills`, not yet run on a lamp):

    lamp-skills [--dir DIR] [--bus-root DIR] [--gaze-socket PATH] [--mood-socket PATH] [--actions-socket PATH] [--say-socket PATH] [--tools-dir DIR] [--clock-control PATH] [--zone system|NAME | --utc-offset-minutes N]
    lamp-skills test SKILL_DIR...

- `--zone` or `--utc-offset-minutes` is the lamp's local time, as the
  Policy, Expression and memory take it, read with the wall clock at each
  message, for a skill's `[quiet]` hours (#1021). `--zone system` follows
  `/etc/localtime` (setup's zone, #820) and `--zone NAME` a named zone, each
  with its offset at that moment, so quiet hours stay at the same clock
  hours across summer time (#1384); `--utc-offset-minutes` (-720 to 840) is
  fixed all year. One or the other; a zone that can't be read ends the start.
  Without either, a skill with quiet hours never starts itself.
- It loads every folder in `--dir` (`/var/lib/lampOS/skills`), looks at the
  folder every two seconds, and loads them all again when a file other
  than a test changed. Cooldowns are kept across a reload.
- It subscribes once to each topic that a trigger or a `[[while]]` names,
  and to `humans/faces/tracked` for a skill that looks at a face, and keeps
  each topic's newest value. A look uses only a list of faces whose lifespan
  still lasts; with none, the other steps run without it. Time comes from its own monotonic
  clock, read when a message arrives.
- **The ask.** `LAMP_SKILLS_ASK` says how a run that a trigger starts is
  allowed (a tool's run is asked for by a person and never is):
  - `policy`: the Policy's gate (#958, `lamp-policy --gate`). The run's
    question goes on `skills/ask` as #958's `lamp_msgs::SkillAsk` (written
    as JSON until that type is on `lamp-voice`): the ask's number (this
    clock in µs, always above the last), the skill, the act (`[ask]
    policy` `hello`, `news` or `reminder`, anything else `unprompted`;
    a `reminder` is held by the Policy only while someone talks to the
    lamp) and, for a
    hello, `away_min` and `there_s` from its trigger. With nobody
    subscribed to `skills/ask` (the Policy down, or without `--gate`) the
    run is refused at once. The run waits for the `Decision` on
    `policy/decisions` that names the ask, looked for every 5 ms while one
    waits (face published to say asked: 6 to 13 ms in 10 runs on a Mac,
    the Policy answering at once): a `say_now` with `shadow: false` starts
    it; one in shadow (or without `shadow`),
    any other choice, or no answer within 2.5 s (the Policy's own 2 s and
    the bus) leaves it undone. On a `say_now` the run is made again from
    the newest values (a look at where the face is now), and dropped if its
    trigger no longer holds or its conditions fail (the person has gone).
    A skill with a dim `until = "ends"` is dropped too once its trigger's
    end has come meanwhile, its own or a reload's (#1309's post-merge P3):
    nothing would give that dim back.
    The trigger's cooldown counts from the ask.

    With `lamp-policy --act hello`, a `say_now` counts as the lamp's hello
    on the Policy's side and starts its 20-minute block, even when
    lamp-skills then does not greet: the answer came after 2.5 s, the
    person had gone, or the skill's last run was still under way. Until
    lamp-skills tells the Policy, such a hello is lost for those 20 minutes.
  - `skip`: yes without asking.
  - unset: no, and nothing is done.

  `answered` says which, and with the Policy its choice, the decision's
  `about` and who decided.
- **The steps.** `look_at`, `gesture` and `mood` are datagrams to the
  body's `gaze.sock` and `mood.sock` (default `/run/lampos-body/`), never
  waited for. A `mood` is said again every second on a thread of its own
  for its `for`, and the run lasts that long: a pose longer than a light's
  `for` keeps that light on until the pose ends too. A tool called again
  while its run is under way holds the run's poses again (and nothing
  else), so each look comes with the head down over the page. `light` is a `light_colour` call (or `light_dimmer` calls) to the
  actions part's `call.sock` (default `/run/lampos-actions/call.sock`). Each
  call is waited for up to two seconds.
- **`say`** goes to the voice's say service (#954, `LAMP_SAY=on` in the
  voice; `--say-socket`, default `/run/lampos-say/say.sock`), built against
  its interface as #939 agreed it, before #954 is merged: one line
  `{"skill","note","priority":5,"expires_in_ms":30000}` (or `"from"` and its priority in place of `"note"`), then a `begun`
  line when the voice starts the turn, then one outcome (`spoken`,
  `dropped`, `expired` or `refused`). A run waits for it in a thread of its
  own, one a skill at a time: a trigger while its run is under way is told
  in `done` as `not made: its last run is still under way`. A run of one
  item of a list (a trigger with `each`) is its own news instead: it waits
  for the run under way and starts when that one is done, in order, at most
  16 waiting (more are told as `not made: 16 runs already wait`). Its calls
  are made again as it starts, a look at where the face is then, and only
  if its item still holds; else it is told as `not made: its item no longer
  holds as its turn came`, and the next waiting is tried. An item's `say = { from = "tasks" }` ask names its task when the key is a number (`"task": N`), so the voice gives that task's news and no other (#1237). A `from = "schedule"` ask never carries it: the voice refuses `task` there. A light's `for`
  is at most 10 minutes. The other steps are made at `begun`; a say never
  begun moves nothing and lights nothing.
- **The light is given back.** Before a run, the light shown is read from
  `light/state`. When the say ends, however it ends, or after the run's
  longest `for` in a run with no say, and after its last clip ends, it is set back from the light shown then:
  off if it was off, else its colour again and its level stepped back to
  the one before. A dim at the lowest level that changed nothing is given
  back as nothing. If `light/state` has said nothing for 3 s, a light step
  is left out: a skill never leaves someone's light changed.
- **With `LAMP_LIGHT_REQUESTS=on`** (on `lamp-skills` and on the voice,
  whose ring takes them; #940), a skill never touches the person's light.
  Each light step is asked of the ring on `light/request` from
  `skill:<name>` (cut to 64 characters) at `skill` priority, on the host's
  boot clock (`--boot-id` on a development host): a colour at its `level`,
  or at the level the light has (3, the middle, when that is not known, so
  a skill that needs a level names it); a dim steps down from the light so
  far, never below 1, and is left out when no light is on. Each ask holds 15 s and is asked again every 10 s while the
  run keeps the light, the same while as above; at the end it is given back
  by asking with a time already passed, and the ring shows the person's
  light again by itself. A person who asks for their light meanwhile ends
  the glow at once (the ring's rule). With nothing taking `light/request`
  (looked at on each step, so a ring that starts during a run is asked from
  the next step), a light step is left out and `done` says the ring needs
  the switch. The
  actions part is not called for the light. `done` says
  `light warm: asked of the ring at level 5` and
  `light given back: asked again with a time already passed`.
- **With `LAMP_BODY_REQUESTS=on`** (#940), each gesture and move a skill
  sends on `mood.sock` says who asks:
  `{"gesture":"nod","source":"skill:<name>","priority":"skill"}`, and the
  same two fields beside `"move"`. The skill runner stamps them, never the
  skill file, so a skill can't claim more. A body with `--arbitration on`
  weighs them ([body.md](body.md)), so the conversation's gestures end a
  skill's move, and a skill's move is dropped while the conversation's
  plays. A body with its switch off takes the gesture and ignores the two
  fields (#1239), so the switches needn't go on together. Only a body older
  than #1239 refuses such a datagram (`not a mood or a gesture this body
  has`): switch this on where the lamp's body has #1239. Off, the datagrams
  are as before.
- `done` says how each step went: `look at the face: told the body`,
  `light warm: done`, `say: spoken`, `light given back to on blue at 3:
  done`, or why not.
- **A test's clock** (#1128, `crates/skills/src/clock.rs`). Every wait of
  the program's own is on one clock: the look at the folder every two
  seconds, the time a message arrives (so every `for`, `absent_for`,
  cooldown and `[again]` `within`), the Policy's 2.5 s, a call's wait for
  its body and light, the voice's line (`SPEAK_BEGIN`, `LINE_LONGEST`), a
  clip's length and its begin on `body/joint_states`, a light's or a pose's
  `for`, the pose said again every second and the light asked again every
  10 s. In production it is the host's monotonic clock. `--clock-control
  PATH` is for tests only: each line `{"skip_us": N}` on that socket moves
  the clock N µs ahead, answered with `{"now_us": ...}` once done, and the
  clock runs on from there at the host's pace. A skip that would take the
  total past a year is refused with `{"error": ...}`; a line is read up to
  8 KiB; an old socket at PATH is taken over, anything else there is left
  and the start fails. The main loop takes the skip:
  first everything already published to it, on the clock as it was, then
  the skip, then the asks to the Policy that are now late, then the
  Policy's answers that came since. The main loop is the only one that
  takes a skip, so one sent while it waits up to 800 ms for a look call's
  body and light (`MADE_BEFORE_ANSWER`) is taken when that wait ends, in
  real time. So
  `crates/skills/tests/skills.rs` skips where it would sit through a reload,
  an absence, a slow Policy or a clip, and keeps every assertion: a skip
  stops just before a wait another part must see end in its own steps (the
  stand-in body's), and real time does the rest. Two program tests run on
  the host's clock, without the flag, as a lamp runs: the reading pose said
  again each second for its 3 s, and a Policy given up at its 2.5 s; and
  `clock.rs` checks that without a test a sleep and a wait take their whole
  time. Stamps stay on the host's
  clocks, which the other parts read: `at_us` on `skills/events`, a light
  claim's `until_us`, and the freshness of what others said (`light/state`,
  `voice/state`, `body/joint_states`, from their `at_unix_ms`). So do the
  socket timeouts to the other parts (the say service, the actions part, a
  tool's caller), which bound a peer that keeps its own time.

`lamp-update install` puts it on a home lamp (#811): the product unit
`deploy/product/lamp-skills.service`, Welcome back in `/var/lib/lampOS/skills`,
and the product voice with the say service on and its own hello off.
`deploy/units/lamp-skills.service` stays the template for a lamp set up by hand.
The product unit passes `--zone system`, the zone setup saved (#820), across
summer time (#1384). lamp-4ace's passes neither yet, so there a skill with
`[quiet]` hours never starts itself. Neither puts Stuffy air in its
folder today (install puts only Welcome back on a home lamp).
lamp-4ace's folder carries `lamp-skills.service`, and its voice unit has the
voice's own hello (`LAMP_SPEAK_FIRST`) off and the say service on, so only
one greets. Elsewhere the voice's hello (`speak_up.rs`, Arrival) still
greets where it is switched on, and a lamp that ran both would greet twice.

Welcome back carries Arrival's rules as data: a face at least 0.14 of the
picture's height (`near`), there three seconds (`for`), after five minutes
with nobody near. Where Arrival counts the time since anything was said to the
lamp or by it, Welcome back has two conditions on `voice/state` (#1140):
nothing heard or said for 5 s (resting), and the lamp itself not speaking for
30 s (`not_equals = "speaking"`). Its quiet was Arrival's 30 s until lamp-06c1
showed that in an office the voice is seldom resting 30 s at the moment
someone sits down (its own talk passes in 1 to 4 s every 5 to 60 s), so nobody
arriving was greeted; but a hello right after the lamp spoke would greet
someone it has just talked with, so its own speech still counts 30 s. The
chance waits up to a minute while the person stays (`wait = "60s"`, with a 3 s
`gap` for a look down) and is taken once both hold. Arrival forgives a face
lost for under 20 s before it counts an absence; Welcome back counts the
absence from the last near face, which comes to the same for an arrival after
five minutes.

**With the Policy's gate** (`LAMP_SKILLS_ASK=policy`) the 5 s quiet is not
enough: the Policy holds every hello asked within 15 s of anything heard or
said (`crates/policy/src/hello.rs`), and the skill's cooldown counts from the
ask, so a hello asked at the first 5 s lull is refused and the 20 minutes are
spent. It greets in a busy room only while the product skips the ask
(`deploy/product/lamp-skills.service`); the Policy's side is #1155.

What is not done, and why:

- **The look is one sight.** The body's gaze takes a sight only while the
  lamp listens or speaks, three in a row, so one datagram does not turn the
  head by itself; with the eyes' own stream it turns as the hello is
  spoken. A move the body plays on request is Third's (move files).
- **Without `LAMP_LIGHT_REQUESTS=on`, the light is given back by hand.**
  Two skills changing the light at once, or a person changing it during a
  hello, are not arbitrated; the give-back sets the light seen before the
  run. With the switch, the ring arbitrates (above).
  A dim held `until = "ends"` through the actions part is given back only
  if the light is still as it was left, and when its trigger topic goes
  silent. `air/state` names no `expires_ms` yet (asked of the air part's
  owner on #1021), so a silent sensor ends Stuffy air's dim after 15
  minutes. A restart of `lamp-skills` while a dim is held does not give it
  back.

### Four additions to V2

1. **A say service in the voice**: `say {skill, note or text, priority,
   expires}`. The voice speaks it as a written turn through the gates it
   already has (pause, a person speaking, a conversation under way). The
   hello, reminders and task news move onto it. Built behind `LAMP_SAY=on`
   ([speaking-first.md](speaking-first.md#asked-by-another-part-the-say-service-939)).
2. **Arbitration at the parts that own the body, the light and the
   speaker**: every request carries its source, a priority and a lease that
   lapses unless renewed (Expression's leases already work this way). The
   order: a person (their voice, their hand) over safety, over the
   conversation, over skills, over idle life. The light's is built behind
   `LAMP_LIGHT_REQUESTS=on`: a skill asks on `light/request` and never
   touches the person's light, which comes back by itself when the claim
   lapses ([ring-choreography.md](ring-choreography.md#the-lamps-own-light)).
   The body's is built behind `lamp-body --arbitration on`: a gesture or a
   move on the mood socket may say its `source` and `priority`
   ([body.md](body.md#who-may-ask-over-whom-940)). With
   `LAMP_BODY_REQUESTS=on`, `lamp-skills` stamps every gesture and move it
   sends with `source = "skill:<name>"` and `priority = "skill"` (#1252):
   the skill runner does, never the skill file, so that no skill can claim
   a higher priority. The voice sends its gestures unstamped, so they are
   the conversation's.
3. **The eyes publish `camera/image`** (a file and when it was taken), so a
   skill can look.
4. **The body plays move files** from a skill, within its own speed and
   force limits. Built: `lamp-body --moves DIR` reads `NAME.json` moves at
   start, and a skill's `do.body.move` sends its `moves/NAME.json` whole on
   the mood socket; both are held to the built-in clips' bounds and played
   as a gesture ([body.md](body.md#moves-from-files)).

### Tools for the model

A skill may declare a tool the model may call. `lamp-skills` writes them
beside the actions part's `tools.json`, which the voice reads at every
session open, so a new skill reaches Gemini without a change to the voice.

As built (#941, the first builder's decision of 09:51; not yet run on a
lamp): `tool = "check_my_work"` in `skill.toml`, described to the model with
`SKILL.md`'s description, which says when to call it. With `--tools-dir DIR`
(the unit template gives `@ACTIONS_DIR@/skills`), `lamp-skills` keeps
`DIR/tools.json`, written whole at each load and removed when no skill has
a tool, and answers on `DIR/call.sock` the way the actions part does
([tools.md](tools.md#the-skills)). A call runs the skill's steps without
asking the Policy, since a person asked: the body and the light are made
as in any run, the light given back after its `for`, and the voice is
answered with what the model is to do next: at once, before they are
made, or, for a call with a look, once they are made (at most 800 ms,
`MADE_BEFORE_ANSWER`; a repeat call's pose made again first too), so the
head is on its way down over the page and the light up before the voice
asks the eyes (#1146) (`{"next": "..."}`): with `look = true`, to say only "Let me take a
look.", and `"look": true` beside it; with `say`, the note.

The look is the voice's, not the model's to remember (#941; at 16:05 the
model answered without one). Once the answer with
`"look": true` has gone to the model, the provider worker tells the
coordinator (`LookAsked`), only for a call routed to the skills
(`skills/`), never for another part's answer with that key. The
coordinator takes the full look (#836) for that turn as it does when the
lamp says it will look (`look_asked` with `"by":"tool"` in the record),
but asks the eyes for the still (`look_full_asked`), at
`LAMP_LOOK_FULL_SIDE` if set, only when the turn's first audio plays or
its reply ends, whichever is first: the moment the model's "Let me take a
look." would have come. The head is then moving, so the eyes wait for it
to settle (`full_look.rs`). The still's wait counts from that ask, and
the model's own "Let me take a look." asks nothing more. The look is
the calling session's: no session is renewed while it is owed (at most
`LOOK_BEGINS_WITHIN` plus `STILL_WAIT`), and if another session is the
ready one when its turn would go, it is dropped. A call in a turn the
lamp wrote itself (the look's own turn, a hello) takes no look, nor does
one whose answer never reached the model; every dropped look is
recorded as `look_dropped` with `why`. Only Gemini Live takes a tool's
look: the second route answers the same tools but sends no `LookAsked`
(its engine answers the call inside the tool round), so there the
model's words are the look, as before. When the turn is over the look's
written turn goes with the still. The still cannot go in the call's answer
itself: a function's answer to Gemini Live is words, and the call is
answered at once while the still takes up to 2.5 s. If no still with
this ask's id comes within `STILL_WAIT` (2.6 s), the look's turn goes with
no picture and tells the model that it could not see and must say so,
never guess (`NO_LOOK_NOTE`). Without `LAMP_LOOK_FULL=on` nothing of this
happens and, as before, the model's own words are the look. A name two
skills give is the first's, by folder. Without `--tools-dir`, or with no
skill that has a tool, nothing changes.

A call is refused, in words the lamp can say, and nothing is done, when the
skill's `[[while]]` conditions do not hold as a trigger would need them
(on a value that still lasts, for their `for`). While its last run is
still under way (a light's `for` not yet over), a call again is told again
what to do next, the look and the say, and nothing more is made: the light
already on stays and is given back when its `for` ends, so a student who
asks again a minute later gets a new look (#941, the first builder's 12:09).
Only a call that is nothing but made steps is refused then, and a call to a
skill with no pose and no look, as Dance: nothing is made again, the
model is answered with the skill's `still` ("I'm still dancing."), and the
call is no run, so an `[again]` window still runs from the run under way. A call the listener gave up on after a second, waiting
for the program's loop, is never run afterwards: whichever comes first,
the run or the giving up, settles it.

A tool takes at most two arguments, a `[choice]` and a `[flag]` (#999),
written into `tools.json` as the function's `parameters`
(`{"type":"object","properties":{NAME:{"type":"string","enum":[...],"description":ABOUT}, ...}}`),
and checked on each call: anything else, a value not one of the choice's,
or a flag not true or false is answered as an error and nothing is done.
Dance (`skills/dance/`) is the first skill with them.

A tool with `[again]` (#999, the first builder's decision of 15:46) does
not run again soon after it ran unless what was heard asks for it: no flat
cooldown, since that would refuse a real "again!". Within `within` of the
last run, the call runs only if the newest thing heard began after that
run began and has one of `if_heard` among its whole words, any case, an entry of more words
with those words in a row; else the model is answered with `told` as an
error, and nothing is done. Whole words (Seventh on #1012): "danc" matched
"guidance" and "again" "against". `voice/heard` is an event topic, so the
bus keeps no latest of it: `lamp-skills` subscribes to it when a loaded
skill has `[again]`, and reads what is queued before it answers a call.
The voice says what it hears in pieces, and `lamp-skills` joins them as
the voice's thread does (`crates/voice/src/thread.rs`, `person`): a piece
is added to the last, whitespace as one space and at most 300 bytes, and
a piece after one with `finished: true`, or more than 20 s after the last,
begins another. So "Lamp, do it again," then " please." is asked, and "one"
then " more" is "one more". What was heard began when its first piece came, so the ask's own words, finished after its call, and a noise joined to an ask never finished, unlock nothing (the reviewer, 01:56). Words still coming (`finished: false`) count,
as the call may come before the end.
The window is counted from when the last run began, the moment the call
was taken: its end depends on the model's line and the clips, and is not
known for a run cut short or never begun. Dance's is 45 s with "dance",
"dances", "dancing", "danced", "again", "another" and "one more", after lamp-06c1 danced unasked
28 s after a dance, on a noise heard as "¿Qué?" (voice request 42). A tool
call from a request the voice judged noise or held is the voice's to stop,
not this.

Not done: a function in `run.py` is not
run. The look happens only on a voice with its look on (`LAMP_EYES_LOOK_FILE`,
with `LAMP_LOOK_FULL=on` for the full one); without the full one, only if
the model's answer begins with those words. A `tools.json` left by a `lamp-skills` that was
killed stays until it starts again; a call then is answered as not
running. The test uses a folder of Desk tutor's shape.

### Reflexes stay in the body and Expression

A tap on the head, a startle at a bang, giving way to a hand must answer in
under 150 ms. They stay built into the parts that own the body and the
light. A skill may add a clip for a reflex as data. Behaviours that unfold
over seconds are skills.

### Building one

"Lamp, learn to …": the lamp hands the request to Claude Code with the lamp
skill creator (V1's `skill-creator`, taught the topics, the services,
`skill.toml` and the tests; it is `lampOS/skill-creator/`, installed into
Claude Code as `~/.claude/skills/lamp-skill-creator`, and the skills it wrote
first are `lampOS/skills/stretch-break/` and `lampOS/skills/stuffy-air/`).
Claude Code writes the skill, tests it on the
lamp's own recordings (`lamp-bus record` and `replay`), installs it, and the
lamp shows the owner, who keeps it or asks for a change. Owners can also
build in Claude Code or Codex on their own computer; a change reloads live
on the lamp. A "My skills" page in the lamp's app (on or off, what each did
and when) comes once there are more than a handful. The store comes after
launch.

## V1's moves and emotions: what we take

The owner, 08:00: "evaluate carefully to pick the good stuff from V1, not
everything." All 29 of V1's recorded moves (`hal/recordings/*.csv`) were
converted the way the first six were (offsets from the standing pose,
scaled into the body's reach, slowed under its speed caps), built into an
evaluation body on lamp-4ace, played one at a time between 08:02 and 08:21,
and photographed from the iMac five times each, about 2.4 s apart (the
sheets are in `~/lamp-camera/review/v1moves-a/sheets/` on the office
iMac). Stills that far apart miss a quick move; the size and length below
are from the converted data, and the owner sees the short list live.

Criteria: does a person read it at once; would a living thing do it; is it
visible at the size the body allows; is it short enough to be a gesture
(three seconds or so) rather than a performance.

| V1 move (V1's meaning) | Length, size | Seen | Verdict |
|---|---|---|---|
| nod, headshake, greeting, laugh, shy, stretching, idle | 3 to 10 s | In V2 since 14 Oct | Keep (already ported) |
| acknowledge ("got it", micro-nod) | 8 s as recorded | Small | Keep, cut to its first nod (about 1.5 s) |
| curious (tilts, looks around) | 10 s | A clear lean to one side | Keep the tilt only (about 3 s); the looking around goes |
| happy_wiggle (good news) | 13 s | A clear sway, the most readable move of all | Keep, cut to about 4 s |
| sad (droops slowly) | 10 s | A clear droop, head low for seconds | Keep as **caring**, for empathy only, about 5 s |
| shock (jolts back) | 9 s | A clear recoil | Keep as **surprised**, smaller; a bang still gets the built-in single glance |
| confused (head tilt side to side) | 20 s at this size | Tilted and lowered | Keep, cut to about 3 s: the lamp's "Sorry?" (#930) |
| sleepy (droop with head catches) | 7 s | A small droop | Keep for Bedtime |
| excited (vertical bounce) | 17 s | Barely visible at this size | Try cut to 3 s for Task news "done"; drop if it still reads as nothing |
| listening (lean forward, head cock) | 4 s | Small | Drop: V2's listening pose already does it |
| thinking_deep | The recording does not move | — | Drop: V2 looks away while thinking |
| goodbye (wave) | 3.5 s, ends folded | — | Drop: it ends in another pose; the lamp folds by itself when left |
| fear | 13 s, big head roll | Little | Drop: fear is not something a desk companion should show |
| playful | 12 s | Little; no clear meaning | Drop for now; games later |
| scanning (looks around) | 18 s | A clear turn | Keep for when a person asks ("look around for my keys"); never on its own (CLAUDE.md rule 8) |
| music_groove, music_hiphop | 18 s | A big dip and rise, clearly a dance | Keep groove for Dance; hiphop is nearly the same recording, drop |
| music_hype | 21 s | Fast (blurred stills) | Keep for Dance, energetic songs |
| music_chill, music_waltz, music_jazz | 10 to 12 s | A sway, small | Keep chill and waltz for Dance; jazz to be judged live |
| music_classical, music_rock | 11, 23 s | Little | Drop unless the live look says otherwise |

V1's emotion table (`skills/emotion/SKILL.md`) paired each move with a
light. Kept as named expressions, with the light toned to the lamp's own
eye rather than V1's colours: **happy** (happy_wiggle, a warm sparkle),
**curious** (the tilt, the eye's arc toward the person), **caring** (sad's
droop, a slow warm breath), **surprised** (shock, one quick brightening),
**sleepy** (sleepy, dimming), **acknowledge** (the short nod, a brief
brighter eye), **confused** (the tilt, a soft pulse). V1's rainbow,
coloured blinks per emotion and the "speaking wave rainbow" are dropped:
they read as a gadget, not a creature.

### The expressions

One name gives both a move and a light. The voice names it when an
answer begins as `body.md` lists. A skill's `do.body.gesture` names them
too (#999), as any of the body's clips but `idle`. The body plays the clip from the pose it is in
(`crates/body/src/clips/data.rs`). The ring shows the accent for 1.6 s
over the speaking face, never above the light's ceiling
(`crates/voice/src/ring_face.rs`, `accent_scale`). The accent is in the lamp's
own light only, a little brighter, warmer, dimmer or moved, never another
colour.

| Name | Move (from V1) | Length (`Clip::duration_us`) | Light, for 1.6 s |
|---|---|---|---|
| happy | happy_wiggle, a sway | 4.00 s | A warm sparkle around the ring, up to about a third brighter |
| caring | sad's droop, head low | 5.00 s | About a sixth dimmer and warmer, slowly |
| surprised | shock, a recoil | 2.00 s | One quick brightening, up to 60 %, falling away |
| confused | a tilt, lowered | 3.96 s | A dim spot swings once along the lower ring |
| curious | the tilt only | 3.00 s | The top of the ring brightens, as if looking up |
| acknowledge | the first nod | 1.95 s | One short brighter blink at the start |
| sleepy | a droop with head catches | 9.11 s | A slow dimming, down to 60 % |
| excited | a bounce, cut short | 4.32 s | None (no accent yet; it is on trial for Task news) |

The six older gestures (nod, headshake, greeting, laugh, shy, stretching)
keep the face as it is. A name that is neither gives no accent and no
move.

## V2's own behaviours: skill or reflex

The owner, 08:00: skills are the fundamental part of how the lamp works;
what was built one-off becomes skills. Each behaviour V2 has today, and
where it goes:

| Behaviour today | Where it lives now | Becomes |
|---|---|---|
| Hello when someone comes back to the desk | The voice (`speak_up.rs`), asking the Policy (#883) | Skill: Welcome back |
| A reminder or timer coming due | The voice's reminder consumer | Skill: Reminders (setting one stays a voice tool) |
| News of a task handed to Claude Code or Codex, and progress news | The voice (`task_news.rs`), the Policy's progress act | Skill: Task news |
| Sleep at night, waking in the morning | Expression's catalogue (`alive`) | Skill: Bedtime and Morning, later |
| Turning to a voice, looking away to think, back before its first word, nods while speaking | Voice, body, Expression | Built in: the conversation, under a second |
| A tap on the head, a hand held on it, three seconds to sleep | Controls, Expression, voice (#711, #849) | Built in: a reflex, under 150 ms |
| One startled glance at a bang | Expression (`Sudden`) | Built in: a reflex |
| Giving way to a hand, rising when spoken to, folding when left alone, idle shifts | Body | Built in |
| Eye contact (the head follows a face) | Eyes and body | Built in |
| Taking a look when asked what it sees | A voice tool (#836) | Stays a tool; skills may call it |
| Light, volume, timers, memory, faces, hand-overs | Voice tools and services | Stay tools and services; skills call them |
| The privacy switch and the self-test | Voice, eyes, self-test | System, never a skill |

Built-in gestures (the nod on "yes") use the same move and expression
library as skills.

## The first skills

| Skill | Trigger | What it does | Replaces |
|---|---|---|---|
| Welcome back | A face arrives after 5 minutes with nobody | Policy; turns to them, the light warms, a short hello, by name if known | The hello (#883, `speak_up.rs`) |
| Reminders | `schedule/due` | Looks at the person, says it: `say = { from = "schedule" }`, the voice keeping the words and the acknowledgement (#939, (c); `skills/reminders`). The ring's pulse waits for a light step that can pulse | The reminder consumer's timing in the voice (`LAMP_SCHEDULE_SPEAK=skill`); its delivery stays there |
| Task news (#1021) | `tasks/status`: a task comes into `settled` (#1048) as `idle` (done) or `failed`, each task on its own | Policy (`news`); done: looks at the person if a face is in view, `happy`, and the voice's own news of it (`say = { from = "tasks" }`); failed: `caring`, its own light a step dimmer until the droop and the words end, and the news, asking whether to try again. No red ring | Task news in the voice (`task_news.rs`), once it has done 5 of 5 on lamp-4ace with the voice's news off; until then both exist and the skill's folder is not installed. Written by jarvis; the per-item trigger (`each`), steps by trigger (`on`) and the waiting of item runs were added to `lamp-skills` for it. Its cases are replays; it has not run on a lamp |
| Stretch break | Someone at the desk for 50 minutes | Stretches itself as an invitation, never a lecture (creature.md) | New |
| Stuffy air (#1021) | `air/state`'s `co2_ppm` above 1,200 for 10 minutes | Droops once (`sleepy`) and dims a shade until the air is under 1,200 again; once a spell, again only after 10 minutes under 1,000; never from 22:00 to 08:00 (a spell that holds through the night droops once at 08:00), nor while someone talks to it; the dim also ends 15 minutes after the sensor falls silent; never speaks of it (the voice's air facts do, when spoken to) | New: reworked by jarvis from ten's; the rearm line, the quiet hours and the dim held until the trigger ends were added to `lamp-skills` for it. Its cases are made by hand (lamp-4ace's sensor is flaky: it answers, with 20 to 30 failed reads a minute). It ran on lamp-06c1 once with the air fed by hand (`docs/verification/stuffy-air-lamp-06c1-20261017/`). No lamp with a working sensor has run it: lamp-06c1's and lamp-cb94's never answer on I2C bus 0 (#705, 17 Oct) |
| Dance (#999) | "Lamp, dance", "show me a dance", "dance to this": its tool, `dance(style, long)` | One short playful line, then, when the line ends, one of V1's five dances (#979) picked by the mood asked for (calm: chill or waltz; upbeat: groove or hype; jazz), played twice for a long one, with a low warm glow given back at its end. Never on its own: the ears cannot yet tell music from talk | New: written by jarvis; its tool's arguments, the body's clips by name and the steps after the say were added to `lamp-skills` for it. Its cases are replays; it has not run on a lamp |
| Desk tutor (the owner, 09:45) | "Lamp, check my work" or "I'm stuck" | Tips its head over the page and brightens it, takes one full-resolution still, tutors by voice (a hint or a question, never the answer), a small nod where a step is right, a curious tilt where it is wrong, then faces the person again when the page's 2-minute light ends | Built: `skills/desk-tutor` (#1002, the look #1125, the reading pose #1011/#1098) and its marks, `skills/desk-tutor-marks` (the nod or the tilt, by the model's `show_verdict`, #941); the camera stays at 1280×960 (#987) |

The first three prove that built-in behaviour moves onto the framework; the
last two that a new skill needs no new code.

## Who builds what

| Piece | Builder |
|---|---|
| `lamp-skills`, the skill format and its tests; Welcome back | Seventh |
| The say service; the Policy as the gate; Reminders and Task news as skills | Second |
| Arbitration at the body and the light; move files | Third |
| The lamp skill creator, and Stretch break written with it | Ten |
| Dance, and the tool arguments, clips and steps after the say it needs; Stuffy air (from ten's, #1021), and the rearm line, quiet hours and held dim it needs; Task news as a skill, with the per-item trigger it needs | Jarvis |
| Each piece on lamp-4ace as it lands | First |

Target: Welcome back running as a skill on lamp-4ace on 2026-10-17; all
five by 2026-10-19.

## Later

Permissions per sensor and actuator for skills from other people, signing,
review and the store: after the first skills work, before anything is
published.
