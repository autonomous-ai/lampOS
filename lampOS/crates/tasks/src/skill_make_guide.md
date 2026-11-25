<!-- Adapted from V1's skills/skill-creator/SKILL.md (autonomous-ai/lampOS, main), Apache License 2.0 (its LICENSE.txt). Changed for the lamp: one skill, drafted, checked with the lamp's own check and fixed in this folder; V1's evals, benchmarks and viewer left out. -->
# Making a lamp skill

You make one skill for this lamp from what a person said. The file rules.md in
this folder holds the skill's form and the person's request; where it and this
guide differ, rules.md wins.

## 1. Capture the intent

Take the intent from the person's words in rules.md alone: there is no one to
ask. Answer for yourself:

1. What should the lamp do? Any step format.md lists (a light, a light show,
   a gesture, words said, a cue, a repeating reminder set in the lamp's
   schedule with `remind`, its own code keeping a note, and every other step
   there), as rules.md allows.
2. When should it do it? The one trigger rules.md lists that fits, with the
   times of day only when the person named them. Again and again ("every 30
   minutes", "every minute while I'm here"): a trigger that holds, such as a
   face at the lamp, with no rearm and its cooldown the repeat (at least a
   minute), as examples/water-while-here shows. A one-off reminder or timer
   ("in 25 minutes", "at 22:00") is no skill: the lamp's own reminders set
   it, so leave it out on rules.md's left-out line.
   A nudge (it speaks up with no one asking: a trigger, not words) holds
   back as the lamp's own sip of water does, as examples/break-timer and
   water-while-here show: `[[while]]` the voice not `speaking` for 30 s and
   `resting` for 5 s (never on its own words or mid-conversation), the room
   not `head_down`, those topics in `[permissions]`; and once a sitting
   (`rearm = "leaves"`) unless they asked for a repeat. The lamp sets its
   other manners itself.
3. What did they ask that the form can't do? Leave it out and name it on
   rules.md's left-out line; never invent a field.

Answer rules.md's cannot line only when the request needs something rules.md
names a skill can't yet do (hearing, knowing who someone is, sending); a light
show, a note, its own code, a repeat, a reminder and facts you know are steps,
never limits. Read format.md before any cannot.

What a skill can do, all of it in format.md and examples/:

- **Started by words** (`asks = ["..."]`): the person says them to the lamp;
  examples/desk-light, coin-flip, breathing-coach.
- **Started by an event or a time of day** (`[trigger]`): someone at the
  desk, a long sitting, a sound, a touch, a clock time, named weekdays.
- **Again every N minutes while it holds**: a held trigger, no rearm, its
  cooldown the repeat; examples/water-while-here.
- **A repeating reminder**: `remind = { every, words }`: again every 30
  minutes (15 m to 24 h) whoever is there; started by words or a trigger;
  examples/water-breaks. Once in or at a time is the lamp's reminders', not
  a skill's.
- **Its own code**: scripts/main.py, Python with the lamp's `lampos` SDK.
  This is the widest thing a skill can do, so use it whenever steps alone
  fall short: any call sdk.md lists (light, say, gesture and the body,
  sounds, events with `lamp.on`, leases, notes, the device, the air, the
  camera, the microphone, speech, linked services), loops, waits, sums,
  several steps in order, and Python's own standard library. Code runs in
  one of two step kinds, chosen now, at the make:
  - **`run`**: a `[[do]]` step written exactly `run = "scripts/main.py"`,
    only in a skill the person's own words start (`asks`, no trigger).
    The script does the whole job and calls `lamp.say` for its success
    line and for its failure line (the fetch failed, nothing was found, a
    service isn't linked). The lamp runs it with no agent at all. This is
    the kind to choose whenever the job is the same each time and the
    person asks for it; examples/exchange-rate, sun-times, breathing-coach.
  - **`task`**: `task = { task = "...", run = "scripts/main.py" }`, when
    each start needs the lamp's agent to sum up or reason (sum up today's
    mail, pick what matters in the calendar), and always when a time, an
    event or a repeat starts the skill. Its helper scripts (fetch the mail,
    read the calendar, filter) are written now too, each its own file in
    scripts/, printing what the agent sums up; examples/agenda-count,
    morning-headline.
  Scripts are written at the make, never at a run: a run only runs what
  you wrote here.
- **Its `grants` must name every grant its code's calls need**, as sdk.md
  gives each call's grant: body, light, speak, look, hear, sound, air,
  device, camera, microphone, notes, speech, net, or a linked service's
  `<code>-read` / `<code>-write`. A call whose grant isn't named is refused
  when the skill runs, and the lamp's check can't see that in the code:
  check each call against sdk.md yourself. Code that talks needs `speak`
  (`lamp.say`), only in a skill the person's words start; code that keeps a
  note needs `notes`. Light, speak and body
  go on at once; the others the owner says yes to once.
- **The internet, when rules.md allows `net`**: what only the web knows at
  the time it runs (a price, the weather, the news, a search) is the skill's
  own code granted `net`. Started by the person's words: `grants = ["net",
  "speak"]`, a `run` step, and scripts/main.py fetches it and says it with
  `lamp.say`; examples/exchange-rate. Started by a time, an event or a
  repeat: `grants = ["net"]`, a `task` step, and scripts/main.py fetches it
  and prints it; examples/morning-headline. Light, body, sound, air and
  device may join them.
  Use Python's own `urllib.request` (no packages to install), a public HTTPS
  API that needs no key, a `timeout` of 15 s or less, and a plain spoken
  line when the fetch fails. The run's proxy is already set (`HTTPS_PROXY`),
  and it reaches only the public web on ports 80 and 443, never the home
  network.
- **Beside `net`**: any grant may sit beside it (camera, microphone,
  speech, look, notes, a linked service): all of a made skill's grants
  are on. For "higher than yesterday", keep today's value with
  `lamp.notes` and compare it next run. Again every N
  minutes with the web: the same held trigger and cooldown, its `[[do]]` the
  `task` step. When rules.md doesn't allow `net`, answer its internet line.

**A skill a time, an event or a repeat starts (any `[trigger]`) speaks
only through the lamp's told answer.** Its `[[do]]` is a `task` step. Its
scripts print what they found and never call `lamp.say`, `lamp.sound`, the
speaker or `control.sock`, and its grants never name `speak` or `speaker`.
The agent's last answer, one to three plain sentences, is what the lamp
tells the owner, once: only when the owner is alone in view, no one has
spoken for a minute, and it isn't quiet hours. A `run` step, and code that
says its own line, are only for a skill the person's own words start. The
lamp refuses any other way; examples/morning-headline.

How a skill runs, so its code fits:

- The lamp starts a `task` step's scripts/main.py on its own each time the
  skill starts: by its words, its trigger, or its repeat. One run at a time;
  keep a run short (a fetch and a line or two). Only `notes` lasts from one
  run to the next, and a skill granted `net` has none.
- In a skill the person's words start, what the person hears is what the
  code says with `await lamp.say(text, wait=True)` (said exactly, within
  900 bytes); in one a trigger starts, what the agent answers.
  Always pass `wait=True`: it returns once the line is said, so lines come
  in order and the script never exits before its last line is said (a
  script that exits first has its line dropped, and its run fails).
  Anything printed is not said.
- Open the lamp with `lamp = await Lamp.local()`, `await` every call, and end
  with `await lamp.close()`; run blocking work such as a fetch with
  `await asyncio.to_thread(...)`. sdk.md lists every call, its grant, and its
  limits (`say`: 6 a minute).

A skill's `[[do]]` steps all start together, never one after another: a
light's `for` holds that light, it does not wait. Colours in turn are a light
show (`show`, in a skill started by its words alone) or a `task` step's code
(any skill, a trigger or a repeat too): `await lamp.light(...)`, then
`await asyncio.sleep(...)`, in order.

The examples are forms only: never take their names, words, asks or what
they say; the skill's name and words come from this person's request.

sdk.md's `phrase.heard` is for a paired computer's program; a made skill is
started by its own words with `asks`, never by listening for phrases in its
code.

A skill does nothing that would surprise the person who asked for it.

## How a made skill runs

- **A `run` step**: the lamp runs scripts/main.py itself, in a fence, with
  the `lampos` SDK and the skill's grants alone, and no agent. What the
  person hears is what the script says with `lamp.say`: its success line,
  or its failure line. Anything printed is not said.
- **A `task` step**: the lamp's agent is sent `Run <skill-start> <run id>`
  and the skill's `task` words, as data. It runs that one command: the
  launcher runs the step's script in a fence, with the `lampos` SDK and the
  run's grants alone. The agent sees only what the script prints, runs the
  helper scripts `## Uses` names, and sums up. Its last answer is said
  aloud to the owner, word for word (about 400 characters at most), so it
  is only the spoken summary; a lone dash (-) is said as nothing, the
  answer when a script has said it with `lamp.say` already, which is never
  so in a skill a trigger starts.
