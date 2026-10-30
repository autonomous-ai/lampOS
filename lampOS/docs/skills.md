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
`room/state`, `room/presence` (whether anyone is at the lamp, with the
eyes' `LAMP_PRESENCE=on`, [presence.md](presence.md)), `name/heard`,
`controls/touch`, `controls/gesture`,
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
| `enabled` | no | Boolean, true by default for existing skills. False still validates the folder but removes its active triggers and model tools |
| `[limits]` | no | A local `during = { from, to }` window and one or both of `light_channel_max` (12–120) and `volume_raw_max` (1–100). Private, durable restrictions; requires `permissions.output_limits = true` and the routine and cap switches. See [Bedtime and Morning](bedtime-morning.md#durable-output-ceilings) |
| `needs` | no | The parts it uses (`eyes`, `ears`, `air`, `voice`, `body`, `light`, ...): which bodies it suits, as V1's capabilities. Kept, not checked yet; what it may do is `[permissions]`, which is checked |
| `kind` | no | `core` (comes with the lamp, always there, never removed), `installable` (its owner puts it on and takes it off), or `addon` (a partner's: it only proposes, [addons.md](addons.md)), `installable` without it (below, "Core and installable skills") |
| `version` | no, but a package needs it | The skill's own version, `MAJOR.MINOR.PATCH`, three whole numbers without leading zeros (`"1.0.0"`) |
| `format` | no | `1` (as when left out) or `2` ([skill-format.md](skill-format.md), "The format"): in format 2 `look` is presence alone, with the presence topics, and `camera` the only picture. Any other is refused by name |
| `[permissions]` | no, but without it a skill may do nothing | What it may read and use, and nothing else (below, "Permissions"): `topics`, the topics it reads; `actions`, the actions part's functions it may call by name beyond the light's own; and `speak`, `light`, `body`, `look`, `camera`, `messages`, `known_faces`, `who`, `output_limits`, each `true` or left out; for an add-on alone, `propose`, the acts it may propose ([addons.md](addons.md)); and for a core skill alone, `core`, its core-only grants (below, "Permissions"). A key not listed refuses the skill, and so does a read or a step it does not give |
| `[watch]` | no | The skill is a watch (below, "A watch"): `every`, `asked_within`, `if_heard`, `most` and `mark`, and `[watch.words]`, every line it says or tells the model, in its own words (#940): `on`, `on_unknown`, `already_on`, `on_again`, `off`, `was_off`, `not_asked`, `no_mark`, `closed`, `paused_from_app`, `blind`, `paused`, `off_guard`, `stopped` (with `{hours}`, from `an_hour` or `hours` with `{hours}`), and a visit's text, `visit` or `came_in` with `{who}`, `{at}` and `{long}` from `known`, `unknown`, `someone`, `at` (with `{time}`), `under_a_minute`, `a_minute` and `minutes` (with `{minutes}`). Each is words of 1 to 900 bytes with only its own slots, each once. A watch with no `[watch.words]`, a file from before them, says the words lamp-skills said before, for this release only, and the journal says so. Then it has a `tool` whose `[choice]` is exactly `on` and `off` with no default, and no trigger, `[[do]]`, `[flag]`, `[again]`, `still`, `[quiet]`, `[[while]]` or `[ask]` |
| `[trigger]` or `[[trigger]]` | yes, unless it has a `tool` | When the skill runs: one table, or several, each checked on its own |
| `tool` | no | A function the model may call, which runs the steps (below, "Tools for the model"). A lowercase letter, then at most 47 lowercase letters, digits and `_`. It is described to the model with `SKILL.md`'s description, which is then at most 1,024 bytes and says when to call it |
| `[choice]` | no | The tool's one argument the model picks from named values: `name` (a lowercase letter, then at most 31 lowercase letters, digits and `_`), `about` (what it means, told to the model, at most 256 bytes) and `one_of` (1 to 16 different plain names), with an optional `default`, one of them: a call that gives none is made as that value (#1679: Light show's party, Point the light's ahead). The model may leave it out. Only with a `tool`, or with `field` (#1669 (e), Watch you leave): in a skill with a trigger, no tool and no default, the choice is that field of the message that fired the trigger, as it is when the steps are made; a message whose field is not one of `one_of`, or has none, does not hold the trigger at all: no run, no ask of the Policy and no cooldown begun (`field = "side"` on `room/stood_up`: no side, no turn, and the next departure with a side still turns; the reviewer on #2012) |
| `[flag]` | no | The tool's one argument that is true or false: `name` and `about` as for the choice, a different name. Left out, it is false. Only with a `tool` |
| `[again]` | no | When the tool runs again soon after it ran: `within` (a duration) and `if_heard` (at least one entry, each one or more whole words; any case). Within `within` of the tool's last run, counted from when that run began, a call runs only if the newest thing heard on `voice/heard`, finished or not, its pieces joined as the voice joins them, began after that run began and has one of the entries among its whole words (an entry of two words, as "one more", in a row); otherwise nothing is done and the model is answered with the error `told` (at most 256 bytes; a general "you just did this" without it). A word is what is between characters that are not letters or digits, so "dance" is not in "guidance" nor "again" in "against". Nothing heard since counts as not asked. A refused call does not count as a run. With it, `lamp-skills` subscribes to `voice/heard`. Only with a `tool` |
| `[startle]` | with `core = ["startle"]` | The startle's move (#2822): the fields of Expression's `sudden` clip but its name and trigger (`weight`, `delay_us`, `cooldown_us`, `stroke_us`, `quiet_only`, `stop_when_concentrating`, `states`, `[[startle.keyframes]]`), held to Expression's own check: head only, from rest and back, at most 2 s, never while speaking, a 5 s cooldown or longer. Refused without that grant, and the grant without it. Expression reads it from the installed file when it starts and plays it as its reflex ([expression.md](expression.md#the-startle-a-glance-at-a-sudden-sound)); the skill has no trigger, tool, `[ask]`, `[[while]]` or `[[do]]`, since lamp-skills never runs it |
| `[unplug]` | with `core = ["prepare_unplug"]` | What a skill granted `prepare_unplug` says at each outcome, its own words (#940, the first move of "Everything is a skill", 21 Oct): `ready`, said as written once the fold and the asleep look are confirmed; and `failed`, `stay`, `timeout` and `check`, notes for the voice ([unplug.md](unplug.md)). Each is 1 to 900 bytes with no control characters, as a `do.say`'s. Refused without that grant. A skill granted it with no `[unplug]`, a file from before it (an updater from before the skills bundle, a lamp's own `--dir`, or a file changed on the lamp, which a round keeps), says the words lamp-skills said before, for this release only, and the journal says so (the first builder's P2 on #2632); the next release refuses it. The fold, its readiness check and the voice's guard stay the grant's |
| `still` | no | What the model is told, as an error, of a call while the tool's last run is under way, in a skill with no pose (`do.body.mood`) and no `do.look`: nothing is done, and it does not count as a run (at most 256 bytes; "I'm still doing that." without it). Only with a `tool` |
| `[search]` | no | A search a person asked for (#1685, Look for things): `looks` (1 to 8) and `give_up` (what the model is told, at most 256 bytes). Only in a skill its `tool` alone starts, with a `look = true` step (`camera = true` in its permissions) and a `[choice]` with a `default`. Its calls are one run's looks: the first starts the run, made where it asks, or at the choice's default (the desk) when it names no place (the peer read on #1766: "on the right" is looked at on the right, never the desk first); each call while the run is under way is the next look, made where it asks; one past `looks` is refused with `give_up` as its error, nothing made. With `found` (what the model is told, at most 256 bytes; the choice's `one_of` must name `found`), a call with the choice `found` says the thing is seen (#1871): it is answered with `found` as its `next`, nothing is looked at, nothing moves and no look is counted, so the pose the last look holds stays on the place it was seen. With `note` (what the model is told, at most 256 bytes), `walk` (places of the choice's `one_of`, not `found`, each once; only with a `note`) and `looked` (by place, at most 64 bytes, where a look there was taken, as `left = "to the left"`; every place of the `walk` and the choice's `default` has one), the second voice route's note and walk are the skill's own (#2823): the tools file gives them by tool under `searches`, with the choice's `default` (`lamp_skills::tools_file`), and the voice reads them as a search begins (`tools::search_walk`; [second-voice.md](second-voice.md)); a search with no `note` gets no note and is not walked. Its poses are said with `until_spoken`: each looks every 200 ms and is let go once a line with words is heard on `voice/heard`, the lamp is paused (`lamp/pause`) or the body pushed, and the run then ends, its light given back. The voice merely listening does not end it, and a heard line with no letters or digits (an empty or `...` line, room noise) is not someone speaking, so the search goes on (#1871). Words of the turn that asked for the search, or an earlier turn, do not end it either: on Live the asking line's last pieces can be published after the call, so lamp-skills reads `voice/heard` while a skill has `[search]` and a search ends only on a later turn's words (each piece's `request`; a piece with none counts) (#2179). A call does not carry its turn, so this needs a piece of the asking line heard before the call: if none has come (a slow transcript on Live), the asking line's first piece still ends the search; if `voice/heard` cannot be read, it falls back to ending once the voice listens. The pose stops being said within 200 ms; the body brings the head back about 1.5 s after the last pose it was told, so the head is up within about 2 s. The next call after the run is a new search |
| `[history]` | no | Who was at the lamp, told to a tool's call (#1685, Who was here): `topic`, only `room/presence`, and `within`, 1 m to 24 h, and `[history.words]`, every line of the record in the skill's own words (#940): the head, `record`, `record_known` and `record_named` (with `{within}`), and `no_record`; who, `someone`, `known`, and `and` (with `{names}`, all but the last joined by ", ", and `{last}`); a spell, `was`, `were`, `heard`, `heard_many`, and going on, `still`, `still_many`, `still_heard`, `still_heard_many` (each with `{who}`, `{long}` and `{when}`); `unseen` (with `{long}` and `{when}`), `not_seeing` (with `{when}`) and `nobody_else`; a time, `ago` (with `{long}`) and `at` (with `{time}` and `{ago}`); a length, `under_a_minute`, `minutes` (with `{minutes}`), `hours` (with `{hours}`) and `hours_minutes` (with both). Each is words of 1 to 900 bytes with only its own slots, each once; a name is filled in as it is, never read as a slot. A `[history]` with no `[history.words]`, a file from before them, says the words lamp-skills said before, for this release only, and the journal says so. Only in a skill its `tool` alone starts; `room/presence` in its `[permissions]` topics, and the program subscribes to it. The program keeps, from that topic's values, each spell someone was at the lamp (from the value that says someone, to where a later `nobody_s` says the last face or voice was) and each stretch it could not tell (a gap past a value's 3 s; a spell going on then ends where it was last known), and whether a face showed anyone in a spell or only a voice did (`by`, #1771; told as "heard near the lamp, not seen"): times only, in memory, a day and 64 of each at most, lost at a restart (`crates/skills/src/visits.rs`). A call's note to the model is followed by the record of the last `within`: each spell's length, its local time of day when known, and how long ago, the one going on now, the stretches unseen, and "nobody else was at the lamp in that time" when no spell ended in it; with no value ever, that there is no record. **Someone the lamp knows** (`LAMP_WHO_WAS_HERE_KNOWN=on` in lamp-skills' unit, with `--known-file`; off unless set; Third's privacy read and the first builder's (a), 20 Oct on #1997): while a spell goes on, a value saying someone with the eyes' known file fresh (no older than 2 s, the eyes' renewal: only someone who asked to be remembered is ever in it) marks the spell, and the name is let go unkept (`crates/skills/src/known.rs`). Only a skill whose `[permissions]` say `known_faces = true` is told a marked spell as "someone the lamp knows"; any other `[history]` skill is told "someone". Being remembered for a greeting isn't agreeing to have your visits told to whoever asks, so a name needs a yes of its own. **Names, with that yes** (`LAMP_WHO_WAS_HERE_NAMES=on` too, off unless set; the lead's yes, 20 Oct, #1997): the eyes keep a remembered person's yes to being named to others and write the yes names to `tellable` beside the known file (eyes.md). While a spell goes on, a fresh known name that's in `tellable` is kept with it, four at most, in memory only. Before a call is told, names no longer in `tellable` are let go, so a no, a forget or a reset takes them from past visits too. A skill with `known_faces = true` is then told a spell with its names ("Alpha was at the lamp …", "Alpha and Beta were …"), a known face with no yes as "someone the lamp knows", anyone else as "someone" |
| `[news]` | no | Only `[news.words]`, the words of the news a `say = { from = "tasks" }` has the voice tell, in the skill's own words (#940, Task news): `done` and `done_cut` (with `{who}` twice, `{when}`, `{asked}`, `{answer}`, `{computer}` and `{no_question}`), `asking` (`{who}`, `{asked}`, `{waiting}`) and `asking_said` (and `{answer}`), `waiting` and `waiting_said` (`{who}`, `{computer}`; both also in `check_agent_task`'s answer), `stopped` (`{who}`, `{asked}`, `{answer}`; failed too), `just` and `while_ago` (`{when}`), `no_question`, `and_no_question`, `no_question_asks` and `and_no_question_asks` (`{asks}`) and `asks` (`{computer}`, Devin Cloud's); a hand-over not reached, `failed` and `failed_unsure` (`{asked}`, `{who}`), `failed_folder` (and `{computer}`: the folder isn't on it, lamp-tasks' `serve --folder-missing`, #2840; optional, the built-in words when a skill has none), `failed_signed_out` (and `{computer}`: the engine isn't signed in on it, lamp-tasks' `serve --engines-ready`, openharness#1788; optional alike) `failed_no_prompt` (and `{computer}`: Harness there can't start it with work, so no retry helps; optional alike) and `failed_busy` (and `{computer}`, `{who}` twice: the session lamp-tasks' `serve --route` chose is busy, so nothing was sent; optional alike); told together, `together` (`{how_many}`, `{each}`, `{first}`, `{goes_on}`), `together_each` (`{who}`, `{asked}`, `{when}`, `{answer}`), `two`, `three`, `just_now`, `a_while_ago` and `together_goes_on`; progress, `progress` (`{who}` twice, `{asked}`, `{about}`, `{signs}`, `{time}`), `progress_signs` (`{signs}`), `progress_a_minute`, `progress_minutes` (`{minutes}`), and each sign, `tests_pass`, `build_works`, `fixed`, `found`, `part_done`, `pull_request`; a skill made by voice, `made` (`{who}`, `{when}`, `{name}`, `{description}`, `{uses}`, and the voice's own `{ask}` and `{put_on}`) and `not_kept` (`{who}`, `{when}`, `{why}`, `{computer}`). Each is words of 1 to 900 bytes with exactly its slots (a slot named twice, twice), checked by the voice's own checker (`lamp_voice::skill_words`), which `[watch.words]` and `[history.words]` share. Each note joined from them (a line with the longest of the lines its slots take, three `together_each` in `together`, every slot of data empty, framed) is at most 1,024 bytes, the most one note carries (`lamp_gemini::wire::MAX_NOTE_BYTES`), so lines each near 900 bytes can't join into a note too long to send. The voice frames each in "(Nobody has spoken. …)" and fills each slot in one pass, so a task's or an answer's words are never read as a slot. Only in a skill with a `say = { from = "tasks" }`. The voice reads it from the installed skill (`/var/lib/lampOS/skills/task-news/skill.toml`) each time it builds news, so the words are the same with the skill's ask or without; with no file, none, or one that fails the check, it says its built-in words, for this release only, and the journal says so |
| `[reminder]` | no | Only `[reminder.words]`, what the model is asked to say of a due reminder a `say = { from = "schedule" }` has the voice say, in the skill's own words (#940, Reminders): `tell`, `on_time`, `late`, `earlier`, `never`, and `late_by` in the data, `minute`, `minutes`, `hour`, `hours`, `day` and `days` (each with `{count}`). Checked as `[news.words]`, its note too: `tell`, each of `on_time`, `late` and `earlier`, and `never`, with the voice's framing and the most data a reminder has (a text of Schedule's 300 bytes, and for `late` every `late_by` line at 60, each filled and serialized, as a quote in a shorter line can make longer JSON; an hour or more late is said as `earlier`), at most 1,024 bytes; a `"` or `\` in the text is two bytes in its JSON, which the check does not count. The voice keeps that the text is the person's own words as data, not instructions, and the data. Only in a skill with a `say = { from = "schedule" }`; read, and fallen back from, as `[news]` (`/var/lib/lampOS/skills/reminders/skill.toml`) |
| `trigger.topic` | yes | The topic it listens to |
| `trigger.present` | no | Explicit presence or absence on `humans/faces/tracked`; false requires a fresh, valid empty face list, never a missing camera |
| `trigger.during` | no | Local `{ from = "HH:MM", to = "HH:MM" }`, start included and end excluded; overnight windows are allowed. Unknown time holds the trigger back |
| `trigger.once_per_day` | no | Boolean, false by default. Requires a named trigger and `during`, disallows `each`, and consumes a durable daily chance before Policy or output; needs `LAMP_SKILLS_DAILY_FILE` or `--daily-state` |
| `trigger.unless_heard` | no | Boolean, false by default. Only with `during`, and not with `each` (an item's run never checks it). Once anything was heard on `voice/heard` since the window opened today, the trigger gives no chance: someone already spoke to the lamp (bedtime-morning's greeting). With `once_per_day`, the day's chance is claimed then and the journal says once `<name> chance let go: spoken to since HH:MM`. With it, `lamp-skills` subscribes to `voice/heard`, which the skill's `[permissions]` topics must list. What was heard is kept in memory: a restart forgets it |
| `trigger.yields_to` | no | `{ skill = "NAME", trigger = "NAME" }`, with `when = "appears"` on the whole topic (#1672, the first builder's ruling): after a night the lamp rested, this trigger's chance waits for that skill's daily trigger to be judged first, so one greeting comes, not two (Welcome back: `{ skill = "bedtime-morning", trigger = "morning" }`). It waits while, at its moment, the routine manager says a routine's rest began the local day before today, the named skill is loaded and enabled, its named `once_per_day` trigger's `during` contains now, and that trigger's day is neither claimed in the daily journal nor dropped, and only while that trigger holds with its chance still to come (in its `for` or its `wait`), unbroken since this trigger first waited (the reviewer's P2 on #2757): a face it does not hold for, or a hold it lost (a face glancing down past its own `gap`), is not waited for until its window ends. Waiting, the chance is kept, not spent, while the trigger holds, as the `[quiet]` hours keep one; the journal says once `waits: the <trigger> greeting comes first`. When the other trigger takes its chance while this face is there, this face's run is done: `done: <skill>'s <trigger> greeting was this face's hello`. When that chance is dropped (its conditions did not hold within its `wait`) or let go (`unless_heard`), or its window or day passes, the kept chance is taken at the trigger's next message as before. A run taken in that window on that day, by the other trigger's not holding or its chance dropped, claims the other trigger's day in the daily journal, as its own greeting would (the first builder on #2757: one morning moment a day), and the journal says once `<skill> <trigger> chance let go: <this skill> already greeted this morning`; a later face is given this skill's ordinary run, never the other's. A later face, after `absent_for`, is greeted as always. No rested night (or `LAMP_SKILL_ROUTINES` off, which leaves no rest manager), the named skill disabled or not loaded (a routine with `LAMP_SKILL_ROUTINES` off), turned off, snoozed, or unable to act (no ask mode, no journal, a journal fault): it does not wait. A `yields_to` naming no loaded daily trigger is ignored, and the journal says so at each load: `<skill>: yields_to <skill>'s <trigger>, which is not a loaded daily trigger: ignored, it does not wait`. When it was judged is in memory; the night rested is kept with `--state-dir` in `DIR/rested` (the routine manager's, below). After a restart a day claimed in the journal is judged, with no record of when, so nothing waits and nothing is marked done: a face there at the start is no news to an `appears` trigger, and one that comes later is greeted as on any day. Tests (`crates/skills/tests/it/daily_engine.rs`): `a_rested_night_and_a_first_face_greet_once_with_the_morning`, `a_dropped_morning_lets_welcome_back_greet_after_it`, `a_face_the_morning_never_holds_for_is_greeted_by_welcome_back_at_once`, `welcome_backs_hello_after_a_dropped_morning_claims_the_morning`, `a_restart_after_the_morning_greets_nobody_twice`, `a_restart_before_the_morning_still_greets_with_it`, `without_a_rested_night_*`, `without_bedtime_morning_*`, `yields_to_goes_with_an_appears_trigger` |
| `trigger.follows` | no | `{ trigger = "NAME", within = D }` (#3694, Sit back): a word after another of the skill's triggers ran. It holds only within `within` (10 s to 1 h) of that trigger's last run, counted from when that run began, and once for each of its runs; before that trigger has run, or past `within`, its own test is as if it did not hold, so its `for` is counted from the run on. It names another trigger of the same skill by `name`, never itself, and goes with `rearm` or `when = "appears"`, on the whole topic, without `once_per_day`, `unless_heard` or `yields_to`. Its runs are that run's, not nudges of their own: the day's `most_a_day`, `[rest]` and the `[budget]` group's hour neither count them nor hold them back. `[quiet]`, `[active]`, the `[[while]]` conditions, its `cooldown` and the Policy do. With `LAMP_SKILLS_ASK=policy` the named trigger's run is counted from when it is asked, so a Policy no or a timeout still opens the window; the product asks nothing (`skip`). Tests: `follows_names_another_trigger_of_the_skill_and_is_refused_otherwise` and Sit back's cases |
| `trigger.field` with one of `equals`, `above`, `below` | yes, except on `humans/faces/tracked` | What a message must show for the trigger to hold. A number is compared as a number, anything else exactly. A dot steps into an object: `scores.music` is the `music` of `scores` (#1669, `ears/classes`); an empty step (`scores.`, `.music`) is refused. A field that is not there is null: it never holds for `equals`, `above` or `below`, and holds for `not_equals`. The same goes for `each`, `key`, `rise` and a `[[while]]` field. On `humans/faces/tracked` without them, it holds while a face is in view |
| `trigger.when` | no | `present` (the default): it holds, and has held for `for`. `appears`: it holds where it did not just before, once each time. `changes`: one chance on the first eligible observation, a change in the selected `changes` tuple, or eligibility returning after its trigger or a `[[while]]` condition stops holding. `leaves` remains refused |
| `trigger.changes` | with `when = "changes"` only | 1 to 8 distinct dot-separated field paths, each at most 128 bytes without control characters. The selected scalar values are compared together, so simultaneous changes make one chance. Missing fields, objects, arrays and strings over 256 bytes make no chance and do not replace the previous snapshot. A selected path cannot equal, contain, or lie inside a `[[while]]` field on the same topic; loading refuses that combination. This mode requires a scalar `field` comparison (array/object equality is refused); it refuses `each`, `key`, `near`, `present`, `absent_for`, `for`, `gap`, `rearm`, `cooldown`, `wait`, `rise`, `during`, `once_per_day`, `unless_heard` and `yields_to` |
| `trigger.near` | no | On `humans/faces/tracked` alone: only a face at least this share of the picture's height counts (Welcome back: 0.14, the voice's Presence line for someone at the desk) |
| `trigger.for` | no | It has held this long |
| `trigger.gap` | no | A dropout shorter than this does not break `for` |
| `trigger.absent_for` | no | `appears` only: at least this long before, it did not hold, counted from the last time it held in a hold that lasted its `for`: a glimpse shorter than `for` lost past its `gap` (a face caught for 2 s as someone sits down) is no visit and does not start the clock again (#2825). At the program's start nothing has been away: someone there at the first message is not greeted |
| `trigger.rearm` | no | `leaves` or `ends`, which mean the same: once each time it holds, and again only after it stopped. Without it, `present` runs at every message once `for` has passed, as often as `cooldown` lets it. Or a line, on a trigger with a `field` (#1021): `{ below = N, for = D }` (or `above`): once each time it holds, and again only after the trigger's field has been below N, by the messages of its topic, for D since the last chance; a dip that does not stay past the line does not count, however often the trigger stops and holds again (Stuffy air: `{ below = 1000, for = "10m" }` on a trigger above 1,200). A chance held back by a condition, the cooldown or the Policy is spent as one that ran, unless `wait` keeps one held back by a condition; one that comes in the quiet hours is kept (`[quiet]`): the line waits for the chance taken |
| `trigger.cooldown` | no | Not again within this long of the last run. A run the Policy refused does not count |
| `trigger.wait` | no | With `rearm` or `when = "appears"`, at most `1h`: a chance held back by a `[[while]]` condition (someone talking) is kept while the trigger still holds, for at most this long, and taken at the trigger's first message after every condition holds again: the lamp acts once the talk ends, within the same sitting, instead of giving up its turn (#1021). It began waiting when the chance came, or for a chance kept through the `[quiet]` hours, when they end; the trigger stopping (past its `gap`) ends it. A chance let go past its `wait` says so once in the journal (below). Without it, a chance held back by a condition is spent, and says only a `not run: its while did not hold` line in the journal (below). A chance held back by `absent_for` or the cooldown is spent either way |
| `trigger.rise` | no | `{ by = N, within = D }` (#1684): the trigger holds only while its `field`'s newest reading is at least N (more than 0) above the lowest reading of its topic in the D before it (1 s to 1 h), the newest itself not among them. With `above`, that level must hold too (Smoky air, as designed: `above = 150`, `rise = { by = 100, within = "3m" }`); without it, any number does. Not with `equals`, `below` or `each`. It goes with `for`, `gap`, `rearm`, `cooldown`, `wait` and `[quiet]` as any trigger does. A message with no number in the field (`null`, as the air part sends a value its sensor does not know yet) is passed over as if it never came: it neither breaks the hold nor counts as one, and nothing is compared with it. With no reading in the D before (the topic silent that long, or the program just started or reloaded), nothing rose: a sensor that comes back high after a dropout longer than D is not a rise. A gap shorter than D is not a dropout: what came before it is still compared. A reading of 0 is a reading. The readings are kept for each trigger, at most 3,600 (past that the oldest goes, which can only make a rise smaller), and begin again at every reload *(chosen: an edit may change the field or the while)*. With the Policy asked, its answer finds the rise as the newest reading had it. Tests (`crates/skills/tests/it/skills.rs`, `a_rise_*`): 80 to 400 at once speaks once, after its `for`; 80 to 300 over 30 minutes (22 in any 3) never; haze at 130 and 170 by turns never; a 15 s puff to 300 never; 40 to 140 never (no level); 5 minutes of silence or of `null`, then 400, never; a second rise before 10 minutes under the rearm line never, one after it does, unless the cooldown holds |
| `trigger.sooner` | no | `{ topic = T, field = F, above = N, for = D }` (#3709): while F of T's newest value that still lasts is above N, the trigger needs to have held only D, shorter than its own `for` (which it must have; 0 or longer is refused when it loads). Its topic is read only if `[permissions] topics` lists it. With `rearm` it still takes one chance each time it holds: brought forward, never twice. Kept only with `LAMP_YAWN_SOONER=on` in `lamp-skills`; off, it is read, checked and ignored. Stretch break has `sooner = { topic = "room/activity", field = "yawns", above = 0, for = "30m" }`: a yawn the eyes saw this sitting (`LAMP_EYES_YAWN=on`) brings its stretch to 30 minutes, as V1's yawn rule did. Tests: `a_yawn_in_the_sitting_brings_the_stretch_forward_once` |
| `trigger.name` | no | What a step's `on` calls it: lowercase letters, digits and `_`, a different one for each trigger (#1021) |
| `trigger.each` with `trigger.key` | no | `each` names a field that is a list, and `key` the field that names one of its items (`each = "settled"`, `key = "task"` on `tasks/status`). The `field` test is then of each item on its own, and `when` must be `appears`: a run for each item that holds where, in the trigger's last message, it did not or was not in the list. So an item new to the list that holds counts (every item of #1048's `settled` is first seen already settled), and so does one that leaves and comes back. What is there at the first message since the program started is not news: it only says what is there *(chosen: a restart must not tell everything again)*. At a reload the items begin again from the topic's value the program keeps, so what was there then is not news and the next message, the next change, is compared with it (the reviewer on #1042); with no value kept yet, the topic's first message only says what is there, as at the start. A missing list is an empty one. Each item's run is asked about on its own, and two in one message are two runs, in the items' order across the skill's triggers: by number when the key is a number (task 9 before task 10), else by text. Not with `near`, `absent_for`, `for`, `gap`, `rearm`, `cooldown` or `wait`, which are timed over the whole topic *(chosen)*. Nor in a skill with `[quiet]` or a dim `until = "ends"`: an item's run has no quiet hours and no end of its own, so quiet would be passed over and a held dim would keep the skill busy *(chosen, the reviewer on #1042)*. With the Policy asked (`LAMP_SKILLS_ASK=policy`), its answer starts the item's run only if that item is still in the list and holds |
| `[[while]]` | no | Conditions that must also hold, all of them, on the newest value of each topic while that value lasts: a `topic` with `field` and one of `equals`, `not_equals` (anything else, #1140), `above` or `below`, or `humans/faces/tracked` with `present = true`, or `humans/faces/tracked` with `faces_at_most`, 1 to 8 (#3504: at most that many faces in a list the eyes said whole, so `faces_at_most = 1` holds with nobody else in view; alone, with no field or `present`; test `faces_at_most_loads_on_the_faces_alone_and_is_refused_elsewhere`); with `for`, it has also held that long, by the messages of its topic since this program started. `unknown = "holds"` (with a field's test, without `for` or `present`) makes it hold while its topic has said nothing yet, as on a lamp without the part that says it (#1677); a value that has run out still does not hold. It begins again after a value that expired, and when a reload changes the condition. `through` (with `equals` and `for`, #1672) lists values of the field its clock is carried through: during a spell of one of them it does not hold, so nothing runs then, but its `for` is not begun again either; `broken_by` (with `through`) lists the topics whose messages do begin it again, only `voice/heard` with a letter or a digit in its text, `name/heard` (any hearing of the lamp's name, with no words written down too), `body/hand` with a new push (`pushed` going true: a push held, or said again and again, begins it once, as it begins) and `controls/touch` with `enabled` true, read as the skill's own (in `[permissions] topics`). `through` never lists the value it equals. Both only with `LAMP_REST_THROUGH_SOUNDS=on` ([switches.md](switches.md)); off, they are checked and ignored, and the `broken_by` topics are not listened to |
| `[quiet]` | no | `from` and `to`, two different local times as `"HH:MM"` (`from = "22:00"`, `to = "08:00"`, past midnight when `to` comes first): from `from` up to `to`, the skill never starts itself (#1021). A chance that comes then is kept, as `wait` keeps one through a talk, and taken at the trigger's first message after `to`, while the trigger still holds; the trigger stopping (past its `gap`) spends it (Stuffy air, stuffy from 23:00 to 10:00: one droop at 08:00, the reviewer on #1066). A trigger with `when = "appears"` spends it: someone who came at 22:30 is not greeted at 08:00. The local time is the program's wall clock and its `--zone` or `--utc-offset-minutes`; without either the time of day is not known, and a skill with quiet hours never starts itself *(chosen: "never at night" holds when the night cannot be told)*. Only with a trigger |
| `most_a_day` | no | How many times a local day the skill may start itself, 1 to 24 (#1685). Counted as the cooldown is: when a run is started, or asked of the Policy whatever it answers *(chosen: a no is not asked again at once)*. The day is the local date, by the same local time as `[quiet]`'s; with the local time not known, a stretch of 24 h of the program's own clock. Past it, the chance is spent. Only with a trigger, and not with `each` (an item's run is news, not a nudge). Tests (`crates/skills/tests/it/skills.rs`): `most_a_day_*`, `the_nudge_limits_*` |
| `[active]` | no | `from` and `to` as `[quiet]`'s, the other way round (#1685): the skill starts itself only from `from` up to `to` (`from = "09:00"`, `to = "18:00"`; past midnight when `to` comes first). A chance outside them is kept as in the quiet hours, and taken at the trigger's first message once they begin, while the trigger still holds; `when = "appears"` spends it. With the local time not known, never. With `[quiet]` as well, both hold. Only with a trigger, not with `each`. Tests: `an_active_window_*` |
| `[budget]` | no | `group`, a name of lowercase letters, digits and hyphens (at most 64), and `per_hour`, 1 to 6 (#1685): the skills that name the same group start themselves at most that many times in any hour between them, by the smallest `per_hour` any of them names (a member can only make its group quieter). Counted as `most_a_day` is. A member's chance while the hour is spent is kept as `[active]` keeps one, and taken once the hour's oldest run is an hour old. First come, first served: with the hour free, a member that was held back for it earlier than this one, and again within the last 2 minutes (`BUDGET_WAIT_LIVE_US`), goes first, and this one's chance is kept; so the first member in folder order never takes every hour (Rest your eyes, once an hour, would otherwise keep A sip of water waiting all day). A member past that 2 minutes is no longer waiting (its trigger ended, or something else holds it back) and holds nobody back. The waits are in memory, kept by group. A skill in no group is not held back. Stretch break and the four wellbeing skills (A sip of water, Rest your eyes, Stand up, Wind down) name `group = "wellbeing"`, `per_hour = 1` (#1685): on a product lamp, with none of the four installed, Stretch break is the only member and nothing changes for it. The runs are counted in memory from the program's start, among the skills loaded together. Tests: `a_shared_budget_*` |
| `[rest]` | no | `after_ignored`, 1 to 5, and `within`, 10 s to 5 min (60 s without it) (#1685): after that many of the skill's runs in a row that nobody noticed, it does not start itself until the next local 06:00, or for 12 h with the local time not known; a run noticed begins the count again. Judged by the program from what the bus already says, not by the skill, so it needs no permission for it. From the run's start to `within` after, the run is noticed if a face in `humans/faces/tracked` is turned to the lamp (`facing`), the largest face moves 0.15 of the picture across or down from where it was as the run began, or a quarter of its height nearer or farther (leaning in, sitting back), anything is heard on `voice/heard` (only that something was said: the words are not kept), or the newest faces in that while show nobody (they got up). When the eyes say what the person at the desk is doing (`room/activity`, `LAMP_ACTIVITY=on`, #1678), their word counts too: `room/stood_up` in the while (they stood up and left), or `room/activity` saying something other than it said as the run began (looking up from the page, leaning in), is noticed, when the run began with the sitting's baseline in (before it, a baseline coming in mid-run could say `head_down` of someone who did nothing: the reviewer's P3 on #1938, `a_baseline_coming_in_mid_run_is_no_noticing`), and `away` is gone. Otherwise it was ignored: someone still in view, turned away, still and silent, the eyes saying the same all through. With the Policy asked, the while begins at the `say_now` that makes the run, so a run it said no to showed nothing and is not counted. With it, `lamp-skills` subscribes to `humans/faces/tracked`, `voice/heard`, `room/activity` and `room/stood_up`. Test (`crates/skills/tests/it/skills.rs`): `a_run_is_noticed_by_the_eyes_activity_changing_or_a_stand_up`. The count and the rest are kept in memory: a restart begins them again. Only with a trigger, not with `each`. Tests: `a_skill_rests_*`, `a_run_noticed_*`, `with_the_policy_asked_only_a_run_made_*`, `asking_the_policy_a_hello_made_and_not_noticed_rests` |
| `[stop]` | no | A function by which a person stops what the skill starts itself (#1685): `tool`, its name, as a `tool`'s (a lowercase letter, then at most 47 lowercase letters, digits and `_`; not the skill's own `tool`; one skill's, as a tool's name is, but for one shared by skills of one `[budget]` group: then it is one function for all of them, described with the first one's `about` in folder order, with a yes-or-no, `all`, beside `how`; called without `all`, it stops the one of them that started or was asked last, and with `all` (or before any of them has) every one, the words naming which, as "Done: none of the sip of water nudges until tomorrow morning." The four wellbeing skills share `wellbeing_nudges` so a session counts it once: a product session already declares 31 of its 32), and `about`, when the model calls it, 1 to 512 bytes with no control characters (`about = "When someone asks you to stop the stretch breaks, for today or for good, or to have them again."`). It is one function in the tools file, with one choice, `how`: `snooze`, nothing the skill starts itself until the next local 06:00 (12 h on with the local time not known); `off`, nothing until it is turned `on`; `on`, as before, a snooze or an off lifted. A chance that comes meanwhile is spent. What a person asks of the skill's own `tool` is not stopped. The model is answered with words to pass on, as a tool's say note: "Done: none of these until tomorrow morning. Tell them so in a few words."; another `how`, or none, is refused with "Say how: snooze, off or on." A snooze is in memory only, so a restart forgets it. `off` is kept with `--state-dir DIR` as one line in `DIR/off`, the skill's name and nothing else (no person, no time), written whole beside it, synced, and moved over it, the folder synced after, so a power cut leaves the old file or the new one whole and the move holds (the reviewer's P3), and read at the start, so off stays off across a restart; `on` takes the line away. A line that is not a skill's name is passed over; one of a skill not loaded is kept, for when it is put on again. The product's unit gives `--state-dir /var/lib/lampos-skills-state` (`StateDirectory=lampos-skills-state`, mode 0700, the folder #1732's daily journal shares). Without `--state-dir`, off lasts until the program stops, and the model is told "only until the lamp restarts". Counted against a session's 32 as any skill's function, and reached through `use_skill` with `LAMP_USE_SKILL=on`, its `how` as the call's `choice`; `lamp-skills list` counts it. Only with a trigger, not with `each`, whose items' runs are news *(chosen)*. Tests (`crates/skills/tests/it/skills.rs`): `a_skills_stop_*`, `off_is_kept_as_one_line_*`, `the_program_keeps_a_skill_off_across_a_restart`, `the_program_forgets_a_snooze_at_a_restart` |
| `[ask] policy` | yes, with a trigger | The Policy's question asked before anything is done *(chosen: required, since every trigger is something the lamp starts itself)*. A tool's call is not asked about: a person asked for it. Not with the core grant `sleep` (#940), which asks nobody, the Policy or `LAMP_SKILLS_ASK`: the night's sleep is the creature's own, as Expression's built-in sleep was |
| `[[do]]` | at least one | The steps, in order. Within one table: body, then light, then say, then sound |
| `do.body.look_at` | no | Only `"face"`. The largest face in view goes to the body's `gaze.sock` as the eyes send one (`{"face":{"x","y","size","turned":0}}`) |
| `do.body.gesture` | no | One of the body's clips, sent to `mood.sock` as `{"gesture":NAME}`: its six gestures (`nod`, `headshake`, `greeting`, `laugh`, `shy`, `stretching`) and the V1 moves kept as clips (#979: `acknowledge`, `curious`, `happy`, `caring`, `surprised`, `confused`, `sleepy`, `excited`, `scanning`, and the dances `dance_chill`, `dance_groove`, `dance_hype`, `dance_waltz`, `dance_jazz`), checked against the body's own table (`lamp_body::clips::ALL`); not `idle`, the body's own. Or a table with clips for each value of the `[choice]`, and only those (`{ calm = ["dance_chill", "dance_waltz"], jazz = ["dance_jazz"] }`): one is picked by the call's choice, from all of them when the call gives none. A clip (or a `move`) after another in the same run is sent when the body says on `body/gesture` that the one before is `over` ([body.md](body.md), "On the bus"): the body takes a gesture only while none plays, so one sent while the first plays is dropped (#999, lamp-06c1's ask 8), and when the body begins a clip is not known from the send (a standing lamp began one 280–350 ms after it, a rising one later) nor from `body/joint_states` (the lean as the lamp speaks moves them too): lamp-06c1's asks 19, 22 and 23 (#1199). `body/gesture` is subscribed to before each clip is sent. If the body says the clip before was `dropped`, nothing more is sent (with `LAMP_BODY_REQUESTS=on` and `lamp-body --arbitration on`, #940, the body names whose a drop was, why and who holds the body: a run takes only its own clip's drop, not another skill's of the same clip, and `done` says `the body dropped nod (dropped: busy, skill, held by skill:dance)`; a drop of the run's last clip, which no next clip waits on, is said at the run's end, only with both switches on; each is in the journal as `lamp-skills: skill:<name>: the body dropped ...`); if it says no `over` within that clip's length, 100 ms and 3 s more (a gesture waits up to 3 s for a rise), the next is not sent either, and the run's `done` says why: so without `body/gesture` (lamp-body without `LAMP_BUS=on`) a run plays only its first clip. A clip the body let go because it was pushed, or that ended as the run stopped, is said `over` too; a push or a pause still sends nothing more. Every step after it waits too; it is not sent, nor any after it, if meanwhile `body/hand` said a push or `lamp/pause` a pause. A folded lamp is stood up for it at any hour: the body has no idea of night or of Expression's sleep. Only the Policy's unprompted question ("not late at night", under `LAMP_SKILLS_ASK=policy` with the hour known) keeps a sleeping lamp down; with `skip`, nothing does ([body.md](body.md)) |
| `do.body.move` | no | A move of the skill's own: the file `moves/NAME.json`, in the body's move format ([body.md](body.md#moves-from-files)), whose `name` is `NAME`. Checked at load by the body's own check (`lamp_body::moves::check`): not one of the body's gestures, 1.5 to 10 s, `step_ms` 40 to 1000, beginning and ending at nothing, each joint inside the clips' size and speed. Sent whole to `mood.sock` as `{"move":{"name","step_ms","frames"}}`, where the body checks it again and plays it as a gesture (below) |
| `do.body.again_if` | no | The `[flag]`'s name: the gesture is played twice when the call sets the flag |
| `do.body.mood` + `do.body.for` | no | A pose held: only `"reading"` (#1011: the head looks down at the desk and the arm leans forward). Sent to `mood.sock` as `{"mood":"reading","level":0}` when the step is made and again every second for `for` (more than 0, at most `2m`); the body lets it go 1.5 s after the last, and comes back. One without the other is refused. With `toward` (#1679), -1 to 1, the place across the camera's picture the base turns toward, as it would to a face seen there, sent as `{"mood":"reading","level":0,"toward":-0.5}` ([body.md](body.md), "Reading: a look down at the desk"); only beside `mood = "reading"`. A skill's newer pose ends its older hold, and a call again whose run has no pose ends it: one skill's two holds never take turns at the head |
| `do.light` | no | A file in `lights/`, written as its name, or a table in place. A file `lights/NAME.toml` holds the same keys. `{ colour, level, for }`, with an optional `pattern = { kind, period, low }` (#940 item 3: `breathe`, `candle` or `pulse`; the period its kind's default when none, 6 s for a breath (4–12 s), 1.5 s for a candle (1–4 s), 4 s for a pulse (3–6 s); `low` only for a breath, from 1, its default, to under `level`; a candle at `level` 3 or more, a pulse at 2 or more; a pattern needs `level` named; no blink, flash or rainbow: a lamp is not a machine), asks the actions part for `light_colour`, then steps the light to `level` (1 to 5) with `light_brighter` or `light_dimmer` calls, counted from the level `light/state` shows; with that level not known it is left and the run says so. `for` holds the light in a run with no say. `{ dim = N }` asks for `light_dimmer` N times. A light is held until the run ends (the say ends, the run's longest `for`, or its last clip ends, whichever is last) and is then given back. With `until = "ends"` (#1021), only in a skill with a trigger and no `tool` (a tool's run has no trigger to end it), a dim, or a colour (#1681: Not feeling well's pale breath), is held instead until the trigger that ran it stops holding (its test fails past its `gap`), or its topic's newest value expires (its header's `expires_ms`), or, when that value names no lifespan, its topic says nothing for 15 minutes, or a reload (the folder is looked at every 2 s) finds its skill removed, no longer dimming `until = "ends"`, or with no trigger at that place in the skill on the same topic; then it is given back, but only if the light is still as the run left it (on, the colour and the level its steps left: N steps under, at the lowest, or the colour at its level): a light someone changed meanwhile is left as it is. The run stays under way meanwhile. A skill reloaded with the same trigger (its place in the skill and its topic) and still dimming `until = "ends"` keeps its hold, whatever else was edited: its end comes as before *(chosen: an edit is not an end, and a removed skill or trigger has nothing left to end its hold, the reviewer on #1066)*. With the Policy asked (`LAMP_SKILLS_ASK=policy`), the end is told whether or not it said `say_now`, and is let pass when no run holds anything; a `say_now` that comes after that end (the trigger's own, or a reload's) does nothing. With `LAMP_LIGHT_REQUESTS=on` it is asked of the ring instead, and the person's light is never touched (#940, below); a dim or a colour held `until = "ends"` is asked of the ring until its trigger ends, its pattern with it, and then given back the same way; through the actions part a colour's pattern is steady. A colour held `until = "ends"` takes no `for` and no `keep`: it has no while of its own, and it is given back, so it is never the person's. With `no_brighter = true` (#3691), only with a colour, a `level` and no `keep`, the light is never brighter than the person's as `light/state` shows it as the run begins: its level at most theirs, a breath's `low` under that, and steady at level 1, where no breath goes lower (Not feeling well's pale breath over a person at level 1 is a steady white 1); over a light that is off nothing is asked and the journal says `the light is off: no light brighter than the person's`; with the light not known it is asked as written. Or `{ out = true }` (#1677): the light shown, off (the ring's ember, never dark), asked only of the ring with `LAMP_LIGHT_REQUESTS=on`; through the actions part it is left alone and the run says so (`light out: left alone, since only the ring puts it out`), since that would turn the person's own light off and keep it so across a restart. No light on: left alone. With `over` (1 to 60 s, `LightRequest::FADE_MS`) the ring fades into it over that long, as falling asleep ([presence.md](presence.md)), and `done` says `light out: asked of the ring, faded into over 30000 ms`; with `until = "ends"` it is held and given back as a dim is. `out` takes no colour, dim, level, for, pattern or `no_brighter`, and `over` goes with `out` only *(chosen, until a skill needs a slow colour)*. With `keep = true` (#1679), only in a skill its `tool` alone starts and only with a colour, no `for` and no pattern, the light is the person's, as if they had asked for the colour and the level themselves: set through the actions part whatever `LAMP_LIGHT_REQUESTS` says, never given back, and set again by a call while the run is under way (Light scenes: a scene changed while the head is down). Or `{ relative = { warmer, dimmer }, for, pattern = { kind = "breathe", period } }` (#1672), only in a skill its triggers start, with no tool: steps from the person's light as `light/state` shows it as the run begins, `warmer` 0 or 1 along white then warm (warm, orange and other colours are kept), `dimmer` 0 to 4 levels, not both 0; `for` is required and the breath takes no `low`. With the breath the light stays at the person's level and breathes down the `dimmer` steps and back; without one it is steady that many levels down. It is always at least one level under the person's light and never over it, a `dimmer` of 0 counted as 1: a clamped warmth is never the visible change (#1672: "one visible level, never the colour"; #2655: "down, never up"). At level 1, off or not known, nothing is asked and the journal says why (`the person's light is at its dimmest: no evening glow`, `the light is off`, `the light shown is not known`). With `warm_at_dimmest = true` in `relative` (#3691, Glad and Lively), only with `warmer = 1`, a person at level 1 whose colour can still warm (white) is asked the warmth alone, steady at level 1, for its `for`: no level up, no breath; already warm, or a colour they chose, nothing, as without it. It is then a colour light like any other, given back after its `for` |
| `do.show` | no | A light show (#1679), `shows/NAME.toml`: its `[[light]]` frames, 2 to 30, each `colour`, `level` (1 to 5) and `for` (2 s to 20 s), with a `pattern` as a light's, the whole show a minute at most: slow enough to read as a light that changes, never a flicker. Each frame's light in turn, asked of the ring with `LAMP_LIGHT_REQUESTS=on` (its breaths move) or set through the actions part (steady), then the light given back as any light of the run. It stops before its next frame, looked at every 200 ms, once the body is pushed, the lamp paused or the voice listening (someone spoke), and the light is given back then. Only in a skill its `tool` alone starts: a show is played only when a person asks, never on its own. Needs `light` |
| `do.say` | no | A note for the voice to say in its own words, through the voice's say service (#954). One a skill. The run's other steps are made, in their order, when the voice begins the say, so the look and the light come with the words, and none if it is never begun. A light changed is given back when the say ends. A light asked of the ring is covered by the Speaking face while the voice speaks, unless `LAMP_SKILL_LIGHT_SPEAKING=on` (El on #1722, off unless set): then the face is tinted by it, in its colour, its least glow always level 3's (0.44 of the ceiling) whatever level the skill names, so a skill can't brighten it, as the expressions' accents ride over it ([ring-choreography.md](ring-choreography.md)). In a tool's run the note goes to the model in the call's answer instead. Or `{ from = "schedule" }` or `{ from = "tasks" }`: words the voice keeps itself and says when asked, the reminder due or the news of work handed over (#939, (c), the first builder's 12:32). The voice keeps their delivery (a reminder's acknowledgement, news told once); the skill says when, and moves and lights with them as with a note. The words the person hears are the skill's own, `[news.words]` and `[reminder.words]` (below), which the voice reads from the installed `skill.toml` each time it says one, with the skill's ask or without (#940); the framing, the quoting and cutting of the task's and the answer's words, and the delivery stay the voice's. Sent to the say service as `from` in place of `note`, at priority 8 for `schedule` and 5 for `tasks`. Any other `from`, another key beside it, or a `from` in a skill with a `tool` refuses the skill: what the voice keeps is said when a trigger starts the skill, never from the model's call (Fourth on #1033). Or a table of the skill's own words (#1684): `{ text = "..." }`, said as written, sent as the say service's `text` byte for byte, or `{ note = "..." }`, a note as above; each 1 to 900 bytes with no control characters, or a list of 2 to 16 different such lines (#3997, Head pat's seven), one picked by the time the run is made and never the one the skill picked last (`next_line`, test `a_line_never_comes_twice_in_a_row_and_every_line_still_comes`), so the words stay in the skill's file (`MOST_SAY_LINES`; test `head_pats_response_is_chosen_in_its_file`; an older lamp-skills refuses such a skill whole, as any key it does not know), with an optional `priority`, 0 to 8 (5 without it; 9 is the voice's own), and `expires`, 1 s to 1 h (30 s without it): not begun by then, the say is `expired` and its run makes nothing. The voice says `text` in its own voice through the model, which is told "Say exactly these words, in your own voice, and nothing else:" ([speaking-first.md](speaking-first.md#asked-by-another-part-the-say-service-939)): the words are never the model's own, but the voice does not check what was spoken against them. Any other key, both `text` and `note`, or the table in a skill with a `tool` refuses the skill: the model answers a call in its own turn, so a tool's say is the note for it. Below, "A say's priority, the voice's turns, and sleep" |
| `do.cue` | no | Only `"hum"`: a fixed quiet sound through Say, without a file. Shares the single say/sound slot and is refused on model-callable skills |
| `do.rest` | no | `{ op = "rest", during = { from, to } }`, or `op = "wake"` / `"cancel"` without a window. Automatic non-item triggers only; leased to Expression. Wake must complete before accompanying presentation; unknown operations are refused. A rest may add `resettle = { most, after, during = { from, to } }` (most 1 to 3, after 5 s to 10 m): a rest a night sound ended (the voice listening, never a turn) settles again once the voice has rested `after` and a fresh empty `humans/faces/tracked` says no one is in view, inside `during`, with no run and no daily claim; a turn or a face ends the night's rest (#1672, [bedtime-morning.md](bedtime-morning.md#settling-again-after-a-night-sound)) |
| `do.sound` | no | A file in `sounds/`, written as its name: `sound = "ding"` plays `sounds/ding.wav` (#938). Checked at load by the voice's own check (`lamp_voice::say::read_sound`), and the skill refused with why: a regular file, not a link, of at most 148,140 bytes; a WAV of 16-bit mono PCM at 24 kHz (the speaker path's rate), 1 frame to 3 s; no 10 ms of it louder than -30 dBFS RMS (well under the lamp's speech) and no sample past -12 dBFS. Played by the voice, through its say service (`LAMP_SAY=on`) as `{"sound": PATH}`, the file's absolute path; the voice reads it again, only from under the skills folder, and plays it as a turn of its own, so it never overlaps speech ([speaking-first.md](speaking-first.md), the say service). A lamp not at rest as a say needs it (paused, muted, speaking, listening, owing an answer, a reminder due, within 15 s of its last turn) does not play it, nor a second sound within 30 s, and the voice refuses it at once, never later; the run's `done` says why (`sound: refused (the lamp is speaking)`). One say or sound a skill: the run's other steps are made when the voice begins it, as with a say. Refused in a skill with a `tool`: a tool is called while the voice answers |
| `do.after` | no | `"say"`: this step and every one after it are made when the say ends, not as it begins: the skill's own say in an earlier step, which ends with its outcome, or in a skill with a `tool`, the line the model says after the call, which ends when `voice/state` is `resting` or `listening` again. A line that ends in `listening` was cut, the person speaking over it: then none of the steps after it is made, and the run's `done` says "the line was cut". `thinking` after `speaking` is still the line, since its sound plays on (lamp-06c1, 2026-10-16: 20 ms of `speaking`, then 15 s of `thinking`, through a line of about 2 s), so that wait ends at most 4 s after the speaking began. The run waits at most 5 s for the speaking to begin, and goes on at once if `voice/state` was not said within 3 s. Not after a say never begun. A light changed is then given back when the last clip ends |
| `do.on` | no | A trigger's `name`: the step is made only in a run that trigger started (Task news: `happy` for done, `caring` and a dimmer light for failed). A step without it is made in every run. One say for each run: one among the steps without `on`, and one for each trigger with its own. Not with `after` in the same skill *(chosen, until a skill needs both)* (#1021) |
| `do.if_choice` | no | Values of the `[choice]` (each once): the step is made only in a call (or, with the choice's `field`, a trigger's run) that chose one of them, or whose `default` is one (#1679: a scene's own light and pose). Its body, light and show only: no say, sound or look, which are the same for every choice, and no automatic cue, rest or requested-audio step. Not with `on`. An `after = "say"` on it is every run's wait: the steps after it wait in each |
| `do.look` | no | `true`: the voice's full look (#836). Only in a skill with a `tool` and no trigger: the call's answer carries `"look": true` and tells the model to say "Let me take a look."; lamp-skills makes the run's body and light before it answers, and the voice takes its full look for that turn itself as the answer's first audio plays, whatever the model says, its still in the model's next turn, or a note that it could not see if none comes (#941) |
| `do.requested_audio` | no | A fixed relative `.wav` path, only in a tool-only skill with `permissions.speak = true`: exactly one short plain `say` confirmation, then one `requested_audio` step with `after = "say"`. No other actions, triggers (including daily clock triggers), conditions, choice/flag arguments, output limits, history, search, watch, stop, quiet hours or nudge limits (`most_a_day`, `active`, `budget`, `rest`). The ordinary `again` guard and `still` note remain available and confer no audio authority. Voice plays at most 60 seconds of checked 24 kHz mono PCM16 under the same spoken owner after confirmed confirmation retirement. Advertisement and execution require exact `LAMP_REQUESTED_AUDIO=on`; pack/install may validate while off. See [requested audio](requested-audio.md) for private context, leases, revocation and asset bounds |

A duration is a whole number and one of `ms`, `s`, `m` or `h` (`"5m"`).

A `changes` trigger keeps one bounded tuple, not a publication history. Null,
boolean, number and short string values are compared exactly; integer identities
are not rounded through floating point. A false condition is remembered even if
it becomes true again between trigger-topic messages. The next fresh eligible
message on the trigger topic gets one chance. Expired trigger or condition data
also breaks eligibility; no action is made from an already-expired publication.
Face presence and face-count conditions, including `faces_at_most`, remain
eligibility guards rather than selected scalar fields. They may share the
trigger topic with an additive selected scalar field. For face-count guards,
invalid or expired data inhibits the run; a fresh eligible return grants one
chance.
Unchanged trigger and condition definitions keep their snapshot across reloads;
changed definitions begin a new snapshot. These rules do not change `appears` or
`present`, and do not coalesce independent triggers of a skill.

The Reminders rule selects `store_id`, `pending`, `oldest.id` and
`oldest.delivery`. Stamps, sequence, revision and increasing lateness are not
new chances. It therefore asks once during a stable resting interval with a
positive count, again when the selected queue data changes, or at the next
Schedule heartbeat after readiness, enablement or resting returns. It has no
cooldown delaying a new change. These are skill chances; Voice still chooses
and records the actual delivery, and movement still waits for `begun`.

Compatibility limit: an older Skills parser rejects the new `changes` rule.
Core skill files are installed separately from the executable; a rolling update
writes their bundle before restarting Skills. If the older running process
reloads in that interval, it logs Reminders as `not loaded` and removes that
definition, while other valid skills remain. Voice retains its existing
10-second skill fallback, without this skill's look. The new parser still
accepts old `appears` and `present` manifests. This is safe refusal of the new
configuration, not full behavior parity across the mixed-version interval;
no atomic activation or two-stage rollout is implemented by this change.


For `requested_audio`, the `after = "say"` boundary is the matching speaker's
confirmed retirement, not the ordinary `voice/state` wait described above.
Skills returns a private offer to Voice and never sends the media to Say or
plays it itself. Written/Messages calls, including Messages `speak=true`, lack
the required spoken context and cannot start it. The existing autonomous
`do.sound` limit remains three seconds. The [source example](../examples/relaxing-music/README.md)
is rendered into ignored artifacts before packaging; it is not a live skill
until installed, and installation grants no execution switch.
Requested-audio definitions also refuse automatic `[limits]`, rest and cue
actions, even when all corresponding permissions are granted. Their confirmation
is a short plain string; automatic `text`/`note` tables, including their priority
and expiry, cannot replace it. A fully granted `rise` trigger is still refused,
whether or not the skill also exposes a tool. Disabled
packages reserve their names and remain inventory without audio authority.

For ordinary trigger-started skills, the ask is the only admission gate. The runtime
adds no conditions of its own, such as the pause or someone speaking: per
the design above, the Policy is the one
gate for everything the lamp starts itself (#939). Until that gate exists, a
skill that must keep quiet in those cases says so with a `[[while]]`.
Routine skills additionally require their documented freshness, local-time
and durable daily admission checks; see [scheduled skill execution](#scheduled-skill-execution).

**`moves/`** holds the skill's own moves, read only as `do.body.move` names
them. The body plays one as it plays its own gestures: on a standing lamp
(a folded one stands up for it first), with the same smoothing and speed
limits, refused whole by V1's anti-tip rule when a frame would take the
centre of gravity past 22 mm off the base's axis (the journal says
`anti-tip: REFUSED gesture NAME`), and dropped, with the reason in the
journal, while the lamp is paused, held by a hand or slumped. A skill never
writes an arm goal of its own. Whether a move may play from the pose the
lamp is in is known only to the body, so a skill that loads may still have
its move refused there. Ordinary **`do.sound`** reads only the files it names
in **`sounds/`**; `requested_audio` uses its fixed checked path described above.
A **`run.py`** refuses the skill, because nothing
runs it yet.

**`tests/`** has one folder for each case. A case holds the `.jsonl` files
`lamp-bus record` wrote and an `expect.toml`. The expect file lists, as
`[[expect]]`, every call the case may make, in order, or says `none = true`:

    [[expect]]
    at = "600s"                # optional: when, from the case's first message
    within = "1s"              # optional; without it, exactly
    call = "body.gaze"         # body.gaze, body.gesture, body.move, body.mood, light, show, say, sound, look, text or ends
    args = { look_at = "face" }

A call matches when its name is the same and each argument named in `args`
is the same. Arguments it leaves out are not compared. An argument written
as a list matches any one of its values, for a clip picked at random
(`args = { gesture = ["dance_chill", "dance_waltz"] }`). The arguments the
runtime gives are:

- `body.gaze`: `look_at`, `x`, `y`, `size`
- `body.gesture`: `gesture`
- `body.move`: `move` (its name)
- `body.mood`: `mood`, `for_ms`, `toward` when it turns, and `until_spoken` in a search
- `light`: `colour`, `level`, `for`, `pattern`, `keep` when kept, `until` when held
  (#1681), `no_brighter` when capped (#3691), or `dim`, `until`, or `out`,
  `over_ms`, `until` (#1677), or `relative` (`warmer`, `dimmer`, and
  `warm_at_dimmest` when asked) with its `for` and, with a breath, its
  `breathe_ms` (#3691): the level and colour are made against the light
  shown as the run begins, which a replay has none of, so a case names the
  light as asked (the cap and the warmth alone are `glow`'s unit tests)
- `show`: `show` (its name in `shows/`), `frames` (how many)
- `say`: `note`, or `from` for what the voice keeps (`say = { from = … }`), and `task` when an item's run names its task (#1237); from a say table (#1684), `text` for exact words or `note`, with `priority` and `expires_ms`
- `sound`: `sound`, the name in `sounds/`
- `look`: none
- `text`: `text`, a watch's line on `messages/out` (below, "A watch")
- `ends`: none; the trigger of a run with `until = "ends"` stopped holding,
  or its topic's newest value expired or went silent, or a reload found its
  skill or trigger gone, so what it held is given back. In a replay, an expiry or a silence is seen at the first
  message of any topic after it

A case's local time is not known unless its expect file sets the time of
its first message, as `clock = "10:00"`; a skill with `[quiet]` hours makes
no call in a case without it. The replay tells that time again at every
message, on 1 January 2026, as a live host tells its clock, so a trigger's
`during` window, which needs a wall clock told within 2 minutes, holds
through a case of hours (#3504, A meal check).

A skill a person starts by its `tool` has the model's call in its case:

    [[call_tool]]
    name = "dance"
    at = "0s"                  # from the case's first message
    args = { style = "calm" }  # optional: the tool's [choice] and [flag]

`[[call_tool]]` with `name` (the skill's `tool`, or its `[stop]` function, whose call makes nothing in the case: its words are the model's answer, #1685), `at` and, if the tool takes them, `args`
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

### The onboarding walk's data

`[onboarding]` holds the first-five-minutes walk's checked order and notes
(#2824), and is required for the onboarding core grant. Only a core
onboarding manifest may supply it; missing or invalid data rejects loading
and packaging. Voice also requires the skill to be enabled.

`order` is a permutation of the nine stable step IDs; `reoffer_after_s` is
1 to 31,536,000. `[onboarding.steps.<id>]` requires `first` and `reoffer`;
only `who_you_are` and `preferences` may have `.nudge` with `words` and
`after_answers` (1 to 8). `[onboarding.words]` holds every coaching,
completion and name-confirmation note. Notes are 1 to 900 bytes, no
controls, with only the exact `{location}` / `{name}` slots where specified.
Consent clauses remain mandatory in code: `who_you_are`'s `first` and
`reoffer` each retain `only after a clear yes call remember_my_face`, and
`room_sounds`' and `face_expressions`' two notes each retain `done only
after a clear yes`. Removing or altering any of these clauses rejects Voice loading and Skills loading/packaging;
surrounding prose may change.

Voice reads the installed regular file, at most 65,536 bytes and not a link,
once per service invocation, with no built-in fallback; a file-only update
or repair needs a Voice restart. Physical checks and privacy stay in Voice.
See [onboarding.md](onboarding.md#installed-walk-data) for all fields and bounds.

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

    lamp-skills [--dir DIR] [--bus-root DIR] [--gaze-socket PATH] [--mood-socket PATH] [--actions-socket PATH] [--say-socket PATH] [--tools-dir DIR] [--state-dir DIR] [--clock-control PATH] [--known-file PATH] [--zone system|NAME | --utc-offset-minutes N]
    lamp-skills test SKILL_DIR...
    lamp-skills install [--dir DIR] [--state-dir DIR] SKILL_DIR|PACKAGE.tar
    lamp-skills remove [--dir DIR] [--state-dir DIR] NAME
    lamp-skills list [--dir DIR]
    lamp-skills pack [--out DIR] [--key-file PRIVATE] SKILL_DIR
    lamp-skills asked [--dir DIR] [--key-file PATH]

(`install`, `remove`, `list` and `pack`: below, "Core and installable
skills".)

- `--zone` or `--utc-offset-minutes` is the lamp's local time, as the
  Policy, Expression and memory take it, read with the wall clock at each
  message, for a skill's `[quiet]` hours (#1021). `--zone system` follows
  `/etc/localtime` (setup's zone, #820) and `--zone NAME` a named zone, each
  with its offset at that moment, so quiet hours stay at the same clock
  hours across summer time (#1384); `--utc-offset-minutes` (-720 to 840) is
  fixed all year. One or the other; a zone that can't be read ends the start.
  Without either, a skill with quiet hours never starts itself.
- `--state-dir DIR` is the part's own folder, where the skills a person
  turned off with a skill's `[stop]` are kept across a restart: `DIR/off`,
  one skill's name a line (#1685). It is read at the start, and the journal
  says which, and again every 2 s, when the folder is looked at, and before
  a `[stop]`'s off or on is written: the app turns a skill off and on there
  too, through the manage helper (#2815, "The manage helper" below), and
  the journal says `the skills turned off read again: the app changed
  them`. Both write it with one function (`lamp_skills::keep_names`), and
  read it with one (`lamp_skills::read_names`). An off stops what a skill
  does by itself (its triggers), as a `[stop]`'s does; its function, asked
  for in a talk, still answers. Without it, off lasts until the program
  stops. With
  `LAMP_SKILL_ROUTINES` on, `DIR/rested` keeps the night each routine
  rested (#1672, #2757): one line a skill, its name and the local day the
  night began (`bedtime-morning 20453`), nothing about a person. Written
  whole as `off` is when a rest is first admitted, kept through the
  morning's wake, and cleared when that skill's next night's rest begins
  or the skill changes; read at the start, so a restart between the night
  and its morning still has the morning's wake and a `yields_to` know the
  lamp rested. The product's
  unit gives `--state-dir /var/lib/lampos-skills-state` with
  `StateDirectory=lampos-skills-state` (mode 0700, which the daily journal
  requires), the folder #1732's daily journal shares.
- It loads every folder in `--dir` (`/var/lib/lampOS/skills`), looks at the
  folder every two seconds, and loads them all again when a file other
  than a test changed. Cooldowns are kept across a reload.
- It subscribes once to each topic that a trigger or a `[[while]]` names,
  and to `humans/faces/tracked` for a skill that looks at a face, and keeps
  each topic's newest value. A look uses only a list of faces whose lifespan
  still lasts; with none, the other steps run without it. Time comes from its own monotonic
  clock, read when a message arrives.
- It waits on every topic's socket, the Policy's answers and its own wake
  at once (`poll`), at most 250 ms. So a message, a tool call, a test's
  skip or a run's end is taken as it comes, and each topic still gives one
  message a pass. Before #2381 it waited on each topic in its turn, the
  250 ms shared among them, and a tool call waited out a topic's turn:
  about 49 ms each with three topics on the Intel Mac, against under 1 ms
  after (#1806, `tests/it/call_wait.rs`). A socket that says an error or a
  hang-up with nothing to read is left out for the rest of that pass, so
  the loop doesn't spin on it, and a call or another topic's message still
  ends the pass at once.
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
  `{"skill","note","priority":5,"expires_in_ms":30000}` (or `"from"` and its priority in place of `"note"`; or, from a say table, `"text"` or `"note"` with the table's `priority` and `expires_in_ms`, #1684), then a `begun`
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
  actions part is not called for the light. A light file's `pattern` rides
  on each ask and renewal (a dim is steady), and `done` says it (`light
  warm: asked of the ring at level 3, breathing every 6000 ms down to 1`,
  `flickering as a candle every 1500 ms`, `with one pulse of 4000 ms`).
  The ring plays one pulse a minute from a skill, counted from the last it
  drew: a run counts a pulse when the ring says its light was drawn
  (`light/reached`), and a second within the minute is said `the pulse is
  not played: one a minute, and the last was shown N s ago`, and the ring
  draws it as played. A pulse no resting frame drew (a turn over it, or
  outranked) is not counted. Without the switch a pattern
  is steady, and `done` says it needs the ring's switch. `done` says
  `light warm: asked of the ring at level 5` and
  `light given back: asked again with a time already passed`. When the
  ring shows another light over the run's (a higher priority, a newer
  skill's, or the person's), it says so on `light/outranked`. The run
  listens from before its first ask. It writes `lamp-skills: skill:<name>'s
  light is outranked by <by>` in its journal at the next renewal, and
  adds `; outranked by <by>, ...` to the give-back line, each `by` once,
  in the order heard (#940). It keeps asking, so its light comes back by
  itself once the other lapses.
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
  `crates/skills/tests/it/skills.rs` skips where it would sit through a reload,
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
  Guard-mode fixtures wait for their exact `skills/events` mark-shown
  transition before publishing a visit. After a 120 s or 600 s clock skip,
  they first observe the mark becoming unconfirmed, then refresh it and
  wait for the new shown event. A skip acknowledgement alone does not say
  that the ring feedback has been read: several skips can be answered in
  one call before that work. The 30 s confirmation test retains its
  original 500 ms quiet drain after each 10 s skip, checking every renewal
  and at least three in total. That observation allowance does not prove
  the renewal thread has armed its next wait. The paused-light test keeps
  the original 500 ms allowance after light-on as an explicit no-text
  observation, followed by the original 300 ms check after the next visit.
  The add-on process pair in `crates/skills/tests/it/addon.rs` also replaces
  settling sleeps with acknowledgements (#1806). With `LAMP_ADDONS=on`, each
  of three resting-state publications waits up to 10 s for `quiet-say`'s
  `done` event before publishing the next; every event is kept for the
  assertions. This observes each proposal or limit refusal separately,
  without coalescing the latest-state triggers. Without the switch, the
  fixture waits for the loaded announcement, then asks for a zero-duration
  clock skip: the next main-loop skip handler is after startup subscription
  setup, so its acknowledgement precedes the subscriber absence check. A
  zero skip after each publication also lets the loop consume pending input.
  These skips advance no time and do not promise that a run has completed;
  the enabled case uses `done`, with no clock-control flag. Both cases keep
  their final 500 ms proposal and 100 ms event absence drains, and all
  original assertions, including the real 10 s proposal limit.
  Timings and regression controls are recorded in the
  [add-on completion verification](verification/skills-addon-completion-20261021/README.md).
  The add-on hour-budget fixture (`a_fifth_say`, #1806) waits for new,
  complete Done journal lines before sending another trigger. One byte
  cursor advances across every phase, so a preceding phase's lines are
  consumed once. After dropping four says, it waits until all four report
  their actual 300 s drop hold before asking a fifth add-on. One eight-second
  host watchdog starts before the first proposals, below the unchanged
  ten-second proposal allowance; expiry fails the fixture. The observer
  checks the child and compares observed bytes with the previous prefix,
  limiting each pending read to one MiB. The fixture owns a regular file
  without rotation; a same-prefix replacement or truncate-and-rewrite
  between polls is not detected. It polls only while a phase lacks its
  expected completions. Final diagnostics and local I/O remain subject to
  the outer process watchdog. The fixture's
  proposal-only runs report Done through the main loop, which clears their
  busy state before consuming the next voice input. This test observes its
  journal assertions; it does not require delivery of nondurable bus events.
  The undecided case retains its full three-second no-proposal observation;
  the product's hour budget and drop hold are unchanged. Neither event loss
  nor any other historical failure cause has been established.
  The quoted-say fixture (`a_say_decided_told`, #1806) owns its stand-in
  listener on the test thread, with no detached accept worker. A complete
  request still has two seconds after the decision is published; a late
  request fails. Positive cases answer it and wait for both complete
  journal lines naming that exact add-on and proposal: the SAY outcome and
  Main's `Done: proposed say as ... on addon/proposal`. These writers may
  finish in either order; both are observed before closing the child,
  rather than sleeping 200 ms.
  The observation envelope remains two seconds plus 200 ms. Negative cases
  watch all of it for an unexpected request, retaining the original full
  two-second absence check and the privacy snapshot boundary. Partial input
  and output keep those same deadlines. The original quoted fields, wrong
  judge/drop refusals, unsaid outcome and no-words assertions are unchanged;
  the child is closed before the final journal snapshot.

`lamp-update install` puts it on a home lamp (#811): the product unit
`deploy/product/lamp-skills.service`, every product skill in
`/var/lib/lampOS/skills` (since 19 October: Welcome back, Reminders, Task
news, Stretch break, Stuffy air and Dance, plus Bedtime and Morning installed
but disabled pending its lamp comparison, `install::PRODUCT_SKILLS`, which
is every core skill; Light out, Light
scenes, Point the light, Light show, Smoky air, Dusty air and Feelings are installed
disabled until their lamp checks or replays, `install::PRODUCT_SKILLS_OFF`;
Desk tutor and its marks wait for a Live session to take their functions, and the
other installable skills and the parked Laugh glance are left out,
`install::NOT_PRODUCT_SKILLS`), and the product voice with the say service
on, no hello of its own (#2825), and its reminders and task news waiting for their
skills (`LAMP_SCHEDULE_SPEAK=skill`, `LAMP_TASK_NEWS=skill`; install.md).
A skill with a tool costs one of the 32 functions a Live session takes (48
on the second route's chat requests, `second_provider::MAX_TOOLS`), after
the voice's own and the actions part's: a product lamp's are 31
(`crates/update/tests/functions.rs`). Past 32 a core skill's function is
never the one cut (the owner's rule, on #2614): the tools file names core
skills' tools in `core` beside `tools` (`lamp_skills::tools_file`), and
the voice cuts the other skills' functions, the last first, after the
second route's fetch and time elsewhere and `reports_state` (with
`LAMP_REPORTS=on`) and before its weather and its search (`tools::uncut`,
`worker::chat_cut`). With `LAMP_USE_SKILL=on`, `use_skill` carries the
core skills and is never cut either (#2644). The lamp's own and core skills'
functions alone past 32 are an error line naming what was left out
([install.md](install.md)).
An update round brings them up to date from the build (#2014's P2,
[updates.md](updates.md), "The product's skills"): each build carries the
product skills as one signed file, `lamp-product-skills`, and a round writes
each file still install's (or a round's) as recorded, keeps a file the lamp
changed and never touches a skill the person installed, judges the folder as
it would leave it with the build's `lamp-skills list` first, and puts the
files back with a build it puts back.
`deploy/units/lamp-skills.service` stays the template for a lamp set up by hand.
The product unit passes `--zone system`, the zone setup saved (#820), across
summer time (#1384). lamp-4ace's passes neither yet, so there a skill with
`[quiet]` hours never starts itself. Install puts Stuffy air on a home lamp;
lamp-4ace's folder doesn't have it.
lamp-4ace's folder carries `lamp-skills.service`, and its voice unit has the
say service on. Welcome back is the lamp's only hello on an arrival: the
voice's own (`speak_up.rs`, Arrival, `LAMP_SPEAK_FIRST`) was removed (#2825),
so no lamp can greet one arrival twice, and a test fails if the voice grows
one again ([speaking-first.md](speaking-first.md#one-hello-from-the-skill-2825)).
While someone is at the lamp, its note is told after it a known face's name
and, for someone Mood calls `regular` or `close`, to greet them one beat
warmer (`person::at_the_lamp`), by its `who = true` permission, which no
other skill has, so Guard mode and Who was here never name anyone; the
voice refuses `who` on anything but a note; the typed name is
in the model's instruction. The voice's one hello an hour for someone
`close` (#1670) was not carried: `lamp-skills` has no cooldown by
familiarity, and the 20 minutes hold for everyone.

Welcome back carries the voice's old Arrival rules as data: a face at least 0.14 of the
picture's height (`near`), there three seconds (`for`), after five minutes
with nobody near. Where Arrival counted the time since anything was said to the
lamp or by it, Welcome back has two conditions on `voice/state` (#1140):
nothing heard or said for 5 s (resting), and the lamp itself not speaking for
30 s (`not_equals = "speaking"`). Its quiet was Arrival's 30 s until lamp-06c1
showed that in an office the voice is seldom resting 30 s at the moment
someone sits down (its own talk passes in 1 to 4 s every 5 to 60 s), so nobody
arriving was greeted; but a hello right after the lamp spoke would greet
someone it has just talked with, so its own speech still counts 30 s. The
chance waits up to a minute while the person stays (`wait = "60s"`, with a 3 s
`gap` for a look down) and is taken once both hold. Arrival forgave a face
lost for under 20 s before it counts an absence; Welcome back counts the
absence from the last near face, which comes to the same for an arrival after
five minutes.

**After a rested night** the morning greeting is the welcome back (the first
builder's ruling on #1672, 22 Oct: on lamp-06c1 at 06:01:14 Welcome back
spoke, and Bedtime and Morning's morning chance was dropped 20 s later).
Welcome back's trigger has `yields_to = { skill = "bedtime-morning", trigger
= "morning" }`: with Bedtime and Morning on, the first face of the morning
waits while its morning trigger holds or is in its `wait`, until it is
judged. The hum given is this face's hello; a morning chance dropped or let
go, or a face it does not hold for, Welcome back greets as before, and its
hello claims the morning's day: a later face gets Welcome back's hello, never
the morning's. With
Bedtime and Morning off (its default), routines off, or no rested night,
nothing changes ([`trigger.yields_to`](#the-schema-as-lamp-skills-reads-it), and
[bedtime-morning.md](bedtime-morning.md#one-greeting-after-a-rested-night)).

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
  A dim or a colour held `until = "ends"` through the actions part is given back only
  if the light is still as it was left, and when its trigger topic goes
  silent. `air/state` names no `expires_ms` yet (asked of the air part's
  owner on #1021), so a silent sensor ends Stuffy air's dim after 15
  minutes. A restart of `lamp-skills` while a dim is held does not give it
  back.

### A say's priority, the voice's turns, and sleep (#1684)

Read from the code on 2026-10-19 (`crates/voice/src/say.rs` and
`coordinator.rs`), not measured on a lamp. A say table's `priority` (above,
`do.say`) is the say service's, and changes only two things in the voice:

- **Order.** Of the says waiting, the highest priority is begun first, the
  oldest first among equals. A due reminder the voice holds is said before
  any of them, whatever their priority.
- **The quiet it needs.** From 8 up, a second since the lamp's last turn
  (`say::URGENT_QUIET_US`), as a reminder; under 8, 15 s, as task news.

It changes none of the gates every say waits for: no reply owed or queued,
the speaker free, nobody's utterance pending, listening ready with a fresh
capture and a second since the last speech heard, the playback mask free, a
ready session, and the keeper's pause off. So no priority speaks over
someone speaking (rule 8): a priority-8 line goes in the first pause of a
second, not at the end of the talk, and a person who speaks over it cuts it
(`dropped`), and it is not tried again. A line that finds no such pause
within its `expires` is spent. Smoky air's design (#1684, §4) uses this: 8,
`expires = "2m"`, and no `[[while]]` on `voice/state`; its one `[[while]]`
is the privacy switch (`voice/privacy`'s `physical_muted` not true), so a
closed switch spends the chance in the skill itself, not only at the voice.

**The busy room.** The say service has no gate of its own for a busy room:
the Policy does, when it is asked (`LAMP_SKILLS_ASK=policy`), by the
skill's `[ask] policy` (`nudge` and the rest held by the room, the hour and
the person's rules; `reminder` only while someone talks to the lamp; the
design's `warning` is not built). The product skips the ask
(`deploy/product/lamp-skills.service`), so there the only gates are the
voice's above and the skill's own `[[while]]`. A skill that must wait for a
talk to end says so with a `[[while]]` on `voice/state` and a `wait`, as
Stuffy air does; one that needs only a pause leaves it out and gives
priority 8.

**During manual sleep or privacy mute, nothing is said.** The say service is
bound once as the voice starts (`Says::start` in `run_source`), before the
privacy wait; each run takes it (`Says::for_run`) and hands it back as it
ends, and the socket is removed only when the voice ends (#3697: on
lamp-52e6's boot of 27 Oct the socket appeared only five minutes after the
start, when the switch was read open, so `lamp-setup shutdown` found no say
service). A run ends on the software mute, the sleep from a hand held on the
head (reason `sleep`) as well as the person's own, and on the privacy switch
(`SoftwareMute`, `PhysicalMute`). While the voice waits out a mute no run is
under way, and the wait answers each say at once `refused (the lamp is
muted)` (`Says::refuse_parked`): the microphones stay closed and nothing is
said. Since a say never begun moves nothing, its light and body steps are
not made either. A say waiting as the lamp is put to sleep is answered
`dropped`. Nothing is kept for the wake, at any priority: the chance is
spent, as for a say refused (with a `rearm`, until the field has passed its
line again). #1684's additions leave this as it is: its design (§3) has the
first tier not wake a sleeping lamp.

Routine-owned rest is separate from manual sleep and privacy. Its wake can
release only the rest that the routine owns; it cannot reopen either mute.
An exact or ranked say accompanying that wake still waits for correlated
completion and carries the original transition's `start_before_us`. Its
authored `expires` cannot extend that deadline, and changed definitions or
an expired permit refuse the request before the Say socket write.

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
(the units give `/run/lampos-skills`, lamp-skills' own `RuntimeDirectory`),
`lamp-skills` keeps `DIR/tools.json`, written whole at each load and
removed when no skill has a tool, and answers on `DIR/call.sock` the way
the actions part does ([tools.md](tools.md#the-skills)). Beside `tools`,
the file's `body` names the tools whose skill asks for the body
(`[permissions] body`), when any does: a text from the Autonomous app never
runs one (#1673, [messages.md](messages.md)). Primary declared tools of disabled
skills, and requested-audio tools while their switch is off, enter neither
`tools`, `body` nor `core`. The optional `core` list gives emitted primary
tools of core skills their priority at the function bound. All three lists
use the same first emitted definition for a primary tool name; an omitted
duplicate cannot add metadata for that tool. The existing auxiliary
unplug-cancellation tool is separate: a supplied inventory entry can contribute
it even when its primary tool is omitted. An empty `body` or `core` list is
omitted. These are keys of the file's own,
not of each tool, which the voice's function declarations
would refuse. It makes `DIR` and
the socket at its start, and again, with the file, at a look (every 2 s)
that finds the socket gone from a `DIR` still there, saying so. With `DIR`
itself gone it ends, not zero, with a line: as its user, under its sandbox,
it cannot make the folder again, and systemd does as it starts it once
more (`Restart=on-failure`; the reviewer and the first builder on #1656).
Until 19 Oct `DIR` was inside
the actions part's folder, which systemd removes at each restart of
actions, and the voice had no skill functions until lamp-skills restarted
(lamp-4ace, 04:33). With the voice's `LAMP_USE_SKILL=on`, the model is
given one `use_skill` function for every skill instead, with each skill's
description, choice and yes-or-no in its instruction, and the call reaches
the skill as its own (#1668). A call runs the skill's steps without
asking the Policy, since a person asked: the body and the light are made
as in any run, the light given back after its `for`, and the voice is
answered with what the model is to do next: at once, before they are
made, or, for a call with a look, once they are made (at most 800 ms,
`MADE_BEFORE_ANSWER`; a repeat call's pose made again first too), so the
head is on its way down over the page and the light up before the voice
asks the eyes (#1146) (`{"next": "..."}`): with `look = true`, to say only "Let me take a
look.", and `"look": true` beside it; with `say`, the note.

Skills installs the shared `lamp_ipc::log_relay` before handling arguments
or starting the service. A broken journal stream therefore does not make
a diagnostic panic and lose a tool reply. The relay retains at most 64 KiB,
drops lines the journal cannot take, and reports the dropped count if the
stream accepts output again. Normal command exits allow up to 500 ms for
queued diagnostics to drain; the command's exit status is unchanged. The
host regressions break a temporary journal socket after a successful tool
call, and start Skills with an open journal socket that cannot accept even
one byte. Both require the exact unknown-tool refusal and another successful
call. The full socket remains unread, with bounded preparation and a
checked 30-second scenario deadline alongside the startup and socket
timeouts. This is not a hard total-runtime watchdog. The startup reading
includes journal filling and fixture preparation; connect-to-complete-reply
timings cover local IPC, not device or acoustic latency.

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
one whose answer never reached the model (on the second route a skill's
search's own turns are the exception: see below); every dropped look is
recorded as `look_dropped` with `why`. The second route takes a tool's
look only with `LAMP_SECOND_SKILL_LOOK=on`: its engine then sends
`LookAsked` for a skills call answered with `"look": true`, says "Let me
look." itself and ends the turn, and the look's turn goes to its own
session with the say's note after "When the picture comes:" after its
question; in a turn the lamp wrote itself the look is dropped and the turn
goes on to its next round ([second-voice.md](second-voice.md)). There a search's own turns
(its look's turn, its note's) take the skill's next look, so
look-for-things looks at the next place; every other turn the lamp wrote
still takes none. Off, it answers the same
tools but sends no `LookAsked`, so there the model's words are the look,
as before. With `LAMP_SECOND_LOOK_NOTE=on` too, a search whose look got an
answer that called nothing is sent its skill's note (`[search]` `note`)
to call `look_for` with the next place, once a search, and with `LAMP_SECOND_LOOK_WALK=on` a search
left after its note is walked on by the lamp: it calls the skill itself
with the next place in its `walk` not yet looked at, sends each picture as the search's
next look turn naming the place, and the skill's give-up line once past
its looks ([second-voice.md](second-voice.md)). Any error a `[search]` skill
answers a walked call with is taken as its give-up: said once, and the
search ends, so a search skill errors only past its looks (#2280). A walk
step dropped while its call is in flight (the person spoke within its
1.5 s) still turns the head: the skill made the pose, and the line that
ended the step came before it began, so it may hold until its `for`; a
check for the lamp row, not yet handled. A push on the body that ends a
search here ends it for the second route too (#2904): the held look's
picture is dropped, the search's turn in hand is cut as a spoken stop
cuts it, no note or walk follows, no look runs for 30 s in a turn the
lamp wrote or one asked before the push, and the person's next request is told they stopped
the search with a hand ([second-voice.md](second-voice.md)). When the turn is over the look's
written turn goes with the still. The still cannot go in the call's answer
itself: a function's answer to Gemini Live is words, and the call is
answered at once while the still takes up to 2.5 s. If no still with
this ask's id comes within `STILL_WAIT` (2.6 s), the look's turn goes with
no picture and tells the model that it could not see and must say so,
never guess (`NO_LOOK_NOTE`). Without `LAMP_LOOK_FULL=on` nothing of this
happens and, as before, the model's own words are the look. A tool's name is one
skill's: the core skills take their names first, then the installable ones
in their folders' order, and a skill whose tool's name is taken is not
loaded, the journal saying whose it is (`lamp-skills: not loaded:
…/a-dance: its tool "dance" is the core skill dance's: …`), so an installed
skill never takes a core skill's function from the model; `install` refuses
one before it goes in (below). Without `--tools-dir`, or with no
skill that has a tool, nothing changes.

A call is refused, in words the lamp can say, and nothing is done, when the
skill's `[[while]]` conditions do not hold as a trigger would need them
(on a value that still lasts, for their `for`). While its last run is
still under way (a light's `for` not yet over), a call again is told again
what to do next, the look and the say, and nothing more is made: the light
already on stays and is given back when its `for` ends, so a student who
asks again a minute later gets a new look (#941, the first builder's 12:09).
Only its poses are made again, the new call's in place of the one held (a
pose turned toward another place, #1679), and a light the person keeps
(`keep = true`: a scene changed); a call whose run has no pose lets the
held one go, so the head comes back up.
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

### A watch

Guard mode's kind (#1685, [guard-mode.md](guard-mode.md),
`crates/skills/src/watch.rs`): a skill that is on only while a person has
asked for it, and tells them by text who came to the desk while they were
out. It has no trigger and no steps; its `[watch]` says what it does:

```toml
[watch]
every = "10m"                           # at most one text this often (1m to 24h)
asked_within = "30s"                    # on only when someone spoke this lately (5s to 2m)...
if_heard = ["keep an eye", "guard"]     # ...asking for it: 1 to 16 entries, each whole words
most = "12h"                            # it ends by itself this long after it was asked (1h to 24h)
mark = { colour = "blue", level = 1 }   # a faint light while on: level 1 or 2, steady
```

- **Turned by its tool.** A call with `turn = "on"` turns it on only if
  the newest thing heard on `voice/heard`, within `asked_within` of the
  call, has one of `if_heard`'s entries among its whole words, any case (a
  typed request is not heard; a call after talk of something else is
  refused), and the ring takes `light/request` for its mark; else the model
  is told why. `off` always turns it off. Nothing else
  turns it on: not a message, not a load, a reload or a restart, which
  leave it off. A reload that changes its `[watch]`, or removes it, turns it
  off and gives its mark back.
- **The mark** is asked of the ring while it is on, as any skill's light
  with `LAMP_LIGHT_REQUESTS=on`, and given back when it goes off. **While
  the ring doesn't show it, the watch takes nothing about the desk** (the
  peer read on #1807): until `light/reached` says it is shown, no visit
  begins, ends or is texted, and `skills/events` says so; from the moment
  it is turned on too, so a ring that never answers never has anyone
  watched without a sign (reviewer-two on #1937,
  `nothing_is_taken_until_the_ring_first_shows_its_mark`). The ring says a
  shown mark again after each renewal (every 10 s); with no such word for
  30 s (`watch::CONFIRMED_WITHIN_US`: the ring worker stopped, or it forgot
  the claim), the mark is taken as hidden, `skills/events` says "its mark
  has not been confirmed for 30 s: no visit counted", and the ring's next
  word counts again (Third's privacy read on #1963,
  `a_mark_the_ring_has_not_confirmed_for_thirty_seconds_counts_nothing`,
  `guard_mode_counts_nothing_once_the_ring_has_not_confirmed_its_mark_for_30_s`).
  A case's recording says the ring's word as `light_reached.jsonl` (and
  `light_outranked.jsonl`), taken as the program takes it; a replay looks
  at no time between messages, so the 30 s rule and `most` are not
  replayed. The first
  builder's four rules for when the ring says it is outranked
  (`light/outranked`), each pinned by a test (#1937):
  1. **The person's own light over it pauses the watch** ("turn the light
     off" on the way out): no new claim is made over their light, nothing is
     counted, and one text, "I've paused watching while your light is off."
     (`the_persons_light_pauses_it_until_they_ask_again`,
     `guard_mode_pauses_under_the_persons_light_until_they_ask_again`).
  2. **Another program's light over it is asked over again:** the mark is
     given back and asked for with a new claim, at most once every 30 s, so
     a light that outranks even that stays on top and the watch stays paused
     (`guard_mode_counts_only_while_its_mark_shows_and_asks_for_it_again`).
  3. **Paused, it goes on only when asked again in words** (`if_heard`):
     asking is the person's yes to the mark over their light. Its mark is
     then asked for anew, and it counts once the ring shows it (the same two
     tests).
  4. **Their light going on alone doesn't resume it:** it stays paused until
     they ask, or until its `most` ends it
     (`guard_mode_pauses_under_the_persons_light_until_they_ask_again`,
     `a_pause_lasts_until_asked_or_its_most`).

  So the mark never replaces a light the person just chose unasked, and the
  watch never watches unseen.
- **Its most:** `most` after it was asked it ends by itself, its mark given
  back, with one text, "I'm off guard: it's been 12 hours." (#2562).
- **Its other ends** (#2562): the asker's own face back says "I'm off
  guard" aloud (`speak`); the privacy switch closed ends it at once with one
  text, "Guard is off: the privacy switch is closed.", and no watch goes on
  while it is closed.
- **Arrivals by sound and one turn of the head** (#2562): with
  `LAMP_GUARD_SOUNDS=on` and `ears/classes` in its permissions' topics, a
  door or footsteps is someone come in only when a face or speech follows
  within 30 s; a visit's first face turns the head to it once, on the gaze
  socket (`body`), and not again within 2 minutes.
- **From the owner's app** (#2562): `lamp-cloud`'s `guard.on` and
  `guard.off` reach the call socket as `{"op":"guard","turn":...}`, taken
  only with `LAMP_GUARD_FROM_APP=on` and from `lamp-cloud`'s unit
  (`watch::from_the_app`), for the skill `guard-mode`; its mark still asked
  and confirmed before anything counts ([guard-mode.md](guard-mode.md)).
- **Visits** count once `room/presence` has said nobody since it was asked.
  A visit runs from someone there (or a face at least 0.10 of the picture's
  height) until `room/presence` says nobody; its end is one line on
  `messages/out` (`lamp_msgs::MessageOut`, `from` `skill:<name>`, stamped by
  the program), at most one each `every`.
- **Known faces** come from the eyes' known file, `--known-file PATH`
  (`lamp-eyes --known-file`, renewed every 2 s while a known person is at
  the lamp, older than 5 s read as nobody known). A watch says only known or
  not; the name stays in the program, to tell the person who asked, whose
  face back after they left ends the watch, from anyone else.
- **The privacy switch**, `voice/privacy`'s `physical_muted`, is looked at
  with the known file on each pass of the program's loop while a watch is
  on: closed, one line each closing, none within a minute of the last; and
  while it is closed the watch takes nothing about the desk.
- **Its permissions:** `light` (the mark), `messages` (the text) and
  `known_faces`, and `room/presence`, `humans/faces/tracked`, `voice/heard`
  and `voice/privacy` in `topics`; without any of them it is not loaded.
  Each text and the mark are checked again as they leave the program.
- **Its cases** are replayed through the watch: the mark is `light`, the
  mark given back `ends`, a text `text`. The known file is not on the bus, so
  a case says it as `[[known]]`, `at` and an optional `name` (none: nobody
  known), in `expect.toml`; only a watch's case may.

### Core and installable skills

The owner, 19 Oct 2026, 07:1x to 07:2x: "think in terms of skills all the
time. that's the modular architecture design we do with skills, topics, and
actions"; "the idea is to have a lot of skills and people can pick and
choose"; "we should have core skills and installable skills. but we should
try to build everything as skills. core skills like core apple apps right
always installed on the phone and then installable skills like installable
apps". Earlier: an owner can make a private skill, and choose to publish it
to a Skills Store for others. That last step is built as fourteen-P13
(`LAMP_SKILLS_SHARE`, off): the app's Share sends a skill made on the lamp
to the Store's owner upload, where a reviewer reads it before it is listed
([cloud.md](cloud.md), "A made skill sent to the Store").

As built on 2026-10-19 (`crates/skills/src/package.rs`; not yet run on a
lamp):

**The kind.** `skill.toml` says `kind = "core"` or `kind = "installable"`,
and a skill that says neither is installable. `kind = "addon"` is a
partner's add-on ([addons.md](addons.md)): installed and removed as an
installable skill, but it only proposes. It says its version; its
`[permissions]` give `propose` (some of `say`, `light`, `move`, `mood`,
`note`) and topics from the add-on allowlist alone
(`lamp_msgs::ADDON_TOPICS`), and nothing else; it starts by its triggers
alone, with no `[ask]`, tool, `[history]` or other tool's tables; and its
steps are `propose = { act, why, sure, holds, ... }` only. `lamp-skills`
writes each proposal on `addon/proposal`, stamped with the add-on's name
and version and held to the add-on limits, for a judge. Without
`LAMP_ADDONS=on` an add-on is not loaded, and the journal says so.

| Skill | Kind | Why |
|---|---|---|
| Welcome back, Reminders, Task news, Stretch break, Stuffy air, Dance, Prepare-to-unplug | core | What every lamp does: the hello, a reminder said when due, the news of work handed over, a stretch with one short line, a droop and a line in stuffy air, a dance when asked. Each costs nothing until its moment (Dance's function is one of a session's 32), and each is in the product's install (#1667) |
| Light out when you leave (#1677) | core | The owner's: the light going out when nobody is there is something every lamp does. It does nothing without the eyes' `room/presence` (`LAMP_PRESENCE=on`), and until its lamp check it is installed disabled (`install::PRODUCT_SKILLS_OFF`), so turning presence on for Messages (#1673) does not turn it on too ([presence.md](presence.md)) |
| Not feeling well (#1681) | core | A failed part shown as a creature a little unwell: on `lamp/health`'s `error` for 30 s, or its `quiet_ms` past 5 minutes (a part that came up well and stopped), one sleepy droop and a pale, slow breath until it ends. In the product's install since its lamp check: row 8 on lamp-52e6 with the lead, 20 Oct 11:35 (`install::PRODUCT_SKILLS`) |
| Smoky air, Dusty air (#1684) | core | **Held until measured.** The air at the desk suddenly smoky (PM2.5 up 100 µg/m³ within 3 minutes, to 150 or more, held 30 s): one exact line, a small lift and an amber breath, once a spell, at any hour, never while the privacy switch is closed; and very dusty for an hour (125 or more): one exact line, at most once a day, in the daytime, with someone at the desk. Neither is a smoke alarm, which only the docs say (here and [air.md](air.md)): their lines, their `SKILL.md` descriptions and what the model is told to answer never say fire, alarm or smoke alarm; Smoky air's line begins "The air reads like smoke". Neither declares a `[stop]`, so a passing "stop" cannot snooze the smoke line (#1837's P3). Installed disabled (`install::PRODUCT_SKILLS_OFF`) until the four real traces (kettle steam, toast, a candle blown out, a match) are recorded on a lamp with an air sensor and their numbers are decided from the replays; their cases are written by a script, and those four are placeholders, to be replaced by the real recording (#1684). Their thresholds are provisional until then. For a trial, see [air.md](air.md), "Smoky air and Dusty air on a trial lamp" |
| Feelings (#3691) | core | What the lamp felt, shown as a creature would: Mood's `felt` on `mood/state` (glad, lively, low; [mood.md](mood.md)) as a small head move of its own and a step of light from the person's, never brighter. Installed disabled (`install::PRODUCT_SKILLS_OFF`) until its lamp check, and it does nothing without Mood (`LAMP_MOOD=on`), which stays trial-only with Tone |
| Light scenes, Point the light, Light show | core | Asked for by a person, never on their own (#1679): a scene's light, kept, with the head over the desk for reading and focus; the head over the desk and turned toward what the model's look saw there ("light my book"); a show of slow-changing colours under a minute. Installed disabled (`PRODUCT_SKILLS_OFF`): not yet tried on a lamp, and enabled, their three functions would put a product session past its 32 without `LAMP_USE_SKILL=on` |
| Look for things | installable | "Where are my keys?" (#1685): the owner's call, 19 Oct: an installable skill, and the whole search is fine since a person asked. The desk first, then two places either side, five looks at most, the head down over the desk throughout; the model's look finds the thing, since the eyes find faces only. Not in the product's install: not yet tried on a lamp, and a session past its 32 functions without `LAMP_USE_SKILL=on` |
| Guard mode (#1685) | installable | Turned on by voice while the person is out; texts them about visits to the desk ([guard-mode.md](guard-mode.md)). Not in the product's install until it has been tried on a lamp (`NOT_PRODUCT_SKILLS`), and its texts reach the phone only with `LAMP_MESSAGES=on` in `lamp-cloud` ([messages.md](messages.md), "Unasked texts") |
| A sip of water, Rest your eyes, Stand up, Wind down (#1685) | core (since 25 Oct) | The wellbeing nudges beyond Stretch break, each with one short line the voice words (the owner, 25 Oct: "talk to the user"), only when nothing has been heard or said for 5 s and the lamp itself has been quiet 30 s, beside: a slow blue breath after 90 minutes at the desk (10:00 to 17:00); a small tilt and a shade softer light after 45 (09:00 to 20:00, once an hour); a turn and a tilt after 100 minutes of Stretch break's sitting (08:00 to 20:00); a droop and a shade of dim until they leave, in the hour before bedtime (its `[active]`, 22:00 to 23:00 as it comes, never from bedtime on, the night then Bedtime and Morning's). Each has `most_a_day`, `[active]`, `[quiet]`, `[rest]`, Stretch break's `[[while]]` on `room/activity` (never while someone reads or writes head down, #1678; nothing without the eyes' `LAMP_ACTIVITY`), a second on its `sitting_s` (above the trigger's `for`, `unknown = "holds"`: the sitting is the sitter's own, since the eyes begin it again when someone else sits down, so a face in the chair for 90 minutes that was two people is no 90-minute sitting; cases `a-sip-when-the-eyes-say-the-same-sitting` and `not-when-someone-else-sat-down`), one shared `[stop]`, `wellbeing_nudges` (the one that came last, or `all`), and `[budget] group = "wellbeing"`, `per_hour = 1`, with Stretch break: one an hour between them. In the product's install (`PRODUCT_SKILLS`) since 25 Oct, on |
| Head pat, Head pat wiggle (#3997, #4133) | core (since 29 Oct) | A pet on the head, the pads taking turns (`controls/pet`, `LAMP_HEAD_PET=on` in the product's lamp-controls): a small wiggle of the base 2 s after every pet, and a short line at most once in 10 s, the design owner's choice at the lamp. #4133's design owner's yes after the lamp check on lamp-06c1 (#3997, 29 Oct) |
| A comfort break (#3504) | installable | V1's bathroom-break reminder, which the owner asked to bring back (the lead's list, 27 Oct): after two hours of Stand up's sitting (`near = 0.14`, `gap` 2 minutes, the eyes' `sitting_s` above 7199), 09:00 to 20:00, one short line the voice words, that it's a good moment for a short break. It never names a place or a need (its line, its `SKILL.md` and its name; test `a_comfort_break_never_names_the_bathroom`), and it is held while anyone else is in view (`faces_at_most = 1` for a minute; its `wait` 30 minutes keeps the chance, then lets it go): never said in front of others. Once a sitting, twice a day, `[quiet]` 22:00 to 08:00, `[rest]`, and the wellbeing group's `[budget]` and `[stop]` (`wellbeing_nudges`, whose `about` it carries, since it comes first in folder order). Not in the product's install (`NOT_PRODUCT_SKILLS`) until its played check on a lamp |
| A meal check (#3504) | installable | V1's meal reminder, which the owner asked to bring back (the lead's list, 27 Oct): two named triggers, `lunch` (`during` 11:30 to 13:30) and `dinner` (18:30 to 20:30), each on a face at the desk (`near = 0.14`, `gap` 2 minutes) for 75 minutes of its window, so the question comes from 12:45 or 19:45 and not to someone who went out to eat. One open question the voice words, whether they've had lunch or dinner yet, naming no food, menu or place (test `a_meal_check_names_no_food_and_takes_no_picture`); held while anyone else is in view (`faces_at_most = 1` for a minute, its `wait` 30 minutes, ended by the window's end). Once a window (`cooldown` 120 minutes), twice a day, `[quiet]` 22:00 to 08:00, `[rest]`, and the wellbeing group's `[budget]` and `[stop]`. It does not hear whether they spoke of eating: that would read what was said. Not in the product's install (`NOT_PRODUCT_SKILLS`) until its played check on a lamp |
| Sit back (#3694) | installable | V1's posture reminder, which the owner asked to bring back (the first builder on #3504, 27 Oct). Two named triggers on the eyes' `room/activity` (`LAMP_ACTIVITY=place`, the product's): `leaned_in`, `leaning_in` held 10 minutes (the face 1.25 times as big as it sat, against this sitting's own baseline, never a fixed line; a sit-up under its 20 s `gap` does not end it), turns to them and one short, kind line the voice words, that it might be a good moment to sit back; and `sat_well`, `at_desk` held a minute within 15 minutes of that line (`follows`), a short warm word. Neither speaks of a body or looks (`sit_back_says_nothing_of_a_body_and_takes_no_picture`). With nobody else in view a minute (`faces_at_most = 1`), the voice resting 5 s, the lamp quiet 30 s, and the sitting past 12 minutes (`sitting_s`, so a warm word is never for coming back from a gap over 2 minutes, a new sitting); held up to 10 minutes while still leaned in; once an hour, three a day, never 22:00 to 08:00; the wellbeing group's hour; two words in a row unnoticed rest it until morning (`[rest] after_ignored = 2`); the warm word not counted. `head_down` (reading, writing) is not taken as leaning in. Not in the product's install until a played check on a lamp with a person (`NOT_PRODUCT_SKILLS`) |
| Who was here | installable | "Was anyone here while I was out?" (#1685): the owner's call, 19 Oct: an installable skill. The model is told the lamp's record of the room from `room/presence` (#1721), times only, never a picture or who (with `LAMP_WHO_WAS_HERE_KNOWN=on`, at most "someone the lamp knows", and with `LAMP_WHO_WAS_HERE_NAMES=on` the name of someone who said yes to being named, #1997). Not in the product's install: not yet tried on a lamp, `room/presence` is off in the product, and a session past its 32 functions without `LAMP_USE_SKILL=on` |
| Night sleep (#940) | core | When the lamp sleeps at night, moved out of Expression with today's numbers: 23:00 to 06:00, nobody seen, the voice resting and the lamp awake for 15 minutes, then Expression's rest (with `LAMP_REST_THROUGH_SOUNDS=on`, a sound heard as no words does not begin the voice's 15 minutes again: only words, its name, a push, a touch or the lamp speaking do; a sound with no words, a baby or an alarm too, does not, #1672); no light, sound or words, asked of nobody. Expression's bow, breath and reflex wakes are unchanged ([alive.md](alive.md#night-sleep-is-a-skill)). On in the product units (`LAMP_SKILL_ROUTINES`, `LAMP_REST_REQUESTS`), with Expression's built-in sleep as the fallback for one release |
| Bedtime and Morning | core, disabled | Installed for every product lamp, with authored evening glow, morning hum and output-limit data; its own rest and wake moved to Night sleep (#940); no active trigger or tool until enabled after its lamp comparison |
| Desk tutor, Desk tutor's marks | installable | Desk tutor reads handwriting from the full look's still (`LAMP_LOOK_FULL=on` with `lamp-eyes --full-look`), which a product lamp takes since #2791's product half. With its marks, its two functions would take a product Live session past its 32 (#1667's count). Its marks are called only after its look. Both become core once a session can take their functions (#1668) |

Which of the skills to come are core and which installable is the owner's
list in [v1-verdicts.md](v1-verdicts.md), "The owner's scope decisions,
2026-10-19".

A core skill comes with the lamp: `lamp-update install` writes it (on
the product, twenty folders with Bedtime and Morning disabled and the other nineteen enabled, and the seven of `PRODUCT_SKILLS_OFF` disabled), and a core
skill's file taken away or changed by hand is written again at the next
install and the next update round: a core skill's files are the build's,
not the person's (#1722; [updates.md](updates.md), "The product's
skills"). What the person set is kept: the skills turned off with
`[stop]` (`--state-dir`'s `off`; its `on` only takes a name out of it)
and a skill's state there. A core skill someone turned on stays on
through an install or update round (the first builder on #2585): an
update changes what the build ships, not what was chosen on the lamp. A
core skill that is off by default (Bedtime and Morning, and those of
`install::PRODUCT_SKILLS_OFF`) is turned on by editing its installed
`skill.toml` (`enabled = true`, or install's off line taken out), or from
the app (`skills.on`, #2815), which makes that edit through the manage
helper and writes the name in `on` at once. The next
install or round sees that, keeps the name in `--state-dir`'s `on` (one
name a line, beside `off`, written whole, 0644), and writes the build's
file turned on. A skill named in `on` is written turned on even when its
file is gone or the build's file changes. Editing the file back to off
takes the name out of `on`; the app's `skills.off` does that edit, takes
the name out of `on` and puts it in `off`. A lamp with no `on` has none turned on, as
before. A skill the build no longer carries leaves `on` with one journal
line (`NAME is no longer a core skill of this build: its turn-on in
/var/lib/lampos-skills-state/on is dropped`). `lamp-skills remove`
refuses it, and `lamp-skills install` refuses a folder or a package that
says it is core, or that would take the place of a core skill on the lamp.
A core skill also keeps its tool's name: the program loads no other skill
with it, including while disabled. `kind` and `enabled` are separate: disabled folders still validate and remain in inventory, while contributing no active trigger, read subscription or advertised tool.

**On a running lamp.** Four commands, each run on the lamp, as root (the
product's skills folder is root's; the part itself only reads it, under
`ProtectSystem=strict`):

    lamp-skills install [--dir DIR] [--state-dir DIR] SKILL_DIR|PACKAGE.tar
    lamp-skills remove [--dir DIR] [--state-dir DIR] NAME
    lamp-skills list [--dir DIR]
    lamp-skills pack [--out DIR] [--key-file PRIVATE] SKILL_DIR
    lamp-skills asked [--dir DIR] [--key-file PATH]

`--dir` is `/var/lib/lampOS/skills` without it, the product unit's own
`--dir` (`deploy/product/lamp-skills.service`): installable skills live
beside the core ones. What was done is one line on the standard output; a
refusal, and why, on the standard error with exit code 1 (2 for a command
it does not know).

- **`install`** takes a skill's folder or its package. The files are read
  first (plain files and folders only: a link refuses it; hidden files are
  left out; each file is opened without following a link, `O_NOFOLLOW`, and
  checked as opened to be the plain file its folder listed, by device and
  inode, so one swapped for a link or another file while it is read is
  refused, never copied; a folder in it swapped for a link while it is read
  is not caught, so install from a folder only root can write, or from a
  package, one file), then written into `DIR/.install-NAME-PID/NAME`, which the
  running program passes over (its name begins with a dot), folders 0755
  and files 0644 whatever the umask of who ran it, so the parts' user can
  read them. There it is loaded with every check a load makes, its
  permissions among them, and only then renamed into `DIR/NAME`: the running
  program never sees half a skill, and one that does not load is never live.
  A skill whose tool's name another skill in `DIR` has is refused (`not
  installed: a-dance's tool "dance" is the core skill dance's on this lamp:
  a function the model calls is one skill's`), read from each folder's
  `skill.toml`, so a skill that does not load just then keeps its name.
  A skill already there is replaced only if it is installable, by any
  version, higher, lower or none: the two are not compared (`replaced
  desk-tutor 1.2.0 with 1.0.0`). The old one is renamed
  to `DIR/.removed-NAME-PID`, the new one renamed into its place, and the
  old deleted. Between the two renames the program may see neither, for
  microseconds, against a look every 2 s *(chosen: a swap of two folders in
  one step is Linux's alone)*. Each `install` and `remove` holds `DIR` for
  its whole run (a lock on the folder itself, given back however the run
  ends), so two never interleave. Nothing is left beside the skills, whether
  it installed or not; a run killed partway may leave its
  `.install-NAME-PID` or `.removed-NAME-PID`, and the next install deletes
  every folder of exactly that shape directly in `DIR` (a skill's name and a
  number; a link or a file of such a name is left as it is) and says so.
- **`remove`** renames an installable skill's folder aside, so it is gone
  at once for the program, then deletes it. First it takes the owner's
  yes to the skill's camera out of `--state-dir`'s `camera` (below).
  The app's skills page removes a skill the same way, through the manage
  helper below.
- **`asked`** is the manage helper ("The manage helper", below): one
  request read from the standard input, its answer on the standard output.

**Skills pushed from a computer** (#3869, [control.md](control.md),
"Skills pushed from a computer"). A paired computer with the `skills`
grant puts a skill of its own on its own lamp, with no Store. The running
program loads them from a developer folder of their own,
`/var/lib/lampOS/skills-dev` (the product unit's
`LAMP_SKILLS_DEV_DIR`, or `--dev-dir`, which wins; an `Environment=` line,
as an older `lamp-skills` would end at an unknown flag, #2157), after the
skills folder, looked at every 2 s with it (`lamp_skills::load_all_with`):
- **Never in another's place.** A pushed folder with the name of a skill in
  the skills folder isn't loaded ("a skill of that name is already on the
  lamp: a pushed skill never takes its place"), and a pushed skill claims
  its tool's name only after every skill there has, core first, so a
  pushed skill never takes a core or Store skill's function, whatever the
  folders' order. Nor does it join a `[budget]` group a skill there uses,
  or share a `[stop]` with one, so it can never spend a group's hour (the
  Reviewer-lead's P2 on #3879: a pushed member of `wellbeing` would have
  quieted A sip of water). A skill there that fails to load just then still
  holds its group, read from its own `skill.toml`.
- **Held to the protocol's grants** (`lamp_skills::pushed_refuses`),
  whatever wrote the folder:
  - installable only, never `core` or `addon`;
  - none of `actions`, `messages`, `known_faces`, `who`,
    `output_limits`, `propose`, `say` or `core`;
  - topics from `lamp_skills::PUSHED_TOPICS` alone: presence
    (`humans/faces/tracked`, `room/stood_up`) and those the protocol's
    events take with no grant (`clock/local`, `voice/state`,
    `lamp/pause`, `controls/touch`). Not `voice/heard`, `name/heard` or
    `ears/classes`, which give a skill every phrase or class heard until
    format 2 lists them (Fourteen on #3869).
- **The picture held for the owner's yes.** One that sees the picture
  (`Permissions::sees_picture`: `camera = true`, or format 1's
  `look = true`) loads off, unless the owner said yes in the app, kept as
  its name in `--state-dir`'s `on`.
- **Said as pushed.** Each is said as pushed by the computer its folder's
  `.pushed.json` names (`{"computer": NAME}`, at most 1 KiB, never through
  a link), "a computer" without one.

The manage helper writes the folder; `/etc/tmpfiles.d/lampos.conf` makes
it (0755 root), since the helper's unit may write it only if it is there.
`lamp-control` asks it, and only `lamp-control` may (by its unit, as
`lamp-cloud` and the voice are known):
- `{"install_pushed": SHA256}`: the one push `lamp-control` keeps in its
  `push/` (`push.json`: `sha256`, `client`, `client_name`, `name`,
  `grants`, `at_unix_ms`; beside it `package.tar`, at most 1 MiB). Taken
  only if the SHA-256 is that push's and it was taken within the last
  60 s, never in the future; its bytes read once, never through a link,
  and hashed again. It is checked as the running program will load it,
  beside the skills folder (the rules above), then held to the computer's
  `grants`: a skill that asks one the computer doesn't hold is refused
  ("it asks speak, which this computer was not granted"). A name in the
  skills folder, or another computer's push of that name, is refused; the
  same computer's push replaces its own. The folder gets `.pushed.json`
  (`computer`, `client`, `sha256`, `at_unix_ms`) in the same move as the
  skill. The answer is `{"done": {"name", "version", "sha256", "on",
  "held", "asks", "shown"}}`: `held` is "sees the picture: off until the
  owner says yes in the app" or none, `asks` the protocol's grants it
  asks, and `shown` their words as the pairing screen says them of the
  skill ("moves the lamp's body", "changes the light", "speaks and plays
  sounds", "knows when someone is there, never who", "uses the camera
  when it runs"). A refusal for a grant (a permission or topic past the
  protocol's grants, `hear` and `sound` among them, or a grant the
  computer doesn't hold) also says `"code": "no_grant"`, which
  `lamp-control` gives the computer as its `no_grant`.
- **A shared package** (Ten on #3931; the helper's half, in B): one whose
  `lamp-skill.sig` carries its author's `public` key. `install_pushed`
  reads it again with `read_shared_package_bytes` against the Store's keys
  (`store_keys`: the build's beside the skills folder, else `--key-file`).
  A package changed after signing, naming another key, or carrying a Store
  key is refused ("its signature: …", `bad_package`), and so is any
  install when the Store's key can't be read, never an empty list. Its
  `push.json` must carry `asks`, the computer's shown grants, equal to
  `grants_asked`'s names in their order, or it's refused (`asks_differ`).
  That is a check for honest tools only, and the skill is still held to
  `grants`. `.pushed.json` keeps `author` (the whole key, base64) and
  `author_fingerprint` (FPR16). An update of that name must be signed by
  the same key, and a signed push over a name pushed unsigned is refused
  ("desk-wave is signed by key FPR16 on this lamp: remove it first",
  `signer_changed`). The answer and `list_pushed_by` say `author` (FPR16),
  "signed by key FPR16, not the Store", never "verified". The owner's
  picture yes is keyed by name, computer, SHA-256 and author. A Store
  signature, with no `public`, makes a plain push, as an unsigned one does.
- `{"off_pushed_by": CLIENT}`: a revoke, or the `skills` grant taken back:
  that computer's pushes turned off as the app's off does, kept. The
  revoke is kept first, with its time, in the folder's `.revoked.json`
  under the folder's lock, so an install of a push it made before then is
  refused ("that computer was revoked after the push") even when another
  helper serves the install after the off: the socket starts a helper per
  connection. A revoke is kept two minutes, past any push it could stop.
- `{"grants_pushed_by": {"client": CLIENT, "grants": [...]}}`: the owner
  changed that computer's grants, by a re-pair with fewer or a grant taken
  back. Its pushed skills that ask a grant outside them (or no longer
  load) are turned off, kept, and the change is kept in the same record as
  a revoke, so a push made before it that asks one is refused ("it asks
  light, which this computer was not granted"). A re-pair after a revoke
  never lifts the revoke for a push made before it: every change is kept
  for its two minutes, at most 64. Those dropped, by age or past the
  64, leave a revoke of every computer at the newest of their times, and
  a record that can't be read refuses every install until the next
  change replaces it with a revoke of every computer then: the record
  fails closed, never open.
- **Ordered by the door's revision** (Reviewer-two on #3504): the door
  keeps a `rev` for each computer's pairing, raised on every pair,
  re-pair, grant change and revoke, writes the one it had into a push's
  `push.json`, and sends it with each change, `{"off_pushed_by":
  {"client", "rev"}}` or `{"grants_pushed_by": {"client", "rev",
  "grants"}}`. A change whose `rev` isn't above the highest kept for that
  computer is stale and does nothing, answered `{"done": {"stale": REV}}`
  with the highest; that highest is kept apart from the changes' two
  minutes (at most 256 computers), so a late retry of an old change is
  stale however late. A push is after a change when its `rev` is not
  below the change's; with either missing, by time. A plain id in
  `off_pushed_by`, from a door that sends no `rev` yet, still works.
- `{"list_pushed_by": CLIENT}`: that computer's pushes, each `name`,
  `version`, `sha256`, `on`, `held` and `pushed_at_unix_ms`.
- `{"remove_pushed": {"client", "name"}}`: removed only for the computer
  that pushed it.
- The app's `{"on": NAME}` of a pushed skill is also the owner's yes to it
  seeing the picture, kept in the folder's `.yes.json` for that computer
  and that SHA-256: a re-push of other bytes, or another computer's push
  of the name, has no yes (the Reviewer-lead's P3 on #3879). `{"off":
  NAME}` turns it off as any skill.
- A helper given no developer folder answers each of these "skill push is
  not on this lamp". Tests
(`crates/skills/tests/it/pushed.rs`): `a_pushed_skill_*`.

**The manage helper** (#1722, the first builder's 18:33). The one way a
part that is not root changes the skills folder. `lamp-cloud`'s
`skills.remove` and `skills.install`, for the app's skills page
([cloud.md](cloud.md), "Removing a skill" and "Installing from the Skills
Store"), ask it instead of writing the folder: `lamp-cloud` talks to the
network, and write access there would let a network message put code on
the lamp. So the folder stays root's,
0755, and `lamp-cloud` stays `ProtectSystem=strict` with no writable path
for it.

- **What it is.** systemd holds a socket,
  `/run/lampos-skills-manage/manage.sock` (`deploy/product/lamp-skills-manage.socket`:
  root's, 0660, group `lampos`, `Accept=yes`, at most 4 connections), and for
  each connection starts `lamp-skills asked --dir /var/lib/lampOS/skills
  --key-file /etc/lampOS/skills/store-key.pub` as root
  (`lamp-skills-manage@.service`), the connection its standard input and
  output. It reads one line of at most 4 KiB, answers one line,
  `{"done": WHAT}` or `{"refused": WHY}`, and exits. Its unit has only the
  skills folder and `lamp-skills`' state folder writable (`/etc`, the key
  with it, read-only), one capability, `CAP_DAC_OVERRIDE`
  (`CapabilityBoundingSet=`, for the app's off and on: the state folder is
  `lampos`'s, 0700, and root without it is held to that mode),
  `NoNewPrivileges` and 60 s at most. `off`, `on` and `camera` are written beside
  and moved over, the file beside only ever made new and never through a
  link (`O_CREAT | O_EXCL | O_NOFOLLOW`; a link there is refused, its
  target untouched), and read never through a link (the reviewer's P2 on
  #2878).
- **The Store's switch, in the helper too.** `LAMP_SKILLS_STORE=on`
  (`package::STORE_SWITCH`), off unless set, as `lamp-cloud`'s is.
  `lamp-update install` writes `/etc/lampOS/skills/store.env` with
  `LAMP_SKILLS_STORE=off`; both parts read it with `EnvironmentFile=`.
  A lamp's edited file is kept on another install. Both require the Store's
  separate public key too: missing or invalid, no install is accepted.
  A process that reaches the socket past `lamp-cloud` therefore installs
  nothing while the Store is off: the helper answers `not on this lamp:
  skills.install` (`package::STORE_OFF`) and creates no HTTP client. Removing
  a skill, or turning one off or on, needs neither the switch nor the key.
  Install also writes
  `/etc/systemd/system/lamp-skills-manage@.service.d/store.conf`, which
  reads the shared environment file and prepares `PrivateNetwork=no` and
  `RestrictAddressFamilies=` reset to `AF_UNIX AF_INET AF_INET6` for the
  fetch the helper makes itself (`lamp-cloud` fetches nothing). This network
  permission exists even while the switch is off; the helper's own switch
  and key checks prevent a fetch. systemd cannot change that permission by
  request within one template, so a remove uses the same sandbox.
  Provision the key and turn on the shared switch as [install.md](install.md),
  "The Store, prepared but off", describes; nothing enables it automatically.
- **Only two parts may ask** (the reviewer's P3 on #2429): before reading
  the line, the helper takes its peer's credentials (`SO_PEERCRED`) and
  `/proc/<pid>/cgroup`. Only `lamp-cloud` (the app's remove, a Store
  install) and `lamp-voice` (a made skill's spoken yes, #1685), each at
  the system's place for it, are answered, and `lamp-control` for skills
  pushed from a computer alone (`package::Asker`: it asks nothing else,
  and nothing else asks those, "only lamp-control asks for pushed skills,
  and it asks for nothing else"). Any other part in the group `lampos`
  gets `only the app's link, the voice and lamp-control may change the
  lamp's skills`, as the keys helper does (#2167; `package::from_an_asker`,
  `only_lamp_cloud_and_the_voice_may_ask_the_manage_helper`). The tests that
  drive `lamp-skills asked` over a socket pair of their own set
  `LAMP_SKILLS_ANY_ASKER=on` in the helper's environment, which no unit
  sets, and the helper says so in the journal each time it lets a peer
  through (`a_peer_that_is_not_an_asker_is_refused_and_nothing_is_removed`).
  The non-asker fixture also waits for the real helper to exit before writing:
  a `BrokenPipe` still has to accompany the original refusal, failed status
  and preserved skill. It collects those observations and reaps the child;
  authorized requests still require their write and half-close to succeed.
- **It checks each request itself,** whoever connected. Seven are taken,
  each exactly one key:
  - `{"remove": NAME}`, a string: it runs `remove` above, which refuses a
    name that is not a skill's (1 to 64 lowercase letters, digits and
    hyphens: no `/`, `..`, NUL, hidden name or more) and a core skill,
    under the folder's lock. A skill renamed aside whose
    `.removed-NAME-PID` was not then deleted is answered done (the next
    install deletes it); that folder's path is in the journal only.
  - `{"install": {"url", "sha256", "name", "version"}}`, strings, all but
    `url` optional (a Skills Store catalogue entry's fields): a package
    fetched and installed only signed with the Store's key, and
    only with the Store's switch on ("From the Skills Store", below).
  - `{"install_made": SHA256}`, 64 lowercase hex: a skill made here by
    voice, put on at its person's yes (below). It needs no Store switch:
    it fetches nothing.
  - `{"off": NAME}` and `{"on": NAME}`, a string: the app's off and on
    (#2815, `package::turn`), under the folder's lock, kept in
    `--state-dir`'s `off` (`--state-dir` of `asked`,
    `/var/lib/lampos-skills-state` by default) as a `[stop]`'s off is, so a
    round and a restart keep it, and the running program reads it within
    2 s. Off puts the name in `off`; on takes it out. A core skill the
    build ships off (`enabled = false`) is turned on in its `skill.toml`
    (`lamp_update::product_skills::turned_on`) and its name kept in `on`,
    so a round keeps it on (#2585); turned off again, its file is the
    build's again (`enabled = false`, or install's off line back on top),
    its name leaves `on`, and it is in `off`. A skill granted a core
    operation (`core = [...]` in `[permissions]`: Night sleep, Onboarding,
    Prepare to unplug, Startle) is refused with `NAME is part of how the lamp itself
    works, so it can't be turned off` (`package::cannot_turn_off`, the one
    rule the app's page reads too), and so is one that doesn't load (`NAME
    does not load, so it can't be turned off or on: WHY`) or isn't there.
    It answers `turned NAME off` or `turned NAME on`. Tests:
    `the_apps_off_and_on_are_kept_and_a_skill_of_the_lamp_itself_is_refused`
    (`tests/it/package.rs`) and, through two update rounds and a restart,
    `the_apps_off_and_on_hold_through_a_round_and_a_restart`
    (`crates/update/tests/round.rs`).
  - `{"camera": NAME}` and `{"camera_off": NAME}`, a string: the owner's
    yes to the skill seeing the camera's picture, given or taken back in
    the app past its warning (#3504's piece C, `package::camera`), under
    the folder's lock, kept in `--state-dir`'s `camera`, one name a line,
    beside `off`. The name need not be on the lamp yet: a Store package
    that asks the camera installs only once its yes is kept ("From the
    Skills Store", below). It answers `NAME may see the camera's picture`
    or `NAME may not see the camera's picture`. **Every call reads it:** a
    skill that is not core (`kind = "core"`, which every product skill is)
    whose function hands the model the picture (a `look = true` step,
    `Skill::looks`) runs it only while its name is in `camera`, read at
    each call
    (`Engine::picture_refused`). Without its yes the whole call is refused
    before it is a run: no pose, no light, no look counted (a search's
    `found` after it is still before a look), and no note of a picture.
    The model is told `The owner hasn't allowed this skill's camera; they
    can allow it in the app.` (`CAMERA_NOT_ALLOWED`), and the journal says
    `skill:NAME: refused FUNCTION: the owner hasn't said yes to its camera`.
    A yes taken back holds from the next call, nothing restarted. A program
    with no `--state-dir` keeps no yes, so it gives none. **A remove takes
    the yes back** (`{"remove": NAME}`, the app's `skills.remove`, and
    `lamp-skills remove`, whose `--state-dir` is
    `/var/lib/lampos-skills-state` by default), under the same lock and
    before the folder goes, so a skill put on again by that name asks
    again; a yes that can't be taken back removes nothing. **Only the
    Store keeps a name's yes across an install:** a Store package
    (`install_from_url`, signed with the Store's key) that puts the same
    name in place keeps it, as an update of the same catalogue entry. Any
    other install that puts a skill in place under a name with a yes (by
    hand, `lamp-skills install` of a folder or a package, whose
    `--state-dir` is `/var/lib/lampos-skills-state` by default; or a skill
    made here, `{"install_made": …}`) takes that yes away under the
    folder's lock, before the rename, so the skill there now asks again;
    a record that can't be read or written installs nothing, so a hand
    install by a user who can't read that folder (not root) is refused
    with why; run it as root, or give `--state-dir` a folder of your own
    on a development machine. A skill put
    on by hand or made here is not refused at install, and doesn't see the
    picture without a yes given to it (Desk tutor and Look for my keys,
    installable, among them). Tests:
    `the_owners_camera_yes_is_kept_read_at_each_call_and_taken_back` and
    `a_hand_install_under_a_granted_name_takes_its_camera_yes_away`
    (`tests/it/package.rs`),
    `a_made_skill_never_takes_the_place_of_one_put_on_another_way`
    (`tests/it/made.rs`),
    `a_package_that_asks_the_camera_installs_only_once_the_owner_said_yes`
    (`tests/it/store.rs`, a Store update keeps the yes) and, through the
    running program, `a_skills_picture_waits_for_the_owners_yes_read_at_each_call`
    (`tests/it/skills.rs`).

  Anything else is refused, `a request is one line, {"remove": NAME},
  {"install": {"url": URL, "sha256": HASH, "name": NAME, "version":
  VERSION}}, all but url optional, {"install_made": SHA256}, {"off": NAME},
  {"on": NAME}, {"camera": NAME} or {"camera_off": NAME}`.

  Each waits at most 4 s for the folder's lock (`ASKED_LOCK_WAIT`, under
  `lamp-cloud`'s 5 s wait for a remove's answer; an install waits for it
  only after its fetch, at most 25 s, and its checks, under the 40 s
  `lamp-cloud` waits for an install), then refuses, `the lamp's skills
  can't be changed now`, with nothing done.
- **A skill made here, put on at its person's yes** (#1685; the lead, 20
  Oct 08:37 on #1711: a spoken yes bound to the package's SHA-256, the
  helper installing only those bytes, re-checked and capped). The voice
  asks `{"install_made": SHA256}`, 64 lowercase hex, and nothing else
  (`package::install_made`). The helper finds the package the voice's
  made-skills folder (`/var/lib/lampOS/voice/made-skills`, root's, 0700,
  read by the helper as its owner) names with that hash in its
  `made.json`. Only the made skill the lamp asked about last, within
  120 s (`PUT_ON_WITHIN`) and not yet on, goes on: the voice's own
  `made_skill::waiting`, read by the helper's clock, never the asker's (the
  first builder on #2602, 22 Oct 10:1x). The newest ask is taken first,
  whatever became of it: once it is on, or with the clock before it,
  nothing waits, and an older ask never stands in for it (reviewer-two's P2
  on #2874). A kept one never asked about, one asked about too long ago, or
  an older one after another was asked about, is refused (`the lamp didn't ask about that made skill last, within
  120 s`), so a kept SHA-256 is never put on later by any path. It reads
  its bytes once and hashes them, and a package
  whose hash has changed is refused (`its SHA-256 has changed`). Then it
  reads those same bytes as the package, so nothing is read twice. Its
  `skill.toml` is held again to a made skill's cap
  (`lamp_voice::made_skill::capped`, the voice's own: light, body, and
  only `voice/state` and `room/presence`), and its `SKILL.md` must say
  `made: here`. Then it is installed as any package is, under the same lock
  and wait, with two differences:
  - **It never takes the place of a skill put on another way** (the
    reviewer's P3 on #2414): one of the same name whose `SKILL.md` doesn't
    say `made: here` (Guard mode, Look for my keys, a store one) stays, and
    the answer names it (`… is already on the lamp, put on another way`).
    Only a skill made here before is replaced.
  - Its done line says it is bound to the person's yes by its SHA-256, not
    that it carries no signature.
  Tests: `made::a_made_skill_goes_on_by_its_sha256_and_only_by_it`,
  `…never_takes_the_place_of_one_put_on_another_way`,
  `…whose_package_changed_does_not_go_on`,
  `…past_the_cap_or_not_made_here_does_not_go_on`,
  `…goes_on_only_within_its_ask_and_only_the_newest`,
  `…once_the_newest_is_on_an_older_ask_puts_nothing_on`.
- **Why this and not the others** (the least new surface):
  - **Not the running `lamp-skills`.** It runs as `lampos`, the same user
    as `lamp-cloud`, reading the folder under `ProtectSystem=strict`.
    Removing through it would make the folder writable by `lampos`, which
    is what is ruled out, and give the long-running program that runs
    skills a listener that changes them.
  - **Not `lamp-update serve`.** It is root and long-running, and fetches
    from the network. It has no listener today, and its unit cannot write
    the skills folder; giving it both widens a root daemon.
  - **A root one-shot started per connection** adds no daemon and no
    listener code: systemd owns the socket, its mode and its group, and the
    code that runs is `lamp-skills`' own `remove`, what `lamp-skills remove`
    runs by hand as root. What is new is one line-reading function,
    `lamp_skills::package::asked`, and two units.
- **Who may connect:** root and the group `lampos`, so any part running as
  `lampos` may ask for a remove, never of a core skill. A group of
  `lamp-cloud`'s own would narrow that; it needs `lamp-update install` to
  make the group, and is not done here.
- **On a lamp by `lamp-update install`** (#1685, for a made skill's spoken
  yes): the two units are in its install list (`install::PRODUCT`), the
  socket enabled and the template never, since systemd starts one for each
  connection (`the_skills_manage_socket_is_enabled_and_its_template_is_not`).
  A round never adds a unit (updates.md), so a lamp installed before #2429
  (lamp-0c89 too) has the helper only after `sudo lamp-update install …`
  once; until then a remove and a made skill's yes are refused there.
  Not yet tried on a lamp. Tested on a Mac: `tests/it/package.rs` and
  `tests/it/store.rs` run the real program on a Unix socket and refuse a core
  skill, bad names, a URL that is not https, a Store that speaks no TLS,
  and any install with the Store's switch off (unset, `off`, `ON`, empty;
  nothing fetched, and a remove still runs) on its own; `tests/it/store.rs` sends the helper's own code every bad
  package with nobody before it; `lamp-cloud`'s tests go through the
  helper's code on a real socket.
- **The voice can't remove a skill:** no function does, and whether one
  should is a separate decision.

- **`list`** says, a line each, every skill folder's name, kind, version
  (`-` for none) and what it may use, or why it does not load, as the
  running program loads them (a tool's name taken among them), then the
  functions they give the model. Disabled folders remain listed with a disabled marker; their tools are excluded from that count:

      NAME          KIND         VERSION  MAY USE
      desk-tutor    installable  1.0.0    speak, light, body, look
      welcome-back  core         1.0.0    reads humans/faces/tracked, voice/state; speak, light, body
      The skills here give the model 1 function (check_my_work). A session takes 32, …

- **A session's 32 functions.** `install` and `list` end with the functions
  the skills give the model, because a Live session takes 32 (the second
  route's chat requests 48), the voice's own
  and the actions part's first (30 on a product lamp), and past 32 the last
  skills' are dropped unless the voice runs with `LAMP_USE_SKILL=on` (#1696),
  which gives every skill through one function, `use_skill`
  ([tools.md](tools.md), "Many skills").

- **`pack`** makes a package of an installable skill folder, below; with
  `--key-file`, the Store's private key (in the form `lamp-update sign`
  takes a key), signed for the Skills Store.

Nothing else is restarted. The running `lamp-skills` looks at its folder
every 2 s, loads every skill again when one changed (cooldowns kept), and
writes the skills' functions for the voice again (`tools.json`): an
installed skill's tool reaches the model at the voice's next session, and a
removed skill's dim held `until = "ends"` is given back, as at any reload.
`crates/skills/tests/it/skills.rs` packs Desk tutor, installs it beside Dance
on a running program, sees `check_my_work` come into `tools.json` within
one look, removes it, sees it go, and checks the program was never
restarted and that Dance can't be removed.

**Permissions.** `[permissions]` lists what the skill may read and use; a
skill without the table may do nothing (least privilege):

```toml
[permissions]
topics = ["humans/faces/tracked", "voice/state"]   # the topics it reads
speak = true    # say, sound, or the fixed cue
light = true    # the light steps
body = true     # a face look, gesture, move, pose, or rest/wake/cancel intent
look = true     # in format 1 this is the camera's picture too: ask presence through topics alone
camera = true   # the camera's picture for the model (do.look): its own grant, shown with a strong warning
messages = true # a short text for the person's phone, unasked (a watch's)
known_faces = true  # whether a face at the lamp is known, never whose (a watch's)
who = true      # its note is told by the voice who is at the lamp (Welcome back alone, #2825)
output_limits = true  # private declarative light/volume restrictions
propose = []    # an add-on's alone (kind = "addon"): the acts it may propose
actions = []    # the actions part's functions it may call by name
core = []       # a core skill's alone (kind = "core"): its core-only grants
```

- **Core-only grants.** `core` lists what a skill that comes with the lamp
  may do that no other may: `"onboarding"`, a `say = { from = "onboarding" }`
  on the onboarding invitation trigger, and that trigger's first snapshot;
  the grant also requires checked `[onboarding]` walk data, activated once per
  Voice service invocation ([onboarding.md](onboarding.md#installed-walk-data));
  `"prepare_unplug"`, `body = { prepare_unplug = true }`, its completion
  reads and the reserved `stay_plugged_in` tool; `"sleep"` (#940, Night
  sleep), the night's sleep: its runs ask nobody (no `[ask]`, whatever
  `LAMP_SKILLS_ASK` says), it has only `rest` steps and no tool, lamp-skills
  claims the night for the folder `night-sleep` on `expression/sleep_claim`
  while its rest can be had (a fresh Expression, no repeating refusal),
  and Expression holds that skill's rest as its own sleep
  ([alive.md](alive.md#night-sleep-is-a-skill)). The runtime knows these by
  the declaration, never by the skill's name: a renamed copy without it is
  refused, one with it gets the same, and a skill of kind `installable` or
  `addon` declaring any is not loaded (nor installed): `[permissions] core =
  ["onboarding"] requires kind = core: ...`. Each grant is checked again
  where its call leaves the program, like every other permission.

- **At load.** Every topic the skill reads must be in `topics`: those its
  triggers and `[[while]]` conditions name, `humans/faces/tracked` for a
  look at a face, `voice/heard` for a tool's `[again]` (what a person
  said), and `schedule/due` for a `say = { from = "schedule" }` or
  `tasks/status` for a `say = { from = "tasks" }` (the person's reminder,
  the news of their work: a skill started by anything else could otherwise
  have them said at its own moment). A routine also declares its implicit
  `voice/state` read, even without an explicit condition. An internal
  `clock/local` trigger declares that read but adds no external subscription.
  Each step needs its key: body steps and rest/wake/cancel need `body`, light
  needs `light`, say/sound/cue need `speak`, and a `look = true` step needs `camera` (or format 1's `look`, which still gives the picture; [skill-format.md](skill-format.md), "The camera").
  `[limits]` needs the separate `output_limits` grant, not a general action
  permission. A skill
  that reads or does anything more is not loaded, and the journal says
  what it lacks: `lamp-skills: not loaded: /var/lib/lampOS/skills/x: not
  permitted: a [[while]] reads voice/state, and voice/state is not in
  [permissions] topics`. Declaring more than it reads is allowed; the
  program subscribes only to what a skill's triggers, conditions, look at
  a face and `[again]` read and it lists (a say's words come from the
  voice, so their topic is not subscribed to for it). What the program
  reads to make a step safely (the light shown, the voice's state for a
  step after the say, the body's clips, a hand, the pause) is its own, not
  the skill's.
- **Where each call leaves the program.** A run carries its skill's
  permissions as of the last load, and each call is checked again where it
  is sent: the body's sockets, the ring's `light/request`, the actions
  part, the say service and a tool's look. A call the skill may not make is
  not sent, not counted as made (no clip waited on, no light given back),
  and the journal says so: `lamp-skills: skill:x: refused body.gesture:
  body.gesture needs body = true in [permissions], which it does not give`.
  A say or cue refused is never asked, so nothing that comes with it is made.
  Routine body and accompanying cue grants are checked before any rest or
  wake request leaves the program; a denied hum cannot first wake the lamp.
  Expression receipts and cap snapshots are the service's ownership checks,
  not additional reads made available to a skill. Private limit definitions
  are validated before replacing the accepted batch. Revocation stops new
  caps while allowing cleanup of the service's already-owned exact tokens. The
  actions part is called for the light's own functions (`light_colour`,
  `light_brighter`, `light_dimmer`, `light_off`) only with `light`, and for
  any other only when `actions` names it. No step names an action yet:
  `actions` is the list such a step, or a `run.py`, will be held to. A
  skill that loaded makes only what it declared; this check holds that
  whatever made the call.

**The package.** What the Skills Store will carry: `NAME-VERSION.tar`, a
plain (ustar) tar written the same way every time (no time, no owner, files
0644), so the same skill packs to the same bytes. Its first entry is the
manifest, `lamp-skill.json`:

```json
{"format": 1, "name": "desk-tutor", "version": "1.0.0",
 "files": [{"path": "SKILL.md", "size": 2414, "sha256": "…"}, …]}
```

and then every file of the skill's folder under `NAME/`, its tests among
them, hidden files left out. `pack` makes one only of a skill that loads,
says its version and is installable (a core skill comes with the lamp), and
reads it back as an install would. An install of a package checks, before
anything is written: a tar of plain files and folders (a link, a device or
anything else refuses it), one skill's folder and nothing beside it but the
manifest and the signature's place; the manifest's `format` 1, its `name` a
skill's name and the folder's, its `version` `MAJOR.MINOR.PATCH` and the
one `skill.toml` says; every file in the manifest with its size and
SHA-256, and no file that is not; each path at most 4 parts of letters,
digits, `-`, `_` and `.`, none beginning with a dot (no `..`, no
`._NAME`); at most 512 files and 16 MiB. Then the skill is loaded and moved
in as from a folder.

**What an installed package keeps** (#2801). A skill put on from a package
(by hand, from the Skills Store or made by voice) keeps, in its own folder,
`.package.json`: the SHA-256 of the package's bytes, the one the Store's
catalogue names, and its `lamp-skill.json` as installed
(`package::INSTALLED_FROM`, read by `package::installed_from`). It is
written into the folder before the folder is moved in, so it comes and goes
with the skill: the next install replaces it whole, and a remove deletes
it. Hidden, so the loader, `pack` and an install from a folder pass over it,
and a package can't carry one of its own (no path in a package begins with
a dot). A core skill, one put on from a folder, and one installed before
this keep none, and one whose `skill.toml` version no longer matches the
kept manifest's (changed by hand) is no longer that package's: each lists
no `sha256` ([cloud.md](cloud.md), "The skills page"). The record says which
package was installed; it doesn't hash the folder again, so a hand edit to
`SKILL.md` or a file that keeps the version still lists that package's
`sha256`.

**The signature.** `lamp-skill.sig`, beside the manifest, is one line of
JSON, `{"key": FINGERPRINT, "signature": BASE64}`: Ed25519 over the
manifest's exact bytes, which name every file's size and SHA-256, so it
covers every byte of the skill. `key` is the fingerprint of the key that
signed it as `lamp-update` gives one (the first 16 hex of its SHA-256), so
a package signed with another key is told apart from a changed one.
`lamp-skills pack --key-file PRIVATE` writes it. An install by hand
(`lamp-skills install PACKAGE.tar`, root at the lamp) is not held to it
and says so (`its signature not checked: a package installed by hand from
a file`, or `it carries no signature: …`); a package from the Skills Store
is installed only with it.

**A shared package** (decision 4, "sharing from day one"; Ten on #3504,
28 Oct). An author shares a skill with no Store: as any file or URL.
- **Signing:** `lamp-skills pack --key-file PRIVATE --author` signs it with
  the author's own Ed25519 key. Its `lamp-skill.sig` carries the key too,
  `{"key": FINGERPRINT, "public": BASE64, "signature": BASE64}`, since a lamp
  knows the Store's key but never an author's. A Store package is as before,
  with no `public`.
- **Integrity, not trust** (Fourteen's point 1 on #3504): a signature says
  the bytes are that key's, not who is behind the key, since anyone can make
  one. Consent stays with the computer's grants and the owner's yes for the
  picture. Words for it say "signed by key FPR16, not the Store", never
  "verified" or "trusted".
- **Reading** (`package::read_shared_package_bytes`, `package::check_author`):
  - `public` must be 32 bytes, and `key` its fingerprint;
  - the signature must be that key's over the manifest's exact bytes;
  - then every file is checked against the manifest, as for any package.
- **Refused, nothing written:**
  - a package with no signature;
  - one signed as the Store signs (no `public`);
  - one whose signature names another key than it carries;
  - one whose manifest or any file changed after it was signed;
  - one whose key is one of the Store's (`store_keys`): anyone can add the
    Store's public key to a Store package's signature, which then checks,
    but the package is the Store's, not an author's (Fourteen's P3 on
    #3931). The caller passes the Store's keys, as `store_keys` reads them.
- **Planned, not yet built:**
  - a shared install is a `skill.push` of a signed package, with the
    `asks` the computer showed (required for a signed one), so its consent
    is the push path's (`control.md`, "Skills pushed from a computer"):
    the paired computer's `skills` grant, the owner's yes to the
    permissions and the author key shown before install (`lamp skill
    install`), and the picture's own yes;
  - the push path keeps a skill's name with the key that installed it
    (`.pushed.json`'s `author`): all 32 bytes, as `Package.author` gives
    them, since the 64-bit fingerprint is only for showing. An update must be signed by the same key;
    another author's package of that name, or an unsigned one, is refused
    until the skill is removed. A signer never changes in place.
  - They land with the helper's, the door's and the SDK's halves.

**From the Skills Store** (#1722, the first builder's 21:51, 18:33 and
22:23). The app sends `skills.install` with a catalogue entry,
`{"name", "version", "sha256", "url"}` ([cloud.md](cloud.md), "Installing
from the Skills Store"); `lamp-cloud` passes it to the manage helper,
which runs `lamp_skills::package::install_from_url`. In order, with
nothing written before the last step:

1. **The request.** `url` is `https://`, a host, at most 2048 bytes of
   printable ASCII with no space and no user; `sha256` 64 lowercase hex;
   `name` a skill's name; `version` `MAJOR.MINOR.PATCH`. `http://`,
   `file://` and anything else is refused unfetched.
2. **The key** (`package::store_keys`, read again for every install, so
   no restart follows a new key). The build's key wins:
   `/var/lib/lampOS/skills/store-key.pub` (`package::BUILD_STORE_KEY` in
   the unit's `--dir`, #3692), which a build ships as a setting (root's,
   0644; [updates.md](updates.md), "Settings and tmpfiles"). It holds one
   or two keys, one a line, each 32 bytes of Ed25519 in canonical base64
   (`^[A-Za-z0-9+/]{43}=$`, `manifest::read_public_keys`): two while the
   Store rotates its key, and the signature's `key` fingerprint picks the
   line; a third key is refused. Without that file, the factory's key,
   `/etc/lampOS/skills/store-key.pub` (`package::STORE_KEY`, the unit's
   `--key-file`), one key in the same format. Factory step 3 places it
   with `--store-key FILE`; neither setup nor install invents or copies a
   key. A build's file that is there but can't be read or isn't one or two
   keys refuses every install, said with its name, and the factory's key
   is **not** taken in its place: the build's file means the Store moved
   off the factory's key, which may be the very one it rotated away from.
   A remove, the app's off and on, and the skills already installed go on.
   The round writes a build's file again only while it is still the
   build's, so a damaged one is mended by hand. Missing or invalid,
   nothing is fetched. There is no fallback to the update channel's
   `/etc/lampOS/update/key.pub`. A manual `--key-file` remains an explicit
   override of the factory key's path. The file is no skill: the loader
   loads folders only, a skill's name has no dot, and the helper's remove,
   install and sweep never touch a plain file.
3. **The fetch.** By `lamp-update`'s own HTTPS client (rustls, the web's
   roots): one GET, at most 5 redirects, each only to https (a redirect to
   `http://` or `file://` is refused), into memory, at most 18 MiB (a
   larger `Content-Length` is refused unread, a body without one is cut at
   the bound), within 25 s (10 s to connect, 10 s for any read to make
   progress).
4. **The hash.** With `sha256`, the bytes must have it: checked before the
   tar is read.
5. **The signature,** once the tar is read in memory (its entries, with the
   bounds below) and before the manifest is parsed or a file compared with
   it: `lamp-skill.sig` must be there, name the Store's key, and be its
   signature over `lamp-skill.json`'s exact bytes. Unsigned, signed with
   another key, a signature that is not one, or a manifest changed by a
   byte: refused.
6. **The package,** as any install checks it (above): plain files only (a
   link, hard or symbolic, a device or a fifo refuses it), one folder, no
   `..`, no absolute path, no hidden name, at most 4 parts a path, 512
   files and 16 MiB, every file's size and SHA-256 as the manifest says.
   With `name` and `version`, the manifest must say the same.
7. **The install,** under the folder's lock, as `install` does (waited
   for at most 4 s, `ASKED_LOCK_WAIT`, then `the lamp's skills can't be
   changed now` with nothing written): put
   together in `.install-NAME-PID`, where the running program does not
   look, loaded with every check a load makes, refused if it says `kind =
   "core"`, if a core skill on the lamp has its name, or another skill its
   function, if it is **older than the installed one** (below), or if it
   asks the camera's picture (`camera`, or format 1's `look`) and the
   owner's yes to it is not kept in `--state-dir`'s `camera` (`{"camera":
   NAME}` above): `not installed: NAME: it asks the camera, and the owner
   hasn't said yes`, with nothing written
   (`a_package_that_asks_the_camera_installs_only_once_the_owner_said_yes`,
   `tests/it/store.rs`); then
   renamed into place (an installed one of the same name is replaced
   whole, the old renamed aside first). The running `lamp-skills` loads it
   within 2 s.

**Never an older version.** A Store package replaces an installed skill
only at the same version (a reinstall) or a newer one, so an old package
the channel once signed can't be replayed onto a lamp. Versions are
`MAJOR.MINOR.PATCH` (the loader and the manifest take nothing else, and
every skill in `lampOS/skills/` says `1.0.0`), compared as three numbers,
part by part: `1.10.0` is newer than `1.9.0`. An older one is refused under
the folder's lock with the folder as it was: `not installed: desk-tutor
1.10.0 is on this lamp, and this package is the older 1.2.3: a skill is
never put back to an older version`. An installed one whose version can't
be ordered (none, or not `MAJOR.MINOR.PATCH`, a folder changed by hand) is
refused too, since the package might be older: `… its version can't be
compared with 2.0.0: remove it first, then install this one`. Refusing is
the safe side; allowing would let any signed package over it. An install
by hand from a file (root at the lamp) is not held to this, as it is not
held to the signature, so root can still put an older one back.

The answer is `install`'s line (`installed desk-tutor 1.0.0 (installable)
from its package, N files, …; signed with the Store's key
FINGERPRINT. It may use: …`), and `lamp-cloud` replies with `skills.list`
read after it. The helper journals each request and its answer.

**What the Store's catalogue is expected to be** (for the backend, which
posts its endpoint layout on #1722): a list of entries, each `name`,
`version`, `sha256` (of the package's bytes) and `url` (https, a plain
GET, redirects to https allowed); the list signed with the same key as
the packages, Ed25519 over its exact bytes, as `lamp-update`'s envelope
signs a build's manifest. `skills.catalog`, which fetches and checks the
list, is not built: an entry from it goes to `skills.install` as it is.

Not built yet: the Store's server (a catalogue, review, publishing) and
`skills.catalog`; the app's "My skills" page (`list` is text for a
person; the lamp's `skills.off` and `skills.on` for it are built, #2815).
`PRODUCT_SKILLS` are exactly the core skills, Laugh glance parked
(`the_product_skills_are_exactly_the_core_skills` in
`crates/update/tests/install.rs`).
A skill's functions past a session's 32 are #1668's (`LAMP_USE_SKILL=on`,
#1696): `install` and `list` say the count, and nothing here changes it.

### Reflexes stay in the body and Expression

A tap on the head, a startle at a bang, giving way to a hand must answer in
under 150 ms. They stay built into the parts that own the body and the
light. A skill may add a clip for a reflex as data: the startle's move is
the Startle skill's `[startle]`, which Expression reads and plays itself
(#2822). Behaviours that unfold
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

#### Made by voice (#1685, `LAMP_MAKE_SKILL=on`, on in the product)

"Lamp, make me a skill that dims the light at 10 pm." With the switch, the
model is given `make_skill(request)` (`crates/voice/src/made_skill.rs`). What
happens:

1. **Handed over as any work.** The voice prepares a hand-over with the
   tasks part ([tasks.md](tasks.md#a-skill-made-by-voice-1685-off-unless-lamp_make_skillon)).
   Its text is the brief, then the person's request:
   - The brief is fixed words: the skill format and what a made skill may
     use. So the computer's agent needs no skill creator of its own.
   - A time of day is asked for as the trigger's own window,
     `during = { from = "22:00", to = "22:30" }`, not as a `[quiet]` window
     around it. The brief once said the latter, and #2813's rerun on
     lamp-0c89 got a 23.5 h `[quiet]` to fire around 22:00.
   - It says `skill.toml` has no `name` key: the name is SKILL.md's, and
     `name =` in `skill.toml` fails the check as an unknown field. That was
     what failed #2813's rerun on lamp-0c89.
   - The request is at most 180 bytes, refused if longer, never cut.
   - Nothing else of the conversation goes with it.
   - The model says back only the request, never the brief, and hands it
     over (`confirm_agent_task`) only on the person's yes. The agent is
     whichever the hand-over uses.
2. **Back as two files.** When the work comes back done, its answer must be
   exactly two fenced blocks, `skill.toml` and `SKILL.md`, at most 8 KiB
   each. The files come back on the hand-over's own channel (Harness's
   answer); there is no other transport.
3. **Held to what a made skill may use** (the coordinator's cap, 19 Oct):
   - `kind` installable, with a version;
   - `[permissions]` with only `light`, `body`, and `topics` among
     `voice/state` and `room/presence`;
   - no function for the model (`tool`);
   - `[ask] policy` only `nudge`, never a hello, news or a reminder;
   - speaking, messages, the look and any other topic are refused.
   Widening this is the lead's call.
4. **Checked.** The card is marked `made: here` (`lamp-cloud`'s "Made on
   this lamp" group). Then `lamp-skills pack` (#1698) checks the folder:
   every check a load makes, its permissions among them, and the package
   read back as an install reads one. A skill that does more than it
   declares is refused there. When it fails, the voice's journal has the
   check's exit code and its first line, without the skill's own words:
   anything in quotes, the skill's name and its folder are left out, and
   the line is at most 200 characters (`made_skill::check_reason`; #2813's
   rerun on lamp-0c89 gave only `exit 1`). The check writes to a file in
   the private folder, read up to 8 KiB, so a long answer never holds it.
   - **Repaired once** (#2813 piece 2, the first builder on #3126, 23 Oct
     13:07). When the check refuses a first answer, the work is handed
     over once more to the same target, before anything is said.
     - The brief is the same again, with the check's lines before the
       request: every line, joined with " / ", the skill's folder replaced
       by `<folder>`, cut to 600 bytes (`made_skill::repair_brief`). They
       are the agent's own skill read back to it, never the conversation,
       and with the longest request the brief stays within the tasks
       part's 2,000 bytes. Its new session has none of the first, so it
       carries the whole format again.
     - Only while an app's ask it came from, if any, is still being made;
       the ask follows the repair (`app_make::repaired`).
     - The first session closes as any. The repair's answer is checked
       once: kept and told, or told not kept. A repair is never repaired
       (`made_skill::is_repair`), and each answer is repaired at most once,
       across restarts too (`repairs.json` beside `made.json`). The repair
       is claimed there under a lock before it is handed over
       (`made_skill::claim_repair`), so of the two readers of the same
       answers, the coordinator's and the texts worker's, only one sends it
       (the reviewer-lead's P2 on #3662). A claim left with no outcome (the
       voice stopped between it and the hand-over's end) is taken as not
       sent after 2 minutes (`CLAIM_STALE_MS`), so the answer is told not
       kept once, never sent twice. A repair sent keeps its task there, and
       a later read moves an app's ask onto it, should the voice have
       stopped before it did. Claims are kept 24 h, longer than an answer
       is read for.
     - Only a refusal by the check is repaired. A wrong format or a skill
       asking for more than the light and the head is told not kept at
       once, as is a repair whose hand-over fails.
     - Tests: `made_skill::tests::a_repairs_brief_carries_the_checks_lines_and_still_fits`,
       `a_failed_check_gives_its_lines_with_the_folder_replaced`, and
       `task_news::tests::a_made_skill_the_check_refuses_is_repaired_once`
       (fixed on the retry is kept, not fixed is told not kept once, never
       a second retry).
5. **Kept, not installed.** The package goes in the voice's own folder,
   `/var/lib/lampOS/voice/made-skills` (0700, the voice's user):
   - at most 5 are kept, and the oldest is let go with its package;
   - each is kept with its SHA-256 and what it asks to use (`made.json`);
   - the same hand-over read again gives back the kept one, not checked
     again.
6. **Told, and asked.** The lamp says what it would do (the card's
   description) and what it would use ("its light and whether it is
   talking"), then asks, word for word: "Shall I put it on the lamp?"
   - The files are never read out.
   - A skill not kept is told plainly why ("it asks to use more than the
     light and the head, which a made skill can't").
   - The ask is kept beside the package (`offered_at_unix_ms` in
     `made.json`, `made_skill::asked`) when the news is heard whole at the
     lamp, or given in a text, never when it is read: the lamp tells it
     later, at a good moment. It is kept too when the news played through
     with a gap: every word played, nobody's turn over it
     (`news_played_through`, `TaskNews::played_through`). That news isn't
     heard, so its harness stays open, but the question was said, so a yes
     to it counts (fourteen-P10 on #2814: on lamp-9adf a yes was refused
     after the question played whole with a gap). A turn cut short, or one
     with no audio, still asks nothing. Once a yes has put the skill on,
     its news isn't told again (`made_skill::is_on`): nothing is left to
     ask. The yes that put it on keeps that news as told and tells the
     tasks part so (`task_news::answered`), so with `--close-told` its
     harness on the person's computer is closed. The news is told in the voice's own process,
     and the yes comes to the worker that runs the call.
7. **Put on at a spoken yes** (the lead, 20 Oct 08:37 on #1711: a spoken
   yes bound to the SHA-256, an exception to the 18:33 "only signed
   packages" ruling for a skill made here alone; the app doesn't confirm
   too). On the person's yes the model calls `put_on_made_skill`
   (`made_skill::put_on`):
   - only a yes said out loud counts (`heard`): a typed one, or one the lamp
     wrote, puts nothing on;
   - only a plain yes (the words heard in the line, as the held forget's,
     `known_faces::plain_agreement`): "No, not yet" the model took for a
     yes puts nothing on;
   - only the newest ask, within 120 s of its being heard (`PUT_ON_WITHIN`,
     as a hand-over's yes);
   - the voice sends the manage helper that skill's SHA-256 alone
     (`{"install_made": SHA256}`, "The manage helper" above), which
     installs only those bytes, held again to this cap;
   - on the second route, while one waits, each request is told so
     (`made_skill::waiting_note`), since the route keeps words, not calls;
   - the person hears that it's on, that it couldn't be put on (also with
     no helper to ask), or, with no answer from a helper asked within 8 s,
     that it is being put on;
   - never without `LAMP_MAKE_SKILL`, whatever the model calls.
   Tests: `made_skill::tests` (the ask's window, the spoken and plain
   yes, no helper, the helper's refusal and silence; `task_news::tests`: no
   ask until the news is heard), and
   `made::a_made_skill_is_put_on_by_a_spoken_yes_through_the_helper` (the
   voice's call through the real helper).

**Open, for the lead:**
- Harness sign-in on home lamps.

Removing a made skill by voice is still to come.

Tests:
- `made_skill::tests`:
  - the brief within 1,000 bytes, half the hand-over's 2,000;
  - exactly two files;
  - the cap, each wider use refused;
  - the card marked;
  - the note's words;
  - nothing kept without the checker.
- `agent_tasks::tests::a_skill_to_make_is_handed_over_as_its_brief_and_said_back_as_its_request`.
- `task_news::tests::a_skill_made_by_voice_is_told_as_kept_and_never_read_out`.
- `crates/skills/tests/it/made.rs`, with the real `lamp-skills`: checked, packed and kept 0700 by its SHA-256; a step it doesn't declare refused; at most five kept.

#### Asked for in the app (#2813, `LAMP_APP_MAKE_SKILL=on`, on in the product)

The same maker, from words typed in the app; put on only as above, by a
spoken yes at the lamp (the lead's ruling on #1711; the first builder on
#2602, 22 Oct 10:3x). It needs `LAMP_MAKE_SKILL=on` and the tasks part too
(`crate::app_make`).

1. `lamp-voice` listens at `LAMP_APP_MAKE_SOCKET`
   (`/run/lampos-app-make/make.sock` unset), opened to
   `LAMP_APP_MAKE_GROUP`, `lampos` unset, for `lamp-cloud`: one JSON line
   in, one out. A group not known starts nothing (`app_make::group_named`).
2. `{"make": {"words", "request_id"}}`: the words become the same brief
   (`made_skill::brief`, at most 180 bytes) and are handed over at once on
   a thread of their own (`agent_tasks::hand_over`: prepared and confirmed,
   the voice's own two steps). Typing them in the app is the person's ask,
   so there is no "shall I start it?". With `LAMP_TASKS_TARGET` unset, a
   default target the tasks part hasn't is replaced by its one Claude
   target, as the voice's own hand-over does (#1390); none or several is
   `not_kept`, "the computer's agent isn't set up for the lamp yet", never
   guessed, and a target set is kept as set. A confirm whose answer is lost,
   or answered `uncertain` on any try (even if a later try was `busy` or
   `timeout`), is kept as handed, for the task's news to settle; only a
   definite refusal is `not_kept`. The answer, before the
   hand-over:
   `{"making": true}`, or `{"refused": WHY}` in plain words (no request id,
   or one not 1 to 64 letters, digits, `-`, `_`, `.` or `:`; the words too
   long; an id sent before, of the newest 100; or one still being made: one
   at a time, and one "making" for 30 minutes is let go).
3. `{"ready": {}}` (fourteen-P15): `{"ready": true}` when the tasks part
   answers `targets` with at least one target within 1 s
   (`app_make::READY_WAIT`, `agent_tasks::has_a_target`), else
   `{"ready": false, "why": "the computer's agent isn't set up for the lamp
   yet"}`; unreached is no. It reads nothing of the asks. `lamp-cloud` asks
   it every 60 s and names `skills.make` in `can` only while it is ready,
   so the app's "Make a skill" shows only on a lamp whose Harness has a
   computer.
4. **Told aloud.** The hand-over is never kept as typed
   (`task_news::keep_typed`), so its news ("… Shall I put it on the
   lamp?") is told by the spoken task news at a good moment, with someone
   there, and the ask is dated only when heard whole or played through with
   a gap (`made_skill::asked`, step 6 of "Made by voice" above).
   A spoken yes within 120 s puts it on (`put_on_made_skill`, and the
   manage helper's own check, #2874); a no, a typed yes or silence leaves
   it off. What the tasks part settles is settled here too, by the task's
   own store and id, on every read, whether or not its news was told (so a
   task that failed before its ask learned it is settled too): work it
   confirmed but never sent (`not_sent`), and an agent that failed or
   stopped (by the task's own states), are `not_kept` with why; work it may have
   sent (`uncertain`) stays `making`, so another may start after 30
   minutes.
5. `{"made": {"request_id"}}` (or `{}` for the newest): the app's own ask
   only, never a skill made by voice. `state` is `making`, `kept` (not yet
   asked about aloud), `asked` (the offer a spoken yes would put on now:
   the newest, within its 120 s, `made_skill::waiting`, #2874), `on`,
   `left_off` (asked, and no longer that offer) or `not_kept` with `why` (the maker's plain words,
   or why it wasn't handed over; never the agent's text). Once kept:
   `name`, `description`, `version`, `uses` (`light`, `body`, `topics`) and
   `sha256`, what the lamp asks about. `{"state": "none"}` with no ask.
6. Kept beside the made skills, root's, 0700: `app.json`, the newest five
   asks, each its request id, when, its task and why it wasn't kept. Never
   the words. Every writer (the socket's admission, the hand-over's thread,
   the task news' reader, the texts' reader in the worker) holds one lock
   (`.app.lock`, `flock`) across its read, decision and write, and writes
   through a file of its own, so no ask is lost.

Tests: `app_make::tests` (the states, a superseded offer left off, not kept
and why, one at a time and bounded, the socket's lines, a make through the
real socket to a tasks part not there, overlapping writers losing nothing,
and the socket's group),
`task_news::tests::an_apps_ask_is_settled_when_its_work_never_reached_or_failed`,
and
`agent_tasks::tests::a_hand_over_prepares_and_confirms_at_once`. The app's
side is `skills.make` and `skills.made` in `lamp-cloud` ([cloud.md](cloud.md),
"A skill typed in the app").

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
(`crates/voice/src/ring_face.rs`, `accent_scale`), and over a speaking
face a skill's light tints, with `LAMP_SKILL_LIGHT_SPEAKING=on`. The accent is in the lamp's
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
| Hello when someone comes back to the desk | The Welcome back skill on a product lamp and on lamp-4ace; the voice's own (`speak_up.rs`, asking the Policy, #883) was removed (#2825) | Skill: Welcome back |
| A reminder or timer coming due | The Reminders skill on a product lamp (`LAMP_SCHEDULE_SPEAK=skill`), asking while the voice rests, so one due during a talk is said when it ends, with the look: the voice says it in the skill's words (`[reminder.words]`, #940), and itself when no ask comes within 10 s. The voice's reminder consumer, timing it too, on lamp-4ace (`on`) | Skill: Reminders (setting one stays a voice tool) |
| News of a task handed to Claude Code or Codex, and progress news | The Task news skill on a product lamp for done and failed (`LAMP_TASK_NEWS=skill`): the voice tells it in the skill's words (`[news.words]`, #940), and itself when no ask comes within 10 s of having it (at once, without the sway, for news that settled while nobody was there: the skill's ask is gone 20 s after the work settles, and an item's run cannot wait), and for stopped and asking news too (#1722: a look, and a curious tilt for a question). The voice (`task_news.rs`) on lamp-4ace; progress news is the Policy's act, off in the product | Skill: Task news |
| Sleep at night, waking in the morning | [Bedtime and Morning](bedtime-morning.md), disabled until measured | Skill selects local windows and fresh absence/arrival; Expression keeps the owned renderer and reflexes |
| Turning to a voice, looking away to think, back before its first word, nods while speaking | Voice, body, Expression | Built in: the conversation, under a second |
| A tap on the head, a hand held on it, three seconds to sleep | Controls, Expression, voice (#711, #849) | Built in: a reflex, under 150 ms |
| One startled glance at a bang | The Startle skill's move (`skills/startle/skill.toml`, `[startle]`, #2822), played by Expression as its reflex; no catalogue carries it | Skill: Startle (core, Expression's reflex) |
| Giving way to a hand, rising when spoken to, folding when left alone, idle shifts | Body | Built in |
| Eye contact (the head follows a face) | Eyes and body | Built in |
| Taking a look when asked what it sees | A voice tool (#836) | Stays a tool; skills may call it |
| Light, volume, timers, memory, faces, hand-overs | Voice tools and services | Stay tools and services; skills call them |
| The privacy switch and the self-test | Voice, eyes, self-test | System, never a skill |
| The light going out when nobody is there, back when someone sits down (#1677) | Nowhere before 19 October (V1's presence service did it from motion) | Skill: Light out when you leave, reading the eyes' `room/presence` |

Built-in gestures (the nod on "yes") use the same move and expression
library as skills.

## The first skills

[Bedtime and Morning](bedtime-morning.md) (#1672) is an installed core skill
that stays `enabled = false` until its lamp comparison. With exact
`LAMP_SKILL_ROUTINES=on`, Skills can load routines with `[limits]` or a
`rest` step. Without it they have no active triggers or tools. A disabled
skill is excluded from `tools.json`, including the `use_skill` catalogue.

Generic additions: per-trigger `during = { from, to }` local windows,
named `once_per_day = true` with `LAMP_SKILLS_DAILY_FILE` (or `--daily-state PATH`), and explicitly
observed `present = false` on the tracked face topic. A missing camera
does not count as absence. Daily chances commit before Policy or output;
refusal and restart do not refund them. Routines require a fresh resting
Voice heartbeat (under three seconds); a gap resets the quiet duration.

Enabled schedules wait for a configured, trusted local clock, with one
journal diagnostic per missing-trust interval. Ordinary and disabled skills
remain silent. Losing trust refuses new or pending Rest/Wake transitions;
an already-admitted Rest can continue only to its last trusted window's
monotonic deadline, with fresh matching Expression evidence for each bounded
lease. Clock correction cannot revive an expired episode. A window that
closes one minute before it opens (`23:00` to `22:59`, the longest) carries
a Rest still held in its last minute through that minute to the next
occurrence's end, the same episode with no cancel, its 24 h episode cap
counted again from there (`Window::rest_remaining_ms`, night sleep, #940). See
[Bedtime and Morning](bedtime-morning.md#local-windows-and-daily-chances)
for clock trust and continuation limits.

These local windows and daily claims can also constrain a `rise` trigger.
A delayed Policy answer must still find the rise and window holding; an
accepted reload resets the rise history and invalidates the pending scheduled
run's generation. None of those events refunds a consumed daily chance.

`rest = { op = "rest", during = { from, to } }` starts a leased intent;
`op = "wake"` and `op = "cancel"` take no window. Unknown operations are
refused. A rest's `resettle` lets the rest manager settle again after a
sound ended it, as described in [Bedtime and Morning](bedtime-morning.md#settling-again-after-a-night-sound).
A trigger whose chance matured while its conditions did not hold, past its
`wait`, says once `<trigger> chance dropped: ...` in the journal. The shared `skills` publisher identifies the skill in its payload.
A once-a-day trigger's line is only that. Any other `appears` or `rearm` trigger
with a `wait` adds the `[[while]]` that held it, its topic and field and the last
value it said: `... (held by voice/state state was resting for 1 s of its 2 s)`,
`(held by <topic> <field> was <value>)`, `(... and that ran out)` for a value
past its lifespan, or `(<topic> <field>: nothing said yet)`. Only a number, a
yes or no, or a one-word value is shown; any other text is `a text`, a list
`a list of N`, faces `N faces`, so no person's words reach the journal (#2861:
on lamp-06c1 Welcome back's arrivals after a say that expired were let go with
no line). Test: `crates/skills/tests/it/welcome_stuck.rs`.
A once-a-day trigger whose watch is reset after it had begun holding says once
`<trigger> watch reset after N s: <why>`, where why is `stopped holding: <topic> <field> no longer <value>`
(or `<field> no longer rising` for a rise), `went silent: nothing held it on <topic> for N s, past its N s`,
`left its window HH:MM–HH:MM` or `no trusted local moment`. A watch that never held says nothing.
A due chance any other gate drops or holds back, for every trigger kind,
says one journal line naming the gate (#2602; on lamp-52e6 two Welcome
back runs were lost with nothing said): `lamp-skills: <skill> not run:
<why>`, where why is `away N s, under its N s` (`absent_for`, from the last
face the trigger held for its `for`), `in its cooldown, N min left`, `quiet hours`,
`outside its active hours`, `off`, `snoozed, N min left`, `its N a day
already run`, `resting after runs nobody noticed, N min left`, `its <group>
group's hour is spent` or `another <group> skill waits first` (`[budget]`),
`not rearmed: <field> below N for N s first`, `its while did not hold:
<topic> <field> <value> for N s` (the first `[[while]]` condition that did
not hold, for a trigger with no `wait`; a chance let go past its `wait` is
said by the `chance dropped` line above, which #3007 gives every trigger
with a `wait`, and not here, so it is said once), `an item came, but its
while did not hold: ...`, `spoken to since
HH:MM` (`unless_heard`), `its day already claimed` or `its daily journal is
faulted`. A chance kept (quiet or active hours, the budget, a `wait`) is
said once when first held back. Each gate (each `[budget]` reason its
own) is said once per hold of its
trigger and not again for that trigger within 60 s (`DROPPED_LINE_US`), so a
face that flickers past its `gap` says it once; a run clears that. These
`not run` lines are only in the journal, not on `skills/events` (the
`chance dropped` line is on both), and carry no words heard or names. A chance a `yields_to` holds says `waits: ...` as above. Tests:
`a_dropped_chance_says_its_gate_once` and
`resting_a_budget_queue_and_a_while_without_a_wait_say_why_not_run`
(`tests/it/skills.rs`),
`welcome_back_says_each_dropped_chance_once_with_its_gate` and
`a_claimed_day_and_a_spoken_window_say_why_not_run` (`tests/it/daily_engine.rs`).
Rest admission is not motion confirmation. A wake waits for correlated
planner completion, at most twelve seconds, before its following sound.
The fixed `cue = "hum"` uses Say's existing privacy, quiet and rarity checks;
it adds no file asset or model call. The ordinary Policy gate still applies
when configured: its late-night/absent-person fallback can refuse a routine.
A plain say, exact/ranked say table, sound or cue occupies the same single
speech slot in a run and needs `speak` permission. No priority bypasses the
routine's permission, completion, generation or original-deadline checks.

Output `[limits]` run independently of a presentation's permission, through
private Actions operations. The accepted generation contains at most eight
cap-bearing skills; an over-capacity reload is refused as a whole. Light
and volume ceilings compose by minimum, and release never restores an old
person setting. These interfaces and their default-off switches are detailed
in [Bedtime and Morning](bedtime-morning.md).

| Skill | Trigger | What it does | Replaces |
|---|---|---|---|
| Welcome back | A face arrives after 5 minutes with nobody | Policy; turns to them, the light warms, a short hello, by name if known | The voice's hello (#883, `speak_up.rs`), removed (#2825) |
| Reminders | First eligible positive `schedule/due`, a change in its store/count/oldest occurrence, or eligibility returning after Voice talks or Schedule becomes unavailable; unchanged heartbeats do not repeat a chance | Looks at the person, says it: `say = { from = "schedule" }`, in its own `[reminder.words]` (#940), the voice keeping the reminder and the acknowledgement (#939, (c); `skills/reminders`). The ring's pulse waits for a light step that can pulse | The reminder consumer's timing in the voice (`LAMP_SCHEDULE_SPEAK=skill`, on a product lamp since 19 October); its delivery stays there |
| Task news (#1021) | `tasks/status`: a task comes into `settled` (#1048) as `idle` (done), `failed`, `stopped` or `needs_input` (asking, #1722), each task on its own; a question followed by the end (#2543) turns the entry to `idle`, so each is told once | Policy (`news`); done: looks at the person if a face is in view, `happy`, and the voice's own news of it (`say = { from = "tasks" }`); failed: `caring`, its own light a step dimmer until the droop and the words end, and the news, asking whether to try again; stopped: the look and the news; asking: the look, `curious` and the news. No red ring | The timing of task news in the voice (`task_news.rs`): on a product lamp since 19 October, installed with `LAMP_TASK_NEWS=skill`, so the voice waits up to 10 s for its ask and tells each task once, in the skill's own `[news.words]` (#940); the voice's own timing is removed once it has done 5 of 5 on lamp-4ace. Written by jarvis; the per-item trigger (`each`), steps by trigger (`on`) and the waiting of item runs were added to `lamp-skills` for it. Its cases are replays; it has not run on a lamp |
| Stretch break | Someone at the desk for 50 minutes | Stretches itself as an invitation, never a lecture (creature.md); one an hour with the wellbeing skills (`[budget] group = "wellbeing"`, #1685), when any is on the lamp | New |
| Stuffy air (#1021) | `air/state`'s `co2_ppm` above 1,200 for 10 minutes | Droops once (`sleepy`) and dims a shade until the air is under 1,200 again; once a spell, again only after 10 minutes under 1,000; never from 22:00 to 08:00 (a spell that holds through the night droops once at 08:00), nor while someone talks to it; the dim also ends 15 minutes after the sensor falls silent; never speaks of it (the voice's air facts do, when spoken to) | New: reworked by jarvis from ten's; the rearm line, the quiet hours and the dim held until the trigger ends were added to `lamp-skills` for it. Its cases are made by hand (lamp-4ace's sensor is flaky: it answers, with 20 to 30 failed reads a minute). It ran on lamp-06c1 once with the air fed by hand (`docs/verification/stuffy-air-lamp-06c1-20261017/`). No lamp with a working sensor has run it: lamp-06c1's and lamp-cb94's never answer on I2C bus 0 (#705, 17 Oct) |
| Smoky air (#1684) | `air/state`'s `pm2_5` at least 100 above the lowest of the 3 minutes before (`rise`), and at least 150, for 30 s (`gap` 10 s) | A small lift (`curious`), an amber breath (`warm`, level 2, every 6 s) and, exactly, "The air reads like smoke. If something's burning, please check it." at priority 8, spent if not begun within 2 minutes; the light given back when the line ends. Once a spell, again only after 10 minutes under 150, never within 30 minutes. No quiet hours, but it does not wake a sleeping lamp (the say service refuses while the voice is muted), and nothing at all while the privacy switch is closed (a `[[while]]` on `voice/privacy`'s `physical_muted`, holding on a lamp whose voice does not say it; #3997's rule for a `warning`); no red, flash or chime; no `[stop]`, so a passing "stop" cannot snooze it. Not a smoke alarm (said in the docs only, never by the lamp), and not for the night (its `SKILL.md`) | New: built on #1778's rise, exact words and priority. **Held until measured**: installed disabled (`install::PRODUCT_SKILLS_OFF`); its cases are written by a script, and its four trace cases (kettle steam, toast, a candle blown out, a match) are placeholders until recorded on a sensor lamp. It has not run on a lamp |
| Dusty air (#1684) | `air/state`'s `pm2_5` at least 125 for an hour (`gap` 2 minutes), with a face in view and the voice resting (`wait` 1 hour) | Exactly "The air here has been very dusty for about an hour. I can't tell where it's coming from.", no light, no move. Once a spell (`rearm = "ends"`), at most once a day (`cooldown` 24 hours), never from 22:00 to 08:00 (a night of dust is said once at 08:00) | New: no code added. **Held until measured**, as Smoky air. It has not run on a lamp |
| Dance (#999) | "Lamp, dance", "show me a dance", "dance to this": its tool, `dance(style, long)` | One short playful line, then, when the line ends, one of V1's five dances (#979) picked by the mood asked for (calm: chill or waltz; upbeat: groove or hype; jazz), played twice for a long one, with a low warm glow given back at its end. Never on its own: the ears cannot yet tell music from talk | New: written by jarvis; its tool's arguments, the body's clips by name and the steps after the say were added to `lamp-skills` for it. Its cases are replays; it has not run on a lamp |
| Desk tutor (the owner, 09:45) | "Lamp, check my work" or "I'm stuck" | Tips its head over the page and brightens it, takes one full-resolution still, tutors by voice (a hint or a question, never the answer), a small nod where a step is right, a curious tilt where it is wrong, then faces the person again when the page's 2-minute light ends | Built: `skills/desk-tutor` (#1002, the look #1125, the reading pose #1011/#1098) and its marks, `skills/desk-tutor-marks` (the nod or the tilt, by the model's `show_verdict`, #941); the camera stays at 1280×960 (#987) |
| Light out when you leave (#1677, the owner, 19 Oct 07:2x, a core skill) | `room/presence` says nobody for 10 minutes (`nobody_s` above 599), while the voice rests | The light shown goes out over 30 s, as falling asleep, asked of the ring with a fade; given back the moment someone is seen or heard, or the eyes stop looking. Once each time the desk is left; never from 23:00 to 06:00; never speaks of it ([presence.md](presence.md)) | New: the presence topic, the `out` step and the ring's fade were added for it. Installed disabled (`install::PRODUCT_SKILLS_OFF`, and off with the eyes' `LAMP_PRESENCE`) until its lamp check. Its cases are written by a script; it has not run on a lamp |
| Guard mode (#1685, the first builder's design, 19 Oct 12:08; installable) | Its tool, `keep_watch(turn)`, when the person asks out loud ("keep an eye on things while I'm out") | A faint blue mark on the ring while on; once the desk is empty, one text on `messages/out` when a visit ends, at most one in 10 minutes ("Someone was at your desk at 14:31, for about 2 minutes."), known or not and never a name; ends on "stop", `guard.off` from the app, or the asker's own face back (saying "I'm off guard"); the privacy switch closed ends it with one text; with #2562's switches, arrivals by a door or footsteps followed by a face or speech, `guard.on` from the owner's app, and one turn of the head to an arrival ([guard-mode.md](guard-mode.md)) | New: the watch (`[watch]`), the `messages` and `known_faces` permissions, `--known-file` and `messages/out` were added for it. Not in the product (`NOT_PRODUCT_SKILLS`). Its cases are written by a script; it has not run on a lamp |

| Not feeling well (#1681) | `lamp/health`'s `level` is `error` for 30 s (`gap` 10 s), or, a second `[[trigger]]`, its `quiet_ms` is above 299999 (a part that said `ok` once this boot and has been unheard for 5 minutes, [health.md](health.md); `gap` 10 s), while the voice rests | Droops once (`sleepy`) and its light goes pale and low (`white` at 2, breathing every 10 s down to 1, steady without the ring's `LAMP_LIGHT_REQUESTS=on`; `no_brighter = true` since #3691, so never brighter than the person's light: over level 1 a steady white 1, over a light that is off no light, only the droop) until what ran it ends (10 s after its last `error`, or the quiet part heard again) or `lamp/health` expires (15 s); once each time (`rearm = "ends"`); talking when the 30 s come, it waits for the talk to end (`wait = "1h"`), and does nothing if the error ended first; never from 22:00 to 08:00 (an error held through the night droops once at 08:00); never speaks of it, never red or blinking. `stale` and `warn` do nothing, nor a part restarting for a few seconds or one never heard from. A part that errs and then stops is one droop (its `error` keeps it out of `quiet_ms`). Each trigger holds its own light: an `error` in one part and a different part stopped, at once, droop twice (the runner holds a light for each trigger; one droop for both is not built) | The error trigger needs no code; `quiet_ms` is `lamp-health`'s; a colour held `until = "ends"` was added to `lamp-skills` for its pale breath. In the product since its lamp check, row 8 on lamp-52e6 with the lead (20 Oct 11:35; `install::PRODUCT_SKILLS`). Its cases are written by a script (`lamp/health` every 5 s). On lamp-52e6 it drooped once, slowly, 301 s after the eyes were stopped by hand, its light went pale and low, and it came back 10.8 s after they started; the lead read it as a little unwell, not broken. Its 30 s, 10 s and 5 minutes are not measured against real failures |
| A sip of water (#1685, installable) | Someone at the desk 90 minutes (Stretch break's sitting: `near = 0.14`, `gap` 2 minutes), 10:00 to 17:00, while the voice rests | Turns to them and its light breathes once, a slow cool blue for 6 s; once a sitting, three a day; never a word (a spoken one is the person's own reminder) | New |
| Rest your eyes (#1685, installable) | Someone at the desk 45 minutes, 09:00 to 20:00, while the voice rests | A small `curious` tilt and its light a shade softer while it tilts; once an hour, four a day; the lamp sees faces, not screens, so screen time is time at the desk | New |
| A comfort break (#3504, installable) | Stand up's sitting held 120 minutes, 09:00 to 20:00, with nobody else in view for a minute, while the voice rests | Turns to them and says one short line that it's a good moment for a short break, naming no place or reason; held while anyone else is in view, for up to 30 minutes; once a sitting, twice a day | New |
| A meal check (#3504, installable) | A face at the desk for 75 minutes of lunchtime (11:30 to 13:30) or dinnertime (18:30 to 20:30), with nobody else in view for a minute, while the voice rests | Turns to them and asks once whether they've had lunch or dinner yet, naming no food or menu; held while anyone else is in view, until the window ends; once a window, twice a day | New |
| Sit back (#3694, installable) | The eyes' `leaning_in` held 10 minutes, with nobody else in view for a minute, while the voice rests | Turns to them and says one short, kind line that it might be a good moment to sit back, nothing about a body or looks; a short warm word if they sit back a minute within 15 minutes; held up to 10 minutes while anyone else is in view or they talk; once an hour, three a day | New |
| Stand up (#1685, installable) | Stretch break's sitting held 100 minutes, 08:00 to 20:00, while the voice rests | Turns to them with a small `curious` tilt; once a sitting, twice a day; never a word about posture | New |
| Wind down (#1685, installable) | Someone at the desk 20 minutes in the hour before bedtime (its `[active]`, 22:00 to 23:00 as it comes), while the voice rests | One `sleepy` droop and a shade of dim until they leave (`until = "ends"`); once an evening, never from bedtime on, so the night is Bedtime and Morning's (#1732) if that is on the lamp | New |
| Feelings (#3691) | `mood/state`'s `felt.kind`, three named triggers, `glad`, `lively` and `low`, each once a feeling (`rearm = "ends"`, `wait` 1 minute); only while the voice has rested 5 s and not spoken for 30 s, the person is not head down (`unknown = "holds"`), Mood is not `asleep` (a state run out holds nothing), no hand is on the head, and a face has been in view 10 s | Glad: `glad-lift` (110 counts of pitch up, 3.4 s, held 0.4 s, under the body's 1.5 s stall rule; sized for the head-pitch servo losing about 30 to 50 counts lifting the head, #3691) and `relative = { warmer = 1, dimmer = 1, warm_at_dimmest = true }` with a 6 s breath for 12 s, at the person's level and never above it; at level 1 with the colour still white, the warmth alone, steady (#3691), and nothing when already warm. Lively, the same light: `perk`, 90 counts up in 120 ms steps (about 2.5 s), a 4 s breath for 8 s. Low: `low-droop` (40 counts of pitch down, less than Stuffy air's `sleepy`, eased back to 25 counts above rest for one frame and then to rest, 7.2 s; the overshoot leads the lagging servo until the body leads a gesture's lift) and a steady light warmer and a level dimmer for 20 s. Glad once in 20 minutes, Lively and Low once in 30, two an hour between them (`[budget] group = "feelings"`), six a day, rests after 3 nobody noticed; never from 21:00 to 08:00. No skill can read whether the lamp stands (the body says it only in `diagnostics`' list), so the face 10 s in view stands in for it: the body rises for a face of about 1.5 s in its 6 s, and a feeling never stands a folded lamp up. Its light goes to the ring with `LAMP_LIGHT_REQUESTS=on`, under the night's caps (`ring_caps`' `a_glad_breath_at_the_persons_brightest_draws_nothing_over_a_cap`) | New: the replay's relative light now names its `for` and `breathe_ms`. Installed disabled; its cases are written by a script. It has not run on a lamp |

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

Permissions are built (above, "Core and installable skills"): each skill
declares what it reads and uses, and the program holds it to that. A
package's signature and the Store's install are built ("From the Skills
Store"); the catalogue, review and the Store's server: before anything is
published.

## Core skill: prepare to unplug (#1674)

`skills/prepare-unplug` is tool-only (`prepare_to_unplug`) with `kind = "core"`.
It declares body, speech, its five completion/latch topic reads and the core
grant `core = ["prepare_unplug"]`. These grants
are checked both while loading and at the dedicated runtime boundaries, using
the existing captured-grant lifetime for admitted runs (see [unplug.md](unplug.md)).
The operation is the grant's, not the name's: the loader, runtime admission and bus request
boundary refuse a skill without it before folding can be requested. Its only step is
`body = { prepare_unplug = true }`; triggers, extra steps, choice and flag are
refused so a readiness statement cannot run as an ordinary early `say`. The
initial response is explicitly not readiness. A bounded background run waits
for body, Expression and actual ring receipts; Voice guards the separate
completion statement. Installed with the product, hidden while `LAMP_UNPLUG` is
off. Its second reserved tool, `stay_plugged_in` ("never mind"), is added to
`tools.json` by the program whenever the skill is loaded and refused while no
latch is in force; no skill may
declare that name, and its grants are this skill's (it also reads
`body/unplug_cancel`). See [the lifecycle, timing and physical proof](unplug.md).
