# Local evaluation evidence (`lamp-evals`)

`lamp-evals` is a Rust library for bounded aggregation, durable local
checkpoints and an explicit background checkpoint writer. It is a foundation
for a fleet evaluation system; it is not the full evaluator. No process, bus adapter, producer switch,
systemd unit, sender, backend endpoint or nightly check is installed by this
crate. The local dashboard prototype is a separate review tool.

The product objective is to evaluate every part, sense and ability against
what the person intended and what the lamp actually did, then identify and
verify improvements by build. This first model records one narrow class of
evidence: a voice delivery terminal outcome. `completed` cannot establish a
correct answer, successful task, natural conversation, successful wake call,
audible sound, accurate perception or a correctly performed emotion.

Implementation: [model](../crates/evals/src/model.rs),
[recorder](../crates/evals/src/core.rs),
[store](../crates/evals/src/store.rs),
[persistence worker](../crates/evals/src/persistence.rs). The
[reuse decision](reuse-decisions.md#bounded-local-evaluation-evidence-2026-11-27)
records the V1 baseline and existing Rust alternatives inspected.

## Internal input and meaning

`Observation` is an internal Rust value, with no external serialization or
free-form payload. It is not a second bus event definition. The adapter for the
reviewed voice `TurnRecord` version 2 remains separate work. Likewise,
`Contract::VoiceDeliveryV1` versions the internal aggregation rubric, not the
producer's wire format. These version numbers must not be equated.

| Field | Type and contract |
|---|---|
| `epoch`, `turn` | Local `u64` identity. The caller must assign a distinct producer epoch after a restart and keep a terminal observation immutable. These IDs stay in the local checkpoint. |
| `time` | `SourceTime::UtcMinute(u64)` only when the source UTC clock is trusted; otherwise `Unknown`. Receipt time is never substituted for source time. |
| `build` | `Option<Build>`; unknown stays `None`. `Build::parse` accepts a canonical positive `u64` sequence, `+`, and 7–40 lowercase hex characters: at most 61 bytes, no leading sequence zeroes, spaces or arbitrary text. |
| `origin` | One of `microphone`, `reminder`, `proactive`, `text`, `computer`, `unknown`. No origin is inferred from an outcome or the latest interaction. |
| `terminal` | Exactly one of the ten partitions below. |
| `audio_started` | `Some(true)`, `Some(false)` or unknown. No source evidence is upgraded by a successful terminal category. |
| `endpoint_to_first_write_ms` | Optional `u32`, 0–4,294,967,295 ms. A value requires `audio_started == Some(true)` or the whole receipt is rejected as invalid evidence. |
| `contract` | Currently only `voice_delivery_v1`. Build, origin and contract remain separate aggregate dimensions. |

The terminal vocabulary is `completed`, `playback_gap`, `no_audio`,
`provider_error`, `deadline`, `fault`, `person_interrupt`, `privacy_cancel`,
`other_cancel`, `unknown`. These are producer terminal categories. In
particular, person interruption and privacy cancellation are retained as their
own outcomes, not silently added to successful deliveries or discarded as
missing data. The exact mapping of producer states, audio contradictions and
unknown older-version fields must be reviewed in the future adapter; the core
does not reconstruct a missing lifecycle from logs or infer intended calls.

## Recorder API and retention clock

The public API is:

```rust
Recorder::new(window_end_hour: u64) -> Result<Recorder, Error>
recorder.observe(observation: Observation) -> Ingest
recorder.advance_window(new_end_hour: u64) -> Result<(), Error>
recorder.checkpoint() -> Result<Vec<u8>, Error>
Recorder::restore(bytes: &[u8]) -> Result<Recorder, Error>
recorder.snapshot() -> Snapshot
```

`window_end_hour` is the exclusive end of the current/open UTC hour, counted
from the Unix epoch. Thus `window_end_hour - 1` is open. The retained interval
is `[max(0, end - 720), end)`: at most 30 UTC days. The valid end range is
1–70,389,528, ending at the start of year 10000.

Only the separate trusted retention clock may advance the window. Equal end
values are harmless; backward movement and a jump larger than 24 hours in one
call are rejected without changing state. Recovery after a long outage needs
an explicit caller clock policy. A future observation cannot advance the
clock or purge history. A known source hour outside the retained interval is
rejected as expired or future before deduplication; an out-of-range minute is
invalid time.

An unknown-time record receives a private receipt cohort solely to bound its
retention. It remains unanchored in every snapshot. Cohorts expire under the
same rolling retention clock and are combined only by build, origin and
contract in the unanchored output. After that cohort expires, the recorder
cannot recognize an old replay whose source time is still unknown; it makes
no all-time uniqueness claim.

## Aggregate snapshot

`snapshot()` returns the serializable schema `lamp-eval-aggregates/1`, a local
aggregate view, not an approved upload format. Its top-level fields are
`schema`, `window_start_hour`, `window_end_hour`, `coverage`,
`opportunity_coverage`, `receipts`, `hourly`, `daily`, `unanchored`. Both coverage
fields are always `unknown`. No ID, source minute, individual event timestamp,
receipt cohort, words, face or speaker identity is present.

Each aggregate has its own nullable build, origin, contract and counts. Hours
and days are numeric Unix UTC hour/day indexes. The projections differ:

| Projection | Included evidence |
|---|---|
| `hourly` | Nonempty source-hour buckets, with `utc_hour` and `closed`. The open hour is retained with `closed: false`. Closed means the calendar hour ended, not that every event was observed. |
| `daily` | Only UTC days strictly before the current/open hour's UTC day. At 23:00, that day's closed earlier hours remain hourly only; they become daily evidence after the trusted clock advances past UTC midnight. |
| `unanchored` | Retained unknown-time counts, with no hour/day attribution. |

A daily row has `utc_day`, `observed_closed_hours`, and `coverage: unknown`.
`observed_closed_hours` counts distinct nonempty hour buckets for that exact
build/origin/contract. It is not hours of uptime or proof of complete recording,
even when 24. Rolling retention can clip the beginning of the oldest completed
day. Missing hours are never interpolated and empty input creates no buckets.

Counts contain `total` and all ten `outcomes`, each with `terminal`, `total`,
`audio_started`, `audio_not_started`, `audio_unknown` and `timing`. The audio
partitions sum to that terminal's total; terminal totals sum to the aggregate
total. Counts are `u64` integers. A later JavaScript/backend/wire adapter must
preserve integer precision or explicitly reject numbers it cannot represent;
this library does not limit them to JavaScript's safe integer range.

Timing has the fixed boundary
`endpoint_decision_to_first_speaker_write`, `edges_ms`, seven `bins`, `samples`
and `unavailable`. The edges are 500, 1000, 1500, 2000, 3000, 5000 ms. Each
measurement enters the first bin whose edge is strictly greater than it; the
last bin includes 5000 ms and above. Timing is kept separately for each
terminal partition. `samples` is the sum of its bins; `unavailable` counts
explicitly started audio without that timing. Audio-start-unknown records do
not acquire a timing denominator.

This boundary is a software endpoint decision to a speaker write. It omits
the preceding end-of-speech detection delay and does not establish when a
person heard substantive answer audio. No rates, scores, percentiles or
averages are produced. A future consumer can combine compatible histogram
counts; it must not average per-hour percentiles or blend different boundaries,
builds, origins, rubric versions or terminal populations without saying so.

## Bounds, deduplication and accounting

| State | Hard bound |
|---|---:|
| Retained time | 720 UTC hours |
| Aggregate segments, including private unknown-time cohorts | 4096 |
| Retained producer epochs | 1024 |
| Dedupe ranges, total across all epochs | 4096 |
| Checkpoint JSON payload | 8 MiB |
| Trusted clock advance per call | 24 hours |

Deduplication uses `(epoch, turn)`. Inclusive turn-ID ranges are sorted per
epoch and merged only for adjacent IDs belonging to the same aggregate
segment. Out-of-order IDs work, including an ID that bridges two ranges at
capacity. Retained dedupe state is never evicted merely to accept more data.
An event exceeding a segment, epoch or total-range cap is rejected and counted.
The 4096-range cap can be reached well before 30 days: sparse IDs do not merge,
and adjacent IDs interleaved between builds, origins or other segment keys
remain separate ranges. Thirty days is a maximum retention horizon, not a
promise that normal traffic fits. Capacity/rejection counters must stay visible
as coverage limitations, and representative producer traffic must calibrate
these caps before rollout. Do not silently drop retained identities to improve
acceptance or interpret refused evidence as a healthy interval.

The first accepted observation owns its identity: a later changed payload is
a duplicate, not an amendment or separately detected payload conflict. This
requires an immutable producer-terminal contract; the core is not a joiner for
several lifecycle events sharing one turn ID.

`Ingest` is `Accepted`, `Duplicate`, or `Rejected(Rejection)`. Rejection
categories are `Expired`, `Future`, `InvalidTime`, `InvalidEvidence`,
`SegmentCapacity`, `EpochCapacity`, `DedupeCapacity`, `CounterOverflow`.
The serialized receipt counters use the corresponding snake-case names for
the first seven categories. `calls`, `accepted`, `duplicates`, `rejected`,
`retired` and `exhausted` describe lifetime recorder attempts:

```text
calls = accepted + duplicates + sum(rejected)
accepted = retained dedupe cardinality + retired
segment total = sum(length of its retained dedupe ranges)
terminal total = audio_started + audio_not_started + audio_unknown
sum(timing bins) <= audio_started
```

Retirement removes aggregates and their dedupe ranges together and increments
`retired`. It is not a failed interaction. On receipt-counter exhaustion,
ingest refuses further calls, sets `exhausted`, and leaves the integer counts
unchanged; it cannot count attempts beyond `u64::MAX`. These counters are not
eligible opportunities and do not reveal calls the producer never observed.

The checkpoint stores private version 1 state, counts and dedupe together.
It uses fixed counter arrays and range-to-segment indexes; it is neither an
event archive nor an export. Restoration checks byte/collection bounds,
required fields, closed enums, build syntax, retained hours, unique segment
keys, sorted unique epochs, normalized nonoverlapping ranges, valid segment
references, per-segment cardinality, timing denominators and checked accounting
arithmetic. Unknown fields, malformed values and overflow fail. `Recorder`
itself cannot be externally deserialized to bypass these checks.

Core errors have fixed names: `invalid_window`, `backward_window`,
`window_jump`, `invalid_build`, `checkpoint_too_large`, `invalid_checkpoint`,
`arithmetic_overflow`. Parsing failures do not echo input strings.

## Persistence and crash boundary

`Store::open` takes an existing caller-owned private directory, normally mode
0700. It does not create it or repair permissions. Lexical trailing separators
and dot components are removed before opening, so they cannot bypass the
final-directory symlink refusal; the path is not canonicalized. Opening also
refuses group/other access, checks the effective owner, and holds an
exclusive nonblocking directory lock for the `Store` lifetime. A second writer
is refused immediately. `save(&mut self, recorder)` also requires exclusive
caller access: the process lock alone would not prevent two threads using one
store from sweeping each other's active candidate. Operations use the held
directory descriptor, so a later replacement of its path does not redirect
checkpoint access.

Recovery runs after taking the lock at open and before each save. It recognizes
only `.state.bin.<16 lowercase hex characters>.new`. A candidate can contain a
partial crash write, so recovery checks metadata rather than decoding its
contents: it must be a private regular file owned by the effective user, with
one link and a nonnegative size no larger than 8 MiB + 54 bytes. A matching
symlink, special file, shared or wrong-owner file, extra link or oversized file
refuses recovery. Other names are left alone.

The scan counts at most 64 directory entries, excluding only `.` and `..`;
hidden and unrelated entries count too. All recognized candidates are checked
before any removal. An overfull scan or unsafe candidate refuses the operation
without removing an entry. When the committed file is absent, at most 62
entries may remain after cleanup; two slots are reserved for that first
checkpoint and its candidate. With a committed file already present, at most
63 entries may remain, including that file: one candidate slot is reserved.
This makes a successful first save reopenable at the same bound. Recovery
never removes unrelated entries to make room. It
then removes the validated leftovers and fsyncs the directory. A removal or
fsync I/O error can leave cleanup partially completed; the committed checkpoint
and unrelated entries are never cleanup targets.

The fixed file is `state.bin`. It contains an 18-byte magic/version prefix,
a four-byte little-endian payload length, a 32-byte SHA-256 digest and the
bounded JSON payload: at most 8 MiB + 54 bytes. The checksum detects accidental
corruption; it is not authentication against another process permitted to
write the same private directory. Reads refuse symlinks, nonregular files,
shared permissions, a different owner, multiple hard links, oversized input
and observed size/time changes during reading. Opening is nonblocking so a
FIFO cannot stall before being rejected. `load()` returns `None` only for a
missing file; malformed, truncated or unreadable state is an error, never a
healthy empty history.

Saving validates the recorder payload before writing. It reuses
[`lamp-checked-replace`](../crates/checked-replace/src/lib.rs): create a unique
0600 candidate beside the destination without following a link, write all
bytes, sync the candidate, reread and compare its bytes, verify checksum and
restore invariants, rename over `state.bin`, then fsync the directory. A
failure before rename leaves the old destination intact; cleanup of the
candidate is best effort. After rename, a directory-fsync error can return
failure while the new file is already visible. There is no rollback guarantee
at that point. A caller must reload and validate rather than assume which
state is durable. Crash leftovers are handled by the bounded recovery above;
they are never promoted to committed history.

An accepted in-memory observation is not a durability acknowledgement. A crash
before the next successful save can lose that observation and its dedupe state
together. A recovered committed checkpoint restores both consistently; safe
replay still depends on a future producer replay protocol. There is no
exactly-once network guarantee, backup slot, automatic corrupt-state reset or
write schedule in this library. Physical power-loss and storage-stall behavior
remain unverified on a lamp.

## Background checkpoint persistence

`persistence::Persistence` moves the existing `Store` into one worker thread.
The caller opens the private store and loads/validates its checkpoint during
startup, before admitting recorder input. This primitive adds no file format,
daemon, product unit, bus reader, network sender or automatic write schedule.

| Operation | Meaning |
|---|---|
| `Persistence::start(Store)` | Starts one writer with exclusive ownership of the already-open store. Thread creation is startup work, outside the observation callback. |
| `try_checkpoint(Recorder)` | Offers one owned, immutable snapshot without waiting for disk. Acceptance returns a local `Ticket`, not a durability acknowledgement. A refusal returns the original snapshot with a fixed reason. |
| `poll()` | Collects a matching completion and reports current status. Until its completion is collected, an outstanding ticket remains busy. |
| `status()` | Observes the controller state; an outstanding write and the last confirmed save remain separate. |
| `request_stop()` | Refuses new work and lets the single accepted snapshot drain. A stop request does not assert that the worker has exited. |

There is one outstanding snapshot, one job-channel slot and one completion
slot. There is no second pending snapshot, coalescing, automatic retry or
replacement worker. The caller keeps observing into its live recorder and
retains its dirty-state/revision policy separately. Capturing or cloning the
snapshot is explicit caller work; neither offer nor poll clones or serializes
the history. Queue pressure must defer persistence, not discard incoming
observations or claim the live recorder has been saved.

Tickets increase without wrapping within one worker. Exhaustion refuses
further submissions rather than reusing a completion identity. Tickets are
local control metadata, distinct from producer turn identities, and do not
belong in aggregate uploads. The caller must not confuse an older saved
snapshot with newer in-memory observations, or compare ticket numbers across
different worker instances.

A matching successful completion is returned only after `Store::save` returns
success, including candidate validation and directory sync. Any save error is
reported as **durability unknown**, with a fixed failure category: atomic
replacement may already have happened before the error. The failure latches;
the writer does not continue with another snapshot or retry silently. Confirm
shutdown, then reopen and validate the committed store before deciding how to
recover. Neither an error nor an absent acknowledgement means the old file is
necessarily still current. Worker loss/panic and mismatched completion cannot
become successful saves. No raw I/O error text or checkpoint content is part
of the status contract.

An outstanding save may remain blocked in the kernel. A caller's elapsed-time
limit does not cancel it and must not start a second writer. Polling checks
the retained thread handle before joining; it does not wait for an unfinished
save. Rust's [thread completion contract](https://doc.rust-lang.org/std/thread/struct.JoinHandle.html#method.is_finished)
still permits final thread teardown after the main function returns, so this
is not a hard real-time shutdown guarantee. A saved completion and confirmed
thread exit are separate facts. Dropping the controller requests stop but
detaches an unfinished thread; it may still finish the accepted write. Use
confirmed shutdown before erasure, replacement, or claiming that no future
write can occur. The store's lifetime lock remains with the worker until it
exits. This primitive does not implement consent withdrawal or deletion.

The proposed offer/poll target is **p99 below 100 microseconds** on the lamp,
with an already-captured snapshot, for first and repeated calls and blocked
storage. It is a soft target, not a hardware result. Measure snapshot capture,
worker serialization/validation and the complete save separately; excluding
them from offer timing does not make their CPU, memory or I/O cost disappear.
The future process must schedule those costs, expose stale/failed persistence,
and verify its effect on real interaction latency before activation.

## Runtime and fleet integration still required

`observe` performs bounded in-memory work, including a segment search and
range copying/merging. It does not log, touch disk, use the network or wait for
another service. The proposed target is **under 1 ms per ingest call** on the
target lamp, first and repeated calls, including near-capacity state. This is
a target, not a measured hardware result. Measure the entry-to-return boundary
and tail latency, resource use and effect on live voice before enabling a
producer. `checkpoint`, `snapshot` and blocking `Store::save` belong outside
the voice path. Filesystem syscalls have no deadline here. The background writer
isolates the save operation, but a supervised process, snapshot scheduling,
dirty-state accounting and recovery policy remain to be integrated and tested.

The full feature system needs an explicit chain for each item: an eligible
attempt or opportunity, expected behavior, observed evidence, terminal result,
dependencies, unknown cases and a versioned rubric. Hardware availability must
remain separate from correct sensing and successful abilities. In particular,
head direction, gaze, face detection, speaker handling, each joint, expressions
such as happy, task outcomes and semantic answer quality need their own
reviewed signals and ground truth. None is inferred from voice delivery here.

Remaining integration gates are:

1. Review the producer contract and map its exact versioned fields, local
   epoch identity, source-clock trust, cancellation and replay semantics. Add
   bounded decoding, loss/unknown coverage and old/new-build compatibility
   controls before connecting a bus reader.
2. Define and test additional feature rubrics and opportunity denominators.
   Maintain the source-backed inventory when features, controls or expression
   clips change. Controlled evaluations need their own evidence and results;
   passive receipt counts cannot stand in for them.
3. Build the supervised recorder and connect snapshot scheduling, the persistence
   worker, recovery and diagnostics with explicit limits and failure behavior.
   An optional nightly passive
   evaluation must respect privacy, presence, conversation and update guards;
   no active sound, light or movement is authorized by this library.
4. Specify an aggregate upload contract, consent/retention/deletion behavior,
   backend storage, retries and fleet reads. Do not upload checkpoints or
   event IDs. Reuse the existing report choice and transport where appropriate,
   after review; the whole snapshot is not a 4 KiB health-report message.
5. Product direction on 26 Nov permits health-report analytics for improvement.
   The backend agreement in [cloud.md](cloud.md#the-backends-side-as-agreed)
   and the setup/app consent wording still require coordinated follow-up.
   This library changes neither the existing report settings nor what leaves
   the lamp.
6. Run privacy canaries through the actual producer, adapter, local store,
   export, transport and backend paths; validate consent withdrawal, malformed
   messages, restart/replay, capacity pressure and dropped observations. The
   current core's closed enums and synthetic canaries do not prove future
   adapters safe.
7. Complete host/Linux gates and authorized hardware latency, crash and
   long-running observations, then a separately approved rollout. Compare
   builds using compatible rubrics and disclosed coverage. Keep unknown or
   unmeasured items visible instead of assigning a fleet success score.

The intended improvement loop is evidence, diagnosis, a scoped change,
controlled verification, then observed behavior after rollout. It must retain
the distinction between a component being available, an operation finishing,
and the person getting the result they intended.

## Verification

The [host verification record](verification/fleet-evals-20261127/README.md)
records the commands, counts and limits for this first library piece.

The [checkpoint-worker verification](verification/eval-persistence-20261127/README.md)
records its separate Mac/Linux gates, controlled stalled-write cases and host
measurements. It does not qualify a running recorder or target-lamp latency.

The [core tests](../crates/evals/tests/core.rs) cover replay after restore,
out-of-order ranges, new epochs, clock and UTC-day boundaries, clipped retention,
unknowns and cancellations, per-terminal timing, mixed builds/origins, every
capacity limit, malformed checkpoints, arithmetic corruption, privacy canaries
and a dense large-counter checkpoint. The
[storage tests](../crates/evals/tests/storage.rs) cover the host filesystem
boundaries independently. These are synthetic/software controls, not acoustic,
perception, movement, physical durability or whole-fleet acceptance.

From the lampOS root, the applicable gates are `cargo test -q -p lamp-evals`,
`cargo clippy -q -p lamp-evals --all-targets -- -D warnings` and formatting
checks. The 27 Nov Linux x86-64 container check passed 28 core tests, 14 storage
tests and strict full-crate Clippy with the actual workspace, pinned lockfile,
source and dependency caches mounted read-only, and networking disabled.
The Mac gate also exercises this library; its storage suite has one fewer
test because the FIFO fixture uses a Linux-only Rustix API. No ARM64 device
test or production readiness follows from those host results. Record the source
revision, exact commands and outstanding device gates with a review;
this document does not turn a prepared command or test case into a passing run.
