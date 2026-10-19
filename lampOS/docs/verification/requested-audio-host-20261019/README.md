# Requested audio: host verification, 2026-10-19

The current preparation combines the Bedtime review fixes on `4a188e789`,
accepted-decline ordering from #1833 and scoped context from #1839. Its
complete 43-crate host gate passed **7,708 tests, 0 failed, 44 ignored**,
none filtered, plus **12 separate own-harness tests**. Native ARM64 remains
pending for this preparation. Independent
source review found no remaining P1/P2 in the combined provider boundaries.

The earlier complete scope/VoiceTurn integration gate qualifies source `befb83aac5991dae2d3dc572c93c69152bea17d0`
on prepared Bedtime `8706fe851d89ea47ec9381a2da8f7017180f6688`. It passed **7,600 tests,
0 failed, 44 ignored**, none filtered, across all **42 crates**,
plus **12 separate own-harness tests**.
#1757 remains queued behind #1732 until its actual landing, final one-task
branch and native ARM64 run are qualified. Earlier results below qualify
only their stated revisions. No device was accessed.

## Current context integration: complete host gate

The [complete gate record](gates.json) qualifies frozen source
`3d2b511dee47a41dd8efead95168546113b95601` on Bedtime
`5e83af10210cb1a4bfbeb15887274d1e519fe4c8`. All **1,634** frozen
non-document source files and **157** non-evidence documents matched at
completion. The publication uses Bedtime `805f56961`; that parent differs
only in three verification documents. The Music amendment also changes
verification documents only. The tested source identity remains recorded
without substituting the publication commit for the frozen gate commit.

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.82 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 57.68 s |
| `cargo test --workspace --exclude lamp-update -- --test-threads=1` | 7,574 passed, 0 failed, 43 ignored | 1,342.36 s |
| `cargo test -p lamp-update -- --test-threads=1` (checkout) | 134 passed, 0 failed, 1 ignored | 75.15 s |
| `cargo build --workspace` | Passed | 18.53 s |

The record preserves every Rust summary, per-crate counts, commands and log
hashes, 43 changed inputs and the complete source digest. The
[identifier and consumer scan](consumer-coverage.json) covers 34 matched
files, five changed crates and the 27-crate reverse-consumer closure; all
43 workspace crates were tested. All 42 non-Update suites and the build ran
in the source-only copy. The entire Update suite ran in the checkout for
its inherited parent image fixtures; no test filter or new ignore hides
that portability limitation. The separate harness count is not added to
the Rust total. This preparation is still queued behind the actual
Bedtime landing; it does not qualify a later final PR head.

## Subsequent decline integration (not the final gate)

Frozen preparation `5d815c9f16` passed [ARM64 39231334336](scope-arm64.json):
7,682 passed (1 leaky), 48 skipped, plus 4 doc and 12 separate harness tests.
That dispatch tested the exact branch head. Release, relay, checksums and
artifact upload passed.

After #1833, [two new Live regressions reproduced dispatch after an accepted
decline](decline-before-fix.json). Later and parked calls now stop before
dispatch, and a declined request cannot promote a private offer. An action
already executed before a later decline remains acted. Both providers retain
original IDs and scope checks. [The corrected focused preflight](decline-after-fix.json)
passed both negatives, prior-action controls, 42 requested-audio tests,
eight decline tests, two credit tests and verdict ordering. Filters overlap.

[The added ReportsStop control](decline-reports-control.json) passes with the
two Live negatives: 3 passed, 0 failed/ignored, 2,147 filtered. It uses an
isolated private bus to prove consent withdrawal remains possible after a
decline with Policy holding off/on, without reviving speech or an offer.
ReportsStop cannot naturally enter the parked queue. This does not implement
the separate held-consent-withdrawal task on #1683.

Independent source review found no remaining P1/P2 in that scope. The actual
prepared Bedtime base and merged #1839 are now integrated, with the complete
host result above and native qualification still pending. The preceding counts qualify only
their recorded sources, not the eventual final Music PR head.