- The `task` words are read back whole for the owner's yes: one plain
  sentence a person understands, at most 120 characters.
- **A linked service** (mail, a calendar, GitHub): `grants =
  ["<code>-read"]` to read it, or `"<code>-write"` to send with it. The
  script reads it itself with `await lamp.service_read(service, args)`
  (sdk.md lists it and its args), or the skill's one step is the lamp's own
  `read` step (format.md, "Reading a linked service") when a plain read
  said aloud is enough. A `service_write` is only prepared: the owner's
  spoken yes sends it, never the run. The script never holds a token.
  examples/agenda-count.
- **The public web**: the skill's own code granted `net`, as "The
  internet" above says: urllib reaches the public web on ports 80 and 443
  through the run's proxy (`HTTPS_PROXY`). Started by words, the code says
  the result with `lamp.say` (`speak` granted too); started by a trigger,
  it prints it for the agent's told answer. It is kept only on a lamp whose
  net switches are on. examples/exchange-rate, sun-times, morning-headline.
- **Tried before it is kept**: once its check passes, the lamp runs each
  script once, in that same fence with the skill's own grants: a `run`
  step's script, or each script of a `task` step. Each must exit 0 within
  30 s; a `run` step's must say at least one line with `lamp.say`, and a
  `task` step's must print something (a trigger's never says anything). The try is a real run: it has
  every grant the skill declares (the internet, the camera, the microphone,
  its voice, sounds, light and body), and what it says, shows, keeps or
  sends is done for real, as at the skill's own run. So a script runs with
  no arguments, does its one job once, handles a failed fetch or an
  unlinked service by saying so, and ends. A failed try comes back to you
  as a repair with its words.

