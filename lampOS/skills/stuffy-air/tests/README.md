# Stuffy air's tests

**How they were made.** Not on a lamp: lamp-4ace's air sensor is stuck at
about 2,000 ppm (#1021, the first builder, 12:29), so there is no real day to
replay. Each case's `.jsonl` files were written by `lamp-bus record` on a
private bus while its messages were published with `lamp-bus pub ...
--at-us` at the case's times, as Welcome back's were (#938). The air is a
series made by hand: a reading every 30 s, CO2 from 700 ppm up to 1,400 in
30 minutes, an hour at about 1,400 (±15), and back down to 700 in 30
minutes. `voice/state` is `resting` from the start unless the case says
otherwise. Each `expect.toml` sets the local time of the case's first
message (`clock`): the skill keeps still from 22:00 to 08:00. Re-recording a
case on a lamp with a working sensor, with its owner's yes, replaces the
synthetic one.

`once-across-nine-minutes-of-good-air`, `waits-out-the-night` and the
shortened `twice-after-good-air` were written by a script in the same
format `lamp-bus record` writes (a reading every 30 s, CO2 joined by
straight lines between the points the row names), not through a bus.

**Run** with `lamp-skills test lampOS/skills/stuffy-air` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #1021) |
|---|---|---|
| `once-a-spell` | One spell from 10:00, then 10 minutes at 700 | Pass: `sleepy` and a dim once, at 31.5 minutes (10 minutes after the air passed 1,200); the dim ends at 99.5 minutes, a minute after the air fell under 1,200 |
| `once-across-a-dip-to-1100` | A spell down to 1,100 only, 20 minutes at 1,100, a second spell from 1,100, then 700 | Pass: once, at 31.5 minutes; the dim ends in the dip (110.5 minutes); the second spell does nothing, since the air never stayed under 1,000 |
| `twice-after-good-air` | A spell; down to 900 by 95 minutes; 11 minutes under 1,000 (94.5 to 105.5); a second spell | Pass: twice, at 31.5 and 118.5 minutes, each dim ending as its air clears (92.5 and 179.5 minutes). With the next case, it pins the skill's own `rearm` `for = "10m"` between 9 and 11 minutes |
| `once-across-nine-minutes-of-good-air` | The same, but 9 minutes under 1,000 (94.5 to 103.5) | Pass: once, at 31.5 minutes; the dim ends at 92.5; the second spell does nothing |
| `waits-for-the-talk-to-end` | One spell from 10:00; `voice/state` listening at 28 minutes, speaking at 29, resting at 35 | Pass: nothing during the talk, then `sleepy` and a dim once, at the first reading after it, 35.5 minutes (`wait = "10m"`, #1089); the dim ends at 99.5. Was `quiet-while-talking`, when a chance held back was spent |
| `ends-when-the-sensor-goes-silent` | From 10:00, up to 1,400 and held, the last reading at 59.5 minutes; `voice/state` each minute from 60 | Pass: once at 31.5; the dim ends at 74.5, 15 minutes after the last reading (`air/state` names no `expires_ms` yet) |
| `quiet-at-night` | One spell from 23:00, over by 01:00 | Pass: nothing: the chance kept through the quiet hours is spent when the spell ends |
| `waits-out-the-night` | Stuffy (1,400) from 23:00 to 10:00, then down to 700 | Pass: nothing in the night; `sleepy` and a dim once at 08:00, the quiet's end, with the chance that came at 23:10 kept; the dim ends at 10:09.5 |
| `quiet-on-a-short-rise` | From 10:00, up to 1,300 and back: 8 minutes above 1,200 | Pass: nothing |
| `stuffy-through-unknown-readings` | From 14:00, stuffy (2,230 to 2,290) from the first reading, every 5 s, with `co2_ppm` null (`feel` unknown) for 90 s in every 4 minutes, 40 minutes | Pass: `sleepy`, a dim and the line once, at 600 s; nothing again while it stays stuffy. The unknown readings are passed over (#6592): before, each 90 s ended the spell past its 1-minute gap and nothing was done or said, as on lamp-52e6 on 12295. Written by a script in `lamp-bus record`'s format, the air part's own message shape (source `air`) |

The `sleepy` look has no case: whether the whole 9.2 s clip reads as a
drowsy lamp, not a gesture on cue, is for the lamp check to judge (still
open; the reviewer on #1066). A smaller droop as a `moves/` file is the
other way, if it does not.

The pause has no case: it is the Policy's gate (#939), which `lamp-skills
test` skips, as Stretch break's `quiet-when-paused` notes.
