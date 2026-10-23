# First Mac clock-control execution

Frozen base `f477d6c49`, corrected candidate `da595851f`, Rust 1.96.0,
x86_64-apple-darwin. `plan/` records the accepted plan and unchanged runner
as a text attachment; `run/` preserves the complete terminal execution.
`export-map.json` binds every copied byte. The 209,100,800-byte source
archive, scratch checkout and compiled Mach-O binaries remain external;
their recorded hashes and source commits make that omission explicit.

The full unchanged-base command was
`cargo test -p lamp-voice --offline --no-fail-fast`, with default test
parallelism. It exited 101 after 260.903 seconds: **3,828 passed, 1 failed,
14 ignored**. The library passed **3,548/0/14**. The sole failure was
`light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`
at its original latency assertion. Its 20 measured calls ranged from
2.842447 to 129.587621 milliseconds. The original failure is retained;
there was no retry or causal conclusion.

The literal raw-clock modulo-240-second held-lines control failed on the
base and passed on the candidate, once each. The base near-boot sweep then
recorded **3 passes and 10 ordinary failures**, before its fourteenth case,
`coordinator::tests::hand_controls::a_ridden_gap_gives_the_camera_and_the_eyes_nothing_until_it_reopens`,
aborted. Its first panic is subtraction overflow at hand_controls.rs:2358,
followed by a thread-local destructor panic and fatal abort. The process
exited -4 after 0.096 seconds, without a libtest summary, watchdog timeout
or remaining process group. It is **one abort, not a counted libtest failure
or pass**. The other 72 baseline cases and all 86 candidate near-boot cases
were not run by this execution. Neither were the stale-audio negative
control, candidate full suite, dependent-reader gates or workspace build.

The runner stopped as planned, preserved its exclusive execution marker,
and restored all 6,523 tracked scratch files to the exact candidate plus
original IPC clock. The authored worktree was never modified. All 101
terminal manifest records rehash correctly, including the omitted archive.
A separately frozen continuation may reuse the saved baseline Mach-O and
run only the remaining cases; this receipt does not claim that continuation
has passed or erase the original stop.
