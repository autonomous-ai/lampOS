# Narrow authorized test-clock plan for #3315

This is implementation planning within Tests' existing ownership of workspace test harnesses and test clocks. No new approval is required merely because the seam lives in Bus or has a test-support API. Production behavior/defaults and the original 10 ms period, 50 ms lease and one-second resync contracts must remain unchanged; an actual product-behavior change would need a yes. No code/build/test has been performed.

## Minimum clock coverage

Use a test-owned handle with pure readers for (1) logical `Instant`, (2) logical lease monotonic microseconds and (3) the wall time used to evaluate message expiry. Keep socket blocking, receive-timeout accounting, host watchdogs and cleanup on real time. No global clock, environment switch, auto-advancement on reads or background ticker.

A narrower option avoids adding a publisher wire-clock API: retain actual published header UTC. After each completed real kept publication, read the retained header using the separate `Bus::latest` accessor, assert its expected session/sequence/type, and explicitly set the fixture's expiry-evaluation wall time to that header's `at_unix_ms`. Then freeze that evaluation time until the next fixture step. This is a documented logical read-at-publication-time test, not a latency measurement. Keep the wall evaluation monotonic; a backward host UTC sample is a diagnostic failure, not silently clamped. Preserve original wire bytes, leases and freshness checks; do not rewrite or re-stamp stored messages. Explicit expiry controls advance the evaluation wall and lease clock rather than extending any deadline.

The existing `publisher_declared_with_cadence_clock` already supplies publisher subscriber-discovery cadence without changing header UTC. Reuse it if explicit publisher scheduling is needed; preserve the first empty subscriber scan and one-hour rescan horizon. The new work is the missing subscriber/reader timing, not a new global time framework.

## Bus API and build boundary

Proposed minimum support is one default-off Cargo feature, e.g. `test-clock`, and one explicit typed-subscription constructor under that feature: `Bus::subscribe_typed_with_test_clocks(...)`, taking pure `Instant` and wall-millisecond callbacks. It delegates to the ordinary Subscriber construction/receive path, differing only in clock samples. Its naming, owned closures and no-side-effects-on-read contract should follow the existing explicit publisher cadence API precedent.

`Subscriber` stores the optional test readers only under that feature. Ordinary constructors set no override and retain real defaults even in a feature-enabled test build. Without the feature, override fields and constructor are absent. Small clock helper functions fall through to the same original `Instant::now`/`wire::unix_ms` calls. No declaration, freshness, gap, notice, source, ordering or timeout rule changes.

Route only semantic Subscriber time through those helpers:

- construction timestamp (`subscribe.rs:252`);
- notice/lateness/source judgement samples (`381`, `388`, `395`);
- message admission Instant and wall expiry/age (`529–530`);
- repeated-datagram `heard_from` (`669`);
- `latest` look time and expiry (`702`, `707`);
- kept-file rescan time (`760`);
- learning the current declaration when a new publisher is accounted (`837`).

Retain **real** `queued` timeout start (`644`) and `started.elapsed` (`676`, `679`), kernel socket waits and filesystem presence timestamps. Source-liveness logic is still present; this kept-value scenario keeps logical subscriber age below its three-second source lease. It does not qualify a simulated filesystem-presence lease. Do not alter `live::touch/list` just to broaden the seam.

The feature is enabled only by Voice's dev dependency on Bus (and Bus's own focused test command), never by Voice's normal Bus dependency or workspace default features. Cargo feature unification must be explicitly checked during validation: a test build can enable it, but ordinary construction must still use real time, and a clean normal production/default workspace build must not activate the override feature. Do not assume the dev-dependency declaration alone proves this.

## Voice seam and fixture changes

Keep `LeaseReader::with_clock`'s existing clock-id string semantics. Add a `#[cfg(test)]` private constructor/override for the owned clocks and the explicit Bus test subscriber. Only that test constructor supplies callbacks; ordinary `open`/`with_clock` remain the production path.

At the frozen source, reader samples requiring the fixture adapter are construction (`privacy_bus.rs:516`), poll monotonic samples (`529`, `539`, `542`) and envelope wall evaluation (`554`). Preserve all comparisons, counters and `notices` calls. The named fixture stamps leases from the same logical microsecond reader and chooses `opened_us` in that domain; host read watchdog remains `Instant::now` with a bounded deadline. Avoid redirecting unrelated privacy worker/watch behavior to the test clock.

Fixture sequence:

1. Prime the actual publisher before the target subscriber exists; verify zero delivery. Construct the fixture reader at a chosen logical epoch.
2. Publish lease 2, verify actual kept-header identity, set the test expiry-evaluation wall, then poll until current sequence is exactly 2. Keep all notice/freshness checks active.
3. Advance logical cadence by the existing 5 ms kept-rescan interval only after that observation. Publish 3 then 4 without an intervening reader call; update evaluation wall from the actual retained 4 header and require gap handling with zero datagrams.
4. Fresh branch retains successful sequence 4. Old-reader branch sets only `opened_us` one logical second before current time, as the existing fixture already does, and requires the unchanged exact lost-value error. Do not advance the unrelated Bus silence clock by a second just to exercise the reader's opened-time predicate.
5. Replace ten attempts and 2 ms success polling sleeps with a single owned logical sequence bounded by a real host watchdog. End on the observed sequence or expected error. Preserve real publish/kept-file transport and all original assertions.

## Controls and validation gates

Before implementation, retain original source/diagnostic hashes. The bounded host-delay control in README remains useful to establish a new observation, not infer historical causation. In the candidate, hold the same continuation after a publication while logical clocks remain frozen; it must still reach the same reader event. Retain negative controls that move logical time across the unchanged >20 ms no-datagram lateness threshold, exact 50 ms lease expiry and exact one-second resync boundary. An explicit first-datagram gap control must still reject under the original rule. Also test that ordinary constructors in a feature-enabled build retain real clocks; avoid checking only the new seam's happy path.

Planning commands, not executed:

```sh
cargo fmt --all --check
cargo clippy --offline --locked -p lamp-bus --all-targets --features test-clock -- -D warnings
cargo test --offline --locked -p lamp-bus --features test-clock --no-fail-fast
cargo test --offline --locked -p lamp-bus --no-fail-fast
cargo tree --offline --locked -p lamp-voice -e features,no-dev
cargo build --offline --locked --workspace
```

The `cargo tree` command is an execution-time feature-graph check, not a current static result. Bind the default build's resolved features/compiler messages too; no test-clock feature or override may be enabled by a normal production dependency. Exact feature syntax and crate manifest placement should be checked against the workspace resolver during implementation.

Recompute the complete normal/dev/build/platform reverse dependency closure of **Bus**, not merely Voice's existing 12-package closure. Run strict Clippy and default-parallel full tests for that closure, with source guards, complete executable/case census, ordinary failure retention and no retries to fish for green. The current 12-package proof does not cover arbitrary Bus changes. Run exact-head native Linux and ARM64 gates as required; do not build ARM64 on the Mac. Broader gates are justified here by actual shared support-code changes, not by the source-only planning exercise.

Do not touch the currently running frozen comparison or prepared candidates. Implement later on the authorized owned task branch after CPU handoff and usual exact-file/one-line-commit gates.
