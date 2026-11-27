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
lamp-skills`, which runs every skill's cases). That the wellbeing skills share no
budget (the owner, 27 Nov), which a case of one skill cannot show, is
`two_wellbeing_nudges_within_minutes_both_speak` and
`the_wellbeing_nudges_each_come_when_due_and_share_one_stop` in
`crates/skills/tests/it/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`, #1685) |
|---|---|---|
| `invites-a-stand-after-70-minutes` | 10:00, 110 minutes at the desk | Pass: At 70 minutes (the owner, 27 Nov): turns to the face, `curious` |
| `a-stand-each-sitting` | 08:00, three sittings of 110 minutes, 5 minutes away between | Pass: At 70, 185 and 300 minutes: once a sitting, no daily most |
| `quiet-when-they-stood-up` | 10:00, an hour, three minutes away, another hour | Pass: Nothing: neither sitting is 70 minutes |
| `quiet-while-the-talk-goes-on` | 10:00, 110 minutes, a talk from minute 65 that does not end (moved from 95, 27 Nov) | Pass: Nothing |
| `a-stand-in-the-evening` | 20:00, 110 minutes | Pass: At 70 minutes, 21:10: no hours of its own |
| `off-when-asked` | 10:00, `wellbeing_nudges(how = "off")` at a minute, 110 minutes | Pass: Nothing (`[stop]`) |
| `a-stand-when-lamp-seen-saw-no-stretch` | As `invites-a-stand-after-70-minutes`, with `lamp-seen`'s `humans/doing` every 30 s and no stretch seen (#6543) | Pass: At 70 minutes, as there |
| `quiet-after-a-stretch-was-seen` | 10:00, 110 minutes, a stretch seen at 65 minutes (#6543; moved from 95, 27 Nov) | Pass: Nothing: `break_s_ago` 300 at 70, held through the wait and let go |
