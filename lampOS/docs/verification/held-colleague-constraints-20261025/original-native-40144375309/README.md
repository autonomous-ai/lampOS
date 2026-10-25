# Original corrected-child manual native gates

{
  "runs": {
    "arm": 40144375309,
    "linux": 40144377428
  },
  "event": "workflow_dispatch full=false; original manual gates, no rerun",
  "authored": "106aecd9579a3c0a31c449ddc44e1efbf6218198",
  "tested": "106aecd9579a3c0a31c449ddc44e1efbf6218198",
  "parent": "b05b1cd77d34dfe69caf0e01d4533ca09eab55b9",
  "authored_tree": "daaf2ced46055b9e91e7c7d908e34e867b2b4395",
  "tested_tree": "daaf2ced46055b9e91e7c7d908e34e867b2b4395",
  "complete_trees_equal": true,
  "selected_terminal": 11276,
  "status_counts": {
    "PASS": 11275,
    "LEAK": 1
  },
  "skipped": 106,
  "failed_unrun": 0,
  "process_12_0": true,
  "required_literal_PASS": [
    "a_recent_call_can_resume_after_forget_without_a_stale_closed_scope",
    "every_held_print_constrains_the_voice_without_eviction",
    "a_missing_held_print_needs_its_own_cached_reply",
    "duplicates_fill_missing_evidence_without_replacing_known_evidence",
    "capacity_exhaustion_stays_face_only_until_a_fresh_call",
    "closed_scopes_and_old_callbacks_cannot_restore_constraints",
    "a_fresh_call_clears_the_previous_callers_held_voice_constraints"
  ],
  "raw_hashes": {
    "arm.txt": "64ca4fdc7804175087749e904ce8c84cbd85f9be8796853c2c6a61f6b1154942",
    "linux.txt": "05b51c4d1ecdc76793a3d03ac487d493436ea3502817c80d3a194155a2b3b279"
  },
  "scope": "Original106 direct-authored dispatch epoch; local162 carry13e is unqualified and not credited this execution. Old negative/insufficient fixtures/full/native/reader failures remain separately scoped. Parent-first, current-base and physical/acceptance gates remain."
}

Both original manual workflows check out authored106 directly. All 11,276 selected cases are terminal: 11,275 literal PASS plus one LEAK, 106 skipped, zero failed/unrun; process checks 12/0. Corrected recent-forget and cached-print/reset/fence/capacity controls literally PASS. Linux actual pinned formatting and strict all-workspace/all-target Clippy pass. Original old failed/restricted/reader/insufficient diagnostic epochs remain distinct. New local13e on162 is UNRUN and not credited this execution; parent-first/current-base/readiness and physical acceptance remain separate. No local ARM64, device, calibration or acoustic claim.
