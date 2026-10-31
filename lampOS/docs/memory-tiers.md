# Tiered memory: isolated engine and benchmark adapter

This experimental Rust library has its own SQLite database. No product crate
uses it, no service starts it, and it changes nothing on a lamp. The library contains storage, validated consolidation, cue recall and
registered ablations. Its benchmark adapter and isolated child-state API require
the default-off `bench` feature. The approved design is in
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
| Fingerprint | Keyed HMAC-SHA256 of normalized forgotten text/overlaps; guessing requires the separate key. |
| Progress | Nightly processed markers, weekly cursor/high-water mark, and cumulative owner-model rewrite accounting for the local day. |

Visitor, uncertain-speaker, and overheard speech is not retained. Assistant lines may
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
   the bench adapter uses the runner's broker and single cost client instead. No key is needed by an arm.
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
recall; their gist and original record remain. `recall` is read-only. A caller can separately acknowledge context it actually
consumed with `acknowledge_recall`; only that action reinforces the episode.
The benchmark protocol has no consumption acknowledgement, so the adapter does
not reinforce on selection or on a possibly late response. Probe mutations
would be discarded in any case.

## Forgetting and cancellation

`forget_records` and `forget_fact` remove source rows and all affected facts,
lines and search entries in one transaction. Independent duplicate claims are
also removed. A dependent line is discarded as a whole; unrelated facts cited
by that line remain. Affected episode summaries/detail are cleared. Empty episodes are deleted;
a mixed episode keeps only the time boundaries of surviving records. No
forgotten-episode marker or erased record timestamp remains in that row. The source
record is the deletion granularity; this API does not surgically remove a word
from a longer utterance.

Fingerprints are checked on ingestion, before sleep, and on proposed derived
text. They are HMAC-SHA256 digests under a random 32-byte key in a separate
0600 `<database>.forget-key` file. A copied database alone cannot confirm a
guess against a digest. A copy of both files can; this is not encryption or
a defense against an attacker controlling the process. Missing or mismatched
keys fail closed, never silently reset the forget history. Fingerprints last
for the lifetime of this owner store so expiry cannot re-enable an erased
claim. A deliberate full reset must close the store and delete both the database
and its key file; a newly created store gets a new key and loses the old
forget protection. The library has no automatic reset operation. Schema 2
refuses unreleased schema-1 stores instead of silently migrating unsalted
fingerprints. They use Unicode normalization, case folding by lowercase conversion,
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

## Cue recall and the session snapshot

A recall receives observed people, activity, objects, topic, words heard so far,
time, access scope, and a byte limit. It ranks eligible FTS5 matches with
recency and episode importance, follows fact links to people/place/life-story
lines, and considers a bounded recent fallback. Up to eight distinct cue terms
are reserved before question words. Excluded kinds and expired facts do not
consume the 128-candidate FTS limit. At most eight items are returned; a
session snapshot may return the bounded 60 owner-model lines.

Facts keep their exact quotes and source certainty. Episode summaries persist
the actual cited record ids, rather than treating the whole episode as evidence;
missing or future sources withhold the summary. Unsummarized recent episodes
use at most four exact owner records. Faded detail is never reconstructed from
the record on the ordinary recall path. An explicit history request may still
retrieve an original record, because fading is not forgetting. The adapter
maps internal numeric ids back to the fixture's stable source ids.

The local budgets are 200 ms for recall and 300 ms for a session snapshot.
SQLite progress callbacks interrupt expensive reads; a late result is empty.
Storage contention is an error, not a successful empty lookup. The runner owns
the complete pipe-to-response deadline and ignores late responses. These limits
do not promise an operating-system storage latency or first-audio bound.

Byte accounting includes context, owner-model text, memory instructions and any
memory tools. This adapter advertises no tools. It subtracts a cached session
snapshot's bytes from each recall allowance and returns no duplicate owner-model
lines in recall. Forget, sleep changes, session end, and scope changes invalidate
its snapshot accounting. The caller must also discard previously delivered
context on those boundaries. The agreed runner rule invalidates all cached
memory after every ingestion or state-changing operation and refreshes the
snapshot before reuse, so ordinary forgetting requires no intent guess by the
runner; a library cannot erase bytes copied by a caller.

## Benchmark protocol and registered arms

Build the isolated adapter from the repository root with its exact revision:

```sh
LAMP_MEMORY_TIERS_REVISION=$(git rev-parse HEAD) cargo build --manifest-path lampOS/Cargo.toml -p lamp-memory-tiers --features bench --bin lamp-memory-tiers-arm
```

