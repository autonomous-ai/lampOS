# IPC current carry: original native checks

PR #3528 authored head `d8dc4cca80626faed14d7ab8f675fabfc1199d6f`, based on `cced429433211e06790f23ed8f51e07be613972d`, was checked out as preview `8df5d3993a3f9b1394e9d6fb5ee16b5560acc2fa` in both original jobs. The preview has the exact base/head parents, and its complete tree `8ad5c0135d44b1620596b4f7a2dd41a2b30d2067` equals the authored tree. The packet binds actual job checkout lines, API receipts and raw-log hashes in [identities.json](identities.json).

[ARM64 40272890604, attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40272890604/attempts/1): **success**. Nextest reports 11,357 unique PASS, 0 FAIL, 0 LEAK and 0 TIMEOUT; 106 profile-skipped and 0 selected without a terminal row. The census excludes 35 identical display echoes and checks ordinal coverage. The changed IPC case has 1 unique terminal observation(s), retained exactly in [selected-results.json](selected-results.json). Failures and incomplete selections, if any, remain explicit.

Separate scopes: workspace doctests report 4 passed and 0 failed across 46 summaries; the process harness has 13 passing rows; relay has 1 success line(s). The standalone promotion-helper workspace reports 25 passed and 0 failed. These are not added to nextest; [census.json](census.json) keeps exact rows and their limits. Skipped job steps are recorded as skipped, never passed.

[Linux Clippy 40272890603, attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40272890603/attempts/1): **success**. The command census includes pinned workspace formatting, strict all-target workspace Clippy, and the separate promotion helper's formatting and Clippy. Neither job was rerun for this audit. Binary artifacts were not downloaded or executed; only API metadata was retained.

The earlier a8d6 sentinel failure remains independent and unwaived. A later success does not prove its cause, establish a base-only reproduction, or erase it. This is native CI evidence for this exact original head; there is no device, provider, physical latency, installation or release claim.
