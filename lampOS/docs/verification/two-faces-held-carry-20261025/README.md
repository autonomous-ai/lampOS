# Held two-face carry verification

Original authored commit `031990a89` is retained in the history. Tested merge
`66cab67e8` on `86416a0d3` passed native Mac formatting, strict all-target
Voice/VoiceEval clippy, workspace build and full default Voice/VoiceEval tests:
**4,662 / 0 / 21 ignored**, eleven summaries, all three added controls passing.
Its committed tree exactly matches the staged tree executed by that gate.

Current merge `1429ba89d` on `bf39e4188` passed
the same full gate: **4675 / 0 / 21 ignored**,
11 harness summaries, all three own controls executed once
and passed. The gate ran on resolved tree `52dbd9ebe60aff131cd6bc0fe4b1ee65d4cf2417`,
then committed exactly that tree with both parents preserved. No source was
changed during either execution. The intermediate 0ddd preparation was saved
but never tested; it is not counted as an execution.

Every added/removed line in the five authored paths equals the original
patch. Current environment census entries and both independent documentation
paragraphs survive the merge conflicts. Off, the prior single-look rule stays;
on, the publisher's monotonic time, bounded history, freshness and privacy
fences remain unchanged. Product units do not enable this optional switch.

The three controls exercise elapsed dwell and newest-look requirements,
gaps/expiry/out-of-order/future/late looks, privacy epochs, actual bus looks
between verdicts and the off-state behavior. They are synthetic host checks,
not a physical two-person desk trial or a calibrated latency result.

Rust 1.96.0, one compiler job, serial tests, existing owned cache, incremental
off and zero development/test debug info. Exact commands, UTC times, exit
codes, literal summaries, identities and retained raw-log hashes are alongside
this file. No disk error marker appeared. No all-fresh or independent
pre-execution artifact attestation is claimed. Original full Linux CI must
qualify the offered PR composition separately.

Original PR #3355's restricted **2,502/1,217/15** run (1,176 explicit EPERM
failures and an unclassified motor assertion), later **4,000/0/21** host run,
and older Linux results retain their own sources and limits. This carry does
not erase those failures, assign their cause, or relabel their passes.
No local ARM64, lamp access, activation, acoustic or two-person acceptance
result is claimed; the original physical acceptance remains with a lamp owner.
