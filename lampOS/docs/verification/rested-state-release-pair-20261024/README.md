# Published Skills rested-night continuity

Part of #2809, continuing the separate
[daily/on/off fixture](../routine-state-release-pair-20261024/README.md).
This record covers two additional actual-release cases only. Both executed
successfully on native ARM64 at corrected head `8bc821736`, alongside the four
parent cases. The exact-head results are recorded below; a later carry still
requires its own gates. The parent record's exclusion of rested-night coverage
remains correct for that separate four-case fixture.

## Exact cases and artifacts

| Case | Process order |
|---|---|
| `release_6498_reads_6408_rested_night_and_6408_keeps_its_morning_spent` | 6408 → 6498 → 6408 |
| `release_6408_reads_6498_rested_night_and_6498_keeps_its_morning_spent` | 6498 → 6408 → 6498 |

The driver is in
[`release_pair/rested.rs`](../../../crates/skills/tests/release_pair/rested.rs),
using the process/artifact helpers in
[`fixture.rs`](../../../crates/skills/tests/release_pair/fixture.rs).
The persisted-state readers and writers are the released `lamp-skills`
executables, never today's Skills library standing in for an old reader.

| Release | Source | Bytes | SHA-256 |
|---|---|---:|---|
| 6408 | `ee53765c8d014eb00e41c1a5a7054d2546c18bdb` | 8,807,104 | `2c6bc8f34c9fd4a56784a54fa2d03b1e76be9d1833ccf37db905f88dba67d3d5` |
| 6498 | `846b3d203fc00ec3886d71f7673d8cc29cc4a634` | 8,786,048 | `87e500127793db88df8bcd7af793879faeee95f36a8e0d2ef8483c59fef26984` |

The existing [artifact identity record](../routine-state-release-pair-20261024/artifacts.json)
retains public asset metadata and decoded-manifest checks. Those checks
establish artifact consistency, not an independently trusted signing key.
The fixture checks size, digest and AArch64 ELF identity, copies executables
privately with mode 0555, verifies the running `/proc/<pid>/exe` bytes, and
checks copied bytes and modes again after the case. Both released sources
share the relevant daily/routine persistence code; the executable digests
differ. This is a fixed published-pair comparison, not arbitrary future-schema
qualification.

## Clock and immutable inputs

One synthetic core skill, `rested-check`, contains named night, morning and
Cancel probe triggers. Each step's `on = trigger` selects its operation.
`skill.toml` and `SKILL.md` are frozen once, then byte-compared and hashed
across all three processes. The driver never replaces live state or skill
definitions to manufacture compatibility.

The fixture derives its offsets jointly from actual wall time: A is near
23:30 on local day D, and B plus returning A are near 00:30 on D+1. Both
offsets must be within the CLI's permitted range and must map to those
consecutive dates. Each phase checks its date and a ten-minute margin around
the selected time. The windows remain unchanged:

| Purpose | Local window |
|---|---|
| Night daily trigger | 23:00–00:00 |
| Owned rest lease | 23:00–01:00 |
| Morning daily trigger | 00:00–01:00 |

Only the launched process's configured offset changes. This is a synthetic
consecutive-day mapping, not an elapsed overnight trial, host-clock change,
timezone migration, DST test or clock-synchronization result.
`{"skip_us":0}` drains preceding bus inputs; it does not advance wall time
or establish that the later routine tick accepted a receipt.

## Required sequence in each direction

1. **A writes an admitted night.** Two separate admissions first receive a
   stale receipt and a fresh wrong-episode receipt. Each must end in a real
   refused Done and Cancel, leaving `rested` absent. A third admission gets
   an exact skill, Skills session, Expression session, episode, operation
   and sequence match. Successful Done, the exact `rested-check D` line and
   a real same-episode renewal must all precede SIGKILL and confirmed reap.
   Raw daily bitmaps must contain exactly the three night-trigger claims.
2. **B reads history without restoring ownership.** The private bus and fake
   Expression publisher survive the process replacement and keep the old A
   receipt fresh. B must report the exact stored night and successfully
   complete a non-daily Cancel probe without publishing a rest request.
   Its morning Wake then sends exactly one `{"gesture":"stretching"}`
   datagram to the private mood socket. Only after receipt of that datagram
   does the fake Body peer publish a matching `body/gesture` Over. The real
   run must finish, the morning daily claim must be present, and `rested`
   must retain its original bytes before the next SIGKILL/reap.
3. **Returning A preserves the spent morning.** With the same morning offset,
   manifest and state, it must explicitly diagnose the consumed day, emit
   neither a stretch nor a rest request, and preserve exact state bytes.
   A neutral phase must produce exactly the named daily watch's negative
   reset, with a numeric elapsed duration. A Cancel probe with stale
   Expression readiness must refuse; a fresh owner must then permit another
   Cancel probe to finish without a rest request. This positive barrier
   prevents a stopped worker or unavailable owner from passing as suppression.

Every rest request is checked against its actual bus source, publisher boot,
sequence, timestamp and the fake Expression owner's session. The suite keeps
exact terminal-event ordering rather than filtering unexpected events. State
snapshots must contain `daily.json` and `rested` with no on/off companions;
manifests must remain unchanged. Raw daily schema-2 bitmaps are inspected
without calling today's Journal reader. No actual schema-1 writer is exercised.

## Private boundaries and execution

Each daemon receives only recorded fixture environment variables, including
`LAMP_SKILL_ROUTINES=on`, `LAMP_SKILLS_ASK=skip` and its private daily path.
Fresh synthetic Voice resting and Expression state/receipt messages use the
real local bus transport. No Voice, Expression, Body, Actions or provider
process runs. There is no account, Store activation, live skill, device,
microphone, speaker, audio capture or playback. Actions, Say and gaze sockets
are private traps; the single morning stretch is the only permitted mood
datagram. The fake Over proves sequencing, never physical movement.

