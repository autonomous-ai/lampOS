# Tiered memory: isolated store and sleep validation

This experimental Rust library has its own SQLite database. No product crate
uses it, no service starts it, and it changes nothing on a lamp. The first
implementation contains storage and validated consolidation; cue recall and
the memory-bench adapter follow separately. The approved design is in
[memory-design.md](memory-design.md); the benchmark boundary is
[memory-bench.md](memory-bench.md).

## What exists and what we take

The existing `crates/memory` implementation supplied the safeguards to keep:
owner/source identity, a generation check after forgetting, bounded model
input, explicit successful empty results, preservation of arrivals during a
pass, and SQLite plus FTS5 secure deletion. Its journal retention and limits
are product behavior and are not changed by this experiment. The older V1
memory safeguards are documented in `crates/memory/src/tidy.rs`; there is no
runtime dependency on V1, Voice, or Memory.

[LightMem](https://arxiv.org/abs/2510.18866) separates topic grouping and offline
consolidation from live inference. [MemoryOS](https://arxiv.org/abs/2506.06326)
uses multiple retention levels. We take those architectural ideas, without
claiming their published benchmark scores for this implementation. We reuse
`rusqlite`, the workspace's bundled SQLite, `lamp-local-time` for IANA day
boundaries, and the established reqwest/rustls transport dependencies.

## Store and evidence

`Store::open(path, owner_id)` opens one private database for one owner. Files
are created with mode 0600 on Unix. An exclusive companion file lock prevents
two library instances from independently mutating the same database. A
mismatched owner is refused. The database uses foreign keys, full synchronous
commits, a delete journal, SQLite secure deletion, and FTS5 secure deletion.
The [SQLite documentation](https://www.sqlite.org/fts5.html#the_secure_delete_configuration_option)
explains why the FTS setting is needed in addition to the ordinary table
setting. This is application-level deletion, not proof against filesystem
snapshots, old backups, flash wear levelling, or copies a caller exported.

The caller establishes speaker and access from observed identity signals.
This library does not recognize a face or voice and does not infer a different
speaker from a model's output. `Incoming` includes an explicit addressed-to-lamp
flag, topic, absolute timestamp, source strength and optional speech confidence.
Unknown confidence remains `None`; it is not converted to a guessed number.

| Stored item | Evidence and lifetime |
|---|---|
| Record | Exact UTF-8 text, speaker, time, confidence and strength; no age deletion. An input over 16 KiB is explicitly refused, never silently truncated. |
| Episode | Topic/presence boundary, time, importance, gist and code-copied detail. Topic changes and owner departure close it; silence alone does not. |
| Fact | Claim, exact source quotes, source ids, speakers, times, confidence, strength, kind, validity interval, expiry and superseded fact id. |
| Memory line | Typed row with fact ids: owner model, chapter, event, person, place, taught rule, learned pattern, ahead or feeling. Removing a cited fact drops the entire dependent line. |
| Fingerprint | SHA-256 of normalized forgotten text/overlaps; no original words. |
| Progress | Nightly processed markers, weekly cursor/high-water mark, and cumulative owner-model rewrite accounting for the local day. |

Visitor and overheard speech is not retained. Assistant and unsure lines may
remain in the record but are never evidence and are not sent as source records
to the sleep model. Owner reads require `Access { owner: true, alone: true }`;
all other access receives empty results. The host application must establish
those flags. This conservative prototype withholds all memory when someone
else is present, including non-sensitive memory.

`Store::remember` is a code-only fast lane for an explicit keep recognized by
the caller. It copies the entire owner record into a fact without a model and
without the generated-claim length limit. It does not produce speech or
recognize ordinary-language requests; the bench adapter owns that integration.
`pause` refuses new capture and invalidates prepared consolidation inputs.

## Nightly sleep and weekly reflection

The caller supplies the clock and cadence. There is no timer, worker thread or
network request inside Store:

1. Set the named zone before ingestion (`set_time_zone`); it is fixed once
   records exist. UTC is the library default. The development benchmark uses
   `Asia/Ho_Chi_Minh`. The input carries the zone for relative-date reasoning;
   persisted validity and expiry are absolute milliseconds.
2. `prepare_sleep` takes at most 128 eligible owner records and at most 64 KiB
   of encoded JSON, with bounded previous facts, owner-model lines and related
   episodes. Weekly batches advance their own cursor and freeze an upper id
   for that local day's pass. The database is released before any model call.
3. `ModelClient::propose` sends that input using its configured transport.
   Nightly and weekly use the same client. Tests inject fixture responses;
   the bench adapter will use the runner's broker. No key is needed by an arm.
4. `apply_sleep` parses at most 32 KiB, with at most 32 facts, 32 lines and
   32 episode changes. It rechecks identity, generation, revision and exact
   source content, and validates every proposal inside one write transaction.
   A failed item rolls the whole batch back. A valid empty response succeeds.
5. Only successful nightly inputs become processed. Successful weekly inputs
   advance the weekly cursor. `ApplyReport.pending` tells the caller that
   another bounded batch remains. A failed batch stays eligible; capture never
   waits for inference or a growing extraction queue.

No model field accepts quotes. A fact cites offered record ids; Rust copies the
words and metadata. Only owner records count. Strength is never stronger than
its sources: `maybe` dominates `plan`, and `plan` dominates `said`. A small,
conservative English word check can also lower certainty; it is not a semantic
classifier or a claim of multilingual understanding. Claim text remains a model
interpretation. Consumers must keep the returned strength and quote attached;
code cannot prove every paraphrase entails its evidence.

Each generated claim/gist/line is at most 600 UTF-8 bytes and cites at most
eight records or facts. A taught rule takes its text from one exact owner quote,
regardless of the model's wording. A learned pattern requires at least two
distinct owner source records. Model output has no delete operation and
unknown fields are errors. A supersession needs newer owner evidence, keeps the
old fact as inactive history, and invalidates its dependent lines.

Owner-model lines are edited individually. A local day's batches share one
rewrite allowance anchored to its original line ids: changing or dropping more
than a quarter is refused, even across multiple batches or weekly reflection.
New additions cannot increase that allowance. Lines keep their identity across
replacement. The encoded owner-model view is limited to 2 KiB and 60 lines.
Expiry and an explicit forget are code operations, not model rewrites.

Appointments need an explicit absolute end. Plans expire 14 days after their
latest source, so late consolidation cannot give an old plan a fresh lifetime.
Preferences, habits and identity have no age expiry unless an explicit validity
end was supplied. Expiry keeps fact history while removing its active views.
Low-importance episodes (below 0.5) lose copied detail after 90 days without a
recall; their gist and original record remain. Recall reinforcement is part of
the next implementation, not demonstrated by this first one.

## Forgetting and cancellation

`forget_records` and `forget_fact` remove source rows and all affected facts,
lines and search entries in one transaction. Independent duplicate claims are
also removed. A dependent line is discarded as a whole; unrelated facts cited
by that line remain. Affected episode summaries/detail are cleared. The source
record is the deletion granularity; this API does not surgically remove a word
from a longer utterance.

Fingerprints are checked on ingestion, before sleep, and on proposed derived
text. They use Unicode normalization, case folding by lowercase conversion,
whole text and five-word overlaps; shorter forgotten strings are also detected
inside later text. This can conservatively reject an unrelated repeated phrase.
It is not semantic matching of arbitrary paraphrases. The benchmark must measure
both successful forgetting and excessive deletion rather than assume either.

Forgetting, pause, expiry, completed consolidation, and dropping the store
invalidate prepared inputs. The model client checks this before dispatch and
cancels an in-flight future when invalidated; apply still rechecks the database.
The library never caches a serialized request internally after completion.
Already transmitted bytes cannot be recalled from a provider, and a caller that
copies source data is responsible for erasing its own conversation/snapshot
caches. Raw exported records are privileged data, not a revocable capability.

## Model transport and cost

The single `ModelClient` has a total operation timeout of at most 60 seconds,
at most 8,192 output tokens, a call-count ceiling and an explicit microdollar
budget for its run. Input is conservatively reserved at one token per UTF-8 byte
plus prompt/framing overhead. The caller supplies upper input/output prices;
there is no guessed tariff or free-call assumption. Dispatch is refused before
a request can exceed the remaining reserve. Success with valid usage reconciles
the charge; timeout, cancellation, malformed output or missing usage retains the
conservative reserve. A new client starts a new run: a long-lived host must keep
its run ledger and must not recreate clients to reset an unfinished budget.

`RelayTransport` loads a private credential file, uses HTTPS, refuses redirects
and retries, and reads a bounded response body under the client's deadline.
It does not print keys, prompts, model output, or provider error bodies. This
transport is for explicitly authorized standalone verification. The benchmark
adapter must use the runner's shared broker, including its monotonic accounting
across isolated probes; it must not call the relay directly.

## Verification and remaining evidence

Run from `lampOS/`:

```sh
cargo fmt --all --check
cargo clippy -p lamp-memory-tiers --all-targets -- -D warnings
cargo test -p lamp-memory-tiers
cargo build --workspace
```

The deterministic fixtures under `tests/fixtures` are fixed synthetic JSON
proposals designed to exercise the validator, not captures from a successful
live relay run. Tests cover exact source copying, uncertainty, speaker isolation,
failed/empty batches, arrivals during sleep, weekly pagination, cumulative
rewrite limits, absolute expiry, word-for-word taught rules, deletion in SQLite
and FTS bytes, stale-input cancellation, input bounds, cost and timeouts.
They use made-up people and source text only.

Not yet shown: live relay extraction or actual billed cost, native ARM64 CI,
cue-recall latency, the bench adapter and ablation scorecard, cross-owner voice
attribution, natural-language forgetting, device first-audio latency, and any
real-owner or lamp trial. No product integration or device deployment is part
of this phase. The adapter target is contract revision `a51e3024f`, with 200 ms
recall, 300 ms session snapshot, aggregate prompt-byte accounting, and isolated
probe state that never resets the runner's costs.