## Scoped-context integration with #1839

Scoped system context and requested audio remain independent switches. The
combined tests cover all four combinations with spoken and Written inputs,
the admitted identity after a face change, revocation during context
construction, private-result removal and owner retirement. Queued-offer and
real IPC credit checks now run with scoped context both off and on. The
Messages isolation regression also covers both requested-audio settings.
Production persistence remains once per dequeue; scope checks repeat at
every pending IPC send attempt.

The [first focused compile](context-preflight-compile-failure.json) passed
formatting, then stopped because the new deterministic revocation fixture
requested a Tokio runtime feature this crate does not enable. No tests ran.
The fixture now uses the existing current-thread runtime and a bounded OS
thread for actual camera revocation, with no dependency or production change.
The [corrected focused preflight](context-preflight.json) passed formatting
(3.76 s), strict all-target Voice Clippy (17.96 s), **44 requested-audio tests**
(38.27 s including compilation; 2,117 filtered), **3 accepted-decline tests**
(0.80 s; 2,158 filtered), and the separate scoped-system, Messages isolation,
Memory-scope and revoked-result filters. Every filter had zero failures and
ignored tests; the filters overlap and are not added together. All 1,634
non-document source hashes matched at completion. The subsequent complete
43-crate workspace gate is recorded above; native ARM64 remains separate.

## Earlier documentation clarification after the scope gate

After the complete gate, requested-audio.md was clarified to distinguish queued-offer scope checks from playback microphone-privacy guards. Camera-only scope revocation is not a cancellation signal after handoff. All 1,600 frozen non-document source files and the other 156 non-evidence documents remain unchanged. A literal path scan found no crate or workflow consumer of this document; no runtime change or additional test result is claimed.

## Earlier scope and final transport integration: complete gate

Both providers carry admitted Memory scope alongside the original spoken
audio context. Direct and wrapped Skills calls preserve their original call
ID, worker, session, request, microphone privacy generation and deadlines.
Memory calls keep the admitted identity after face changes. Missing or
revoked configured scope never falls back to an unscoped call. Late replies
and private offers are suppressed; completed RPC side effects are not undone.

For queued requested-audio offers, the final IPC send boundary repeats the
scope check on every retry. Exhausted
credit or socket capacity cannot preserve a revoked offer. Regressions cover
camera-only revocation with the same microphone owner, restored output credit,
and valid scope after generation completion. Scope authority stays internal
to the provider and cannot replace the recording's other playback guards.

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.63 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 55.50 s |
| `cargo test --workspace --exclude lamp-update -- --test-threads=1` | 7,470 passed, 0 failed, 43 ignored | 1,290.12 s |
| `cargo test -p lamp-update -- --test-threads=1` (checkout) | 130 passed, 0 failed, 1 ignored | 69.27 s |
| `cargo build --workspace` | Passed | 17.76 s |

[The earlier complete gate record](before-context-integration.json) preserves every Rust summary, per-crate
counts, commands, log hashes, 42 changed inputs and the
full 1,600-file non-document source digest. At gate completion, all frozen
source hashes and 157 non-evidence documentation hashes matched. The later
documentation clarification is recorded separately above.
[Identifier and consumer coverage](consumer-coverage.json) includes
33 matched files and the complete
27-crate reverse-consumer closure.
All 41 non-Update suites and the workspace build ran in the source-only copy.
The entire Update suite ran in the checkout because its inherited image
fixtures live in the parent repository; no test filter or added ignore hides
that portability limitation. Own-harness counts are separate from Rust totals.

The preceding rise integration reapplied Music onto Bedtime based on
`b03cb4a4b3`. The earlier combined `aba4e44a5` results remain preserved in
`before-rising-trigger-rebase.json` and `before-rising-trigger-arm64.json`;
they do not qualify the current source.

Allocated music slice of #1685. The earlier text-origin integration layered
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

## Failed first rise-integration gate

