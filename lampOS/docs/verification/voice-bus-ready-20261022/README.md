# Voice bus fixtures start with their existing subscriber

Part of #1806. Base `789f1f609c49537175365e17a10b2e6eefda9eed`.
Only four 250 ms startup sleeps and their misleading first comment change
in `crates/voice/src/bus_out.rs`. The entire production prefix is unchanged.
Every original timestamp, request, expected tuple/vector, assertion and
receive window remains, including the 30 ms state/room/speech drains,
100 ms function-filter drain and 500 ms receive bound. The separate real
silence/lease test is unchanged.

`Subscriber::new` binds a socket under a temporary name and makes its
`.sock` path discoverable before returning. Each fixture subscribes before
its first publication. New publishers have `scanned: None`; declaring a
topic updates presence without scanning. First delivery therefore discovers
the already-bound subscriber synchronously before sending. There is no
asynchronous setup to wait for. Later subscribers after a previous scan
still use the ordinary rescan interval. Heard/said events are unretained;
no retained-state fallback is needed to justify readiness.

## Same-base host measurements

One exact test per compiled process, serial, three runs before and after.
Wall time includes launch, execution, cleanup and captured output;
compilation is excluded. The first invocation is retained. These are Mac
fixture durations, not lamp response measurements. [Environment and targets](environment.json),
[before measurements](before-host-timings.json), [after measurements](after-host-timings.json),
[before output](before.txt), [after output](after.txt).

| Case | Before (s), runs 1 / 2 / 3 | After (s), runs 1 / 2 / 3 |
|---|---|---|
| State cadence | 2.3455 / 0.5407 / 0.5341 | 2.0842 / 0.2810 / 0.2842 |
| Heard/said routing | 0.3520 / 0.3489 / 0.3511 | 0.0927 / 0.0916 / 0.0903 |
| Function-call filtering | 0.3899 / 0.3915 / 0.3929 | 0.1303 / 0.1281 / 0.1299 |
| Room cadence | 0.4744 / 0.4759 / 0.4728 | 0.2165 / 0.2137 / 0.2136 |

The four-process sums are [3.5617, 1.757, 1.7509] s before and
[2.5236, 0.7144, 0.7179] s after (runs 1 / 2 / 3).

All 24 invocations passed. Repeated runs meet the planned state <0.4 s,
speech <0.15 s, calls <0.2 s and room <0.3 s targets. The first state run
still misses its target and remains in the record. No causal explanation
for first-use overhead or claim about device behavior is inferred.

## Original assertions still detect errors

Three temporary mutations compile and produce four expected test failures:

- [Make repeat suppression include the exact one-second boundary](mutant-late-repeat.patch):
  the original state and room vectors both fail when their repeat disappears.
- [Swap heard and said topics](mutant-swapped-topics.patch): the original
  speech text/request tuple fails.
- [Remove function-call filtering](mutant-unfiltered-calls.patch): the
  original exact two-event vector fails with the unwanted call text.

[Control output](mutation-controls.txt) includes compilation and each exit
101. [Source hashes](source-sha256.json) cover the restored file and unchanged
production prefix. No mutation ships. The patches apply relative to the
repository root to the final source, for replay.

## Validation and scope

[Recorded commands](commands.json) and [local gate output](local-gate.txt)
contain the final results. The package closure includes ordinary, development,
build and platform-specific dependencies: [all twelve packages](affected-packages.json).
Commands run sequentially from `lampOS/`: formatting, strict all-target Clippy
for all twelve, the full Voice suite with `-- --test-threads=1`, all eleven
dependent suites with their default test concurrency, the workspace build
and diff check. Voice's serial gate is explicit; it does not claim a full
parallel Voice run. No test is filtered out by these gate commands.
The final Voice gate passed **3,300 tests**, with zero failures and 14
existing ignored cases. The final dependent gate passed **2,871 tests**,
with zero failures and 12 existing ignored cases. Formatting, strict
all-target Clippy, the workspace build and diff check passed.

The first dependent gate stopped in the Skills library at
`daily::tests::forward_claim_and_date_correction_preserve_exact_days_across_restart`:
reopening its journal returned `WouldBlock`, "daily journal directory is
already owned". The failed command and output remain in the record. With
the entire Voice source temporarily restored to the unchanged base, all
129 Skills library tests passed in each of three parallel runs
([comparison output](unchanged-skills-comparison.txt)). The failure was not
reproduced on that base, so it is not claimed to be inherited. This patch
changes only Voice's test module; its production prefix and Skills code
are unchanged. No cause for the one failure is inferred. Candidate source
was restored before repeating the complete eleven-package dependent gate.

Static review independently confirmed the socket-bind/first-scan boundary
and byte comparison of all original observations/assertions. Measuring
scripts remain outside the repository. Compiler JSON output is reduced to
the selected tested artifact and build-finished record; test output is kept.

Earlier full-main [ARM64 #5220](https://github.com/autonomous-ai/lampOS/actions/runs/39639887119)
is [before-only context](arm64-before-5220.json) from another runner/base.
The native PR result must be reported separately, with its exact PR head
and merge checkout. No ARM64 build runs on the Mac and no device is
contacted. Native CI must validate the final code on Linux ARM64; physical lamp
behavior remains unmeasured. #1806 remains open.
