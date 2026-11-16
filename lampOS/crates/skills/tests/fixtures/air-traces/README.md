# Four synthetic air traces (#1684)

Smoky air and Dusty air (`lampOS/skills/smoky-air`, `lampOS/skills/dusty-air`)
are installed off because their numbers were never checked against a
trace. The project lead ruled on 16 Nov 2026 that nobody makes real smoke
or dust for them. These four `air/state` sequences stand in: **made by
hand, not recorded.** No lamp, no fire, no pan and no dusty room is behind
them, so what a real one reads on this sensor is still not known. They
show what the skills' own rules do with four plain shapes.

## The format

One `air/state` message a line, every 5 s, the sensor's period:

    {"at_s":0,"air":{"pm1_0":10.8,"pm2_5":12.0,…,"co2_ppm":null,"feel":"unknown"}}

`at_s` is the seconds from the trace's start. `air` is the message's
payload as it goes on the bus (`lamp_msgs/AirState`,
`crates/msgs/src/air.rs`), byte for byte: the tests and the lamp kit both
cut it out between `"air":` and the line's last brace.

## The sensor and its range

- **The sensor** is a Sensirion SEN63C on I2C bus 0 at 0x6B, read by
  `lamp-air` every 5 s (`lampOS/docs/air.md`, `crates/air`).
- **Its figures:** PM1, PM2.5, PM4 and PM10 in µg/m³, read in tenths
  (`crates/air/src/lib.rs`, `reading`: each word over 10).
- **Its range:** 0 to 1,000 µg/m³ for each (`AirState::check`,
  `crates/msgs/src/air.rs`). Sensirion's product page for the SEN63C says
  the same, with a mass concentration precision of ±10 % (read 16 Nov
  2026). The highest figure here is 494.4 (PM10 in the smoke).