Cases use unique retained private `kn-*` directories, with root and state
mode 0700. The overall bound is 90 seconds per case, each progress/readiness
phase at most 15 seconds, each stream operation two seconds, and failure
cleanup five seconds. Children are owned by cleanup before executable
inspection; inter-process cuts require real SIGKILL and reap. These cuts occur
after completed writes, not inside rename/fsync, and do not qualify power-loss
durability, updater installation or reboot behavior.

To select only the two new cases, run from `lampOS/`:

```sh
LAMP_COMPAT_SKILLS_6408=/artifacts/6408/lamp-skills \
LAMP_COMPAT_SKILLS_6498=/artifacts/6498/lamp-skills \
LAMP_COMPAT_OUTPUT=/tmp/pair-output \
cargo test --locked --release -p lamp-skills --test release_pair rested_night -- --ignored --nocapture --test-threads=1
```

The output parent must already exist with mode 0700 and a short absolute path.
Execution requires isolated native Linux ARM64. The source also typechecks on
macOS; it does not hide the driver behind a Linux-only compilation condition.

## Gate record and remaining evidence

| Gate | Result |
|---|---|
| Mac formatting | Passed |
| Mac strict Skills Clippy, all targets | Passed |
| Mac ordinary Skills tests | 514 passed / 0 failed / 7 ignored; no filters |
| Corrected native six-case run at `8bc821736` | 6 passed / 0 failed / 0 ignored, including both rested directions |
| Ordinary ARM64 at `8bc821736` | 11,163 PASS + 1 LEAK; 0 failed, 77 skipped |
| Linux strict Clippy at `8bc821736` | Passed |
| Later carried-head gates | Required after the parent lands and this continuation rebases |

The seven ordinary-suite ignores include all six release-pair cases and one
existing test. Compilation and ignored-test enumeration are not compatibility
execution evidence. Host results describe the local fixture before its final
carry/publication; they do not qualify an untested later head.

Each native case retains artifact pins, clock mapping, explicit
argv/environment, running executable identity, input/event/request transcripts,
synthetic receipt boundaries, exact state/manifest snapshots, exit and cleanup
receipts, and final executable hashes. The corrected run identifiers and observed counts follow. Failed attempts
remain separately retained. Published evidence must omit copied binaries and sockets and
review private temporary paths with an explicit redaction record.

## First native execution

[Run 40056359883](https://github.com/autonomous-ai/lampOS/actions/runs/40056359883)
on `b76fe12ae` executed all six cases: **4 passed / 2 failed / 0 ignored**.
The four parent daily/on-off cases passed. Both new rested-night cases stopped
when the opposite process's private directory `b` collided with the existing
bus root `b`; strict directory creation correctly returned `AlreadyExists`.
The process directories now use a separate `child-<stage>` namespace, and all
five socket arguments point to that directory. Directory creation, private
permissions and all state/protocol assertions remain strict. The failed
attempt remains evidence; corrected native execution was still required at that point.

## Corrected native execution

Corrected head `8bc821736be09d62bd27f635f054350fca85a1d0`, based on `9ed44f88c`
with parent #3323, passed [dedicated run 40057211166](https://github.com/autonomous-ai/lampOS/actions/runs/40057211166):
**6 passed / 0 failed / 0 ignored**, including both rested directions.
[Ordinary ARM64 40057211171](https://github.com/autonomous-ai/lampOS/actions/runs/40057211171)
passed **11,163 PASS + 1 known LEAK**, zero failed, 77 skipped;
[Linux Clippy 40057211165](https://github.com/autonomous-ai/lampOS/actions/runs/40057211165)
passed. The known journal-test LEAK is retained separately from clean PASS.

All 6,974 source hashes match authored Git and both captured manifests. The
two rested cases each completed A→B→A, with six matching owned SIGKILL/reap
receipts in total, admitted renewals over one second, exact night/morning
claims, one private Stretch/Over/Done sequence per direction, stale/fresh
owner controls, unchanged inputs and return suppression. The historical
four-pass/two-fail collision run above remains evidence, not a pass.

These results qualify this exact synthetic released-reader fixture. They do
not qualify a later head, a real elapsed night, Expression or Body output,
updater installation, flash durability, or a lamp. Parent #3323 must land and
this continuation must rebase and pass its current gates before integration.

## Parent integration carry

Parent #3323 landed as `14cdabec5` in train 70. This continuation is now
rebased onto `d7b85b1dd`; its fixture and workflow are unchanged by the carry.
The intervening README-only head `22bf27d34` also passed dedicated **6/0**,
ordinary **11,163 PASS + 1 LEAK**, 77 skipped, strict Linux Clippy and fresh
Mac **514/0/7 ignored**.
[Its retained receipts](https://github.com/autonomous-ai/lampOS/tree/aaabe1987/lampOS/docs/verification/rested-state-current-native-20261024/readme-22bf)
remain bound to that earlier head. Fresh current-base gates are required
before this carried head is offered for integration.

## Absolute reply deadline correction

The shared clock/asked reply helper now uses one absolute deadline capped
by the original case, step and two-second I/O budget. Each buffer read uses
the remaining time; partial bytes cannot renew it. Ordinary complete and
dribbling-peer controls execute on the host; dedicated ignored selection
filters those two controls while retaining the six artifact cases. Earlier
fast native passes remain historical evidence, not timeout qualification.
