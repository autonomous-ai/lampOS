# Continuing-answer gaps, 21 October 2026

This is the recount for #2602's second-route silence task. It uses the
timing-only exports of lamp-4ace's `ab1840base` and `ab1913po350` sessions
supplied by the first builder. Fourth has no lamp; no new lamp run,
recording or provider request was made.

The measured boundary is **zero PCM accepted while a reply is unfinished**,
not acoustic silence. The render rate is 24,000 frames per second. A gap
is over 300 ms only when its terminal receipt has more than 7,200 zero
frames. `started` and its `resumed` or `cancelled` receipt describe one gap.
A gap that resumed stays resumed even if the reply was cancelled later.
All origins are included, including spoken notices and missing or null origin,
as are listening gaps. This raw accepted-PCM report is not the coordinator's
`playback_gaps` counter, which applies origin and listening rules. The files
contain 15 and 13 null-origin cancelled terminal receipts respectively; these
remain paired by their nested owner and start timestamp.

## Recount

| Timing export | Started gaps | Resumed, all | Cancelled, all | Resumed >300 ms | Cancelled >300 ms | Replies with resumed >300 ms |
|---|---:|---:|---:|---:|---:|---:|
| `ab1840base` | 33 | 18 | 15 | 11 | 12 | 9 |
| `ab1913po350` | 32 | 19 | 13 | 10 | 13 | 10 |
| Total | 65 | 37 | 28 | **21** | **25** | 19 |

These are gap counts and distinct gap-bearing replies, not the count of
all answered turns. One reply in the first session had three resumed
gaps over 300 ms. A reply can also have a resumed gap and a later cancelled
gap, so the distinct-reply columns for those phases must not be added.

The earlier breakdown said 22 continuing gaps and 24 cancellations. It
counted the 24 cancellations matched to a yield in the journal, leaving
one cancelled gap without a journal match in the wrong denominator. The
terminal phases give **21 resumed and 25 cancelled**. The earlier cause
ranking is unchanged: 13 cue-to-answer gaps, six yielded then resumed,
one slow second speech piece on a reused connection, and one gap across a
tool round. Those causes came from the separate journal audit; the Rust
counter does not infer them from queue occupancy or a reply's final outcome.

## Inputs and reproduction

The exports are retained locally under
`~/lamp-voice-data/sessions/gaps-1021/`. Their identities are:

| Arm | File | JSONL records | SHA-256 |
|---|---|---:|---|
| `ab1840base` | `7d22bb8a22634c90a316b33da953842c.timing.jsonl` | 12689 | `b533ba764656fc0793afcdc99222bcad79993d9364553418cb5b18fa60f05cc1` |
| `ab1913po350` | `3afa5034d9e34e1a9b7a0edc27cb64d5.timing.jsonl` | 10694 | `9bf20e8986f9f1b2532f2ae325298d553428bf123573619f5e7b03bf16816708` |

From `lampOS/`, after building `lamp-voice-eval`:

```sh
target/debug/lamp-voice-eval answer-gaps \
  ~/lamp-voice-data/sessions/gaps-1021/7d22bb8a22634c90a316b33da953842c.timing.jsonl \
  ~/lamp-voice-data/sessions/gaps-1021/3afa5034d9e34e1a9b7a0edc27cb64d5.timing.jsonl \
  --json
```

The input may be a complete session's `events.jsonl` or a timing export
that retains the gap receipts. Missing, duplicate, conflicting or invalid
receipts must be reported as evidence problems, not interpreted as a clean
run. No transcript or journal text belongs in the report.
Even byte-identical duplicate starts or terminals exclude their gap identity
from paired counts rather than being deduplicated; raw phase counts remain.

## Reader checks and reporting time

[counts.json](counts.json) is the reader's unedited JSON output, invoked
from the exports' directory with the two bare filenames above. It agrees
with a separate raw terminal-phase/frame recount. All 65 starts pair with
exactly one terminal receipt; both files have zero malformed, duplicate,
conflicting, orphan or incomplete receipts.

The focused reader tests passed: eleven module tests and four CLI tests. They cover
the 300 ms boundary, full owner identity, repeated gaps within a reply,
later cancellation, notice/listening/null-origin pairing, identical duplicates,
mismatched and incomplete receipts, input bounds and exclusion of record text
from reports and errors. Four shared-predicate/coordinator tests and two existing
coordinator gap/listening tests also passed. The report and coordinator use
`PlaybackGap::is_valid_receipt` for the same frame, epoch and timestamp checks;
the runtime's counting policy remains separate and unchanged.

