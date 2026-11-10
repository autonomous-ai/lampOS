# Spotify's tests

**How they were made.** Not on a lamp. `never-on-its-own`'s `.jsonl` files
are in `lamp-bus record`'s shape, written by hand (`voice/heard`,
`voice/state`); `asked-play` has no recording: it is the model's call of the
tool, as `[[call_tool]]` in `expect.toml` (`docs/skills.md`, "tests/").

**Run** with `lamp-skills test lampOS/skills/spotify` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds |
|---|---|
| `asked-play` | `spotify(action = "play")` with playback off: only its line, that the lamp can't play Spotify yet |
| `never-on-its-own` | Talk of a song on Spotify and music on a phone, no call: nothing |

The playing itself is not this skill's: with playback on, the voice offers
lamp-services' `service_play` in this skill's place, tested in
`crates/services` (the Spotify Web API calls, against a local fake server)
and `crates/voice` (who may call it, and that it replaces this function).
A played row on a lamp with a Premium test account is owed (#5791).
