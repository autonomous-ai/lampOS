# Quiet focus's tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files and its `expect.toml` were written by
`lampOS/tools/quiet-focus-cases.py` (outside the skill: a skill folder holds
no script, `docs/skill-format.md`), in the format `lamp-bus record` writes, as Sit back's were: the eyes' `room/activity` every 10 s while
someone sits (under the triggers' gaps, 2 minutes and 30 s; its
declaration's 3 s lifespan as the eyes publish it), every 30 s while they
are away, the baseline in from a sitting's second minute and `sitting_s`
from 0 after each time away; faces every 30 s, and every 1.5 s in the
minutes around each moment a word may come (under the faces' 2 s lifespan,
so that `faces_at_most`'s minute can be held at all), a second, smaller
face to one side where a case says someone else is in view, an empty list
while they are away, and the voice resting unless a case says they talk.
Each case's local time is its `expect.toml`'s `clock`. Re-recording a case
on a lamp, with its owner's yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/quiet-focus` (or `cargo test
-p lamp-skills`, which runs every skill's cases). That it speaks of no body
or break, asks nothing of them, takes no picture, says nothing when the
focus comes due and is in the wellbeing group, with its shared `[stop]`, is
`quiet_focus_says_nothing_of_a_body_or_a_break_and_takes_no_picture` in
`crates/skills/tests/it/skills.rs`.

The focus (`focused`, head down 45 minutes) comes due at minute 48 in each
case below, and says nothing; the word (`looked_up`) comes when they look
up within 10 minutes of it. Only the word is a nudge, in the day's two and
the wellbeing group's hour: that the silent focus takes no hour is
`a_silent_trigger_spends_no_hour_and_the_word_after_it_is_the_nudge` in
`crates/skills/tests/it/skills.rs`.

| Case | What it holds | Result (`lamp-skills test`) |
|---|---|---|
| `a-word-when-they-look-up` | 10:00, head down from 3, looks up at 50 | Pass: nothing at 48; at 50 turns to the face and says its line, once |
| `nothing-while-they-stay-head-down` | 10:00, head down from 3 to 60, looks up at 60 | Pass: nothing: past the 10 minutes after the focus |
| `a-short-look-up-is-let-be` | 10:00, head down from 3, a minute looking up at 25, looks up at 50 | Pass: no word at 25; the word at 50 (the minute under the 2-minute gap) |
| `a-short-focus-is-let-be` | 10:00, head down from 3 to 40, then sat up | Pass: nothing |
| `a-long-look-up-begins-it-again` | 10:00, head down 3 to 30, three minutes looking up, head down 33 to 50, looks up at 50 | Pass: nothing: past the gap, and 17 minutes after it |
| `speaks-with-someone-else-in-view` | 10:00, looks up at 50, a second face in view from 49 to 52 | Pass: said at 50, with them there (the owner, 27 Nov: others in view are fine) |
| `held-while-they-talk` | 10:00, looks up at 50 into a conversation to 52:30 | Pass: held at 50, said by 53 once the voice has rested |
| `once-a-sitting` | 10:00, the word at 50, ten minutes looking up, head down again from 60 to 110, looks up at 110 | Pass: the word only once (`cooldown` 180 minutes, inside the sitting's 3 hours) |
| `a-word-each-sitting` | 09:00, three sittings of 55 minutes, looking up at 50 in each, each 3 h 50 after the last | Pass: at 50, 280 and 510 (no daily most, the owner, 27 Nov) |
| `a-word-in-the-evening` | 19:30, head down from 3, looks up at 50: due at 20:18 | Pass: the word at 20:20 (no hours of its own) |
| `off-when-asked` | 10:00, `wellbeing_nudges(how = "off")` at a minute, head down from 3, looks up at 50 | Pass: nothing (`[stop]`) |
| `a-word-at-night` | 22:00, head down from 3, looks up at 50 | Pass: the word at 22:50 (no quiet hours: a person may work late) |

Red runs, each against these cases: with the word on `focused` instead of
`looked_up`, it is said at 48 while they are head down
(`a-short-look-up-is-let-be` fails, first in order); without `follows`,
the word comes at any look up (`a-long-look-up-begins-it-again` fails);
with `follows`' `within` at 20 minutes, the word comes at 60
(`nothing-while-they-stay-head-down` fails); without the voice resting 5 s,
it is said at 51 in the conversation (`held-while-they-talk` fails); with a
1-minute `cooldown`, the second focus in the sitting is told at 110
(`once-a-sitting` fails); with the old `most_a_day = 2`, the third
sitting's word does not come (`a-word-each-sitting` fails); with the old
`[active]` 09:00 to 20:00, nothing in the evening (`a-word-in-the-evening`
fails) or at night (`a-word-at-night` fails); with a 5-minute
`gap`, the three minutes looking up do not end the focus and the word comes
at 50 (`a-long-look-up-begins-it-again` fails).
