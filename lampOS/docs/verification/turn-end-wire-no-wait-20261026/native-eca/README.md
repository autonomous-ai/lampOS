# PR #3524: original native CI audit

Authored head `ecaed2484b2df1b1723e222d90647825f3e65764`, based on `c5c7603a743690e291f85a664a7ee18624232581`.
Both original attempt-1 jobs checked preview `78a942629db15a84eb91970f0041aa272940302d`. Its parents are the
recorded base and authored head, in that order; its tree
`60ec89d89f1c25e717d0c4e477e4aef0eec570c5` is exactly the authored tree.

- [ARM64 run 40253857057](https://github.com/autonomous-ai/lampOS/actions/runs/40253857057/job/121351373956): SUCCESS, **11,325 unique PASS / 0 FAIL / 0 LEAK / 0 TIMEOUT / 106 skipped**, PR profile, whole workspace, 4 cores, 210.034 s. All numbered outcomes 1–11,325 are present once before the summary; 35 later replayed PASS rows are excluded.
- Separately: **13/0 process-harness cases**, including the new startup-return case; **4/0 doctests** across 46 groups; `relay_signals: ok`. The two custom binaries excluded from nextest therefore ran separately. These are not added to its census. Promotion helper tests are another separate **25/0**.
- [Linux Clippy run 40253857128](https://github.com/autonomous-ai/lampOS/actions/runs/40253857128/job/121351373981): SUCCESS. Strict workspace/all-target Clippy and the promotion helper's formatting/strict Clippy passed.

No native CI finding. No rerun, local compiler, deployment or device action.
The PR profile skips 106 tests; this is not the nightly full/realtime profile.
Promotion, release publishing and failure-report jobs were skipped as recorded;
this audit does not claim a published release. The uploaded artifact metadata
is recorded, but no artifact was downloaded or run locally.

[results.json](results.json) binds jobs, commands, actual checkout, immutable
commit metadata and raw-log hashes. [selected-results.txt](selected-results.txt)
keeps compact literal outcomes without private paths or personal identifiers.
Raw logs/metadata remain private; GitHub evidence access depends on its retention
and permissions. Reported test milliseconds are host observations, not lamp
latency measurements. The 300-second/one-hour native watcher exited successfully
because the run had completed before its first interval.
