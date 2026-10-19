# Stand up's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#1685), faces every 30 s while someone sits at the desk (a fifth
of the picture to one side and back each time, so each nudge is noticed,
or still where a case says so), every 1.5 s where a light is held until the
sitting ends (under the faces' 2 s lifespan, as the eyes publish them), an
empty list while they are away, and the voice resting. Each case's local
time is its `expect.toml`'s `clock`. Re-recording a case on a lamp, with
its owner's yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/stand-up` (or `cargo test -p
lamp-skills`, which runs every skill's cases). The shared hour between the
wellbeing skills, which a case of one skill cannot show, is
`the_wellbeing_skills_share_one_nudge_an_hour_in_the_order_they_came_due` in
`crates/skills/tests/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`, #1685) |
|---|---|---|
| `invites-a-stand-after-100-minutes` | 10:00, 110 minutes at the desk | Pass: At 100 minutes: turns to the face, `curious` |
| `twice-a-day-at-most` | 08:00, three sittings of 110 minutes, 5 minutes away between | Pass: At 100 and 215 minutes, not at 330 (`most_a_day = 2`) |
| `quiet-when-they-stood-up` | 10:00, an hour, three minutes away, another hour | Pass: Nothing: neither sitting is 100 minutes |
| `quiet-while-the-talk-goes-on` | 10:00, 110 minutes, a talk from minute 95 that does not end | Pass: Nothing |
| `quiet-in-the-evening` | 20:00, 110 minutes: due at 21:40 | Pass: Nothing (`[active]` 08:00 to 20:00) |
| `off-when-asked` | 10:00, `wellbeing_nudges(how = "off")` at a minute, 110 minutes | Pass: Nothing (`[stop]`) |
