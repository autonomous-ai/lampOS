# Privacy/search plan source audit

Reviewed commit `dcd06c1f2070d599c10ca91f8774482aae1502cc`. No confirmed P1/P2 in the bounded committed-plan/patch scope. This is not an execution grant or runtime qualification.

- All six manifest entries and all four patch envelope/result hashes independently verify. Context application was reconstructed in memory. Both original source files match base `09f8e7412` and executed snapshot `72a62a51c`; [integrity.json](integrity.json) records checks.
- `privacy_bus.rs` original lines 1224–1233/1305–1320: the same poll/read watchdog, polling cadence, retry handling and assertions remain. Added `statistics()` reads copy stored counters (`bus/src/subscribe.rs:339`) and do not judge/drain notices. The forced pause occurs after reader construction and before fresh lease 2, rather than aging that newly published lease. No extra receive/state-judging read is added.
- `coordinator/tests/second_route.rs` original lines 456–475/1690–1710/1727–1764: the same tick result, child-exit call and cleanup/exit probes remain. Diagnostic records are bounded to 64 plus overflow and printed afterward. Extra raw/logical clock reads are passive (`scenario_clock.rs:132,146,160`). Original `wait_child_exited` at 261 probes once before checking its deadline, so an expired deadline need not produce a child-exit timeout.
- Original test `second_route.rs:2517` retains its assertions and unpushed comparison. The forced watchdog arm is expected to fail the first `assert_clean`, so that invocation does not reach its later search assertions or unpushed comparison; this must not be presented as their exercise.

Clarifications/limits already substantially acknowledged by the plan:

1. Committed README line 9 and fixed-plan.json lines 101–105: the shared unavailable error cannot distinguish Late from SourceGone. A fresh-lease/aged-subscriber forced result supports that bounded mechanism, not classification of the historical failure.
2. README lines 11/15 and fixed-plan.json line 105 correctly place the forced search hold after fixture publication, with coordinator consumption unobserved. It is not reproduction of the earlier search_pushed trace.
3. README line 13 and fixed-plan.json lines 92–100 specify future artifact/source inventories, bounded owned-group cleanup and finally restoration. No runner implementation or executed cleanup evidence is included, so those remain preconditions for any later grant/run.
4. Added allocation, formatting, mutex/atomic loads and clock reads have observer overhead despite deferred stderr. The plan discloses this at README line 15; neither normal nor forced traces establish the earlier uninstrumented cause.

During this audit, no Cargo, formatter, build, test, fixture, target or remote post was run. No actual repository source/index/ref/worktree was changed. Only this isolated audit packet was written.
