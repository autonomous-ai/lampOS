# Add-on drop completion: terminal CI appendix, 22 October 2026 UTC

Ready for lamp-voice: **NO**. [PR #3076](https://github.com/autonomous-ai/lampOS/pull/3076)
remains draft pending a coordinated current-parent window and independent review.
Its source head is `273f8fb96e8674d27495bb11176fa3170b385a40`; its branch parent
remains `464438fc549e528f1d812e25c14c459e4d609379`. This log-branch publication
changes neither source nor CI head. The [native/control proof](https://github.com/autonomous-ai/lampOS/tree/273f8fb96e8674d27495bb11176fa3170b385a40/lampOS/docs/verification/addon-drop-events-20261022)
is preserved unchanged, including every failure stream and replay patch.

Both required workflows **and their actual jobs** report terminal success:
[Linux ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/39815285950/job/119952117592)
and [Linux workspace Clippy](https://github.com/autonomous-ai/lampOS/actions/runs/39815286125/job/119952118426).
Both checked synthetic merge `9728c9064323ec29faff94440f2959f2df4667bf`, combining
target parent `7cedcc71c27cd2073356344047159c38bbf52a85` and the source head.
That successful generated merge is not a rebase of the owned branch or a waiver
of the latest-parent rule. [Terminal metadata](terminal-metadata.json) retains
actual workflow/job identities, steps, conclusions, times and raw-source digests.
It selects relevant API fields; it does not reproduce account profiles or
unrelated repository metadata.

The [early snapshots](early-status-disagreements.json) preserve both intervals
when a workflow said success but its actual job still said in progress with a
null conclusion. The watcher waited for both actual jobs and workflows to become
terminal, downloaded both logs successfully, and exited 0. No early aggregate
success was treated as final.

ARM64 release nextest passed **10,536 tests (1 leaky), 0 failed, 67 skipped** in
**178.315 s**. It listed 162 binaries; 142 distinct binaries emitted executed
test rows. The main result interval has 10,535 PASS plus one LEAK, with every
index 1 through 10,536 present exactly once and no duplicate binary/test pair.
Rows repeated after Summary are excluded. The leaky pass is
`lamp-voice same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
Separate doc tests pass **4/0**, relay signals reports `ok` without a numeric
count, and the process harness passes **12/0**. Skills contributes **500**
passing cases: library 156, binary unit 23, integration `it` 290, requested-audio
31. All nine add-on integration cases pass. [Parsed results](results.json)
retain the exact census, boundaries and counts. The PR selector ran the whole
workspace because the new JSON evidence path was not one of its excluded
Markdown paths; its exact reason is preserved. The run used profile `pr`, four
cores and parallel execution.

| Individual ARM64 release observation | Tested source / merge | Context | Case time |
|---|---|---|---:|
| Full #6055 | source `16b1a2a15` | full, serial per shard | 5.632 s |
| PR #2693 baseline | merge `18a1b3e29`, target `ec5428a34` | PR, parallel | 5.655 s |
| PR #3076 after | merge `9728c9064`, target `7cedcc71c` | PR, parallel | 3.113 s |

The case is `addon::says_the_judge_dropped_do_not_spend_the_hour`.
[Source comparison](source-comparison.json) verifies Git blob identities and
SHA-256 values from all three tested sources plus the owned source head. The
two baseline `a_fifth_say` helpers are byte-identical; the tested after-helper
matches this head's exact replacement. Production `skills/src/main.rs` and
`skills/src/addon.rs` blobs are identical throughout, and the original asserting
test function is identical. This scope is not whole-workspace equivalence.
Different parents and run profiles remain: these are individual case observations,
not a full-binary speedup or a matched ARM64 causal measurement.

[Selected output](selected-output.json) preserves exact checkout, command,
case, summary, add-on, leaky-pass and separate-harness lines. Each UTF-8 value
round-trips its original bytes, including newline, with a line number, length
and SHA-256. [Bulk log digests](bulk-log-digests.json) record the complete ARM64
and Clippy success logs and the two baseline log sources, their byte counts,
SHA-256 values, Actions links and external paths. Those full logs are not
committed here: GitHub retention or local cleanup may remove them, and a digest
alone cannot reconstruct a missing log. This appendix is a portable selected
receipt, not the full workspace stdout or source export.

The [scoped self-review](self-review.md) applies the builder checklist introduced
at `bad114496`: wrong-person, stop/forget, default-on effects, census, and gates.
It records the unchanged product paths and original negative windows without
claiming new dedicated product privacy runs. The native receipt records Mac
fmt, strict Skills Clippy, focused add-on tests, full serial Skills
**496/0/1 ignored**, and workspace build. Required Linux Clippy and ARM64
build/tests passed as above. A separate Linux fmt invocation and coordinated
latest-parent qualification are not recorded. Ready remains **NO**. No tests,
local ARM64 build, device operation or source rebase ran while packaging this
appendix. New JSON/hash/whitespace validation applies to this evidence only.
