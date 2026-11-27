# Caller match trial for ordinary follow-ups

`LAMP_FOLLOW_UP_CALLER_MATCH=on` enables sensor 6 on the second route for a trial. All product units leave it off. It uses the CAM++ worker already available for turn-taking on that route, including the private-room cut-word join. It needs `LAMP_SAID_TO_ME`. Enabling it starts no model and requests no extra recordings.

An admitted spoken call supplies the reference in RAM. Its spoken answer opens the 20-second window. Follow-ups can extend the window through their spoken answers while the reference stays with the opening call. A stop, privacy change, conversation close, forget or replacement caller invalidates it. An ordinary follow-up with a timely cosine of at least **0.70** can pass the look check. Confirmation replies, identity, speaker enrollment, memory and linked-service permissions keep their own rules.

The worker observes available prints on the audio path, at admission and playback, and on the existing room tick at 20 ms intervals. All reads are nonblocking. Admission freezes on the first verdict. Both prints must be available by that verdict and within **100 ms after line end** for a positive admission vote. A later match can help only at the first playback receipt of an already admitted ordinary line whose words alone failed to tie it to the conversation. A newer line, an expired window, an incomplete clip, overlapping playback, an unavailable reference or a failed printer supplies no help.

## Offline replay

We applied the existing fixed cosine bands to the archived 1.8-second CAM++ scores from 19 lamp-quiet clips of synthetic voices. The microphone, clip durations, speaker labels and complete pair scores are recorded in the [clip manifest](../same-voice-20261015/clips.tsv) and [pair table](../same-voice-early-20261018/pairs-early.tsv). The earlier experiment describes the [audio cuts and timing](../same-voice-early-20261018/README.md).

| Label | Pairs | Same (≥0.70) | Different (<0.30) | Between the bars | Shorter clip <1.5 s |
|---|---:|---:|---:|---:|---:|
| Same voice | 75 | 18 | 0 | 19 | 38 |
| Different voices | 96 | 0 | 27 | 2 | 67 |

To reproduce the table, join each pair ID to its row in the clip manifest. Label the pair by equality of the two speaker labels. Count the short-clip column first using the smaller recorded duration, then classify the remaining cosine. This yields 18/75 correct same-voice decisions and 27/96 correct different-voice decisions when abstentions count as misses. The fixed bands and the short-clip rule miss the 95% target on this historical set. Pairs reuse clips. The archive has no per-line availability timestamp and cannot establish how many matches would arrive in time.

## On-lamp scoring

Each `lamp-voice caller-match` journal row is numeric JSON with a kernel capture `clock_id`, caller and request IDs, cosine and class. The first `admission` row freezes the vote. `input_end_us` is the actual clip submission time. `line_end_us` is the acoustic line end. `ready_us` records when both prints were available to the worker. `decision_us` records when this gate evaluated the line. A separate `admitted` row supplies `first_admission_us` only when the worker lets the line through. `available` can report a subsequent result for a held or admitted line. `renewal` records the first playback receipt. These are observations of one trial.

Measure worker latency as `ready_us - input_end_us` (warm p95 target 500 ms). Separately report the share ready by the manifest’s acoustic end plus 100 ms and the share ready before actual first admission. Preserve the class denominators when a clip is missing or abstains. Only the frozen, live on-body holdout can qualify. [Reviewer’s scoring contract and example files](https://github.com/autonomous-ai/lampOS/issues/8850#issuecomment-7211763273)

### Producing a run

From `lampOS/`, use the Reviewer’s version 1 manifest, private sidecar, and run header for `voice_match` with an empty `predictions` array. Bind the latter two to the SHA-256 of the exact manifest file. Set `location` to `local` and record the Git build, model and configuration hashes. The adapter preserves optional producer-binary and source-patch hashes. Keep `development` and `holdout` in separate runs.

```sh
cargo run -p lamp-voice-eval --example caller_match_predictions -- \
  live manifest.json sidecar.json header.json audit.json journal.txt > run.json
cargo run -p lamp-voice-eval --example caller_match_predictions -- \
  replay manifest.json sidecar.json header.json audit.json SESSION_ROOT CAMPLUS.onnx > replay.json
```

The private audit is an array of capture observations. Each entry supplies `case_id`, the observed `added_wait_us`, `held_line` and `safety_violation`, and for live runs the opening request’s `caller_request_id` string. The journal must retain the worker’s phase rows. The adapter joins them by worker epoch, request, opening and clock. Missing observations leave the opportunity in the manifest without a prediction. In the Reviewer’s invented example the prediction is ready 70 ms after submission and 80 ms after acoustic end. Actual first admission has already happened 30 ms earlier.

For replay, set the header’s `evidence` to `replay`. Supply the candidate’s recorded submission time in `input_end_us` and the observed lamp playback flags `over_playback` and `reference_over_playback`. A missing submission or clip path produces no prediction. Overlap or missing playback status yields `unknown`. The private WAV paths must resolve under their respective consenting, unexpired session folders. Supply the captured 16 kHz mono PCM as heard by the worker, including its 300 ms pre-roll. The adapter verifies the model hash and uses the first 1.8 s. Replay records the host calculation time in `replay_elapsed_us`, with `ready_us` null. The scorer can compare the raw class to the label. Lamp readiness and first admission require a live capture. [Exact input and output examples](https://github.com/autonomous-ai/lampOS/issues/8850#issuecomment-7211763273)

On the builder’s Mac, the live adapter reproduced the Reviewer’s complete invented voice-match run. Replaying two identical synthetic waveforms through the pinned product CAM++ model returned `same` and `ready_us: null`. One debug run took 1.15 s to compute both embeddings and the cosine, after loading the model and reading both files. The accuracy figures above come from the labelled archive.

For the host timing check, run `cargo test -p lamp-voice --lib measure_caller_match_admission -- --ignored --nocapture`. In two runs on the builder’s Mac, the additional vote on two cached 256-value test vectors took 51–99 µs on first use and 5.5–8.0 µs at warm p95 over 2,000 calls per run. With a missing print it took 2.1–3.0 µs initially and 0.17–0.21 µs at warm p95. The diagnostic queue had a stalled reader. These timings cover the vote function. CAM++ inference, ASR, audio playback and the hardware timing use separate measurements.

The team lead records 240 distinct held-out pairs on the team lamp with at least six consenting speakers. Use 120 same-caller pairs across sessions and 120 other-source pairs (80 other people, 40 TV). Include short replies and 40 lines over playback, balanced by class. Keep calibration speakers and sessions separate. The positive and negative classes must each reach 114/120, with no holds or lost renewals caused by the trial. The lead measures cold and warm model timing, the share usable within 100 ms, and end of speech to first audible reply. Before switch-on the Reviewer and lead compare stable and candidate on the same played and live voice sets. [Sensor plan](https://github.com/autonomous-ai/lampOS/issues/8849) · [Shared measurement protocol](https://github.com/autonomous-ai/lampOS/issues/8850) · [Tiếng Việt](../../vi/caller-match_vi.md)
