# Light scenes' tests

**How they were made.** Not on a lamp. `never-on-its-own`'s `.jsonl` files
are in `lamp-bus record`'s shape, written by hand (`voice/heard`,
`voice/state`); the `scene-*` cases have no recording: each is the model's
call of the tool, as `[[call_tool]]` in `expect.toml` (`docs/skills.md`,
"tests/").

**Run** with `lamp-skills test lampOS/skills/light-scenes` (or `cargo test
-p lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, twelve/scenes-and-aim) |
|---|---|---|
| `scene-reading` | `light_scene(scene = "reading")` | Pass: the note, the reading pose for a minute, white at 4, kept |
| `scene-night` | `light_scene(scene = "night")` | Pass: the note, orange at 1, kept, no pose |
| `scene-energise` | `light_scene(scene = "energise")` | Pass: the note, white at 5, kept |
| `scene-none` | `light_scene` with no scene | Pass: only the note, which asks which scene |
| `never-on-its-own` | Talk of a dark room and a reading light, no call | Pass: nothing |

A replay lists the calls in order. That a kept light is set through the
actions part and never given back, and that another scene asked while the
head is down sets its light and lets the pose go, is the program's, tested
in `crates/skills/tests/skills.rs`
(`a_scene_keeps_the_persons_light_and_the_next_scene_lets_the_pose_go`).
Whether each scene looks right is for a lamp, with its person (#1679).
