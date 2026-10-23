# #3218 current-head native gate — failed observations retained

The single planned native gate on phone head
`4e1e00e5298b31f797fd90f58d2b439ff2b4e32e`, parent
`cf462adb04f64d699c226dd3b1781c0ad927068f`, completed with **7,150 passed,
3 failed and 32 ignored**. The overall exit is **1**. The owned phone case
`messages::tests::a_text_never_waits_on_the_news_for_the_phone` passed.
This receipt does not clear the three failed observations or claim a causal
baseline comparison.

| Phase | Passed | Failed | Ignored | Filtered | Actual exit | Wall seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Full default-parallel Voice | 3,923 | 1 | 15 | 0 | 101 | 494.044769 |
| All 11 readers, default parallel | 3,227 | 2 | 17 | 0 | 101 | 1466.909509 |

Both phases compiled successfully and have complete target/case census
bindings. Voice has six completed target summaries; readers have 68. There
are no incomplete or unrun targets. The two unsupported custom Update
harness checks are counted as ignored on Mac, not as passes. Actual per-target
results, ignored cases, artifact hashes and all case identities appear in
[census-and-outcomes.json](census-and-outcomes.json); the owned case's actual
row is in [owned-phone-case.json](owned-phone-case.json).

## Three failures

- Voice integration `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`:
  `crates/voice/tests/it/light_call.rs:90` checks the sorted middle of 20 calls,
  index 10, strictly below 25 ms. The checked value was **53.211259 ms**. The
  maximum, 119.695347 ms, is context only. All 20 calls returned `Outcome::Done`;
  the ring was joined and fixture directory removed before the timing assert.
- Expression runtime `enabled_output_uses_a_new_session_and_expires_after_input_loss`:
  `crates/expression/tests/runtime.rs:127`, `fresh voice and joints`.
- Expression runtime `only_a_service_with_the_offline_face_draws_listening_unavailable_when_offline`:
  `crates/expression/tests/runtime.rs:1048`, `a light request`.

[failures.json](failures.json) preserves every full raw failure block as exact
UTF-8, its byte offset/length/hash in the original stream, every light-call
timing and the failed binary identity. The light-call source is byte-identical
to the parent; that fact alone does not establish causality or waive its
failure. No failed case or full command was retried. No assertion or timeout
was changed. Any later clean-base control must retain its own source and run
identity and cannot relabel this candidate observation.

## Independent completed gates

| Gate | Exit | Wall seconds |
| --- | ---: | ---: |
| `cargo fmt --all --check` | 0 | 12.551482 |
| Strict all-target Clippy for 12 packages | 0 | 114.273108 |
| `cargo build --offline --locked --workspace --message-format=json-render-diagnostics` | 0 | 75.528290 |
| `git diff --check` | 0 | 0.055400 |

The exact argument arrays, deadlines, times and raw-stream hashes are in
[command-results.json](command-results.json); [gate-plan.json](gate-plan.json)
retains the frozen plan. Both test commands used `--no-fail-fast`, ordinary
libtest default parallelism and `RUST_TEST_THREADS` unset. Fully bound ordinary
exit 101 observations remained failed while independent later gates ran.
The final native tool session 3533 exited 1; CPU/MAIN were released before any
receipt packaging or separate baseline work.

## Exact source and environment

The clean committed head was guarded before and after every gate. All 10,026
tracked repository paths, including 6,680 lampOS paths, docs, root lamp data
fixtures, Git executable modes and the literal symlink target, were hashed.
Input mtimes were refreshed before Cargo and equal before/after byte aggregates
proved freshness without content edits. The final aggregate equals the initial
`8f8bd4d3e9d9b5c3aba92f3df242444c6f43acb79007a0b6926db57285df1efd`.
[source-and-environment.json](source-and-environment.json) records these checks,
commit/tree/parent identities and controlled environment. No source hook or
worktree overlay was used.

The only owned diff from the parent is the phone fixture in
`lampOS/crates/voice/src/messages.rs` and `lampOS/docs/messages.md`; its exact
patch is [authored.patch.json](authored.patch.json). The static all-workspace
manifest audit found the same 12-package conservative Voice reader closure,
with no package or selected-manifest hash change from the earlier e5 plan:
body, cloud, expression, keeper, lean, memory, peek, setup, skills, update,
voice and voice-eval, each with the `lamp-` prefix. See
[dependency-closure.json](dependency-closure.json). No old test total was used
as a count constraint. The earlier e5 plan and observations remain separate.

The authorized tool launch used `sandbox_permissions=require_escalated`.
Before Cargo, both the private Unix datagram bind and loopback TCP bind/listen
passed and were closed. [preflight-revision.json](preflight-revision.json)
records the narrow revision and original/new prepared hashes. No default-
sandbox attempt or retry occurred for this exact-head phone gate.

Rust is 1.96.0 with host `x86_64-apple-darwin`; test artifact `file` identities
are recorded with the census. The captured OS reports Darwin x86_64; physical
hardware ISA was not independently measured. The environment is jobs 4,
dev/test debug 0, incremental 0, offline/locked, using only the existing
`/private/tmp/tests-codex-1569-target`. No ARM build or device access occurred.
These Mac observations do not substitute for Linux/ARM CI or prove device
performance. No product behavior or privacy case is claimed newly qualified
beyond the actual cases listed in this gate.

## Evidence representation

[native-streams.json](native-streams.json) includes exact complete stderr and
non-Cargo stdout. Cargo stdout uses an explicit byte-range projection removing
only parsed Cargo build JSON records; every test row, summary and failure
message is preserved byte-for-byte. Each projection records the full original
stream hash/length and every retained range plus its own hash. It is not called
the unchanged full stdout. [artifact-bindings.json](artifact-bindings.json)
retains actual build artifact identities before MAIN reuse.

All original raw streams, inventories, source maps and execution records remain
external at [external-raw-index.json](external-raw-index.json)'s local paths.
Those paths are not a remote availability promise. The publication includes all
failure bytes and the complete retained test output/case matrix; it excludes
executables, Python orchestration, full Cargo JSON dumps, whole source exports
and the large tracked-input map. JSON encoding preserves terminal newlines and
whitespace in raw streams without adding invalid Markdown whitespace.

[validation.json](validation.json) records byte/hash, count, link and whitespace
checks. [manifest.json](manifest.json) binds every publication payload file.
