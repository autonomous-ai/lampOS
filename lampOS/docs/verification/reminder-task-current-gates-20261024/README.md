# Corrected reminder/task reader gates

Second, 24 October 2026. PR #3312 corrected head
`cec3372b7f26a728d0b96e0e681d257d191ff7fb`, containing `59c4ac795`.
The authored corrections are private Tasks socket prerequisites and Voice's
stable first-attempt Ack request ID. No runtime code changed.

Mac formatting and strict all-target Clippy pass for Schedule, Tasks, Voice.
The current-base scoped test stages establish **4,395 passed / 0 failed /
32 ignored** without double-counting retries: Schedule+Tasks 434/0/11;
Voice library 3,680/0/15, binary 1/0, integration 270/0, manual pair 0/0/6,
example checks 10/0. All three doctest stages pass with zero tests.

This is a completed set of stages, not one uninterrupted successful command:
- The current-base four-thread command passed Schedule, Tasks and Voice unit
  stages, then failed one unchanged news-told integration assertion with
  stale_confirmation. The whole 270-case integration target passed serially;
  the remaining examples and docs also passed. That failed output stays here.
- One attempted targeted command named nonexistent integration targets and
  executed zero tests. Its error is retained separately from the corrected
  command using actual targets `it` and `release_pair`.
- On the earlier base, one unchanged consent fixture failed under default
  concurrency, then passed alone and in the full serial Voice suite. That
  suite passed 3,961 cases but its rustdoc stage could not resolve dependencies
  while another worktree used the shared target directory. The doc-only rerun
  afterward passed. The compiler failure is not presented as a test pass.

Exact-head source-only Linux formatting passes: **6,973 regular files
unchanged**, owned unprivileged offline container removed and absence checked.
No local ARM64 build/test gate ran. Workflow YAML and four shell blocks parse.

Historical ordinary ARM run 40051251716 attempt 2 passed **11,118 tests
(one leaky), 83 skipped**, then docs and 12 own-harness checks on `ba7031f58`.
Its original attempt remains failed. The unchanged uptime fixture is owned
by #3190; the successful retry does not prove the failed runner's uptime or
qualify the corrected head. New native gates and actual released-reader
execution remain separate. No lamp or physical power-cut claim.
