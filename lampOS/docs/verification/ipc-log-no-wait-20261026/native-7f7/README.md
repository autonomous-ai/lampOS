# PR #3528: original native CI audit

Authored head `7f7dfe42759a7d637e708804e13b78bee7e3c10e` is based on `41233c1b6954abc812186d7014a7adb19422e29e`.
Both attempt-1 CI jobs checked preview `f2807ec99becc9da850ecdb39ef4c2897bb8d8d7`, with those exact
base/head parents. Its tree `96b9eb9c699d044c297d8644e394e0cd74253500` equals the authored tree.

- [Original ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/40256867184/job/121360682372): SUCCESS. **11,328 unique PASS / 0 FAIL / 0 LEAK / 0 TIMEOUT / 106 skipped**, whole workspace under PR profile. All numbered terminal cases are accounted for once; 35 replayed PASS display rows are excluded. The changed full-journal IPC case passes exactly once in that indexed census.
- Separately: **12/0** Turn-end-wire process harness, **4/0** workspace doctests across 46 groups, and **`relay_signals: ok`**. The two harness binaries excluded from nextest ran here. None of these is added to the nextest census.
- Promotion helper: **25/0 overall**, from exact summaries **2,0,22,1,0**. The **22/0** promotion integration component is not the whole helper count. This command is also separate from workspace nextest.
- [Original Linux Clippy](https://github.com/autonomous-ai/lampOS/actions/runs/40256867139/job/121360495181): SUCCESS. Strict workspace/all-target Clippy and promotion-helper formatting/strict Clippy passed.

No CI finding or rerun. The skipped PR-profile tests remain unrun; this is not
the nightly full/realtime profile. These original native runs exercise the
ordinary IPC fixture, not the host-only deliberately broken variants. The
unchanged Polled-pipe race with another writer remains outside the fixture.
No physical latency, persisted journal delivery, release publication or device
acceptance is claimed. Promotion/publishing jobs were skipped as recorded.

[results.json](results.json) binds commands, terminal census, actual checkout,
commit trees/parents, run metadata and raw-log hashes. Selected literal evidence
is in [selected-results.txt](selected-results.txt). Raw logs and metadata remain
private; GitHub access depends on retention and permissions. Artifact metadata
is recorded, but no artifact was downloaded or executed locally. Native watchers
used a 300-second interval and one-hour deadline; their terminal receipts are
retained. No local compiler was used in this audit.
