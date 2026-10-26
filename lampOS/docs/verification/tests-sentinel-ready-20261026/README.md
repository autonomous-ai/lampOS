# Sentinel fixture qualification

PR #3543, head `0777341815` on `cced42943`, is ready with three current-head
reads. Original [ARM64 40274519868](https://github.com/autonomous-ai/lampOS/actions/runs/40274519868)
passed 7,492 unique selected cases, no failure/leak/timeout and 53 skips.
All fourteen Reader cases passed once. This is an affected selection, not
a whole-workspace census. Exact preview parents and equal tree are in the audit.
[Linux Clippy 40274519862](https://github.com/autonomous-ai/lampOS/actions/runs/40274519862)
also passed. Mac formatting, strict twelve-package Clippy, workspace build and
four supported Reader tests pass with source/executable guards intact.

The original preparation failure remains cause-unproved; privileged DAC was
not run. The nonblocking exact-refusal assertion is a separate follow-up.
Correction to an earlier discussion: unlinking the open record leaves nlink 0,
so `record` is checked before `named`; the proposed path-comparison diagnostic
was withdrawn without any code change. A follow-up must observe the actual
Linux diagnostic before fixing its text in an assertion.
