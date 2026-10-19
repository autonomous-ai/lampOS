# Point the light's tests

**How they were made.** Not on a lamp. `never-on-its-own`'s `.jsonl` files
are in `lamp-bus record`'s shape, written by hand (`voice/heard`,
`voice/state`); the `point-*` cases are the model's calls of the tool, as
`[[call_tool]]` in `expect.toml`.

**Run** with `lamp-skills test lampOS/skills/point-light`.

| Case | What it holds | Result (lamp-skills, twelve/scenes-and-aim) |
|---|---|---|
| `point-ahead` | `point_light` with no argument (its default, ahead) | Pass: the look, the reading pose for two minutes, white at 4 kept, the note |
| `point-left` | `point_light(toward = "left")` | Pass: the same, then the pose turned toward -0.5 |
| `point-right` | `point_light(toward = "right")` | Pass: the same, then the pose turned toward 0.5 |
| `never-on-its-own` | Talk of a book and a keyboard, no call | Pass: nothing |

That a turned pose takes the place of the one held is the program's
(`crates/skills/tests/skills.rs`, `a_turned_pose_takes_the_place_of_the_one_held`),
and that the base turns for it the body's (`crates/body`,
`a_reading_lamp_turns_toward_the_place_it_is_asked_and_back`). Whether the
turn lands the light on a book, with `--gaze` as lamp-4ace has it, is for a
lamp (#1679).
