# Cloud query original native qualification

PR #3644 authored head `129774137f7455e8473200df64dd5a3f6959b8b0`, base `737331535d0cab813170d288110fc44ff5d41a68`. Original attempt-1 ARM64 and Clippy jobs checked out preview `7dc91aa9312699220de5c75a2486204f8061915e` with ordered parents [base, head] and tree `50e6534c5ef962a5e540f2f85c36be0a2e839c79` identical to the authored tree. No workflow rerun.

ARM64 run 40434211639: **cancelled**. Unique nextest census: **6552 PASS, 0 FAIL, 0 LEAK, 0 TIMEOUT**, 106 skipped, 4891 selected unrun. 0 identical report echoes excluded. Eight Cloud query controls have 8 distinct terminal rows; missing/nonpassing controls are explicit. Actual scope witnesses and PICKED filter are retained, without assuming a workspace-wide selection.

Separate from nextest: 0 process pass witnesses, 0 relay success witnesses, 0 doctest passes / 0 failures across 0 summaries, and 25 promotion-helper passes / 0 failures. Any Linux custom driver rows are a subset of nextest, never extra passes. Skipped promotion is not release/signing evidence. Linux Clippy run 40434211673: **success**. Reader workflow evidence is collected separately, not added to these totals.

Source and raw log hashes are retained. Historical host environment failure and corrected host pass remain separate; isolated old public-wrapper expected failure is not added to native totals. No device/provider acceptance or hard kernel-I/O bound is claimed.

Separate original reader workflows: Surround run 40434211598 passed 2/0/0 (3 filtered); Cloud provenance run 40434211614 passed 2/0/0. Uploaded native tests.txt, source revisions and execution receipts independently bind both to the same 129/737 preview. These four reader cases are not added to nextest; source/validation exits and raw artifact hashes are in reader-workflows.json.

The original ARM64 run was cancelled after superseding the PR head. It is **not a green gate**: 6,552 observed PASS rows out of 11,443 selected, with 4,891 selected tests having no terminal row. All eight Cloud controls passed once before cancellation. The log selected the whole workspace due to lampOS/Cargo.lock, with 183 binaries selected and two binaries excluded. No test-failure terminal rows were observed before cancellation; this does not qualify the remaining tests. Doc/process/relay steps did not run. The 25 helper passes occurred earlier and remain separate.
