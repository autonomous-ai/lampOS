# Failed-units report: rebase onto the actual Music merge

**Successful unpublished intermediate.** The local Cloud candidate for
[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764) is
`d05d22ce43376078b58cd059261e1600dd25d827`, based on the actual
[Music merge](https://github.com/autonomous-ai/lampOS/commit/2d1d353965b6b9f9bdc043ee4d19832630b3a6d8)
`2d1d353965b6b9f9bdc043ee4d19832630b3a6d8`.
The pre-push guard withheld publication because `lamp-voice` advanced to
`5c8b03a2eb2f7755262397c46216be2af41b4a95` while this gate ran.
This head was not pushed. Native qualification remains pending for the
subsequent published candidate; this receipt contains no new native result.

[range-diff.txt](range-diff.txt) records an equal patch from the prior head
`93d93ecb1415d1e3b97a7a94d3852fa10cb03018` on
`fab69b0c4c4333ff8d51459e063169af7355e1dc`.
[identity.json](identity.json) preserves six identical before/after follow-up
file hashes. The complete Cloud tree remains
`1dd3e10a85ee130c788c245b789a6ae779c6434f`, matching the
[previous rebase identity](../report-failed-units-rebase-fab69-20261022/identity.json).
The new integration base was checked with a fresh Mac gate.

The commands ran from `lampOS/`, in order, with Rust 1.96.0:

| Command | Result | Command elapsed |
| --- | --- | --- |
| `cargo +1.96.0 fmt --all --check` | Exit 0 | 5.138 s |
| `cargo +1.96.0 clippy --locked --offline -p lamp-cloud --all-targets -- -D warnings` | Exit 0 | 17.194 s |
| `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-cloud -- --test-threads=1` | 177 passed / 0 failed / 1 ignored / 0 filtered out; exit 0 | 71.547 s |

[mac-gate.json](mac-gate.json) records all three terminal stages, exact commands,
raw-log hashes and four test summaries. Its `base` field identifies the tested
candidate head, not that head's parent. All **464 recorded source-file hashes**
are identical before and after the gate (`source_unchanged: true`). The retained
raw logs were checked against their recorded hashes and their test summaries
recounted before this receipt was copied.

The prior head's
[failed ARM64 run](../report-failed-units-rebase-fab69-native-failure-20261022/README.md)
remains a separate failed result: 7,885 passed and 1 failed, with 63 profile
skips and 2,132 selected tests not run after failure. Its Clippy success and
separate passing comparisons do not qualify that failed run. This successful
Mac gate does not replace it or establish a cause for its failure.

This is formatting, strict Cloud Clippy and Cloud test evidence for the exact
unpublished head above. It is not a full workspace gate, ARM64 qualification,
merge approval, deployment, physical-lamp test or latency measurement.
The original historical receipts remain unchanged. Preparing this folder did
not run tests, builds, Git operations, network requests or device actions.

The three records below are byte-for-byte copies of their retained inputs:

| File | SHA-256 |
| --- | --- |
| [identity.json](identity.json) | `81b41ef37cb5cb0d5e03f423aafe5a4023e46129d49e7d6d83646531bbeffbf0` |
| [range-diff.txt](range-diff.txt) | `b518714861d55b27453ccfbf51b1dd2b44d44db0818bb4f5194807de752925a4` |
| [mac-gate.json](mac-gate.json) | `4ce160a4a6ac1178fee2714b644a85e6cb04caafcffe853213d1cc2c8d71cb50` |
