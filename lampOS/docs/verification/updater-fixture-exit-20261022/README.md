# Update fixture timeout diagnostics, 23 October 2026 (+07)

This is a diagnostic follow-up to the Update failure reported on
[#3012](https://github.com/autonomous-ai/lampOS/pull/3012#issuecomment-6346276126)
and #1569 during #3023's Mac consumer gate. That command remains failed:
1,916 passed, one failed, eleven ignored. Its unchanged-base focused pass
and this change do not establish the original cause or clear that result.

The interrupted put-back case performs 36 executable checks. Its previous
panic identified none of them. The helper now reports the test-thread check
number, caller, path, expected old/new hash, PID and last poll timing. Failure
cleanup reports kill and reap results. A final nonblocking status read is
only evidence: even an observed successful exit cannot clear the timeout.

The two-second post-spawn deadline, five-millisecond polling, exact
script/mode/output checks and recovery assertions remain. The fork guard is
taken before the measured window; stdout is read after successful exit.
There is no runtime updater change. An independent source read found no
P1/P2; its repeated-call ambiguity was addressed with the check number.

## Controlled failures

Both were run once on native ARM64 macOS with the exact interrupted
put-back test selected, serially. They are intentional source mutations,
not reproductions of the original failure. Authored source was restored
byte-identically before the final gate.

| Control | Observed timeout | Last status | Cleanup | Test result |
|---|---:|---|---|---|
| Add a builtin infinite loop to the old stub, with no descendants | 2.000229334 s | no exit observed | SIGKILL, reaped signal 9 | expected failure, exit 101 |
| Hold the parent for 2.1 s and inject a missed status observation | 2.109898042 s | exit 0 observed by the diagnostic read | reaped exit 0 | expected failure, exit 101 |

Both name check 1, the initiating test call site and the selected old updater.
The second control specifically proves that a late successful observation
does not turn a timeout into a passing test. It does not prove that parent
scheduling caused the original failure. The existing cleanup wait semantics
are unchanged; this patch does not introduce a stronger cleanup time bound.

Exact commands, source and raw-log hashes, normalized diagnostic excerpts,
and final Mac gate results are recorded in [evidence.json](evidence.json).
Only private Update test helpers and their documentation change; the crate
name search found no dependent production API to retest. Official PR ARM64
and Linux Clippy qualification is separate. No lamp, physical cut, latency,
release publication, full local workspace gate or local ARM64 gate is claimed.

## Current-base Mac gate

After carrying onto `b8a9148bd`, the unchanged diagnostics were gated with
the merged Update test targets: formatting and strict all-target Update
Clippy passed; the full Update suite passed **308 / 0 / 4 ignored**, with
no filtered tests. [mac-current.json](mac-current.json) records the commands,
36 input hashes and result lines. The earlier 307 / 0 / 2 run in
`evidence.json` remains evidence for its original `bad114496` base.
Official PR checks qualify each pushed head separately.

Architecture correction: `uname -sm` reports `Darwin arm64`, Rust 1.96.0
reports host `aarch64-apple-darwin`, and the retained Round executable is
Mach-O arm64. The earlier x86_64 label was wrong. The post-run artifact
inspection and its provenance limit are recorded in `evidence.json`.
