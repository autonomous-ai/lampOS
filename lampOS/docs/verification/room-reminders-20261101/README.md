# Room reminders — 1 November 2026

The first builder's decision on Queue #4710 makes every unowned reminder
and timer the room's. Any voice caller may list, cancel or snooze one.
Known-person items still require the exact setter id; an owner flag does
not bypass that check. Existing unowned records need no rewrite. No wire
field or store schema changes, new I/O, retry, wait or network call.

## Scope and privacy checks

The Schedule tests cover two known people, an unknown/room caller, owner
status without the matching person, persisted records, repeating snooze
and cancellation, short timers, the former two-hour boundary and a
three-week relative reminder. Forget cancels a person's items instead of
making them unowned. Actions exercises the actual Actions and Schedule
sockets, including calls without caller metadata. App and delivery reads
without an asker retain their existing whole-store view.

The immediately previous build already preserves setter metadata. Builds
predating setters cannot preserve that field when rewriting the store;
this patch does not claim safe downgrade to such an ancient build.

## Host timing

Measured on x86_64 macOS while the dependent tests were also running.
The visibility target set before implementation was less than 1 ms for a
full-store permission/count read. The first sample was 0.725 ms, repeated
p50 0.657 ms and p95 0.906 ms; the maximum was 1.093 ms, above that target.
This is the new behavior's measurement, not a before/after speedup claim.

`cargo test -q -p lamp-schedule --test it a_full_store_keeps_room_counts_private_and_measures_visibility_latency -- --nocapture`
passed one test (192 filtered). It measures `Engine::answer(ListSet)` entry
to returned JSON, 128 stored items, a pending-only room caller, one first
read and 100 repeated reads. In-memory persistence; IPC, disk writes,
process startup, provider, acoustic input and lamp hardware excluded.
The functional assertions check the visible count and absence of private
items; timing is printed evidence rather than a flaky assertion.

`cargo test -q -p lamp-actions --test it host_first_and_repeated_actions_to_durable_schedule_receipts_are_measured -- --nocapture`
passed one test (35 filtered). All 21 durable writes succeeded through
real local Actions and Schedule sockets with a SQLite FULL store. First
write: 482.141 ms; repeated p50: 343.688 ms; p95: 488.493 ms; maximum:
496.099 ms. These exceed the existing 250 ms host target. Ten pending-only
lists had p50 37.185 ms and maximum 83.515 ms. The boundary starts before
the client socket connection and ends at the parsed durable response;
process/store startup is excluded. See [raw measurements](host-timing-loaded.json).
These loaded-host results do not demonstrate meeting the timing target.

After the broad suite and workspace build finished, the same two commands
passed again. Full-store read: first 0.770 ms, repeated p50 0.382 ms, p95
0.602 ms, max 0.691 ms (100 repetitions). Durable write: first 89.148 ms,
p50 86.958 ms, p95 91.348 ms, max 93.215 ms (20 repetitions); all 21
writes succeeded. Pending-only lists: p50 5.966 ms, max 9.302 ms.
[These measurements](host-timing-after-build.json) meet the stated host
targets in this run. Both runs use the same source and boundaries; they
are observations under different host load, not an implementation speedup.
The two isolated motor tests also ran during this interval, so this is
not a claim that the whole host was idle.

No lamp, SSH, live provider or played end-to-end measurement was used.
The existing slow-adapter test checks the shared deadline and that an
uncertain mutation is not reissued. Storage acknowledgements and input
readiness behavior are unchanged.

## Validation record

Local validation started on lamp-voice `e34f3db38` plus this patch, using
`CARGO_TARGET_DIR=/Users/d/code/autonomous-os/lampOS/target`.

- `cargo fmt --all --check` passed.
- `cargo clippy -q -p lamp-schedule -p lamp-actions -p lamp-cloud -p lamp-keeper -p lamp-memory -p lamp-setup -p lamp-skills -p lamp-update -p lamp-voice -p lamp-voice-eval --all-targets -- -D warnings` passed.
- `cargo test -q -p lamp-schedule -p lamp-actions` passed 350 tests, nine ignored, before the final full-store timing test was added.
- The first sandboxed socket run was denied local IPC; it was rerun with IPC permitted. No device connection was involved.
- The broader ten-package `cargo test -q` command stopped in unchanged Memory test `health::tests::held_publication_cannot_block_a_real_socket_or_bounded_shutdown`: `WouldBlock` after its five-second read timeout (117 passed, one failed, two ignored in that suite). No Memory implementation was edited.
- `cargo test -q -p lamp-memory --lib health::tests::held_publication_cannot_block_a_real_socket_or_bounded_shutdown -- --exact --test-threads=1` passed in 0.27 s. The original failure is retained; this does not prove its cause.
- `cargo test -q -p lamp-memory -- --test-threads=1` passed 360 tests, nine ignored.

The remaining dependent command is:
`cargo test -q -p lamp-schedule -p lamp-setup -p lamp-skills -p lamp-update -p lamp-voice -p lamp-voice-eval -- --test-threads=4`.
Schedule's current integration suite passed 190 tests, three ignored,
including the final full-store test. Update's round suite passed all 253
cases in 1,490.25 s; Linux syscall tracing cases remain explicitly skipped
on this Mac. The Voice library suite finished with 4,404 passed, two failed and 15 ignored. It failed in
unchanged `coordinator::motor::tests::motor_crash_preserves_actual_voice_owner_and_next_audio_dispatch`
and `coordinator::motor::tests::privacy_does_not_hide_actual_discard_or_close_failure`.
The first hit its two-second host deadline; the second reported motor
startup timeout before connection. Both passed individually (0.97 s and
1.95 s) from a preserved copy of the actual compiled Voice test executable,
with `--exact --test-threads=1`, using their full test names above.
Executable SHA-256:
`77aa311c895eac29f022a2fc2af1196a3e1ed99f9eb7c7e1e31c6f6da2b42a6c`.
Its dep-info names this frozen source checkout. The original failures
remain recorded; isolation does not prove their cause.

Across the completed suites, that six-package command passed 6,244 tests,
failed two and ignored 38, then exited 101. Later Voice binary/integration,
Voice-eval and doc-test stages did not run locally in that command. They
remain native-CI coverage; no clean full-local-suite claim.

`cargo clippy -q -p lamp-schedule --all-targets -- -D warnings` passed after
the final full-store timing test was added. `cargo build -q --workspace`
passed with `CARGO_BUILD_JOBS=2`. The broad run used frozen `1f06941f2`; the first rebase to `0b3db9c13`
onto `45e3df5c6` preserved the authored patch by range-diff. Schedule and
Actions source trees are identical across that rebase. Native ARM64/Linux
CI and the required two reads are recorded in the PR rather than frozen
here as a claim of readiness.
