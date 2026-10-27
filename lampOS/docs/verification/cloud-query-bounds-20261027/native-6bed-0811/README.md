# Cloud query original native qualification

PR #3644 authored head `6bedf2631c472126572c7065a7c1b72c98d9819d`, base `0811b553b76823fd19fa018d7af0137007204b32`. Original attempt-1 ARM64 and Clippy jobs checked out preview `91ad1051145f00d92621d373f9cbca9377baec0d` with ordered parents [base, head] and tree `c155a7ae43870c7db375a298c590a8d59ed6d0d3` identical to the authored tree. No workflow rerun.

ARM64 run 40435363645: **success**. Unique nextest census: **11446 PASS, 0 FAIL, 0 LEAK, 0 TIMEOUT**, 106 skipped, 0 selected unrun. 35 identical report echoes excluded. Eight Cloud query controls have 8 distinct terminal rows; missing/nonpassing controls are explicit. Actual scope witnesses and PICKED filter are retained, without assuming a workspace-wide selection.

Separate from nextest: 13 process pass witnesses, 1 relay success witnesses, 4 doctest passes / 0 failures across 46 summaries, and 25 promotion-helper passes / 0 failures. Any Linux custom driver rows are a subset of nextest, never extra passes. Skipped promotion is not release/signing evidence. Linux Clippy run 40435363619: **success**. Reader workflow evidence is collected separately, not added to these totals.

Source and raw log hashes are retained. Historical host environment failure and corrected host pass remain separate; isolated old public-wrapper expected failure is not added to native totals. No device/provider acceptance or hard kernel-I/O bound is claimed.

Separate original reader workflows: Surround run 40435363632 passed 2/0/0 (3 filtered); Cloud provenance run 40435363644 passed 2/0/0. Uploaded native tests.txt, source revisions and execution receipts independently bind both to the same 6bed/0811 preview. These four reader cases are not added to nextest; source/validation exits and raw artifact hashes are in reader-workflows.json.

Actual scope: whole workspace selected because lampOS/Cargo.lock changed, with empty PICKED. All 11,446 selected tests in 183 binaries have unique PASS terminal rows; 106 tests and two binaries were skipped. This is qualified only for immutable 6bed/0811, not later carried heads.
