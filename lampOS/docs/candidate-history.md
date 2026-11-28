# Candidate history, inactive (#8971)

`eyes::candidate_history` keeps bounded, local observations for a future
confirmation reader. [Vietnamese](candidate-history_vi.md).

There is **no runtime caller, default operating limit, presence verdict,
model, storage, bus field or switch**. A displaced face can be a picture, a
moving screen or tracking error. Neither this history nor comparison quality
establishes a real person. The [failed static](https://github.com/autonomous-ai/lampOS/pull/8980)
and [anti-spoof evaluations](https://github.com/autonomous-ai/lampOS/pull/8982)
leave presence observational. The [shared quality API](https://github.com/autonomous-ai/lampOS/pull/8981)
is separate.

## Reuse and the two reproduced faults

The inherited draft processed faces one at a time, matching nearby centres.
Two detections in one frame could share a slot and appear to move. It also
tested displacement before expiring the origin: repeated stationary detections
could keep an old origin until a move beyond the intended window confirmed it.
Both cases were [reproduced against the exact draft](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7241111762).

Reuse `bus_out::Ids::of` for upstream batch assignment, rather than creating
another geometric tracker. It marks an assigned track taken within a frame.
The new history accepts those numeric passing ids, rejects duplicate ids, and
never follows aliases or re-found identities. This does not qualify the
tracker's association accuracy, which still needs an evaluation before use in
confirmation. The current runtime order is unchanged; moving id assignment
ahead of a future gate requires a separate integration.

`History::observe` consumes a whole frame. It expires an origin **before**
measuring displacement. A first observation has no displacement; a later one
requires a positive capture interval. The result is a number, not a confirmed
person. No new movement threshold is introduced.

This is an origin-based window, not an exact rolling history of every pair.
Resetting the origin can discard useful recent movement. That conservative
loss must be measured before any consumer uses it; no retention claim is made.

## Contract

`History::new(epoch, Limits)` requires explicit nonzero durations:

| Limit | Meaning |
|---|---|
| `window_us` | Maximum age of the window's original observation |
| `maximum_gap_us` | Largest allowed gap between captures of a retained track |
| `maximum_frame_age_us` | Largest allowed capture age when processed |
| `MAX_TRACKS = 8` | Fixed work/storage bound, not a measured people limit |

Capture and processing times must use the **same monotonic clock**. An epoch is
a caller-owned, increasing generation for camera/privacy lifetimes, not a camera
grant. Sequence and capture time strictly increase within an epoch; processing
time cannot go backwards. Future frames, stale frames and repeated/reordered
captures reject the entire batch and break history. A valid timestamp is
consumed even if its candidate list is invalid, so that frame cannot be replayed
with different candidates. A rejected invalid timestamp does not advance time.

Candidate centres must be finite and in [0,1] of the filled image. More than
eight candidates, duplicate ids or any invalid centre reject the entire batch;
there is no truncation or partial success. `usable=false` is caller-supplied
quality evidence, not presence. Missing/unusable ids lose their window at that
frame. A reappearing or replacement id begins fresh, even in the same place.
This conservative mechanics rule does not prescribe a product presence hold.

Before a temporal delta, the caller must establish `CameraInterval::KnownStill`
for the **whole interval**, with complete, fresh motion evidence. `Moved` or
`Unknown` resets the origins at the current frame; only a later valid interval
can produce a delta. `speaking::JointHistory` is reusable capture-time context,
but its single nearest joint reading does not by itself prove the whole interval
was still. Parallax would need a different calibrated interpretation; this
component does not infer it.

`close()` clears everything and refuses frames. `begin_epoch` first closes and
requires a strictly newer generation; failed or wrapped generations remain
closed. Numeric results already returned are **not authority tokens**. A future
caller must bind the face, image and capture, recheck freshness and the current
camera/privacy grant at every consequential use, and never use these numbers as
permission for recognition, storage, greeting or publication.

## Bounds and validation

The component uses eight fixed optional slots and an eight-entry output array;
it performs no heap allocation. Validation and matching scan at most 8×8 entries.
It stores centres, passing numeric ids and times, never frames, prints, names or
words. Counters saturate; timestamps are checked before subtraction; epochs do
not wrap. There is no worker, delayed task or blocking I/O to cancel.

Generated controls cover the two exact failure shapes, actual upstream distinct
ids, repeated/out-of-order captures, processing clock reversal, stale/future
frames, inclusive boundaries, privacy close/reopen, epoch wrap, duplicate ids,
invalid geometry, full-capacity reordering, overflow refusal, missing/unusable/
replacement ids, large gaps and unknown/moving camera intervals. They validate
mechanics, not physical presence, association accuracy or a product duration.

`cargo run --release -p lamp-eyes --example candidate_history_measure` measures
an eight-candidate frame to history result: first call, then 1,000 calls with all
timings retained. The host budget is p95 ≤100 μs per eight-candidate frame.
Upstream detection, tracking, quality, camera-interval validation, camera capture
and publication are excluded. ARM64, voice load and actual confirmation delay
remain unmeasured; no runtime is wired up by this benchmark.

On the shared Intel iMac (macOS 26.6.2, Rust 1.96.0, release), the frozen
generated-input run took **1.470 μs for the first frame**, then **0.237 μs
median / 0.255 μs p95 / 0.578 μs maximum** across 1,000 further eight-candidate
frames. All 1,001 raw timings are retained locally; a separate calculation
agrees with every reported statistic. This meets only the stated host
arithmetic budget, not an end-to-end or device target.
[Aggregate evidence and source/protocol hashes](verification/candidate-history-20261128/numeric.json).

Validation: all 16 focused controls and 526 tests across the eyes targets
passed; five existing manual/model tests remain ignored. Workspace formatting
and strict all-target Clippy passed. The first full suite run could not bind
local Unix sockets in the sandbox; the rerun with local socket access passed.
Neither run used a lamp, learned model or real person.
