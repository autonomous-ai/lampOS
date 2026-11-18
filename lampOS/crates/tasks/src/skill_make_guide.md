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
- **Its own code**: a `task` step running scripts/main.py, Python with the
  lamp's `lampos` SDK. This is the widest thing a skill can do, so use it
  whenever steps alone fall short: any call sdk.md lists (light, say,
  gesture and the body, sounds, events with `lamp.on`, leases, notes, the
  device, the air, the camera, the microphone, speech, linked services),
  loops, waits, sums, several steps in order, and Python's own standard
  library; examples/breathing-coach.
- **Its `grants` must name every grant its code's calls need**, as sdk.md
  gives each call's grant: body, light, speak, look, hear, sound, air,
  device, camera, microphone, notes, speech, net, or a linked service's
  `<code>-read` / `<code>-write`. A call whose grant isn't named is refused
  when the skill runs, and the lamp's check can't see that in the code:
  check each call against sdk.md yourself. Code that talks needs `speak`
  (`lamp.say`); code that keeps a note needs `notes`. Light, speak and body
  go on at once; the others the owner says yes to once.
- **The internet, when rules.md allows `net`**: what only the web knows at
  the time it runs (a price, the weather, the news, a search) is the skill's
  own code granted `net` beside `speak`: `grants = ["net", "speak"]`, and
  scripts/main.py fetches it, says it with `lamp.say`, then prints only
  `Said aloud through the lamp.`: the lamp's agent that ran it sees that,
  ends its answer with a dash, and the person hears it once;
  examples/exchange-rate. Light, body, sound, air and device may join them.
  Use Python's own `urllib.request` (no packages to install), a public HTTPS
  API that needs no key, a `timeout` of 15 s or less, and a plain spoken
  line when the fetch fails. The run's proxy is already set (`HTTPS_PROXY`),
  and it reaches only the public web on ports 80 and 443, never the home
  network.
- **The one limit with `net`**: it never sits beside a private grant
  (notes, camera, microphone, speech, look, or a linked service), so
  nothing private leaves the lamp; the check refuses the pair. So a net
  skill keeps nothing between runs: for "how much it changed", ask the API
  for its own change (e.g. a 24-hour change), never `lamp.notes`. A request
  needing both the web and a private grant is two skills. Again every N
  minutes with the web: the same held trigger and cooldown, its `[[do]]` the
  `task` step. When rules.md doesn't allow `net`, answer its internet line.

How a skill runs, so its code fits:

- The lamp starts a `task` step's scripts/main.py on its own each time the
  skill starts: by its words, its trigger, or its repeat. One run at a time;
  keep a run short (a fetch and a line or two). Only `notes` lasts from one
  run to the next, and a skill granted `net` has none.
- What the person hears is what the code says with `lamp.say(text)` (said
  exactly, within 900 bytes; `wait=True` to say lines in order). Anything
  printed is not said.
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

## 2. Write a draft

Make the folder draft/NAME in this folder, NAME the skill's name: lowercase
letters, digits and hyphens. Write in it, as rules.md says:

- skill.toml: kind, version, permissions, the trigger, ask and its steps.
- SKILL.md: front matter with name (NAME, the same as the folder) and a
  one-line description, and always made: here (as the examples show; the
  check needs it), then a few plain lines on what it does and when.
- scripts/main.py for a task step's code, as rules.md allows, using any
  call in sdk.md.
- shows/NAME.toml only for a light show: every frame written out in order, as
  rules.md says. Within 30 frames and a minute nothing is left out.

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
