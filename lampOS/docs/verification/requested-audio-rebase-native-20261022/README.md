# Music: prior-head native CI evidence

[PR #1757](https://github.com/autonomous-ai/lampOS/pull/1757), head `6208a602bd7c409aa7a8396404518ac218cb17c1`.
**Successful prior head, superseded by subsequent rebase.**
Prior-head CI evidence verified: **yes**.
This receipt does not establish current PR readiness. The prior head conflicts
with newer integration and is being rebased; a later head needs its own CI.

[ARM64 run 39698425178](https://github.com/autonomous-ai/lampOS/actions/runs/39698425178)
and [Linux Clippy 39698425278](https://github.com/autonomous-ai/lampOS/actions/runs/39698425278)
were checked against their terminal metadata, checkout logs and GitHub commit parents.
The tested base is `515e43d3abc37db719338883d0aa7e9465ca94ac`; merge hashes are retained in [ci.json](ci.json).
The base supplied at assignment was `515e43d3abc37db719338883d0aa7e9465ca94ac`. The actual CI base above
comes from both checkout logs, run API associations and merge-commit parents.

```text
Summary [ 172.251s] 10071 tests run: 10071 passed (1 leaky), 63 skipped
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

Source: [Skills asset checks](https://github.com/autonomous-ai/lampOS/blob/6208a602bd7c409aa7a8396404518ac218cb17c1/lampOS/crates/skills/src/requested_audio.rs),
[Voice playback lifecycle](https://github.com/autonomous-ai/lampOS/blob/6208a602bd7c409aa7a8396404518ac218cb17c1/lampOS/crates/voice/src/requested_audio/runtime.rs),
[speaker-state regressions](https://github.com/autonomous-ai/lampOS/blob/6208a602bd7c409aa7a8396404518ac218cb17c1/lampOS/crates/actions/tests/speaker_state.rs).

This is PR-profile native Linux evidence, with realtime tests skipped; it is
not a physical-lamp or latency measurement. Artifact metadata is retained,
but the payload was not downloaded or independently verified. Any earlier
failed results remain separate. Qualification errors, if any, are listed in ci.json.
