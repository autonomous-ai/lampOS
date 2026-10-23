# Corrected updater head: official native gates

PR #3269 head `46f5f384ad94f9c2dcc5f7050991ef4325621384`, based on
`810e05109bced3beca88a5b602930cfed5fba4e5`, passed its first corrected-head
[official Linux ARM64 run](https://github.com/autonomous-ai/lampOS/actions/runs/39956841003)
and [required Linux Clippy run](https://github.com/autonomous-ai/lampOS/actions/runs/39956840831).
The PR was marked ready after both completed successfully. Third's and
Reviewer-lead's current-head review dispositions remain separate; First merges.

- Nextest: **11,067 passed, 0 failed, 69 skipped**, 199.237 seconds.
- One passing case was reported **LEAK**:
  `lamp-voice same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
  A green workflow does not erase that cleanup diagnostic.
- Doc tests: **4 passed, 0 failed**.
- Separate process harness: **12 passed, 0 failed**.
- The signing/publishing job was skipped for this PR. No signed release is claimed.

The checked merge commit `690ab0d432e51426d82720ae5d813edeab36d923` has exactly
the authored head's tree `c069dbad50071717a8cab4827d39ebc08bcac08b`.
[receipt.json](receipt.json) binds both run identities, their head, merge parents,
the artifact identity/digest and the full retained native log's SHA-256.
[native-excerpts.txt](native-excerpts.txt) contains unmodified selected checkout,
test-total and leak lines; it is not the complete log. The full output remains
available at the official run above. No build artifact is committed here.

Mac Update remains **371/0/4 ignored**, with 28 head-touch round cases and
both exact Third regression controls. Earlier dependent Skills' Mac lock
failure remains recorded in the PR; this native pass does not relabel it.
The earlier held head's gates are separate historical evidence.

[Six provisional actual-old-helper checks](../previous-helper-6408-20261023/README.md)
exercise the previous published ARM64 helper with this current Mac source.
They are independent of this native CI receipt and make no lamp, physical
power-cut, signature-trust or supported-release-pair acceptance claim.
