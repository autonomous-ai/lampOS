# Music: exact-head native CI evidence

[PR #1757](https://github.com/autonomous-ai/lampOS/pull/1757), head `d96f484ec0567ecd66819cb45bd44f5c39961118`.
**Pinned head CI qualified: yes.**
This receipt qualifies only the exact head above. The earlier superseded-head
receipt remains separate; no merge or device acceptance was performed here.

[ARM64 run 39713220661](https://github.com/autonomous-ai/lampOS/actions/runs/39713220661)
and [Linux Clippy 39713220653](https://github.com/autonomous-ai/lampOS/actions/runs/39713220653)
were checked against their terminal metadata, checkout logs and GitHub commit parents.
The tested base is `fab69b0c4c4333ff8d51459e063169af7355e1dc`; merge hashes are retained in [ci.json](ci.json).
The branch base was `17fdfda1d757324b8437d997304e855e8ecbf9dd`. The actual CI base above
comes from the pinned checkout logs and immutable merge-commit parents, with
both run metadata sources independently agreeing on the head. Embedded live
PR head/base associations are recorded separately and are not authority for
this qualification.

```text
Summary [ 184.814s] 10172 tests run: 10172 passed (1 leaky), 63 skipped
```

Doc tests: 4 passed / 0 failed /
0 ignored. Own-harness tests:
12 passed / 0 failed.
Relay success: **yes**; selected Music regressions passed: **12/12**.
The JSON records their exact results, 1 leaky pass, skips,
8 ARM64 warning entries, infrastructure notices,
log line numbers and raw evidence hashes. Strict Clippy reported
0 Rust warnings. The workspace was selected in full;
the PR profile still skips realtime tests.

Source: [Skills asset checks](https://github.com/autonomous-ai/lampOS/blob/d96f484ec0567ecd66819cb45bd44f5c39961118/lampOS/crates/skills/src/requested_audio.rs),
[Voice playback lifecycle](https://github.com/autonomous-ai/lampOS/blob/d96f484ec0567ecd66819cb45bd44f5c39961118/lampOS/crates/voice/src/requested_audio/runtime.rs),
[speaker-state regressions](https://github.com/autonomous-ai/lampOS/blob/d96f484ec0567ecd66819cb45bd44f5c39961118/lampOS/crates/actions/tests/speaker_state.rs).

This is PR-profile native Linux evidence, with realtime tests skipped; it is
not a physical-lamp or latency measurement. The log does not emit a complete skipped-name list; the summary records the exact count.
The publication job is required to be skipped. Artifact metadata is retained,
but the payload was not downloaded or independently verified. Any earlier
failed results remain separate. Qualification errors, if any, are listed in ci.json.
