# Cue peer-closure fixture: fixed control observations

All four planned observations ran once on Mac x86_64 and matched their frozen
expectations. The runner finished with exit 0 and restored the exact candidate
after every variant and at completion. The two negative controls remain actual
failed tests. Full package gates have not run and are not qualified by this
receipt.

The uncommitted two-file candidate is on base
`3e75cc3cbe4c544dee433fb8a81dc3b06daf688d`, tree
`0af3ca0dc1c800eeb442b9b856c144d41ee85f3f`, in the owned
`tests/1569-cue-peer-closed` worktree. This base SHA is not a candidate commit.
The [source binding](source-binding.json) and [complete patch](candidate.patch.json)
identify its exact bytes. All five original assertions, the stale-cue half,
trace, `AT`, and single peer-loss scan remain unchanged. Product sources are
byte-identical to the base. No holder thread or artificial receiver copy ships.

| Observation | Actual result | Process wall time |
| --- | --- | --- |
| Old fixture with acknowledged receiver copy held through the scan | 0 passed / 1 failed, exit 101, at the original scan assertion | 0.096068 s |
| New wait: release and join only after its live-probe event | 1 passed / 0 failed, exit 0 | 0.044194 s |
| New wait with copy retained | 0 passed / 1 failed, exit 101, at the five-second setup watchdog after 4,083 live probes | 5.146759 s |
| Unmodified candidate exact case | 1 passed / 0 failed, exit 0 | 0.044035 s |

Each row selected only
`fixture_provider::stale_cue_and_peer_loss_latch_invalid_without_blocking_or_retrying`.
All four compiled integration inventories were equal: 270 cases, zero ignored;
269 cases were filtered in each actual execution. The inventory is not a full
integration pass. The quoted process times include test-process startup and
teardown and are not lamp latency measurements. Libtest's own raw durations are
retained separately.

The old control observed a live endpoint, then failed at the unchanged final
assertion while the owned copy remained held. The new control's stderr records
live observation, explicit release and successful join, then closure observed
before the unchanged single scan. The watchdog control records its setup
failure and successful release/join; its marker for reaching the final scan is
absent. Assertion unwind preserves the original failure after cleanup. These
are controlled socket-lifetime observations, not identification of the original
failed full-run cause or a claim about an observed fork/exec interleaving.

[Raw output](raw-output.json) contains full lossless UTF-8 stdout/stderr for all
four actual case executions, every inventory/file check, and toolchain commands.
[Observations](observations.json), [commands](commands.json),
[artifact identities](artifacts.json) and the [compiled census](inventory.json)
separate expected negative results from invalid timeouts or build failures. All
four compilations succeeded, no external watchdog expired, no case was retried,
and none remained unrun. The unchanged candidate executable was
`cad08a88aaca6cb6dd5077d18a16d13cb6f96196973400cce85925edcb837c91`
(94,198,908 bytes, Mach-O x86_64). No executable is included.

The [execution plan](execution-plan.json) binds the offline/locked commands,
MAIN target, jobs/debug/incremental settings and separate 1,800 s compile and
120 s process bounds. Before compilation, all 10,028 tracked repository paths
(including 6,682 lampOS paths) were refreshed for Cargo freshness; complete
bytes/modes/symlink hashes were identical before and after. Every restoration
returned the same aggregate
`25253cdcfe6c0c5e60023458eaf33373df95009ea1ac16040c31e4bf386fe3a2`.
The full source map and measuring scripts remain external, with exact digests.
The [successful compile streams](external-raw-digests.json) likewise remain
external; their full bytes were verified against the captured command records.
Cargo-message JSON and full source copies are not included here.

Three replayable fixture-only patches are stored as lossless JSON envelopes under [controls](controls/):
[old held copy](controls/01-old-held-copy.patch.json),
[event-driven release](controls/02-new-live-then-release.patch.json), and
[setup watchdog](controls/03-new-held-copy-watchdog.patch.json). They contain no
product mutation. The [envelope map](patch-envelope-map.json) maps historical
execution-plan paths to public envelopes. Decode each envelope's `text` field
and encode it as UTF-8 without adding or removing bytes; verify its declared
byte count and SHA-256, then use the reconstructed patch in a scratch checkout.
The complete candidate patch uses the same encoding. This avoids trailing
whitespace in the enclosing evidence files while preserving every original
unified-diff byte, including blank context lines. The [separate gate plan](gate-plan.json) is unexecuted: local
formatting, strict Clippy, full Voice and reverse-reader tests, workspace build,
then required native CI and exact-source Linux formatting remain. No reader
failure is waived, and no Linux, ARM64, device or full-parallel result is claimed.

[SHA256SUMS](SHA256SUMS) binds this compact receipt. Full external execution:
`/private/tmp/tests-cue-peer-closed-execution-20261023/run-2026-10-23T120536.569682Z`.
