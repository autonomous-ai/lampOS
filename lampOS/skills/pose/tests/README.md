# Pose's tests

**How they were made.** Not on a lamp. Each case is the model's call of
the tool, as `[[call_tool]]` in `expect.toml`.

**Run** with `lamp-skills test lampOS/skills/pose`.

| Case | What it holds |
|---|---|
| `pose-lower` | `set_pose(pose = "lower")`: the note, and the stand at 60 |
| `pose-sit` | `set_pose(pose = "sit")`: the note, and the stand at 30 |
| `pose-normal` | `set_pose(pose = "normal")`: the note, and the stand at 100 |

That the stand is published on `body/stand_asked` only with
`LAMP_BODY_POSE_ASK=on` is the program's (`crates/skills/tests/it/skills.rs`,
`a_stand_is_published_only_with_the_pose_ask_switch_on`), and that the body
takes it, the body's (`crates/body/src/stand_in.rs`). Whether the arm gets
there and holds through a conversation is for a lamp (#4135).
