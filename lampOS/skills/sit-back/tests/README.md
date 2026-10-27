# Sit back's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#3694), as A comfort break's were: the eyes'
`room/activity` every 5 s, its baseline in throughout (under the triggers'
gaps; its declaration's 3 s lifespan as the eyes publish it), faces every
30 s, and every 1.5 s in the minutes around each moment a word may come
(under the faces' 2 s lifespan, so that `faces_at_most`'s minute can be
held at all), a second, smaller face to one side where a case says someone
else is in view, and the voice resting unless a case says they talk. Each
case's local time is its `expect.toml`'s `clock`. Re-recording a case on a
lamp, with its owner's yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/sit-back` (or `cargo test -p
lamp-skills`, which runs every skill's cases). That it speaks of no body or
looks and takes no picture is `sit_back_says_nothing_of_a_body_and_takes_no_picture`,
and `follows`' rules `follows_names_another_trigger_of_the_skill_and_is_refused_otherwise`,
both in `crates/skills/tests/it/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`, #3694) |
|---|---|---|
| `a-word-after-ten-minutes-leaned-in` | 11:00, sat 3 minutes, then leaned in to the end | Pass: at minute 13, ten minutes in: turns to the face and says it, once |
| `a-warm-word-after-they-sit-back` | 11:00, leaned in from 3 to 16, then sat back | Pass: the word at 13, and a short warm word at 17, a minute after they sat back |
| `a-short-lean-is-let-be` | 11:00, leaned in 8 minutes, then sat back | Pass: nothing |
| `shifting-near-the-line-is-let-be` | 11:00, half an hour of a minute leaned in, half a minute sat back | Pass: nothing: each sit-back is past the 20 s gap |
| `no-warm-word-without-a-word-first` | 11:00, leaned in 5 minutes, sat back 10 | Pass: nothing |
| `no-warm-word-long-after` | 11:00, the word at 13, still leaned in until 33, then sat back | Pass: the word only: sat back past the 15 minutes after it |
| `held-while-someone-else-is-in-view` | 11:00, a second face in view from 11 to 16 | Pass: held at 13, said at 17 once they have been gone a minute |
| `held-while-they-talk` | 11:00, a conversation from 12 to 16 | Pass: held at 13, said by 17 once the voice has rested |
| `quiet-at-night` | 22:00, leaned in from minute 3 | Pass: nothing (`[quiet]` 22:00 to 08:00) |
| `three-a-day-at-most` | 09:00, four times leaned in 11 minutes, an hour apart | Pass: a word and a warm word the first three times, nothing the fourth (`most_a_day = 3`, the warm words not counted) |
| `off-when-asked` | 11:00, `wellbeing_nudges(how = "off")` at a minute | Pass: nothing (`[stop]`) |

Red runs, each against these cases (#3694): with `follows` ignored, the warm
word comes after a lean that drew no word (`a-short-lean-is-let-be` fails);
with the warm word counted and held back as a nudge, it waits for the
group's hour (`a-warm-word-after-they-sit-back` fails); without its
`faces_at_most` condition, the word is said at 13 with the second face in
view (`held-while-someone-else-is-in-view` fails).
