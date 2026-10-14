# Pilot metadata fixture correction v7

Focused validation on Fourth's Intel development Mac: macOS 26.6.2 (25G83),
x86_64 Core i7-10700K, 64 GiB RAM, Rust/Cargo 1.96.0. This is not a lamp.
The source base is `cf2f5b43c8113a704324c2743af3cd92bbc4fdda`.
The corrected pilot file SHA-256 is
`38826ab14ec9ce55ef4c1fe867fb7c1b9ca7ff653fa53baf73dc7bfc3bf43606`.

The [failed loaded v6 run](https://github.com/autonomous-ai/lampOS/tree/157adef7a64635cba38428dd2d22f4f227e55b78/lampOS/docs/verification/voice-fixtures-loaded-v6-20261015)
reported lost privacy freshness before its metadata-stall scenario reached
any accepted output or scripted SIGTERM. The measured producer gap was
50,902 us; coordinator consumption age was not recorded. Its assertion
unwound before releasing the blocked writer, causing the retained secondary
RecvError panic. The source and raw failure hash are linked in
`failure-source.json`; no earlier failure is discarded.

The correction is confined to the pilot test module. It primes the actual
metadata writer through the normal record/flush path and waits, within a
real two-second startup budget, for entry into its first blocked event.
This happens before children or scoped time. Its synthetic peers then use
stepped policy time, while the separate writer thread, actual SIGTERM,
child startup/reaping and logging finish remain real. The ordinary privacy
bound is still 50,000 us. Rapid mute/reopen remains threaded.

Separate host instants bracket SIGTERM send and the final verified child
exit/reap checks. The original less-than-one-second shutdown assertion and
500 ms finish wait plus 250 ms host assertion margin remain. The original
unacknowledged report must show zero written events. Checked release then
obtains a later real writer-finish acknowledgement without changing that
original report. An RAII guard sends the release token during unwind, and
bounded unwind cleanup keeps the entered receiver alive while finishing.
A regression preserves a typed scenario panic and requires the actual
writer's completion after release.

Independent review found no actionable findings. All 35 original tests and
179 assertions remain; 34 test bodies are unchanged. The metadata scenario
retains each original assertion with actual host timing. The one added
regression makes 36 tests and 185 assertion macros. The module now has 29
explicit stepped scenarios, one threaded cross-cycle scenario, and separate
helper regressions. `assertion-preservation.json`, `independent-review.json`
and the exact incremental diff retain those checks.

The frozen snapshot ran these commands sequentially from its `lampOS/`:

```sh
rustfmt --check --edition 2024 crates/voice/src/coordinator/tests/pilot_faults.rs
cargo check --locked --offline -p lamp-voice --tests
cargo test --locked --offline -p lamp-voice --lib coordinator::tests::pilot_faults -- --test-threads=1 --nocapture
```

All three passed. The focused test command recorded **36 passed, 0 failed,
0 ignored, 950 filtered out**, 13.79 test seconds and 44.212 seconds including
Cargo compilation. It is explicitly a module filter, not a full Voice run.
Formatting took 0.307 seconds; the compile check took 22.199 seconds.
All 2,367 recorded frozen and main source hashes stayed unchanged. Raw output,
exact commands, machine identity and elapsed durations are retained; no
measurement script or binary is committed. No permission failure or retry
occurred in this focused invocation.

The metadata case measured **6 ms actual SIGTERM send to verified reap** and
**506.854938 ms actual logging finish**, with `acknowledged: false`,
`degraded: true`, zero written events and 65 unconfirmed events. The later
release acknowledgement assertion passed. These are measurements from this focused invocation, without this task's
concurrent ARM64 build loop; host load was not recorded. They are not lamp
timing or arbitrary host starvation guarantees. The raw log includes the intentional typed
panic from the unwind regression; there was no unexpected background panic,
RecvError or AlreadyClosed diagnostic.

Raw focused test SHA-256:
`da8b96702691404d0556edf814ee289a6f1f5717a7a6609bed4648ee22fb6a29`.

`loaded-v7-source.json` identifies the fresh full-sequence snapshot, with
manifest SHA-256
`ca1134be293b53de76ff744ea74fb38bad259e82d11638bdb77141869d81a039`.
Only the pilot test file and its two docs differ from frozen v6; 2,364
files match. No earlier pass counts toward the restarted ten-run sequence.
Loaded qualification, remaining workspace/dependent-crate gates, exact-head
native ARM64 CI and the lamp compatibility minute remain separate.
