# Fourth builder's log

Fourth writes `lamp-fourth` and works on `fourth/<task>`. The task list is
`docs/work-split.md` on `origin/lamp-voice`; Fourth never edits it. All
results here are from the development machine unless explicitly attributed
to the first builder's lamp measurements. There is no lamp access here.

## 2026-10-14: task 13, memory-days — taken

- Taken from `origin/lamp-voice` at
  `aceb39518ea3e11c5aa8acb01130dd89eae26a89`.
- Task branch: `fourth/memory-days`.
- Scope: `crates/memory` and its English documentation. Leave diagnostics
  to task 8. No changes to the voice, other builders' crates, or `deploy/`.
- Build ranked retrieval, dated and linked observations, transitive
  forgetting, expiry, a bounded session note, and a post-conversation pass
  behind a model interface with scripted tests. Keep changed behavior off
  unless explicitly enabled. Run the public LongMemEval-S retrieval
  comparison and local scripted multi-day cases without a model or key.
- Evidence planned: pre-change workspace build and memory gate, regression
  and failure tests, baseline versus ranked retrieval on one harness,
  first and repeated request timings with exact machine and boundaries.
  Voice callers already allow 300 ms for their initial note and 1.5 s for
  a tool request; target reads below 50 ms p95 with 6,000 exchanges on this
  development machine, leaving device performance explicitly unverified.
- The hand-back will contain exact lamp commands, expected results, and
  likely device-specific failure points. Questions and blocked dependencies
  will be recorded here while independent work continues.

## Not shown

Nothing from this task has been built, tested, or run on a lamp yet.
