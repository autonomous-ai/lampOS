---
name: startle
description: The lamp glances up once at a sudden sound, a bang or something dropped, then lets it go.
---

This core skill is the lamp's startle. It has no model tool and says
nothing, lights nothing and plays nothing.

When a short sound rises suddenly out of the room's quiet (`ears/sudden`),
the head glances up with a slight tilt and comes back to rest, in 1.6 s.
Only while the lamp rests, listens or thinks, never while it speaks; held
still while someone works close; and once: the next sounds within 30 s get
nothing. Its move is `[startle]` in `skill.toml`. Expression reads it when it
starts and plays it as a reflex, since a startle that comes late is no
startle; there is no other startle. See [Expression](../../docs/expression.md).
