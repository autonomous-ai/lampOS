# #3190 current-head native gate

The single prescribed gate on `4b92b677d379b6ac1dadbd9e306320a04af48672`, parent `8e3eaa6c26cbe1a5a0c924f5b3a7e972d9083850`, completed
with **7,155 passed, 4 failed and
32 ignored**. The native session 53346 exited
**1**. All counts below come from this execution,
not an earlier head. The selected 87 affected cases have actual outcomes of
**87 passed, 0
failed and 0 ignored**.

This receipt leaves every failure unwaived. It does not provide a paired
causal baseline comparison, substitute for Linux/ARM CI, or clear review and
current-parent requirements. No failed case or full command was repeated.

| Phase | Passed | Failed | Ignored | Filtered | Exit | Wall seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Full Voice, default parallel | 3927 | 1 | 15 | 0 | 101 | 394.452383 |
| All 11 readers, default parallel | 3228 | 3 | 17 | 0 | 101 | 1304.594164 |

Both test phases compiled successfully. Their actual Cargo launches,
per-target results and separate binary inventories bind completely: there are
no incomplete or unrun targets. The unsupported custom Update harness checks
are counted as ignored on Mac, not as passes. See
[census-and-outcomes.json](census-and-outcomes.json) for every case's terminal
row and artifact binding, and [affected-cases.json](affected-cases.json) for
the 86 boot-clock cases plus loading-child case and their source bindings.

The 87 names are the previously scoped selection, rebound to this head's
unique declarations, compiled library inventory and actual full-run rows;
none is ignored. Historical package totals were not count constraints.
Source-file changes outside a selected test did not silently reuse an old
binary qualification.

## Retained failures

- `lamp-voice:test:it`: `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`.
- `lamp-memory:test:it`: `voice::a_refused_follow_up_forget_survives_the_next_turn_note_refresh`.
- `lamp-memory:test:it`: `voice::forget_that_right_after_a_follow_up_forgets_the_entry_it_was_about`.
- `lamp-memory:test:it`: `voice::forget_that_after_a_follow_up_leaves_nothing_of_its_conversation`.

[failures.json](failures.json) includes every complete failed diagnostic block
as exact UTF-8, with its offset, byte length and SHA256 within the original
raw stream. For the light-call timing failure it records all 20 actual values,
the checked sorted middle at index 10, and the unchanged strict 25 ms bound.
The maximum is context only. All 20 calls returned `Outcome::Done` and cleanup
preceded the checked assertion. Source byte identity to the parent is recorded
as source evidence, not as a causal conclusion or a waiver.

## Independent gates

| Gate | Exit | Wall seconds |
| --- | ---: | ---: |
| fmt | 0 | 14.178887 |
| clippy | 0 | 92.392277 |
| workspace-build | 0 | 67.425362 |
| diff-check | 0 | 0.050832 |

[command-results.json](command-results.json) retains exact argument arrays,
bounded deadlines, actual exits, command durations and stream hashes.
[gate-plan.json](gate-plan.json) is the frozen plan. The two test commands used
`--no-fail-fast`, ordinary libtest default parallelism and `RUST_TEST_THREADS`
unset. Ordinary completed failures remained failed while independent planned
gates continued. CPU/MAIN was released at terminal before receipt packaging.

## Source and environment

All 10,028 tracked repository paths, including
6,682 lampOS paths, docs, root lamp data fixtures,
executable modes and the literal symlink target, were captured. Input mtimes
were refreshed before Cargo with content identity preserved. Exact clean
HEAD/branch and complete source aggregates were checked before and after every
command. The final aggregate equals the initial
`288a2d090b2690b9b4dd28d96ae16eb3a6202f493b2b72e4ab4df9a9b65fa1d2`. No source overlay or control hook was
used. See [source-and-environment.json](source-and-environment.json).

[authored.patch.json](authored.patch.json) preserves the exact eight-file
fixture/documentation patch. This is a fresh qualification after train45's
upstream Voice task/backchannel changes; earlier-head gates remain separate.
[dependency-closure.json](dependency-closure.json) records the current static
manifest closure of 12 packages: body, cloud, expression, keeper, lean, memory,
peek, setup, skills, update, voice and voice-eval, each with the `lamp-` prefix.

The authorized tool launch used `sandbox_permissions=require_escalated`.
Private Unix datagram and loopback TCP bind/listen preflights both passed and
closed before Cargo. Rust is 1.96.0 with host `x86_64-apple-darwin`; the binary
`file` outputs are retained. The captured OS reports Darwin x86_64; physical
hardware ISA was not independently measured. Environment: jobs 4, dev/test
debug 0, incremental 0, offline/locked, and only the existing MAIN target. No
ARM build, device access or unrelated workload was launched by this runner.
[preparation-revision.json](preparation-revision.json) preserves the
pre-execution declaration-matcher correction, which did not execute Cargo or
change source.

## Representation and validation

[native-streams.json](native-streams.json) includes exact complete stderr and
non-Cargo stdout. Cargo stdout is a declared byte-range projection removing
only parsed Cargo build records; every actual test row, summary and diagnostic
remains byte-identical. Original and retained lengths/hashes plus every range
are included. These projections are not called unchanged full stdout.
[artifact-bindings.json](artifact-bindings.json) retains actual compiler
artifact identities captured before shared-target reuse.

[external-raw-index.json](external-raw-index.json) indexes the full external
raw streams and inventories. Local paths are not a remote availability promise.
This publication contains all failure bytes and the complete retained test
output and case matrix; binaries, Python orchestration, full Cargo JSON,
whole source exports and bulk tracked-input maps are excluded. JSON wrappers
preserve raw whitespace and terminal newlines without Markdown whitespace
errors. No authors, workflow actors or other person-attribution metadata are
added.

[validation.json](validation.json) records exact-byte/hash reconstruction,
count binding, links and whitespace checks; [manifest.json](manifest.json)
binds every publication payload. No new test execution occurs in sealing.
