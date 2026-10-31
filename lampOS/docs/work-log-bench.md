# Bench work log

## 31 October 2026: benchmark core and pre-registration (#4745)

Ready for lamp-voice: no

Revision `d0836285d` adds the isolated Rust benchmark core and development
fixture. It has 30 days, 1,560 events, 200 questions, separate evaluator keys,
strict scorers, a recorded model client with cost reservations, and private
append-only run ledgers. No arm result or model call preceded this commit.

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
