# Promote gate of build 15860 on lamp-ee17

This is the first builder's gate (#7814, 12:02 and 12:08, and the 12:29 update). It promotes **15860**. Round 2's build
(15866) only proves 15860's updater.

The kit has not run on a lamp yet. `mock/run-all.sh` runs it against stub ssh, lock, afplay and osascript, and needs no
lamp.

## Before the run
- Read this file, then `gate/kit.sh` (settings) and `run.sh` (steps).
- Use an env file (`KIT_ENV=<path>`). The kit sources it. It never copies or prints it:
  ```
  LAMP_HOST=<address>          # LAMP defaults to lamp-ee17, LAMP_USER to orangepi
  LAMP_SUDO_PASS=<sudo password>
  LOCK_WHO=<lock holder>       # with KIT_LL; LOCK_SESSION defaults to gate-ee17
  KIT_LL=<path>/lamp-lock.sh
  ```
- The numbers have defaults, and each one can be set: `STABLE_SEQ=15759 STABLE_UPD=b213d9041a88 CAND_SEQ=15860
  CAND_UPD=e00e49d4785d NEXT_SEQ=15866 NEXT_UPD=e00e49d4785d`. 15866 carries 15860's lamp-update. If `NEXT_SEQ` is set
  empty, round2 refuses to run.
- The Mac needs bash, python3, ffmpeg, ffprobe, afplay and osascript. Its output volume must be 70 or lower. The kit
  reads that volume and never sets it.
- **lamp-ee17's speaker cap is 28.** `restore`, both peek reads and `core` read the kept level
  (`/var/lib/lampos-output-caps/kept-volume`) and the bus value (`speaker/volume`). The kit refuses to run when either
  is above 28, or when both are unread (`VOL_UNREAD_OK=1` goes on anyway). It never sets a volume.
- The kit does not lift a mute. If the voice is muted, the peek reads and `core` stop.
- No earlier kit's binary swap may be on. If there is a mount over `/opt/lampOS`, `restore` stops and changes nothing.

## Commands (from `kits/gate-ee17/`)
`KIT_ENV=<env> ./run.sh` runs every step in order. Each step stops the run on a FAIL (exit 3).
- `./run.sh <step>` resumes at that step.
- `./run.sh <step> <then>` runs one step, then goes on at `<then>`.
- `UNTIL=<step>` stops before that step.

On any exit, the EXIT trap puts lamp-peek's monitor drop-in back if the gate laid it. The lock is kept.

