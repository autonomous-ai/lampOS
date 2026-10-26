# Matched speaking-Ready setup-delay control

One original and one candidate observation, on cced42943 with a controlled
1,100 ms setup delay. The original exits 101 at the expected Resting assertion:
Ready was admitted but Voice had expired and no Voice frame was consumed.
The candidate exits zero with one fresh Speaking frame consumed after Ready.
Both take one actual step, with zero refused/input/output errors. All original
assertions, the 500 ms lifespan and 50 ms grace are unchanged.

The actual 27-case inventory, profile, features and dependency bindings match.
Both workers joined and their owned roots were absent after cleanup. Raw logs
and binaries are preserved privately by the hashes in `audit.json`. Submitted
source bytes and modes were restored; restoration changes mtimes, as recorded.
All 116 earlier executable witnesses remained equal. No observation was retried.

This is one controlled pair, not a historical-cause proof, performance result,
canonical suite qualification or device result. A separate gate on restored
candidate source remains required before publication.
