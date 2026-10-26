# Mood 02485 original native checks on b234

Original [#3623](https://github.com/autonomous-ai/lampOS/pull/3623) head `02485e90a8960fa10ae72d340139af51a1a0f8d7` passed [ARM64 40420007810](https://github.com/autonomous-ai/lampOS/actions/runs/40420007810) and [Linux strict Clippy 40420007893](https://github.com/autonomous-ai/lampOS/actions/runs/40420007893), both pull-request attempt 1.

Both jobs actually checked out preview `9a1707eb2ccea8a8354f133e0517b8f46cad7e96`, ordered parents `b2344a07df6b38d2736e01297999751c26e477a5` then `02485e90a8960fa10ae72d340139af51a1a0f8d7`. Its tracked tree `577ae890dfe74826598488793fa2f6485f21ad40` equals the authored head's tree. Main was still b234 at collection. Both workflows, nextest configuration, Reader source, composed Expression runtime and Expression documentation match immutable authored Git blobs. Source/tree equality is not a binary-equality claim.

The actual selected ARM64 nextest run produced **7,546 unique PASS; 0 FAIL, LEAK or TIMEOUT**, 53 aggregate skipped tests and 114 skipped binaries, across twelve selected packages and 70 binaries. The execution window is log lines 823–8371 (166.298 s), with distinct identities and ordinals 1–7,546 and no duplicate execution or retry/flaky record. This is selected-package runtime, not full-workspace runtime qualification or an idle-host/performance result.

All **340 ordinary Expression cases actually executed and passed on ARM64** (27 library, four binary, 281 integration and 28 runtime). Both affected controls passed exactly once:

- Mood breath: `sleep_name::the_service_breathes_the_head_with_moods_breath_from_the_bus`, ordinal 1,150, 0.495 s.
- Journal: `sleep_name::an_expired_sleeping_name_is_journaled_once_by_the_service`, ordinal 1,149, 0.126 s.

The [separate current Mac carry qualification](../tests-mood-b234-reuse-20261027/) explicitly has **zero current runtime attempts** and reuses the earlier f441/90ca 340/0/0 result after eight-executable/input/environment comparisons. This native run is fresh actual Linux execution. The two platform scopes and counts are not combined or relabelled.

All **14 ordinary Main Reader controls** passed once. This does not qualify the separate Sentinel PR's root-fixture patch or any future Restart source. The isolated Linux root/setpriv DAC case remained ignored; the non-Linux ACL fallback case was not compiled. Voice contributes 3,848 passes within the selected total.

The 34 post-summary PASS rows are configured final-output reprints, 22 realtime and 12 privacy-worker. Each matches its execution identity/status/duration/ordinal and its configuration filter; no blind deduplication or extra execution is counted. The 53 skips are aggregate-only: individual skipped identities were not emitted. Separate auxiliary steps passed: promoter **25/0/0**, workspace doctests **4/0/0**, relay-signals **ok once** and process **13/0**. These are not added to nextest. Linux formatting and workspace/all-target strict Clippy, including separate promoter checks, passed on the same preview.

Artifact ID `12697812834`, size 116,093,232 bytes, has matching API/upload digest `sha256:c8bd9de477e31bfb74c304316ab64989ab42bc62422ed060866486316f4a916e`. The export reports 47 workspace binaries, two selected examples and `SHA256SUMS` (50 uploaded files). Only metadata/logs/source text were read: no archive or ARM64 binary was downloaded or executed locally, and individual payload checksums were not independently verified.

Earlier fa67/f441 native evidence retains its exact old-tree scope, including the separately preserved collector-manifest clarification. Historical failures and bounded alias dispositions remain unchanged, with no historical-cause waiver. Readiness still depends on current-head reads and the latest base; device acceptance and physical latency remain separate.

[audit.json](audit.json) exports all selected per-binary counts, both affected records, Reader controls, source/preview bindings, exclusions, configured-reprint accounting, auxiliary results, artifact metadata and retained receipts. The bounded watcher closed zero for both originals; every included collection-manifest hash was rechecked after output closure. There was no CI write or rerun.
