# Carrier current-base parallel failure

The full Mac Update gate on `f281a5575` / base `2d18e296c` **failed**: **439 passed, 3 failed, 6 ignored**, all **448** actual target/case pairs accounted for, with no ordinary test filtered out. It ran once with `--test-threads=16`; no runtime was reused. Formatting, strict Clippy for the actual fourteen-package closure, and the workspace build passed first. Standalone promotion runtime did not run after the failure.

All **12 carrier**, **35 head-touch**, and **16 round Setup-lifecycle** controls passed. Two additional Setup-lifecycle library tests also passed. Per-target counts, control names, ignored cases and receipt hashes are in `audit.json`; the zero-case doctest result is kept separate.

The three failures were:

- `self_cuts::without_self_opt_in_parts_update_but_no_handover_prefix_is_written`: fixture check 3, expected old fallback, at the 5 s startup watchdog.
- `self_cuts::cut_after_current_moves_keeps_new_runnable_and_can_confirm_or_put_back`: fixture check 4, expected new current updater, at the same watchdog.
- `self_cuts::interrupted_put_back_keeps_a_runnable_current_and_finishes_on_restart`: fixture check 4, expected new current updater, at the same watchdog.

Each diagnostic had `first_byte=None`, no captured output, `eof=false`, and child status still `None`; elapsed times were approximately 5.001, 5.002 and 5.000 s. The command ultimately exited 101 without a stage timeout or disk-reserve breach. These observations identify the failed fixture boundary; they do not establish a product cause. The self-cut and runnable-event source files equal the pinned base, which also does not establish causality or waive the failures.

Source, all **197** protected witnesses and current executable guards remained unchanged. The owned command was reaped and its process group absent at completion. The earlier ba950 serial 424-pass result has a different source/scope and does not clear this current parallel failure; no timing comparison is claimed.

A matched-base 2d18 full Round run with sixteen test threads was being prepared when this packet was requested, and no result from it is claimed here. The later 7d hardware-only source carry still needs current-base qualification. This packet grants no retry waiver, push clearance or ready status. It derives retained receipts only; no new test, device action or raw private log publication was performed.

## Original Round artifact receipt addendum

The retained original executable manifest, independent frozen-copy manifest and Cargo compiler record report the same Round executable: **16,984,804 bytes**, SHA-256 `a29ebb7d9f576433d1550cf0f908ab50f4f5c80143f80017bc84e98055ea682e`. Its normalized runtime path is `debug/deps/round-86f3bcf98e2c5ca2`; the independent copy is `update-frozen/0-round-86f3bcf98e2c5ca2`. Compiler profile, features, modes and receipt hashes are exported in `audit.json`. This is an exported receipt binding, not an independent binary re-execution.

The separately [published serial three-case observation](https://github.com/autonomous-ai/lampOS/tree/f7f01e798/lampOS/docs/verification/tests-carrier-candidate-three-control-20261026) reports that same frozen hash and 3/0/0. The original full **439/3/6 remains FAIL**, with no established cause or waiver. The [original public packet at 839863049](https://github.com/autonomous-ai/lampOS/tree/839863049/lampOS/docs/verification/tests-carrier-2d-parallel-failure-20261026) remains an immutable history record; this addendum does not replace its outcome.
