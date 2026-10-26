# Carrier008 complete serial qualification

PR #3466 head `09b75f05a86f527c5cf60b300f39bafda738012a` on
`008e7a9746e7ddd58e951f8ea919a7d8beebb5ef` passed the full Mac Update
package **442/0/6**, once with `--test-threads=1`, in **2,475.635 s**.
All **448** actual target/case pairs are accounted for, with no ordinary filter.
Standalone promotion then passed **25/0/0**, once serial in **0.409 s**;
its counts and zero-case documentation results remain separate.

Formatting, strict all-targets Clippy for the actual fourteen-package closure,
and workspace build passed before test compilation and freezing. Supported
invalidation was limited to Update, Setup and Body; `fresh:false` was required
only for those invalidated packages. All source bytes, modes and index, all
**215** final sealed witnesses and actual executable bindings remained unchanged.
All owned process groups closed, without a timeout, reserve breach or retry.

All **12 carrier**, **35 head-touch**, **16 Round Setup-lifecycle** and two
library Setup-lifecycle controls passed exactly once. Round passed **195/0/0**.
Its executable is the same **16,984,804-byte** SHA-256
`a29ebb7d9f576433d1550cf0f908ab50f4f5c80143f80017bc84e98055ea682e`
reported in the original failed parallel run. These three historical failures
passed in this serial run:

- `self_cuts::without_self_opt_in_parts_update_but_no_handover_prefix_is_written`
- `self_cuts::cut_after_current_moves_keeps_new_runnable_and_can_confirm_or_put_back`
- `self_cuts::interrupted_put_back_keeps_a_runnable_current_and_finishes_on_restart`

The [original sixteen-thread 439/3/6](../tests-carrier-2d-parallel-failure-20261026/)
remains **FAIL**. The [unmodified Main Round 183/0/0](../tests-carrier-unmodified-round-control-20261026/),
[isolated candidate three-case 3/0/0](../tests-carrier-candidate-three-control-20261026/)
and [ninety-launch auxiliary diagnostic](../tests-carrier-shell-launch-diagnostic-20261026/)
retain their separate source, executable and workload scopes. The serial pass
establishes no product cause, timing comparison or waiver of the earlier failure.

The qualified `09b75` head has been pushed. Main has since advanced to
`e5889ebb53e9222762d76ac463ec7ca4718174fa`. The clean equal-patch local carry
`d645d56981da576c51a7994a6ea007aca4463e08` is source-only; its gates, fresh
native qualification and merge disposition remain pending. No current-Main or new
ARM64 result is claimed here. The installed lamp GOOD/BAD/fallback proof and
restoration remain separate. [audit.json](audit.json) exports all case outcomes,
control names and sanitized receipt bindings; no binary or private log is published.
