# Offline sensor scorers

The protocol, collection matrix, thresholds, privacy boundaries and clock
definitions are in [sensor-evaluation.md](../../docs/sensor-evaluation.md).
Run from a Mac/server, never as a lamp service. This is a standalone Rust
workspace; it does not add a product dependency or a product switch.

Publication is draft-only under the owner's 27 November hold: **no merge or
train**. Each sensor must show its own quality before a separate addition to
the reviewed "Hey Lamp" plus 20-second renewing-window baseline. No sensor
decides on that baseline. The baseline build/configuration and matching
voice rubric still need to be pinned; the historical v1 played cases do not
qualify the reset voice policy.

## Build and check

```sh
cd lampOS/tools/sensors
cargo fmt --all --check
cargo clippy --offline --locked --all-targets -- -D warnings
cargo test --offline --locked
cargo build --offline --locked --workspace
```

Install the repository's pinned Rust toolchain and prepare the locked Cargo
cache before a recording session. Wrappers enter this directory; input and
output arguments therefore use absolute paths. `run.sh validate MANIFEST`
prints the SHA-256 of the exact manifest bytes. Each `score-*.sh` takes
`MANIFEST RUN NEW_OUTPUT_DIRECTORY`. `compare-voice.sh` takes
`COMPARISON NEW_OUTPUT_DIRECTORY`. Exit codes: 0 pass, 1 failed gate with a
report, 2 invalid input/I/O. Validation alone is not a gate pass. Existing
output directories are refused, including an input directory.

The `lampOS sensor scorers` PR workflow runs these same gates natively on
Linux ARM64. It exercises this standalone workspace; the product workspace's
existing jobs do not replace the scorer tests.

## Minimal annotation example

Exact addressed, replay, voice-match and private adapter join examples are
in [adapter-examples.md](adapter-examples.md).

This is invented schema data. It cannot qualify (one speaker/episode,
not the required shared collection). Real recordings never go in this folder.

```json
{
  "version": 1,
  "collection_id": "example-only",
  "frozen_unix_s": 1000,
  "sessions": [{
    "id": "s01", "split": "holdout", "started_unix_s": 2000,
    "consent": true, "delete_after_unix_s": 88400,
    "clock_id": "capture-boot01", "sync_error_us": 1000,
    "recording_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  }],
  "trials": [{
    "id": "c001", "session_id": "s01", "speaker_id": "p01", "source": "person",
    "start_us": 1000000, "end_us": 2000000, "call_end_us": 1300000,
    "last_lamp_audio_end_us": null, "recipient": "lamp", "script_id": "call01",
    "look_target": "screen", "head_target": "lamp", "bearing_deg": 0,
    "distance_mm": 600, "light": "daylight", "noise": "quiet",
    "visible_faces": ["track7"], "subject_face_id": "track7", "subject_visible": true,
    "eyes_epoch": "eyes-boot01", "overlap": false, "first_use": true,
    "condition": "normal"
  }],
  "cases": [{
    "id": "head-c001", "sensor": "head", "trial_id": "c001",
    "reference_trial_id": null, "expected": "lamp", "fault": false,
    "opportunity_us": 1800000
  }]
}
```

For the matching run, replace `manifest_sha256` with the hash printed by
validation. Do not copy a hash from a differently formatted file.

```json
{
  "version": 1,
  "manifest_sha256": "HASH_FROM_VALIDATE",
  "sensor": "head",
  "build_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "model_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
  "config_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
  "evidence": "synthetic", "location": "local",
  "predictions": [{
    "case_id": "head-c001", "clock_id": "capture-boot01", "output": "lamp",
    "input_end_us": 1900000, "ready_us": 1950000,
    "subject_face_id": "track7", "eyes_epoch": "eyes-boot01", "reason": null,
    "added_wait_us": 0, "held_line": false, "safety_violation": false
  }]
}
```

This row reports 100 ms sampling delay, 50 ms capture-to-result and 150 ms
opportunity-to-result. It never substitutes post-mesh time for capture time.
A missing row remains in the denominator. Optional numeric fields are null
when unavailable, never synthetic zeroes. Explicit `unknown` is an output,
not permission to remove a trial.

Every prediction's `clock_id` must equal its trial session's capture clock.
For server inference, timestamp receipt on the capture host (or apply the
measured clock mapping); do not subtract server and lamp monotonic clocks.
For a model-free rule, `model_sha256` hashes the rule artifact, with that
choice recorded in the private run manifest. `config_sha256` pins thresholds
and settings as well as the model/build identity.

`build_sha` is a 40-hex Git revision, never a binary SHA-256. Optional
`producer_binary_sha256` and `source_patch_sha256` retain exact producer
provenance in the run/report. An uncommitted build supplies its base revision,
actual executable hash and complete patch hash (including untracked source);
a patch hash requires the binary hash. See the adapter examples for private
source retention. These fields are distinct from the scorer's own binary hash.

For `evidence: "replay"`, `ready_us` must be null or absent. Never manufacture
capture-clock readiness by adding a replay host duration to a recorded ASR
timestamp. The optional `replay_elapsed_us` is the measured host duration
from invoking the rule/worker to obtaining its result; it is reported only
under that name, not used for any lamp latency gate. Keep recorded
`input_end_us`/`asr_final_us` in their original capture clock. Replay forbids
`associated_us`, `face_in_frame_us` and `first_admission_us`; original live
observations belong in a separate live run. Live/synthetic predictions
require `ready_us` and forbid `replay_elapsed_us`.

