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
  - **Step 2:** the head pads' timeline, started at the yes. It fits inside
    discover-head's fixed 40 s:
    - hands away for 11 s;
    - then **10 s for pad one** ("Press and hold it for two seconds,
      twice");
    - 10 s for pad two;
    - any other pad until 39 s.
- **`--auto`, one go.** At each question (the stand, letting a held lamp go,
  the head pads, the self-test) the lamp says what to do, then "Starting in
  five seconds", and answers the question itself after those 5 s. The reset
  is never answered for the person. Ctrl-C stops at any time.
  `factory-lamp.sh` runs this way by default; `ASK=1` answers each question
  at the laptop.
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
  - **`--auto-reset`** (a batch's explicit choice) answers a passed
    self-test's reset too, after `--before-reset CMD` has run.
- **A factory tool, not the product:** the prompts belong to the hardware
  team's bench and are never part of the image.
- `--no-voice` mutes it and `--dry-run` prints each prompt instead.
  `tools/factory-voice-test.py` tests it on a host with stand-ins (no lamp, no
  sound).

## `tools/factory-lamp.sh` and `tools/factory-run.sh`: many lamps from one Mac

Each lamp runs its own factory steps (`factory-run.sh`, started detached on
the lamp) and says every step on its speaker. The Mac only starts runs and
collects results, so one person keeps several lamps going at once:

```bash
export LAMP_PASS=<lamp password> STEPS=<steps.sh from lamp-voice> STEPS_SHA=<its sha256> PROMPTS=prompts
tools/factory-lamp.sh start 3e83 29e7 75c9     # unset lamps: ~20 s each over its hotspot, then the next
tools/factory-lamp.sh follow 3e83              # optional: watch one lamp's run
# … set every lamp up from the app (iPhone Auto-Join off) …
RESET=yes tools/factory-lamp.sh selftest 172.168.20.180 172.168.20.189   # all at once, bench network
tools/factory-lamp.sh collect 3e83 29e7 75c9   # one table: factory-records.csv
```

**The unset run, on the lamp:**
1. **Checks:** r2 (no `country_code`), 4b (camera `01da:5875@3-1.1.4`), 4f, 4g, that the lamp is unset, and the servos (`servo-calib.py check`, below): V1's settings are stored when missing; servos never calibrated stop the run before anything moves.
2. **The head onto its stop:** `head-nudge.py`, when the rest read refuses.
3. **Step 1, the arm stand.**
4. **Every input in one window** (`input-check.py`, below).
5. **Step 3, the key.**
6. **4d:** `--gaze`.

The person answers nothing: each question is said, then "Starting in five seconds", then answered (`factory-voice.py --auto`).

**The self-test (step 4)** runs on set-up lamps in parallel.
- **The ring:** the hardware process accepts a colour drawn **within 4 s**; lamp-selftest allows 1 s, which can't be changed. When the ring is the self-test's only failed part, `ring-check.py` decides it. Any other failed part fails the lamp.
- **Not checked:** only `touch_by_hand` and `storage` may be not checked (#3530; `ALLOWED_NOT_CHECKED`). Any other part not checked (`head_move` while the lamp listens or a joint is warm, the speaker, the camera) fails the step, to be run again, and the lamp is never reset.
- **The reset:** with `RESET=yes`, a passed lamp is reset for packing the way `steps.sh` resets it. Its record is written first; then the lamp is stopped and each part waited on, then `lamp-setup reset-now` runs as `lamp-setup-wipe`.

**On the lamp, in `/var/lib/lamp-test`** (outside the reset's wipe list, so it survives the 5 s reset):
- `factory.env` — the record:
  - `HEAD_PAD_LINES` (two of 37, 96, 97, 98, differing by unit);
  - `BUTTON_PRIMARY`, `RESET_BUTTON`, `PRIVACY_SWITCH`;
  - `HEAD_REST`, `BODY_POSES`;
  - each step's result, `SELFTEST_FAILED`, the build and `steps.sh`'s sha256.
- `factory-run.log` — everything the steps printed.
- `factory-run.status` — `RUNNING <step>`, `PASS <mode>` or `FAIL <mode>: <why>`.

**The Mac:** each unset lamp is at `192.168.100.1`, so each gets its own `known_hosts` (in `STATE`, default `~/.factory-lamp`), and its hostname is read before anything runs. A re-flashed card's new host key is accepted once, and that hostname check decides. `follow` and `collect` use the bench network first, and a lamp's hotspot only when it is unset.

## `tools/servo-calib.py`: the servos' own settings

A lamp whose servos never ran V1 HAL (every V2-only image) has neither of
V1's two settings, and the stand fails without them (lamp-220c, 28 Oct):

| | As the servos come | V1 (lamp-3e83) | Effect when missing |
|---|---|---|---|
| Max acceleration (EEPROM 85), P (21) | 50, 32 | 254, 16 | the body asks for acceleration 60, reads back 50, and refuses the stand |
| Zero (31), limits (9, 11) | one shared offset, 0–4095 | each joint its own; limits on yaw, roll, pitch | the head reads off its stop; lamp-4ace's stand drove lamp-220c's elbow into its body |

V1 stored the first at every start (lerobot `configure_motors`, `hal/follower/hal_follower.py`) and the second once by hand (`hal.calibrate`). All of it is in the servos' EEPROM: it survives a reflash, an update and the reset, and is redone only for a new servo or a remounted horn.

```bash
tools/factory-lamp.sh calibrate 220c   # an unset lamp, a person at it: configure, sweep, the plan, then yes to write
sudo python3 servo-calib.py check      # read only: exit 0 calibrated, 3 not configured, 4 not calibrated
```

- **Nothing is driven.** Torque must be off (lamp-body stopped); every write refuses otherwise.
- **The sweep:** the lamp starts folded at rest, head on its stop; the person moves each joint end to end by hand (base turn, shoulder, elbow, head roll, head pitch) and folds it back. A joint moved under 400 counts refuses the plan.
- **The plan** matches lamp-3e83: folded, the shoulder and elbow read 2047 and the head pitch 2010 (in the body's home range); yaw and roll have their swept range centred on 2047. Limits sit 30 counts inside the sweep on yaw, roll and pitch, so a joint is never sent into its hard stop; the shoulder and elbow keep 0–4095, as on lamp-3e83. lamp-220c calibrated this way stands at 2629,2564; lamp-3e83 at 2630,2562.
- **The factory run** checks before the head is nudged: it stores V1's settings itself (nothing moves) and stops a lamp never calibrated with "factory-lamp.sh calibrate ID". Records `SERVOS` and `SERVO_OFFSETS`.
- **Test:** `tools/servo-calib-test.py` runs it against stand-in servos on a pty (no lamp).

## `tools/input-check.py`: every input in one window

```bash
sudo python3 input-check.py            # 30 s touch and press, 30 s hands off, write the pins
sudo python3 input-check.py --dry-run  # the same reading, nothing written
```

**The windows** (nobody races a clock):
1. **Waiting.** The lamp says "Input test. Touch any head pad or press my button to begin." and waits up to 5 min (`--wait-seconds`).
2. **30 s touch,** from that first touch, which counts: "Thirty seconds. Touch each head pad, press my button, and press the reset button briefly. Any order, as many times as you like."
3. **Hands off.** After "Now hands off me", touches finishing late still count as touches. The 30 s quiet window opens only once nothing has been touched for 3 s (`--settle-seconds`).

**What it reads:**
- the four pad candidates, each probed with both pulls (a driven line is a pad's chip, and its level is its rest);
- the button (line 99), the reset (line 100) and the privacy switch (chip 1, line 9), where the pins file names them.

**When it passes:**
- two or more pads, each touched twice or held 0.5 s, and still in the quiet window, as `steps.sh` judges;
- the button and the reset each pressed;
- nothing stuck.

The privacy switch is recorded whether or not it moved.

**What it writes:** the pads into the pins file exactly as `steps.sh` step 2 writes them (`head_pads` and `head_rest`, which lamp-controls reads, #711), with the profile kept beside. Every result goes out as `RECORD` lines for `factory.env`.

**lamp-controls is stopped** while the lines are read, so a reset held here resets nothing.

## `tools/ring-check.py`: the ring, each colour within 4 s

```bash
sudo python3 ring-check.py               # a set-up lamp: the ten colours, 4 s each at most
sudo python3 ring-check.py --within 2    # a stricter bound
```

- **The bound:** the hardware process's ring check (the hardware lead, #3530, 27 Oct). lamp-selftest's ring allows 1 s (`DRAWN_WITHIN` in `crates/selftest/src/ring.rs`); on lamp-29e7 on 7071 it failed three runs in a row with colours late.
- **How:**
  - Each of the self-test's ten colours is asked for exactly as lamp-selftest asks: `lamp_msgs/Light` on `light/test`, level 3, asked again every second.
  - It is seen on `light/drawn`, from the ring owner's own write receipts.
  - The bus is spoken directly (docs/bus.md), because the image has no `lamp-bus` binary.
- **Records** `RING` and `RING_DRAW_S`, each colour's time.
- **Exit:** 0 all in time, 1 some not, 3 none drawn (the privacy switch closed, or a turn owns the ring).
- **Test:** `tools/ring-check-test.py` tests it against a stand-in ring owner (no lamp).

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
