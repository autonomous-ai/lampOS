---
name: reminders
description: When a reminder the person set comes due, the lamp looks at them and says it, in its own voice, once.
---

# Reminders

A reminder or timer the person asked for, said when it is due
(`lampOS/docs/skills.md`, "The first skills"). The lamp looks up at the
person, as someone at the next desk does before saying "your call's in
five minutes", and says it. Nothing more: no gesture, no light.

- **Trigger:** `schedule/due` shows a reminder due where none was, while
  Schedule is on and ready and the voice rests (`voice/state`): one due
  while someone talks with the lamp is kept until the talk ends, and asked
  for then, at Schedule's next second, when the voice can say it too
  (`wait`). Asked for during the talk, the ask would be gone 20 s later,
  and the voice would say the reminder 10 s after the talk with no look.
- **Ask:** the Policy's `reminder` question. The person asked for this, so
  it should hold only while someone is talking to the lamp.
- **Do:** look at the face, if one is in view, and ask the voice to say the
  reminder it keeps (`say = { from = "schedule" }`, #939). The voice keeps
  the words and the delivery: it says the reminder as it always did, and
  acknowledges it to Schedule only once it was heard whole. With
  `LAMP_SCHEDULE_SPEAK=skill` the voice waits up to 10 s for this skill,
  then says the reminder itself, so a reminder never waits on this skill.

Two reminders due at once are one trigger: the second is said by the
voice when the first is done, after its 10 s.

Asked about it, the lamp says it reminds people of what they asked it to,
and that a reminder can be cancelled by asking.
