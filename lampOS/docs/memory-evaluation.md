# Memory retrieval: the first public benchmark

On 2026-10-14, Fourth ran the complete cleaned LongMemEval-S dataset on its
development machine. Ranked FTS5 search found every required session in the
first five results on **81.86%** of the official user-only questions, versus
**69.69%** for the existing whole-word scoring rule. The corresponding
both-speaker result was **82.13% versus 66.38%**. These are evidence-retrieval
scores. No model answered a question, and nothing was measured on the lamp.

[The recorded results](memory-retrieval-results.json) preserve the complete
summary as compact JSON, including every question type, cutoff, timing
distribution, exclusion, machine label and dataset hash. The tables below
round that artifact. [The independent verification
result](memory-retrieval-verification.json) records the separate score check.

## What exists and what we take

LongMemEval supplies timestamped conversation histories and evidence labels
for testing long-term memory. Its authors now recommend the cleaned dataset,
which removes interfering history sessions. The dataset card identifies it
as English and MIT-licensed. We use that release, retaining its full histories
and source labels. [Pinned dataset and release
description](https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned/blob/98d7416c24c778c2fee6e6f3006e7a073259d48f/README.md)

The official retrieval runner supports session and user-turn units. Its
published BM25 implementation uses `rank_bm25.BM25Okapi` with literal-space
tokenization. The comparison here instead measures the lamp's whole-word
rule against **SQLite FTS5 BM25**; it does not reproduce that published BM25
baseline. [Pinned official
runner](https://github.com/xiaowu0162/LongMemEval/blob/9e0b455f4ef0e2ab8f2e582289761153549043fc/src/retrieval/run_retrieval.py)

FTS5 supplies ranked lexical retrieval without an embedding model or service.
Its `unicode61 remove_diacritics 2` tokenizer handles compound Vietnamese
diacritics missed by the default mode. BM25 assigns better matches lower
scores, with fixed `k1=1.2` and `b=0.75`. The shared search code additionally
normalizes NFC and case and folds `đ` to `d`; the original text remains
unchanged. These are search normalization choices, not evidence of Vietnamese
semantic understanding. [SQLite's tokenizer and ranking
documentation](https://www.sqlite.org/fts5.html)

## Reproduce the run

The input is `longmemeval_s_cleaned.json`, not the original uncleaned S file
or LongMemEval-V2. It is a JSON array of 500 questions. Parallel arrays hold
history session IDs, timestamps and message lists; messages contain `role`,
`content` and optional `has_answer` labels. `answer` has both string and
integer values and is deliberately ignored by the evaluator.

- Dataset revision: `98d7416c24c778c2fee6e6f3006e7a073259d48f`.
- File size: **277,383,467 bytes**.
- SHA-256: `d6f21ea9d60a0d56f34a05b609c79c88a451d2ae03597821ea3d5a9678c3a442`.
- Audited upstream evaluator revision: `9e0b455f4ef0e2ab8f2e582289761153549043fc`.

Download and check the public file, without an account or key:

```sh
mkdir -p /private/tmp/lamp-fourth-research
curl --fail --location --silent --show-error \
  https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned/resolve/98d7416c24c778c2fee6e6f3006e7a073259d48f/longmemeval_s_cleaned.json \
  -o /private/tmp/lamp-fourth-research/longmemeval_s_cleaned.json
shasum -a 256 /private/tmp/lamp-fourth-research/longmemeval_s_cleaned.json
```

The following commands were run from `lampOS/`. This machine's Cargo
executable was `/Users/autonomous/.cargo/bin/cargo`; another installation can
use its own `cargo` executable. The evaluator creates the per-query output
with exclusive creation, so use a new filename when repeating the run.

```sh
/Users/autonomous/.cargo/bin/cargo build -p lamp-memory --bin lamp-memory-eval --release
target/release/lamp-memory-eval \
  /private/tmp/lamp-fourth-research/longmemeval_s_cleaned.json \
  --units both --repeats 3 \
  --machine 'Fourth host: macOS 26.6.2 x86_64 Intel i7-10700K, 16 logical CPUs, 64 GiB; not the lamp' \
  --per-query /private/tmp/lamp-fourth-research/eval-query-results.jsonl \
  > /private/tmp/lamp-fourth-research/eval-summary.json \
  2> /private/tmp/lamp-fourth-research/eval-run.log
```

The successful run selected all 500 questions and produced 1,778 scored
question/track records. Each query ran three times. The executable was a
release build with Rust 1.96.0 and bundled SQLite 3.53.2. The machine was
macOS 26.6.2, x86_64 Intel i7-10700K, 16 logical CPUs and 64 GiB RAM. The
evaluation loop took **58.598 seconds**; that includes corpus construction,
indexing, repetitions, metric calculation and optional result writes, but
excludes initial dataset reading/validation and final summary encoding.

## Protocol and denominators

Every question gets a fresh index. Session units concatenate included
messages with spaces; turn units are individual messages, not the lamp's
paired exchanges. Timestamps determine newer-first ties but are not indexed
terms. Labels, answers, question types and session IDs never enter indexed
text. Only the question supplies query terms.

The whole-word baseline uses the existing lowercasing and alphanumeric
boundaries, counts distinct matched query words, and prefers newer entries
on ties. Both methods use the first sixteen distinct words of at least two
characters. FTS5 uses normalized text and quoted literal OR terms, so a
question cannot inject FTS operators. Scores are evaluated in relevance
order before any chronological presentation to a reader. This comparison
changes tokenization and normalization as well as ranking; it does not
isolate BM25's contribution.

**User-only tracks: 419 questions.** The upstream runner excludes the 30
questions whose IDs contain `_abs`, then excludes 51 questions without a
user-side `has_answer=true` target. All 51 belong to
`single-session-assistant`. Session targets require an ID containing
`answer` and a positively labeled user message; turn targets require that
particular user message. The
upstream print script excludes only abstentions, so it can report a
different denominator from the runner. This report follows the runner's
filter. [Runner
filter](https://github.com/xiaowu0162/LongMemEval/blob/9e0b455f4ef0e2ab8f2e582289761153549043fc/src/retrieval/run_retrieval.py),
[separate print
script](https://github.com/xiaowu0162/LongMemEval/blob/9e0b455f4ef0e2ab8f2e582289761153549043fc/src/evaluation/print_retrieval_metrics.py)

**Both-speaker tracks: 470 questions.** These separately defined tracks
exclude the same 30 abstentions and retain assistant-only evidence. Session
targets use `answer_session_ids`; turn targets use positive user or assistant
labels inside those sessions. This is not the official user-only protocol.
In the downloaded file, user-derived session targets differ from
`answer_session_ids` on 83 answerable questions, so changing label definitions
silently would change the task.

`recall_all@k` is one only when all required target IDs appear within the
first `k` results; `recall_any@k` needs one. The JSON also includes fractional
target recall. nDCG preserves the upstream discounts **1, 1, 1/log2(3), …**;
the first two positions have equal weight. This differs from the common
`log2(rank+1)` convention. No upstream turn-to-session conversion is claimed:
turn scores here require actual labeled turns. [Official scoring
functions](https://github.com/xiaowu0162/LongMemEval/blob/9e0b455f4ef0e2ab8f2e582289761153549043fc/src/retrieval/eval_utils.py)

## Retrieval results

Each result cell below is **recall-all / recall-any / nDCG**, all expressed
as percentages. “Words” means the preserved whole-word baseline. “User”
means the official user-only filter described above.

| Corpus | Method | Questions | @1 | @5 | @10 |
|---|---|---:|---|---|---|
| User sessions | Words | 419 | 17.18 / 72.08 / 72.08 | 69.69 / 89.50 / 74.18 | 80.19 / 94.03 / 76.99 |
| User sessions | FTS5 | 419 | 22.20 / 81.86 / 81.86 | 81.86 / 94.27 / 85.31 | 88.31 / 96.42 / 86.87 |
| Both-speaker sessions | Words | 470 | 16.60 / 66.81 / 66.81 | 66.38 / 87.02 / 71.43 | 78.51 / 92.77 / 74.97 |
| Both-speaker sessions | FTS5 | 470 | 28.94 / 85.32 / 85.32 | 82.13 / 96.38 / 87.29 | 89.36 / 98.09 / 88.85 |
| User turns | Words | 419 | 11.46 / 39.62 / 39.62 | 45.58 / 72.08 / 50.06 | 55.61 / 81.86 / 53.52 |
| User turns | FTS5 | 419 | 16.47 / 57.76 / 57.76 | 57.28 / 82.58 / 63.04 | 69.21 / 87.35 / 66.28 |
| Both-speaker turns | Words | 470 | 6.60 / 18.72 / 18.72 | 26.17 / 46.38 / 29.44 | 35.96 / 58.30 / 33.14 |
| Both-speaker turns | FTS5 | 470 | 16.60 / 48.51 / 48.51 | 52.98 / 77.23 / 56.86 | 65.32 / 84.26 / 60.42 |

The six question types below show **recall-all@5, Words → FTS5**. Counts
are the same in both speaker scopes except assistant questions, where
`5 / 56` means user-only / both-speaker. All other metrics at all three
cutoffs, by question type, are in the JSON artifact.

| Question type | Questions | User sessions | Both-speaker sessions | User turns | Both-speaker turns |
|---|---:|---|---|---|---|
| single-session-user | 64 | 98.44 → 100.00 | 93.75 → 100.00 | 84.38 → 92.19 | 64.06 → 90.62 |
| single-session-assistant | 5 / 56 | 60.00 → 100.00 | 57.14 → 94.64 | 60.00 → 60.00 | 37.50 → 67.86 |
| single-session-preference | 30 | 50.00 → 70.00 | 43.33 → 83.33 | 13.33 → 30.00 | 6.67 → 26.67 |
| multi-session | 121 | 47.93 → 70.25 | 52.89 → 66.94 | 23.14 → 33.06 | 4.96 → 21.49 |
| temporal-reasoning | 127 | 70.08 → 77.17 | 64.57 → 73.23 | 42.52 → 58.27 | 18.11 → 52.76 |
| knowledge-update | 72 | 88.89 → 97.22 | 84.72 → 97.22 | 66.67 → 76.39 | 41.67 → 72.22 |

Aggregate gains do not mean every ranking improved. At recall-all@5, FTS5
gained/lost **55/4** user-session questions, **83/9** both-speaker session
questions, **62/13** user-turn questions and **128/2** both-speaker turn
questions. User-session knowledge-update recall-all@1 fell from 2.78% to
1.39%. Both-speaker multi-session recall-any@5 fell from 96.69% to 95.87%;
user-turn temporal recall-any@10 fell from 86.61% to 84.25%. These failures
remain in the retained per-query results.

## Time and returned text

Times below are milliseconds on the stated development host. Query timing
covers normalization and ranking to IDs in memory. It excludes index
construction, reading the journal, sockets, response encoding and model
work. First-query measurements follow index construction; repeated
measurements are the next two calls. Neither is a cold operating-system
cache measurement. Index construction includes opening SQLite, normalizing
bodies and inserting documents, but excludes building the corpus strings.

Query pairs show **Words → FTS5**. Index pairs show **FTS5 p50 / p95**.

| Corpus | First query p50 | Repeated query p50 | Repeated query p95 | Index build p50 / p95 |
|---|---|---|---|---|
| User sessions | 0.353 → 0.381 | 0.344 → 0.327 | 0.506 → 0.624 | 5.671 / 7.025 |
| Both-speaker sessions | 2.928 → 0.495 | 2.947 → 0.419 | 3.473 → 0.914 | 29.901 / 30.965 |
| User turns | 0.375 → 0.496 | 0.367 → 0.424 | 0.523 → 0.680 | 11.688 / 13.193 |
| Both-speaker turns | 2.716 → 0.839 | 2.727 → 0.751 | 3.240 → 1.232 | 52.913 / 56.412 |

There are 419 or 470 first-query samples and twice as many repeated samples
per method/track. FTS5 was slower for user-turn queries and for several
user-session latency measures. A fresh index costs construction plus the
first query. The largest measured index build was 72.975 ms; that is not a
bound for a live store with a different amount of history.

The full-context reference returns all indexed text and therefore covers
all its targets: **100% retrieval recall**, with no reader or answer score.
Mean full-context sizes were 61,683 bytes for user sessions and 490,024 bytes
for both-speaker sessions. FTS5's first five sessions averaged 8,185 and
62,818 bytes respectively; its first five turns averaged 1,311 and 4,865
bytes. These are original UTF-8 text bytes, not tokens or measured network
egress. Several averages exceed the live default 4,096-byte response cap.

## Checks and retained failures

The evaluator's eight unit tests and ten retrieval integration/CLI tests
passed. They cover metric examples, partial and missing evidence, label
isolation, duplicate occurrences, abstention exclusions, mixed answer types,
UTF-8 byte counts, literal queries, Vietnamese composed/decomposed text,
rank ties, corpus isolation and output-file protection. Focused Clippy and
format checks passed. From `lampOS/`, the focused Cargo commands were:

```sh
/Users/autonomous/.cargo/bin/cargo test -p lamp-memory --bin lamp-memory-eval --test retrieval
/Users/autonomous/.cargo/bin/cargo clippy -p lamp-memory --bin lamp-memory-eval --test retrieval -- -D warnings
```

A separate Python calculator reread the public labels and the 1,778
recorded rankings. It independently recomputed recall-all, recall-any,
fractional recall and nDCG at each cutoff for both methods: **42,672 values
agreed within 1e-12**. It did not import the Rust scorer or execute the
upstream Python scorer. Its script, raw per-query results and logs remain
under `/private/tmp/lamp-fourth-research` on Fourth's machine; the small
verification artifact records the completed check. This check establishes
arithmetic agreement, not the correctness of every benchmark annotation.

The first full-data attempt stopped before scoring at question `58bf7951`
because the validator rejected a repeated session ID. Audit found 13 such
questions, all with duplicated distractors. The corrected evaluator retains
those occurrences, as upstream does; recall uses unique target IDs, while
nDCG counts corpus occurrences. A regression test covers this distinction.
The failed run is retained as `eval-run-failed-duplicate-session.log` in the
same temporary directory. That attempt's optional `/usr/bin/time -l`
resource report also hit a sandboxed `sysctl` restriction. The successful
run used the evaluator's explicit timers and reported no resource total.

## Local store and socket measurements

The separate `tests/days_timing.rs` fixture compares the default file store
with the optional database, through the same Unix-socket server. The final
run on the Mac identified above used the completed implementation with
SQLite DELETE/EXTRA/fullfsync, in release mode. The two functional test
suites had finished before measurement. One invocation on 2026-10-14 used
30 days of 200 exchanges each (6,000 total) and 200 explicit kept items.
It does not reuse the LongMemEval corpus. Exact values and machine metadata
are in [memory-socket-results.json](memory-socket-results.json).

```sh
/Users/autonomous/.cargo/bin/cargo test -p lamp-memory --release --test days_timing -- --ignored --nocapture
```

The boundary starts before socket connect and request serialization, then
ends after reading the complete newline-terminated response, before JSON
decoding. Each operation has one first call and 50 subsequent calls; p95
is sorted repeated sample 48 of 50, and maximum excludes the first call.
The store is newly opened, but the filesystem cache is not evicted: this
is not a cold-disk experiment. All times below are milliseconds.

| Operation | Backend | First | Repeated median | Repeated p95 | Repeated maximum |
|---|---|---:|---:|---:|---:|
| Keyword recall, limit 50 / 16,384 text bytes | Files | 15.984 | 15.650 | 15.975 | 16.260 |
| Keyword recall, same request | SQLite | 11.949 | 11.451 | 12.843 | 13.108 |
| Newest recall | Files | 0.513 | 0.460 | 0.498 | 0.505 |
| Newest recall | SQLite | 4.946 | 4.478 | 4.925 | 5.309 |
| Day recall | Files | 0.466 | 0.456 | 0.470 | 0.472 |
| Day recall | SQLite | 5.775 | 6.064 | 6.525 | 6.539 |
| Kept, 1,200 text bytes | Files | 0.162 | 0.143 | 0.153 | 0.165 |
| Kept, same request | SQLite | 1.588 | 1.487 | 1.695 | 1.971 |
| New ranked search | SQLite | 10.828 | 11.324 | 12.924 | 14.303 |
| Session snapshot | SQLite | 75.343 | 1.590 | 1.765 | 2.035 |
| Keyword recall while a tidy job is pending | SQLite | 11.062 | 11.518 | 13.012 | 14.489 |

Every repeated-read p95 meets the declared 50 ms development target.
Keyword recall is faster in this fixture; newest/day/kept reads are slower
with SQLite. The first snapshot call durably creates it, costing 75.343 ms;
repeats retrieve that same frozen snapshot. This remains below the current
voice's 300 ms initial-fetch allowance on this machine. The voice's Days
client calls this operation (`session_open`, `crates/voice/src/remember.rs`)
when `LAMP_MEMORY_DAYS=on`, as the product sets it. None of these figures
promises a bound on the lamp or when another client stalls the sequential
socket loop.

One file-store open took 11.348 ms; first SQLite open including import took
1,848.618 ms; reopen took 80.086 ms. The database then occupied 6,004,736
bytes. Import finishes before readiness and must not be placed on an
interactive request. The first builder should allow memory to become ready
before expecting the voice's initial cached note to contain imported facts.
The model-pending row means a prepared job had not received a result; no
model inference, worker contention or network delay was simulated.

The native release `lamp-memory` binary is 3,267,704 bytes on this x86_64
Mac, and the evaluator 2,839,096 bytes. These include bundled SQLite; no
before/after binary-size comparison or ARM64 size estimate is claimed.
The attempted ARM64 build failed with E0463 (target `core` unavailable).
The required C compiler/sysroot setup and precise lamp commands are in
[memory-days.md](memory-days.md#native-build-and-first-lamp-checks).

An earlier timing attempt failed because the chosen fixture source lay
after the injected clock; it was corrected to an already elapsed exchange.
An intermediate successful run used weaker SQLite synchronization settings
and is superseded by the final measurements above. Neither is silently
combined with these samples. SIGKILL/restart and transactional failure
tests are functional evidence; they do not substitute for an SD-card
power-loss test.

## Not shown

No embedding, model, paid service, account or key was used. This English
dataset provides no measured Vietnamese corpus accuracy; Vietnamese evidence
here is limited to synthetic normalization and retrieval tests. Accent
folding can also collapse distinct words. No answer generation, answer
accuracy, abstention accuracy, diarization or person recognition was tested.
Scripted abstention and wrong-person cases belong to the memory-days tests,
not this LongMemEval retrieval score.

The LongMemEval benchmark applies no production input truncation, retention window or
response-byte budget. It does not test the derivation model, supersession,
forgetting, session notes, the live per-exchange store, or latency through
the memory socket and voice path. It does not measure lamp performance,
Linux ARM64 behavior, or deployment readiness. The full-context ceiling and
the session-level gains cannot be read as results for the live capped
recall response.
