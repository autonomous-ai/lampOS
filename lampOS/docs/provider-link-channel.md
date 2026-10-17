# Provider link observations

`Reporting::Coordinated` separates provider availability from the ordered answer
stream. It is an additive worker contract. `run_with_reporting()` selects the
contract explicitly; `run()` continues to select `Legacy`. The directed CLI's
paired selection is described in the [coordinator contract](coordinator-outage-contract.md).
The provider's cancellation barrier remains `Require` unless the provider
file or `LAMP_PROVIDER_PATIENCE` says otherwise, and input is never replayed,
except a line lost with its connection before any output or tool call, sent
once more on the next ready session under `LAMP_RESEND_LOST_LINE=on` with the
guards in [live-runtime.md](live-runtime.md) (`crate::resend`, #705).

On each 2 ms worker control tick, after urgent Stop/privacy controls, the worker
calls `Supervisor::poll_link()`. This runs even when PCM credit is zero, the
bounded answer queue is full, or input service yields after its control budget.
The worker uses `next_output()` for answer data and never mixes the split API
with `next()`. The 2 ms tick is a scheduling target, not a measured worst-case
deadline. A remote Close hidden behind the transport's intentional PCM read
gate is not yet an observed outage; this mechanism does not bypass that gate.

## Wire and ordering

Control carries `WorkerEvent::ProviderLink { link }`. A link contains a checked
monotonic revision, a connection/attempt session ID, its source observation time,
and one of these states:

- `Ready`: this attempt's setup duration and attempt count, explicit context
  (`Initial`, `Resumed`, `ResumedBeforeLatest`, or `Fresh`), and the original
  outage time and fixed fault category when recovered.
- `Unavailable`: the fixed fault category for the observed loss.
- `Terminal`: the supervisor's final failure category, fault and attempt count.

Resumed context refers to the service's last advertised resume point and known
supersession/invalidation evidence. It does not acknowledge consumption of every
input message or prove exact model memory.

There is exactly one unsent link slot. A blocked control send retains its source
timestamp; a newer validated observation replaces it. Skipped revisions are
therefore possible. A recovered Ready still includes its outage evidence even
when the intermediate Unavailable was coalesced. A known-unavailable session
cannot later become Ready, and Terminal cannot be superseded. Strict decoding
rejects unknown fields. These observations grant neither speech ownership nor
permission to ignore local microphone/privacy authority.

The data channel retains its cumulative packet credit and the existing
512-packet worker bound / 384-packet high-water point. PCM, transcripts, turn
completion, `TurnFailed` and `OwnershipAssumed` remain ordered. The latter two
use the same flat wire representation as the earlier explicit Typed mode.
Retirement prunes obsolete unsent answer data, but never erases terminal
receipts. Coordinated mode sends no credited Ready/Unavailable/Recovered packet.

`Ended` is emitted once after `next_output()` reaches its terminal error and all
earlier retained answer data and exact turn outcomes have been queued. It means
**the upstream/model output has ended**, not process EOF. No model input is
submitted and no model PCM follows it. A Start already in flight may arrive
later and must receive its own `TurnFailed(NotDelivered)`; those local refusals
can follow Ended. The worker stays alive to service controls, credit and such
receipts until explicit Stop or privacy closure. The consumer must not exit on
Ended while an admitted request lacks an outcome/cancellation or retained
playback still needs to drain.

## Timestamp boundary

Gemini observations use Tokio's monotonic `Instant`; cross-process messages use
the host monotonic microsecond clock. These clock domains need not advance at
exactly the same rate. A fixed offset was insufficient on the Mac: a retained
private probe measured a 10 µs projection difference in about 618 ms despite
1–2 µs sampling brackets, and the paired blocked-write test previously ended
with a clock-divergence error instead of its typed turn failure.

`LinkClock` now samples host-before, Tokio-now, host-after on existing control
ticks and when converting a new source observation. Each bracket must be at most
1 ms; a preemption permits at most three sampling attempts. There is no new wait.
A fixed 1,024-anchor history retains samples at least 1 ms apart; the normal worker
tick is 2 ms. Each original source `Instant` is enclosed by actual earlier/later
host sample brackets. This uses monotonic ordering, not a rate projection or a
fitted drift tolerance. An enclosing interval wider than the existing 250 ms
authority lifetime means the worker was held up between two samples (a hot,
throttled board: lamp-4ace on 2026-10-16 at 16:38, 302 ms, ended the worker).
Such a source is dated at the first sample after the gap, its bracket being
that sample's, never earlier than it was, and the link goes on, as lapsed
authority snapshots are skipped; the journal says `held up N ms between clock
samples`. The coordinator reads a link's time only to keep links in order and
for the record, so a late date decides nothing. Future, regressed,
stopped-clock, or uncovered source observations fail closed.

`observed_at_us` is the conservative lower host bound, except after a
hold-up as above, where it is the first sample after the gap. Optional `source_clock`
contains `observed_upper_us` and, for recovery, `outage_started_upper_us`; the
corresponding lower outage bound remains `state.outage_started_at_us`. Both ends
are inclusive, validated, and retained unchanged after correlation. One cached
original outage interval survives history rollover during a long reconnect, and
the last observation retains its interval if reused. Unknown evicted source
times are never replaced with receipt time. A blocked/coalesced send stores only
the already-correlated status, so delayed delivery cannot freshen it.

Host-native observations, including the fixed fixture, omit `source_clock`;
older wire records therefore remain decodable without inventing a correlation.
Gemini's Coordinated producer supplies the bounds. The trace records both
`at_us` and `at_us_upper`, labels `at_us_role=source_interval_lower`, and preserves
separate coordinator receipt time. Consumers use the oldest bound for any age
check and must not treat the interval as an exact latency sample. Normal
uncertainty is the sampling separation plus brackets; target scheduling and
physical latency remain unmeasured.

`Ready.setup_us` is attempt-start to setup-completion. It does not silently
include backoff or delayed dequeue. Recovery has its separate source outage
timestamp. A fresh IPC envelope can legitimately carry an older source
observation; the coordinator validates its identity/revision history and actual
state instead of mistaking observation age for transport freshness.

## Verification scope

Focused tests exercise drift without rate assumptions, conservative intervals,
long-outage caching, history rollover, strict fields/bounds, one-slot
coalescing, preserved WouldBlock state, zero-credit recovery, ordered retained
PCM and terminal outcomes, a late Start after Ended, full output bounds,
priority Stop, and privacy closure. Scripted provider transport uses an
in-memory service; worker transport tests use private local Unix datagrams.
Neither proves provider endpoint behavior, acoustic latency or physical privacy
latency. Consumer integration and target qualification remain separate gates.
