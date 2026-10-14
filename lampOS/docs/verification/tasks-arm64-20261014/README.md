# Tasks native ARM64 gate — 2026-10-14

GitHub Actions run [38547291775](https://github.com/autonomous-ai/lampOS/actions/runs/38547291775)
at Third head `5bfab7b17b74fedcd176afc4dd917323ba1826a1`, on an
`ubuntu-24.04-arm` runner inside `debian:12`, Rust 1.96.0. This is CI evidence,
not a lamp result. The workflow does not record CPU model or RAM; neither is
inferred here. [Run/job/step metadata](run-38547291775.json) records the exact
GitHub timings and status.

`cargo build --locked --release --workspace --bins --examples` passed.
`cargo test --locked --release --workspace -- --test-threads=1` failed in
`lamp-controls --test program` before reaching Tasks tests. Six cases passed
and `sigterm_finishes_recording_and_releases_the_actual_serve_owner` failed
at `program.rs:340`: launching `/bin/kill` returned `ENOENT`. The minimal
container does not supply that external utility. The failure occurred after
the test had received the live Controls diagnostic and checked ownership.

[The failed step](run-38547291775-failed.txt) is retained unedited, including
all preceding output and failures. [Its summary](run-38547291775-summary.json)
counts only completed test harnesses in that step; it is not a whole-workspace
pass count. The binary hash and artifact steps were skipped after the failure.

Third's owned Controls test will use a direct Rust signal call, preserving the
real SIGTERM and recording/owner-release checks. Other crates' external-utility
assumptions were reported in Third's log. Tasks remains unqualified by native
tests until a full run reaches and passes them.

## What has not been shown

No Tasks test executed in this run, no ARM64 binary artifact was uploaded,
and no lamp, real Harness, credential, remote task or sound was exercised.
Successful compilation alone does not establish the new native test gate.
