# Startle checks

No lamp recording has been collected for this skill, and this directory is
not evidence of a lamp run. `crates/expression/tests/it/sudden_engine.rs`
reads this skill's `skill.toml` as Expression reads it and plays a sudden
sound through the planner: the same glance as the catalogue's clip before
#2822, frame for frame, and none from a catalogue's own sudden clip.
`crates/skills/tests/it/skills.rs` loads the folder as lamp-skills loads it.
The lamp check is a played clap with someone at the arm.
