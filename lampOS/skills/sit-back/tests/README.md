# Sit back's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script (the last three with `away` spans: no face, and a new sitting with no baseline for its first 2 minutes after a gap over 2 minutes, as the eyes say it) in the format `lamp-bus record`
writes (#3694), as A comfort break's were: the eyes'
`room/activity` every 5 s, its baseline in throughout (under the triggers'
gaps; its declaration's 3 s lifespan as the eyes publish it), faces every
30 s, and every 1.5 s in the minutes around each moment a word may come
(under the faces' 2 s lifespan, as the eyes publish them), a second,
smaller face to one side where a case says someone
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
| `a-word-after-a-minute-leaned-in` | 11:00, sat 3 minutes, then leaned in to the end | Pass: at minute 4, a minute in (the owner, 27 Nov): turns to the face and says it, once |
| `a-warm-word-after-they-sit-back` | 11:00, leaned in from 3 to 16, then sat back | Pass: the word at 4, and a short warm word at 17, a minute after they sat back |
| `a-short-lean-is-let-be` | 11:00, leaned in 45 seconds, then sat back (was 8 minutes, 27 Nov) | Pass: nothing |
| `shifting-near-the-line-is-let-be` | 11:00, half an hour of a minute leaned in, half a minute sat back | Pass: nothing: each sit-back is past the 20 s gap |
| `no-warm-word-without-a-word-first` | 11:00, leaned in 45 seconds, sat back 14 (was 5 minutes, 27 Nov) | Pass: nothing |
| `no-warm-word-long-after` | 11:00, the word at 4, still leaned in until 33, then sat back | Pass: the word only: sat back past the 15 minutes after it |
| `speaks-with-someone-else-in-view` | 11:00, leaned in from 3, a second face in view from 2 to 8 | Pass: the word at 4, with them there (the owner, 27 Nov: "drop all the other in view") |
| `held-while-they-talk` | 11:00, leaned in from 11.5 (moved from 3, 27 Nov), a conversation from 12 to 16 | Pass: held at 12.5, said by 17 once the voice has rested |
| `a-word-at-night` | 22:00, leaned in from minute 3 | Pass: the word at 4: no quiet hours (the owner, 27 Nov) |
| `a-word-each-lean` | 09:00, four times leaned in 11 minutes, an hour apart | Pass: a word and a warm word each time: no daily most (the owner, 27 Nov) |
| `off-when-asked` | 11:00, `wellbeing_nudges(how = "off")` at a minute | Pass: nothing (`[stop]`) |
| `no-warm-word-on-coming-back` | 11:00, the word at 4, then away from 16 to 20 and back (a new sitting, its baseline again) | Pass: the word only: back past the 15 minutes after it |
| `a-second-lean-within-half-an-hour-is-let-be` | 11:00, the word at 4 and the warm word at 17, leaned in again from 20 to 28 (moved from 30 to 45, 27 Nov) | Pass: nothing more: the 30 minutes since the word not past (`cooldown`, the owner, 27 Nov) |
| `no-rest-after-two-unnoticed` | 09:00, leaned in from 3, 80 and 150, each word unnoticed for a minute | Pass: the word at 4, 81 and 151, a warm word at 96 and 166: no `[rest]` (the owner, 27 Nov) |
| `a-bad-posture-alone-is-let-be` | 11:00, sat at the desk 20 minutes, never leaned in, `lamp-seen`'s `humans/posture` `bad_share` 0.9 on the neck all through (written as Posture's cases are) | Pass: nothing: the posture reading is Posture's (`skills/posture`, the owner's call of 23 Nov), never Sit back's |
| `a-word-after-a-minute-with-no-sitting-first` | 11:00, the eyes' baseline, then leaned in from 2 to the end (written from `a-word-after-a-minute-leaned-in`, 27 Nov) | Pass: the word at 3: no twelve minutes' sitting first (the owner, 27 Nov) |

Red runs, each against these cases (#3694): with `follows` ignored, the warm
word comes after a lean that drew no word (`a-short-lean-is-let-be` fails);
with the warm word counted and held back as a nudge, it waits
(`a-warm-word-after-they-sit-back` fails); without `cooldown`, the second
lean within half an hour is told (`a-second-lean-within-half-an-hour-is-let-be`
fails). With the old 10 minutes, `faces_at_most` or the sitting's 12 minutes
(before the owner's 27 Nov call), `a-word-after-a-minute-leaned-in`,
`speaks-with-someone-else-in-view` and
`a-word-after-a-minute-with-no-sitting-first` fail.
