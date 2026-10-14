# Native gate failures, 14 October 2026

`native-first.txt` is the unedited failed build step from GitHub native
Ubuntu 22.04 ARM run 38559498265 on Task 33 commit `42bdf65a`. The new
Linux-only strip child closure needed an explicit `io::Result<()>` type
(E0282). Fixed in the task branch working tree; no tests or artifacts were
produced in that run. A Mac build cannot establish that cfg-gated body compiles.

`native-baseline-first.txt` is the unedited failed test step from First's
baseline run 38556357172. Its release build passed; tests stopped with
1,866 passed, two failed and 12 inherited ignored across 126 completed
harnesses. The two failures are unchanged Records JSON atomic-write tests
returning EPERM as an ordinary user, consistent with the known O_NOATIME
ancestor ownership issue. Remaining suites were not reached. See the exact
counts/identities in `native-first-results.json`.

Reported on [Task 33](https://github.com/autonomous-ai/lampOS/issues/711#issuecomment-6167873860).
Records and the workflow were not changed by this task. Neither failed run
is a ready gate or a lamp measurement. Third has no lamp; calibration,
applied controls and physical latency remain unshown.
