# PR #3524: original native qualification on 41233

Head `ca70965343208bf64f8be03d53eb5705b811ec72` is based on `41233c1b6954abc812186d7014a7adb19422e29e`. Both original attempt-1 jobs checked out `e4d5a3cd372909e7cb079dd3efe9fc873efa53fd`, whose exact parents are that base and head. Its complete tree `142244e86997e692b7fe250b837947d3538bf8cc` equals the authored tree.

[ARM64 run 40256859060](https://github.com/autonomous-ai/lampOS/actions/runs/40256859060/job/121360471018) succeeds: **11,328 unique PASS / zero FAIL, LEAK or TIMEOUT / 106 skipped / zero selected unrun**. All ordinals reconcile exactly once; 35 identical displayed echoes are excluded. The five Turn-end-wire library cases are included in that census.

The separate process harness passes **13/0**, including missing-executable and held-startup return cases. Workspace doctests pass **4/0 across 46 summaries**, and relay-signals succeeds. The separate whole promotion-helper command passes **25/0**, comprising library 2, main 0, promotion integration 22, workflow integration 1 and docs 0. None of these separate scopes is added to the nextest count.

[Linux Clippy run 40256859116](https://github.com/autonomous-ai/lampOS/actions/runs/40256859116/job/121360470909) succeeds, including pinned workspace formatting, strict workspace/all-target Clippy and the helper's separate formatting/strict Clippy.

[results.json](results.json) binds terminal jobs, checkout parents/tree, unique counts, original log/metadata hashes and artifact metadata. [selected-results.txt](selected-results.txt) retains compact outcomes. The original read-only watches used 300-second intervals and one-hour deadlines. No workflow was rerun or local test/build launched for this audit. Earlier c5 evidence remains separate. PR-profile skips are not full-nightly coverage; artifact metadata is not an independently downloaded or executed binary, and no release, lamp or latency acceptance is claimed. Raw logs and metadata remain privately retained.
