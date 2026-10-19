# A sip of water's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#1685), faces every 30 s while someone sits at the desk (a fifth
of the picture to one side and back each time, so each nudge is noticed,
or still where a case says so), every 1.5 s where a light is held until the
sitting ends (under the faces' 2 s lifespan, as the eyes publish them), an
empty list while they are away, and the voice resting. Each case's local
time is its `expect.toml`'s `clock`. Re-recording a case on a lamp, with
its owner's yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/sip-of-water` (or `cargo test -p
lamp-skills`, which runs every skill's cases). The shared hour between the
wellbeing skills, which a case of one skill cannot show, is
`the_wellbeing_skills_share_one_nudge_an_hour_in_the_order_they_came_due` in
`crates/skills/tests/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`, #1685) |
|---|---|---|
| `a-sip-after-90-minutes` | 10:00, 100 minutes at the desk | Pass: At 90 minutes: turns to the face, the light blue at 2 for 6 s |
| `once-a-sitting` | 10:00, four hours without getting up | Pass: Once, at 90 minutes (`rearm = "leaves"`) |
| `three-a-day-at-most` | 10:00, four sittings of 100 minutes, 5 minutes away between | Pass: At 90, 195 and 300 minutes, not at 405 (`most_a_day = 3`) |
| `rests-after-two-nobody-noticed` | 10:00, three sittings, the face never moving | Pass: At 90 and 195, then nothing (`[rest]`) |
| `quiet-when-short-sitting` | 10:00, 80 minutes, then away | Pass: Nothing |
| `quiet-after-its-hours` | 17:00, 100 minutes | Pass: Nothing (`[active]` 10:00 to 17:00) |
| `off-when-asked` | 10:00, `wellbeing_nudges(how = "off")` at a minute, 100 minutes | Pass: Nothing (`[stop]`) |
