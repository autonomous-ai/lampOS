# Unplug-gap fixture source audit, no new execution

Claim: Queue 6391145177. A complete paginated census of 20 open PRs has no unplug.rs owner. Prior #2523/#2548/#2569 work was inspected; #2569 already changed fixed windows to bounded phase waits and retained 100 ms freshness/500 ms settling, 2 s phase bounds and early Failed rejection.

All three original current observations fail at the common Ready wait with `no Ready in 2 s: Some((Folding, ""))`. The same helper serves the first Ready and Ready after the gap; these raw logs do not identify which wait failed. Their total duration does not establish that distinction or any historical cause. The 0/3 row remains unqualified.

The current test starts the real Adapter background thread, samples on its test thread, and uses host time for both samples and phase waits. Existing `Shared::tick` already accepts a pure explicit clock and is driven directly by other same-module tests; `Proof` has exact fake-time freshness/settling regressions.

Recommendation under source review: exercise the complete Shared tick/real kept-bus path with one controlled monotonic timeline, sample at the original 20 ms cadence and tick using the actual PERIOD constant, with every bus publication observed before time advances. Preserve Ready/Folding/Ready/unsafe-Failed sequence, early-failure and why assertions, 500 ms minimum proof, and 2 s logical phase plus host watchdog bounds. Distinguish first Ready from Ready after the gap in diagnostics. Keep runtime Adapter/sample/worker code unchanged and document that this tests temporal behavior rather than host thread scheduling or physical latency.

The existing 3 s PERIOD and 100 ms settling mutants must still fail the new bounded functional checks, as must treating a gap as permanent failure. These would be distinct predeclared controls; none has run. A phase-clock conversion is not a historical cause claim or an unchanged-case retry. Before implementation, inspect the full kept-bus time semantics and preserve separate Adapter/thread lifecycle coverage. No code edit, compilation, test, device or allocation has occurred for this second audit.

Kept-bus source check: `Bus::latest` returns the last message even when expired, with an expired flag for the caller. This fixture reads its phase without a freshness filter, so a controlled Shared timeline does not require a new bus expiry policy. No transport or runtime clock behavior changes.
