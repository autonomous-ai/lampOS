# Quiet focus's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes, as Sit back's were: the eyes' `room/activity` every 10 s while
someone sits (under the trigger's 2-minute gap; its declaration's 3 s
lifespan as the eyes publish it), every 30 s while they are away, the
baseline in from a sitting's second minute and `sitting_s` from 0 after
each time away; faces every 30 s, and every 1.5 s in the minutes around
each moment a word may come (under the faces' 2 s lifespan, so that
`faces_at_most`'s minute can be held at all), a second, smaller face to one
side where a case says someone else is in view, an empty list while they
are away, and the voice resting unless a case says they talk. After each
word the person looks up for a minute, as someone who heard it would, so
the word is noticed (`[rest]`). Each case's local time is its
`expect.toml`'s `clock`. Re-recording a case on a lamp, with its owner's
yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/quiet-focus` (or `cargo test
-p lamp-skills`, which runs every skill's cases). That it speaks of no body
or break, asks nothing of them and takes no picture is
`quiet_focus_says_nothing_of_a_body_or_a_break_and_takes_no_picture` in
`crates/skills/tests/it/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`) |
|---|---|---|
| `a-word-after-45-minutes-focused` | 10:00, sat 3 minutes, head down to 60 | Pass: at minute 48, 45 minutes in: turns to the face and says its line, once |
| `a-short-look-up-is-let-be` | 10:00, head down from 3, a minute looking up at 25 | Pass: still at 48: the look up is under the 2-minute gap |
| `a-short-focus-is-let-be` | 10:00, head down from 3 to 40, then sat up | Pass: nothing |
| `a-long-look-up-begins-it-again` | 10:00, head down 3 to 30, three minutes looking up, head down 33 to 70 | Pass: nothing: past the gap, and 37 minutes after it |
| `held-while-someone-else-is-in-view` | 10:00, a second face in view from 46 to 52 | Pass: held at 48, said at 53, once they have been gone a minute |
| `let-go-while-someone-else-stays` | 10:00, head down from 3 to 80, a second face in view from 46 to 70 | Pass: nothing: held past its 15-minute `wait`, then let go |
| `held-while-they-talk` | 10:00, a conversation from 46 to 49:30 | Pass: held at 48, said by 50 once the voice has rested |
| `once-a-sitting` | 10:00, the word at 48, ten minutes looking up from 55, head down again from 65 to 115 | Pass: the word only once (`cooldown` 180 minutes, inside the sitting's 3 hours) |
| `twice-a-day-at-most` | 09:00, three sittings of 55 minutes head down, each 3 h 50 after the last | Pass: at 48 and 278, not at 508 (`most_a_day = 2`) |
| `quiet-in-the-evening` | 19:30, head down from 3: due at 20:18 | Pass: nothing (`[active]` 09:00 to 20:00) |
| `quiet-at-night` | 22:00, head down from 3 | Pass: nothing (`[quiet]` 22:00 to 08:00; outside `[active]` too) |
| `off-when-asked` | 10:00, `wellbeing_nudges(how = "off")` at a minute | Pass: nothing (`[stop]`) |

Red runs, each against these cases: without its `faces_at_most` condition,
the word is said at 48 with the second face in view
(`held-while-someone-else-is-in-view` fails); with a 1-minute `cooldown`,
the second focus in the sitting is told at 110 (`once-a-sitting` fails);
with `most_a_day = 3`, the third sitting's word comes at 508
(`twice-a-day-at-most` fails); with a 1-hour `wait`, the word comes at 71
once the second face has gone (`let-go-while-someone-else-stays` fails);
without `[active]`, the word comes at 20:18 (`quiet-in-the-evening` fails);
without `[active]` and with `[quiet]` from 23:30, at 22:48
(`quiet-at-night` fails); with a 5-minute `gap`, the three minutes looking
up do not end the focus and the word comes at 48
(`a-long-look-up-begins-it-again` fails).
