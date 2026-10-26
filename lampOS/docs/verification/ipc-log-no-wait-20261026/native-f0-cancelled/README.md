# PR #3528: superseded original attempt

Head `f0c7072be3582eda1d1ca6bf7464031b3e2b0434`, base `c5c7603a743690e291f85a664a7ee18624232581`. Both jobs checked preview `a924c69a24dfab1d6da5a93d2e44a7a2d3afe938` with those exact parents; its tree `637dfd238199bddd55abd25a92979359f30b6e8a` equals the authored tree.

- [ARM64 original attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40255922947/job/121357616152): **CANCELLED** during compilation in the test step. There was no nextest start/summary and no indexed terminal case. Workspace pass/fail/leak/timeout/skip counts are **unavailable**, not zero-success evidence. The changed IPC case has no native execution result in this attempt.
- The earlier, separate promotion helper completed **25/0**, exact groups **2,0,22,1,0**. Its 22-case integration suite is one component of the total.
- Workspace doctests, relay/process harnesses, artifact export and upload were skipped. No artifact or device/release acceptance is claimed.
- [Linux Clippy original attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40255923159/job/121357616836): **SUCCESS**.

This superseded attempt is preserved separately from the rebased head. No reruns or local compilation. Raw logs and metadata are retained privately; hashes are in results.json. The original native watchers used a 300-second interval and a one-hour deadline; ARM64 returned exit 1 for cancellation, Clippy exit 0.
