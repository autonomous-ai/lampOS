# Who was here's tests

**How they were made.** Not on a lamp: no lamp has said `room/presence`
yet. The `.jsonl` files are in `lamp-bus record`'s shape, written by hand:
the eyes' `room/presence` every 2 s (each good for 3 s), someone while a face
is seen and for 20 s after, `nobody_s` counting otherwise, as
`crates/eyes/src/away.rs` says it. The calls are the model's, as
`[[call_tool]]` in `expect.toml`; each note is the whole of what the model
is told, the record after it.

**Run** with `lamp-skills test lampOS/skills/who-was-here`.

| Case | What it holds | Result (lamp-skills, first/skill-who-was-here) |
|---|---|---|
| `a-visit-while-away` | Someone for 2 min, gone; a visit of 2 min from 200 s; someone back at 500 s, who asks at 600 s | Pass: the three spells, each 1 min (its faces' length), 10 and 6 min ago, the last going on |
| `nobody-came-between` | Someone for 2 min, gone, back at 400 s, asks at 500 s | Pass: their two spells and nothing between |
| `no-record` | Asked with no `room/presence` ever (presence off) | Pass: no record, and the model told to say it can't tell |
| `never-on-its-own` | Ten minutes of presence with a visit, no call | Pass: nothing |

The recordings carry no `by`, as an older eyes' values do, so each spell
counts as seen; a spell only a voice showed is tested in `visits.rs`
(`a_spell_only_a_voice_showed_is_told_as_heard_not_seen`). Each spell's length is from the value that said someone to the last face
or voice (`nobody_s` back from the value that said nobody), so "1 min" is
the 119 s of faces. The record's lines and a stretch the lamp could not see
are tested on their own in `crates/skills/src/visits.rs`, and the local time
of day in `crates/skills/tests/skills.rs`
(`a_history_is_told_to_the_call_from_room_presence_and_held_to_its_rules`).
Whether the model answers in a few kind words, without guessing who, is for
a lamp (#1685).
