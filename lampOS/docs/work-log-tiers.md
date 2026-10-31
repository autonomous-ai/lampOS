# Tiers work log

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
