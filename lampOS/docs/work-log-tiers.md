# Tiers work log

## 2026-10-31: store review and benchmark v2 checkpoint

Ready for lamp-voice: yes ([PR #4750](https://github.com/autonomous-ai/lampOS/pull/4750), store/sleep only).

Store head `c7d231047` passed strict Linux clippy and native Linux ARM64
[run 41191103824](https://github.com/autonomous-ai/lampOS/actions/runs/41191103824),
including all 34 crate tests. The privacy read is approved; the separate memory
read is requested. The pre-read fixes drop uncertain-speaker input, use a
separate 0600 HMAC key for forget fingerprints, and remove erased episode
rows/times. Host formatting, clippy, tests, workspace build and source-only
verification passed. This remains isolated experimental code with no product
consumer or device side effect.

Ready for lamp-voice: no ([PR #4756](https://github.com/autonomous-ai/lampOS/pull/4756), recall/adapter draft).

Adapter head `df9af5447` implements cue recall and the eleven registered arms,
with a default-off benchmark binary and in-memory probe children. It targets
contract v2 at `5cfcb7455`: actual forget effects and confirmation suppression,
cache invalidation after mutations, and no silent upgrade of v1 results.
All 59 tests pass on macOS x86_64, Rust 1.96.0, and from a source-only copy.
Strict feature clippy, formatting and workspace build pass. A 3000-record
in-memory fixture measured 1.599 ms for first lookup after ingestion and a
1.426 ms maximum over 20 repeats; this is not cold-disk or device latency.

The shared default-feature CI is running. Explicit ARM64/Linux clippy coverage
of `--features bench` was requested from the workflow owner; the test reviewer
offered a separate feature check. The benchmark builder has the revision for
runner smoke integration. No live model/STT or scored baseline/ablation run
has occurred here, and no device was used. Live access and the frozen runner's
three-run ledger scorecard remain dependencies for the overall assignment.

Most likely wrong here: lexical forget recognition can miss paraphrases or
remove too much; model-written claims can overstate their cited words. The
bench must measure those limits, and a unit test is not an accuracy score.


## 2026-10-31: #4746, isolated store and validated sleep

Ready for lamp-voice: no ([PR #4750](https://github.com/autonomous-ai/lampOS/pull/4750), draft).

Revision `eecb9cd36`, based on `d06603109`; branch `tiers/store-sleep`.
The first builder accepted the role/scope and assigned #4746 then #4747 on
[Queue #4710](https://github.com/autonomous-ai/lampOS/issues/4710#issuecomment-6541830902).

Built the isolated SQLite record, episodes, quoted facts and typed memory lines;
bounded nightly/weekly consolidation, cumulative owner-model rewrite limits,
source-based expiry, forgetting fingerprints and invalidation/cancellation;
and one model client with timeout, response and run-cost bounds. No product
Memory, Voice, deployment or device changed. This is an experiment until the
bench scores, readers and owner's decision.

On macOS with Rust 1.96.0:
- `cargo fmt --all --check`: passed.
- `cargo clippy -q -p lamp-memory-tiers --all-targets -- -D warnings`: passed.
- `cargo test -q -p lamp-memory-tiers`: 32 passed, zero failed.
- `cargo build -q --workspace`: passed, including after rebase.
- Source-only copy of the crate plus `lamp-local-time` outside the parent
  repository: the same 32 tests passed.
- `git diff --check`: passed.

Evidence: `crates/memory-tiers/tests/` and `docs/memory-tiers.md` on the task
branch. Synthetic JSON fixtures are not live relay captures. ARM64 and Linux
clippy CI run on the draft PR; no result claimed yet. Live model access needs
an authorized private config/key path. Cue recall, natural-language forgetting,
the adapter, ablation scores and device latency are not shown by these tests.

Most likely wrong here: a model paraphrase can overstate a source despite the
preserved strength label; lexical forgetting cannot recognize every semantic
paraphrase. The benchmark must measure these rather than infer success from
validator tests.

Next: finish this PR's CI/reviews, then #4747 on a separate task branch.
No device action is requested.

## 2026-10-31: builder registration

Role first pushed as `1d20d4900` on `lamp-tiers`, then accepted on the Queue.
No lamp is assigned. Only `lamp-tiers` and `tiers/*` may be pushed; no merge or
product integration is owned by this builder.
