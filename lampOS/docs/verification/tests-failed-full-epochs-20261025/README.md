# Failed Mac full runs retained before qualification

Ready for lamp-voice: no

These are closed macOS x86 full-run records. Update ran frozen fb8d2f71b on base 4270083a2; NoPause ran frozen 20874923d on that base. Neither execution is renamed as the current f978 PR head. All copied payloads are byte-for-byte, with hashes and sizes in manifest.json; the original absolute scratch paths in command/provenance records are historical paths, not portable execution instructions. No executable or measurement script is committed.

| Epoch | Passed | Failed | Ignored | Native controller exit |
|---|---:|---:|---:|---:|
| Update | 7,316 | 8 | 52 | 1 |
| NoPause | 7,265 | 49 | 52 | 1 |

Both remain FAILED. Formatting, strict twelve-package Clippy and workspace build passed separately. Update has 4,017/1/21 Voice and 3,299/7/31 readers; all 167 Round cases, including ten directed controls, pass. NoPause has 79 actual target outcomes; its changed runtime case fails after refusing 654 ms-old joints. The exact failure names, primary excerpts and source/target bindings are retained below each epoch. Green native ARM64 runs and older standalone observations do not waive these failures.

The commands and selected environment/profile values are in the copied records and plans. The Update run separately executes default-parallel Voice and its eleven dependent reader packages; NoPause executes the twelve-package default-parallel suite. Both use offline/locked Cargo and the pinned toolchain. The scripts bound each command, captured stdout/stderr, checked direct-child completion and owned process-group absence, and verified the full source byte/mode/mtime guards at closure. Source and executable lineage limits remain in their original receipts, including the Update canonical-copy provenance resolution.

The runs explicitly shared CPU allocation at four plus two owned Cargo jobs, across distinct targets, after the recorded amendment. They establish neither isolated host load nor historical cause. Fixed latest-base qualification is a subsequent, separately recorded experiment: one matched-base observation per context/case first, followed only for unmatched/pass/different/uncertain results by exactly three observations at the corresponding current PR head. No extra retries or best-three selection are authorized.

Read update/full-gates/full-gate-final-receipt.json and update/full-gates/final-failure-excerpts.json for Update. Read no-pause/run/closed-run-review.json and no-pause/run/closed-test-failure-review/all49-failures.json for NoPause. Raw complete test/compiler streams and actual command records sit beside their stage results. SHA-256 manifests bind all published payloads.
