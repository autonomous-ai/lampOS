# Requested music: second integration rebase

PR #1757, current head `bd1d79a5ad1c4ce1fcd4f6674702265cc879b8b0`,
rebased onto `fc800db6f839cf006098f73ee84942572acbccfa`.
The reviewer-read head is `0a878cd4d9d44867b2971e70c118244f0c21ad38`.
The prior integration head was `6208a602bd7c409aa7a8396404518ac218cb17c1`.

- [Complete range-diff from the reviewed head](from-reviewed-range-diff.txt).
- [Incremental range-diff from the first integration](range-diff.txt).
- [Source identity and upstream preservation](identity.json).
- [Mac gate and all 481 before/after source hashes](mac-gate.json).

Only the `Script` helper fields conflicted in this second rebase:
upstream `speech_hold` and `speech_ready` remain alongside Music `skipped`.
Both execution paths and test sets remain (221 distinct helper/test
functions, the exact union of the two sides). Automatic merges retain
playback-gap receipt validation, per-turn Memory links and the 500 ms
endpoint floor. All 65 nonoverlapping upstream paths match the new base;
all 34 earlier proof files are unchanged. Integration read found no P1/P2.

Rust 1.96.0 commands from lampOS, each exit 0:

- `cargo +1.96.0 fmt --all --check`.
- `cargo +1.96.0 clippy --locked --offline -p lamp-msgs -p lamp-actions -p lamp-skills -p lamp-voice -p lamp-voice-eval --all-targets -- -D warnings`.
- `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-voice -p lamp-voice-eval -- --test-threads=1`: **4067 passed / 0 failed / 14 ignored / 0 filtered out**, 532.869 seconds.

Only Voice and Voice-eval changed since the successful five-crate gate.
Msgs, Actions and Skills have identical Git trees to `6208a602b`, so their
prior **167**, **120** and **452** passing tests (zero failures/ignores)
are retained separately, not described as a fresh run. The prior full gate
was **4780/0/14/0**; its [receipt](../requested-audio-rebase-20261022/README.md)
remains intact. All five crates passed strict Clippy on this new source.

The prior head passed ARM64 **39698425178** and Linux Clippy
**39698425278**: [historical native evidence](../requested-audio-rebase-native-20261022/README.md).
That success does not qualify this new head, whose own PR runs must pass.
No full local workspace gate, local ARM64 build or device result is claimed.
