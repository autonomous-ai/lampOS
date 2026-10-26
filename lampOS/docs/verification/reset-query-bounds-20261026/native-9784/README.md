# Reset-query original native qualification

PR #3631 authored head `9784e3e26407b63584930c1303d60b722c3ddb7c`, base `b2344a07df6b38d2736e01297999751c26e477a5`. Both original attempt-1 jobs actually checked out preview `393f06d1a514262c53669d0ff2419e97e8a510c5`, with ordered parents [base, head] and full tree `8eff6826b4f12dbcf6c7087aece304c0a2b15d54` identical to the authored tree. No workflow was rerun.

[ARM64 run 40417716476](https://github.com/autonomous-ai/lampOS/actions/runs/40417716476/attempts/1): **success**. Unique nextest terminal census: **1438 PASS, 0 FAIL, 0 LEAK, 0 TIMEOUT**, 9 skipped, 0 selected unrun. 0 identical report echoes excluded. Seven query controls have 7 distinct terminal rows; missing and nonpassing rows are explicit in census.json. This is the dependency-selected PR suite, not an inferred full-workspace run.

Separate scopes: 13 process pass witnesses, 1 relay success witnesses, 4 doctest passes/0 failures across 46 summaries, and 25 promotion-helper passes/0 failures. These are not added to nextest. Skipped promotion jobs are not executed promotion, signing or release evidence.

[Linux Clippy run 40417716408](https://github.com/autonomous-ai/lampOS/actions/runs/40417716408/attempts/1): **success**. Exact jobs/steps and checkout witnesses are retained independently from runtime evidence. Raw API/log bytes and complete terminal census remain in scratch with hashes. The sanitized packet contains no binaries or scripts. Historical host and isolated negative-control results retain their own source identities and are not added here. Native query tests do not establish installed reset acceptance or hard kernel-I/O bounds.
