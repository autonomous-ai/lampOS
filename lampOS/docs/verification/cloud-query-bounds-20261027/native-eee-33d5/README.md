# Cloud query original native qualification

PR #3644 authored head `eee85fa51cff7a7d910158bba8c795b03cae8c76`, base `33d5627cadad7184f9ea2dcdc5b858f3aa3de58d`. Original attempt-1 ARM64 and Clippy jobs checked out preview `f821dff13c9e4bcb2b187a6d3ca6b1737317f15f` with ordered parents [base, head] and tree `d9fbc8c3b77f17d017cd1ac8a6c239adcb4d2ea7` identical to the authored tree. No workflow rerun.

ARM64 run 40437575133: **success**. Unique nextest census: **11446 PASS, 0 FAIL, 0 LEAK, 0 TIMEOUT**, 106 skipped, 0 selected unrun. 35 identical report echoes excluded. Eight Cloud query controls have 8 distinct terminal rows; missing/nonpassing controls are explicit. Actual scope witnesses and PICKED filter are retained, without assuming a workspace-wide selection.

Separate from nextest: 13 process pass witnesses, 1 relay success witnesses, 4 doctest passes / 0 failures across 46 summaries, and 25 promotion-helper passes / 0 failures. Any Linux custom driver rows are a subset of nextest, never extra passes. Skipped promotion is not release/signing evidence. Linux Clippy run 40437575131: **success**. Reader workflow evidence is collected separately, not added to these totals.

Source and raw log hashes are retained. Historical host environment failure and corrected host pass remain separate; isolated old public-wrapper expected failure is not added to native totals. No device/provider acceptance or hard kernel-I/O bound is claimed.

Separate original reader workflows: Surround run 40437575155 passed 2/0/0 (3 filtered); Cloud provenance run 40437575139 passed 2/0/0. Uploaded native tests.txt, source revisions and execution receipts independently bind both to the same eee/33d preview. These four reader cases are not added to nextest; source/validation exits and raw artifact hashes are in reader-workflows.json.

Actual scope: whole workspace selected because lampOS/Cargo.lock changed, with empty PICKED. All 11,446 selected tests in 183 binaries have unique PASS terminal rows; 106 tests and two binaries were skipped. Evidence qualifies immutable eee/33d only. Earlier6bed success and129 cancellation remain separate, with no test-count transfer.
