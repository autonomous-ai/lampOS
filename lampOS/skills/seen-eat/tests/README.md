# Seen: eating's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#6543, #7401, #7668): every 30 s while someone sits at the desk, the face
(a fifth of the picture to one side and back each time, or still where a
case says so) and Expression's state just before lamp-seen's
`humans/doing_more`, whose counts and timers are the sitting's; an empty
face list while they are away, and the voice resting unless a case says it
talks or speaks, and the privacy slide open (`voice/privacy`'s
`permitted`, said once at the start, as lamp-skills tells it on a change). Each case's local time is its `expect.toml`'s `clock`.
Re-recording a case on a lamp, with its owner's yes, replaces the written
one.

**Run** with `lamp-skills test lampOS/skills/seen-eat` (or `cargo test -p
lamp-skills`, which runs every skill's cases). What a case of one skill
cannot show, the four seen reactions together (each kind's line in its
turn, a different kind sooner than the per-kind rest, one function to stop
them, a line never heard costing nothing, the privacy slide dropping a
kept rise), is
`crates/skills/tests/it/seen.rs`.

| Case | What it holds | Result (`lamp-skills test`) |
|---|---|---|
| `a-late-rise-is-dropped` | 10:00, eating seen at 10 in a talk from 9 to 12, and at 36 in a talk from 35 to 40: one line at 12, once the voice rests; the one at 36 is kept 3 minutes from the sighting, dropped at 39 while they still talk, and never said after the turn. | Pass |
| `a-line-at-ate` | 10:00, eating seen at 10 minutes: it turns to them and says one line. | Pass |
| `a-new-meal-past-half-an-hour` | 10:00, eating seen at 10, 12, 43 and 75 minutes: a line at 10, 43 and 75; the one at 12 is the same meal as the one at 10, and 43 is 31 minutes after the last bite at 12, a new meal (the owner, 01:5x on #7668: a meal ends after 30 minutes with no eating seen). | Pass |
| `a-new-sitting-is-no-sighting` | 10:00, eating seen at 10, away from 20 to 25, back till 60: one line, at 10; the count beginning again at 0 is nothing seen. | Pass |
| `at-night-too` | 21:30, eating seen at 5 minutes (21:35) and at 35 (22:05): a line at each (no quiet hours; a new meal). | Pass |
| `fires-twenty-seconds-after-its-reply` | 10:00, the lamp spoke from 8:40 to 9:40, eating seen at 10, the voice resting: one line (no wait after its reply). | Pass |
| `not-while-asleep` | 10:00, eating seen at 10 minutes while the lamp sleeps (5 to 30): nothing. | Pass |
| `off-when-asked` | 10:00, seen_remarks(how = "off") at a minute, eating seen at 10: nothing ([stop]). | Pass |
| `said-once-its-own-reply-ends` | 10:00, eating seen at 10 minutes while the lamp speaks (9:30 to 11): one line once it rests at 11. | Pass |
