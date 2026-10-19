# Requested audio: host verification, 2026-10-19

Allocated music slice of #1685. The current local Music integration layers
its source onto Bedtime `65ffb51c77636f4d5359d5593083aa8170c3bb3b` on integration
base `6ea2b8ffe12502918987d1d6c52f004900679d23`, including the text-origin body
tool guard from #1673. Its complete integration consumer gate passed
**4,288 passed, 0 failed, 20 ignored**, none filtered out.

The complete light-scene workspace gate passed **7,432 passed, 0 failed,
44 ignored**, none filtered, on the earlier snapshot with Bedtime
`b7dbae26ff657d8a7487dc4e8fed40be9b491b38` on `4bcf71bd8`. The subsequent
complete ten-crate consumer gate passed on the text-origin integration.
The earlier full run and [changed-source coverage](text-integration-coverage.json)
are recorded separately, with the final follow-up counts below.

This records software checks for the default-off requested-audio step and
original installable music example. The earlier standalone gate used
`89bd0e65f` and passed for published music head `97cdfbea7`; its results below
remain historical evidence. Native ARM64 must pass on each published head.
No lamp was accessed.

## Boundaries under test

| Evidence | What it establishes |
|---|---|
| [Coordinator fixtures](../../../crates/voice/src/coordinator/tests/requested_audio.rs) | Real admitted owners, private Skills socket exchanges, speaker IPC dispatch and authored acceptance/retirement. Generation completion and accepted final chunks cannot substitute for retirement. A repeated request gets a new owner; the prior recording cannot resume. |
| [Provider worker](../../../crates/voice/src/provider_worker.rs), [second route](../../../crates/voice/src/second_provider.rs), [tool routing](../../../crates/voice/src/tools.rs) | Both spoken routes preserve the original call/session/worker/privacy identity and hide private offers from the model. Written, typed Messages, empty-input, non-Skills, stale and retired requests cannot acquire an offer. |
| [Voice runtime](../../../crates/voice/src/requested_audio/runtime.rs) | Capacity one, two workers reused across conversations, immutable checked samples, original deadlines, bounded socket exchanges and expiring leases. Cancellation during preparation rejects a later successful reply. Mute, stale state, transport loss and restart cannot revive an old ticket. |
| [Skills socket tests](../../../crates/skills/tests/requested_audio.rs) | Strict definition and private receipt shapes, speech permission, immutable asset identity, duplicate/terminal calls, definition revocation, daemon restart and default-off refusal through the actual service binary. |
| [Actions socket tests](../../../crates/actions/tests/speaker_state.rs) | One mixer owner, mute and tighter-cap withdrawal before mixer I/O, retained zero/loss cancellation generations, cap compliance and recovery, all four switch combinations, publication/storage failure cleanup and no extra mixer I/O when disabled. |
| [Renderer](../../../crates/skills/examples/render_relaxing_music.rs) | Deterministic source rendering, exact duration/WAV fields, non-silent structure, fades and all sliding 10 ms amplitude windows. |

The cancellation matrix invokes the coordinator cancellation helper directly;
it covers terminal tickets and late workers, not all main-loop event routing.
A separate test changes Controller privacy and verifies dispatch refusal.
The route-binding test refuses rebinding an accepted offer to another session.

## Text-origin integration: complete consumer gate

The combined source preserves the typed body-tool guard with requested audio
both off and on. A typed direct skill call or translated `use_skill` call
whose tool is named in catalog `body` metadata is refused before the Skills
socket is called. Ordinary typed non-body calls retain their route, and
spoken body calls remain available. The tool-routing regression
`a_text_never_runs_a_skill_that_moves_the_body` exercises both switch values
and keeps a live socket peer for the refused typed call and allowed control.

Typed direct and wrapped media calls never send `requested_audio_context`.
An unexpected private `requested_audio` offer in the service reply is stripped
and discarded before the model sees the result. The existing private-route
regression covers both typed paths. A verified spoken Skills call retains its
original call/session/worker/privacy binding and private typed offer, including
through `use_skill`; the model receives only the ordinary result.

Skills builds `tools` and optional `body` metadata from the same emitted
definitions, after disabled/media-off filtering and first-name selection.
`catalog_body_metadata_follows_filtered_duplicate_tool_winners` covers omitted
definitions, duplicate winners with differing body grants, media off/on and
the unchanged tools-only shape when no emitted tool has a body grant. The
earlier strict `show`/`if_choice` media-shape rejection remains in place.
The complete ten-crate gate passed **4,288 passed, 0 failed, 20 ignored**,
none filtered out. [Its record](gates.json) pins exact commands, every Cargo
summary, all 41 changed inputs and the complete 1,558-file source-copy digest.
All source hashes matched at completion. [The source comparison](text-integration-coverage.json)
shows runtime/test changes since the full workspace gate only in Memory,
Skills and Voice; the complete suites cover their entire ten-crate reverse
consumer closure. Other non-`docs/` changes are two builder documents.
Counts from the two runs are separate, not added.

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.62 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 55.69 s |
| `cargo test -p lamp-body -p lamp-cloud -p lamp-expression -p lamp-keeper -p lamp-lean -p lamp-memory -p lamp-skills -p lamp-voice -p lamp-voice-eval -- --test-threads=1` | 4,163 passed, 0 failed, 19 ignored | 728.52 s |
| `cargo test -p lamp-update -- --test-threads=1` | 125 passed, 0 failed, 1 ignored | 61.81 s |
| `cargo build --workspace` | Passed | 34.01 s |


## Focused preflight

[Preflight records](preflight.json) preserve command output summaries, log
hashes, the earlier failures and the successful package round trip. These are
historical focused results; the final complete gate covers the final source.

