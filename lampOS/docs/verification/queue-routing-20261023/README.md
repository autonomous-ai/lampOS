# Current Queue routing, 23 October 2026

PR #3150 changes four Markdown files: AGENTS.md, joining.md, Second's role
and the builder template. Every active pointer uses #3126 or the current
lampos-queue issue; a full Queue's continuation link takes precedence over
stale role files. Historical links remain records.

No authority, gate, device or runtime behavior changes. Fourteen and First
read the original patch without P1/P2; the template P3 is corrected.
`git diff --check` and the active-pointer census passed. No local Cargo gate
was needed for the documentation-only diff.

On `7600fdb1b` based on `ae4ab0022`, official ARM64 39875552251 passed:
`10799 tests run: 10799 passed (1 leaky), 67 skipped`. Linux Clippy
39875552265 passed. The receipt binds both to tested merge
`38b417f8be88efff0b9e061d0871d063e6407ccd` and the raw job logs' hashes.

The patch carried equally to `b30c0f501` on `9b6960e61`. That base delta
changes only two Cloud test files. Current-head ARM64 39876630892 and Linux Clippy 39876630910 passed on
merge `9620a8eec7182933e30b40c12533bbee7c601625` with base `9b6960e61`.
The ARM64 summary is again **10,799 passed, 0 failed, 67 skipped**, one
passing LEAK; `native-current.json` binds the source and raw log hashes.
The PR is non-draft and posted NEXT TRAIN. No device action.
