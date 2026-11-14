# Requested music: request object and actual confirmation

Third's carry of #5802 preserves Sixth's four authored commits. Original
head: `b2148739f5ea4261e0eeb130418a2e447deff288`. Its merge
`b676ec14c496893df5617fcafbb02478741c2844` was an upstream merge of
`a9eb18ecea88714510e66b4248879e963a40130a`; rebasing retains the four source
patches, with no separate merge-only source change.

## Findings and controls

- The recording noun now belongs to the immediate request object. Sheet
  music in a later clause and chess with a later music/mood mention do not
  authorize a recording. Whole-line “forget it” cancels the request.
- Exact-source classifier extraction: 36 cases, five mismatches before and
  zero after. Actual Engine tests cover the five refusal cases and four
  added positive controls through direct and wrapped skill calls.
- A wait cue and its retirement previously authorized a recording with no
  spoken confirmation. The clean fail-first run with packet provenance
  added but the old coordinator gate still in place failed the expected
  assertion (0 passed / 1 failed, 0.03 s).
- Only validated current-request answer PCM marks confirmation eligibility.
  Text without TTS PCM does not. Cue-only, transcript-before-cue and
  transcript-after-cue controls refuse playback. A spoken confirmation
  with gaps, or retired before the offer, still permits it after retirement.
- Live model answer audio retains its previous eligibility. Wait cues and
  announcements are specific Second-route outputs. Private worker and
  parent use the same versioned executable; the new field is not a
  bidirectionally compatible cross-version public protocol. Absent fields
  decode false; old decoders reject a true field.

## Local gate on the initial carry base

Base `aef04ab4966e404db08f79aac158591f87406014`, four authored commits rebased
through `75cd355b10bb8eaa640b4f77fd5204f0fa89981d`, then this repair:

- `cargo test -q -p lamp-voice --lib requested_audio -- --test-threads=1 --nocapture`:
  66 passed / one ignored manual timing test, 8.07 s.
- `cargo test -q -p lamp-voice --lib talk_about_music_gets_no_recording_and_the_model_is_told_so -- --test-threads=1 --nocapture`:
  one Engine matrix passed, 0.07 s.
- Exact serde and Live forwarding controls: one passed each.
- `a_line_begun_over_only_the_wait_sound_is_begun_in_the_clear`: one passed;
  the actual cue has false provenance.
- The broad `provenance --include-ignored` filter inadvertently selected
  `real_worker_session_records_verdict_provenance`, a manual endpoint test
  requiring `LAMP_ENDPOINT_WORKER`. That invocation was 4 passed / 1 failed;
  it is retained as a failed invocation, not reclassified as green. The
  intended controls were then run by exact name. No endpoint behavior changed.

Formatting (`cargo fmt --all --check`) and strict Voice all-target lint
(`cargo clippy -q -p lamp-voice --all-targets -- -D warnings`) passed.

## Host latency and limits

Target: under 1 ms for the in-memory packet-acceptance path; no new I/O,
poll, worker, network request or wait. The exact ignored diagnostic creates
256 fresh replies per case, then times `append_provider_packet` only.
Input-sample allocation, owner setup, IPC and playback are outside the
boundary; PCM queue allocation inside append is included.

The exact diagnostic run measured spoken-packet first 1.719 us, p95 1.248 us,
maximum 1.725 us; cue-packet first/max 26.978 us, p95 1.281 us. The earlier
broad invocation is retained separately. These are two packet kinds, not a
before/after speedup comparison. Neither this timing nor authored retirement
receipts prove acoustic or device latency. The existing first/repeated
retirement-to-write host control measured 538 us / 550 us against its
250 ms target in the initial requested-audio suite.

`LAMP_REQUESTED_AUDIO` stays off in the product. No activation, device action,
live provider account or public playback claim. Original two-read, played-row,
Store and default-on gates remain with their assigned owners.

## Rebase provenance

The final runtime repair is `4a03002e15ef293f4d73745e2fc75e3b2734f17b` on
`b3e2d0903fb9a9a5fd34c41d84af761f9db3413c`. All five commits compare `=`
against the initial carry. Documentation checks on this base pass: nine
tests, 192 documents, 1900 local links, none broken. The rebased runtime gate completed successfully:

