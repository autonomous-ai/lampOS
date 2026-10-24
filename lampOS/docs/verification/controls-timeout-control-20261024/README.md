# Controls timeout-control local evidence, 24 October 2026

PR #3369, authored head `714c30820`, base `83da28dc7`. Intel macOS only; no device or ARM64 link/execution. The four file hashes in final-gates.json bind unchanged sources before and after all local gates. The target began empty; compiler-artifacts.json retains 11 workspace package artifact paths from this exact checkout. This avoids relying only on source equality with reused workspace outputs.

Formatting and strict Controls Clippy passed. Full affected package tests: **509 passed, 0 failed, 3 ignored**. Full workspace tests were not rerun for this tests/docs-only follow-up. Official Linux ARM64, Clippy and dedicated released-pair gates remain separate.

The fresh native fixture executable rerun passed **1 / 0 / 2 ignored**. The helper was still running at its 25 ms deadline; the primary guard reaped it, the group was absent and independent waitpid reported no child. The fallback was unused. The 32,739 microsecond recorded interval spans deadline wait through cleanup checks; this is a synthetic process-cleanup observation, not touch or device latency.

The scratch mutation removed only the Owned destructor body. It failed as expected (**0 / 1**, Cargo 101) because the primary receipt was missing. Its separate fallback reaped the helper and the parent independently confirmed group absence. This does not turn a failed probe into a pass. The earlier scratch include-layout compile error is preserved in mutant-packaging-failure.txt and counted as no execution.

Actual published-pair cases remain two cases with 18 commands each and one filtered ordinary test. They do not execute this helper. Historical #3346 pair runs do not qualify the new control. No descendant-adoption, kernel-hard-deadline, live disk quota, runtime behavior or hardware acceptance claim. Raw output is retained verbatim; files.json hashes this packet's inputs.

## Completed dedicated Linux ARM64 pair gate

Original run **40087074211**, attempt 1: **2 passed, 0 failed, 0 ignored, 1 filtered**. Both case receipts show 18 commands started/reaped and group absence. Published 6408/6498 payloads remain identical (2,463,440 bytes, SHA256 fc3f73157e21c6b3f662ee469f5febeeaa56bbc84091e0b781157192a3961555). Hashes identify bytes, not signature trust.

The executed preview is `d333f10cf` (tree recorded in native-pair-audit.json), with authored PR head `714c30820`. All four authored changed files equal both that preview and its before/after source hashes; all source path/hash/symlink maps are unchanged (10,437 regular-source hashes). There are 70 base-difference paths between authored and preview trees, so no whole-tree identity is claimed. The artifact contains 195 regular files, all digests/sizes retained in the audit inventory; original tarball SHA256 is 07086ea4a484d09952ee06904fbff32488777968cf06e2635eb4e5eff9ea2d6c.

The dedicated cases intentionally filter the ordinary fixture, so they do not execute the new hanging-child control. Its Linux result must come from the ordinary ARM64 suite. Linux strict Clippy run **40087074220** passed; ordinary ARM64 run **40087074229** subsequently passed. Its checkout log names the same d333f10cf merge preview. The suite reports **7,964 passed (1 leaky), 56 skipped**, ordinary Controls fixture PASS, and process controls **12 passed / 0 failed**. The ordinary fixture includes the new timeout control; its private helper receipts are not uploaded by that workflow, so the retained CI evidence is test execution/status rather than separately retained Linux helper JSON. arm-summary.json binds the raw GitHub job log SHA256 and exact result lines. No runtime, physical or device acceptance is added.

## Reader and formatting disposition

Lead READ-OK covers authored 714c30820. Reviewer-two separately ran Mac fmt/strict Controls Clippy/full serial 509/0/3 with unchanged source and found no P1/P2. Their Linux fmt gate finding is valid: those original workflows do not run formatting. Third claimed the workflow fix on separate branch third/linux-format-gate (d57c27792); its actual Linux execution and Second's exact-source manual formatting request remain separate from these successful test/Clippy results. No retroactive formatting pass is claimed.

## Exact authored-head Linux formatting completed

Second published the separate [source-only Linux formatting packet](https://github.com/autonomous-ai/lampOS/tree/a4c6b7b95/lampOS/docs/verification/controls-cleanup-linux-format-reader-20261024). Third read its report and primary structured receipts, verified all three published receipt hashes against export.json, and checked repository/lampOS tree IDs against authored 714c30820. Formatter exit is 0, all 7,020 source hashes remain unchanged, and the owned container removal/absence check passes. This was Second execution, no ARM build/test or device execution. It closes the earlier exact-head formatting qualification; the original CI jobs still did not run formatting retroactively.

Controls #3369 landed through train 86 as lamp-voice b23ae5339. All four authored changed files also equal subsequently fetched lamp-voice 2f40cf898. Original ordinary ARM64 retained one leaky Voice journal test (same_voice::tests::each_line_the_check_says_is_said_to_its_journal); it is not erased by the successful gate. No physical acceptance is added.
