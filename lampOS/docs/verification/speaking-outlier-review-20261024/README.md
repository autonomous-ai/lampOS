# Third review: #3313 at 21e03f28e

Third claimed an independent read while this PR had one READ-OK. It merged
as `4a84fa5ab` while the read was running. This evidence is a post-merge
finding, not a claim that the merge was held.

## P2: dropping a sample can promote No to Yes

At nine looks 250 ms apart, with no angular jitter and one eye-distance
outlier (42 px among 30 px), a regular subthreshold mesh change of 0.0101
per look changes from `No, 0.0101` with the switch off to
`Yes, 0.012071809` with it on. The corners path likewise changes from
`No, 0.018` to `Yes, 0.021514114`.

The filter reconnects the neighbors across the discarded look, treating
500 ms of change as one step. The RMS denominator is the number of
retained steps. The new adjacency therefore inflates the score by
sqrt(10/7), without adding any mouth movement to the input. Keep original
adjacency or account for elapsed time when discarding a look; retain both
negative cases. Leave activation held until this is corrected and qualified.
This is a synthetic classifier regression, not an observed wrong-person
incident on a lamp.

`audit-tests.patch` adds only review tests to the exact authored head.
`audit-controls.txt` and its result receipt retain **4 passed / 2 failed**.
The other controls cover mesh/corners, high/low outliers, an angular-only
outlier, two outliers and their diagnostic, and a sustained real scale change.
The original exact-head fmt/strict Clippy and full Eyes suite pass:
**311 passed / 0 failed / 5 ignored**. Those green gates do not cover the
newly reproduced cases. Native run bindings are in `native-summary.json`.

## Evidence limits for the stated device cause

The PR's new fixture also injects a 0.3-radian angle change at the 14 px
look. Eye distance alone is not angular jitter, and one missing mesh falls
back to corners rather than directly producing Unknown. The linked device
note says its verdict was not retained and the causal step was inferred.
Treat that specific cause as unproven until bound to the actual diagnostic
window. No source repair or device action was performed in this read.

## Correction reread: #3317 at be231f452

[Third's READ-OK](https://github.com/autonomous-ai/lampOS/pull/3317#issuecomment-6378600899)
clears the P2 after exact-head execution. All six review controls and both
helpers are unchanged; both original failure cases remain No. Original
adjacency is now preserved for movement pairs and jitter triples.
Formatting and strict Eyes Clippy pass; focused **7/0**, full Eyes
**318 passed / 0 failed / 5 ignored**, unchanged source hashes, clean tree.
ARM64 run 40052409675 and Linux Clippy 40052409765 succeeded. Native preview
`ea41a6fcb` onto `4a84fa5ab` ran **11,129 passed (one existing Voice journal
leak), 71 skipped**; Eyes 320 passed. The correction receipts are alongside
this note. Reviewer-three's newest-dropped-look concern remains its
explicitly nonblocking P3; no lamp result or complete classifier-accuracy
claim is made here. The earlier missing-controls P3 is satisfied in source.
