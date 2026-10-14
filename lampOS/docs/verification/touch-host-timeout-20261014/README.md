# Touch gate: inherited host motor cleanup timeout

Source `08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca` stayed unchanged.
Third's Intel i7-10700K x86_64 Mac, macOS 26.6.2 / Darwin 25.6.0,
Rust 1.96.0. No lamp or physical motor was accessed.

The full twenty-package serial host test command stopped with **2,804
passed, one failed, fifteen inherited ignored, 142 completed harnesses**
after 1,819.333908 seconds. Voice's library harness reported 830 passed,
one failed and six ignored. Later harnesses and the workspace-build stage
were not reached in that invocation. Format and strict all-target Clippy had
passed. The raw stream and exact command/count/source record are retained.

The failure was the unchanged inherited test
`coordinator::motor::tests::crash_and_stalled_control_never_allow_replacement_or_false_cleanup`
at `crates/voice/src/coordinator/motor/tests.rs:441`:
`owned child timed out: None`. Its inert subprocess uses no hardware.
The helper waits two seconds for its predicate; the production supervisor
allows three seconds for startup. That mismatch may explain sensitivity to
startup delay, but the failed run did not record which of the two child modes
or startup phase timed out. **The cause is not established.**

One focused Cargo invocation then passed unchanged. It built a separately
feature-unified library binary and reported one test passed, zero failed,
836 filtered. It is not the exact failed artifact. The subsequent three
serial direct runs used the exact original full-gate binary,
`target/debug/deps/lamp_voice-b15efe69bd87193c`, SHA-256
`de05a4465230f41753ce3285ee20536c1c9681e5c2ea3605142c7734160d0ced`.
All three passed, with process wall times 4.084656, 3.582238 and 3.563616
seconds. Each test exercises both crash and hang modes with the original
per-wait deadlines; the process duration is not a child-start latency.
All raw outputs and commands are retained. No source, assertion or deadline in the test was changed. These are focused diagnostic reruns, not
a complete replacement gate or a proof of root cause.

The exact-head native ARM64 run 38573575450 independently passed its release
workspace build and all 4,054 tests (29 inherited ignores). A fresh complete
serial Mac test gate is running, followed by the workspace build. Readiness
remains no until that complete gate passes. This folder retains the failure
even if the later gate passes; counts from overlapping runs are never added.

Not shown: the cause of the intermittent host timeout, scheduling behavior
on a lamp, motor cleanup on hardware, or any physical touch result. The
Voice/Body test and implementation belong to their owners and were not
changed by the touch task.
