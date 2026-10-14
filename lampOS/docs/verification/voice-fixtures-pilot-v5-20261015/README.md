# Pilot capture fixture correction before loaded qualification v5

Fourth's Intel x86_64 Mac, macOS 26.6.2 (25G83), Core i7-10700K,
64 GiB RAM, Rust/Cargo 1.96.0. No lamp, cloud, device or real key was used.
Commands, UTC boundaries, source identity and raw hashes remain in the
original `summary.json` and `ipc-summary.json`.

The v4 full qualification stopped at the rapid mute/reopen scenario with
`capture discontinuity`. Its original raw output and complete frozen source
identity remain in the adjacent
[failed v4 qualification](../voice-fixtures-loaded-v4-20261015/README.md).
The pilot fixture scheduled 10 ms blocks but stamped delayed blocks at
delivery, manufacturing gaps in otherwise continuous synthetic samples.
This is a confirmed source defect; its specific role in that failed run is
inferred, because the old run retained no capture timing diagnostics.

Capture now starts when the actual `StartCapture` message is received and
retains the original acquisition timestamp for every block. Both peer modes
send at most 64 due blocks per step. Sequence numbers advance on attempted
sends, so a failed send still leaves a detectable gap in later received
blocks. No old audio is refreshed at delivery. Real privacy bounds, the
100 ms input ceiling, deliberate faults and real exit/reap deadlines remain.
Rapid-cycle failures print diagnostics before the service assertion,
including capture send-attempt gaps and source age. These include failed
attempts and do not measure coordinator consumption.

All 32 original tests and 163 assertions remain. Three added deterministic
regressions use the production detector and input admission to cover a 60 ms
delivery pause, admission at 99,999 us versus refusal at exactly 100,000 us,
and actual dropped blocks or source gaps. Independent static review found
no actionable issues; its scope and limits are recorded in `review.json`.

The sequential focused gate ran:

```sh
rustfmt --check --edition 2024 crates/voice/src/coordinator/tests/pilot_faults.rs
cargo check --locked --offline -p lamp-voice --tests
cargo test --locked --offline -p lamp-voice --lib coordinator::tests::pilot_faults -- --test-threads=1 --nocapture
```

Formatting and compile checks passed. The first test invocation lacked
local Unix-socket permission: 5 helper tests passed and 30 scenarios failed
at channel bind with `EPERM`, before exercising their scenarios. That raw
output is retained as `pilot-tests.txt`, SHA-256
`d533b7fde79986ddbf8813b5bd203c300b7f3bf96aa584d6ea5116144b66853e`.
With the required local IPC permission, the unchanged-source module passed
35 / 0 / 0 in 15.79 test seconds, 16.125 command seconds. Its raw output
`pilot-tests-local-ipc.txt` has SHA-256
`1a69a8a9ec208c19a8f1826f700951435c57c96f457c4a341eac6c48a4997cf2`.
There were no panic diagnostics in the permitted run. All 696 recorded
source hashes stayed unchanged during each gate.

The final pilot file has SHA-256
`70a6608cf49ad78243dbddd74206fbe3526b59e1ff6a7060358f68846b227ae3`.
The subsequent task rebase from `78b984b69` onto `f2261ec48` changed only
upstream Memory documentation; all fifteen task files were byte-identical.
The v5 source-only export contains 1,678 files, 27,469,487 bytes. Its full
manifest has SHA-256
`ef52b710a28f72c2296cc3cbd37990e09fc13c44abeef8b05a095148c4fe0a73`.
The full loaded qualification is separate and was only beginning when this
focused evidence was published. No ten-run or native ARM64 task-head success
is claimed here.

Not shown: physical privacy response, source consumption age under lamp
load/heat, acoustic or mechanical behavior, or immunity to arbitrary host
starvation. The Body scope question remains pending and no Body source
was edited.
