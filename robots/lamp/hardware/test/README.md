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
- `--only` / `--skip` take the step names; `--dry-run` says what it would do.
- `tools/routine-run-test.py` tests it on a host with stand-ins (no lamp, no sound).

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
