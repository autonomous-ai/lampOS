# Relative-reminder catalog: isolated host evidence

The implementation passed **54 test executions / 0 failures / 0 ignored**, a workspace build, and **72 default-off cases / 144 exact stdout comparisons**. Separately, pristine integration source plus only the new regression produced the expected **Cargo exit 101, 0 passed / 1 failed**, at `missing timer guidance: relative reminder`. This failed negative control remains separate from positive totals.

Base: `cf2f5b43c8113a704324c2743af3cd92bbc4fdda`. Copy-time task HEAD: `365e36dbcd78ff247adf30b2d18afd4d9d6f1065`. All **711** frozen Rust/manifest/toolchain hashes match source-map SHA256 `517af82a7dde4930ccacc48934bd635dd59df983b9d410e9b8c49e2917822c1c`; the supplied map records no HEAD. The task changes two catalog descriptions, one test and three docs, with no timing or schema change. The test proves published guidance, not actual model tool choice or lamp timing.

All three independent copies contain 1,025 source files: baseline 19,511,879 bytes, negative control 19,513,347, implementation 19,517,487. The negative control adds only 1,468 bytes / 49 lines in one test file; removing that block restores pristine bytes. All `docs/verification` content is excluded. Metadata proved 35 local package manifests contained within each copy. The audit rechecked 4,005 historical originals; mutable parent prose is excluded from this history claim.

## Execution

Host: `Darwin 25.6.0`, `x86_64`, processor string `i386`; `rustc 1.96.0 (ac68faa20 2026-05-25)`. Cargo used offline/locked mode and separate target directories. Local Unix sockets were permitted. No live provider, account, network or lamp operation occurred. Times span monotonic process launch through exit and log closure, excluding audits.

Negative-control test: 15.217849251 seconds, Cargo 101. Clean baseline helper build: 30.267960342 seconds. Metadata: 0.357131471 seconds baseline / 0.480253125 negative control / 0.376924623 implementation.

| Implementation stage | Exit | Seconds | Passed / failed / ignored |
| --- | --- | --- | --- |
| workspace-build-first | 0 | 80.881996933 | build |
| helper-build-first | 0 | 25.722381692 | build |
| actions-tests-first | 0 | 29.395052909 | 34 / 0 / 0 |
| voice-tools-tests-first | 0 | 28.195564626 | 20 / 0 / 0 |

Sequential commands ran in their respective copies:

```sh
cargo metadata --offline --locked --format-version 1
cargo test --offline --locked -p lamp-actions --test schedule published_catalog_routes_relative_reminders_to_elapsed_timers -- --exact --test-threads=1
cargo build --offline --locked -p lamp-actions -p lamp-voice --example schedule_off_actions --example schedule_off_voice
cargo metadata --offline --locked --format-version 1
cargo build --offline --locked --workspace
cargo metadata --offline --locked --format-version 1
cargo build --offline --locked -p lamp-actions -p lamp-voice --example schedule_off_actions --example schedule_off_voice
cargo test --offline --locked -p lamp-actions --all-targets -- --test-threads=1
cargo test --offline --locked -p lamp-voice --lib tools -- --test-threads=1
```

Actions reported `published_catalog_routes_relative_reminders_to_elapsed_timers ... ok`. Voice uses the broader `tools` filter. No retries, weakened assertions or skipped planned stages. Full affected-package gates, native CI and lamp qualification are separate parent work.

## Byte comparison and retained originals

Each `ordinary` and `tasks` profile independently sets both Schedule flags to absent, `off`, `ON`, `true`, `1`, or ` on `: 72 cases. Both copies use the same fresh baseline-owned catalog fixture path. Comparison uses raw bytes without normalization: **144 stdout readings / 554,076 bytes and 144 empty stderr streams per copy**; 3 unique stdout payloads total 14,197 bytes.

The unchanged helper pair totals **179 + 1,576 = 1,755 bytes per helper copy**; the negative-control copy has no helpers. Its boundary is the public default Actions catalog and Voice arguments, declarations, fixed existing routes and Written serialization. It does not prove model choice, enabled-catalog behavior, elapsed delivery, direct `check_agent_task` routing or End.sound serialization. The separate regression covers enabled published guidance. Invalid relative Schedule and missing Memory/Tasks fixture paths remain absent.

- [Expected baseline failure](records/negative-control-completed.json) and [raw assertion](negative-control/expected-regression-failure-first.txt)
- [Clean baseline](records/baseline-completed.json) and [implementation](records/implementation-completed.json) reports
- [Execution audit](records/execution-audit-first.json) and [preparation audit](records/preparation-audit.json)
- [All 288 stdout / 288 stderr reading mappings](comparison-readings.json)
- [Original-path index](retention-index.json), [artifact identities](artifact-identities.json), [excluded references](excluded-references.json)

All current reports, metadata stdout/stderr from three copies, command logs and comparison streams retain their exact original bytes. Deduplication requires hash and byte equality. The UTF-8-only export excludes source trees, Python, archives, fixtures and binaries; their hashes remain where relevant. The one test-only patch is textual provenance. `SHA256SUMS.txt` covers every file except itself, including the index. These host checks make no whole-gate, model-quality or lamp-readiness claim.
