# Music: current-head native CI evidence

[PR #1757](https://github.com/autonomous-ai/lampOS/pull/1757), head `bd1d79a5ad1c4ce1fcd4f6674702265cc879b8b0`.
**Current head CI qualified: yes.**
This receipt qualifies only the exact head above. The earlier superseded-head
receipt remains separate; no merge or device acceptance was performed here.

[ARM64 run 39702508957](https://github.com/autonomous-ai/lampOS/actions/runs/39702508957)
and [Linux Clippy 39702508955](https://github.com/autonomous-ai/lampOS/actions/runs/39702508955)
were checked against their terminal metadata, checkout logs and GitHub commit parents.
The tested base is `bf8c3f2e18695179bd70973821368f01d6a2d9e3`; merge hashes are retained in [ci.json](ci.json).
The branch base was `fc800db6f839cf006098f73ee84942572acbccfa`. The actual CI base above
comes from both checkout logs, run API associations and merge-commit parents.

```text
Summary [ 171.423s] 10118 tests run: 10118 passed (1 leaky), 63 skipped
```

Doc tests: 4 passed / 0 failed /
0 ignored. Own-harness tests:
12 passed / 0 failed.
Relay success: **yes**; selected Music regressions passed: **12/12**.
The JSON records their exact results, 1 leaky pass, skips,
8 ARM example-output collision warnings, infrastructure notices,
log line numbers and raw evidence hashes. Strict Clippy reported
0 Rust warnings. The workspace was selected in full;
the PR profile still skips realtime tests.

Source: [Skills asset checks](https://github.com/autonomous-ai/lampOS/blob/bd1d79a5ad1c4ce1fcd4f6674702265cc879b8b0/lampOS/crates/skills/src/requested_audio.rs),
[Voice playback lifecycle](https://github.com/autonomous-ai/lampOS/blob/bd1d79a5ad1c4ce1fcd4f6674702265cc879b8b0/lampOS/crates/voice/src/requested_audio/runtime.rs),
[speaker-state regressions](https://github.com/autonomous-ai/lampOS/blob/bd1d79a5ad1c4ce1fcd4f6674702265cc879b8b0/lampOS/crates/actions/tests/speaker_state.rs).

This is PR-profile native Linux evidence, with realtime tests skipped; it is
not a physical-lamp or latency measurement. Skipped identities were not emitted
by the retained log; its summary records the count. Artifact metadata is retained,
but the payload was not downloaded or independently verified. Any earlier
failed results remain separate. Qualification errors, if any, are listed in ci.json.