- Voice: **34 passed, 0 failed, 0 ignored, 2,038 filtered out**.
- Skills authorization integration: **12 passed, 0 failed, 0 ignored**.
- Renderer: **4 passed, 0 failed, 0 ignored**.
- Initial complete Actions/message suites: **183 passed, 0 failed, 0 ignored**.

The prepared host handoff measured **483 us first / 592 us repeated**, from
confirmed confirmation retirement to authored acceptance of the first media
PCM, against a **250,000 us** target. The fixture uses a small checked WAV and
the real dispatch/retirement helpers. This is neither acoustic onset nor a
measurement of playback of the full example on a lamp.

## Combined-feature preflight before the light-scenes rebase

The [combined preflight](combined-preflight.json) records strict all-target
Clippy and complete focused tests on the isolated combination of Bedtime
`d27632ab6` and music: Skills **185 passed, 0 failed, 0 ignored**; Actions
**105 passed, 0 failed, 0 ignored**. Neither test command filtered tests, and
all frozen dependency source hashes matched afterward. The Actions socket
target initially included 18 tests covering the shared mixer and cap boundaries.
A review then found that rejected volume arguments could revoke audio under
a confirmed cap without mixer work. The fix distinguishes actual observation
progress; its follow-up strict Clippy and complete Actions suite passed
**107/0/0**, none filtered, including 20 socket tests. Genuine read/write
failures still revoke permission. The earlier 105-test result remains in the
record. These results precede the later light-scenes rebase; the final integrated workspace
gate must cover that new source separately. The scene integration also found
that the two media-step predicates omitted the new `show` field. Explicit
`show` and `if_choice` exclusions, with a valid ordinary-show control, now
preserve the exact two-step contract. That new regression is included in
the completed light-scene workspace gate below.

## Reproducible package

The original integer score produces 1,152,000 frames: 48 seconds of 24 kHz
mono PCM16, 2,304,044 bytes including its WAV header. Peak is 900/32768
(approximately -31.2239 dBFS); maximum sliding 10 ms RMS is approximately
-38.6041 dBFS. Source, renderer and generated output carry Apache-2.0
provenance. No recording, package or binary is committed.

The existing `lamp-skills pack` and `install` commands succeeded in an ignored
disposable directory. All seven installed files match their staged inputs
exactly. The WAV SHA-256 is:

```text
134362ab72a19c143c627dbd438f3d17ceabe9c6923bad6f1ea3f7d5fb9579fa
```

The first install attempt exposed a missing prerequisite in the example:
the destination root must exist. The README now creates it before install;
the generated package was rebuilt and the full round trip repeated.

## Standalone workspace gate before Bedtime integration

The [consumer scan](consumer-coverage.json) selects 27 changed/reverse-dependent
crates. The final gate passed across all 42 workspace crates:

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.46 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 119.84 s |
| `cargo test --workspace --exclude lamp-update -- --test-threads=1` | 7,135 passed, 0 failed, 43 ignored | 1,324.82 s |
| `cargo test -p lamp-update -- --test-threads=1` (checkout) | 122 passed, 0 failed, 1 ignored | 47.20 s |
| `cargo build --workspace` | Passed | 21.39 s |

Combined: **7,257 passed, 0 failed, 44 ignored**, none filtered out. The
[standalone gate record](before-bedtime-integration.json) includes every Cargo summary, per-crate counts,
command/log hashes, all 40 changed source/configuration hashes and the full
1,509-file source-copy digest. All source hashes matched at completion. The
final Actions/message suites total 187 passing tests, including the later
retained-observation regressions; the earlier 183-test result remains history.

Update's inherited image-journal test uses a parent repository fixture, so
the complete Update suite runs in the checkout; other suites and the build
run in a source-only copy. No extra ignore or test filter is added to this
complete gate. The renderer's four example tests are recorded separately in
the focused preflight; its source remained unchanged through the final gate.

## Complete combined workspace gate before the text-origin rebase

This source combines Bedtime and Music on integration base `4bcf71bd8`.
All 42 crate suites and every reverse consumer passed:

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.57 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 55.19 s |
| `cargo test --workspace --exclude lamp-update -- --test-threads=1` | 7,307 passed, 0 failed, 43 ignored | 1,329.35 s |
| `cargo test -p lamp-update -- --test-threads=1` | 125 passed, 0 failed, 1 ignored | 63.67 s |
| `cargo build --workspace` | Passed | 17.97 s |

Combined: **7,432 passed, 0 failed, 44 ignored**, none filtered out. The
[complete record](before-text-guard-rebase.json) includes every Cargo summary,
command/log hashes, all 41 changed source/configuration inputs and the full
1,556-file source-copy digest. Every source hash matched at completion.
The Actions suite passed 108 tests and Skills passed 192, including the
combined cap/media, rejected-volume and exact media-shape regressions.
Update ran its entire suite in the checkout for the inherited parent image
fixture; all other suites and the workspace build ran from the source-only
copy. The later text-origin integration receives its complete affected-consumer
gate separately; this result does not alone validate the later changes.

## Standalone native ARM64

Published head `97cdfbea7` passed [run 39204794234](https://github.com/autonomous-ai/lampOS/actions/runs/39204794234):
**7,342 passed (1 leaky), 48 skipped**, plus **4 doc tests** and **12 own-harness
tests**, all passing. Release build, checksums and artifact upload succeeded.
The existing leaky test was `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
[The native record](standalone-arm64.json) pins the exact head and log hash;
it does not validate the later combined source.

## Remaining validation

The final Linux ARM64 run must pass on the published head. Device listening,
first/repeated acoustic start, physical Stop/mute timing, cold filesystem work
and combined-feature rollout remain separate lamp checks. The runtime switch
stays off in all product and lamp units; installing the example cannot enable it.