The full local evaluator gate passed **623 tests, zero failed and zero
ignored**: 378 library tests, seven binary tests and 238 integration tests.
The separate full voice gate passed **3,292 tests, zero failed and 14 ignored**:
3,021 library tests, one binary test, 260 integration tests and ten example
tests. Those complete suites, formatting, workspace Clippy with warnings denied
and the workspace build passed on base `b87b3e91a` before a clean rebase to
`4efcf4eb0`. The rebase preserved the patch and all three changed Rust file
hashes. The full-suite wall times were 122.958 s for the evaluator and
588.371 s for voice. From `lampOS/`, the exact commands were:

```sh
cargo fmt --all --check
cargo test -q -p lamp-voice --lib playback_gap_receipt --locked --offline
cargo test -q -p lamp-voice --lib accepted_gap_latches_discontinuous_delivery_and_old_gap_cannot_taint_new_reply --locked --offline
cargo test -q -p lamp-voice --lib a_listening_gaps_zeros_are_no_delivery_gap_unless_they_ran_past_its_hold --locked --offline
cargo test -q -p lamp-voice-eval --lib --test it answer_gaps --locked --offline
cargo test --locked --offline -p lamp-voice-eval --no-fail-fast -- --test-threads=1
cargo test --locked --offline -p lamp-voice --no-fail-fast -- --test-threads=1
cargo clippy -q --workspace --all-targets --locked --offline --message-format=short -- -D warnings
cargo build -q --workspace --locked --offline
```

After the rebase, formatting, all 21 focused checks (six shared/coordinator,
eleven reader and four CLI tests), workspace Clippy and the workspace build
passed again. The latter two took 65.642 s and 53.736 s respectively. The full
local suite counts above are from the earlier base, not a second full run on
the integrated base. Native ARM64 CI checks the published head. Cargo used
`--locked --offline`, four build jobs and `CARGO_INCREMENTAL=0`.

The first sandboxed evaluator attempt recorded 509 passes and 114 failures
with fixture socket binds denied by `Operation not permitted`. Its log remains
`review-eval-full.log`; the complete rerun with local socket access passed as
recorded in `review-eval-full-local.log`. The full voice log is
`review-voice-full-local.log`. These logs and exact command timings are in
`/private/tmp/fourth-answer-gaps-checks/` on the development Mac. The pull request
records its native ARM64 and workspace Clippy runs; those are the readiness
gates after publication.

The final reader's first and repeated outputs on the two private exports are
byte-identical to committed `counts.json`. It also reads the six timing slices
in [#2697](https://github.com/autonomous-ai/lampOS/pull/2697): 201 records and
12 receipts produce exactly six resumed gaps over 300 ms, zero cancelled gaps,
one pair per file and complete evidence in all six files. That check took
17.218 ms; its output is `review-post-rebase-yield-slices.json` in the log
directory above.

[latency.json](latency.json) holds the measured reporting time for both
whole exports: 23,383 JSONL records, 130 gap records and 8,173,590 bytes.
On the development Mac, the debug executable took **329.098 ms** on its
first invocation over these inputs and **320.181 ms** on the immediate
repeat in the original implementation measurement. With the shared predicate
review fixes before the final rebase, it took **1,339.043 ms first / 322.414 ms
repeat**. That first launch **missed the one-second reporting target** and
remains a limitation; a warmed pass does not replace it. Both review outputs
were byte-identical to `counts.json`. The boundary is the host's
monotonic time immediately before launching the child through captured
JSON output and child exit. It includes file reads, parsing and output;
the disk cache was not flushed. This is command responsiveness, not a
before/after improvement in the lamp's voice.

The new post-rebase build also missed the same target on its first invocation:
**1,303.071 ms first / 316.480 ms repeat**. Child user/system CPU time was
300.775/5.711 ms first and 304.402/8.939 ms on repeat, measured from
`resource.getrusage(RUSAGE_CHILDREN)` differences. Remaining wall time is
unassigned; these measurements do not establish a startup or scheduling cause.
Each build was measured once and immediately repeated once. The final debug
binary is 36,089,432 bytes with SHA-256
`b4688064cc61de138836850ee423ca8167a93517b461dd084da2543cb8c7cf1b`.

## What was not shown

This recount does not measure an acoustic gap, the quality of a response,
or the effect of moving the cue. The first builder still runs the paired
lamp-4ace comparison at 1500 and 3000 ms in the same hour, counting gaps
that resume. A new connection was not the cause of either remaining
non-yield gap, so this change does not add a connection pool or alter
speech ordering, cancellation, buffering or runtime deadlines.
