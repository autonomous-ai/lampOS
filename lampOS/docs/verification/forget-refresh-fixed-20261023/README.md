# Refused forget survives the next session refresh

Second independently checked PR #3130 at `82bc0f4ec46f1a8b088805b0aa281a8605bce539`
on a Mac (Darwin arm64, Rust 1.96.0), 23 October 2026. These are exact-head
focused tests, not a current integration carry or a full gate. All 6,530
bound source files were unchanged throughout; `current-manifest.json` keeps
their SHA-256 values. Each result JSON names the complete command, elapsed
time, exit code and raw-output hash. Its `.txt` file is that unedited output.

| Run | Command output |
| --- | --- |
| voice-refusal | 2 passed; 0 failed; 0 ignored; 0 measured; 3569 filtered out; finished in 0.02s |
| session-change | 1 passed; 0 failed; 0 ignored; 0 measured; 3570 filtered out; finished in 0.01s |
| refresh-counter | 1 passed; 0 failed; 0 ignored; 0 measured; 158 filtered out; finished in 0.55s |
| shared-evidence | 1 passed; 0 failed; 0 ignored; 0 measured; 158 filtered out; finished in 0.43s |
| bare-time-window | 4 passed; 0 failed; 0 ignored; 0 measured; 3567 filtered out; finished in 0.01s |
| conversation-erase | 3 passed; 0 failed; 0 ignored; 0 measured; 156 filtered out; finished in 2.36s |

Total: **12 passed, zero failed**. The refresh regression uses the real
Voice client and SQLite Memory store. A refused Asked-ID deletion survives
the next automatic same-person session refresh; the retry deletes the same
ID and preserves the unrelated keep. It failed on the preceding implementation:
[original negative control](../forget-refresh-retry-20261023/README.md).

The session cases also check a different person, no known person and expiry.
Source review confirms the original timestamp and line count survive a valid
refresh, privacy close still clears the slot, and confirmed removal clears it.
The older-server capability check remains before the ID deletion. The prior
[exact legacy Memory run](../forget-refresh-retry-20261023/README.md#older-memory-path-checked-separately)
is separate evidence on `a1b8034bf`, not a fresh execution on this head.

No actual committed deletion followed by transport EOF was injected here.
The existing lost-reply fixture sends an incomplete acknowledgement; the
real lost-acknowledgement path was read in source. Repeating the retained ID
after a committed but unacknowledged deletion removes zero rows and cannot
fall through to deleting the latest unrelated keep.

One P3 remains: a confirmed named forget returns through `forget_about` and
bypasses the shared Asked-slot cleanup, while the documentation describes
all confirmed own-person forgets as clearing it. Route named forget through
the same person-aware cleanup in a follow-up. No new deletion counter is
claimed for this P3. No remaining P1 or P2 was found in this read.

No lamp, microphone, camera or real person's stored data was used. Native
ARM64, Linux Clippy and an updated integration carry belong to the PR gate.
