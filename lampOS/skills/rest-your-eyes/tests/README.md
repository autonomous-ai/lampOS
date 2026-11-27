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
lamp-skills`, which runs every skill's cases). That the wellbeing skills share no
budget (the owner, 27 Nov), which a case of one skill cannot show, is
`two_wellbeing_nudges_within_minutes_both_speak` and
`the_wellbeing_nudges_each_come_when_due_and_share_one_stop` in
`crates/skills/tests/it/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`, #1685) |
|---|---|---|
| `rests-the-eyes-after-30-minutes` | 09:00, 50 minutes at the desk | Pass: At 30 minutes (the owner, 27 Nov): `curious`, the light a shade dimmer while it plays |
| `every-45-minutes-all-day` | 09:00, five hours at the desk | Pass: At 30, 75, 120, 165, 210 and 255 minutes (`cooldown` 45, no daily most, the owner, 27 Nov) |
| `no-rest-after-two-nobody-noticed` | 09:00, five hours, the face never moving | Pass: As when noticed: no `[rest]` (the owner, 27 Nov) |
| `waits-while-head-down` | 09:00, 70 minutes; the eyes' `room/activity` head down from 25.5 to 45.5 (moved from 40 to 60 for the 30 minutes, 27 Nov) | Pass: Nothing at 30; the tilt at 45, as they look up (`[[while]]` not `head_down`, #1678) |
| `rests-the-eyes-in-the-evening` | 20:00, 100 minutes | Pass: At 30 and 75 minutes: no hours of its own |
| `snoozed-until-the-morning` | 09:00, `wellbeing_nudges(how = "snooze")` at a minute, 100 minutes | Pass: Nothing (`[stop]`) |
