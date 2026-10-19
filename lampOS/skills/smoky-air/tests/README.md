# Smoky air's tests

**How they were made.** Not on a lamp: no smoke, cooking, candle or steam
trace has been recorded anywhere yet (#1684), and lamp-06c1, where the
skill was written, has no air sensor. Each case's `.jsonl` files were
written by a script in the format `lamp-bus record` writes, as Stuffy air's
later cases were: `air/state` every 5 s, as the sensor gives it, PM2.5 joined
by straight lines between the points each case names (PM1 0.8, PM4 1.05 and
PM10 1.1 times it; CO2 700 ppm, humidity 50 %, temperature 38 °C; a null
reading has every PM null), and `voice/state` resting from the start. The
script is a scratch tool, not kept in the repository. Each `expect.toml`
says what its case holds, and sets the local time of its first message
(`clock`).

**Run** with `lamp-skills test lampOS/skills/smoky-air` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #1684) |
|---|---|---|
| `fires-once-on-a-sharp-rise` | 80, then up to 400 in 90 s (300 to 390 s), held to 900 s, back to 80 | Pass: `curious`, the amber breath and the line, exactly, once at 360 s (the rise and the level hold from 330 s, then 30 s) |
| `speaks-at-night-too` | The same at 03:00 | Pass: once at 360 s: no quiet hours |
| `never-on-a-slow-cooking-rise` | 80 up to 300 over 30 minutes, then 300 for 10 | Pass: nothing (about 22 in any 3 minutes) |
| `never-on-a-short-puff` | Puffs to 300 for 15 s and to 320 for 25 s | Pass: nothing (neither held 30 s) |
| `never-on-noise-or-haze` | An hour of 100 ± 25, with 10 minutes of 130 and 170 by turns | Pass: nothing |
| `never-after-a-gap-or-a-dropout` | 5 minutes with no message, then 400; later 5 minutes of nulls, then 400 | Pass: nothing (no reading in the 3 minutes before to rise from) |
| `rearms-after-ten-minutes-under-150` | The rise; 400 to 1,500 s; under 150 for 11.5 minutes; a second sharp rise | Pass: twice, at 360 and 2,300 s |
| `a-second-spell-within-the-cooldown-is-silent` | The rise; under 150 for 12 minutes; a second sharp rise 23 minutes after the line | Pass: once (the 30-minute cooldown) |
| `a-short-dip-does-not-rearm` | The rise; under 150 for 7 minutes; slowly back to 300; a rise to 450 past the cooldown | Pass: once (not 10 minutes under 150) |
| `kettle-steam` | **Placeholder**: no recording | Counted, not run |
| `toast` | **Placeholder**: no recording | Counted, not run |
| `candle-blown-out` | **Placeholder**: no recording | Counted, not run |
| `match` | **Placeholder**: no recording | Counted, not run |

Changed by hand, the cases catch each number: a 10-minute cooldown, a
rearm of 4 or 12 minutes, a hold of 10 s, a rise of 30, a window of 6
minutes or a level of 400 each fails one of them.

**The four placeholders** are to be replaced by the real recording (#1684,
§6; the first builder, 12:56: "Record real traces: a kettle beside the
lamp, toast, a candle blown out, a match"). Each holds only its
`expect.toml`, which says the event and whether the skill should speak:
the kettle, the candle and the match never; burnt toast, once. A case with
no recording is counted and not run (`lamp-skills test` says "4 have no
recording yet"). To fill one, record on a lamp with an air sensor
(lamp-4ace or lamp-52e6) while the event happens, read only:

    lamp-bus record skills/smoky-air/tests/toast air/state voice/state --seconds 1800

then set the case's `clock` and each call's `at` from it. The skill's
numbers (the rise, the level, the hold) and any steam or combustion rule
are decided from those replays, and the skill stays out of the product's
install (`install::NOT_PRODUCT_SKILLS`) until then.

What no case shows: the say service's own gates (a line in the first pause,
never over speech, spent after 2 minutes), the sleeping lamp saying
nothing, and whether the lift and the breath read as a lamp noticing
something rather than an alarm: those are for the lamp check.
