# Mood 0bd3 original native checks on 4bff

Original [#3623](https://github.com/autonomous-ai/lampOS/pull/3623) head `0bd3f854c03806b69c4a8d17b89c29a775db350e` passed [ARM64 40425319056](https://github.com/autonomous-ai/lampOS/actions/runs/40425319056) and [Linux strict Clippy 40425319013](https://github.com/autonomous-ai/lampOS/actions/runs/40425319013), both pull-request attempt 1.

Both actual checkouts were preview `9052bb250e07e4015b40d10ffdc96fedd12e849b`, ordered parents `4bff5f2c517bf64d4a32c31a90e0b24dc1a4810a` then `0bd3f854c03806b69c4a8d17b89c29a775db350e`. Its tracked tree `87a71bdf47b90af4800d84d98147dfe16afd5c79` equals the authored head's tree. Main remained 4bff at collection. Both workflows, nextest config, Reader source, composed Expression runtime and Expression documentation match immutable authored Git blobs; source equality is not binary equality.

The actual selected ARM64 run produced **7,551 unique PASS; 0 FAIL, LEAK or TIMEOUT**, 53 aggregate skipped tests and 114 skipped binaries, across twelve packages and 70 binaries. The execution window is log lines 828–8381 (158.062 s), with distinct identities and ordinals 1–7,551, no duplicate execution and no retry/flaky records. This is selected-package runtime, not full-workspace runtime or a performance claim.

All **340 ordinary Expression cases actually executed and passed on ARM64**: 27 library, four binary, 281 integration and 28 runtime. Both affected controls passed once:

- `sleep_name::the_service_breathes_the_head_with_moods_breath_from_the_bus`: ordinal 1,150, 0.213 s.
- `sleep_name::an_expired_sleeping_name_is_journaled_once_by_the_service`: ordinal 1,147, 0.065 s.

The [separate current Mac carry qualification](../tests-mood-4bff-reuse-20261027/) has **zero current runtime attempts** and explicitly reuses **ORIGINAL f441/90ca 340/0/0**, after executable/input/environment comparisons. Its 02485 handoff also has zero runtime. This native run is fresh actual Linux execution; these platform scopes and counts are not combined or relabelled.

All **14 ordinary Main Reader controls** passed once. This does not qualify a separate Sentinel or future Restart patch. The isolated Linux root/setpriv DAC case remained ignored; the non-Linux ACL fallback case was not compiled. The 34 post-summary PASS rows are configured final-output reprints (22 realtime, 12 privacy-worker): each matches original identity/status/duration/ordinal and its actual config filter. No blind deduplication or extra execution was counted. The 53 skips are aggregate-only; individual skipped identities were not emitted.

Separate auxiliary steps passed: promoter **25/0/0**, workspace doctests **4/0/0**, relay-signals **ok once**, process **13/0**. They are not added to nextest. Linux formatting and workspace/all-target strict Clippy, including separate promoter checks, passed on the same preview.

Artifact ID `12700260155`, size 116,115,534 bytes, has matching API/upload digest `sha256:a3ef6c30d29e76aeb3cd6035d563197d161081bd1fb0049dd27b80515b880e0a`. Export reports 47 workspace binaries, two examples and `SHA256SUMS` (50 uploaded files). Only metadata/logs/source text were read; no archive or ARM64 binary was downloaded or executed locally, and individual payload checksums were not independently verified.

Older native results, historical failures and bounded alias dispositions keep their original source scopes. Current-head reads and latest-base disposition remain separate, as do device acceptance and physical latency. [audit.json](audit.json) exports selected per-binary counts, both affected records, Reader controls, source/preview bindings, exclusions, configured-reprint accounting, auxiliary results, artifact metadata and retained receipts. The bounded watcher closed zero for both originals; all 31 included collection-manifest entries were rechecked after closure. No CI write or rerun occurred.
