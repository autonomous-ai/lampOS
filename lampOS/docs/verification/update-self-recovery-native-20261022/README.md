# Updater recovery: initial native qualification

Recorded 2026-10-22 01:56 +07. [PR #2709](https://github.com/autonomous-ai/lampOS/pull/2709)
head `52b2f5f3dc21dabda8f043650295dd8984ccb657` passed
[ARM64 39652804662](https://github.com/autonomous-ai/lampOS/actions/runs/39652804662)
and [x86 Clippy 39652804524](https://github.com/autonomous-ai/lampOS/actions/runs/39652804524).
The checkout was merge `9f4a8c85d7adfb36a2c02aafc3d56468b568f390`, with
parents `849c0829fd29837bc28fd8b44341e361550e7918` and the PR head, verified
against the GitHub commit API and the checkout log.

```text
Summary [ 166.173s] 9795 tests run: 9795 passed (1 leaky), 63 skipped
```

The separate doc-test stage reports **4 passed / 0 failed**;
`relay_signals: ok`; own-harness tests report **12 passed / 0 failed**.
All 19 new updater tests passed. The leaky pass is the existing Voice test
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
[ci.json](ci.json) preserves exact output, line numbers, log hash and
GitHub artifact metadata. The artifact payload was not downloaded or
independently hashed. Mac evidence remains **296 passed / 0 failed /
2 ignored / 0 filtered out**, with fmt and strict updater Clippy passing,
as [recorded on the tested head](https://github.com/autonomous-ai/lampOS/blob/52b2f5f3dc21dabda8f043650295dd8984ccb657/lampOS/docs/verification/update-self-recovery-20261022/README.md).

**Historical result; not the final ready line.** First's
[review](https://github.com/autonomous-ai/lampOS/pull/2709#issuecomment-6323539077)
requests reuse of `output_within` with `JOURNAL_WITHIN`, removal of the
separate query implementation, cut fixtures and added dev-dependency,
and `Fixes #2700` in the body. That revision needs its own Mac and native
gates. The parser and recovery readiness behavior stay. No full local
workspace gate, local ARM64 gate or lamp action was run.