- **Not confirmed:** that it counts no particle under about 0.3 µm
  (`docs/air.md` says so from #1684's design, unconfirmed there too).

## What is assumed, not measured

- **PM2.5** is a straight line between the points in the table below,
  rounded to a tenth. No noise is added: the shape is the claim, and noise
  would be a guess.
- **PM1, PM4 and PM10** follow PM2.5 by fixed ratios, each rounded to a
  tenth: 0.9, 1.02 and 1.03 for the smoke and the cooking spike (fine
  particles), 0.5, 1.3 and 1.6 for the dust (coarser), and 0.8, 1.05 and
  1.1 for the slow rise (as the skills' other cases). Both skills read only
  `pm2_5`.
- **Humidity** is 45 % and **temperature** 36 °C, inside the lamp's warm
  base.
- **CO2 is left unknown** (`co2_ppm` null, `feel` "unknown"), as the sensor
  says it in its first seconds. So a replay on a lamp says nothing false
  about stuffiness: Stuffy air passes an unknown CO2 over (#6592).
- **The lengths.** How long real smoke stays, or how fast a kitchen clears,
  is chosen here, not known. The cooking spike is on the short side on
  purpose: a longer one only gives Smoky air more time.

## The four traces, against the skills' own numbers

Smoky air speaks when PM2.5 is at least 100 above the lowest reading of the
3 minutes before, and at least 150, for 30 s. Dusty air speaks when PM2.5
has been at least 125 for 60 minutes (each skill's `skill.toml`).

| Trace | Stands for | PM2.5 at each point (seconds: µg/m³) | Lasts | Against Smoky air | Against Dusty air |
|---|---|---|---|---|---|
| `smoke.jsonl` | Something smouldering near the desk: a fast rise that stays | 0: 12, 180: 12, 240: 480, 540: 480, 660: 12, 690: 12 | 11.5 min, 139 lines | **Over.** Up 468 within a minute (its 100, 4.7 times), to 480 (its 150, 3.2 times), over 150 for 7 minutes (its 30 s, 14 times) | **Under.** Over 125 for 7 minutes, not 60 |
| `dust.jsonl` | A dusty hour: the air very dusty and staying so | 0: 30, 300: 30, 1500: 180, 5400: 180, 5700: 30, 5760: 30 | 96 min, 1,153 lines | **Under its rise.** 22.5 in any 3 minutes (under a quarter of its 100), though over its 150 for 70 minutes | **Over.** At or over 125 for 74 minutes, 14 more than its hour |
| `cooking-spike.jsonl` | A pan frying in the same room: high and brief | 0: 12, 180: 12, 270: 300, 330: 300, 450: 12, 480: 12 | 8 min, 97 lines | **Over, all three.** Up 288 in 90 s (its 100, 2.9 times), to 300 (its 150, twice), over 150 for 165 s (its 30 s, 5.5 times) | **Under.** Over 125 for 3 minutes |
| `slow-rise.jsonl` | Haze drifting in, or a room filling slowly | 0: 20, 300: 20, 5700: 200, 5850: 200 | 97.5 min, 1,171 lines | **Under its rise.** 6 in any 3 minutes (a sixteenth of its 100), though it ends at 200, over its 150 for 27.5 minutes | **Under.** Over 125 for 40 minutes when it ends, not 60 |

`crates/skills/tests/it/air_traces.rs` holds each file to these figures, so
a trace cannot change without this table.

## What the skills do with them

Each trace replayed in virtual time through the shipped skills, from
10:00, a face in view and the voice resting (the same test file; `cargo
test -p lamp-skills air_traces`):

| Trace | Smoky air | Dusty air | |
|---|---|---|---|
| Smoke | Its lift, its amber breath and its line once, at 230 s: 30 s after the air read 168 at 200 s | Nothing | As meant |
| Dust | Nothing | Its line once, at 4,660 s: an hour after the air read 125 at 1,060 s | As meant |
| Cooking spike | **Its line at 255 s**: 30 s after the spike read 156 at 225 s | Nothing | **As designed: one line a cooking session** |
| Slow rise | Nothing | Nothing | As meant |

**The cooking spike's line, plainly.** With its present numbers Smoky air
says "The air reads like smoke. If something's burning, please check it."
for a pan. Its rule tells a spike from smoke only by the spike being over
150 for under 30 s (its `never-on-a-short-puff` case, puffs of 15 and
25 s), and this spike is over 150 for 165 s. Nothing in the rule looks at
how long the air stays high after the line, so a spike that clears in 2
minutes and smoke that stays for 5 read the same to it.

That is the design, not a fault in it. On PM2.5 alone a pan frying hard and
toast burning are the same shape, and the skill's own `toast` case says
burnt toast gets the line within 2 minutes
(`lampOS/skills/smoky-air/tests/toast/expect.toml`). A hold long enough to
pass this pan over (3 minutes) would pass the toast over too. So the design
allows one line a cooking session (`SKILL.md`: "Frying in the same room may
set it off, once"), and the first builder kept the numbers as they are on
16 Nov 2026 (#1684). The numbers were not changed and the trace was not
shortened to pass.

**The slow rise and Dusty air.** Dusty air is silent on the slow rise
because the trace ends 40 minutes after the air passed 125. The same air
left at 200 for 25 minutes more makes it speak at 7,050 s, an hour after
125 (the test shows it), and rightly: by then the air has been very dusty
for an hour.

**What no trace shows:** a real sensor's noise, steam, a candle, a match,
anything about PM1 or humidity, the say service's own gates, or how the
line sounds in a room.

## On a lamp

`lampOS/tools/air-replay.sh` replays one trace on a real lamp's bus in real
time and says what the two skills did, from `lamp-skills`' own journal
lines. It runs on the lamp itself, as the lamp's user, in a 15-minute slot:
copy the script and one trace there, then

    bash air-replay.sh --check smoke.jsonl    # the preconditions only
    bash air-replay.sh smoke.jsonl            # the trial

- **Which traces.** `smoke.jsonl` and `cooking-spike.jsonl` fit a slot, one
  a slot, and 45 minutes apart on one lamp: after a run Smoky air says
  nothing for 30 minutes whatever comes. `dust.jsonl` and `slow-rise.jsonl`
  are refused as test-only: an hour and more of real time cannot be
  shortened without changing the skills' numbers or the trace.
- **The live sensor.** It stops `lamp-air` for the replay and starts it
  again. Beside a live sensor the room's own reading every 5 s crosses the
  trace, and any replayed level 100 above the room's then reads as a sudden
  rise: with a room's 12 µg/m³ between its readings the slow rise makes
  Smoky air speak (`a_rooms_own_readings_between_a_traces_spoil_the_replay`).
- **What it changes.** The `enabled = false` line out of each of the two
  installed `skill.toml` (how a trial puts them on, `docs/air.md`), and
  `lamp-air` stopped. Both are put back and read back at every exit; its
  header says the rest, and what a power cut leaves (`--put-back`).
- **Its verdict** says whether each skill ran as these tests say: within
  10 s of them, since a live hold is counted on each reading's arrival and
  can end one reading (5 s) later than in virtual time.
- **Not run on a lamp yet.** It was checked on a Mac against the real
  `lamp-skills` and `lamp-bus`, with stand-ins for systemd, the journal and
  the sensor.

## Making them again

A scratch script, not kept: for each second 0, 5, 10, … it takes the
straight line between the two points around it, rounds to a tenth (half
up), and writes the line above with the ratios, humidity, temperature and
CO2 as said.
