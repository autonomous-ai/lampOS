# Rest your eyes' tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#1685), faces every 30 s while someone sits at the desk (a fifth
of the picture to one side and back each time, so each nudge is noticed,
or still where a case says so), every 1.5 s where a light is held until the
sitting ends (under the faces' 2 s lifespan, as the eyes publish them), an
empty list while they are away, and the voice resting. Each case's local
time is its `expect.toml`'s `clock`. Re-recording a case on a lamp, with
its owner's yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/rest-your-eyes` (or `cargo test -p
lamp-skills`, which runs every skill's cases). The shared hour between the
wellbeing skills, which a case of one skill cannot show, is
`the_wellbeing_skills_share_one_nudge_an_hour_in_the_order_they_came_due` in
`crates/skills/tests/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`, #1685) |
|---|---|---|
| `rests-the-eyes-after-45-minutes` | 09:00, 50 minutes at the desk | Pass: At 45 minutes: `curious`, the light a shade dimmer while it plays |
| `four-a-day-at-most` | 09:00, five hours at the desk | Pass: At 45, 105, 165 and 225 minutes, not at 285 (`most_a_day = 4`) |
| `rests-after-two-nobody-noticed` | 09:00, five hours, the face never moving | Pass: At 45 and 105, then nothing (`[rest]`) |
| `waits-while-head-down` | 09:00, 70 minutes; the eyes' `room/activity` head down from 40 to 60 | Pass: Nothing at 45; the tilt at 60, as they look up (`[[while]]` not `head_down`, #1678) |
| `quiet-in-the-evening` | 20:00, 100 minutes | Pass: Nothing (`[active]` 09:00 to 20:00) |
| `snoozed-until-the-morning` | 09:00, `wellbeing_nudges(how = "snooze")` at a minute, 100 minutes | Pass: Nothing (`[stop]`) |
