# Composed package ordinary ARM64 attempt 1

Run **40073299536**, head `4f44e25b4`, tested preview `fbfd214bb`,
base `411965dcb`: **6,003/7,352 executed, 6,002 passed (one leaky), one
failed, 49 skipped, 1,349 unrun**. The failed Voice test is
`with_name_or_follow_up_two_peoples_held_lines_make_a_quiet_room_busy`,
asserting Quiet but receiving Unknown. The dedicated eight-case package
run and strict Clippy passed separately; this ordinary gate is failed.

The failed worker and RoomTold sources exactly match the tested base.
Source analysis identifies unsigned `now_us - 400_000_000` and
`now_us - 90_000_000` in the fixture: release arithmetic can wrap on a
monotonic clock below 400 seconds. A temporary Mac test through the actual
unchanged RoomTold classifier confirms Quiet for a 500-second clock and
Unknown for a 100-second clock with explicit wrapping arithmetic. The
probe source, output and runtime hash are retained; its file was removed
after execution, with no runtime/assertion patch to the PR.

The synthetic probe confirms this clock hazard, not the original CI clock
at failure, which was not captured. Identical source does not establish
an observed original-test failure on the base. The full failed log and
metadata remain private; the exact failure excerpt has its line inventory
and full-source hash. One official failed-job rerun on unchanged source
is requested, with attempt 1 retained rather than relabelled green.
Fixture repair is routed to Tests; the package stays draft behind #3327.
