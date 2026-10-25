# Factory head discovery requires observed hands-away coverage

A completely skipped hands-away phase previously left `rest_changes = 0`
and could still qualify both held pads. The factory interpreted that zero
as a measured baseline. A delayed first acquisition also left `gaps = 0`.

Discovery now applies its existing 100,000 us freshness bound to missing
coverage inside the hands-away interval. The interval starts after the
500,000 us settle and excludes its end. At least one sample must fall
inside it, including when it is shorter than the freshness bound. A failure
latches for that scan; later touches cannot repair missing baseline evidence.
Gaps entirely before or after rest retain the existing contact recovery.

No report field, pins schema, product switch, live touch behavior or factory
script changes. The CLI already propagates discovery errors; the factory
already restores Controls and leaves the original pins file unchanged on
that path. This correction applies to explicit calibration with rest.

## Reproduction and boundaries

The original production at 14c64a8db ran two new negative controls: **0 passed,
2 failed**, exit 101, at the missing-baseline assertion. Both traces could
qualify pads 97 and 98 as held-level inputs with zero rest changes despite
no rest samples. The exact test overlay is preserved as a string in `negative-tests.json`; its
source and binary identities, command, time bounds and raw-log hash are in
`negative.json`. The executed original binary is retained privately and is
not committed.

The first fixed discovery suite passed **16/0**, recorded in `focused.json`.
That earlier execution preceded two additional assertions: a fresh backfill
cannot repair a latched failure, and the same boundary works with a nonzero
clock epoch. The complete final gate covers the final test source separately.
The six new cases also cover a 99,999/100,000 us gap, first and last phase
edges, a short phase with no inside sample, and ordinary outside-phase gaps.

Latency target: a successful scan adds no wait to the existing 40 s factory
flow. A missing baseline is rejected by the first ingest that reveals it;
there is no I/O or retry in that decision. These are synthetic acquisition
time boundaries, not measured GPIO, acoustic or physical hand latency.
No lamp was accessed, no profile installed and no ARM64 binary linked on
macOS. Native Linux ARM64 and Linux clippy are separate PR gates.

## Self-review

This bench evidence has no person, face, memory or voice ownership. Failure
ends the scan and prevents later evidence from reviving it. No new switch,
topic, unit line or message field needs a census entry. No-rest discovery,
old report shapes and live touch policy remain unchanged; existing Controls
and reverse-dependent tests exercise those readers. V1's transient pull
probe and edge timestamps were inspected; the reuse note records why this
is a V2 evidence correction, not a claim of hardware or behavioral parity.

## Completed qualification

The final full gate ran on source commit `20c1d9c6d`, based on abc, with
7,117 source files frozen. Mac Rust 1.96.0 formatting, strict all-target
clippy across 14 affected crates, workspace build and full serial tests
passed: **7,971 passed, zero failed, 55 ignored**, 88 summaries. All six new
controls passed exactly once, including the final latch and epoch assertions.
The existing factory stand-ins passed with zero failures. Commands, time
bounds, exact source identity and raw-log hashes are in full-abc.json.

The patch was rebased unchanged onto release `409bf7268`, source commit
`37f823a1a`; the range-diff is equal. Fresh formatting, strict 14-crate
clippy and workspace build pass. Latest-base follow-ups pass **41/0**:
16 discovery, 21 same-voice, one real Prints consumer, one compiled install
marker and two CI scanner controls. All 7,122 source hashes remained fixed.
See latest-409.json. This scoped continuation does not rename the earlier
full suite as a 409 execution. Native Linux gates and independent current
reads remain separate requirements on the published head.
