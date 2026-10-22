# Voice serial result and failed consumer qualification, 22 October 2026

Ready for lamp-voice: **no; #3023 remains draft**. The complete five-consumer
command **failed with exit 101: 1,916 passed / 1 failed / 11 ignored**.
Full serial Voice passed, and a separate workspace build passed; neither
clears the consumer failure. This receipt is for pushed head
[`735b2cb5d`](https://github.com/autonomous-ai/lampOS/commit/735b2cb5db21176a0ad80fcb42c48e739f8362fe),
parent `ec5428a34`, on [#3023](https://github.com/autonomous-ai/lampOS/pull/3023).

| Native command | Actual terminal result | Wall time |
|---|---|---:|
| `cargo fmt --all --check` | exit 0 | 12.693 s |
| strict all-target Clippy: Voice and five consumers | exit 0 | 89.647 s |
| full serial Voice package | exit 0 | 798.116 s |
| full serial five-consumer command, `--no-fail-fast` | **exit 101** | 2,213.501 s |
| separate `cargo build --workspace --offline` | exit 0; source unchanged | 67.417 s |

Native x86_64 macOS, offline dependencies, four compiler jobs, no debug info
or incremental compilation, explicit `--test-threads=1`. Exact argv,
environment, times and hashes are in [results.json](results.json) and
[workspace-build-result.json](workspace-build-result.json). The original
driver stopped on consumer exit 101 and did not run its planned build; the
recorded build was a separate invocation. It did not retry any failed suite.

Voice passes library **3,384/0/14**, binary unit **1/0**, integration **269/0**,
examples **10/0**, docs **0/0**. All seven assigned cases, kept-start and
light-call pass. Library test time is 655.20 s; whole-command time includes
compilation and all targets. [Assigned results](assigned-and-disclosed.json)
and [full raw Voice output](voice-serial.txt.json) preserve those observations.

## The consumer failure remains

| Consumer package | Passed | Failed | Ignored |
|---|---:|---:|---:|
| Cloud | 193 | 0 | 2 |
| Memory | 298 | 0 | 6 |
| Skills | 496 | 0 | 1 |
| Update | 306 | **1** | 2 |
| Voice Eval | 623 | 0 | 0 |
| **Total** | **1,916** | **1** | **11** |

All targets, including five zero-test doc targets, completed under
`--no-fail-fast`. The only failed target is Update `round`, **105 passed /
1 failed**. [Target audit](consumer-targets.json) maps every terminal summary
to its package; `lamp-memory-eval` is a Memory binary, not another selected
package. [Complete consumer output](consumers-serial.txt.json) is lossless,
230,969 bytes, SHA-256
`f34508439300c9d60472bcb333970854870d6d4bbac0ccde9a892eb9cdefcd73`.

The failing case is
`self_cuts::interrupted_put_back_keeps_a_runnable_current_and_finishes_on_restart`.
Its [raw assertion](update-diagnostic/failure-excerpt.json) is
`crates/update/tests/round/self_cuts.rs:48: updater fixture did not exit`.
[Source boundary audit](update-diagnostic/execution-boundary.json) preserves
the helper's unchanged two-second deadline. After exact script/mode checks,
fixture fork-guard acquisition and a successful spawn, it polls child exit
at five-millisecond intervals. The scripts are `/bin/sh` with builtin
`printf` of `old-updater` or `new-updater`. The failure bounds exit observation
after spawn, not Voice response latency; it does not identify which repeated
helper invocation timed out. All original assertions and deadlines remain.

The focused baseline diagnostic ran the exact case **once** on unchanged
`ec5428a34`, with the same five selected packages and serial test setting.
It passed **1/0, 105 filtered**, 28.46 s test time, 135.988 s command wall time.
[Command](update-diagnostic/update-focused.json),
[stdout](update-diagnostic/update-focused.stdout.json),
[stderr](update-diagnostic/update-focused.stderr.json) and
[completed observation](update-diagnostic/update-baseline-observation.json)
are retained. This focused pass is not a passing whole-consumer comparison.

The failed candidate Round artifact is
`5bb29df8755ad7caddbeaae7e8af054d866bad5774be93840e9d229f333dad57`;
the freshly compiled baseline artifact is
`693a8bc43fd1a54151317badb2d94774cb5e0ba061d6003f692e5b908887b1f4`.
[Candidate provenance](update-diagnostic/candidate-round-artifact.json) and
[baseline target fingerprint](update-diagnostic/baseline-round-cargo-fingerprint.json)
show that **compiled bytes differ**. The fingerprint is not a complete
resolver graph. The candidate contains 179 global `lamp_voice` symbols;
there is no claim that the whole Round binary excludes Voice code. Update
and root Cargo source are identical between the two heads, but that fact,
a focused pass and the specific shell-script path establish neither an
inherited fault, host-load cause nor patch causation. The cause stays unproven.

## Source scope and remaining qualification

The rebase has range-diff **`=`** from `6d45857eb`; the eight Rust fixture
changes and all prior assertion/deadline protections remain as reviewed.
All **1,382** scoped file hashes match committed `735b2cb5d`. The runner
checked byte equality through the failed consumer command, and the separate
build reports unchanged source. [Source scope](source-scope.json) covers
crates and root Cargo/toolchain inputs, not the full repository or complete
doc/fixture closure. The [compiled inventory](compiled-inventory.json) is
**3,398 = 3,390 + eight upstream additions**, with no removed test names;
both raw name lists were compared and remain external with their digests.

[Qualification state](qualification.json) remains failed/not ready. Fresh
current-head CI is pending; no old-head CI is reused as current evidence.
The fixed default-parallel base/candidate Voice pair is running separately;
its live outputs and results are excluded here. The latest-parent window
and independent reviewer read remain. Earlier
[full failures and controls](../voice-full-run-events-final-20261022/README.md)
and the [prior serial timeout/completion](../voice-serial-refresh-20261022/README.md)
remain recorded; this receipt erases none of them.

Every raw `*.txt.json` or `*.stdout.json`/`*.stderr.json` wrapper preserves
original UTF-8 bytes, whitespace, byte count and SHA-256. The
[package manifest](package-manifest.json) maps originals to packaged files.
Scripts, binaries, Cargo machine-message streams and bulk symbol/source-map
outputs are excluded. No new gate ran to package this evidence; no latency
improvement, device qualification or local ARM64 build is claimed.
