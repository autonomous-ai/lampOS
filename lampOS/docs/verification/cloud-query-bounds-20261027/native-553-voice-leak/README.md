# Cloud query original native qualification

PR #3644 authored head `553e0d653eb9557a7b804e8fb6c469c102159808`, base `5982e4b7fa465177f55697ddb362d78b964809b7`. Original attempt-1 ARM64 and Clippy jobs checked out preview `ac5bd8f6fe70eb181152a8c65639b23420f754b1` with ordered parents [base, head] and tree `7d25d6fcae8daf56384108fdb610c85c6640667a` identical to the authored tree. No workflow rerun.

ARM64 run 40439732017: **success**. Unique nextest census: **11458 PASS, 0 FAIL, 1 LEAK, 0 TIMEOUT**, 106 skipped, 0 selected unrun. 35 identical report echoes excluded. Eight Cloud query controls have 8 distinct terminal rows; missing/nonpassing controls are explicit. Actual scope witnesses and PICKED filter are retained, without assuming a workspace-wide selection.

Separate from nextest: 13 process pass witnesses, 1 relay success witnesses, 4 doctest passes / 0 failures across 46 summaries, and 25 promotion-helper passes / 0 failures. Any Linux custom driver rows are a subset of nextest, never extra passes. Skipped promotion is not release/signing evidence. Linux Clippy run 40439731939: **success**. Reader workflow evidence is collected separately, not added to these totals.

Source and raw log hashes are retained. Historical host environment failure and corrected host pass remain separate; isolated old public-wrapper expected failure is not added to native totals. No device/provider acceptance or hard kernel-I/O bound is claimed.

Separate original reader workflows: Surround run 40439731963 passed 2/0/0 (3 filtered); Cloud provenance run 40439731949 passed 2/0/0. Uploaded native tests.txt, source revisions and execution receipts independently bind both to the same 553/5982 preview. These four reader cases are not added to nextest; source/validation exits and raw artifact hashes are in reader-workflows.json.

**Quality limit:** GitHub reports success, but nextest reports `11459 tests run: 11459 passed (1 leaky), 106 skipped`. The unique terminal census is **11,458 PASS plus 1 LEAK**, not 11,459 clean PASS. The LEAK is `lamp-voice same_voice::tests::a_child_that_hangs_after_it_is_ready_is_killed_and_started_again`, ordinal 9202/11459. No causal attribution or waiver is made. The original strict clean-pass packet guard stopped; its failure receipt is retained. Only packet accounting was reconciled to retain the literal leak; no test was rerun. All eight Cloud query controls passed once. Actual scope is whole workspace due Cargo.lock, empty PICKED, 183 selected binaries and two excluded. Evidence is immutable553/5982 only; prior EEE evidence is separate.
