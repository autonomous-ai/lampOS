# One light-call phase observation

The one approved full Voice integration observation finished **269 passed,
1 failed, 0 ignored**, with all 270 names accounted for. Actual test and
controller exit were 101; no retry ran. The only failure was the original
light-call strict 25 ms upper-middle assertion: **53.314423 ms**. All 20 Done
checks, ring join and directory cleanup passed. This separate diagnostic clears
no previous gate and does not recreate the preceding library process or the
full-package history, earlier child cleanup or host load.

The frozen source was `4e8ea747cd` on `09f8e7412`, its two exact inherited peer
overlays, and the separate scratch `light_call.rs` instrumentation. Both 2 ms
intervals, 600 ms confirmation, 20 calls, real clocks and all original assertions
were retained. Source and artifact hashes matched after execution. Compile
passed once in 45.949983 s, retaining one unused-import warning (`Light as _`);
no strict-Clippy success is claimed. The full target took 9.93 s in libtest and
9.975794 s at the process boundary, under the unchanged 300 s outer watchdog.
The binary is Mach-O x86_64; Rust 1.96.0 reports x86_64-apple-darwin. No physical
ISA/Rosetta, ARM-build, device or end-to-end lamp claim is made.

Both trace lanes are complete: caller 236 records, ring 248, each capacity 65,536
and lost 0. The ordered first seven calls took 2.649879–4.038214 ms; calls 7–19
took 35.619366–131.204470 ms. Eleven delegated `ask` intervals reached 25 ms;
call 18 spent 130.933892 ms in that enclosing method within 131.204470 ms total.
All 62 ring sleep intervals stayed 2.015606–3.027612 ms. Fourteen of 62 recorded
ring ticks reached 25 ms, including one before the first timed call; 13 of 45
ticks fully inside the 20-call window did so.

These are enclosing wall-time observations, including instrumentation and
possible descheduling. They do not establish a filesystem, syscall or scheduler
cause. Nested reads in `there`/`covered` remain inside those intervals; concurrent
ring durations must not be added to caller durations. No cadence or product
change follows from this receipt without the owner's decision.

[Observation](observation.json), [case outcomes](case-outcomes.json) and
[phase records](phase-observations.json) retain the exact counts, ordered samples,
original sorted list and measured boundaries. [Raw streams](raw-streams.json)
include complete integration stdout/stderr, both compiled lists, other small
command output and the warning; exact UTF-8 strings have original lengths and
SHA256. [Commands](commands.json) retain every attempted command, duration,
exit and cleanup. All tracked children were reaped; no cancellation or timeout
occurred. CPU/MAIN was released before packaging.

[Source binding](source-binding.json) and the [scratch patch](diagnostic.patch.json)
bind source, assertions, artifact, census and frozen execution. [External index](external-raw-index.json)
identifies omitted executable, full Cargo JSON, bulk tracked inventory and
hostname-bearing metadata; inherited PATH and hostname are not published.
Local paths are not promises of remote availability. [Validation](validation.json)
and [manifest](manifest.json) cover wrappers, source hashes and whitespace.
