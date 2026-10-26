# Promotion-helper count scope correction

The promotion-helper command passed **25 tests, not 22 overall**, in all five
original ARM64 runs below. Its five terminal summaries are **2, 0, 22, 1, 0
passes**, each with zero failures or ignored tests: library, main binary,
promotion integration, workflow integration, and doctests. **22/0 remains the
correct promotion-integration count**; it omits the two library tests and one
workflow integration test when used as the whole-command total.

Command: `cargo test --locked --manifest-path .github/tools/lampos-promote/Cargo.toml`.
This helper workspace is separate from the lampOS workspace nextest census;
none of these 25 observations is added to the published nextest totals.

| PR | Authored head | Original ARM64 run | Helper total |
| --- | --- | --- | --- |
| #3515 | `ee9b6253d2` | [40251274254](https://github.com/autonomous-ai/lampOS/actions/runs/40251274254) | 25/0 |
| #3510 | `9941a11c49` | [40251804763](https://github.com/autonomous-ai/lampOS/actions/runs/40251804763) | 25/0 |
| #3515 | `dde97152c1` | [40253287298](https://github.com/autonomous-ai/lampOS/actions/runs/40253287298) | 25/0 |
| #3510 | `3b78d3b836` | [40253331342](https://github.com/autonomous-ai/lampOS/actions/runs/40253331342) | 25/0 |
| #3524 | `ecaed2484b` | [40253857057](https://github.com/autonomous-ai/lampOS/actions/runs/40253857057) | 25/0 |

The earlier #3515 `ee9b6253d2` and #3510 `9941a11c49` packets published at
[`lamp-second` commit 24d8c895c](https://github.com/autonomous-ai/lampOS/commit/24d8c895c)
remain unchanged. Their field described 22 passes as the separate tool workspace;
this addendum corrects that scope to 25 overall. Their nextest, doctest, custom
harness, skip, failure, leak and timeout results are unaffected. The newer #3515
and #3510 unpublished packets are being corrected by their audit owners; #3524
already reported 25 overall.

This is a read-only reconciliation of retained original logs, not a test or
workflow rerun. [results.json](results.json) retains each exact command section,
five literal summaries, original raw-log hashes and the two earlier published
parser-field hashes. [selected-results.txt](selected-results.txt) keeps the
summaries together. Ordered target headers and summaries are recorded separately
because stdout and stderr can interleave; adjacent output lines alone are not
used to infer target ownership. The immutable earlier packets and their parser
outputs remain evidence of the reporting discrepancy.

Full raw logs and preserved prior parser copies remain private, with access
arranged separately. No private path, personal identifier or raw environment is
included in this addendum. No code, source identity, CI conclusion or release
qualification changed through this reconciliation.
