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
