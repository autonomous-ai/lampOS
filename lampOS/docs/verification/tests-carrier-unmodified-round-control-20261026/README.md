# Unmodified Main Round control

One full Round integration-binary run on unmodified Main **`7d0017f08`** passed **183/0/0**, with sixteen test threads and no case filter. The actual baseline is 7d, despite the older private scratch-directory name. This is a Round-only control, not a full Update-package baseline.

All three self-cut cases that failed in candidate `f281a5575` passed in this observation: `without_self_opt_in_parts_update_but_no_handover_prefix_is_written`, `cut_after_current_moves_keeps_new_runnable_and_can_confirm_or_put_back`, and `interrupted_put_back_keeps_a_runnable_current_and_finishes_on_restart`.

The [original candidate failure](https://github.com/autonomous-ai/lampOS/tree/839863049/lampOS/docs/verification/tests-carrier-2d-parallel-failure-20261026) remains **439 passed, 3 failed, 6 ignored** across the full package. Its Round binary had **192 passed, 3 failed**: 183 cases common with the baseline, plus twelve passing carrier cases absent from Main. Among the common cases, the candidate recorded 180 passes and three failures. No candidate test was rerun by this control.

Compiler target, profile and features match, and the literal self-cut/runnable-event helper sources are identical. The compiled binaries differ; their full hashes and sizes are in `audit.json`. Neither the source equality nor this one baseline pass establishes a product cause, byte-equivalent execution, a fix, or clearance to waive the original failures. No timing comparison is claimed.

Baseline source and all 197 protected witnesses remained unchanged. The owned test group closed and the control exited zero. Candidate source bytes, modes, index, branch and clean status were restored afterward; checkout changed sixteen source-file modification times, which is explicitly recorded. This packet reports retained receipts only and grants no readiness or push clearance. It contains no local paths, private fixture directories or host process identities.
