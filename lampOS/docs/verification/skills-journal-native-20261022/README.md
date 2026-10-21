# Skills journal failure: native qualification

Recorded 2026-10-22 00:17 +07. [PR #2692](https://github.com/autonomous-ai/lampOS/pull/2692)
head `54e0b5e1e0d2097b7db2b8fcbc27d6520e871089` passed [ARM64 run 39639679042](https://github.com/autonomous-ai/lampOS/actions/runs/39639679042)
and [x86 Clippy 39639679055](https://github.com/autonomous-ai/lampOS/actions/runs/39639679055).
The actual checkout is merge `6708ab0f2c0610983a78c1816f146e8468b07a9d`, combining that head
with `lamp-voice` `56cfc3983571645a500f2c0d8e0271292e45cc5f`.

The nextest output is:

```text
Summary [ 151.135s] 9750 tests run: 9750 passed (1 leaky), 63 skipped
```

Doc-test sections report **4 passed / 0 failed**. `relay_signals: ok`;
the separate own-harness output reports **12 passed / 0 failed**. Release
build, binary classification, binary checksums and artifact upload passed.
The sole leaky pass is the existing Voice journal test
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.

The Mac Skills command remains **387 passed / 1 failed / 0 ignored**. Its unchanged unplug fixture returned Darwin `WouldBlock`; the later focused comparisons do not erase that result. Both new journal regressions and the unchanged unplug scenario pass in this native run. The fixture correction is separate PR #2694.

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
