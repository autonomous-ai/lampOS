# Ordinary-user ARM64 CI failures, 2026-10-14

These are GitHub Ubuntu 22.04 ARM64 runner results, not lamp measurements.
Both runs passed the release workspace build and failed the serial workspace
test command in Records. The failed-step files are unchanged `gh run view
<run> --repo autonomous-ai/lampOS --log-failed` output.

| Task head | Run | Records library result |
|---|---|---|
| Judge `e09f44b834c2fd31c24f583676509c2f05644504` | [38557228565](https://github.com/autonomous-ai/lampOS/actions/runs/38557228565) | 87 passed, 2 failed |
| Records `fbc220942b5bfe321114e0487fcc2106dd29571c` | [38557569694](https://github.com/autonomous-ai/lampOS/actions/runs/38557569694) | 104 passed, 2 failed |

Both failed `json_file::tests::atomic_result_replaces_only_known_file_and_cache_detects_change`
and `one_private_reserved_temporary_is_recovered_after_an_interrupted_write`,
at `json_file.rs:255` and `:298`, respectively, with EPERM. The shared helper
opened root-owned ancestors using `O_NOATIME`, which requires ownership or
the corresponding Linux capability. This was hidden by earlier root runs.

First independently reproduced the same failure on `lamp-voice` and repaired
the helper in `21c24fc0a`: retry opening without `O_NOATIME` only on EPERM.
Fourth picked up that repair by rebasing, without changing the workflow,
skipping the tests, or modifying the helper itself. See First's
[answer on #707](https://github.com/autonomous-ai/lampOS/issues/707#issuecomment-6167687328).
The new runs must finish before either task is ready.

## Not shown

Neither failed run is a green workspace gate, a deployed build, or a lamp
result. Successful local checks do not replace the required green native
ARM64 run on each task's eventual head.
