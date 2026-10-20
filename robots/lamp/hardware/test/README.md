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
