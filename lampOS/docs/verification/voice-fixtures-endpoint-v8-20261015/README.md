# Endpoint real-worker fixture privacy tolerance, V8 focused gate

Measured on Fourth's Intel development Mac (macOS 26.6.2 x86_64), not a lamp.
The fresh source-only snapshot and all 2,367 file hashes are in `source.json`.
All hashes remained unchanged through validation.

The four active finite real-ProcessClassifier ordering cases and the existing
ignored model-provenance case now explicitly select the existing per-instance
100,000 us test privacy policy and a real 12-second conversation watchdog.
Six stepped scenarios and three mute/cleanup scenarios retain default 50,000 us
privacy. Real classifier clocks and cleanup, source timestamps, 100 ms input/IPC
refusal and every original assertion are preserved. The watchdog excludes setup
and cleanup. No Ready wait hides intentional loading or missing workers.

Validation, sequentially from the frozen source:

- `rustfmt --edition 2024 --check crates/voice/src/coordinator/tests/endpoint_session.rs`: exit 0.
- `cargo check --locked --offline -p lamp-voice --tests`: exit 0.
- `cargo test --locked --offline -p lamp-voice --lib coordinator::tests::endpoint_session:: -- --test-threads=1 --nocapture`: exit 0; 17 passed, 0 failed, 1 existing ignored, 970 filtered; 25.16 s test time, 65.818 s command time including compilation.

Cargo used the existing Mac target, four build jobs and one test thread. The test
command had authorized local Unix-socket/child-process permission and stayed
offline; no device, cloud account or model was accessed. No focused command failed
or was retried. The sole panic diagnostic is the named expected clock-guard test.
The preceding loaded V7 failure is retained by the parent; this pass does not
establish that failure's exact scheduling cause.

`assertion-preservation.json` retains the source comparison: all 16 original
tests and 103 original assertion expressions remain; two regressions add eight
assertions. Independent read-only review found no actionable issues.

Not shown: the full loaded sequence, dependent/workspace gates, native ARM64,
arbitrary host starvation or lamp privacy latency. The bounded privacy diagnostics
record producer acquisition and after-send observations, not consumption age.

The scratch Python runner is intentionally outside this Rust-only repository.
It executed the three recorded commands sequentially, recorded UTC start/end
and monotonic command durations, enforced a 20-minute outer watchdog, retained
raw output, and checked the frozen source hashes before and after each gate.
Fourth's compiler-load loop was stopped during this focused validation.
Other activity on the shared Mac and host load were not recorded, so this
is not described as an idle-machine measurement.
