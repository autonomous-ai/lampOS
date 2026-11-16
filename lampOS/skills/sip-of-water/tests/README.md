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
`crates/skills/tests/it/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`, #1685) |
|---|---|---|
| `a-sip-after-30-minutes` | 10:00, 100 minutes at the desk | Pass: At 30 minutes: turns to the face, the light blue at 2 for 6 s |
| `once-a-sitting` | 10:00, four hours without getting up | Pass: Once, at 30 minutes (`rearm = "leaves"`) |
| `three-a-day-at-most` | 10:00, four sittings of 100 minutes, 5 minutes away between | Pass: At 30, 135 and 240 minutes, not at 345 (`most_a_day = 3`) |
| `rests-after-two-nobody-noticed` | 10:00, three sittings, the face never moving | Pass: At 30 and 135, then nothing (`[rest]`) |
| `quiet-when-short-sitting` | 10:00, 25 minutes, then away | Pass: Nothing |
| `quiet-after-its-hours` | 17:00, 100 minutes | Pass: Nothing (`[active]` 10:00 to 17:00) |
| `a-sip-when-the-eyes-say-the-same-sitting` | As `a-sip-after-30-minutes`, with the eyes' `room/activity` 100 ms before each face, its `sitting_s` the sitting's | Pass: At 30 minutes, as there |
| `not-when-someone-else-sat-down` | 10:00, a face at the desk 100 minutes, but `room/activity`'s `sitting_s` begins again at 20 minutes: someone else sat down | Pass: Nothing: that sitter has sat 10 minutes at 30, 20 when the wait lets it go |
| `off-when-asked` | 10:00, `wellbeing_nudges(how = "off")` at a minute, 100 minutes | Pass: Nothing (`[stop]`) |
| `present-every-45-minutes` | `present = true` (`LAMP_NUDGES_PRESENT=on`, #5207): 10:00, four hours without getting up | Pass: At 30, 75, 120, 165 and 210 minutes (no rearm, `cooldown` 45 minutes) |
| `present-quiet-at-night` | `present = true`: 21:40, 100 minutes | Pass: Nothing: due at 22:10, in its quiet hours |
| `present-quiet-when-25-minutes` | `present = true`: 10:00, 25 minutes, then away | Pass: Nothing |
| `a-sip-when-lamp-seen-saw-no-drink` | As `a-sip-after-30-minutes`, with `lamp-seen`'s `humans/doing` every 30 s and no drink seen (#6543) | Pass: At 30 minutes, as there: `drank_s_ago` is the sitting's length |
| `quiet-after-a-drink-was-seen` | 10:00, 100 minutes, a drink seen at 20 minutes (#6543) | Pass: Nothing: `drank_s_ago` 600 at 30, held through the wait and let go |
| `a-sip-when-lamp-seen-went-quiet` | 10:00, 100 minutes, a drink seen at 10 minutes, then `lamp-seen` quiet from 20 (#6543) | Pass: At 30 minutes: the last value ran out, so `unknown = "holds_stale"` holds |
| `present-30-minutes-after-a-drink` | `present = true`: 10:00, 100 minutes, a drink seen at 20 minutes (#6543) | Pass: At 50 and 95 minutes: 30 minutes from the drink, then the `cooldown`'s 45 |
