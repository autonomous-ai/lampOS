# Reminder completion before successor admission: focused evidence

On release base `bf39e4188`, a new actual-coordinator-loop regression failed
against unchanged production at the expected semantic assertion: the fully
retired reminder was abandoned at successor admission. The exact one-test
command compiled, ran one test and exited 101 (**0 passed, 1 failed**). This
was not a fixture setup or compilation failure. The two test-source hashes,
unchanged production hashes, raw-log hash, and preserved post-execution binary
hash are recorded. The failed binary is retained privately, not committed.

The qualified-receipt guard then passed native Mac formatting and **46/0**
focused tests: reminder controls **29/0**, provider-terminal controls **13/0**,
and durable reminder roundtrip controls **4/0**. Production and fixture hashes
bind this uncommitted fixed source to the same bf39 base. Full qualification
is running separately on the later d7 release base; these are not its results.

The positive regression delivers final retirement and the last onset block in
the same actual IPC pump. It observes Complete plus Spoken before successor
admission, no cut-in flag, and the full ordered prefix retained exactly once.
The stepped scenario measures **12,000 us capture-read to successor admission**
and **10,000 us reminder completion to successor admission**, with completion
in the candidate-processing pass. Only six frames are voiced; the next one is
quiet. This is a host scenario bound, not acoustic or device latency. No fresh
capture means no guaranteed progress; ordinary admission expiry remains.

Negative cases retain Abandon and Dropped for failed, missing-terminal,
unretired, clock-reset and private delivery. A late old retirement cannot
revive a cancelled receipt. Existing roundtrip tests separately cover the
real reminder ledger/recovery boundary; the new loop fixture alone does not
prove a durable Ack. The unrelated natural-reset/say mismatch is outside this
fix and remains a separate repair.

Rust 1.96.0, native x86_64 Mac, one compiler job, serial tests, existing owned
cache, incremental off and zero development/test debug info. Commands, UTC
times, exit codes and original failure remain in the JSON; raw logs and both
executed binaries are retained privately. No all-fresh, independently rehashed
private binary, pre-execution attestation, local ARM64 or lamp result is claimed.
