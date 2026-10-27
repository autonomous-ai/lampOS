# Common 314 native CI, 27 October 2026

Ready for lamp-voice: no

[ARM64 40504365418](https://github.com/autonomous-ai/lampOS/actions/runs/40504365418)
and [Linux Clippy 40504365398](https://github.com/autonomous-ai/lampOS/actions/runs/40504365398)
completed successfully for published head `314a0f6bcb4061e360b85b8e0c9f0a9048cba8cd`.
Both supported watchers exited zero. The [receipt](receipt.json) binds the
actual checkout: merge `433935815339d6d773d44e2107635db7d73b7a0c`, tree
`fd5a3439849af70dd65fc06927163cc6cbb23d78`, with parents
`241f51500f258ea06594f9edf3928a702d6a2038` and the published head.
The published authored source remains tree `8d93ad178efd868fbf728d4d49bd81b48c28e2ed`
on base `3b9bfb628e1c416926b958fecf56f65c539dc533`. These CI results do not
qualify the current mutable worktree, a later carry, or the earlier private preview.

ARM64 records **11,905 unique outer passes, zero failures, leaks, timeouts or
duplicate pairs**. Its summary reports **106 skipped**; no discovery catalog or
complete skipped-identity census was exported, so no identities are invented.
The original GitHub step log independently matches the full terminal set.
Repeated final-status lines and nested libtest output are not extra executions.
Profile `pr` selects `all()` in the pinned configuration; stale workflow comments
do not establish the selection.

All [112 required cases](required112-terminal-ledger.json) and all
[525 current cases](current525-terminal-ledger.json) passed once. Their 25 overlaps
are not added: the mapped union is 612. The separate
[Linux repair](supplemental-linux-repair.json) passed once. It preserves all eight
original assertions and adds one ticket-denial assertion, then tests cleanup of
an explicitly invalid stale source while preserving the enrollment guard.
It does not create valid V3 proof or demonstrate live-grant revocation.
Its reported 0.005 seconds is test duration, not acoustic or physical latency.

Separate checks cover **47 workspace doctest targets, 4 passes / 0 failures**,
**one relay aggregate OK**, and **13 process cases / 0 failures**. They are not
added to nextest. Export completed for **48 binaries and two examples** with a
complete SHA256SUMS reported by the exporter. Artifact `12736055317` is
118,176,415 bytes; its API and upload-log ZIP digest agree. The archive exceeds
the 32 MiB audit download cap and was not downloaded. Individual exported hashes
were not independently validated, and the log does not supply all 50 distinct
hashes; no per-binary digest manifest is claimed. Original text-log archives and
raw/API evidence remain private, with hashes in the receipt.

The [local qualification](../common-8d93-qualified-local-20261027/README.md)
at [e253b58871](https://github.com/autonomous-ai/lampOS/commit/e253b5887189f7e363f4bacc72569eb4835b345f)
remains separate. The [earlier ARM failure](../common-3e9-native-failure-20261027/README.md)
at [0a5c4adb1a](https://github.com/autonomous-ai/lampOS/commit/0a5c4adb1ad1d26be51142ea81656473331ee7a6)
remains failed: 2,780 passes / one failure / 8,961 selected cases unrun.
No later green result rewrites that outcome.

Current-base qualification, current-head readers, privacy acceptance, matching
pins and assigned lamp/device/physical acceptance remain **HOLD**. This packet
makes no Ready transition or hardware acceptance claim.