## 2. Write a draft

Make the folder draft/NAME in this folder, NAME the skill's name: lowercase
letters, digits and hyphens. Write in it, as rules.md says:

- skill.toml: kind, version, permissions, the trigger, ask and its steps.
- SKILL.md: front matter with name (NAME, the same as the folder), a
  one-line description, a `when` line (when the voice's model should call
  it, at most 200 bytes) and always made: here (as the examples show; the
  check needs it), then a few plain lines on what it does and when, and,
  for a skill with scripts, these three sections, each a `## ` heading with
  lines under it, as the examples show:
  - `## Uses`: each script and lamp-services tool it uses, and its
    arguments;
  - `## Result`: where each script's result is (what it prints, a file);
  - `## Answer`: what is said aloud, in one to three plain sentences:
    the agent's final answer is only the spoken summary, or, only in a
    skill the person's words start, a script says it with `lamp.say` and
    the agent's answer is a lone `-`.
  Its `asks` in skill.toml are the words a person says to start it: one to
  three short, plain phrases, so the voice finds it ("sunset time", "count
  my meetings"). Never a question (no what, how, when, who, is or does) and
  never a no: the lamp refuses such an ask, as a question is the model's to
  answer.
- scripts/main.py for a task step's code, as rules.md allows, using any
  call in sdk.md.
- shows/NAME.toml only for a light show: every frame written out in order, as
  rules.md says. Within 30 frames and a minute nothing is left out.

You write only in this folder, and only the draft you were given: never
edit, patch or write the lamp's own skills, its other skills or any file
outside draft/NAME and answer.md.

## 3. Check it with the lamp's own check

From this folder run, with NAME the draft's name, this and no other command:

    /opt/lampOS/skills/current/lamp-skills pack --out /tmp/lamp-check draft/NAME

"packed" means the lamp's check passed. Anything else is its refusal: read its
words, fix the draft and run the check again. Fix it at most twice, then answer
with the best draft.

## 4. Answer

Write answer.md in this folder: each of the draft's files as its own fenced
block, as rules.md says, and its left-out line when something was left out.
Nothing else goes in answer.md. The lamp checks the answer once more; if it
refuses it, a new make gives you its words in rules.md, and the same steps.
