# Skills full-journal startup: native qualification

Recorded 2026-10-22 00:49 +07. [PR #2692](https://github.com/autonomous-ai/lampOS/pull/2692)
head `624c98887c3f613a95c5315cf60007226e1df698` passed [ARM64 39644702150](https://github.com/autonomous-ai/lampOS/actions/runs/39644702150)
and [x86 Clippy 39644702241](https://github.com/autonomous-ai/lampOS/actions/runs/39644702241).
The tested checkout is merge `1ff2207071d5a30c503dd3aef8a7790079c85ba4`, combining that head
with `lamp-voice` `4efcf4eb0cda0ef730d30658572f49da7592a0b8`.

```text
Summary [ 165.726s] 9762 tests run: 9762 passed (1 leaky), 63 skipped
```

Doc-test sections report **4 passed / 0 failed**; `relay_signals: ok`;
the separate own-harness stage reports **12 passed / 0 failed**. The
existing Voice journal test is the one leaky pass. All three Skills
journal regressions pass: broken after startup **0.016 s**, full and unread
before startup **0.016 s**, normal CLI error **0.015 s**. These durations
are CI test durations, not device latency measurements.

The Mac formatting and strict all-target Skills Clippy checks passed;
Skills tests report **389 passed / 0 failed / 0 ignored / 0 filtered**.
[The local evidence and controls](https://github.com/autonomous-ai/lampOS/tree/624c98887c3f613a95c5315cf60007226e1df698/lampOS/docs/verification/skills-journal-backpressure-20261022)
retain the initial stalled fixture attempt, the corrected guard-removal
failure, the restored three-test pass and the complete Mac gate. The
older **387 passed / 1 failed** gate remains historical evidence and is
not replaced by this run.

[ci.json](ci.json) records the exact identities, raw-log hash, summaries,
line numbers and artifact metadata. Doc counts include only the summary
following each `Doc-tests` heading. GitHub artifact `12367631659` is
`112635241` bytes; its digest is GitHub-reported, with no independent
payload download or hash here.

No full local workspace or local ARM64 gate was run. No lamp or sound was
used. First owns the default-change integration on a lamp. Later changes
to `lamp-voice` are outside this recorded native qualification.
