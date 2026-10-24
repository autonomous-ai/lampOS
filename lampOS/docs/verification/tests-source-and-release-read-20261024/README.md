# Current-source audit and rested-reader review

Ready for lamp-voice: no.

## Integration changes

The frozen comparison remains on 4a84fa5ab. Current integration 9ed44f88c changes 24 paths across four commits, including shared Voice admission, transport and cancellation code, new enrollment fixtures, and two rows in the document compiled by Voice-eval. All 47 manifest hashes remain unchanged; the conservative twelve-package dependency gate still applies. Each recorded changed-file hash was independently checked against Git. The existing comparison is preserved; after its candidate and assigned standalone observations, the latest carry needs fresh source/case inventories and full applicable candidate gates. Static inspection does not qualify the new source.

## Rested-reader finding

The assigned #3327 final commit b76fe12ae is held for a test setup collision: its persistent bus uses `root/b`, then its opposite-reader stage creates that same directory. Both new native ARM cases fail at `fixture.rs:84` with `AlreadyExists`; the four earlier cases pass. The distinct-path fix must keep the create-directory assertion and all cross-version checks. [Finding](https://github.com/autonomous-ai/lampOS/pull/3327#issuecomment-6379083565) and [native job](https://github.com/autonomous-ai/lampOS/actions/runs/40056359883/job/120760101535).

The retained artifact has 316 regular files. All 6,974 before/after source hashes are equal and independently match the authored Git tree, which equals the recorded preview tree. Each new case has 60 transcript records ending with writer A killed/reaped and its after-A state snapshot. Neither opposite reader B nor returning A executed. The raw failure segment and its original byte offsets are retained here. Source review found no further concrete P1/P2: the intended lease-reset, stale/wrong-receipt, exact daily bitmap, one-stretch and immutable-manifest assertions remain substantive but need corrected-head execution. Real released Skills processes consume synthetic Expression/Body receipts; this is no physical or elapsed-night qualification.

## Prepared sign-in fixture cleanup

Four sign-in mock scripts now use their owned, joined server instead of a detached helper. The existing asynchronous privacy read also has an unwind join guard. Reply scripts and original assertions remain. Product source is unchanged from the approved draft. Only `git diff --check` ran after this test-only edit; formatting, compilation and tests remain pending. The unapplied legacy-continuation control was regenerated against source ae7fe529d and passed `git apply --check`; neither control nor candidate executed. The earlier prepared control remains unexecuted. OS scheduling and final joins are not hard timing guarantees.
