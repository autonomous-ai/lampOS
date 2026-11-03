# How the hardware side works

The production test process for lamps before they ship
([#1700](https://github.com/autonomous-ai/lampOS/issues/1700)), run by the
hardware team with their own agents the way the software side runs with
its builders. Written for the lead on 19 Oct 2026, at the owner's
direction: the hardware lead's agent coordinates hardware builders as the
first builder coordinates software builders, so hardware work never waits
on the first builder.

Everything here that names a limit, a duration or a lamp is a proposal
until the hardware lead approves it. The hardware lead changes this file
as the process settles.

## Who does what

| Who | Does |
|---|---|
| **The hardware lead** (a person) | approves every limit, duration, fixture and stop condition, and the shipment gate; is a repository admin and grants hardware members' access |
| **`hw-first`** (the hardware lead's agent, [builders/hw-first.md](../builders/hw-first.md)) | keeps the Hardware Queue, opens, labels, assigns and closes `team:hardware` issues, approves or declines hardware builders' proposals, finds failures lamps share, posts the hourly line |
| **Hardware builders** (one agent per hardware member, [builders/HARDWARE-TEMPLATE.md](../builders/HARDWARE-TEMPLATE.md)) | run stations, the pilot and stress tests with their person's hands, record every run, report failures, propose fixes |
| **The first builder** | only a finding that needs a software change, posted by `hw-first` as a linked proposal on the software Queue ([#1722](https://github.com/autonomous-ai/lampOS/issues/1722)); it assigns it to a software builder |
| **Software builders** | skip `team:hardware` issues |

## Joining

1. The hardware lead grants the member's own GitHub account access. Never a
   shared login, never another builder's identity.
2. The member's agent copies `builders/HARDWARE-TEMPLATE.md` to
   `builders/<name>.md` and fills it in. No person's name or handle goes in
   it: this repository is public. The account-to-role mapping lives in the
   fleet record.
3. The role file is proposed on the Hardware Queue; `hw-first` approves it,
   and it reaches `lamp-voice` through a pull request like every other
   change.
4. Every session starts with **"you are `<name>`"**, then reads, in order:
   this file, its role file, the Hardware Queue, and the issues labelled
   with its name. [joining.md](joining.md) covers what is the same for every
   builder: branches, pull requests, sharing a lamp.

## The Hardware Queue

One pinned issue labelled `team:hardware`, kept in order by `hw-first`, as
the software Queue is kept by the first builder. No priority labels: the
order is the issue's own list.

- **Labels:** `team:hardware` on every hardware issue, `builder:<name>` for
  whose task it is, `status:taken` once claimed.
- **Claim before work.** Add `status:taken` and one line, `taken by <name>
  · session <id> · <lamp or station>`, before the first run or commit.
- **At most two per builder:** one taken, one next. The rest stay in the
  Queue's order.
- **Untouched for 6 hours means returned** to `hw-first`, who relabels it.
  "queued behind #N" keeps it.
- **Free:** a builder with nothing left posts `free: <name>` on the Queue;
  `hw-first` gives it the next item within the hour.
- **Proposals and faults:** `<name>-P<n>` for a proposal, `Found by <name>`
  for a fault, as a comment on the issue it belongs to, or on the Queue
  when none fits. `hw-first` answers: widens an issue, opens one, or says
  no.
- **Hourly line** from `hw-first` on the Queue while work moves: what ran,
  what failed, what's next, and what needs the hardware lead.
- **Pull requests** carry `builder:<name>` and `team:hardware` from the
  moment they open.

## How a hardware builder works

The hardware lead's direction (24 Oct): hardware builders work the way the
software builders do ([joining.md](joining.md), "The loop"; the software
Queue, #3126). Only what differs for hardware is written here.

1. **Look before taking.** Search open *and merged* pull requests and the
   Hardware Queue for the work first. Then post `taken — <what>` on the
   issue (and add `status:taken`), so no two builders do it twice.
2. **Gates before a pull request is ready.** Each script has a host test
   with stand-ins, so it runs with no lamp and no sound (as
   `tools/routine-tools-test.py` and `tools/routine-run-test.py` do). Run
   it, plus `bash -n` or `python3 -m py_compile`. Write the exact counts in
   the pull request: "15 cases, 15 pass", never "tested".
3. **Read by another builder.** A hardware pull request needs a
   `READ-OK <sha>` from another hardware builder or `hw-first`, after they
   ran its tests. Or a `HOLD <sha>` with numbered findings (P1 blocks; P2
   blocks until answered; P3 is a note). A new push needs a new READ-OK.
4. **Merged by the train, never by hand.** Once it's read, green and
   current on `lamp-voice`, post `NEXT TRAIN: #<PR>` on the software Queue
   (#3126). The train bot carries it with others, runs one ARM64 build and
   merges by rebase. Nothing merges any other way. Rebase your own pull
   request when `lamp-voice` moves: a stale one is not carried.
5. **Sound in the office.** Every lamp in the office hears every other.
   Before any step that makes a sound (the routine runner's prompts, the
   self-test, the spoken exchange), post `PLAYING <lamp> HH:MM–HH:MM` on
   #3126. Before a step that needs silence (a mic check), post `QUIET
   <lamp> HH:MM–HH:MM`. Never in another builder's slot. Never a test tone
   or a raised volume: the lamp's own level, and only the sound the step
   needs.
6. **When your piece waits,** take the next, in this order, without asking:
   findings on your own pull requests; your pull requests rebased; your next
   issue on the Hardware Queue; a stalled hardware pull request (2 hours
   silent), carried on your own branch with its commits kept; a hardware
   pull request with fewer than two readers, read and answered with
   `READ-OK` or `HOLD`.
7. **A result on a lamp** says the commit tested, the lamp, how many times,
   what came out, what failed and what was not shown. The run's records go
   to the fleet record, outside this repository. A public summary keeps no
   person, face, picture, recording or credential, as joining.md's "What a
   result on a lamp is" says.
8. **Hourly line.** Each builder posts one status line on the Hardware
   Queue each hour while it works: what it ran or pushed, and what's next.
   `hw-first` posts the hardware side's line there and on #3126.

## One lamp's run

1. The operator puts the lamp on a station and connects power and network.
   The builder takes the lamp's lock as joining.md's "Sharing a lamp" says:
   one test owns a lamp at a time.
2. The lamp runs its own `lamp-selftest` (about 2 minutes): microphones,
   speaker, the five joints, ring, camera, touch, sensors, board, storage.
3. The operator does the manual steps the coverage matrix lists: looking
   at the ring, listening to the speaker, touching every pad, pressing the
   buttons. The head's touch pads are wired per unit, on any of lines 37,
   96, 97 and 98 (lamp-52e6's on 97 and 98, lamp-dbda's on 37, 96 and 97;
   #1362, #2182). The product reads all four until the station finds this
   unit's pads (two to four) and their polarity, with a person holding each
   pad in turn, and writes them into the unit's pins file (#1744's
   `steps.sh`, #1798).
4. `lamp-station` turns the self-test report and the manual steps into one
   run record against the matrix
   ([#1741](https://github.com/autonomous-ai/lampOS/pull/1741),
   `production/matrix-v<N>.toml`). A missing, skipped, interrupted or
   `not_checked` check makes the run `incomplete`, never a pass. A question
   the operator didn't answer is not a yes.
5. The record goes into the fleet record, passes included: device id,
   hardware revision and batch, lampOS commit, suite version, station,
   every check's verdict and measurement, manual steps with times, repairs,
   retests.
6. A failure is reported on its `team:hardware` issue with the run linked.
   A failure two or more lamps share gets one issue from `hw-first`, with
   every affected lamp listed. The builder and its person repair, then run
   the failed checks and the final checks again.

Audio checks on two lamps at once are scheduled or isolated, so a lamp
never hears another lamp's tone.

## A new lamp at the factory

Every lamp off the line goes through the same steps, from one Mac at the
bench ([the hardware test README](../../robots/lamp/hardware/test/README.md),
"Every new lamp"): flash the ship image, the camera in port 3-1.1.4,
`factory-lamp.sh start`, the app setup, `RESET=yes factory-lamp.sh
selftest`, `collect`, pack.

**The servos come first.** A lamp whose servos never ran V1 HAL has
neither of V1's servo settings (lamp-220c, 28 Oct): a maximum acceleration
of 50, which the body's stand refuses, and no zero or limits of its own,
so the head reads off its stop and lamp-4ace's stand drove lamp-220c's
elbow into its body. `start` checks each lamp's servos before its run,
stores V1's settings (nothing moves), and calibrates a lamp never
calibrated with its person: each joint moved end to end by hand, the plan
read, then written. A lamp left uncalibrated is skipped, never run. A
servo replaced or a horn remounted is calibrated again.

## Stress tests (qualification)

Stress tests can wear or damage a unit, so they run on a few qualification
units per hardware revision and batch, never as part of a routine run.
Each one starts with its person's yes at the lamp and the operator's stop
in reach; the body moves only with someone at the arm. A stop ends the run
at once and records `stopped`, never a pass.

| Test | What runs | Proposed stop / pass | Matrix id |
|---|---|---|---|
| Servo endurance | the arm's normal moves in a loop, 30 min, within the joint limits | stops at the body's own bounds (`docs/body.md`): a joint's temperature at 62 °C, or load over pitch 240, roll 150 or yaw 220 for six steps in a row; passes under 58 °C with no temperature or load stop in the 30 minutes, set from a known-good unit's run first | `servo_endurance` |
| Give way | a person pushes the arm while it holds and while it moves, 20 times | lets go every time | `servo_give_way` |
| Power under peak | all servos at peak together, speaker loud, ring full | supply never below the board's brown-out level (the hardware lead names it) | `power_servo_peak` |
| Power cut | cut and back, 20 times, at idle and mid-move | back by itself every time, nothing left muted, arm safe; the self-test's storage check after every cut, stopping on the first SD-card I/O or ext4 error, since repeated cuts can corrupt a card | `recovery_power_cut` |
| Board heat | fixed 10-minute CPU load, both fans | under 85 °C | `board_load_temperature` |
| Memory and storage | a RAM test over the memory it can take, and an SD write and read | no error | `storage_write_read`, and a new id for memory |
| Audio duplex | speech on the speaker while both microphones listen, 10 min; servos moving while mic2 listens | no dropped blocks; noise under a limit the hardware lead sets | `interference_speaker_room_mic`, `interference_servos_mic` |
| Network | Wi-Fi lost and back, the relay unreachable and back, 10 times each | recovers by itself every time | `recovery_wifi_lost` |
| Part restart | each part restarted alone | the others keep working | `recovery_part_restart` |
| Air sensor | 10 minutes of reads | failed reads under a limit the hardware lead sets | `air_read_failures` |
| Long soak | everything above at a gentle pace, 8 hours | no fault, no unplanned restart | a new id |

**Order on a new revision:** the routine run first (it must pass), then
give way, then power, heat, memory and storage, audio, network and part
restart, and the long soak last. A test with a new id joins the matrix in
its next version, with the hardware lead's yes.

## When software is needed

A finding that needs a code change is never fixed by a hardware builder in
`lamp-voice`. `hw-first` posts it as a linked proposal on the software
Queue (#1722), with the runs that show it; the first builder assigns it.
The hardware issue stays open until the fix is on the lamps and the failed
checks pass again.

## The shipment gate

A lamp ships when every mandatory check of its revision has passed in one
recorded run, no `team:hardware` blocker is open against it, and the
person the hardware lead names has approved it. A lamp repaired after a
run ships only after the failed checks and the final checks pass again.

## Starting it

The first items on the Hardware Queue, in order:

1. **Fill in the matrix's blanks** in #1741: revision names, mandatory
   checks, every limit, device id, batch and operator id forms, where the
   fleet record lives, who approves shipment.
2. **Approve or change the stress tests** above.
3. **Port the hardware lead's bench scripts to V2** (a full component test
   and a burn-in). They were written for V1's runtime. On a V2 lamp they
   read through the lamp's own parts instead of stopping a service, never
   count an unanswered question as a pass, and never write one lamp's servo
   calibration into another: calibration is per lamp, in the servos' own
   memory. A lamp's own comes only from its own hand sweep
   (`servo-calib.py`, below).
4. **The pilot:** 3–5 lamps by key, known-good ones and ones with a known
   fault, judged on whether faults are found and false failures avoided.
5. **Hardware members' builders join**, and `hw-first` hands them the
   pilot. Then a staged rollout to every lamp before shipment.
