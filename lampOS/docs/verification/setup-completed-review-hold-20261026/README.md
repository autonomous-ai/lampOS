# Setup review follow-up: fixture P2 HOLD

HOLD `ef08682c4` and its source-identical executed predecessor `9531e2289`. Reviewer-two found that the new mkfifo process bypasses the existing `forking()` guard in the round suite; Second independently confirmed the mismatch with all other guarded spawns. The guard excludes inheritance of a sibling test's lock descriptor during fork/exec on Linux. The correction must scope the guard only around mkfifo status(), dropping it before lamp.round().

This supersedes Second's earlier no-P1/P2 / READ-OK source disposition on this point. The frozen serial **18/0/0** and its source/artifact guards remain actual observations, but do not cover concurrent spawn isolation and do not clear this defect. The separately reported Mac full-round lock failure still has an unproven cause; it must not be attributed to this guard omission without its original errno/path evidence.

Third reports a local correction and fresh qualification underway. No corrected-head result or acceptance is claimed here. Source finding: https://github.com/autonomous-ai/lampOS/pull/3570#issuecomment-6413592716. Second's corrected disposition: https://github.com/autonomous-ai/lampOS/pull/3570#issuecomment-6413693965. Earlier source and runtime receipts remain unchanged.
