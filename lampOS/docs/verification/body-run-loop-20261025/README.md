# Body run-loop carry, 25 October 2026

This carries [#3449](https://github.com/autonomous-ai/lampOS/pull/3449).
The authored change is retained, with no additional runtime change.
The original head is `f430a182b` on `056a76132`.

`body.rs` is byte-for-byte identical to the original offered head. Removing
exactly the 18 added wrapper/signature lines reproduces the complete
original base file. The recorded range-diff is equal. The closure
arguments retain their order and mutable borrows; port and runtime stay
generic. The loop's cancellation, torque checks and sleep calls remain
unchanged. No heap allocation, worker, queue, message field or unit setting
is added. The existing 20 ms step is unchanged; the cost of the indirect
calls has not been measured on a lamp.

## Verification identities

The source identities and original command results are retained separately:
a pass on an older base is not a pass on a later integration. All local
commands use native Mac x86_64 Rust 1.96.0, one compiler job and serial tests.
They use an existing exclusive Cargo target, so these are not cold-build
measurements. Complete command lines, UTC start/end times, exit codes and
harness totals belong in `gate-results.json`; literal harness result lines
belong in the accompanying text files. There is no local ARM64 build.

### Original complete gate

Head `640198b2c`, base `8e2405736`: formatting, strict all-target clippy
for Body and all 11 dependants, workspace build, and all affected tests
exited 0. Body: **608 passed / 0 failed / 0 ignored**, eight harness
summaries. Dependants: **6,729 passed / 0 failed / 52 ignored**, 71 harness
summaries. Combined: **7,337 / 0 / 52**. No disk-space error marker was
present in these logs. The built Body executable is Mach-O x86_64.

The broad original dependency closure includes development dependencies.
The runtime closure has six crates: Body, Cloud, Expression, Skills, Voice
and VoiceEval. The newer gates retain their exact source/base identities below.

### Runtime closure on the Store integration

Head `05b29451f`, base `a786e0454`: fmt, strict all-target clippy for
12 crates, workspace build and the full six-crate runtime test closure
all exited 0. Body: **608 / 0 / 0** in eight harnesses. Cloud, Expression,
Skills, Voice and VoiceEval: **5,706 / 0 / 34** in 27 harnesses. Combined:
**6,314 passed / 0 failed / 34 ignored**. No disk-space error marker was
present. This result is not substituted for the broader older gate.

### Note-wait integration

The authored carry rebased to `f2ddbc5f4` on `22d93d199`, with equal
range-diff and identical Body bytes. Only Voice implementation and docs
changed in the intervening base. Its integration gate repeats fmt,
strict all-target clippy for all 12 crates, workspace build, and full
Voice/VoiceEval tests. All stages exited 0: **4,656 passed / 0 failed /
21 ignored** in 11 harness summaries, with no disk-space error marker.

After preserving logs, results and 265 executable hashes from the completed
runs, Third cleaned the entire owned target (153,899 files, 42.4 GiB).
The fresh integration uses `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0`, still with
one compiler job and serial tests. Earlier timings are not comparisons
against this different build profile. No failed control binary was removed.

### Final memory-force integration

Train 128 then moved Main to `86416a0d3`. The authored carry became
`d29d8536b`, again equal by range-diff with identical Body bytes. The
new base contains the separately reviewed memory-force repair, its product
setting and docs. The final incremental gate repeats fmt, strict clippy12,
workspace build, full Body, the memory-ask/process/note regression groups
and full VoiceEval. All stages exited 0: Body **608/0/0**, memory-ask
**3/0/0**, worker-process **42/0/0**, note retries **25/0/0**, note deadline
**1/0/0**, and VoiceEval **623/0/0**; combined **1,302/0/0** in 16 harness
summaries. Each focused filter executed tests. No disk-space error marker
was present. The new PR's original Linux gates qualify the combined head
before readiness.

For this carry, the maintained CI source-times helper restored mtimes only
for identical Git blobs (7,039 files); eight changed/new files retained
checkout times and are rebuilt. The original source-time record is kept
separately. The compiler/profile settings remain the same as the note-wait
gate. This avoids recompiling identical source solely because of checkout
times, without treating any changed source as cached.

## Self-review and limits

The wrapper does not change person/session routing, privacy close, stop,
forget, output ownership or cancellation. It adds no default-on product
behavior or switch requiring a census entry. It changes dispatch inside
the existing loop and must still pass the native Linux integration gates.

The original PR's Linux runs belong to its old offered head. The replacement
PR must report its own original Linux ARM64 and strict-clippy runs before
readiness. No lamp, local ARM64 link or execution, nightly realtime group,
cold-build benchmark or physical latency acceptance is claimed. Original
build-speed measurements are on #3449; they are not Third's measurements.
