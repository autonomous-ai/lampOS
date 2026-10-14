# First loaded Voice fixture qualification — failed

This is Fourth's development Mac, not a lamp: x86_64 macOS 26.6.2
(25G83), Intel Core i7-10700K, 64 GiB RAM, Rust/Cargo 1.96.0.
The uncommitted source is based on `6573ce6c1b594bf9c8cf7836bbd0e162c09e4cd1`.
`source.json` hashes every frozen source file. `source-changes.patch` plus
the two `new-*` files retain the changes used for this attempt.

From that fixed source, `cargo fmt --all --check` and
`cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
passed. The first full serial run used:

```sh
cargo test --locked --offline -p lamp-voice --no-fail-fast -- --test-threads=1
```

It ran from 2026-10-14 19:43:09.328 to 19:51:39.157 UTC and exited 101:
**1,051 passed, two failed, six ignored, 21 suites**, in 509.801 seconds
including Cargo work. The library itself took 442.93 seconds. The three
original #739 cases and all motor cases passed; the adjacent cases
`a_complete_verdict_ends_at_one_second_and_its_answer_plays` and
`failure_notice_yields_mid_play_to_a_new_owner_under_vad_only` failed
privacy freshness. Their assertions and diagnostics are unedited in
`mac-voice-01.txt`. The pilot peer's 43,883 us send gap is not the age at
which an observation was consumed. It does not isolate the whole cause.

Two actual release ARM64 cross-build cycles overlapped the Mac test run.
They used the existing x86_64 Linux Podman VM, ordinary UID 1000, offline,
no capabilities, read-only source/registry and Fourth's own target volume.
Each cycle cleaned only workspace package release ARM64 outputs and then ran
`cargo build --offline --locked --release --workspace --bins --examples
--target aarch64-unknown-linux-gnu`; dependency caches were retained.
Both passed (362.486 and 150.848 seconds). This is ARM64 compilation load
on an x86_64 VM, not ARM64 execution or a lamp result. Compiler-start,
cycle intervals, exact command, image hash and raw output are retained.
The host sampler took 103 readings every five seconds: one-minute load
ranged from 5.028 to 11.315, minimum available filesystem space was
48,049,704,960 bytes. These are host-level samples, not CPU utilization
or per-process scheduling measurements.

The harness stopped future runs after the first failure and let the
current build finish. Ten successes are **not** claimed. Every frozen
source hash still matched at completion. Next work extends the same
clocked fixture to other scripted endpoint/pilot cases, and covers
First's additional early-end scenario. Production bounds remain unchanged.

The scratch Python harness is outside lampOS. It ran each command with a
30-minute outer watchdog, recorded UTC and monotonic wall durations,
parsed standard and custom test summaries, recorded load/free space every
five seconds, hashed source and output, and stopped on the first failed
suite/build or source mismatch. It did not retry a failed run.

No physical privacy, servo, acoustic, hot-lamp or end-to-end latency was
measured. The real 50 ms privacy bound remains an on-lamp question.