`raw_classification` counts label agreement independently of measured
readiness, including missing predictions in its denominator. It does not
check physical motion or waking behaviour and cannot qualify a sensor.
The ordinary `counts`, checks and confidence interval keep their complete
timing/behaviour requirements; replay has no timely observations and never
qualifies. Per-row `raw_output` and `classification_correct` retain the
offline classification for diagnosis without presenting it as a live pass.

### Sensor-specific fields

| Sensor | Truth/output values | Additional prediction fields |
|---|---|---|
| `call` | `call`, `non_call` | Observed `wake_count` on every non-call exposure |
| `addressed` | `lamp`, `room` | `asr_final_us` equal to `input_end_us`; report ready minus ASR separately from speech-end |
| `direction` | `turn`, `no_turn` | `bearing_deg`, observer-labelled `framed_speaker_id`, `face_in_frame_us`, `turns`, `natural_motion` |
| `head` | `lamp`, `screen`, `phone`, `unknown` | Actual `subject_face_id`, `eyes_epoch`, optional `reason`; case has `opportunity_us` |
| `speaking_face` | Visible temporary track ID, `no_visible`, `unknown` | `eyes_epoch`, `frame_work_us`, `associated_us`; source `input_end_us` is the last required frame |
| `voice_match` | `same`, `different` | Case has `reference_trial_id`; source `input_end_us` is clip submission |

For voice match the 500 ms worker duration is `ready_us - input_end_us`;
`trial.end_us` remains the acoustic line end. Optional `first_admission_us`
records the actual first admission on the capture clock. The report keeps
`ready_before_first_admission` (null when unmeasured) separate from
`within_admission_budget` (ready by line end +100 ms). A print arriving after
actual admission cannot justify that admission, even if within the budget.

Use a new eyes epoch after a worker/camera restart. A known track from the
old epoch does not become the current person's evidence. The `subject_*`
fields in the manifest identify the annotated person, not whichever face
the detector considers largest. Keep expected no-face distinct from a missed
visible face. Detector failures belong in predictions, never in eligibility.

`frame_work_us` is the measured combined mouth work for the sampled frame;
`ready_us` is when that evidence was published. `associated_us` is the
cached line-close association time, not a second inference. Do not invent
one average frame duration for every trial. The case references its single
declared decision window; use separate raw per-frame diagnostics privately.

Private model-input sidecars can carry ASR words, last eligible person/lamp
exchange and script wording, keyed by trial ID. They are not scorer inputs
and are not copied into these reports. The timestamp/recipient/label contract
above is shared by every adapter; `src/schema.rs` is the exact executable
schema. Unknown fields are rejected, so keep private text out of that file.

## Voice comparison input

This v1 comparator preserves the earlier #8673 case labels for historical
comparisons and tool tests. `passed: true` means its numeric checks passed;
it does not approve the new voice baseline, a switch-on, merge or train.
Review and freeze the reset policy's cases, kit/scorer revision and rendered
assets before using a comparison as evidence for adding a sensor. Keep v1's
labels unchanged so older results remain reproducible.

See the exact typed contract in [src/voice.rs](src/voice.rs). Top-level fields:

```text
version: 1
baseline_build_sha, candidate_build_sha: 40-digit commit hashes (may match for an off/on test of the stable binary)
changed_switch: one LAMP_* name
kit_sha256: source + actual rendered-audio/noise manifest hash
kit_reviewed: true only after the two reads and asset verification
scene_manifest_sha256: frozen planned scenes, including the live prompts
other_config_sha256: all settings except that one switch
cases: frozen played-cases-v1.json plus the fixed live cases
runs: A1/B1/B2/A2 for each of played and live
```

Each case has `id`, `group`, `expected` (A/Q/S), `kind`
(`call`/`followup`/`room`/`stop`) and `mode` (`played`/`live`). Each run has
`leg`, `mode`, `build_sha`, `other_config_sha256`, `kit_sha256`,
`scene_manifest_sha256`, `sensor_enabled`, `safety_violation`, and `lines`.
Each line has `id`, `observed` (A/Q/S/missing), `heard`, `speech_end_us`,
`first_audio_us`, `interrupt_onset_us`, `last_audio_us`.

A/Q/S means audible answer, left alone, or stopped respectively. A needs
an actual first audible sample; S needs interruption-onset and last-output
markers. A Q with nothing heard is not a pass. Missing rows are counted,
never trimmed by zipping two lists. Only scored A/Q/S lines appear in
`cases`; the pinned kit still plays its unscored setup/restoration lines.

The frozen played cases are exact, not optional examples. A partial smoke
run is useful but cannot pass the full comparison. Live uses at least ten
prompts of each kind; attach the same stable prompt IDs to repeated legs.
The comparator reports per-group planned/native counts, missing/unheard
counts and exact-boundary p50/p95 times. It does not read raw WAVs, infer
subject identity or infer safe physical motion from a classifier's confidence.

## Tool tests versus lamp evidence

The synthetic fixtures exercise pass/fail logic and CLI behavior. They do
not count toward any sensor's 95%/98%, the six real speakers, or the A/B/B/A
device evidence. `lamp_live` is an assertion checked by the lead's evidence
review, not something this offline program can independently authenticate.