- `cargo fmt --all --check`: passed.
- `cargo clippy -q -p lamp-voice --all-targets -- -D warnings`: passed.
- Requested-audio filter: 66 passed / one ignored manual diagnostic, 7.72 s.
- Entire `music_policy` module: four passed, 0.10 s, including the expanded
  direct/wrapped matrix and the existing boundary controls.
- Exact serde and Live forwarding tests: one passed each.
- Actual provider wait-cue control: one passed, 0.02 s.
- Prepared first/repeated retirement-to-write host handoff: 579 us / 580 us,
  both inside its 250 ms target. Acoustic onset remains unmeasured.

All commands ran sequentially from `lampOS/`, with Cargo jobs 2 and the
shared target directory. Follow-up evidence commits change only these
verification files; the runtime tested is the hash above. Full ARM and
Linux CI qualification follows publication; no local full-Voice pass is
claimed for this carry.

## Selector regression found by ARM CI

ARM run [42757338768](https://github.com/autonomous-ai/lampOS/actions/runs/42757338768)
failed: 12,931 passed, one failed, 113 skipped, with the run stopped before
all tests completed. The failed test was
`replaced_warm_listeners_hear_current_words_and_current_music_identity`:
“Now play the second recording.” was refused by the immediate-object
classifier. The [raw failure excerpt](arm-42757338768-failure.txt) is retained;
this run is not a full green ARM gate.

The follow-up is on base `75da83b34`, rebased head `8719c0699`, with the
selector repair still uncommitted when these local checks ran. The grammar
accepts one bounded selector (`first`, `second`, `next`, `previous`, `last`,
`another`, `different`) before the existing optional mood adjectives and
required audio noun. Actual fixtures also use “Play a different recording.”
and “Play another recording.” The warm fixture was not changed. Seven
selector positives are covered by classifier tests and direct/wrapped Engine
calls; unrelated-object negatives remain refused.

- The same 46 classifier cases ran against extracted exact production
  functions before and after: [seven mismatches before](selector-before.txt),
  [zero after](selector-after.txt). These standalone Rust probes exercise
  classification, not Engine behavior.
- `cargo test -q -p lamp-voice --lib music -- --test-threads=1 --nocapture`:
  [33 passed](selector-music.txt), 0.70 s.
- `cargo test -q -p lamp-voice --lib requested_audio -- --test-threads=1 --nocapture`:
  [66 passed, one ignored](selector-requested.txt), 7.51 s.
- Formatting and `cargo clippy -q -p lamp-voice --all-targets -- -D warnings`
  passed; the parent observed the completed gate's exit status 0. Both checks
  produced empty output logs.

These focused local results do not replace the failed ARM run or establish a
new full-suite pass. No device or acoustic latency verification was performed
for this selector follow-up.

## Compound requested objects

Second's HOLD on `0aa71fb62` showed that “Play music trivia with me”
authorized recording in both direct and wrapped Engine calls. The noun
prefix alone was insufficient. The follow-up requires a complete bounded
object ending: English please/now/on endings, or the existing Vietnamese
relaxation/recipient/politeness forms. It does not maintain a list of game
names. Unrecognized trailing phrases are conservatively refused.

The same 53-case exact-source probe has [five mismatches before](compound-before.txt)
and [zero after](compound-after.txt). Five compound-object negatives and
two polite-ending positives were added to the classifier and actual Engine
matrix. All previous 46 probe controls remain. Native follow-up on the train-768 base: 33 music tests passed (0.92 s),
66 requested-audio tests passed / one ignored (7.16 s), formatting passed.
Strict all-target Clippy first failed writing its incremental cache with
ENOSPC; [the failure](compound-clippy-enospc.txt) is retained. After removing
two completed clean Third worktrees and unlocked old incremental caches,
the same Clippy command passed (empty output, exit 0). No source changes
were made between these lint attempts. Native tests are retained in
[the music log](compound-music.txt) and [requested-audio log](compound-requested.txt).

## Selector-head ARM and current-base attempts

The watcher for [42761649338](https://github.com/autonomous-ai/lampOS/actions/runs/42761649338)
stopped on GitHub REST quota exhaustion. The later retrieved actual result
was failure: 1,856 passed / one failed, 113 skipped, stopping after 1,857
of 15,012 selected tests (37.800 s). The inherited Control test
`microphone::tests::never_while_someone_else_is_here` failed on
`rig.ears.send(block(2)).unwrap()` with SendError. The music carry does not
edit that source. The [failure excerpt](arm-42761649338-failure.txt) is
retained and sent to the #5839 owner; no harmless-flake claim is made.

All eight music commits rebased equal onto `c29008d21`, runtime head
`02432e3a5`. The first current-base gate exited 101 during disk exhaustion,
with an empty music log (so no detailed compiler/test diagnostic survived).
A separate attempted evidence write failed explicitly with no space left.
Eleven old, clean Third review worktrees were archived under Third refs and
removed, restoring about 4 GiB; dirty and other-builder checkouts were
untouched. Fresh current-base checks follow, not a relabeling of the failed
invocation.

Current-base retry completed successfully on runtime `02432e3a5`:
formatting and strict Voice all-target Clippy passed (empty output, exit 0);
[33 music tests](current-music.txt) passed in 0.73 s and
[66 requested-audio tests / one ignored](current-requested.txt) passed in
6.83 s. Current docs: 192 documents, 1900 links, zero broken. The added
evidence commit changes no runtime or tests. Fresh CI and the required
reads still qualify the resulting published head separately.

## Later spoken retractions (Fourth's follow-up)

On published head `f75e3da9456a3b07b7cf4ecdbb2c467a1536ae49`,
“Play some music. Scratch that.” and “Play some music. I changed my mind.”
still authorized a recording. The same two new classifier tests
[failed before](retractions-before.txt) and [passed after](retractions-after.txt).
The actual Engine matrix [failed before](retractions-engine-before.txt) at
its first new direct “Scratch that” case (one recording offered instead of
zero). That stopped the matrix: it does not prove that every new case was
observed failing before the repair.

The bounded whole-word retraction list now includes “scratch that” and
“changed my mind” alongside “forget it”. The actual Engine matrix covers
both direct and wrapped calls, including “I've changed my mind” and positive
controls with unrelated “scratch” and “changed my playlist”. This remains a
conservative whole-line veto, not a general natural-language intent parser.

Local checks after the repair, on the same `c29008d21` base:

- `cargo fmt --all --check`: passed. An initial invocation from the repository
  root [failed to find Cargo.toml](retractions-fmt-initial.txt); the first
  correctly located check [found a wrapping difference](retractions-fmt.txt).
  After `cargo fmt -p lamp-voice`, the repeated check passed.
- `cargo clippy -q -p lamp-voice --all-targets -- -D warnings`: passed,
  empty output, completed gate exit 0.
- `cargo test -q -p lamp-voice --lib retracts_the_recording_ask`:
  two passed, 0.00 s.
- `cargo test -q -p lamp-voice --lib music -- --test-threads=1`:
  [33 passed](retractions-music.txt), 0.77 s, including the expanded Engine matrix.
- `cargo test -q -p lamp-voice --lib requested_audio -- --test-threads=1`:
  [68 passed / two ignored manual diagnostics](retractions-requested.txt), 7.10 s.
- `cargo test -q -p lamp-voice --lib requested_audio::tests::recording_retraction_admission_latency -- --exact --ignored --nocapture`:
  [one passed](retractions-latency.txt). Across 256 alternating positive and
  negative admissions, first/max 253.262 us, p95 46.061 us, within the
  preselected 1 ms host target. The boundary includes folding/allocation and
  phrase checks inside `asks_to_play`, excluding input allocation, provider,
  IPC and playback. No before/after speedup or acoustic latency is claimed.

Docs links: 192 documents / 1900 links / zero broken before this evidence
append. The earlier green ARM run and reads qualify `f75e3da94`, not this
runtime delta; fresh CI and delta reads are required. The PR stays draft and
`LAMP_REQUESTED_AUDIO` stays off. No device work was performed.
