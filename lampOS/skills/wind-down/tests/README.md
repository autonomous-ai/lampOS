# Wind down's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#1685), faces every 30 s while someone sits at the desk (a fifth
of the picture to one side and back each time, so each nudge is noticed,
or still where a case says so), every 1.5 s where a light is held until the
sitting ends (under the faces' 2 s lifespan, as the eyes publish them), an
empty list while they are away, and the voice resting. Each case's local
time is its `expect.toml`'s `clock`. Re-recording a case on a lamp, with
its owner's yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/wind-down` (or `cargo test -p
lamp-skills`, which runs every skill's cases). The shared hour between the
wellbeing skills, which a case of one skill cannot show, is
`the_wellbeing_skills_share_one_nudge_an_hour_in_the_order_they_came_due` in
`crates/skills/tests/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`, #1685) |
|---|---|---|
| `winds-down-before-bedtime` | 22:00, 40 minutes at the desk, then away | Pass: At 20 minutes: `sleepy`, a shade of dim until it ends; ended at 42 |
| `waits-for-its-hour` | 21:30, an hour at the desk | Pass: The 20 minutes are up at 21:50; the chance kept to 22:00 (30 minutes), ended at 62 |
| `once-an-evening` | 22:00, 25 minutes, 5 away, 22 more | Pass: Once at 20, ended at 27, not again: its hour's `[budget]` holds the second sitting too, so a `most_a_day` of its own shows only with wider hours |
| `never-from-bedtime-on` | 23:00, an hour | Pass: Nothing (`[quiet]` from 23:00, after `[active]`) |
| `off-when-asked` | 22:00, `wellbeing_nudges(how = "off")` at a minute, 40 minutes | Pass: Nothing (`[stop]`) |
