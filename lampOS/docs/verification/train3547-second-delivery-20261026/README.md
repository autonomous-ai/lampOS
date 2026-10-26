# Delivered train 3547: original native qualification

Train head `9ce6b924a7d374377464cca32429f37fde181a1b` was tested at preview `b1e8c6c8a105150bdb7422852948a4174d4cf757` with exact parents `cced429433211e06790f23ed8f51e07be613972d` and the train head. Both original job logs name that checkout. The preview, authored train and landed `d5dc2e87f9a64ae8d21992145d3960035582e6cc` share complete tree `762ff5505007ec03276b2955de3da31b97a7d90a`. The landed commit has its own parent identity retained in [identities.json](identities.json); it is not represented as the tested preview commit.

[Original ARM64 40276784523, attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40276784523/attempts/1) succeeds with **11,361 unique PASS, 0 FAIL, 0 LEAK and 0 TIMEOUT**, 106 profile-skipped and 0 selected without a terminal row. All ordinals reconcile exactly once; 35 identical report echoes are excluded. Both Second Voice memory admission fixtures and its IPC held-pipe/socket fixture pass once; [selected-results.json](selected-results.json) records exact names and ordinals.

Separate scopes: process harness **13/0**, workspace doctests **4/0** across 46 summaries, relay success, and promotion-helper workspace **25/0**. These are not added to nextest. [census.json](census.json) retains exact summaries, report boundaries and the interleaving limitation for doctest headers.

[Original Linux Clippy 40276784387, attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40276784387/attempts/1) succeeds with pinned workspace formatting, strict all-target workspace Clippy, and the helper's own formatting and Clippy. Promotion, signing and failure-report jobs are skipped. No original job was rerun for this audit. Binary artifact metadata is retained; its archive was not downloaded or executed.

All **13 authored files** from #3542 at `69c0f26e9754648a38ab5dcb511951d67951e0cc` and **seven authored files** from #3528 at `d8dc4cca80626faed14d7ab8f675fabfc1199d6f` have identical Git blob IDs and modes in the delivered tree. Complete, non-truncated source/tree responses and the PR changed-file counts reconcile all 20 entries. The constituent PRs were closed through the train; their obsolete preview IDs are not direct merge-delivery commits.

Earlier failed gates and controls remain independent and unwaived, including #3528's earlier sentinel failure. This later success proves neither that failure's cause nor a base-only reproduction. There is no device, provider, installation, release, acoustic or physical latency claim.
