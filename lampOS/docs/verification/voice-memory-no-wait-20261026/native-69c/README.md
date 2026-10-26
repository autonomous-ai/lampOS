# Memory admission fixture original native evidence

PR #3542 authored head `69c0f26e9754648a38ab5dcb511951d67951e0cc` is based on `cced429433211e06790f23ed8f51e07be613972d`. Both original jobs checked out merge preview `462df0e73d397aa7c792bd88ac6538bc8a921a8f`. Its complete tree `59ce0d9e0fb66dda45efa2081ac1400680edf5a0` equals the authored tree, and its parents are exactly that base and head. [identities.json](identities.json) binds the original runs, jobs, checkouts, step outcomes and artifact metadata.

[ARM64 run 40272878802, attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40272878802/attempts/1) concluded **success**. The unique nextest census is **11,357 PASS / 0 FAIL / 0 LEAK / 0 TIMEOUT**, with **106 skipped** and **0 selected unrun**. 35 identical report echoes are excluded. The two changed Memory admission fixtures have 2 distinct terminal rows, preserved in [selected-results.json](selected-results.json). [census.json](census.json) records any missing rows and every non-passing terminal case; no unrun case is counted as passing.

Separate scopes are retained independently: 13 process pass witnesses, 1 relay success witnesses, 4 doctest passes and 0 failures across 46 summaries, and 25 helper-workspace passes with 0 failures. These are not added to nextest. Interleaved doc/tool stdout summaries are not assigned to crate headers. Skipped job steps remain skipped.

[Linux Clippy run 40272878805, attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40272878805/attempts/1) concluded **success**. Its exact step outcomes are retained. Successful strict workspace Clippy and formatting are distinct from test evidence. Neither original run was rerun for this audit.

Raw logs and API receipts remain private with their hashes recorded. Artifact metadata is retained, but no archive was downloaded or executed. This evidence qualifies only the named authored/tested tree. It does not turn previous local control-parser failures into successful procedures, and establishes no physical lamp, provider, latency, activation, signing or release result.
