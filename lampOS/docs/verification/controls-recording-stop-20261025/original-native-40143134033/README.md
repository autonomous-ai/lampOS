# Current Controls original native gate

{
  "runs": {
    "arm": 40143134033,
    "linux": 40143134071
  },
  "authored": "ea4580c3679e0f66e15260b5f8a50db4dab6e3d4",
  "tested": "2f5b76a884772cfd7ab27f27ff7507fe90287fa4",
  "parents": [
    "efbf42a02974ff130cb979abc0499ba699c1b34d",
    "ea4580c3679e0f66e15260b5f8a50db4dab6e3d4"
  ],
  "authored_tree": "46c19cff4f537200c76e293dce3c8fd161c5efed",
  "tested_tree": "46c19cff4f537200c76e293dce3c8fd161c5efed",
  "tree_changes": [],
  "selected_terminal": 8002,
  "status_counts": {
    "PASS": 8001,
    "LEAK": 1
  },
  "skipped": 56,
  "failed_unrun": 0,
  "process_12_0": true,
  "required_literal_PASS": [
    "finish_reports_acknowledged_pending_and_disconnected_writers_truthfully",
    "sigterm_reports_bounded_recording_and_releases_the_actual_serve_owner",
    "acquisition_recording_preserves_receipt_and_edge_times_and_replays",
    "privacy_only_touch_recording_replays_the_actual_tagged_acquisitions"
  ],
  "raw_hashes": {
    "arm.txt": "f250485a800f2491a0adc8a44bb75c69aaaddb14c29e264f77fbc56493d24b2f",
    "linux.txt": "66b74dc952a88051c3590319341a75c45e18645aef55fe06d62ed1dff091d0a1"
  },
  "scope": "Current efbf original native epoch; old failed05 and distinct reader failures retained without cause or waiver. No runtime deadline, local ARM64 or device change.",
  "complete_trees_equal": true
}

All 8,002 selected tests are terminal: 8,001 literal PASS and one LEAK, 56 skipped, zero failed or unrun; all four required truthfulness, SIGTERM and complete-recording controls pass. Process checks pass 12/0. Linux actual formatting and strict Clippy pass. Complete authored/tested trees are equal on efbf. Upstream name-refusal adds one selected case versus db16. Historical failed 05 and separate reader failures are preserved without cause or waiver. No product deadline change, physical test or device claim.
