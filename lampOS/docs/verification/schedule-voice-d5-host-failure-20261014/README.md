# Schedule voice: retained failed host gate on d5

On 2026-10-14, the development Mac (Intel i7-10700K, 64 GiB, x86_64,
macOS 26.6.2 / Darwin 25.6.0, Rust/Cargo 1.96.0) ran the Task 31 source
based on `d5d765862ab1f73629ee7a2c5a9a8086f0246d11`. The task source was
uncommitted; [source.json](source.json) identifies all 675 Rust/manifest
paths and the gate verified they stayed unchanged. No lamp or provider ran.

Formatting passed in 3.910193 s and strict all-target Clippy for Actions,
Schedule, Voice and Voice-eval passed in 43.095510 s. Their complete serial
test command failed with exit 101 in 676.865360 s: **1,111 passed, 2 failed,
8 ignored across 16 completed harnesses**. The Voice library accounted for
918 passed, 2 failed and 8 ignored, in 491.59 s. The workspace build was not
reached. Exact commands and ignored cases are in [results.json](results.json).

Both failures were inherited motor fixtures:

- `coordinator::motor::tests::crash_and_stalled_control_never_allow_replacement_or_false_cleanup`
- `coordinator::motor::tests::failed_reap_after_stopped_is_latched_and_reported_once`

Both panicked at `motor/tests.rs:441`: `owned child timed out: None`.
The earlier failing `frozen_config_and_executable_are_exact_and_source_changes_are_not_adopted`
passed in this invocation. No assertion, deadline, test filter or motor source
was changed to pass this gate. Its cause is not established. This failed run
remains failed even if later diagnostics pass.

The subsequently fetched integration `7fbb03e1f` adds First's IPC own-socket
bind/chmod startup correction. Third rebased the uncommitted task onto it,
with every task file byte-identical, and will verify that new source. This
record does not assert that the upstream correction caused or fixed either
failure. Task 31 is not ready for handoff.

The raw logs are unedited. [files.json](files.json) records their source names
and SHA-256. This is host software evidence, not lamp timing, physical output,
Linux execution, power-loss durability, or a completed Task 31 gate.
