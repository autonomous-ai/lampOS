# Frozen local gate

{
  "gate_start_head": "ea4580c3679e0f66e15260b5f8a50db4dab6e3d4",
  "head": "ea4580c3679e0f66e15260b5f8a50db4dab6e3d4",
  "tree": "46c19cff4f537200c76e293dce3c8fd161c5efed",
  "base": "efbf42a02974ff130cb979abc0499ba699c1b34d",
  "source_count": 7041,
  "every_source_matches_frozen_map_and_committed_blob_mode": true,
  "summary": {
    "summaries": 6,
    "passed": 510,
    "failed": 0,
    "ignored": 3,
    "source_unchanged": true
  },
  "worktree_clean": true,
  "scope": "Controls test-only recorder completion/CLI contract; production prefix unchanged, full Controls tests plus workspace build; no runtime/API/dependency/schema/default change"
}

All four command exits and complete before/after source maps are retained. Every frozen source hash/mode was checked against the committed Git blob/mode after qualification. Targets were exclusive to their own worktrees and reused within each worktree; not an all-fresh build claim. Native runs, old failed epochs, reader failures and latest-base ancestry remain separately scoped. No hardware, acoustic or calibration claim.
