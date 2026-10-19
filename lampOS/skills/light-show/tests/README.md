# Light show's tests

**How they were made.** Not on a lamp. `never-on-its-own`'s `.jsonl` files
are in `lamp-bus record`'s shape, written by hand (`voice/heard`,
`voice/state`); the `show-*` cases are the model's calls of the tool, as
`[[call_tool]]` in `expect.toml`.

**Run** with `lamp-skills test lampOS/skills/light-show`.

| Case | What it holds | Result (lamp-skills, twelve/scenes-and-aim) |
|---|---|---|
| `show-party` | `light_show(show = "party")` | Pass: the note, then the party's ten frames |
| `show-any` | `light_show` with no show (its default, party) | Pass: the note, the party |
| `show-music` | `light_show(show = "music")` | Pass: the note, eight breathing frames |
| `show-calm` | `light_show(show = "calm")` | Pass: the note, five frames |
| `never-on-its-own` | Talk of a party and a light show, no call | Pass: nothing |

A replay lists the calls in order; it does not wait. That the show waits
for the line, plays each frame its while, gives the light back after, and
stops within a fifth of a second when someone speaks is the program's
(`crates/skills/tests/skills.rs`,
`a_light_show_plays_its_frames_in_turn_and_stops_when_someone_speaks`).
Whether it reads as a show and not as a machine is for a lamp (#1679).
