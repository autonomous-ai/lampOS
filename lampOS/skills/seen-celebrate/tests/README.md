# Seen: celebrating's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#6543, #7401): every 30 s while someone sits at the desk, the face
(a fifth of the picture to one side and back each time, or still where a
case says so) and Expression's state just before lamp-seen's
`humans/doing_more`, whose counts and timers are the sitting's; an empty
face list while they are away, and the voice resting unless a case says it
talks or speaks. Each case's local time is its `expect.toml`'s `clock`.
Re-recording a case on a lamp, with its owner's yes, replaces the written
one.

**Run** with `lamp-skills test lampOS/skills/seen-celebrate` (or `cargo test -p
lamp-skills`, which runs every skill's cases). What a case of one skill
cannot show, the four seen reactions together (each kind's line in its
turn, one function to stop them, a line never heard taking nothing), is
`crates/skills/tests/it/seen.rs`.

| Case | What it holds | Result (`lamp-skills test`) |
|---|---|---|
| `a-new-sitting-is-no-sighting` | 15:00, celebrating seen at 10, away from 20 to 25, back till 60: one line, at 10; the count beginning again at 0 is nothing seen. | Pass |
| `a-warm-line-at-celebrating` | 15:00, celebrating seen at 10 minutes: it turns to them and says one line. | Pass |
| `at-night-too` | 21:50, celebrating seen at 22:05: one line (no quiet hours). | Pass |
| `every-one-seen-has-its-line` | 15:00, celebrating seen at 10, 12, 40 and 75 minutes: a line at each (no cooldown, no day's most, no group's hour or gap, no rest), the face never moving. | Pass |
| `fires-twenty-seconds-after-its-reply` | 15:00, the lamp spoke from 8:40 to 9:40, celebrating seen at 10, the voice resting: one line (no wait after its reply). | Pass |
| `not-while-asleep` | 15:00, celebrating seen at 10 minutes while the lamp sleeps (5 to 30): nothing. | Pass |
| `off-when-asked` | 15:00, seen_remarks(how = "off") at a minute, celebrating seen at 10: nothing ([stop]). | Pass |
| `said-once-its-own-reply-ends` | 15:00, celebrating seen at 10 minutes while the lamp speaks (9:30 to 11): one line once it rests at 11. | Pass |
| `said-once-they-stop-talking` | 15:00, celebrating seen at 10 minutes while they talk to the lamp (9 to 14): nothing while they talk, one line once the voice rests at 14, though the count rose four minutes before. | Pass |
