# Peek session read status: host verification, 22 October 2026

Part of [#2789](https://github.com/autonomous-ai/lampOS/issues/2789), piece 1. Authored and tested on base `fab69b0c4c4333ff8d51459e063169af7355e1dc`. This changes the read status and its display; it grants no filesystem permission. Reader-group, unit and authorized lamp checks remain separate issue pieces.

## Local gate

All commands ran from `lampOS/`, with four Cargo build jobs, incremental compilation off, and the shared local target. Peek has no Cargo reverse dependencies at this base. Cloud, Voice-eval and Update are CI’s unconditional file readers and remain covered by the exact-head native gate; their broad suites were not repeated locally.

| Command | Result | Wall time, including compilation |
| --- | --- | --- |
| `cargo fmt --all --check` | passed | 11.267 s |
| `cargo clippy -q --workspace --all-targets --message-format=short -- -D warnings` | passed | 2.331 s |
| `cargo test -p lamp-peek -- --nocapture` | 26 passed, 0 failed, 0 ignored | 9.402 s |
| `cargo build -q --workspace` | passed | 82.795 s |

The 26 tests are 15 library tests and 11 HTTP integration tests; binary and doc-test targets contained zero tests. The five added tests cover real denied root/metadata/file reads, unfinished-line recovery, a denied new session retaining stale old rows, missing/other errors versus readable emptiness, cancellation with one in-flight cache read, and one authenticated stream through denial and recovery. Permission fixtures assert the OS refuses access and must run as an ordinary user.

The cancellation test observes the acquired guard before aborting its requester, then explicitly polls the second request and requires Pending. A held filesystem-side lock has a 3 s test escape; the single runtime thread must remain responsive within 1 s. No scheduling yield is treated as proof that a task ran.

An initial lint attempt rejected a non-octal zero literal in a new chmod fixture; both new fixtures now use `0o0`. The final gates above all passed. Earlier passing runs are not added to these counts.

## Stream timing and privacy

One live authenticated HTTP stream, with synthetic records and a fast synthetic unit-status reader. The first boundary starts at requesting the stream; the remaining boundaries start at changing file permissions and end when both Turns and Overview carry the expected state. No reload or reconnection.

| Boundary | Host time |
| --- | --- |
| First denied read → permission notice | 1.103 ms |
| Permission restored → current rows | 1007.352 ms |
| Permission denied again → stale rows | 1007.041 ms |
| Permission restored again → new rows, warning cleared | 1008.003 ms |

All met the host target of less than 2 s. The stream kept the older row visibly stale, excluded a planted word and synthetic private path, hid stale first-audio summaries, and displayed unread bytes exactly once after recovery. An unauthenticated request exposed neither events nor the read-status notice.

This is not device timing or an arbitrary-filesystem deadline. An existing slow unit-status refresh can still hold the entire push for up to its 10 s deadline. No device access, install, account/group change, ACL change or private session read was performed. Native ARM64 and workspace Clippy CI on the published/rebased head are still required for readiness.

## Source identity and retained logs

SHA-256 of the validated files (paths relative to the repository root):

| File | SHA-256 |
| --- | --- |
| `lampOS/crates/peek/src/turns.rs` | `e82832bdf974ec0e5fe780b6b24ea1fe6ea9e3d2bd50216a6d7ef3cc1492c4bc` |
| `lampOS/crates/peek/src/serve.rs` | `a7baa82c4d42a9d67a2b4eaf497c6e393bae48cde76f18b4e39c2a424a19bc9e` |
| `lampOS/crates/peek/src/page.rs` | `5baf8583fd04071c4e5fc9730f101131c620bab6a5e1d215a40946d75bd37a64` |
| `lampOS/crates/peek/tests/peek.rs` | `1b889fcb390b64b0fa119d6666dfabfd773e8cc4239e82b14ca41c4e15364c19` |
| `lampOS/docs/peek.md` | `9c06d6c5025e47d96747b23a51d4f287747b7e0bb56a2d7fc61a6e746fc2fc77` |

Local evidence, not committed: `/private/tmp/fourth-peek-read-status-checks/validation.json`, `final.json`, `final-build.json`, `final-{fmt,clippy,peek}.log`, `final-build-build.log`, and `source.json`. The validation receipt records commands, exit codes, UTC times, counts, source hashes and log hashes; `run_checks.py` retains the command runner.

The source rebased cleanly onto `2d1d353965b6b9f9bdc043ee4d19832630b3a6d8` before publication. All five hashes above remain identical. The source range-diff was `2c805df8d = 6cde4f593`; only this evidence note was then folded into the commit. The host commands above remain attributed to `fab69b0c4`; the rebased head is gated by its native ARM64 and Clippy runs.
