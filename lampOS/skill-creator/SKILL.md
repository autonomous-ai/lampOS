---
name: lamp-skill-creator
description: Create, test and install physical skills for an Autonomous lamp (lampOS V2) - things the lamp does in the room by noticing something with its senses and answering with its body, light and voice. Use when someone says "Lamp, learn to ...", asks to teach the lamp a new behaviour, or to change, test or install one of its skills. Not for software-only skills for Claude Code or Codex.
---

# Lamp skill creator

A physical skill is something the lamp does in the room: it notices
something with its senses (topics on the lamp's bus) and answers with its
body, its light and its voice (services). This skill helps an owner make one
for their own lamp, test it on the lamp's own recordings, and install it.

Read [references/senses-and-actions.md](references/senses-and-actions.md)
before writing anything: a skill can only use what is listed there. The
lamp's design is `lampOS/docs/skills.md` in its repository; you do not need
it to write a skill, and if it disagrees with these files, it is right.

The person is usually the lamp's owner, not a programmer. They said "Lamp,
learn to ..." out loud; the lamp handed it to you. Speak plainly. Never ask
them about topics, TOML or JSON; ask about what they want the lamp to do,
when, and how it should feel in the room.

## The process

1. **Capture what they want** (below). One or two questions at most, then a
   short description back to them in their words.
2. **Check it can be done** with today's senses and actions. If it needs a
   sense or an action the lamp does not have, say so plainly and offer the
   nearest thing it can do. Never invent a topic or a service.
3. **Write the skill folder** ([references/skill-folder.md](references/skill-folder.md)).
4. **Test it on the lamp's own recordings** ([references/testing.md](references/testing.md)):
   at least one recording where it should act and one where it must not.
5. **Install it** ([references/installing.md](references/installing.md)). It is live at once;
   no restart.
6. **Show them**: play the "it acts" recording once on the lamp, so they see
   and hear what it will do, then ask "Keep it?". They say keep it, or what to
   change (go back to 3); nothing is kept that they have not seen.
7. **Tell them** in two sentences what it will do and when, and that it can
   be taken out whenever they like. (Turning a skill off by voice, "Lamp,
   stop doing ...", is not built yet: until it is, a skill is turned off by
   removing its folder, [references/installing.md](references/installing.md).)
8. **Iterate** on what they say after living with it.

## 1. Capture what they want

Get these, from what they said first and as few questions as possible:

- **When**: what in the room starts it. Translate their words into a sense:
  "when I sit down" is a face appearing; "if it gets stuffy" is CO2 high;
  "when I've been at it too long" is a face present for a long time. "When I
  ask you to" is not a sense: it is a `tool` the model calls
  ([references/skill-folder.md](references/skill-folder.md)), and it takes no
  number: "for 25 minutes" is the voice's own timer, not the skill's.
- **What it does**: with the body, the light, the voice. Prefer the body and
  the light; speak only if they asked for speech or it is the point.
- **How often at most**: every skill with a trigger has a cooldown. If they did not say,
  choose one that a person would find natural, not mechanical, and tell
  them.
- **Who it is for**: anyone at the desk, or one known person.

## 2. The lamp is a creature, not a machine

Every skill is judged by what a person in the room sees and hears, not by
whether its trigger fired. Before writing one, ask: would a person or a pet
do this here?

- **Small and rare.** Unprompted behaviour is a glance, a droop, a shade of
  light. Never a lecture, never a scan of the room, never the same line
  twice in a row.
- **Body and light before voice.** A lamp that keeps talking unasked is
  tiring. Speech in an unprompted skill is the exception, said once, short.
- **Silence and stillness are valid.** When in doubt, do less.
- **Never pretend to sense what it cannot.** Say what the lamp actually
  noticed.
- **A person wins.** A person speaking or touching the lamp comes first.
  The runtime does not enforce it yet: the Policy's gate (#939) will be the
  one gate for the pause and a person speaking. Until then a skill keeps out
  of a conversation only with its own `[[while]]` on `voice/state`, and
  **nothing keeps it quiet while the lamp is paused**: say so to the owner
  before installing.
- **Numbers live in `skill.toml`**, never in the prose of a `say`: the runtime
  applies durations and cooldowns, the model never compares numbers.
- **Least privilege.** A skill may read and use only what its
  `[permissions]` lists, and without the table it may do nothing: the
  runtime refuses one that reads a topic or makes a step it does not give.
  Give exactly what it uses, and tell the owner what that is.

## 3. Write the skill folder

```
<name>/
  SKILL.md      name, description, and how the lamp talks about it
  skill.toml    needs, kind, version, permissions, a trigger (or a tool), ask, do
  tests/        recordings of the bus and what the skill should do in each
  moves/ lights/ sounds/   optional data
```

The format and every field: [references/skill-folder.md](references/skill-folder.md).
Use the name the owner would use, in lower case with hyphens:
`stretch-break`, not `ergonomic-reminder-v2`.

No code unless the data truly cannot say it. A `run.py` is optional in the
design, but the first skills prove a new skill needs none; do not write one
without saying why.

## 4. Test

A skill is not finished until it has passed on the lamp's own recordings:

- one recording in which it **must act**, and the calls it must make;
- one in which it **must not** (too soon after the last time, nobody there,
  a person talking to the lamp), and that it makes none.

How to record and replay, and the test file: [references/testing.md](references/testing.md).
If no recording exists for a case, record one on the lamp (with the owner's
yes) or say which case is untested. Never claim a test you did not run.

## 5. Install

Copy the folder to the lamp's skills folder; the runtime reloads it live.
[references/installing.md](references/installing.md) says how, and how to take it back out.
Ask the owner before touching their lamp, and say what will move or light up.

## 6. Show them before keeping it

The owner keeps a skill only after seeing it. The live runtime times
`for`, `gap` and `cooldown` by when messages arrive, so a recording replayed
fast does not set off a 50-minute skill; the tests (recorded time) are the
proof it is right, and the showing is only for the owner to see it:

1. Install a copy named `<name>-demo` whose durations are cut to seconds
   (`for = "3s"`, `gap` and `absent_for` the same, `cooldown = "1m"`), beside
   the real one, which waits for its real durations.
2. Say what is about to happen, then publish what its trigger needs for a
   few seconds by hand, as the eyes or the air part would (a face list every
   second with `lamp-bus pub humans/faces/tracked latest '...' --type
   lamp_msgs/Faces`; an `air/state` with the CO2 high). It acts once, in the
   room.
3. Take the demo copy out at once ([installing.md](references/installing.md)).

Then ask "Keep it?": keep it, change it, or take it out. A skill they did not
keep is removed.

## 7. Tell them, and listen

Tell the owner, in their words, what the lamp will now do and when, and that
it can be taken out whenever they like. After they have lived with it, ask what felt right and what felt
odd; change the numbers first (how long, how often), the behaviour second.

## When this skill is wrong

If something here is not true of the lamp (a topic renamed, a field the
runtime refuses), the lamp's own `lampOS/docs/skills.md` and the runtime's
error are right and this file is stale: follow them, and say what was wrong
so the skill creator can be mended.
