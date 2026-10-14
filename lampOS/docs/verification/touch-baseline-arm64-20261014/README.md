# Native ARM64 integration baseline, 14 October 2026

This is GitHub's native ARM64 Debian 12 runner, Rust 1.96.0, not a lamp.
[Run 38552800341](https://github.com/autonomous-ai/lampOS/actions/runs/38552800341)
tested `a2b5e127600fd202d823becaf82c96b6ab9e02b2`, before any Task 33 code.
The release workspace binaries/examples built. The serial workspace test
command failed in unchanged Surround's `selection.rs:140`: the test removes
directory write permission, then expects collection to fail. It succeeded.
The workflow runs inside a default-root Debian container. Root's ability to
bypass directory write permission is the source-based explanation; this log
does not include a measured UID or capabilities dump.

`failed-step.txt` is the unedited output of `gh run view 38552800341 --log-failed`.
`summary.json` counts completed harness summaries only. Execution stopped
before the remaining workspace tests and before artifact collection. No
expectation was relaxed, test skipped or source changed in this evidence.
First owns the workflow; the request is to run tests as an unprivileged user,
or provide an equally strict interruption fixture independent of root.

Not shown: a green complete native gate, all later test harnesses, real lamp
execution, or a fix. This failure must not be presented as Task 33 validation.
