# Current authored native result and landing distinction

Original a039/3d runs ARM64 40133487641 and Linux 40133487864 succeed.
All 7,407 selected ARM cases are terminal: 7,406 PASS plus one passing LEAK,
53 skipped, zero failed/unrun. All 111 required module/idle-pool/soft-start
cases bind literal PASS. Actual preview 88fa568ef and authored whole tree
0a0d8f284e955370d286a603ab4b450fb23eb8d1 equal. Actual pinned Linux fmt
and strict every-target Clippy steps pass. No rerun or local ARM build.

Train 104 landed fc31e0123 while source-readiness HOLD was still open and
#3412 was draft. Second did not restore readiness or merge the train.
Reviewer-two reported the observed bypass in 6389780744; First owns the
remediation/designated Lead disposition. The subsequent authored green is
not retrospective approval. Both owned blobs match landed fc31 exactly,
but complete authored/landed trees differ because the train carries other
work; this current authored run is not renamed to the landed composition.
Actual train native/landing proof remains separate. Old failed Mac epoch
and three-alone disposition stay immutable. No automatic revert or device action.
