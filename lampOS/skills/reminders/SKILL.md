---
name: reminders
description: When a reminder the person set comes due, the lamp looks at them and says it, in its own voice, once.
---

# Reminders

A reminder or timer the person asked for, said when it is due
(`lampOS/docs/skills.md`, "The first skills"). The lamp looks up at the
person, as someone at the next desk does before saying "your call's in
five minutes", and says it. Nothing more: no gesture, no light.

- **Trigger:** once on the first eligible positive `schedule/due`, when its
  store, pending count or oldest occurrence changes, or when Schedule is
  ready/enabled and the voice rests again (`voice/state`). A held older item
  need not disappear before a later reminder gets a chance. An unchanged
  resting heartbeat makes no new chance; growing lateness is not a change.
  The next eligible heartbeat after a talk can ask again even with the same
  count. One reminder run may execute at a time; additional chances while
  it waits are rejected as busy.
- **Ask:** the Policy's `reminder` question. The person asked for this, so
  it should hold only while someone is talking to the lamp.
- **Do:** ask the voice to say the reminder it keeps (`say = { from = "schedule" }`, #939). The voice keeps
  the reminder and the delivery. Only after Voice says it has begun does the
  skill look at the face, if one is in view. Voice says it in this skill's own words
  (`[reminder.words]` in its `skill.toml`, #940, `docs/skills.md`), and
  acknowledges it to Schedule only once it was heard whole. With
  `LAMP_SCHEDULE_SPEAK=skill` the voice waits up to 10 s for this skill,
  then says the reminder itself, so a reminder never waits on this skill.

A positive count is an opportunity, not a delivery identity. Voice selects
the actual due occurrence and keeps its existing five-second attempt pace.
A later eligible reminder can get another skill chance while pending stays
positive; Voice's ten-second fallback remains if no skill takes it.

Asked about it, the lamp says it reminds people of what they asked it to,
and that a reminder can be cancelled by asking.
