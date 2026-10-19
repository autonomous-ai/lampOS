# Look for things' tests

**How they were made.** Not on a lamp. `never-on-its-own`'s `.jsonl` files
are in `lamp-bus record`'s shape, written by hand (`voice/heard`,
`voice/state`); the `look-*` cases are the model's calls of the tool, as
`[[call_tool]]` in `expect.toml`.

**Run** with `lamp-skills test lampOS/skills/look-for-things`.

| Case | What it holds | Result (lamp-skills, first/skill-look-for-keys) |
|---|---|---|
| `look-desk` | `look_for` with no place (its default, the desk) | Pass: the look, the reading pose until someone speaks, the desk lit white two minutes, the note |
| `look-left` | `look_for(place = "left")` | Pass: the same, then the pose turned toward -0.5 |
| `look-far-right` | `look_for(place = "far_right")` | Pass: the same, then the pose turned toward 1.0 |
| `never-on-its-own` | Talk of lost keys and glasses, no call | Pass: nothing |

A replay makes each call a fresh run. What one search is is the program's,
tested in `crates/skills/tests/skills.rs`: its first look at the desk
when no place is named, each where it is asked, and none past its looks
(`a_search_looks_at_the_desk_first_then_where_asked_and_no_more_than_its_looks`);
someone speaking ending it, its pose and its light, within a fifth of a
second, and the next ask's first look where it asks
(`someone_speaking_ends_a_search_its_pose_and_its_light`); and a pause
ending it the same way (`a_pause_ends_a_search_its_pose_and_its_light`). Whether the
model finds a thing in the working frame from the reading pose, and whether
the turns read as a glance around rather than a scan, is for a lamp (#1685).
