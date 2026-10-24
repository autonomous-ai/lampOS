# Requested-audio cache retry: Third’s second read

#3319, authored `606e84a11`. Independent Mac formatting, strict Voice
all-target Clippy and focused requested-audio **55/0** pass, source unchanged.
No remaining source P1/P2 established. Runtime retry changes are confined to
the speaker-cache acquisition and fixed refusal reasons. Original identity,
lease, stale/newer observation, fence and terminal-state checks remain.

The actual output permit remains anchored at `permit_at`, sampled before
the retry; the retry does not renew its original expiry. Cancellation stays
terminal and a fresh later check cannot revive a cancelled ticket. The
existing in-flight cancellation window is widened by spinning; a post-wait
terminal/running/ready recheck remains a nonblocking robustness suggestion,
not an independently executed new safety counterexample.

The test uses a 200 ms retry budget, while production uses 200 microseconds.
The positive holds an unchanged cache for 2 ms, so it establishes retry
semantics rather than production scheduling success. The successful-acquire
path has no elapsed-time rejection; strict “at most” wall-time language is
therefore not a measured guarantee under preemption. Actual lamp retry
latency and corrected playback remain First’s qualification, as does folding
the intermediate unsafe boot-fix commit into its correction. This source
read does not replace those outstanding integration requirements.

No device action or full local Voice rerun by Third on this head; reviewer-two
separately reported full Voice/native gates. The older boot/diagnostic read
remains bound to `8b54c1c32`.

Also retained here: exact std-only extraction of #3334’s corrected absolute
reply deadline helper and its two socket controls, independently compiled
with Rust 1.96: **2/0**. That correction has Third’s renewed READ-OK on
`63e2d963d`; prior full Actions/actual-pair receipts remain separate.
