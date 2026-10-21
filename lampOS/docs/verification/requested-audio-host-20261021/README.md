# Requested audio: 21 October host verification

The rebased Music revision uses integration base
`892b1d3233efb1f339ca60e64e4004b40659e006`. Its non-document source tree has
2,152 files, with combined SHA-256
`bbd21f1d16e601b9ea3ef3591f79b4f55ce6cc1d3afc70bd6a469222c686f021`.
The [complete host gate](host.json) passed: **8,620 tests passed, 0 failed,
36 ignored, 0 filtered** across the 36 selected consumer crates. All 2,152
frozen non-document source hashes still matched in both the checkout and
source-only copy after the final stage. The [consumer record](consumers.json)
includes changed crates, reverse dependencies and the cross-crate name grep.

| Complete test command scope | Passed | Failed | Ignored | Filtered |
|---|---:|---:|---:|---:|
| 35 consumer crates in a source-only copy | 8,362 | 0 | 35 | 0 |
| All `lamp-update` tests in the checkout | 258 | 0 | 1 | 0 |

The updater's inherited image tests read `robots/lamp/rootfs` outside the
standalone tree; none were filtered out. Its complete crate used a second
fresh target in the checkout. Formatting, strict workspace/all-target Clippy
and workspace build passed from the source-only copy. Exact commands,
environment, stage durations, result lines and raw-log hashes are in the
record. Native ARM64 CI and lamp playback are separate qualifications.

The [scan comparison](scan-benchmark.json) uses sixteen independent 60-second,
24 kHz mono PCM16 recordings. Only `Scan::unchanged` is timed, after loading;
fixture creation, snapshot cloning, cache copyback and logging are excluded.

| Host debug scan | Before | After | Payload bytes before / after |
|---|---:|---:|---:|
| First after load | 1,380.945167 ms | 1.481916 ms | 46,085,516 / 0 |
| Repeat 1 | 1,401.177416 ms | 0.749791 ms | 46,085,516 / 0 |
| Repeat 2 | 1,407.403209 ms | 0.762500 ms | 46,085,516 / 0 |

These are three samples on one host. They do not establish cold-filesystem,
ARM64, whole-loop, acoustic-onset or physical-stop latency. The baseline's
asset implementation has the same Git blob as public head `fd05557ba14`;
the diagnostic helper adapts byte accounting to that older API and omits the
new zero-read assertion. Its source hash and commands are in the record.

Additional focused checks: two descriptor/path stability tests, one scan-cost
test, 12 Voice integration tests, 49 Voice audio tests, all 30 Skills audio
integration tests and one prepared-handoff test passed. These repeats are
separate from the complete consumer totals. The prepared handoff measured
213 microseconds first use and 186 repeated, from confirmation retirement
to authored speaker IPC acceptance; its target is at most 250 milliseconds.
It does not measure acoustic onset.

All four renderer tests passed, as did render, package and disposable install.
The generated recording remained 48 seconds, 1,152,000 frames, 2,304,044 bytes,
24 kHz mono PCM16, with maximum absolute sample 900 and SHA-256
`134362ab72a19c143c627dbd438f3d17ceabe9c6923bad6f1ea3f7d5fb9579fa`.
Every installed file matched its staged hash; generated audio and packages
are not committed. No lamp, microphone, speaker or provider was used.

The [mutation checks](scan-mutations.json) ran five exact baseline tests and
then five independent runtime-only mutations. Every baseline passed; every
mutant compiled and failed its intended assertion. They cover the ctime
guard, binding the public digest to the verified bytes, copying refreshed
proofs back into inventory, rejecting a stale successful scan, and
rechecking the parent directory during a read. The stale-scan check proves
freshness revalidation, not inventory reinsertion or reauthorization. These
focused runs are separate from the complete consumer totals. The receipt
includes commands, source and log hashes, failure excerpts and the five
patches; the isolated sources were restored after each check.

Retained failed preflights:

- [Attempt 1](preflight-1.json): Clippy rejected the enlarged inline cache as
  a large enum variant. The verified metadata is now behind one box.
- [Attempt 2](preflight-2.json): Clippy rejected replacing that existing box
  unnecessarily. Scan refresh now reuses the allocation.
- [Attempt 3](preflight-3.json): 29 Skills tests passed and one failed because
  the new test parsed a TOML document as a value. The assertion now uses
  `toml::from_str`; all 30 passed in the current frozen run.

Each attempt used fresh build targets. The failed attempts' source copies
and raw logs are retained; their hashes, commands, exit codes and result
lines are recorded here. Completed build caches may be removed to respect
the scratch-space limit.