| step | what it does | go on at |
|------|--------------|----------|
| lock | lamp-lock try, then a renew every 60 s | `LOCK TAKEN` |
| render | renders TIME, TASK, MAKE and YES (`gate/clips/clips.txt`) through the lamp's ElevenLabs relay | `RENDER OK` |
| restore | puts the lamp on 15759 with stable's updater (DOWN, below) | `RESTORE OK` |
| round1 | checks 15860's signed manifest on the Mac and that the lamp runs 15759 with updater b213d9041a88, then round 1 | an `EXIT` line |
| after1 | the after-round checks for 15860 (prev 15759, digest e00e49d4785d), then #8417's BEFORE read | `AFTER PASS\|FINDING`, `PEEK before ...` |
| round2 | checks 15866's manifest and that the lamp runs 15860 with updater e00e49d4785d, then round 2 | an `EXIT` line |
| after2 | the same checks for 15866 (prev 15860, digest = 15866's entry, e00e49d4785d), then #8417's AFTER read and its PASS/FAIL line | `AFTER PASS\|FINDING` |
| core | the three core rows (`gate/core.sh`) | `CORE PASS` |
| putback | back to 15759 (DOWN), with the monitor drop-in as it was before | `PUTBACK OK` |
| release | releases the lock. It needs `PUTBACK OK` (`FORCE=1` releases anyway) | `LOCK RELEASED` |
| summary | the gate line's draft in `gate/logs/summary.txt` | `GATE 15860 on lamp-ee17: PASS\|FAIL` |

## The rounds
Each round is `gate/round.lamp.sh`. It runs the **installed** updater, `/opt/lampOS/update/current/lamp-update`:

`once --channel-url https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice-<N>/ --channel lamp-voice
--key-file <the unit's> --apply on`

It runs with lamp-update.service's own `Environment=` lines, such as `LAMP_UPDATE_SELF=on`, because a self-switch needs
them. The key is the unit's production key (`/etc/lampOS/update/key.pub`). No test key is used.

When the updater changes, the round self-switches and then execs the new build, which ends with `up to date: build N`.
The whole output and the `EXIT` line go to `gate/logs/round<n>.txt`.

Exit 4 means nothing switched: there was no quiet moment, or `serve` held the lock. The round is tried once more after
60 s, with the same T0.

A watcher logs the gateway port's LISTEN and DOWN changes into `~/tgate/gwwatch.<tag>.log` during the round.

## After each round (`gate/after.lamp.sh` reads; `gate/eval.py after` judges)
after.lamp.sh only reads. It runs straight after the round, before anything restarts a unit.

| check | reads | PASS when |
|-------|-------|-----------|
| installed, previous | `/var/lib/lampos-update/state.json` | installed = N, previous = the build before |
| outcome, exit, no-refused | the round's output | an `up to date: build N` line, `EXIT 0`, no `refused` line and no `putting build` line |
| digest | sha256 of `/opt/lampOS/update/current/lamp-update` (link followed), and the release's `lampos-manifest.json` (read on the Mac, decoded from base64) | equal to the manifest's `lamp-update` entry, which starts with the expected prefix |
| gw-inode | `stat -Lc %i /run/lampos-control/control.sock` on the host, and the same path inside the gateway's mount namespace (`nsenter -t <pid> -m`, else `/proc/<pid>/root`) | the same inode. The pid is the first one in the unit's cgroup whose mount namespace is not pid 1's |
| gw-active | ActiveState, and `/proc/net/tcp*` port 18789 in LISTEN | active and listening |
| gw-unit | `systemctl show -p PartOf,TimeoutStopUSec` | PartOf has lamp-control.service, and the timeout is `2min 30s` |
| gw-nokill | the unit's journal since T0 | no KILL, SIGKILL or Killing line, and no systemd stop-timeout line. The gateway's own line "Unable to read systemd stop timeout" does not count |
| gw-exit1 | `status=1/FAILURE` exits since T0, and the watcher's first LISTEN after each one | 0 exits: PASS. 1 exit, listening again within 96 s: **FINDING** (#8429). 1 exit, not listening within 96 s: FAIL. More than 1: FAIL |
| units | `systemctl list-dependencies lampos.target` (lamp units), the gateway and lamp-update, plus `systemctl --failed` | every one active (a oneshot that is inactive with result=success counts), and none failed |

The verdict is `AFTER PASS`, or `AFTER FINDING` when there is no FAIL, or `AFTER FAIL`.

## #8417's read (`gate/peek.lamp.sh`)
**BEFORE** runs in after1, on 15860. **AFTER** runs in after2, on 15866, which carries #8417. Each read does this:
1. Records whether `/etc/systemd/system/lamp-peek.service.d/monitor.conf` existed and whether lamp-peek was active.
2. Writes `[Service] Environment=LAMP_PEEK=monitor` and restarts lamp-peek.
3. Gets a one-time code from `/run/lampos-peek/admin.sock`. Per docs/peek.md, each connection is given one code, 64 hex
   digits and a newline, with no request.
4. POSTs `code=<code>` to `http://127.0.0.1/in` and keeps the `peek_session` cookie on the lamp at 0600. The code and the
   cookie are never printed.
5. Idle minute: `timeout 60 curl -sN -b peek_session=.. http://127.0.0.1/events | wc -c`, and lamp-peek's CPU over the
   same 60 s (pidstat's `%CPU` when present, else the `/proc/<pid>/stat` utime+stime delta / (CLK_TCK*60) * 100).
6. Task minute: the same two numbers, with the TASK clip played 3 s in (the core row's OpenClaw task).
7. Puts the drop-in back exactly (removed, or its old bytes) and puts lamp-peek back in its old state.

Each read prints `PEEK <before|after> idle bytes=.. cpu=.. task bytes=.. cpu=..`.

`gate/logs/peek-8417.txt` says `PEEK 8417 PASS` when, in both minutes, the after CPU is not higher and the after bytes
are lower. That line is #8417's own verdict, not a condition of 15860's gate. `PEEK_FAIL_STOPS=1` makes it stop the run.

## Core rows (`gate/core.sh`, after round 2)
| row | clip | PASS when (the journal lines are quoted as `QUOTE`) |
|-----|------|--------------------------------------------|
| TIME | "Lamp, what time is it?" | `request N heard`, then a `pieces began` line (its answer said) |
| TASK | "Hey Lamp, OpenClaw, look up the weather in Hanoi this weekend." | a handed line (`TASK_HANDED_RE`), then `[lamp-tasks] skill start: run <id>'s script started` (`SKILLSTART_RE`), and no refusal. lamp-control's token and end lines are quoted |
| MAKE | "Hey Lamp, make a skill that tells me a fun fact about the moon when I say moon fact." (a skill ee17 lacks: the days-left ask collided with its own `days-left`) | `a skill made here was kept in N ms`, then `a made skill is on the lamp: <name>`, at once or on the played YES. The skill stays. Its name is in `gate/logs/core-made.name` |

## DOWN: back to stable (restore and putback)
1. It reads the lamp's state. It stops if a round is under way, if a mount over `/opt/lampOS` is on, or if 15860 or
   15866 is in lamp-update's `refused` list. The kit never edits that list.
2. It removes leftover `/run/systemd/system/*.d/zz-row-*.conf` and `zz-gate-*.conf`, such as 7901's
   `lamp-update.service.d/zz-row-7901-hold.conf`, and restarts each unit that had one. Each removal goes into
   `~/changes.txt`.
3. Then it acts on the installed build:
   - **15759:** nothing to do.
   - **Below 15759:** one round to 15759's release.
   - **Above 15759:** lamp-update refuses an older build (`round.rs`: "build N is older than build M"), so the kit
     needs one of two things:
     - `PUTBACK_CMD`: your own put-back. The kit runs it with `PUTBACK_SEQ` and waits for 15759 to be installed.
     - `REWIND_YES=yes`: the owner's yes. The kit stops lamp-update, keeps state.json as it was under `~/tgate`, sets
       `installed.sequence` to 15758 (commit, previous and refused untouched), starts lamp-update, then runs a round to
       15759's release.

     Without either one, it stops.
4. It verifies installed 15759, updater b213d9041a88, and no row or gate drop-in.

## Mock
`mock/run-all.sh` runs every check in this order:
1. `bash -n` and `sh -n` on every script, py_compile, and shellcheck when it is installed.
2. A grep that no script sets a volume, stops lamp-cloud, or resets.
3. The cases:
   - **E.\*:** each after-round check, with a PASS and a FAIL fixture, and the exit-1 FINDING within 96 s against FAIL.
   - **P.\*:** #8417's comparison and its CPU arithmetic.
   - **R.\*:** `run.sh` from lock to summary, in PASS and STOP paths.
