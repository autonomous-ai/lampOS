# Body successor-socket fixture: source-only candidate

Second claimed this narrow repair on Queue comment 6390840447 after all 22 open PR file lists showed no supervisor path. The original fixed observations from packet `744d9fae32` are 1/0, 0/1, 1/0; the failed second observation reaches `next.clean_exit()` and reports `motor inhibit acknowledgement timed out`. This identifies the failed condition, not its historical cause. The failed full and 2/3 qualification remain unchanged.

The candidate on exact `da57072b2` changes only the named supervisor test and `docs/realtime-tests.md`. Every byte before its teardown is unchanged, including creation/readiness of the actual successor, replacement admission, predecessor drop and all successor-socket/ready/unreaped assertions. Only later fixture teardown changes: explicitly kill the owned successor, wait for actual reap, require a truthful non-clean exit, no IPC cleanup error, peer cleanup and directory removal. The product supervisor bytes and all timing limits are unchanged.

`actual_child_uses_same_worker_loop_and_stop_reaps_before_clean_replacement` retains positive cooperative-stop/receipt coverage. `hung_owned_child_is_killed_and_reaped_after_missing_inhibit` retains the failed-acknowledgement contract. Removing the unrelated positive stop assertion from this ownership case does not waive those contracts or establish timing on hardware. Prior #1314/#1315 startup work was read; this failure occurs at teardown, not that initial Authority assertion.

Candidate validation is UNRUN apart from workspace formatting and whitespace checks. First formatting check found a line-wrap difference; it was corrected before the passing check. No compiler allocation, new case observation, product change or device action has occurred.

## Frozen validation plan for the next available allocation

- Exactly one direct baseline observation on frozen da570, and one direct candidate observation; preserve either outcome. These are new observations, not relabelled old fc31 executions or historical-cause proof.
- One candidate complete `lamp-body` test run plus fmt and strict all-target Clippy, with test-executable/source/command bindings and owned-child cleanup. Test-only private helper has no cross-crate reader; product dependency tests are unnecessary unless the scope changes. No local ARM64 build/test.
- One negative ownership control: remove a successor peer immediately before the preserved post-predecessor assertions. The exact case must fail that peer assertion. This is a distinct deliberate mutation, never a changed product deadline or an unchanged-case retry. Restore all source bytes afterward.
- No retry-until-green, removed assertion, increased deadline, synthetic scheduling claim or historical waiver. A failure is retained and diagnosed before any changed plan. Source/head and actual resource overlap are recorded; the shared compiler allocation must be available before compilation.

This published source packet is not a passing code gate, pull-request readiness or CI result.
