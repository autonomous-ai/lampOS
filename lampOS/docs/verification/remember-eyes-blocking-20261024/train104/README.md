# Actual train 104 landing evidence, separate from readiness

Train 3415 authored 861480341, actual preview 989909436 and landed fc31e0123
have the same complete tree 20695fa3e7ff2dc8f39c212dc1de1650dee6ee5d.
Original ARM64 40132470053 and Linux 40132470035 complete successfully.
ARM has 11,262 PASS plus one passing LEAK, 106 skips, zero failed/unrun;
all 11,263 selected cases are terminal. All 111 required fixture/soft-start/
idle-pool cases bind literal PASS. Exact repeated status/index/target rows
are collapsed only when identical. Actual Linux pinned fmt/strict every-
target Clippy steps pass. No new native execution or local ARM build.

This is a distinct complete composition from authored a039's native run;
its 7,406 PASS plus one LEAK are not renamed to the train. Both actual
train and authored source results retain their own head/preview/census.
The current fixture's two blobs independently match the landed source.

The train merged while source-readiness HOLD 6389559587 was unlifted;
Second explicitly kept it in 6389631513 and 6389665038. Source PR 3412 was
draft and its new authored native qualification had not completed at the
merge. Reviewer-two's report 6389780744 preserves the observed bypass.
A later green does not approve the earlier merge. First owns pre-merge
revalidation and #3387's designated Lead disposition; this native audit
is not clearance of those gates. No automatic revert, physical acceptance,
activation, provider or device action by Second.
