# Original #2964 ARM failure classification

Read-only inspection of [run 39884377570, attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/39884377570/attempts/1),
job 120188999822. No test or rerun was started for this audit.
The checked merge `3b3fa4910` combined `2f259bf69` with `7bfcb31b8`.
The job failed after **9,508 passes, one failure and 67 skipped tests**;
**1,318 selected tests were unrun** after cancellation. Its original log,
exit 100 and failure remain preserved, separately from attempt 2's later
success metadata. No count or causal inference is made from that later
status.

The failing case was
`second_provider::worker::tests::with_name_or_follow_up_two_peoples_held_lines_make_a_quiet_room_busy`:
`worker.rs:8342`, `Unknown` versus `Quiet`, before Heeding construction.
The complete checked Voice tree equals the PR head, its base and our frozen
`f477d6c49` control base: `c99c6568d38b96369f9e696c4e5af3f9837f2647`.
The GitHub tree responses and source hash map support that equality.

This is consistent with the existing boot-history defect. If release
unsigned subtraction wraps and the sampled monotonic clock lies between
90 and 400 seconds, subtracting 400 seconds wraps while subtracting 90 does
not. The room's saturating elapsed time then stays zero, producing exactly
`Unknown`. **The failed log records no uptime**, so the interval is a
source explanation, not a measured fact about that runner.

The corrected `da595851f` fixture uses one shared 600-second history and
retains the original offsets and assertions. Existing Linux x86 controls
show baseline subtraction overflow at 60.0905 seconds and corrected passage
at 60.8256 seconds. Those debug controls do not reproduce this exact ARM
release assertion, and no ARM after-result is claimed here.

`export-map.json` identifies unchanged evidence copies. The complete ARM log
is retained byte-for-byte; its SHA256 is
`d141ab71e6b6b4b8a1d8d6b67509961e55881a1be515435092321fcea1edca11`.
The derived commit metadata retains technical identities and omits account
attribution. Original complete API receipts remain in the external audit
directory; unrelated PR conversations are not included in this export.
