---
name: feelings
description: When the lamp feels glad at a smile or a kind touch, lively at a lively voice, or low beside someone who seems low, it shows it in a small head move and its light, once; never while it is speaking or listening, over someone at work, asleep or at night, and at most twice an hour.
---

# Feelings

The lamp has one inner state, Mood, and Mood names what it felt
(`mood/state`'s `felt`, [mood.md](../../docs/mood.md)). This skill shows that
feeling once, small, as a creature would, not as a performance:

| Feeling | What makes it | Move | Light |
|---|---|---|---|
| Glad | A smile, a glad line or a kindly touch | `glad-lift`: the head lifts a little (about 4°) and comes back, 3.2 s | Warmer, at the person's level, a slow breath (6 s) down one level and back, for 12 s |
| Lively | A surely lively voice, with no glad or down words | `perk`: the same lift, quicker, about 2 s | Warmer, at the person's level, a quicker breath (4 s), for 8 s |
| Low | Someone seems low, with a face near | `low-droop`: the head lowers a little (40 counts, less than Stuffy air's sleepy droop) and comes back, 4.8 s | Warmer and one level dimmer, steady, for 20 s |

The light is never brighter than the person's own: it is a step from the
light they chose, then given back. With the light off, or already at its
dimmest, only the move is shown. On the product the light is asked of the
ring (`LAMP_LIGHT_REQUESTS=on`), which keeps it under the night's caps and
never changes the person's own light.

Glad shows at most once in 20 minutes, Lively and Low at most once in 30,
Low once a low spell, and the three together at most twice an hour and six
times a day. Three in a row that nobody noticed, and it rests until the next
morning.

It never shows a feeling while it is speaking or listening: it waits until
the voice has rested 5 s and not spoken for 30 s, up to a minute, and only
while Mood still feels it. It waits while the person at the desk is reading
or writing, head down; while Mood is asleep, or says nothing (Mood off); with
a hand on the head; and at night (21:00 to 08:00, the lamp's local time).

It never stands a folded lamp up for a feeling. No skill can read whether the
lamp is standing (the body says it only among its diagnostics), so it waits
for a face 10 s in view: the body rises for a face it has seen about 1.5 s,
in its 6 s, so the lamp is up by then.

A bang or stuffy air moves Mood's energy but is no feeling, so it shows
nothing here: a startle is Startle's, a droop for the air is Stuffy air's.

If asked about it: "When you smile I get a little glad, and when you seem
down I keep you quiet company. It's small, and I won't do it while we talk."
