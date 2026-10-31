# Production testing: the coverage matrix and the run record

Deliverables 1 and 4 of [#1700](https://github.com/autonomous-ai/lampOS/issues/1700),
drafted by the first builder from what the software reports today:

1. **The coverage matrix:** every check a production run may hold, which
   are automated and which manual, which are routine and which are
   qualification tests that could wear or damage a unit, and what stays
   untested. In the repository as `production/matrix-v0.toml`.
2. **The run record (fleet evidence):** one JSON record for every test
   station run, passes included, made by the station tool `lamp-station`
   (`crates/station`), which also decides the run's verdict.

**Every limit, duration, fixture, revision name and "mandatory" below, and in
the matrix file, is a proposal awaiting the hardware lead.** The hardware
lead approves the limits and the shipment gate (deliverables 2 and 6). Until
then the matrix says `approved = false`, and every record made with it says
`"matrix_approved": false`, so no record of version 0 can be read as a
shipment pass.

Nothing here runs on a lamp. The tool runs on the station, reads files, and
writes one; it opens no device and sends nothing anywhere.

## The coverage matrix

Carried over from the first draft on #1700 (19 Oct, 08:4x). It is built from
what `lamp-selftest` checks ([selftest.md](selftest.md), "How each part is
checked"), so a station and the lamp's own check can't disagree. The last
column is the checks' ids in `production/matrix-v0.toml`.

| Part | Production check (automated unless said) | From the self-test today | Missing for production | Kind | Check ids |
|---|---|---|---|---|---|
| Assembly | (deliverable 1 lists it; not in the 08:4x draft) the operator looks the lamp over | no | the list of what to look at | routine, manual | `assembly_inspection` |
| Voice microphone (Jieli, mic2) | 1 s of `mic2/audio` above −90 dBFS, blocks lost counted. **Add:** a played tone at a fixed level, read back within ±X dB, and its noise floor | level and losses only | a reference tone and fixture, the dB window, the noise floor | routine | `voice_mic`, `voice_mic_tone` |
| Room microphone (C-Media, mic1) | `ears/level` above −90 dBFS. **Add:** the same tone | level only | tone, window | routine | `room_mic`, `room_mic_tone` |
| Speaker | from the lamp's own last speaking turn of ≥ 4 s, at the factory and on demand | yes, at the factory | a level measured by a reference mic at a fixed distance; buzz and rattle (a person listens, or a sweep) | routine, partly manual | `speaker`, `speaker_level`, `speaker_buzz` |
| Servos 1–5 | `body/joint_states` fresh for all five; no warm joint and no resistance warning. `head_move` (pitch) at the factory | yes | each joint's full range within its limits; load at rest under X; temperature after N minutes of moves; that it gives way to a hand (#780) | routine + **qualification** (endurance) | `base_yaw`, `shoulder`, `elbow`, `head_roll`, `head_pitch`, `head_move`, `servo_range`, `servo_give_way`; qualification `servo_endurance` |
| Ring (32 pixels) | each of 10 colours at level 3, seen on `light/drawn` | yes, at the factory | dead or stuck pixels: a camera or a person looks | routine, partly manual | `ring`, `ring_pixels` |
| Camera | `humans/faces/tracked` within 2 s; health has no warning | yes | focus and exposure on a test card at desk distance; a face found at 40–80 cm (lamp-52e6's eyes trial on 19 Oct is why) | routine, partly manual | `camera`, `camera_test_card`, `camera_face_distance` |
| Head pads, button | ready, no fault, nothing held ≥ 10 s; `touch_by_hand` at the factory (head, press) | yes, at the factory | nothing for the base's touch strip: it's off in V2, not used at the project lead's decision (19 Oct; [controls.md](controls.md), "The base strip is off"), so the draft's strip calibration (#1682) and its electrodes are out | routine, manual | `touch`, `touch_by_hand` |
| Privacy switch | (not in the self-test) | no | the switch closes both microphones and the camera: a manual step | routine, manual | `privacy_switch` |
| Air (SEN63C) | `air/state` fresh (15 s) and in range | yes | CO2 and PM2.5 against a reference, or a sealed-box test; a failed-read rate over 10 min | routine + qualification | `air`, `air_reference`; qualification `air_read_failures` |
| Board | hottest < 85 °C, ≥ 3% disk free | yes | temperature under a fixed 10-minute load with both fans (17 Oct: 87 °C before the second fan) | routine | `board`, `board_load_temperature` |
| Storage (SD) | no SD I/O error in the kernel log since boot | yes | a write and read check; card make and size recorded | routine | `storage`, `storage_write_read` |
| Power | (not in the self-test) | no | the supply's voltage under the servos' peak; behaviour at a cut (the voice comes back unmuted, #784) | qualification | `power_servo_peak`, `recovery_power_cut` |
| Interference | (not in the self-test) | no | all parts at once: servos moving while the microphones listen (noise in mic2), the speaker playing while the room mic listens | integrated | `interference_servos_mic`, `interference_speaker_room_mic` |
| Recovery | (not in the self-test) | no | approved scenarios only: power cut, Wi-Fi lost, a part restarted alone | qualification | `recovery_power_cut`, `recovery_wifi_lost`, `recovery_part_restart` |
| Final acceptance | the self-test passes with no `not_checked` | — | + one spoken exchange: "Hey Lamp, what time is it?", answered | routine | every self-test check, `spoken_exchange` |

**Rules** (from the draft):

- Missing, skipped or `not_checked` is never a pass. Qualification checks
  that could wear or damage a unit stay out of the routine run.
- What each run records is the record below.
- **Untested even then:** long-term wear past the endurance window, and
  faults that need opening the unit.

The pilot (3–5 lamps, known good and known faulty, deliverable 7) shows
which limits catch real faults and which pass bad units.

### The matrix file

`production/matrix-v0.toml` is the matrix as data. It is versioned: a changed
limit, duration or "mandatory" is a new file (`matrix-v1.toml`, …) with a new
`version`, the hardware lead's decision, never an edit made to pass a unit.
Each record names the version and the file's SHA-256, so an edited file that
kept its version shows.

- `[suite]`: `version`, and `approved` (false until the hardware lead
  approves this version).
- `[rules]`: `selftest_without_lamp`, what a run is whose self-test report
  does not name its lamp (below, "Whose report it is"): `incomplete` (the
  default, and version 0's) or `refused`. The hardware lead's to set.

**Version 1** (`production/matrix-v1.toml`, 21 Oct) is version 0 with `approved = true`, every check, limit and rule unchanged, for the pilot (batch `pilot-1`). The lead gave the three decisions to the hardware team on 21 Oct (#1764): the limits as version 0 proposed them unless the hardware team objects; the pilot lamps theirs to pick, not the office lamps, which have been taken apart and run hard; and the records kept in their own private place. A limit the pilot shows wrong is version 2.
- `[[revision]]`: the hardware revisions the matrix knows, each with an
  `id` and a line saying `what` it has. Version 0's two are placeholders the
  hardware lead names: `draft-a` (every part fitted, the air sensor among
  them, as lamp-4ace is built) and `draft-a-no-air` (no air sensor, as
  lamp-06c1 and lamp-cb94 are).
- `[[check]]`, one per check:

| Field | Means |
|---|---|
| `id` | its name in records; a self-test check's is the part's name in the lamp's report (`crates/msgs/src/selftest.rs`) |
| `part` | the part, for a person |
| `kind` | `routine` (every lamp, every run), `qualification` (a few units; can wear or damage one; never in a routine run) or `integrated` (all parts at once, in the routine run) |
| `method` | `selftest` (automated, from the lamp's own report), `station` (automated, by the station's fixture) or `manual` (a person at the station) |
| `mandatory` | missing, skipped, interrupted or not checked makes a run of its kind `incomplete` |
| `not_fitted_allowed` | the revisions on which `not_fitted` is fine (default none) |
| `limit` | for a station or manual check that gives a measurement: `min`, `max` or both, and the `unit`. A self-test check's limits are the self-test's own |
| `what` | what is checked, in one line |

An unknown field, kind or method, a check named twice, a revision a check
names but the matrix does not, or a limit with min above max is refused.

**Mandatory in version 0 (proposed):** every self-test part, and the manual
steps that need no fixture: `assembly_inspection`, `speaker_buzz`,
`ring_pixels`, `privacy_switch`, `spoken_exchange`;
in a qualification run, every qualification check. The station checks that
need a fixture nobody has built yet (the tones, the speaker's level, the
test card, the servos' range, the air reference, the board under load, the
SD write), the two interference checks and two manual steps not yet settled
(`servo_give_way`, `camera_face_distance`) are recorded when given but not
mandatory, so a version 0 run says what it could check. Only
`board_load_temperature` has a limit in version 0 (at most 85 °C, the
self-test's own hottest); the others' windows are still X in the draft.
Only the air checks may be `not_fitted`, and only on `draft-a-no-air`.

## The run record

### What the station gives

- **The lamp's self-test report**, `/var/lib/lampOS/selftest/latest.json`,
  copied off the lamp by the station after a factory run
  (`lamp-selftest --occasion factory`, which the factory step runs with no
  `--dir`, so its report is in the tool's own folder;
  `/var/lib/lamp-selftest` is the first-boot unit's, and holds first-boot
  reports): only that occasion asks for the touch by hand; the head's move,
  the ring's colours and the speaker are asked for at the factory and on
  demand, never at first boot, and the speaker is checked only after the
  lamp has spoken for 4 s or more ([selftest.md](selftest.md)). A first-boot
  report leaves those `not_checked` or missing, and an on-demand one has no
  touch by hand, so either run is `incomplete`. Read strictly: a field
  `lamp_msgs::SelfTest` does not have is refused, and so is a report that
  fails its own check (a verdict that is not the worst of its parts, a fail
  with no reason). The report names the lamp that ran it (`lamp`, below).
- **The station's answers**, one JSON file for the run:

```json
{
  "started": "2026-10-19T08:00:00Z",
  "ended": "2026-10-19T08:30:00Z",
  "steps": [
    {"check": "privacy_switch", "verdict": "pass", "at": "2026-10-19T08:10:00Z"},
    {"check": "ring_pixels", "verdict": "fail", "reason": "pixel 12 stays dark",
     "at": "2026-10-19T08:12:00Z"},
    {"check": "board_load_temperature", "verdict": "pass",
     "measurement": {"value": 71.5, "unit": "°C"}, "at": "2026-10-19T08:25:00Z"}
  ],
  "repairs": []
}
```

  `started` and `ended` are the run's, by the station's clock. Each step is
  a station or manual check: its `check` id, its `verdict` (`pass`, `fail`,
  `not_fitted`, `not_checked`, `skipped` or `interrupted`), its
  `measurement` if any (`value` and `unit`), a one-line `reason` (needed for
  anything but a pass, at most 200 bytes) and its UTC time `at`. A retest's
  `repairs` say what was done since the run it retests: each `at`, the
  `check` whose failure it answers, the `action` (`replaced`, `reseated`,
  `reworked` or `adjusted`) and a one-line `detail`.
- **The lamp:** `--device` (its host name, such as `lamp-4ace`: the name its
  self-test report gives), `--revision` (one the matrix names) and
  `--batch`.
- **What judged it:** `--lampos-build` (the lampOS commit the lamp runs),
  `--suite <matrix version>+<tool commit>` (the station tool's commit; the
  version must be the matrix file's own) and `--station` (`station-<id>`).
- **The operator:** `--operator`, an opaque id local to the station,
  `op-` and 1 to 12 digits, never a name. Which person an id is stays with
  the station, never in a record.
- **For a retest**, `--retest-of`, the record of the run it retests.

Times are UTC, written `2026-10-19T08:00:00Z`; any other offset is refused.

### Whose report it is

A self-test report names the lamp that ran it: `lamp`, its host name as the
kernel has it, the name `lamp-cloud`'s `info` answers as `hostname`
([selftest.md](selftest.md), "What a run gives"). The station's `--device`
is that name.

- A report that names **another lamp** is refused, and no record is made: it
  was copied off the wrong lamp.
- A report that names **no lamp** is from a build before reports named
  theirs, and cannot be matched to `--device`. By the matrix's
  `selftest_without_lamp`, the run is `incomplete` (the default: the record
  is made, with `selftest` first in `incomplete`, `not_checked`, saying so)
  or refused. Its parts are judged as given either way, so a fail in it
  still fails the run.

### The command

```sh
lamp-station --matrix production/matrix-v1.toml --answers answers.json \
  --selftest latest.json --device lamp-4ace --revision draft-a --batch pilot-1 \
  --lampos-build dab3f1242486 --suite 1+dab3f1242486 --station station-01 \
  --operator op-7 --occasion routine --out records/
```

`--occasion` is `routine`, `retest` (with `--retest-of`) or
`qualification`. It prints each failure and each unfinished check, then
`run <run id> <verdict>`, and keeps the record in `--out`. With
`--records PLACE` (off unless given; #2794) it also keeps it in the
records' place, in the lamp's own folder, `PLACE/<device id>/<run id>.json`
(the `--device` a self-test report naming its lamp must match, below),
written whole and never over another record, as in `--out`.

Exit status: 0 the run passed, 1 it failed, 3 it is incomplete, 2 no record
was made (an argument or an input was refused, or the record could not be
written). With `--records`, a device id that can't be a folder of its own
there is refused before anything is written, `--out` included, and the
refusal says why (El on #3091): one starting with a dot, such as `..`, one
with an upper-case letter, since a share may not tell `Lamp-4ACE` from
`lamp-4ace`, or one lamps share (`autonomous`, `orangepi`, `lamp-`; below),
for which it says to give the lamp its own, such as
`lamp-<the last four of its serial>`.

A record kept in `--out` but refused by the place also exits 2:
- one already there: nothing to copy;
- a lamp folder that is a link to elsewhere, which is never followed: no
  copy is offered either;
- a place that can't make hard links: it says so with the `--out` path to
  copy there by hand once the place can take it, since the same run can't
  be made again. A record is put in place by a hard
  link from a part file, so it's never half there and never replaces
  another, and many shares (SMB, most FUSE and NAS mounts) don't make them.
  The place must.

More than one station may write to the place at once. Each writer's part
file is its own, made new, so a second station keeping the same run never
changes the record the first kept (reviewer-two on #3054).

What the place holds and who reads it: every field of each record, the
one-line `reason`s and repair details among them, which the operator types
and which could hold a person's name if one is typed there. Nothing heard,
no face, still or key is in a record. Files and folders take the station's
umask (`0644` and `0755` by default), so whoever can read the place reads
every record. Nothing is ever removed: records stay until someone with the
right to the place removes them.

### One lamp's records

```sh
lamp-station records --records PLACE --device lamp-4ace [--matrix production/matrix-v1.toml]
```

lists the lamp's records in the place, oldest first (a run id begins with
its start): `<run id> <verdict> matrix v<version> tool <commit>`. A file in
the lamp's folder that isn't a record of that lamp under its own run's name
is listed as `not a record of <lamp>` with why, never left out. With
`--matrix`, each record is also judged for completeness against that matrix
version (#2795): `complete: <verdict>`, or `not complete:` and each gap, so
a run with a mandatory check left out never reads as passed. A lamp stands
by its latest run: a failed run retested and passed stands passed, and every
run is still listed. Exit status: 0 the latest run passed (and is complete,
with `--matrix`) and every file in the lamp's folder is its record, 1 not, 3
the lamp has no records there, 2 an argument was refused or the place could
not be read.

### Every lamp's records against one matrix

```sh
lamp-station complete --records PLACE --matrix production/matrix-v1.toml [--device lamp-4ace]...
```

asks the pilot's question (#2795): is every lamp's record complete against
that matrix version? It takes each lamp's folder in the place, or only the
lamps named with `--device`, and judges each lamp's latest routine or retest
record against the matrix, judged as `records --matrix` judges one. Which
run it takes can differ from `records`, which counts every run and takes the
last file by name: here the latest is the one with the latest `started`,
then `ended`, then run id, the record's own start by the station's clock,
never a file's name or modified time, which a copy to a share or a restore
changes, and a qualification run is passed over. It prints one line per lamp:
`<lamp> complete: run <run id> <verdict>`, or `<lamp> not complete: run
<run id> <verdict>; gaps:` and each gap's kind and check only (as `missing
privacy_switch`), the verdict being the one the run's checks give. A lamp
with no such record is `<lamp> no record`. A file in a lamp's folder that
isn't its record is named, and that lamp is not complete, since the file may
be its latest run; an entry in the place that isn't a lamp's folder is
listed and not judged. A last line says how many of the lamps are complete.
It reads the place and changes nothing. Exit status: 0 every lamp is
complete, 1 one is not or has no record, 3 there is no lamp, 2 an argument
was refused or the place or the matrix could not be read.

### The record, field by field

`<out>/<run id>.json`:

| Field | Holds |
|---|---|
| `format` | the record's format, 1 |
| `run_id` | `<started>-<station>-<device>`, as `20261019T080000Z-station-01-lamp-4ace` |
| `occasion` | `routine`, `retest` or `qualification` |
| `verdict` | the run's: `pass`, `fail` or `incomplete` (below) |
| `started`, `ended` | UTC, from the answers |
| `device` | `id`, `hardware_revision`, `batch` |
| `lampos_build` | the lampOS commit the lamp runs |
| `suite` | `matrix_version`, `matrix_sha256`, `matrix_approved`, `tool_commit` |
| `station` | `station-<id>` |
| `operator` | `op-<digits>` |
| `retest_of` | a retest's: the run id it retests (absent otherwise) |
| `repairs` | a retest's repairs, as given (absent when none) |
| `selftest` | the lamp's report the run used: its `occasion`, `started` (the lamp's clock, UTC), `took`, `verdict`, `build` and `lamp` (absent when none was given) |
| `checks` | every check of the run, in the matrix's order (below) |
| `failures` | each check that failed: `check`, `reason`, and `measured` and `limit` where there are |
| `incomplete` | each mandatory check that did not finish: `check`, its `verdict` and `reason`; first, `selftest` when the report names no lamp |

Each of `checks`:

| Field | Holds |
|---|---|
| `id`, `part`, `kind`, `method`, `mandatory` | from the matrix, so a record reads without it |
| `verdict` | as judged: `pass`, `fail`, `not_fitted`, `not_checked`, `skipped`, `interrupted`, or `missing` when neither the self-test nor the station gave it |
| `said` | what the lamp or the station said, when the judged verdict differs: a `pass` outside its limit, or `not_fitted` on a revision that must have the part, is `fail` with `said` beside it |
| `reason` | the self-test's or the station's one line |
| `values` | the self-test's readings for its part (`key`, `value`) |
| `measurement`, `limit` | a station or manual check's measured value and the matrix's limit |
| `at` | when a station or manual step was done |

A routine run or a retest lists every routine and integrated check, given or
missing. A qualification run lists every qualification check, and the
routine ones it was given.

### The verdict

In this order:

1. **Refused, nothing recorded:** an unknown field anywhere in the answers,
   the self-test report, the matrix or an earlier record; a check id the
   matrix does not name; a self-test part given as a station step, or a
   station check in the self-test report; a check given twice; **a
   qualification check in a routine run or a retest**; a revision the matrix
   does not name; a suite version that is not the matrix's; an operator id
   that is not `op-<digits>`; a step outside the run's times, a run that ends
   before it starts or more than 5 minutes after now, or a self-test report that started more
   than 5 minutes outside the run (an old `latest.json`; the slack is for a
   lamp's clock, which has no battery); a self-test report that names
   another lamp than `--device`, or names none where the matrix's
   `selftest_without_lamp` is `refused`; a non-pass step with no reason; a
   pass with no measurement on a check with a limit, or a measurement in
   another unit. For a retest: no earlier record, or one of another lamp,
   one that passed, a qualification run, or one that ended after this run
   began; a repair outside the time between the two. Repairs and an earlier
   run belong to a retest only.
2. **`fail`** if any check failed: one the lamp or the station said failed
   (mandatory or not), a measurement outside its limit, or `not_fitted` where
   the matrix does not allow it for the lamp's revision.
3. **`incomplete`** otherwise, if any mandatory check of the run's kind is
   missing, skipped, interrupted or `not_checked`, or the self-test report
   names no lamp (where the matrix does not refuse it). Never `pass`.
4. **`pass`** otherwise, if at least one check passed. A run that would pass
   with no check passed (a matrix that asks nothing of it) is refused.

A failed run still lists what did not finish, in `incomplete`. A retest is a
whole routine run again, not only the checks that failed.

### Written whole

The record is written to a part file of this writer's own
(`.<run id>.<process>-<nanoseconds>-<count>.part`), synced, hard-linked to
`<run id>.json` and the part removed (`store.rs`), so a reader or a power
cut never finds half a record. A record already there is never written over:
the tool refuses and makes nothing. A run made again is a retest.

## Never in a record

- credentials or keys: the voice's key, the update key, Wi-Fi or account
  passwords;
- audio and pictures: no microphone recording, no camera frame, no
  recording of the spoken exchange (its step is a verdict and a time);
- Wi-Fi names or passwords;
- a person's name or handle: the operator is `op-<digits>`, and reasons and
  repair details describe the lamp, never who did what;
- sensitive manufacturing data, such as supplier prices or supplier
  contracts.

The tool keeps out what it can: every input is read strictly, so a field
the format does not have (an `operator_name`, a `cost`) is refused, and the
only free text is the one-line reasons and repair details, which the
operator writes about the lamp. It cannot tell a name inside such a line
from a fault; the operator's workflow (deliverable 3) has to.

## Where records are kept

**Outside this public repository.** Where the fleet record lives is still
open with the hardware lead (#1700, 19 Oct 08:55, "Still needed from the
hardware lead"), so no destination is chosen here. The tool writes to the
station's own `--out` folder and, when it is given one, to the records'
place (`--records`, a folder: a share the hardware lead chooses can be
mounted there), and sends nothing anywhere. Records are
never committed to this repository; the tests' inputs are made up.

## Open for the hardware lead

- Every limit, duration and fixture, and which checks are mandatory.
- The revisions' names (`draft-a` and `draft-a-no-air` are placeholders),
  and which parts each revision may lack.
- What the device id is, and how batches are named. Here it is the lamp's
  host name, which its self-test report gives: `lamp-` and only the last
  four hex digits of its serial or Ethernet address, so among 100 lamps two
  may share one (about a 7% chance). Whether the record needs a longer id,
  such as the whole serial, is open.
- Whether a self-test report that names no lamp makes a run `incomplete`
  (version 0) or is refused (`selftest_without_lamp`).
- The operator id's form (`op-<digits>` here).
- The 5 minutes allowed between the lamp's clock and the station's.
- Proposed here: `not_fitted` on a revision that must have the part is a
  fail, not incomplete; integrated checks belong to the routine run; a
  qualification run is not retested, only run again.
- Where the SD card's make and size are recorded.
- Where the fleet record lives.

A lamp whose hotspot never came up keeps a host name it shares with other
lamps: the image's `autonomous`, a stock board's `orangepi`, or `lamp-` with
nothing after it when no serial could be read. Such a name is no lamp's own
(`lamp_msgs::SelfTest::is_own_name`: a `-` and a suffix after it): the
self-test leaves it out of its report, and the station treats a report naming
it as one that names no lamp, by `selftest_without_lamp` (El on #3073). So two
such lamps never match each other's reports. `--records` refuses such a name
as `--device` too, so their records never share a folder: give a lamp with no
own name a label with a suffix of its own, such as its sticker's
`lamp-<last four of its serial>`.

A lamp on a build from before reports named their lamp is recorded, but under
version 0's `incomplete` it never reaches `pass` until it updates. Update the
station tool before the lamp image: a station tool from before this change
reads its input strictly and refuses every report that has `lamp`.

## What was not shown

- Nothing ran on a lamp or a station.
- `cargo test -p lamp-station` on a Mac, every verdict rule and refusal
  mutation-checked by hand (each broken in turn, a test failing, then
  restored), and one run of the binary on made-up inputs.
