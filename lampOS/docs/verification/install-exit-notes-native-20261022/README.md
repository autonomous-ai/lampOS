# Exit-note installation: native CI evidence

[PR #2767](https://github.com/autonomous-ai/lampOS/pull/2767), head `19d1a46d952d8301871a69caf48c04d5c21c0bfa`.
Qualification complete: **yes**.

[ARM64 run 39701589308](https://github.com/autonomous-ai/lampOS/actions/runs/39701589308)
and [Linux Clippy 39701589363](https://github.com/autonomous-ai/lampOS/actions/runs/39701589363)
were checked against their terminal metadata, checkout logs and GitHub commit parents.
The tested base is `372188cc71881e0eeac8d1cf79e7f2a987ce7069`; merge hashes are retained in [ci.json](ci.json).
The branch base was `fc800db6f839cf006098f73ee84942572acbccfa`. The actual CI base above
comes from both checkout logs, run API associations and merge-commit parents.

```text
Summary [ 173.949s] 9951 tests run: 9951 passed (1 leaky), 63 skipped
```

Doc tests: 4 passed / 0 failed /
0 ignored. Own-harness tests:
12 passed / 0 failed.
Relay success: **yes**; added installer/Cloud fixtures passed: **6/6**.
The JSON records their exact results, 1 leaky pass, skips,
8 ARM example-output collision warnings, infrastructure notices,
log line numbers and raw evidence hashes. Strict Clippy reported
0 Rust warnings. The workspace was selected in full;
the PR profile still skips realtime tests.

Updater integration subsets (already included in nextest):
76 install tests and 99 round tests passed.
The round fixture reads units while skipping the installed per-unit drop-in directories.

Source: [installer fixtures](https://github.com/autonomous-ai/lampOS/blob/19d1a46d952d8301871a69caf48c04d5c21c0bfa/lampOS/crates/update/tests/install.rs),
[directory-aware round fixture](https://github.com/autonomous-ai/lampOS/blob/19d1a46d952d8301871a69caf48c04d5c21c0bfa/lampOS/crates/update/tests/round.rs#L146),
[Cloud fixtures](https://github.com/autonomous-ai/lampOS/blob/19d1a46d952d8301871a69caf48c04d5c21c0bfa/lampOS/crates/cloud/src/reports.rs).

This is PR-profile native Linux evidence, with realtime tests skipped; it is
not a physical-lamp or latency measurement. Skipped identities were not emitted
by the retained log; its summary records the count. Artifact metadata is retained,
but the payload was not downloaded or independently verified. Any earlier
failed results remain separate. Qualification errors, if any, are listed in ci.json.
