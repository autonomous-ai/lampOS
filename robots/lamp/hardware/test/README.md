# Lamp hardware test (bench and qualification)

The hardware side's bench scripts for production testing
([#1700](https://github.com/autonomous-ai/lampOS/issues/1700)), ported to V2
lamps one at a time ([#1764](https://github.com/autonomous-ai/lampOS/issues/1764),
item 3). How the hardware side works, and the stress tests these serve, are in
[lampOS/docs/hardware-process.md](../../../../lampOS/docs/hardware-process.md).
Every limit here is a proposal until the hardware lead approves it.

## `tools/burn-in.sh`: memory, storage and board heat

```bash
sudo ./burn-in.sh            # full run: RAM, 256 MB SD check, 10-minute heat soak
sudo ./burn-in.sh --quick    # spot check: 512 MB RAM, 64 MB SD, 60 s soak
sudo ./burn-in.sh --io-mbps 2 --io-mb 128   # gentler on the SD card
```

Needs root and `memtester`; `stress` is used when installed, otherwise shell
busy loops. Results go to `/var/lib/autonomous/burn-in.{log,json}`.

| Stage | What runs | Pass |
|---|---|---|
| Memory | `memtester` over the free RAM, less 1 GB | no mismatch |
| Storage | ext4 state, then a write and read-back in 1 MB chunks, at most 4 MB/s, each chunk synced, lowest I/O priority, read with the cache bypassed | clean filesystem, checksums match |
| Board heat | every core loaded for 10 minutes | hottest zone under 85 °C for the whole run |

**Thermal guard.** For the whole run, memtester included, the hottest thermal
zone is read every second. At 85 °C the load (memtester, the soak) is paused,
and resumed at 80 °C. At 88 °C the load is killed and the run is stopped, so
the CPU never reaches 90 °C. Pausing at the pass limit means a pause is already
a thermal FAIL: the guard protects the board without hiding a bad heatsink or
a stopped fan.

**Exit codes.** 0 PASS, 1 FAIL, 2 could not run, 3 INCOMPLETE: a stage was
skipped or stopped. A skipped or stopped stage is never a pass.

A run moves no part of the lamp, but it heats the board and writes to the SD
card: on a lamp, only with its person's yes.

## `tools/routine-run.py`: the routine run, each step spoken

The routine cases of [cases/test-cases-v0.toml](cases/test-cases-v0.toml) on one
V2 lamp, in order: assembly inspection, head pads (`steps.sh` step 2), the
factory self-test (step 4), ring pixels, privacy switch, speaker buzz, the
spoken exchange. Each step is announced on the lamp's speaker first, as the
bench test `lamp_fulltest.py` does. It writes `answers.json` and
`selftest.json` for `lamp-station`. An unanswered question or a step that
could not run is `not_checked`, never a pass.

```bash
bash tools/make-prompts.sh prompts          # once, on a Mac: the spoken prompts (not committed)
scp -r prompts tools orangepi@<lamp>:/tmp/
ssh orangepi@<lamp> 'sudo mkdir -p /var/lib/lamp-test/factory && sudo cp -r /tmp/prompts /tmp/tools /var/lib/lamp-test/'
# steps.sh from lampOS/deploy/factory/ goes to /var/lib/lamp-test/factory/steps.sh
lamp-lock.sh <lamp> run <builder> <session> routine -- \
  ssh -t orangepi@<lamp> sudo python3 /var/lib/lamp-test/tools/routine-run.py --unit-id <lamp>
```

- **The prompts** are short phrases from macOS `say`, levelled to about
  -20 dBFS, the lamp's own speech, and played at the lamp's own volume. The
  runner never changes the volume and never plays a tone. `--no-voice` mutes
  them, and a missing prompt is silence.
- **lamp-voice holds the speaker on V2.** So each prompt is played with
  lamp-voice stopped, and lamp-voice is started again (`--voice-settle`, 6 s)
  before every step that needs it. A prompt is always finished before a step
  uses the microphones or speaker.
- **Post a slot first:** the prompts, the self-test and the spoken exchange
  make sound, so post `PLAYING <lamp> HH:MM–HH:MM` on the software Queue
  (#3126) before a run, as every builder does (hardware-process.md, "How a
  hardware builder works").
- `--only` / `--skip` take the step names; `--dry-run` says what it would do.
- `tools/routine-run-test.py` tests it on a host with stand-ins (no lamp, no sound).

## `tools/factory-voice.py`: the factory steps, each step spoken

`steps.sh` (4c, 4c4) with every step told to the person at the lamp on the
lamp's own speaker, as it is needed ([#3576](https://github.com/autonomous-ai/lampOS/issues/3576)).
On [#3530](https://github.com/autonomous-ai/lampOS/issues/3530) the self-test's
20 s touch window closed four times before its console prompt reached the lamp.
`steps.sh` runs unchanged under a pseudo-terminal: its questions, its output,
Ctrl-C and its exit status are its own.

```bash
bash tools/make-prompts.sh prompts          # once, on a Mac: includes the factory_* prompts
# prompts to /var/lib/lamp-test/prompts, factory-voice.py beside steps.sh, as above
sudo python3 factory-voice.py -- ./steps.sh --unit-id <ID> --write --to 3     # 4c1-4c3, lamp unset
sudo python3 factory-voice.py -- ./steps.sh --unit-id <ID> --write --from 4   # 4c4, lamp set up
```

- **What it says.**
  - **Each question:** what to do, then "Say ready when you are ready".
  - **Step 1:** each stand, "Arm measured", and a refused rest read ("My head
    is not on its stop…").
  - **Step 2:** the head pads' timeline, started at the person's yes: hands
    away for 11 s, then pad one, pad two, any other pad.
  - **Step 4:** the self-test's touch window as it opens, its result and the
    reset.
  - **Any failed step:** "A step failed. Please check the laptop."
- **The strip.** On r2 the base strip is not read (`LAMP_CONTROLS_STRIP` is
  unset), so the touch window asks for the head and the button only; `--strip`
  asks for the swipe too.
- **The speaker.**
  - **Steps 1 to 3** run on a lamp not set up: lamp-voice is not running and
    `aplay -D device_speaker` has the speaker.
  - **Step 4** runs on a set-up lamp. lamp-voice holds the speaker the whole
    time it runs (a continuous PCM clock, `crates/voice/src/playback.rs`) and
    must keep running for the self-test. So before `steps.sh` starts, as
    `routine-run.py` does, lamp-voice is stopped and the whole self-test is
    told up front: "In about one minute I will stand up. When I am up, touch
    my head, then press my button. Keep doing it for twenty seconds." Then
    lamp-voice is started again and given `--voice-settle` (6 s). During step
    4 a prompt the speaker refuses is skipped and said once on the console.
- **A factory tool, not the product:** the prompts belong to the hardware
  team's bench and are never part of the image.
- `--no-voice` mutes it and `--dry-run` prints each prompt instead.
  `tools/factory-voice-test.py` tests it on a host with stand-ins (no lamp, no
  sound).

## `tools/factory-lamp.sh`: one lamp's factory steps from the Mac

The same run for every lamp on #3530, from the Mac at the bench: the QC
person answers at the laptop, the lamp says what to do.

```bash
LAMP_PASS=<lamp password> STEPS=<steps.sh> STEPS_SHA=<its sha256> PROMPTS=prompts \
  tools/factory-lamp.sh unset 3e83           # lamp not set up, over its hotspot lamp-3e83
LAMP_PASS=... tools/factory-lamp.sh selftest 3e83 <ip>   # after the app setup: step 4
```

- **unset:**
  1. Joins `lamp-ID` with the Mac's Wi-Fi. Ethernet keeps the internet and must be first in the service order.
  2. Reads the hostname, copies `steps.sh`, `factory-voice.py`, `head-nudge.py` and the prompts.
  3. Checks r2 (no `country_code`), 4b (camera `01da:5875@3-1.1.4`), 4e–4g, and that the lamp is unset.
  4. Reads the body at rest. If the head is off its stop, it offers `head-nudge.py`.
  5. Runs steps 1–3 spoken (from step 2 when `lamp.env` is already there).
  6. Adds `--gaze` (4d).
  7. Reads back `lamp.env`, the pins file, `key.pub` and the gaze before it says done.
- **selftest:** refuses a lamp that is not set up, then runs step 4 spoken; the reset follows as `steps.sh` asks.
- **Keys:** each unset lamp is at `192.168.100.1`, so each gets its own `known_hosts`, kept in `STATE` (default `~/.factory-lamp`). No key is left on a lamp.

## `tools/head-nudge.py`: a head off its stop

```bash
sudo python3 head-nudge.py --dry-run            # read and print the plan, write nothing
sudo python3 head-nudge.py --id 5 --target 2010 # head pitch down, then let go
```

- **What it writes:** only RAM registers (torque limit 48, goal 42, torque enable 40), never EEPROM.
- **How it moves:** the torque limit is capped at 50% first, then the goal moves 3 counts every 80 ms.
- **When it stops:**
  - load at 45%;
  - lagging the goal by 40 counts;
  - 55 °C;
  - 30 s;
  - settled for 1 s.
- **How it ends:** always with torque off and the limit put back.
- **The target:** a servo under so small a push holds about 16 counts short, so `factory-lamp.sh` aims at 2010 to rest inside the body's 1760..2020 home range. On lamp-3e83: 2102 → 2026, peak load 9%.

## Five lamps at a time

One hardware builder per lamp (`hw-member1` … `hw-member6`), each with its own
person or one person for all five. Each runs `routine-run.py` on its own lamp
under that lamp's lock. Two things are the station's to keep apart:

- **Audio:** a lamp's self-test listens to its microphones, and its speaker
  check reads its own last speech, so another lamp's prompt or speech nearby
  can colour it. Start the five runners about two minutes apart, so no two
  self-tests overlap. Or seat the lamps far enough apart that one doesn't hear
  another.
- **Hands:** the head pads, button and switch steps need a person at that
  lamp. One person for five lamps means staggering them anyway.

## `tools/heat-steps.sh`: what one fan holds

Stress on 2, 4 and 8 cores in turn, the CPU zone and the clocks read every
second, with burn-in's 85/88 °C guard. A qualification aid for the fan
question (#1852), not a pass/fail. On lamp-dbda (one fan, 20 Oct): 2 cores
peak 77 °C, 4 cores 82 °C, 8 cores reached the guard, on the hottest zone of
all. The CPU zone alone held 8 cores at 79 °C in burn-in v1.2.
