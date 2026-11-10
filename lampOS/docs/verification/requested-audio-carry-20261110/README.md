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
retirement-to-write host control remains in the requested-audio suite.

`LAMP_REQUESTED_AUDIO` stays off in the product. No activation, device action,
live provider account or public playback claim. Original two-read, played-row,
Store and default-on gates remain with their assigned owners.
