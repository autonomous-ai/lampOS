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
recording yet"). To fill one, record it with `lampOS/tools/air-trace.sh`
(below). The skill's numbers (the rise, the level, the hold) and any steam
or combustion rule are decided from those replays, and the skill is
installed disabled (`install::PRODUCT_SKILLS_OFF`) until then.

What no case shows: the say service's own gates (a line in the first pause,
never over speech, spent after 2 minutes), the sleeping lamp saying
nothing, and whether the lift and the breath read as a lamp noticing
something rather than an alarm: those are for the lamp check.

## Recording the real events (#1684), about 15 minutes

On a lamp whose SEN63C answers (lamp-4ace read one on 17 Oct; lamp-06c1
and lamp-c0a4 have none), with a window shut and a fan off. The builder
who holds the lamp runs the tool on the Mac beside it; the person does the
events. Each run is one case:

    LAMP_HOST=<lamp> lampOS/tools/air-trace.sh EVENT "what was done"

On the lamp it runs only `lamp-bus record air/state voice/state`, as the
lamp's own user, into a temp folder it removes: no unit stopped, nothing
changed. It counts down 2 minutes of quiet, takes Enter as the event
starts and again as it ends, records 5 minutes after, and writes
`tests/EVENT/` here: the `.jsonl` files as `lamp-bus record` wrote them
and an `expect.toml` with `clock`, the two marks and the PM2.5 before and
after. Its verdict is left for the person who saw the event: until it is
filled in (`none = true`, or the three calls with their `at`),
`lamp-skills test` fails the case. A placeholder's folder is replaced, its
event lines kept; any other folder is kept unless `--force`. The raw
recording stays on the Mac (`~/lamp-played/air-trace/EVENT-<time>.raw/`)
and on the lamp until the case is written and read back the same; if it
cannot be, the tool says where both are and exits nonzero. `--dry-run`
says what it would do. Its stand-in test is `tools/air-trace-test.sh`.
The lamp is reached by key only, as `lampOS/docs/joining.md` says: if it
still takes only a password, the person runs `ssh-copy-id
orangepi@<lamp>.local` once first.

1. **Kettle**, boiling 50 cm from the base for 1 minute: `kettle-steam`.
   Then the same at 2 m: `kettle-steam-2m`. The design's done line asks
   for three runs at 50 cm (`kettle-steam-2`, `kettle-steam-3`) if there
   is time.
2. **Toast** browned in a toaster 1 m away: `toast`.
3. **A candle** burning 1 minute at 50 cm, then blown out (Enter at the
   blow): `candle-blown-out`.
4. **A match** struck and blown out at 50 cm (Enter, Enter at once):
   `match`.
5. **Dust**, for Dusty air (`--skill dusty-air`, which also records
   `humans/faces/tracked`): a duster shaken 1 m from the base for 1
   minute, the person at the desk:

       LAMP_HOST=<lamp> lampOS/tools/air-trace.sh --skill dusty-air dust-shaken-1m "duster shaken 1 m away, 1 minute, person at the desk"

   It writes `lampOS/skills/dusty-air/tests/dust-shaken-1m/`. Dusty air
   speaks only after an hour over 125, so a short event is a never case
   (`none = true`); an hour-long dusty spell is the same command with
   `AIR_POST_S=3600 AIR_CAP_S=5400` (the recording ends by itself at
   `AIR_CAP_S`).

One run of each is enough for provisional numbers. Then fill in each
verdict, run `lamp-skills test lampOS/skills/smoky-air`, and set the
numbers from the replays.
