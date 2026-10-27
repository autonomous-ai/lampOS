# Cloud query original native qualification

PR #3644 authored head `238c12bf981c157029e5962a59a345b761626126`, base `f7252d81dc2e72494afc6ea842bd8b0445f1991c`. Original attempt-1 ARM64 and Clippy jobs checked out preview `924951fdb054dd2da040dac08a58bf94af24149e` with ordered parents [base, head] and tree `f88b521e7ca4c9ec9b9eaaadfad3ba4b918fc38c` identical to the authored tree. No workflow rerun.

ARM64 run 40469605818: **success**. Unique nextest census: **11551 PASS, 0 FAIL, 0 LEAK, 0 TIMEOUT**, 106 skipped, 0 selected unrun. 35 identical report echoes excluded. Ten Cloud query controls have 10 distinct terminal rows; missing/nonpassing controls are explicit. Actual scope witnesses and PICKED filter are retained, without assuming a workspace-wide selection.

Separate from nextest: 13 process pass witnesses, 1 relay success witnesses, 4 doctest passes / 0 failures across 47 summaries, and 25 promotion-helper passes / 0 failures. Any Linux custom driver rows are a subset of nextest, never extra passes. Skipped promotion is not release/signing evidence. Linux Clippy run 40469605765: **success**. Reader workflow evidence is collected separately, not added to these totals.

Source and raw log hashes are retained. Historical host environment failure and corrected host pass remain separate; isolated old public-wrapper expected failure is not added to native totals. No device/provider acceptance or hard kernel-I/O bound is claimed.

Separate original reader workflows: Surround run 40469605937 passed 2/0/0 (3 filtered); Cloud provenance run 40469605977 passed 2/0/0. Uploaded native tests.txt, source revisions and execution receipts independently bind both to the same 238c/f725 preview. These four reader cases are not added to nextest; source/validation exits and raw artifact hashes are in reader-workflows.json.