A source-only build can set the same variable from its recorded source manifest.
Without it, initialization reports `unversioned`, unsuitable for scored runs.
The process speaks [contract v2](memory-bench.md#json-lines-arm-contract-draft-v2),
pinned at `5cfcb7455`, on standard input/output. It requires a fresh private
state directory and an explicit IANA zone. Frames are bounded to 4 MiB. Unknown
operations, bad ordering, duplicate request ids and backward clocks fail.
Diagnostics contain no source text. It opens no socket and loads no credential.

Each sleep handles one bounded batch, with one runner-brokered chat request,
at most 8192 output tokens and a timeout no longer than 60 seconds. Successful
apply reports whether work remains and an opaque cursor. A rejected proposal
keeps its records pending. Broker refusal, timeout or identity mismatch ends the
process without retry; the runner records the failure and keeps its reservation.
The runner is the sole model-cost owner, including calls during isolated probes.
Unknown provider usage is accepted without substituting zero; the runner keeps
its full reservation. Owner ids accept the contract's 128-byte limit.

Ordinary English input recognizes explicit remember, pause/resume and forget
requests using documented deterministic prefixes. Remember copies the owner
line. Forget supports "forget …", "please forget …", "do not remember …"
and "do not keep …" (including contractions), the previous line, or an explicit
full-memory target. A lexical target selects matching record/fact words; code
then performs the same cascading deletion and fingerprinting. Commands are not
stored as new evidence. "Don't remember this" pauses capture until an explicit
resume. An identified owner can erase or pause even with other people present;
the owner-alone restriction applies to capture and disclosure. This is a limited recognizer, not a claim of general
paraphrase or multilingual understanding. The main benchmark must measure its
misses and excessive deletions. No hidden intent flag is accepted. Ingestion
retains only attributed owner speech in an observed owner-alone room; assistant,
visitor, unsure and mixed-room speech is not retained by this conservative arm.

Version 2 ingest and forget replies include `forget_effect`: `ran` records
an admitted forget attempt, including a no-match attempt; it stays false for
an earlier identity/target refusal or a bare word. `suppress_confirmation` is
true after actual removal, and false for an explicit no-removal result. The
runner clears its common recent conversation when the attempt ran and omits
the confirmation when requested. Direct protocol forgetting uses this admitted
path; it is not a store-only diagnostic. This adapter accepts v2 requests only
and echoes unsupported request versions in errors; older recorded v1 data and
its unknown effect remain the runner reader's responsibility.

`probe_begin` copies the SQLite state into a feature-gated in-memory child.
The original connection stays open and untouched. Child rows, fingerprints,
cursors, snapshot accounting, mappings and policy counters are discarded by
`probe_end`. There is no operation that overwrites the original database from
an older snapshot. Global request identities and model-call counts are outside
the child and never roll back. No probe API exists in default builds.

The ordered [arm manifest](../crates/memory-tiers/arm-manifest.json) registers
all eleven arms before any candidate score. Every arm runs three times and is
reported, including failures. Protocol name `tiers` is labelled `ours (tiers)`.
The seven component removals are episodes, life story, facts/people/places,
owner model, how-to-be, what is ahead, and feelings. The three mechanism removals
are nightly sleep, importance, and fading. Nightly removal retains weekly
reflection. Importance removal stores zero proposed importance, removes it
from ranking and treats all old episodes equally for fading.

The facts arm is exactly **facts not recalled; kept only as evidence**.
Proof rows still validate the other components, but fact/people/place text is
never recalled by that arm. All arms preserve provenance, owner attribution,
uncertainty and forgetting. Prompts and bounds stay the same; code filters the
removed outputs. The manifest also records the agreed three-run-spread test
for keeping or removing a tier. No score has yet been used to select a tier.

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
cargo clippy -p lamp-memory-tiers --all-targets --features bench -- -D warnings
cargo test -p lamp-memory-tiers --features bench
cargo build --workspace
```

The deterministic fixtures under `tests/fixtures` are fixed synthetic JSON
proposals designed to exercise the validator, not captures from a successful
live relay run. Tests cover exact source copying, uncertainty, speaker isolation,
failed/empty batches, arrivals during sleep, weekly pagination, cumulative
rewrite limits, absolute expiry, word-for-word taught rules, deletion in SQLite
and FTS bytes, stale-input cancellation, input bounds, cost and timeouts.
They use made-up people and source text only.

The store/sleep checkpoint passed native Linux ARM64 and Linux clippy. The
recall/adapter checkpoint adds source-citation, access, aggregate-byte, deadline,
contention, fading, arm-manifest and subprocess protocol tests. A synthetic
3000-record lookup test reports first-use and repeated host lookup times; it
measures local recall only, not a voice turn or a live provider. On macOS x86_64
with Rust 1.96.0, the first lookup after synthetic ingestion took 1.599 ms and
the slowest of 20 subsequent lookups took 1.426 ms. The in-memory SQLite pages
were populated by ingestion; this is not a cold-disk measurement. Feature-specific
ARM64 and Linux clippy coverage must include `--features bench` explicitly.

Not yet shown: live extraction or billed cost, the three-run ledger scorecard
against every baseline, real STT, held-back human review, cross-owner voice
attribution, device first-audio latency, or any real-owner/lamp trial. Recorded
proposals and scripted forgetting tests establish implementation behavior only.
No product integration or device deployment is part of this phase.
