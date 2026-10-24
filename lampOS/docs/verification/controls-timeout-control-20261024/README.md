# Controls timeout-control local evidence, 24 October 2026

PR #3369, authored head `714c30820`, base `83da28dc7`. Intel macOS only; no device or ARM64 link/execution. The four file hashes in final-gates.json bind unchanged sources before and after all local gates. The target began empty; compiler-artifacts.json retains 11 workspace package artifact paths from this exact checkout. This avoids relying only on source equality with reused workspace outputs.

Formatting and strict Controls Clippy passed. Full affected package tests: **509 passed, 0 failed, 3 ignored**. Full workspace tests were not rerun for this tests/docs-only follow-up. Official Linux ARM64, Clippy and dedicated released-pair gates remain separate.

The fresh native fixture executable rerun passed **1 / 0 / 2 ignored**. The helper was still running at its 25 ms deadline; the primary guard reaped it, the group was absent and independent waitpid reported no child. The fallback was unused. The 32,739 microsecond recorded interval spans deadline wait through cleanup checks; this is a synthetic process-cleanup observation, not touch or device latency.

The scratch mutation removed only the Owned destructor body. It failed as expected (**0 / 1**, Cargo 101) because the primary receipt was missing. Its separate fallback reaped the helper and the parent independently confirmed group absence. This does not turn a failed probe into a pass. The earlier scratch include-layout compile error is preserved in mutant-packaging-failure.txt and counted as no execution.

Actual published-pair cases remain two cases with 18 commands each and one filtered ordinary test. They do not execute this helper. Historical #3346 pair runs do not qualify the new control. No descendant-adoption, kernel-hard-deadline, live disk quota, runtime behavior or hardware acceptance claim. Raw output is retained verbatim; files.json hashes this packet's inputs.
