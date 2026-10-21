# Requested audio Actions correction: documentation checkpoint

**ready: no — host gate FAILED (exit 1).** The 36 complete consumer suites
recorded **8,773 passed, 1 failed, 48 ignored and 0 filtered**. All 17 stages
completed. Workspace formatting, strict all-target Clippy, build, renderer,
package and disposable installation passed. This own-log checkpoint publishes
reviewable evidence only; it neither commits the runtime correction nor
supplies runtime, native Linux or physical-lamp qualification.

[Complete evidence](complete/README.md) preserves that failure, the passing
23-test Actions speaker suite and actual Voice Ticket expiry/no-revival test.
[Focused evidence](focused/README.md) retains the old-runtime failure,
intermediate fixture failure and final 120-test Actions pass. All ten copied
JSON records are byte-identical to the feature receipts. Only historical links
in the copied complete README changed; checksum lists were regenerated.

[correction.json](correction.json) stores the exact four-file diff from
`c74b0f710ecac48f8668c6ab8258777788068c0a` as `patch_utf8`, its decoded SHA-256,
and all four before/after source hashes. Decode the string as UTF-8 before
applying it. Applying the patch to independent copies of those starting blobs
reconstructed the exact frozen correction bytes. No source worktree, index,
build or test was changed or run during this validation. Integration base is
`bd43346de29bba069f741baa3eb32d03c6d5794f`; the complete source proof binds
2,204 files to aggregate
`1e624e3d1ca0f315399c851ef1076668bcef57b11c86970ce934d70d9a4c2d40`.
The old checkout commit alone does not identify the corrected working tree.

[Eyes diagnosis](eyes-diagnosis.json) binds the failed test source to the same
bytes in the integration base. Its unconditional `/dev/full` open failed with
`NotFound` before the assertions on this macOS host. The failed consumer log
is identified by SHA-256 and lines 2657–2667. The earlier
[Linux ARM64 run](https://github.com/autonomous-ai/lampOS/actions/runs/39553990174)
shows that test passing at line 2762 on merge
`3fd5914f5adb88fc3bd17054736cb47ad1a2693d` of `c74b0f710…` into `bd43346de…`.
That native result is historical and does not qualify the later correction.

The [source delta](complete/source-delta.json) uses the immutable
[archived baseline](https://github.com/autonomous-ai/lampOS/blob/d2e171f9ac5af88955e631812fa0038b24fa1af8/lampOS/docs/verification/requested-audio-host-20261021-current/source-files.json).
Earlier [full-content qualification](https://github.com/autonomous-ai/lampOS/blob/ae3384f301c364938eb02afb23e68096b2d0825e/lampOS/docs/verification/requested-audio-content-20261021/README.md)
remains tied to its recorded source version. All 2,204 gate inputs and five
checkout-only fixtures matched after completion. No audio was played, device
accessed or external service called for this checkpoint. Native qualification
of the correction and physical playback remain outstanding.

[SHA256SUMS](SHA256SUMS) covers every checkpoint file except itself. Raw logs,
whole source maps and generated binary/audio artifacts are not duplicated.
