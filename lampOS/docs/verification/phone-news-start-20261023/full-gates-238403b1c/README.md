# #3218 current-head native gate

The single prescribed gate on `238403b1c5959fe8834aba09cfbbf24f03b6d893`, parent `8e3eaa6c26cbe1a5a0c924f5b3a7e972d9083850`, completed
with **7,156 passed, 3 failed and
32 ignored**. The native session 8703 exited
**1**. All counts below come from this execution,
not an earlier head. The owned phone case has the actual result
**ok** from the completed full library run.

This receipt leaves every failure unwaived. It does not provide a paired
causal baseline comparison, substitute for Linux/ARM CI, or clear review and
current-parent requirements. No failed case or full command was repeated.

| Phase | Passed | Failed | Ignored | Filtered | Exit | Wall seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Full Voice, default parallel | 3927 | 1 | 15 | 0 | 101 | 378.17345 |
| All 11 readers, default parallel | 3229 | 2 | 17 | 0 | 101 | 1302.40676 |

Both test phases compiled successfully. Their actual Cargo launches,
per-target results and separate binary inventories bind completely: there are
no incomplete or unrun targets. The unsupported custom Update harness checks
are counted as ignored on Mac, not as passes. See
[census-and-outcomes.json](census-and-outcomes.json) for every case's terminal
row and artifact binding, and [owned-phone-case.json](owned-phone-case.json)
for the actual phone fixture row. Its unique compiled name and full-run result
were checked; historical package totals were not count constraints.

## Retained failures

- `lamp-voice:test:it`: `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`.
- `lamp-update:test:round`: `self_cuts::cut_after_confirmation_clears_pending_keeps_new_runnable_on_restart`.
- `lamp-update:test:round`: `self_cuts::interrupted_put_back_keeps_a_runnable_current_and_finishes_on_restart`.

[failures.json](failures.json) includes every complete failed diagnostic block
as exact UTF-8, with its offset, byte length and SHA256 within the original
raw stream. For the light-call timing failure it records all 20 actual values,
the checked sorted middle at index 10, and the unchanged strict 25 ms bound.
The maximum is context only. All 20 calls returned `Outcome::Done` and cleanup
preceded the checked assertion. Source byte identity to the parent is recorded
as source evidence, not as a causal conclusion or a waiver.

Both Update failures occurred in `prepared()` at its first OLD-script
executable check, before either test reached its intended switch/confirmation
or put-back assertions. The existing two-second watchdog observed
`final_status=Ok(None)`. Full stderr also records successful kill and reap of
each owned child (exit status signal 9). These exact cleanup lines are included
with both complete panic blocks; no live PID was signalled during packaging.

[update-round-comparison.json](update-round-comparison.json) compares the
retained companion boot gate at 4b92. All 123 compiled Round names agree; boot
passed 123/0/0 and this phone run returned 121/2/0. All 44 Update crate source paths and the manifest/lock/toolchain bytes match
the common parent 8e3. The selected compiler-artifact profiles and features
match between the two runs, but the Round executable SHA256s **differ**. A compile-time `CARGO_MANIFEST_DIR` reference
exists and the worktree paths differ; binary-difference localization was not
performed. Neither source equality nor the passing companion run waives these
failures or proves why they happened.

The complete Voice stderr retains a LoadingChild missing-script diagnostic;
the actual library summary nevertheless reports no failed case. The message
is retained as a diagnostic, not relabelled as an additional libtest failure.

## Independent gates

| Gate | Exit | Wall seconds |
| --- | ---: | ---: |
| fmt | 0 | 14.785901 |
| clippy | 0 | 123.786295 |
| workspace-build | 0 | 63.460831 |
| diff-check | 0 | 0.054598 |

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
`d3085323677a3f773df6f870d83e2abd688ae4a9946c0a6ed4d1824ae2ab7290`. No source overlay or control hook was
used. See [source-and-environment.json](source-and-environment.json).

[authored.patch.json](authored.patch.json) preserves the exact two-file
fixture/documentation patch. This is a fresh qualification after train 45’s
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
[runner-provenance.json](runner-provenance.json) preserves the
byte-identical completed predecessor runner/support and the separate current
source preparation. Its unexecuted flags describe preparation, not this result.

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
