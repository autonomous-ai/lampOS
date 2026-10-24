# Common Mac Voice failure audit

Frozen source tree `89b46e6f1cc57211c7746ed6c72abdf2e0108aa2`, base `9ed44f88c`. This is a source/log audit only. No Cargo/test/cache or repository source/ref/index writes were performed.

The Mac gate completed all eight stages and exited 101. Formatting, strict workspace all-target Clippy, workspace build, Eyes (357/0/5), Update (386/0/6), Skills (514/0/1) and Memory (314/0/6) passed. Voice's raw summary sum is **4,068 passed / 28 failed / 15 ignored**, but one failed child invocation is printed inside its failed parent. The top-level test count is **4,068 / 27 / 15**, including library **3,787 / 27 / 15**. The child `an_accepted_decline_allows_reports_withdrawal_without_reviving_speech` is not a second distinct failed test.

Linux completed all eight stages with exit 0: Eyes 360/0/5; Voice 4,156/0/18; Update 388/0/4; Skills 517/0/1; Memory 314/0/6. Neither the Linux pass nor Mac compilation gates waive the Mac failures.

## Observed boundaries

- **20 unique provider-worker failures** panic at `provider_worker.rs:9978`, the fixture's `Rig::publish` unwrap of `heartbeat()`. That method refreshes a 100,000 µs capture/admission authority lease and sends `Control::Authority`. The observed error is transport `ConnectionReset` (`provider.c.p`), including the one isolated child. Several logs show skipped authority snapshots and 140–579 ms stalls. This identifies the failed send, not why the worker closed.
- **Two catalog failures** observe renewal while a turn remains owned/open. Their logs include 140/214 ms holds and lapsed authority snapshots. The 100 ms fixture heartbeat lease is relevant, but elapsed stalls alone do not prove the renewal's cause.
- **Two reminder failures**: a fresh durable offer is absent after a ledger write aged its due notice; another completes a turn with `StaleOwner`.
- **Two hand-control failures**: head touch never reaches the owner within two seconds (health received 2/rejected 1/discontinuities 2/pump stopped), and the nominal waiting pump records 55 passes in one second against a maximum 10.
- **One privacy-bus failure**: all ten attempts refuse an invalid privacy message, exhausting the fixture's host-stall recovery. Its injected datagram has 50 ms expiry. The assertion does not establish whether expiry alone caused every refusal.

All nine inspected immediate failure/fixture files are byte-identical to base `9ed44f88c` and comparison `5074d860c`. Shared guards inspected are itemized in `audit.json`. This rules out a direct change in those files; it does **not** prove a baseline failure or absence of an indirect effect from Common's changed code/dependencies.

## Next verification

Do not relax authority leases, freshness checks, retry counts or assertions. First run a bounded, matched Mac baseline/candidate experiment on the same controlled load and existing cache, with one writer, retaining original full-gate failure evidence. Start with the 20 provider-worker failures grouped by fixture, the two catalog controls and the five remaining exact controls. Alternate base/candidate order and preserve each result, phase and stderr. A quiet standalone pass is diagnostic only; it does not replace the required full-suite gate. Add temporary scratch-side observations of the worker exit result and authority timestamps if the reset cannot be explained; do not suppress the failed heartbeat or renew an expired grant.

If the baseline reproduces the same boundaries, report the paired results to the fixture owners and qualify their correction on both sources. If only Common fails, trace the candidate's worker termination/control ownership path before changing code. The waiting-pump 55-pass outcome especially requires a paired observation of wake events; treating every failure as generic scheduling would be unsupported. No production bug, host-load cause, or waiver is established by this audit.
