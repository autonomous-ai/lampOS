# Shared Bus fixture-clock scope proposal — unapplied

Pinned remote/source: `419d6f64b0244ed62547f7f697f808bc994a79e5`, independently matched by `git ls-remote`. This is a concrete proposal for First's narrow scope decision, not an applied API, successful build, or permission request sent by this worker. The private #3315 draft is unchanged.

Proposed new API, available only under Bus's default-off `fixture-clock` feature (or its own unit tests): `FixtureClock::{now,wall_ms}` and explicit `Bus::with_fixture_clock(Arc<dyn FixtureClock>)`. One owner supplies pure, non-advancing reads and advances checked logical time. A configured Bus and its clones pass that exact clock to publishers, typed/untyped subscribers, service-created subscribers and retained-value readers. `Bus::new`/`from_environment` always select System, including feature-enabled builds. Existing public signatures, wire schema, runtime flags, global environment and production clock-set checks do not change. Expression enables the feature only in a dev dependency. The existing Bus Debug representation is preserved.

This consciously extends #3315's subscriber-only idea to publisher UTC and presence. It is not already implemented or explicitly API-approved by #3315. First assigned Tests the fixture ownership; the full authorization audit distinguishes that from an API-shape decision. This proposal must be reviewed before any real source/API implementation.

## Time-domain inventory

| Domain/read | Proposal | Why and limit |
|---|---|---|
| Normal/guarded publisher `at_unix_ms` | Shared fixture wall only on explicit Bus | Both serialization paths; `at_us`, schema, source, sequence and boot binding unchanged. |
| Publisher subscriber-directory rescan / presence refresh / warning cadence | Shared fixture Instant by default | Existing explicit cadence-clock factory still overrides cadence intentionally. Caller must use the same domain if combined with an opted-in Bus. |
| Typed/untyped subscriber construction, began/spoke/arrived, declaration learning, repeated-datagram hearing | Shared fixture Instant | Includes subscribers constructed internally by the real Expression service and its Bus clones. |
| Subscriber admit expiry/statistical age, `latest` expiry | Shared fixture wall | Original envelope grace and payload rules unchanged. |
| Subscriber retained rescan, late and source-gone judgment | Shared fixture Instant | Same original intervals and greater-than boundaries; no fake-read advancement. |
| `Bus::latest` expiry and `Bus::publishers` source age | Same shared fixture wall | Stored messages are not rewritten/refreshed to hide age. |
| Publisher presence-file mtime and directory lease evaluation | Same fixture wall, via private clock-aware touch/list helpers | This is necessary for coherent logical SourceGone controls and avoids a residual host lease. Only owned fixture presence entries get synthetic mtimes. Default helper samples SystemTime at the original point after opening/listing. |
| Boot-token entropy / PID / filesystem operations and permissions | Real, unchanged | Identity is not an age clock; constructors may fail on actual I/O errors. No privacy validation bypass. |
| Subscriber blocking timeout `started/elapsed`, socket wait / send behavior | Real, unchanged | Frozen semantic time cannot turn a bounded real receive into an infinite wait. |
| File I/O completion, socket/parent/child watchdogs, cleanup | Real, unchanged | Disk-full, I/O errors or a host suspension exceeding a watchdog remain failures. |
| Bus CLI, logging/projection timestamps, retention/sync and logger liveness loops | Real, outside this in-process fixture API | No logger/CLI process is started by SleepingService. Do not mix an ordinary cross-process actor into a fixture root. A separate process cannot inherit an Arc clock without a separately reviewed protocol. |

Every production Bus time read in lib/publish/subscribe/live is accounted for. In the proposed actor path, remaining `Instant::now` is the subscriber's real blocking-timeout origin; remaining publisher `SystemTime::now` creates boot identity. System branches retain normal wall/Instant/presence helper calls. `wire::Header::expired_at`, expiry grace and Expression product runtime source are unchanged.

## SleepingService wrapper

An owned FrameClock starts from one real wall/Instant epoch, then advances only at explicitly checked logical ticks. All actors, including the service's typed inputs, head privacy/claim readers, mood/online retained readers and pause Bus clones, inherit it. Service construction still precedes input publication. The local-night offset samples this same wall. Each step keeps the existing loose-to-strict publication order, samples the SAME owned wall before/after publication, and passes it to `Service::step`. It does not redetermine now from host delay.

The #1198 excess-over-tick bridge is preserved with checked arithmetic: `publication_us = (wall - publishing)*1000`, then add `max(publication_us - TICK_US,0)`. Normal frames do not advance time inside publication; a narrow fixture-only after-room callback supports an explicit logical-publication-time regression. The original rejected/input-error assertions, case bodies, ticks/windows, clock-set callbacks and speech/name/privacy rules remain. Product 500 ms Voice lifespan, 50 ms future/slack/grace as applicable, 500 ms joint transport and 300 ms engine freshness stay distinct and unchanged.

## What can still fail after a real >1.1 s pause

For the selected pause AFTER joints publication and BEFORE Service wall sampling: all semantic timestamps and their age evaluations are frozen. No blocking socket wait is active there; the service subsequently drains with zero timeout. Presence cannot expire from host time either, because both its mtime and lease evaluation use the owned wall. Thus no inventoried semantic age domain should reject solely because of that host pause; this is an untested prediction, not a result.

Actual I/O failure, disk exhaustion, socket delivery loss, malformed data, an owner deliberately advancing logical time, or exceeding an unchanged real cleanup/child watchdog can still fail. A pause inside a real bounded blocking socket call can expire that real wait. A separate ordinary Bus/CLI on the same fixture root would mix clock domains and can expire packets/presence; this is outside the explicit single-domain contract, not a supported hidden fallback. Do not claim arbitrary stalled-host liveness or end-to-end hardware latency.

## Concrete controls in the proposed sources (all unexecuted)

Four Bus cases cover default System constructors / opted-in clone propagation / typed and untyped paths; exact 50+50 ms expiry/grace; original strict >20 ms Late and >3 s SourceGone plus actual presence restoration; 5 ms retained rescan and guarded-header stamps. Two Expression cases cover unchanged future 50/51 ms and joint 500/501 ms admission, and the #1198 bridge with 350 ms of explicitly advanced logical publication time. All original SleepingService tests remain.

The existing root-owned old pause observation is retained: selected case failed its original refused assertion, left 2/right 0, with 1,108 ms joints and 1,107,434 us held/joined. It is not historical ARM causation. After scope approval only, freeze a matched old/candidate controlled plan using the identical one-shot post-joints channel hook; candidate host-pause success is still unobserved. Mutation controls should remove each clock route, widen expiry/future/joint limits, freeze SourceGone, or remove the #1198 bridge and require the relevant unchanged/new assertion to fail. No successful test ends on a fixed sleep and no failed observation is retried until green.

## Gates after a scope decision, not granted here

Static conservative all-normal/dev/build/all-target/optional Bus reverse closure is 31 packages; exact manifests/lock are bound in reverse-closure.json. Feature selection changes require one fresh actual offline/locked metadata resolution after a later approved application. Planned gates include fmt, strict all-target Clippy and full tests for that actual closure, exact controlled counterfactuals, and an ordinary workspace build on fresh local outputs. Inspect actual production Bus compiler artifacts to prove `fixture-clock` absent; a printed no-dev tree or test-build feature union is not enough. Keep every original failure, exact case/ignored census and executable/child bindings. Native ARM/Linux CI and required scope/source readers remain separate later work. No local ARM build, device operation, Cargo or test has run for this proposal.
