# Cue peer-closure fixture native gate on 810e

The one prescribed local gate completed with **7,164 passed, 1 failed and 32 ignored** across its two test phases. Native session 65126 exited **1**. Source was exact base `810e05109bced3beca88a5b602930cfed5fba4e5` plus the two declared candidate files; no candidate commit existed at execution time. The changed peer-loss case's actual result is **ok**. Every observed failure remains unwaived.

| Phase | Passed | Failed | Ignored | Filtered | Exit | Wall seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Full Voice, default parallel | 3933 | 1 | 15 | 0 | 101 | 366.360908 |
| All eleven readers, default parallel | 3231 | 0 | 17 | 0 | 0 | 1262.25198 |

Both test phases compiled successfully and every actual target/case result binds to the separately listed binary census. There are no incomplete or unrun targets. Unsupported custom Update harness checks are counted as ignored on Mac, not passed. [Census and outcomes](census-and-outcomes.json) retain each case row and artifact binding; a compiled inventory is not execution. [Changed-case result](changed-case.json) is separate from the earlier fixed controls.

## Retained failures

- `lamp-voice:test:it`: `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`.

[Failures](failures.json) retain each complete diagnostic with original raw stream offset, byte length and SHA-256. [Light-call boundary](light-call-boundary.json) identifies the checked sorted middle at index 10 and unchanged strict 25 ms bound when that case fails; the maximum is descriptive only. These are actual candidate results. No unchanged-source or prior timing observation is presented as a causal waiver. No failed case or full test command was repeated.

## Independent gates

| Gate | Exit | Wall seconds |
| --- | ---: | ---: |
| fmt | 0 | 11.860601 |
| clippy | 0 | 82.052001 |
| workspace-build | 0 | 68.829905 |
| diff-check | 0 | 0.05645 |

The two test commands use `--no-fail-fast` and default libtest parallelism with `RUST_TEST_THREADS` unset. Complete ordinary test failures remain failed while independent planned gates continue. [Commands](command-results.json) retain exact arguments, watchdogs, exits, durations and stream hashes. [Frozen plan](gate-plan.json) and [dependency closure](dependency-closure.json) cover Voice plus all eleven current reverse readers. This Mac gate does not substitute for required native Linux CI or current-parent review. The separate exact-source Linux amd64 formatting check passed before this gate.

## Source and boundaries

All 10,033 tracked repository paths, including 6,687 lampOS paths, docs, repository lamp fixtures, file modes and symlink values, were pinned. Input mtimes were refreshed before Cargo with bytes/modes unchanged. Source identity was checked before and after every stage, ending at the same aggregate `3e1f5da09ccd82ebce4cbec531c9e0e1cd0abc4baf301faf4e4abdfa7d7bc61f`. [Source and environment](source-and-environment.json) retain the proof; the bulk map remains external. Only `fixture_provider.rs` and its existing English documentation differ from the base. [Exact patch](authored.patch.json) and [carry](carry.json) show the reviewed patch and both authored blobs remained byte-identical from the earlier control base.

The shipping fixture uses fresh nonblocking connect-only probes to establish receiver closure, accepts only ConnectionRefused, drops each probe before its 1 ms poll and has a five-second setup watchdog. It sends no cue and preserves the original single scan, stale-cue half, trace/timestamps and all five assertions. The controlled-copy experiment proves a socket-lifetime mechanism, not the cause of the historical failure. Product code, identity, cancellation and deadlines are unchanged.

Rust host is `x86_64-apple-darwin`; captured artifacts identify Mach-O x86_64. Physical hardware ISA was not independently measured. The launch was explicitly authorized with local fixture socket access; both preflight binds passed before Cargo. Jobs 4, debug 0, incremental off, offline/locked and the exclusive existing MAIN target were used. No ARM build, device action or unrelated workload was launched by this runner. MAIN was released only at terminal.

## Representation

[Native streams](native-streams.json) retain exact stderr and non-Cargo stdout. Cargo stdout removes only parsed Cargo build records using explicit byte ranges; every human test row, summary and diagnostic remains byte-identical. Original and retained hashes/lengths are separate, so a projection is not called unchanged full stdout. [Artifact bindings](artifact-bindings.json) capture actual compiler identities before shared-target reuse. [External raw index](external-raw-index.json) lists complete raw streams and inventory digests; local paths do not promise remote availability. No binaries, Python, whole Cargo message streams or bulk source maps are published. JSON envelopes preserve raw whitespace without Markdown whitespace exceptions.
