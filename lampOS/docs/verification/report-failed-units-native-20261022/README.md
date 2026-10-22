# Failed-unit reports: native CI evidence

[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764), head `99d92073823cc9e119506b8fde05640274d22eee`.
Qualification complete: **yes**.

[ARM64 run 39698837988](https://github.com/autonomous-ai/lampOS/actions/runs/39698837988)
and [Linux Clippy 39698837995](https://github.com/autonomous-ai/lampOS/actions/runs/39698837995)
were checked against their terminal metadata, checkout logs and GitHub commit parents.
The tested base is `0566796d79c162077fd9b254956d66e3728adbb3`; merge hashes are retained in [ci.json](ci.json).
The base supplied at assignment was `515e43d3abc37db719338883d0aa7e9465ca94ac`. The actual CI base above
comes from both checkout logs, run API associations and merge-commit parents.

```text
Summary [ 171.328s] 9917 tests run: 9917 passed (1 leaky), 63 skipped
```

Doc tests: 4 passed / 0 failed /
0 ignored. Own-harness tests:
12 passed / 0 failed.
Relay success: **yes**; new Cloud tests passed: **3/3**.
The JSON records their exact results, 1 leaky pass, skips,
8 ARM example-output collision warnings, infrastructure notices,
log line numbers and raw evidence hashes. Strict Clippy reported
0 Rust warnings. The workspace was selected in full;
the PR profile still skips realtime tests.

Source: [report implementation](https://github.com/autonomous-ai/lampOS/blob/99d92073823cc9e119506b8fde05640274d22eee/lampOS/crates/cloud/src/reports.rs),
[wire contract](https://github.com/autonomous-ai/lampOS/blob/99d92073823cc9e119506b8fde05640274d22eee/lampOS/crates/cloud/tests/it/report_contract.rs).

This is PR-profile native Linux evidence, with realtime tests skipped; it is
not a physical-lamp or latency measurement. Artifact metadata is retained,
but the payload was not downloaded or independently verified. Any earlier
failed results remain separate. Qualification errors, if any, are listed in ci.json.
