# Published reminder and task state readers

Part of #2809. These fixtures use the actual published Linux ARM64 binaries
from releases **6408** and **6498** as the readers and writers. Test code
speaks their public protocols; it does not serialize a replacement store on
behalf of either release. This is a fixed historical pair, not a claim about
every newer release.

[Artifact identities](artifacts.json) record source commits, publication times,
sizes and SHA-256. All six downloaded executables matched GitHub's public asset
metadata and the corresponding decoded release manifests, and had AArch64 ELF
headers. This checks artifact consistency, not the manifest signature against
an independently trusted public key. That remains #1812's separate boundary.

Schedule is **byte-identical** in these two releases. Its pair checks the
published payload's continuity through process replacement; it cannot show
compatibility across a Schedule implementation change. Tasks and Voice have
different executable hashes. A new pair requires new pins and a new execution.

## Scope

- **Schedule:** both directions, A writes → B reads and writes → A reads.
  Pending reminders remain pending, a snoozed reminder retains its delivery
  identity, completed reminders stay absent, and the last mutation's exact
  receipt remains retryable. Separate stores end with an ACK or a snooze
  receipt because Schedule retains only its latest receipt. Same-boot and
  changed-boot-identity cases also distinguish untrusted and trusted clocks.
- **Tasks:** released daemon writers and readers, including restart fencing
  and the retained target/name companions. Restart does not mean prepared,
  queued or uncertain launches may run again. A test Harness supplies synthetic
  replies; no real task service or account is contacted.
- **Voice delivery ledger:** released `schedule-consumer` processes recover
  unfinished, completed and acknowledgement-intent cuts. The fixture supplies
  synthetic due events, Schedule replies and completion receipts. The
  consumer opens no provider, microphone, speaker or actuator.

The rows do not establish spoken delivery, completion of a real external
task, clock synchronization, a physical reboot or power-cut durability.
They also do not expand unknown enum acceptance: existing reader rules remain
unchanged. This change adds test fixtures and CI, not runtime behavior.

## Running and retaining evidence

The dedicated `lampOS published state compatibility` PR workflow runs on
Linux ARM64 in a fresh container. Its empty `/run` keeps the host's timesync
marker out of the synthetic clock cases. It downloads the six fixed public
artifacts; each fixture independently checks its expected size, hash and ELF
before executing a copy. The usual ARM64 and Linux Clippy gates are still
required separately.

For each of `schedule`, `tasks` and `voice`, set
`LAMP_COMPAT_<UPPERCASE_PART>_6408` and `LAMP_COMPAT_<UPPERCASE_PART>_6498` to
the corresponding downloaded binary paths. Set `LAMP_COMPAT_OUTPUT` to an
existing, short absolute directory, such as `/tmp/pair-output`, for Unix
socket path limits. In an isolated native Linux ARM64 environment, run:

```sh
cargo test --locked --release -p lamp-schedule --test release_pair -- --ignored --nocapture --test-threads=1
cargo test --locked --release -p lamp-tasks --test release_pair -- --ignored --nocapture --test-threads=1
cargo test --locked --release -p lamp-voice --test release_pair -- --ignored --nocapture --test-threads=1
```

These tests are ignored in the ordinary suite because they require external
published executables and the isolated ARM64 environment. Listing or compiling
them is not an execution result. The dedicated job keeps each command's exit
status and raw output, source-file hashes before and after execution, artifact
hashes, and the fixtures' per-case protocol/process/state records. Failed
attempts remain evidence; a later pass does not replace their raw records.

Execution results will be bound to the tested PR head and native run, with
case counts reported separately from the ordinary crate tests.

## First executed attempt

Native run `40051251652` on `ba7031f58` executed all twelve cases:
Schedule **4 passed / 0 failed**, Tasks **0 / 2**, Voice **4 / 2**.
The [original raw failures](https://github.com/autonomous-ai/lampOS/tree/993fc3530/lampOS/docs/verification/reminder-task-release-pair-failed-20261024)
remain evidence. Tasks correctly refused the fixture's non-private socket
parent; the fixture now sets and checks its mode and owner before startup.
Voice correctly retained the occurrence's first-attempt Ack ID while advancing
the completion attempt; the fixture now checks these distinct counters against
their respective saved values. Neither correction changes a released reader
or relaxes the later exact replay, settlement and return-reader assertions.
The corrected execution on `cec3372b7` passed all twelve cases in
[native run 40054409458](https://github.com/autonomous-ai/lampOS/actions/runs/40054409458):
Schedule **4/0**, Tasks **2/0**, Voice **6/0**, with zero ignored.
Ordinary ARM64 run `40054409431` separately reports **11,128 PASS + 1 LEAK**,
zero failed or unrun, and 83 skipped; the existing leak is not counted as PASS.
Strict Linux Clippy run `40054409449` passed. The corrected native results
belong to that authored head on base `59c4ac795`; a later folded/current-base
head must receive its own gates. Original failed receipts remain unchanged.
