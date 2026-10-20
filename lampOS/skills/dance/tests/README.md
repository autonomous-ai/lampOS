# Dance's tests

**How they were made.** Not on a lamp. `never-on-its-own`'s `.jsonl` files
were written by `lamp-bus record` on a private bus while its messages were
published with `lamp-bus pub ... --at-us`, as Welcome back's were (#938).
The four `dance-*` cases have no recording: each is the model's call of the
tool, written as `[[call_tool]]` in `expect.toml` (`docs/skills.md`, "tests/").

**Run** with `lamp-skills test lampOS/skills/dance` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, jarvis/dance) |
|---|---|---|
| `dance-any` | `dance` with no arguments | Pass: the note to the model, the warm light, two steps dimmer, one of the five dances |
| `dance-calm` | `dance(style = "calm")` | Pass: the same, with chill or waltz |
| `dance-jazz` | `dance(style = "jazz", long = false)` | Pass: the same, with jazz, once |
| `dance-long` | `dance(style = "upbeat", long = true)` | Pass: the same, with groove or hype, twice |
| `never-on-its-own` | A minute of someone talking about a song and dancing (`voice/heard`, `voice/state`), no call | Pass: nothing |

A replay lists the calls in order; it does not wait. That the dance waits
for the line to end, does not dance after a line the person spoke over,
sends its second clip when the body says on `body/gesture` that the
first is over (and none after `dropped`, or with no `over`), sends no
second after a push, refuses a call while it plays, and
gives the light back at its end is the program's, tested in
`crates/skills/tests/it/skills.rs` against a stand-in voice and a stand-in
body that drops a gesture sent while one plays, as the body does. Whether
it looks like dancing is for a lamp, with its person (#999).
