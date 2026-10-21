# Unplug fixture: native qualification

Recorded 2026-10-22 00:20 +07. [PR #2694](https://github.com/autonomous-ai/lampOS/pull/2694)
head `1ffd3f3b4ee3ad0b5714a591e3d89bc40c751e37` passed [ARM64 39640912728](https://github.com/autonomous-ai/lampOS/actions/runs/39640912728)
and [x86 Clippy 39640912678](https://github.com/autonomous-ai/lampOS/actions/runs/39640912678).
The native checkout is merge `1538834dccf7ac28a59076838ace2249d6776e29` with
`lamp-voice` `b87b3e91ad04b5811fe441efa118773d33416630`.

```text
Summary [ 157.680s] 9752 tests run: 9752 passed (1 leaky), 63 skipped
PASS [   0.139s] (4917/9752) lamp-skills::it skills::unplug_tool_returns_before_folding_and_says_ready_only_after_render_receipt
```

The separate doc-test stage reports **4 passed / 0 failed**;
`relay_signals: ok`; the own-harness stage **12 passed / 0 failed**.
Release build, binary classification, checksums and artifact upload passed.
The known Voice journal test is the one leaky pass.

The Mac gate remains **386 passed / 0 failed / 0 ignored / 0 filtered**,
with formatting and strict Skills Clippy passed. The controlled Darwin
accepted-socket reproduction and the original #2692 Mac failure remain
separate evidence. The correction changes only the fixture's accepted
socket mode; all receipt assertions and its two-second timeout remain.

[ci.json](ci.json) records exact summaries, line numbers, source identity,
the unedited GitHub log's hash and artifact metadata. Doc counts include
only summaries immediately belonging to `Doc-tests` sections.

## What was not shown

No lamp, playback or device latency was measured. Artifact digests are
recorded as reported by GitHub; payloads were not downloaded or independently
hashed. This native result does not erase the original failed Mac gate
or qualify later integration commits.
