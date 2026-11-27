# Evaluation checkpoint worker: host verification

Part of #8793. This adds an explicit single-write worker to the unused
`lamp-evals` library. It builds on the recorder/storage implementation reviewed
in #8830. No deployed binary consumes this worker; no process, unit, bus
adapter, producer switch, report sender or lamp setting changes here.

## Gates and fault boundaries

From `lampOS/`, with Rust 1.96.0:

```sh
cargo fmt --all --check
cargo test --offline --locked -q -p lamp-evals
cargo clippy --offline --locked -q -p lamp-evals --all-targets -- -D warnings
```

Mac x86-64: **53 tests passed**, two explicit measurements ignored by the
ordinary gate. Linux x86-64: **54 tests passed**, two measurements ignored.
Workspace formatting and strict all-target Clippy passed on both. Linux used
the actual workspace/lockfile with source and dependency caches mounted
read-only, no network, one compiler job, two CPUs and a 2 GiB memory limit;
source hashes remained unchanged during the run. The extra Linux storage test
checks nonblocking refusal of a writerless FIFO.

Ten new worker tests use controlled latches for blocked writes, Busy ownership,
matching acknowledgement, ordered tickets, exhaustion, fixed-error privacy,
latched save failure, panic, foreign completion, requested stop and observed
exit. Two real-Store integration tests verify mode `0600`, a single atomic
checkpoint, the lifetime lock, and restoration of receipts, retention and
deduplication together. A live recorder advanced after snapshot capture is
not mistaken for the older saved snapshot. Refused work is not silently queued.

Independent source review found a transient second-snapshot allocation after
acknowledgement. The worker now releases the completed snapshot before sending
the acknowledgement that permits another offer. The final read found no
remaining P1/P2. Save success, unknown durability after error, and confirmed
thread exit remain different states; no failure causes an automatic retry or
replacement writer.

## Measured host boundaries

The two ignored measurements were run explicitly and passed:

```sh
cargo test --offline --locked --release -q -p lamp-evals measure_ -- --ignored --nocapture --test-threads=1
```

Measurements used the reviewing x86-64 Mac, release code and uncontrolled host
load. Each size has one first call plus 31 repeats. With 31 repeats, the
reported p99 is the sample maximum, not a qualified population tail.

| Synthetic state | Checkpoint payload | Capture first / p99 | Offer first / p99 | Complete Store save first / p99 |
|---|---:|---:|---:|---:|
| One observation | 671 B | 3 / 68 µs | 2 / 16 µs | 26.513 / 36.536 ms |
| 4096 segments, 1024 epochs, 4096 identity ranges | 1,551,388 B | 2.274 / 3.378 ms | 7 / 22 µs | 93.318 / 118.927 ms |

Near-capacity capture median/p95 were 1.197/3.105 ms; full-save median/p95
were 73.062/112.767 ms. Offer excludes snapshot capture, thread startup,
serialization, filesystem work and waiting for completion. Those excluded
costs remain necessary work. Capture exceeds a 1 ms observation-path budget
and must remain separate maintenance work.

Sampled poll p99 and completion-return poll p99 were below the 1 µs reporting
resolution; zero in the raw microsecond table means below that resolution,
not zero cost. Sampling retains at most the first 100,000 poll calls per size.
Poll maxima were 39 µs for the small fixture and 25 µs near capacity. Waiting
and yielding between calls is excluded from per-call timings.

A separately latched backend stayed blocked through 10,000 repetitions.
Busy offer first/p99/max were 267 ns / 89 ns / 10.949 µs; poll first/p99/max
were 599 ns / 85 ns / 34.869 µs. The prepared-offer/poll target of p99 below
100 µs passed these host samples. This is neither a lamp measurement nor a
hard scheduling deadline. Real-Store success fixtures and blocked-backend
fixtures establish different boundaries; neither simulates all flash faults.

## Remaining integration gates

Native ARM64 and complete-workspace Linux CI remain required at the submitted
head, followed by the repository's independent reads. No physical power-loss,
storage-stall, process-supervision, consent-deletion, target-latency, live-voice
or fleet test ran here. A kernel-stalled write cannot be cancelled by this
thread primitive. Final thread teardown also has no hard time bound.

The production process still needs the reviewed producer adapter, a bounded
snapshot schedule, dirty-state and loss accounting, startup/recovery policy,
quiet evaluation, upload and deletion contracts. Dropping an unfinished owner
does not prove it stopped writing. See [the persistence contract](../../evals.md#background-checkpoint-persistence)
before integrating it into a recorder or a shutdown/erasure path.
