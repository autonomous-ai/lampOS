# Shared Bus fixture clock: scope proposal only

Ready for lamp-voice: no

The [concrete unapplied Rust patch](shared-bus-fixture-clock.unapplied.patch) proposes one explicit, default-off fixture clock per Bus root. It covers publisher header wall time, typed and untyped subscriber admission/latest/cadence, retained Bus reads and presence leases, including subscribers created by Expression's real service. Ordinary constructors remain System. Socket/I/O/cleanup watchdogs remain real; production expiry, freshness, privacy and clock-set rules stay unchanged.

This extends the subscriber-only #3315 proposal. Fixture ownership is established; authorization for this broader shared API has not been established. No source worktree was changed, no API implemented, and no compilation, test or container ran for this proposal. The six proposed boundary controls and mutation/counterfactual plan are unexecuted. The source was pinned at 419d6f64b; none of its affected source paths changed through 4270083a2. Any later implementation requires a fresh source/feature graph and the applicable gates, including actual normal-build compiler proof that the fixture feature is absent.

[Time domains and limits](scope-and-domains.md) account for source presence, future/expiry and service-created readers, and distinguish semantic time from real waits. [Planned controls/gates](scope-control-gate-plan.json) retain #1198's publication-order bridge and every original assertion. The conservative reverse closure has 31 packages; actual feature resolution remains future work.

Root read the complete 733-line proposal and its time-domain/control plan. This establishes a reviewable scope decision, not compile correctness or a ready change. The existing controlled stale-input negative remains separate and proves no historical ARM failure cause. No local ARM64 build or device action.
