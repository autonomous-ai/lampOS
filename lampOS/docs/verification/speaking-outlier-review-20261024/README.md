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
`audit-controls.log` and its result receipt retain **4 passed / 2 failed**.
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
