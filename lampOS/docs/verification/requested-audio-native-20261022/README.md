# Requested local music: native qualification

Recorded 2026-10-22 00:17 +07. [PR #1757](https://github.com/autonomous-ai/lampOS/pull/1757)
head `0a878cd4d9d44867b2971e70c118244f0c21ad38` passed [ARM64 run 39638544014](https://github.com/autonomous-ai/lampOS/actions/runs/39638544014)
and [x86 Clippy 39638543959](https://github.com/autonomous-ai/lampOS/actions/runs/39638543959).
The actual checkout is merge `ec437b94a7201ab851b001ef736e13a0f8c2c415`, combining that head
with `lamp-voice` `a7dbfcc2cd8b2b1cbfa4cdfae2309599c5224aa1`.

The nextest output is:

```text
Summary [ 166.548s] 9903 tests run: 9903 passed (1 leaky), 63 skipped
```

Doc-test sections report **4 passed / 0 failed**. `relay_signals: ok`;
the separate own-harness output reports **12 passed / 0 failed**. Release
build, binary classification, binary checksums and artifact upload passed.
The sole leaky pass is the existing Voice journal test
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.

The Mac five-crate gate remains **4,665 passed / 0 failed / 14 ignored**, and the post-rebase Skills gate **421 passed / 0 failed / 0 ignored**. All 86 historical failures have an exact candidate-native match: **85 PASS and one LEAK**, no missing or failed match. They remain separate from their unchanged-head comparisons.

[ci.json](ci.json) retains the run identities, exact summaries, line numbers,
raw-log hash, doc-test results and GitHub artifact metadata. The source is
the completed run's unedited `gh run view --log` output. Doc-test totals
include only the summary following each `Doc-tests` heading, excluding
nextest's repeated output and the separate own-harness stage.

## What was not shown

No lamp, sound, physical mute or device latency was tested here. GitHub's
artifact digest is recorded as reported; the payload was not downloaded or
independently hashed. Historical failed runs are retained, not replaced by
this success. The tested integration base is the one above, not a claim
about later `lamp-voice` commits.