The first complete gate on source `4bcdf9fee1` passed formatting and strict
workspace/all-target Clippy, then stopped in Voice's
`real_worker_loads_renews_and_finishes_only_after_marked_retirement` test.
That binary reported **2,088 passed, 1 failed, 12 ignored**. Completed Rust
test summaries through the failure total **6,588 passed, 1 failed, 43 ignored**,
none filtered, plus the separate **12 passed, 0 failed** own-harness summary.
The complete Update suite and workspace build did not run after the failure.
[The failed record](rising-integration-failed-gate.json) preserves stage times,
source identity, every test summary and log hashes.

The assertion expected Start but did not record its received operation.
A rejected duplicate offer could read the cache while the first worker's
nonblocking permission check needed it, cancelling before Start. Monitor
contention or freshness can also cancel; the log cannot attribute the failure
to one path. Admission now reserves capacity before reading the cache and
releases it on rejection. The capacity fixture uses an observed gated Start;
a deterministic real-worker regression proves cache contention cancels without
Start and cannot revive after fresh data. Lifecycle assertions report the
actual operation and offer identity. No permission deadline is relaxed.
The corrected admission source passed its separate complete gate below; this
failed result remains preserved. The newer scope integration requires its own gate.

## Scope integration: earlier focused preflight and review

[The first compile attempt](scope-preflight-first-failure.json) stopped before
tests: a local runner variable shadowed an input-fixture helper. Renaming the
variable corrected that test-only error.
[The corrected focused preflight](before-transport-scope-preflight.json)
passed formatting, strict all-target Voice Clippy, and each of these filters:
requested audio **38/0/0** (2,097 filtered), scoped audio **2/0/0** (2,133
filtered), Memory scope **9/0/0** (2,126 filtered), admitted Memory with audio
**1/0/0**, revoked tool result **1/0/0**, and verdict ordering **1/0/0** (the
last three each 2,134 filtered). These overlapping filter counts are not
combined into a workspace total.

Independent review then found a later boundary: an offer held for IPC credit
could outlive personal-scope revocation while the microphone owner remained
current. Both final transport queues now repeat the scope check before every send
attempt. The separate final focused preflight below qualifies that correction;
the preceding preflight remains historical evidence only.

## Final scope and transport preflight

[The final focused record](scope-preflight.json) contains source hashes and
all command/log summaries. Formatting and strict all-target Voice Clippy
passed. The two real-IPC credit/revocation regressions passed **2/0/0**,
with 2,135 filtered. The complete requested-audio filter passed **40/0/0**,
with 2,097 filtered. Separate scoped-audio, Memory-scope, admitted-Memory,
revoked-result and verdict-ordering filters also passed; overlapping filter
counts are not combined. Independent static review confirmed the final queue
finding closed. The complete workspace gate remains separate.

## Admission correction: complete gate before scope integration

[The frozen admission record](before-scoped-memory-rebase.json) qualifies source
`067c05bbb7c6803bd38ba879b961a130f526006f`, above Bedtime `c7f9c90879`.
All 42 crates passed: **7,509 passed, 0 failed, 44 ignored**, none filtered.
The separate own-harness summaries report **12 passed, 0 failed**.
The original worker lifecycle regression passed in this complete run.

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.67 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed | 51.86 s |
| `cargo test --workspace --exclude lamp-update -- --test-threads=1` | 7,379 passed, 0 failed, 43 ignored | 1,261.13 s |
| `cargo test -p lamp-update -- --test-threads=1` (checkout) | 130 passed, 0 failed, 1 ignored | 69.92 s |
| `cargo build --workspace` | Passed | 18.21 s |

All 1,566 frozen non-document source hashes and 156 non-evidence documentation
hashes matched. All non-Update suites and the build ran in a source-only copy;
the entire Update suite ran in the checkout for its inherited parent image
fixture. [The focused admission preflight](worker-admission-preflight.json)
is separate. These results do not qualify the later scope integration above.

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
none filtered out. [Its historical record](before-rising-trigger-rebase.json) pins exact commands, every Cargo
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
