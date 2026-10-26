# PR #3528: original native CI audit

Authored head `6e177dee0c40557fe7ceedc9b414734c26add031` is based on `0c1f6437ee57eae3416e3281d91b813d1c3bec1d`.
Both attempt-1 CI jobs checked preview `52eba79d15f2794feb814a4ca6d652f98b8fff6e`, with those exact
base/head parents. Its tree `41533c6ef7fba4703d1327f5706ada8193b759c2` equals the authored tree.

- [Original ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/40258770126/job/121366308498): SUCCESS. **11,343 unique PASS / 0 FAIL / 0 LEAK / 0 TIMEOUT / 106 skipped**, whole workspace under PR profile. All numbered terminal cases are accounted for once; 35 replayed PASS display rows are excluded. The changed full-journal IPC case passes exactly once in that indexed census.
- Separately: **13/0** Turn-end-wire process harness, **4/0** workspace doctests across 46 groups, and **`relay_signals: ok`**. The two harness binaries excluded from nextest ran here. None of these is added to the nextest census.
- Promotion helper: **25/0 overall**, from exact summaries **2,0,22,1,0**. The **22/0** promotion integration component is not the whole helper count. This command is also separate from workspace nextest.
- [Original Linux Clippy](https://github.com/autonomous-ai/lampOS/actions/runs/40258770130/job/121366308679): SUCCESS. Strict workspace/all-target Clippy and promotion-helper formatting/strict Clippy passed.

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
