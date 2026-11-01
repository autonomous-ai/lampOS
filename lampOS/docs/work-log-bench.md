# Bench work log

## 31 October 2026: opt-in benchmark executable

The `bench` Cargo feature now gates the executable, so a default product
workspace build does not emit an unclassified deployment binary. All library
checks remain available by default. Local feature-enabled strict clippy
(`cargo clippy -q -p lamp-memory-bench --all-targets --features bench -- -D warnings`)
and tests (`cargo test -q -p lamp-memory-bench --features bench`) pass with the
same bounded target/job environment below: 32 passed, zero failed. Formatting
and the resource-bounded workspace build also pass. Bench-P3 on the Queue
requests explicit native ARM64/clippy feature coverage alongside Tiers.

## 31 October 2026: v2 runner, privacy fixture and Tiers smoke (#4745)

Ready for lamp-voice: no

Revision `e2479bb5a` adds nonblocking adapter pipes, ordered operations,
bounded background draining, a serial budgeted model worker, durable
pre-dispatch accounting hooks, strict v2 replies, fresh snapshots before
answers, and recent-history clearing/suppression from actual forgetting.
The registration is version 2, still pending human review: three fabricated
second-resident lines carry the default-owner observation, with three
wrong-person probes and an explicit inspection gate. No scored run exists.

On the Mac with Rust 1.96.0:

- From `lampOS/`, `cargo fmt --all --check`: pass.
- `CARGO_TARGET_DIR=../.harness/bench-target CARGO_BUILD_JOBS=2 cargo clippy -q -p lamp-memory-bench --all-targets -- -D warnings`: pass.
- `CARGO_TARGET_DIR=../.harness/bench-target CARGO_BUILD_JOBS=2 cargo test -q -p lamp-memory-bench`: 32 passed, zero failed.
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 cargo build -q --workspace`: pass on the final source tree. Those profile options bounded local disk use; only this task's target cache was cleaned with Cargo when free space fell below the Queue threshold.
- Two independent version-2 generations are byte-identical and the committed fixture validates.
- `target/debug/lamp-memory-bench smoke ../.harness/tiers-smoke-target/debug/lamp-memory-tiers-arm tiers df9af5447f20bb1560ee0c71f0470285122388f4 ../.harness/tiers-v2-smoke-1`: pass, twelve operations, zero model calls, no scores or missed recall/snapshot deadlines. The Tiers source-only copy was built with `--features bench` and that full revision in `LAMP_MEMORY_TIERS_REVISION`.

The ingestion-prefix regression demonstrably failed before its fix. Other
new tests cover blocked writes, incomplete/duplicate replies, late recall,
model deadlines and retained charges, slow pre-dispatch audit, v1 readability,
fresh memory after plain-language forgetting, and clearing all recent scopes
without retaining the forget confirmation.

Not shown: a full-month orchestrator, the three in-tree arms, speech/STT cache,
live transport, multi-turn rubric judging, scorecard, or baseline numbers.
The owner is still choosing live access. All recorded-output runs must be
labelled plumbing checks, never accuracy results. The current-product arm
stays partial until #4748's complete clock/drain/snapshot boundary is verified.

Docs #4752 is separately pushed at `eeae018aa`: contract v2 and Reviewer-three's
default-owner P2, keyed-fingerprint/export P3s. Final-head ARM64 41193871363
and clippy 41193873490 were requested; reads are pending. Prior-head checks
are not claimed for this head.

## 31 October 2026: benchmark core and pre-registration (#4745)

Ready for lamp-voice: no

Revision `d0836285d` adds the isolated Rust benchmark core and development
fixture. It has 30 days, 1,560 events, 200 questions, separate evaluator keys,
strict scorers, a recorded model client with cost reservations, and private
append-only run ledgers. No scored arm run or live model call preceded this commit.

Local checks on the Mac, Rust 1.96.0, from `lampOS/`:

- `cargo fmt --all --check`: pass.
- `CARGO_TARGET_DIR=../.harness/bench-target CARGO_BUILD_JOBS=2 cargo clippy -q -p lamp-memory-bench --all-targets -- -D warnings`: pass.
- `CARGO_TARGET_DIR=../.harness/bench-target CARGO_BUILD_JOBS=2 cargo test -q -p lamp-memory-bench`: 19 passed, zero failed.
- `CARGO_BUILD_JOBS=2 cargo build -q --workspace`: pass, including the final incremental build.
- Two independent `generate` runs produced byte-identical files; `validate`
  accepted the committed fixture. JSON examples, local links and whitespace
  checks passed.

Two new scorer regression cases failed before their fixes: an unbooked class
was incorrectly keyed as undecided, and a valid refusal to interrupt a call
was penalized as a recommendation. Both pass now. Other tests cover future
citations, DST calendar days, malformed wire frames, context byte limits,
ledger corruption/interruption, budget refusal before dispatch, and retaining
known/uncertain model charges.

Not shown: process orchestration, memory arms, real TTS/STT cache, live
provider calls, rubric-model invocation, scorecard, independent held-back
persona, human fixture/audit review, or any lamp measurement. Registration is
still marked draft pending human review. The runner and adapters are next.

Most likely wrong here: generated questions can be semantically unfair even
when chronology and hashes pass. A person must review the fictional month
and answer key before live evaluation. The current-product arm remains
partial until the test-only clock/drain hooks in #4748 are available.

## 31 October 2026: design and contract (#4744)

Ready for lamp-voice: no (#4752)

Docs PR #4752 is at `016d7574a`, rebased onto `3c080235b`. It documents the
approved tiered design, the complete benchmark method, fixed removal arms,
200 ms recall, 300 ms snapshots, persona timezones and pending sleep batches.
The first builder accepted the isolated experiment on the Queue, with product
memory, voice and deployment unchanged. The release exclusion stays in force.

Formatting, JSON examples and local links passed. Final-head ARM64 run
41190531360 and Linux clippy run 41190533666 were requested explicitly;
results and the two required reads are pending. Earlier-head results are not
claimed for this head. Readers are Friday and Reviewer-three.

## 31 October 2026: registration and proposals

The role was accepted and PR #4743 merged in train 461. Assigned tasks are
#4744 (docs) followed by #4745 (benchmark). No lamp is assigned.

- Bench-P1: the first builder accepted isolated memory experiments while
  retaining the product release exclusion.
- Bench-P2: test-only wall clock/timezone and drain/checkpoint hooks in voice's
  memory paths, assigned to Friday in #4748, with Tests for clock fixtures.
  They must be absent from product builds and unreachable from product flags
  or sockets. Restore must not revive a pre-forget snapshot over newer
  deletion state. Benchmark probes use private fictional child copies.
