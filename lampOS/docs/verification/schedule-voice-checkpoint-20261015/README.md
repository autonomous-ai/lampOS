# Task 31 checkpoint publication

Ready for lamp-voice: no

The task checkpoint is
[`c0bd596ac15ca64d7dc09534a65165e2be1eab03`](https://github.com/autonomous-ai/lampOS/commit/c0bd596ac15ca64d7dc09534a65165e2be1eab03)
on `third/schedule-voice`, based on `c1b8c75916cb2d6861a0c5c38d178f9dffeed4bb`.
The single-line message is `lampOS: add gated Schedule tools and due reminder speech`.
The exact staged inventory contained 674 text files; all 697 frozen Rust and
manifest hashes still matched before commit. The working tree was clean
after the commit. Only the authorized task branch was pushed; no merge-ready
pull request was opened.

[The task's verification folder](https://github.com/autonomous-ai/lampOS/tree/c0bd596ac15ca64d7dc09534a65165e2be1eab03/lampOS/docs/verification/schedule-voice-host-20261014)
retains the complete original host evidence, including failed gates,
source-copy comparisons and the motor diagnostic correction. This log
folder links that exact commit rather than duplicating its evidence payloads.

## Initial native run record

- [The first branch run listing](native-first-runs.json) identifies push-triggered
  run [38620759412](https://github.com/autonomous-ai/lampOS/actions/runs/38620759412),
  created at 2026-10-14T19:05:47Z for exactly `c0bd596ac15ca64d7dc09534a65165e2be1eab03`.
- [The initial job metadata](native-first-job.json) reports `in_progress`:
  setup/cache completed, the release workspace build running, and tests not
  yet started. A running build is not a passed build or test result.
- [Retention](retention.json) records the original paths, bytes and SHA-256
  of those two unmodified GitHub CLI JSON responses.

The workflow uses GitHub's Ubuntu 22.04 ARM runner, Rust 1.96.0, ordinary-user
execution, `cargo build --locked --release --workspace --bins --examples`,
then `cargo test --locked --release --workspace -- --test-threads=1`. Those
are CI commands, not lamp commands. No lamp was accessed, no runtime defaults
changed, and no native pass or qualified binary artifact is asserted here.

## Limits

The latest complete Mac gate remains failed with five inherited motor waits.
Passing focused checks and an instrumented diagnostic do not clear it. The
new #739 task specifies privacy-freshness fixtures; it does not explicitly
assign these motor failures. The earlier scope and Unknown-room questions
are still pending. First's eventual seven lamp cases and scheduler checks are
in [the task document](https://github.com/autonomous-ai/lampOS/blob/c0bd596ac15ca64d7dc09534a65165e2be1eab03/lampOS/docs/schedule.md#task-31-lamp-qualification).
