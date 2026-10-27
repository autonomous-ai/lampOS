# Cloud query original native qualification

PR #3644 authored head `dcd4b9b2dfc917714d0718e48f71b2af96499eef`, base `e8e7ac36a74062f567b4afb3d64beef6002d1f5c`. Original attempt-1 ARM64 and Clippy jobs checked out preview `abb656c2d6e4a9502a607d969e52631666d6d4f0` with ordered parents [base, head] and tree `dcea7d5f85281e903ca4c98580df91ccd0f35796` identical to the authored tree. No workflow rerun.

ARM64 run 40443284601: **success**. Unique nextest census: **11459 PASS, 0 FAIL, 0 LEAK, 0 TIMEOUT**, 106 skipped, 0 selected unrun. 35 identical report echoes excluded. Eight Cloud query controls have 8 distinct terminal rows; missing/nonpassing controls are explicit. Actual scope witnesses and PICKED filter are retained, without assuming a workspace-wide selection.

Separate from nextest: 13 process pass witnesses, 1 relay success witnesses, 4 doctest passes / 0 failures across 46 summaries, and 25 promotion-helper passes / 0 failures. Any Linux custom driver rows are a subset of nextest, never extra passes. Skipped promotion is not release/signing evidence. Linux Clippy run 40443284626: **success**. Reader workflow evidence is collected separately, not added to these totals.

Source and raw log hashes are retained. Historical host environment failure and corrected host pass remain separate; isolated old public-wrapper expected failure is not added to native totals. No device/provider acceptance or hard kernel-I/O bound is claimed.

Separate original reader workflows: Surround run 40443284600 passed 2/0/0 (3 filtered); Cloud provenance run 40443284590 passed 2/0/0. Uploaded native tests.txt, source revisions and execution receipts independently bind both to the same dcd/e8e7 preview. These four reader cases are not added to nextest; source/validation exits and raw artifact hashes are in reader-workflows.json.

Actual scope: whole workspace due Cargo.lock, empty PICKED. 11459 selected tests in 183 binaries have unique PASS terminal rows, zero LEAK, and zero selected unrun; 106 tests and two binaries skipped. This new dcd/e8e7 execution does not erase or waive the earlier553 Voice LEAK. No earlier counts transfer.
