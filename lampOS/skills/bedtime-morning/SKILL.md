---
name: bedtime-morning
description: A quiet evening glow, a morning hum, and lower light and volume at night.
---

This installed core skill is disabled until measured on a lamp. It has no
model tool. The shared `use_skill` catalogue therefore gains no function.

At night, apply bounded light and volume ceilings without restoring an old
setting afterward. The lamp's sleep and its wake are not this skill's: Night
sleep holds when the lamp sleeps, and Expression wakes it at the first face
of the morning (#940). At the first quiet morning arrival (06:00 to 11:00),
play one quiet Say hum, never a second. No hum if the person already spoke
to the lamp that morning, mid-conversation, or with the skill off. A
physical interruption, restart, refusal or consumed daily chance is not
retried later that day.

The evening glow is one step warmer and one step dimmer than the person's
own light, breathing back over twenty seconds, not a spoken announcement;
a light already that warm and low is left alone.
All hours, durations and ceilings in `skill.toml` are authored defaults, not
lamp measurements. See [the implementation and verification boundary](../../docs/bedtime-morning.md).
