# Seen: tired's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#6543, #7401, #7668): every 30 s while someone sits at the desk, the face
(a fifth of the picture to one side and back each time, or still where a
case says so) and Expression's state just before lamp-seen's
`humans/doing_more`, whose counts and timers are the sitting's (each
yawn raising `yawn_seen` and `tired_seen` together, each stretch
`stretch_seen` and `tired_seen`, #8123); an empty
face list while they are away, and the voice resting unless a case says it
talks or speaks, and the privacy slide open (`voice/privacy`'s
`permitted`, said once at the start, as lamp-skills tells it on a change). Each case's local time is its `expect.toml`'s `clock`.
Re-recording a case on a lamp, with its owner's yes, replaces the written
one.

**Run** with `lamp-skills test lampOS/skills/seen-tired` (or `cargo test -p
lamp-skills`, which runs every skill's cases). What a case of one skill
cannot show, the four seen reactions together (each kind's line in its
turn, a different kind sooner than the per-kind rest, one function to stop
them, a line never heard costing nothing, the privacy slide dropping a
kept rise), is
`crates/skills/tests/it/seen.rs`.

| Case | What it holds | Result (`lamp-skills test`) |
|---|---|---|
| `a-late-rise-is-dropped` | 14:00, a yawn seen at 10 in a talk from 9 to 12, and at 36 in a talk from 35 to 40: one line at 12, once the voice rests; the one at 36 is kept 3 minutes from the sighting, dropped at 39 while they still talk, and never said after the turn. | Pass |
| `a-light-line-at-a-stretch` | 14:00, a stretch seen at 10 minutes: it turns to them and says the stretch line, which never names a yawn. | Pass |
| `a-light-line-at-a-yawn` | 14:00, a yawn seen at 10 minutes: it turns to them and says one line. | Pass |
| `a-new-sitting-is-no-sighting` | 14:00, a yawn seen at 10, away from 20 to 25, back till 60: one line, at 10; the count beginning again at 0 is nothing seen. | Pass |
| `a-yawn-and-a-stretch-each-their-line` | 14:00, a yawn seen at 10, a stretch at 12 and a yawn at 14: the yawn line at 10 and the stretch line at 12, each trigger its own rest; the yawn at 14 falls within 20 minutes of the yawn line. | Pass |
| `again-past-twenty-minutes` | 14:00, a yawn seen at 10, 12, 40 and 75 minutes: a line at 10, 40 and 75; the one at 12 falls within 20 minutes of the line at 10 (the per-kind rest), the face never moving. | Pass |
| `at-night-too` | 21:30, a yawn seen at 5 minutes (21:35) and at 35 (22:05): a line at each (no quiet hours; its 20 minutes have passed). | Pass |
| `fires-twenty-seconds-after-its-reply` | 14:00, the lamp spoke from 8:40 to 9:40, a yawn seen at 10, the voice resting: one line (no wait after its reply). | Pass |
| `neither-when-it-cannot-tell` | 14:00, tiredness seen at 10 minutes only from a yawn and a stretch together (`tired_seen` up, neither `yawn_seen` nor `stretch_seen`): nothing, as it cannot tell which. | Pass |
| `not-while-asleep` | 14:00, a yawn seen at 10 minutes while the lamp sleeps (5 to 30): nothing. | Pass |
| `off-when-asked` | 14:00, seen_remarks(how = "off") at a minute, a yawn seen at 10: nothing ([stop]). | Pass |
| `said-once-its-own-reply-ends` | 14:00, a yawn seen at 10 minutes while the lamp speaks (9:30 to 11): one line once it rests at 11. | Pass |
