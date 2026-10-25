# Complete a retired reminder before admitting its successor

An actual-coordinator-loop test on unchanged `bf39e4188` production reproduced
one failure: the fully retired reminder was abandoned at successor admission.
The final speaker retirement and the sixth onset frame arrived together. The
failed source, raw log and post-execution binary/hash were retained before the
fix. [Preserved negative and focused positive](https://github.com/autonomous-ai/lampOS/tree/cc00a4305/lampOS/docs/verification/reminder-successor-boundary-20261025/focused-bf39).

The fixed source first passed **46/0** focused controls on bf39 (reminders
29/0, provider-terminal 13/0, durable reminder roundtrip 4/0). The same patch
was then carried onto `d7d20bfc0`, whose only base delta was a scope test and
two documentation files. The current native Mac full gate passed formatting,
strict all-target Voice/VoiceEval clippy, workspace build and full Voice/Eval:
**4674/0/21 ignored**, eleven summaries.
All 7062 tracked source hashes stayed unchanged during this
execution. The two new controls and the existing real-ledger lost-Ack recovery
control each passed once. Exact commands, timestamps, exits, hashes and literal
summaries are retained alongside this file; the full source map is private and
its hash is recorded. No all-fresh or pre-execution binary attestation is made.

The guard reuses the sole ordinary completion path and its qualified receipt.
It adds no timer, I/O, thread or second acknowledgement path to admission.
Failed, missing-terminal, partial, unretired, reset and private delivery stays
unacknowledged. A late old retirement cannot revive a cancelled receipt.
Existing multi-generation and real-ledger recovery controls remain intact.
The adjacent natural-reset/say outcome mismatch is a separate repair.

The new actual-loop scenario observes Complete and Spoken before the successor,
no cut-in flag, and the entire ordered prefix once. Only six frames are voiced;
the next is quiet. Its stepped clock measures **12 ms capture-read to successor
admission**, **10 ms reminder completion to admission**, with completion in the
candidate-processing pass. No fresh capture means no promised progress; normal
expiry and owner/privacy checks remain. This is not an acoustic or device bound.

Rust 1.96.0, native x86_64 Mac, one compiler job, serial tests, existing owned
cache, incremental off and zero development/test debug info. Original offered-
head Linux ARM64 and Linux clippy remain separate gates. No local ARM64 link,
lamp installation, acoustic measurement or reproduced physical timing is claimed.
