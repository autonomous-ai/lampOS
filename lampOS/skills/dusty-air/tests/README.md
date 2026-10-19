# Dusty air's tests

**How they were made.** Not on a lamp: no long dusty day has been recorded
yet (#1684), and lamp-06c1, where the skill was written, has no air sensor.
Each case's `.jsonl` files were written by a script in the format
`lamp-bus record` writes: `air/state` once a minute (the trigger's gap is 2
minutes), PM2.5 joined by straight lines between the points each case names
(PM1 0.8, PM4 1.05 and PM10 1.1 times it; CO2 700 ppm, humidity 50 %,
temperature 38 °C); `humans/faces/tracked` half a second before each
reading, since a face lasts 2 s to `lamp-skills`, one face at the desk or
none; and `voice/state` resting unless the case says otherwise. The script
is a scratch tool, not kept in the repository. Each `expect.toml` says what
its case holds, and sets the local time of its first message (`clock`).

**Run** with `lamp-skills test lampOS/skills/dusty-air` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #1684) |
|---|---|---|
| `once-after-an-hour-at-the-desk` | From 10:00, 140 for three hours, someone at the desk | Pass: the line, exactly, once at 11:00 |
| `never-in-an-office-at-about-100` | Four hours between 85 and 124 | Pass: nothing |
| `a-dip-under-125-starts-the-hour-again` | 140 for 40 minutes, 110 for 5, then 140 | Pass: once, at 105 minutes |
| `once-a-day` | 140 10:00 to 12:00 and 13:00 to 15:00; the next day from 10:30 | Pass: twice, at 11:00 and at 11:30 the next day; nothing for the second spell of the first day |
| `waits-for-someone-at-the-desk` | 140 from 10:00, nobody in view until 11:30 | Pass: once, at 11:30 |
| `lets-it-go-with-nobody-there-for-an-hour` | 140 from 10:00, nobody in view until 12:10 | Pass: nothing (kept an hour, then spent) |
| `waits-for-the-talk-to-end` | 140 from 10:00, a talk from 10:58:20 to 11:04:30 | Pass: once, at 11:05, the first reading after it |
| `never-at-night` | 140 from 22:00 to 02:00 | Pass: nothing |
| `once-at-eight-after-a-night-of-dust` | 140 from 22:00 to 10:00 | Pass: once, at 08:00 |

Changed by hand, the cases catch each number: a 1-hour cooldown, quiet
hours from 23:30, or a level of 99 each fails one of them.

The skill stays out of the product's install (`install::NOT_PRODUCT_SKILLS`)
until it is measured: Smoky air's four traces (its `tests/README.md`) and an
office's own days, recorded read only on a lamp with an air sensor and
replayed here. What no case shows: the Policy's gate (`nudge`), which
`lamp-skills test` skips, and the voice's own gates for a say.
