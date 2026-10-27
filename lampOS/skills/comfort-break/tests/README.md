# A comfort break's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#3504), as Stand up's were: faces every 30 s while someone sits at
the desk (a fifth of the picture to one side and back each time, so each
nudge is noticed), and every 1.5 s around each moment a nudge may come
(under the faces' 2 s lifespan, as the eyes publish them, so that
`faces_at_most`'s minute can be held at all), a second, smaller face to one
side where a case says someone else is in view, an empty list while they
are away, and the voice resting. Each case's local time is its
`expect.toml`'s `clock`. Re-recording a case on a lamp, with its owner's
yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/comfort-break` (or `cargo test
-p lamp-skills`, which runs every skill's cases). That it never names a
place or a need is `a_comfort_break_never_names_the_bathroom` in
`crates/skills/tests/it/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`, #3504) |
|---|---|---|
| `a-break-after-two-hours` | 10:00, 130 minutes at the desk alone | Pass: At 120 minutes: turns to the face and says its line |
| `twice-a-day-at-most` | 09:00, three sittings of 130 minutes, 5 minutes away between | Pass: At 120 and 255 minutes, not at 390 (`most_a_day = 2`) |
| `quiet-when-they-stood-up` | 10:00, an hour, three minutes away, 70 minutes more | Pass: Nothing: neither sitting is two hours |
| `quiet-in-the-evening` | 18:30, 130 minutes: due at 20:30 | Pass: Nothing (`[active]` 09:00 to 20:00) |
| `quiet-at-night` | 21:00, 130 minutes: due at 23:00 | Pass: Nothing (`[quiet]` 22:00 to 08:00) |
| `held-while-someone-else-is-in-view` | 10:00, a second face in view from minute 115 to 125 | Pass: Held at 120, said at 126, once they have been gone a minute |
| `let-go-while-someone-else-stays` | 10:00, a second face in view from minute 115 to 160 | Pass: Nothing: held past its 30-minute `wait`, then let go |
| `off-when-asked` | 10:00, `wellbeing_nudges(how = "off")` at a minute, 130 minutes | Pass: Nothing (`[stop]`) |

Without its `faces_at_most` condition, `held-while-someone-else-is-in-view`
fails: the line is said at 120 minutes, with the second face in view.
