# A meal check's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#3504), as A comfort break's were: faces every 30 s while someone
sits at the desk (a fifth of the picture to one side and back each time,
so each nudge is noticed), and every 1.5 s around each moment the question
may come (under the faces' 2 s lifespan, as the eyes publish them, so that
`faces_at_most`'s minute can be held at all), a second, smaller face to one
side where a case says someone else is in view, an empty list while they
are away, and the voice resting. Each case's local time is its
`expect.toml`'s `clock`. Re-recording a case on a lamp, with its owner's
yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/meal-check` (or `cargo test
-p lamp-skills`, which runs every skill's cases). That it names no food and
takes no picture is `a_meal_check_names_no_food_and_takes_no_picture` in
`crates/skills/tests/it/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`, #3504) |
|---|---|---|
| `lunch-after-most-of-lunchtime` | 11:00, 150 minutes at the desk alone | Pass: at 12:45 (minute 105), 75 minutes into lunchtime: turns to the face and asks |
| `dinner-after-most-of-dinnertime` | 18:00, 150 minutes at the desk alone | Pass: at 19:45 (minute 105) |
| `once-at-lunch-once-at-dinner` | 11:00, at the desk until 20:40 without leaving | Pass: at 12:45 and 19:45, never between or again |
| `not-when-they-went-out-for-lunch` | 11:00, an hour, 45 minutes away, an hour | Pass: nothing: 45 minutes of lunchtime left on return, under 75 |
| `asks-with-someone-else-in-view` | 11:00, a second face in view from minute 100 to 110 | Pass: asked at 105, with them there (the owner, 27 Nov: others in view are fine) |
| `asks-at-1315-with-someone-else-there` | 11:00, away an hour, at the desk from 12:00, a second face in view from minute 130 to 158 | Pass: asked at 13:15 (minute 135), with them there |
| `let-go-when-lunchtime-ends` | 11:00, away an hour, at the desk from 12:00, a conversation from minute 130 to 158 (restored 27 Nov, the Reviewer-lead's P3 on #8868: the voice now holds it, not a second face) | Pass: nothing: due at 13:15 and held by the talk; lunchtime ends at 13:30 while the 30-minute wait still runs (to 13:45), so the window's end is what lets it go |
| `quiet-between-meals` | 14:00, four hours at the desk | Pass: nothing, outside both windows |
| `quiet-at-night` | 22:00, 150 minutes at the desk | Pass: nothing at night: no meal window is open then (it has no `[quiet]` since the owner's 27 Nov call; the windows alone keep it silent) |
| `off-when-asked` | 11:00, `wellbeing_nudges(how = "off")` at a minute, 150 minutes | Pass: nothing (`[stop]`) |
| `lunch-when-lamp-seen-saw-no-meal` | As `lunch-after-most-of-lunchtime`, with `lamp-seen`'s `humans/doing` every 30 s and no eating seen (#6543) | Pass: the question at 12:45, as there |
| `not-when-a-meal-was-seen` | 11:00 to 13:30, eating seen at 12:20 (#6543) | Pass: nothing: `ate_s_ago` 1500 at 12:45, held through the half hour's wait and let go |

Before the owner's 27 Nov call its `faces_at_most` condition held the
question while someone else was in view; with it, `asks-with-someone-else-in-view`
fails (asked at 111, not 105).
The replay tells the case's clock at every message (since #3504): without
that, a `during` window's local time lapses two minutes in and nothing is
ever asked.
