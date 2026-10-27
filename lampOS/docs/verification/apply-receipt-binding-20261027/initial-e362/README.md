# Apply receipt identity binding

Exact base e362659ce65f15c0e1e708a86e786bcea8bc87a6, branch second/apply-receipt-binding. This is a four-file uncommitted worktree result, not a commit or full-tree qualification. See proof.json for the patch and source hashes.

The parent now supplies its actual expected operation and sequence to merge_for. A mismatched Done is rejected before any insertion/removal in State.settings. The legacy merge wrapper uses the staged Request; the waiting parent does not trust that mutable request. Later version numbers and additive fields remain tolerated by the permanent apply format; unknown enum operations are refused. This does not implement the frozen guard, helper authentication, nonce or receipt record/postimage validation.

Actual old-production negative controls were executed first, once. Both old production prefixes were byte-identical to the base:

- Library: 0 passed, 2 failed, 0 ignored, 149 filtered; exit 101. Both wrong operation and wrong sequence inserted/deleted setting records.
- Round: 0 passed, 1 failed, 0 ignored, 196 filtered; exit 101. A rewritten request and agreeing forged Done incorrectly produced Switched(2). The first wrong-operation iteration fails, so the later wrong-sequence iteration is not reached in that old run; the separate old library sequence control also fails.

Actual fixed gate on the Mac:

- cargo +1.96.0 fmt --all --check: exit 0.
- cargo +1.96.0 clippy --locked -p lamp-update --all-targets -- -D warnings: exit 0.
- Fresh compiler JSON binds the actual library and round executables, preserved with matching hashes before execution.
- Full Update library: 152 passed, 0 failed, 2 ignored, 0 filtered; exit 0.
- 65 exact settings/image/reset-plan/head-touch/product-carrier round cases: 65 passed, 0 failed, 0 ignored, 132 filtered; exit 0. Both rewritten-request variants complete on the fixed source.

All five new library functions and the new parent-round function pass exactly once. The six new controls are subsets of the reported totals, never additional passes. Eight changed/manifest/toolchain inputs match before/after; all tracked source differences are the four authorized paths and there are no untracked worktree files. Original and fixed actual executables, raw compiler receipts and logs remain private with hash bindings. No local ARM/Linux, device, CI, full workspace or full round run is claimed.

One initial file-placement command used the wrong cwd and failed before any production copy. It was corrected with errexit; that setup error and the original expected-failure evidence remain retained. No old-source test was retried to green.
