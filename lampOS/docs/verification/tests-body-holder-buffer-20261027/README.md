# Body holder fixture: bounded writes, retained read checks

Part of [#1569](https://github.com/autonomous-ai/lampOS/issues/1569); the fixture was introduced in [#3791](https://github.com/autonomous-ai/lampOS/pull/3791).

Commit `2adcc4e209` on `02677931d` changes one test and its English Body paragraph. Product code and dependencies are unchanged. Each producer makes one nonblocking write before reading its peer, asserts that it queued more than the unchanged 4 KiB read budget, and checks EOF byte conservation. Every original line-limit, read-budget and EOF-loop assertion remains. No sleep, thread, or timeout was added to the shipped test.

The unchanged Main case at `36a42f30a` started but reached no result within a supervised five-second control. Its process group was closed; this is a reproduced timeout, not a test pass or a full baseline package result. The repaired exact case then passed under the same watchdog. These observations remain separate from the full package runs.

Current local gates on `2adcc4e209` passed: formatting, strict Clippy across all 12 metadata-derived affected packages and targets, workspace build, actual Body test compilation, and all/ignored inventories. One serial full Body command observed **646 passed, 0 failed, 0 ignored** across seven test drivers. Doc-tests declared no cases. The audit includes every ordinary case outcome and all 14 actual executable bindings. An earlier full run on the prior base also passed; it is separate history, not added to the current census.

All 10,806 tracked source files remained unchanged. All 30 sealed copies and 14 actual executable bindings passed their guards. The policy predeclared inode observation for seven metadata-derived non-test Cargo convenience aliases; their content and other bound attributes remained strict. Test-driver and sealed-copy inode checks remained strict. All owned process groups closed.

One earlier controller failed before Cargo because its environment equivalence check included the changing tool-call identifier. Its FAIL receipt remains intact. A new continuation recorded that exact exclusion and ran only the still-unstarted gates.

The own commit was carried as one equal patch to the current Main, preserving both authored file blobs, and pushed after its current-base gates. Native Linux ARM64 CI is pending the draft PR: the actual workflow runs on PRs and Main pushes, not a new own-branch push alone. No ARM build or device test ran on the Mac. Timings in the audit are raw observations with no performance or idle-host claim.
