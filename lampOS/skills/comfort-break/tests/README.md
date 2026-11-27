# A comfort break's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#3504), as Stand up's were: faces every 30 s while someone sits at
the desk (a fifth of the picture to one side and back each time, so each
nudge is noticed), and every 1.5 s around each moment a nudge may come
(under the faces' 2 s lifespan, as the eyes publish them), a second, smaller face to one
side where a case says someone else is in view, an empty list while they
are away, and the voice resting. Each case's local time is its
`expect.toml`'s `clock`. Re-recording a case on a lamp, with its owner's
yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/comfort-break` (or `cargo test
-p lamp-skills`, which runs every skill's cases). That it never names a
place or a need is `a_comfort_break_never_names_the_bathroom` in
`crates/skills/tests/it/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`, #3504; the owner's 27 Nov timers) |
|---|---|---|
| `a-break-after-an-hour` | 10:00, 130 minutes at the desk alone | Pass: At 60 minutes: turns to the face and says its line, once |
| `a-break-each-sitting` | 09:00, three sittings of 130 minutes, 5 minutes away between | Pass: At 60, 195 and 330 minutes: one each sitting (130 minutes, so no repeat at 135), no daily most |
| `a-break-an-hour-into-the-next-sitting` | 10:00, an hour, three minutes away, 70 minutes more | Pass: Nothing in the first, just short of 60 minutes; at 123, 60 minutes into the second |
| `a-break-in-the-evening` | 18:30, 130 minutes | Pass: At 60 minutes, 19:30: no hours of its own |
| `a-break-at-night` | 21:00, 130 minutes | Pass: At 60 minutes, 22:00: no quiet hours |
| `speaks-with-someone-else-in-view` | 10:00, 130 minutes, a second face in view from minute 50 to 70 (written from `a-break-after-an-hour`, 27 Nov) | Pass: At 60 minutes, with them there (the owner: the toilet speaks with others in view) |
| `again-75-minutes-later-while-they-stay` | 10:00, 140 minutes at the desk without getting up (written 27 Nov, faces every 30 s) | Pass: At 60 and again at 135 minutes (the owner, 27 Nov: "Again 75 min later") |
| `off-when-asked` | 10:00, `wellbeing_nudges(how = "off")` at a minute, 130 minutes | Pass: Nothing (`[stop]`) |
| `present-a-break-after-60-minutes` | `present = true` (`LAMP_NUDGES_PRESENT=on`, #5207): 10:00, 130 minutes alone | Pass: At 60 minutes, once |
| `present-a-break-at-night` | `present = true`: 21:00, 130 minutes | Pass: At 60 minutes, 22:00 |
| `present-a-break-an-hour-into-the-next-sitting` | `present = true`: 10:00, an hour, three minutes away, 70 minutes more | Pass: At 123, 60 minutes into the second sitting |
| `present-again-75-minutes-later-while-they-stay` | `present = true`: 10:00, 140 minutes without getting up | Pass: At 60 and 135 minutes |
| `a-new-sitting-counts-60-minutes-afresh` | `present = true`: 10:00, at the desk to 90 minutes, away 10, back at 100 to 175 | Pass: At 60, then at 160 (60 minutes into the new sitting), not at 135 |

With the old `faces_at_most = 1` condition, `speaks-with-someone-else-in-view`
fails: the line is held until the second face has been gone a minute, at 71.
