# Adapter input examples

These are invented fixtures, not recordings or qualification evidence. The
same manifest defines both addressed and voice-match cases. Use the exact
manifest bytes with `run.sh validate`, then replace `HASH_FROM_VALIDATE`
in each run/sidecar with the printed hash. All times below are microseconds
unless the field ends in `unix_s`.

## Shared manifest

```json
{
  "version": 1,
  "collection_id": "adapter-example-only",
  "frozen_unix_s": 1000,
  "sessions": [
    {
      "id": "s01",
      "split": "holdout",
      "started_unix_s": 2000,
      "consent": true,
      "delete_after_unix_s": 100000,
      "clock_id": "capture-boot01",
      "sync_error_us": 1000,
      "recording_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    },
    {
      "id": "s02",
      "split": "holdout",
      "started_unix_s": 10000,
      "consent": true,
      "delete_after_unix_s": 100000,
      "clock_id": "capture-boot02",
      "sync_error_us": 1000,
      "recording_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    }
  ],
  "trials": [
    {
      "id": "c001",
      "session_id": "s01",
      "speaker_id": "p01",
      "source": "person",
      "start_us": 1000000,
      "end_us": 2000000,
      "call_end_us": 1300000,
      "last_lamp_audio_end_us": null,
      "recipient": "lamp",
      "script_id": "c001",
      "look_target": "unknown",
      "head_target": "unknown",
      "bearing_deg": 0,
      "distance_mm": 600,
      "light": "daylight",
      "noise": "quiet",
      "visible_faces": [],
      "subject_face_id": null,
      "subject_visible": false,
      "eyes_epoch": null,
      "overlap": false,
      "first_use": false,
      "condition": "normal"
    },
    {
      "id": "f001",
      "session_id": "s02",
      "speaker_id": "p01",
      "source": "person",
      "start_us": 1000000,
      "end_us": 2000000,
      "call_end_us": null,
      "last_lamp_audio_end_us": 900000,
      "recipient": "lamp",
      "script_id": "f001",
      "look_target": "unknown",
      "head_target": "unknown",
      "bearing_deg": 0,
      "distance_mm": 600,
      "light": "daylight",
      "noise": "quiet",
      "visible_faces": [],
      "subject_face_id": null,
      "subject_visible": false,
      "eyes_epoch": null,
      "overlap": false,
      "first_use": false,
      "condition": "normal"
    },
    {
      "id": "v001",
      "session_id": "s02",
      "speaker_id": "p01",
      "source": "person",
      "start_us": 3000000,
      "end_us": 4000000,
      "call_end_us": null,
      "last_lamp_audio_end_us": null,
      "recipient": "lamp",
      "script_id": "v001",
      "look_target": "unknown",
      "head_target": "unknown",
      "bearing_deg": 0,
      "distance_mm": 600,
      "light": "daylight",
      "noise": "quiet",
      "visible_faces": [],
      "subject_face_id": null,
      "subject_visible": false,
      "eyes_epoch": null,
      "overlap": false,
      "first_use": false,
      "condition": "normal"
    }
  ],
  "cases": [
    {
      "id": "addressed-f001",
      "sensor": "addressed",
      "trial_id": "f001",
      "reference_trial_id": null,
      "expected": "lamp",
      "fault": false,
      "opportunity_us": null
    },
    {
      "id": "voice-v001",
      "sensor": "voice_match",
      "trial_id": "v001",
      "reference_trial_id": "c001",
      "expected": "same",
      "fault": false,
      "opportunity_us": null
    }
  ]
}
```

## Addressed prediction and run

```json
{
  "version": 1,
  "manifest_sha256": "HASH_FROM_VALIDATE",
  "sensor": "addressed",
  "build_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "model_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
  "config_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
  "evidence": "synthetic",
  "location": "local",
  "predictions": [
    {
      "case_id": "addressed-f001",
      "clock_id": "capture-boot02",
      "output": "lamp",
      "input_end_us": 2100000,
      "asr_final_us": 2100000,
      "ready_us": 2140000,
      "added_wait_us": 0,
      "held_line": false,
      "safety_violation": false
    }
  ]
}
```

The final-ASR-to-result duration is 40 ms; speech-end-to-result is 140 ms.
Only the former is checked against the 50 ms incremental budget. The
synthetic flag is mandatory for these invented numbers.

## Voice-match prediction and run

```json
{
  "version": 1,
  "manifest_sha256": "HASH_FROM_VALIDATE",
  "sensor": "voice_match",
  "build_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "model_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
  "config_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
  "evidence": "synthetic",
  "location": "local",
  "predictions": [
    {
      "case_id": "voice-v001",
      "clock_id": "capture-boot02",
      "output": "same",
      "input_end_us": 4010000,
      "ready_us": 4080000,
      "first_admission_us": 4050000,
      "added_wait_us": 0,
      "held_line": false,
      "safety_violation": false
    }
  ]
}
```

The worker duration is `ready_us - input_end_us` = 70 ms; `input_end_us`
means actual clip submission. The acoustic end is the manifest's
`trial.end_us`. Readiness is 80 ms after that end, inside the 100 ms
budget, but 30 ms after actual first admission: the report returns
`within_admission_budget: true` and `ready_before_first_admission: false`.
That print cannot be used to justify first admission; it can only be
considered for renewal under the runtime's validity/cancellation checks.
If admission was not observed, omit `first_admission_us`; the comparison
is null, never assumed true.

## Offline addressed replay

```json
{
  "version": 1,
  "manifest_sha256": "HASH_FROM_VALIDATE",
  "sensor": "addressed",
  "build_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "model_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
  "config_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
  "evidence": "replay",
  "location": "local",
  "predictions": [
    {
      "case_id": "addressed-f001",
      "clock_id": "capture-boot02",
      "output": "lamp",
      "input_end_us": 2100000,
      "asr_final_us": 2100000,
      "ready_us": null,
      "added_wait_us": 0,
      "held_line": false,
      "safety_violation": false,
      "replay_elapsed_us": 40000
    }
  ]
}
```

Keep the recorded ASR/input timestamp. Measure `replay_elapsed_us` using
one host monotonic clock around the rule/worker invocation. Do not add it to
the recorded time. `ready_us` must be null/absent for every replay row.
The report retains the output and raw label agreement but has no lamp
readiness or ASR-to-result latency; it cannot qualify. Original live
readiness logs belong in a separate `lamp_live` run for the model/build
that actually produced them.

## Private adapter sidecar

This is an adapter convention, not a file accepted by the scorer. Private
words and audio paths stay under the lead's
`~/lamp-research/sensor-sessions/`, with the same session consent and
deletion dates. Never put actual sidecar text, audio or personal paths in
GitHub or reports. The following words and paths are invented.

```json
{
  "version": 1,
  "manifest_sha256": "HASH_FROM_VALIDATE",
  "entries": [
    {
      "case_id": "addressed-f001",
      "trial_id": "f001",
      "reference_trial_id": null,
      "clock_id": "capture-boot02",
      "worker_epoch": "addressed-process01",
      "asr_final_us": 2100000,
      "request_id": "request01",
      "input": {
        "asr_text": "And tomorrow?",
        "last_exchange": {
          "person_text": "What is the weather today?",
          "lamp_text": "It is dry today."
        },
        "audio_relpath": null,
        "reference_audio_relpath": null
      }
    },
    {
      "case_id": "voice-v001",
      "trial_id": "v001",
      "reference_trial_id": "c001",
      "clock_id": "capture-boot02",
      "worker_epoch": "print-process01",
      "asr_final_us": null,
      "request_id": "request01",
      "input": {
        "asr_text": null,
        "last_exchange": null,
        "audio_relpath": "s02/v001.wav",
        "reference_audio_relpath": "s01/c001.wav"
      }
    }
  ]
}
```

Join each entry to exactly one manifest case by `case_id`; verify its
`trial_id`, `reference_trial_id` and capture `clock_id` agree with that
case. Validate the exact manifest hash, reject duplicate/unknown cases and
duplicate `(worker_epoch, request_id)` pairs. The process epoch changes on
restart; request IDs alone can repeat. A sidecar may cover a subset of
cases for one adapter; absent predictions remain missing, not negatives.

An addressed entry's `asr_final_us` is outside `input`, on its recorded
`clock_id`. If it is unknown, store null and emit no prediction for that
case; keep the case in the manifest and retain the omission reason in the
private adapter report. Never use acoustic end or host invocation time as
a substitute. The only accepted split names are `development` and
`holdout`; canonical qualification scoring includes only holdout cases.

`build_sha` accepts exactly 40 hexadecimal characters for the Git revision,
not an executable hash. Optional run/report fields `producer_binary_sha256`
and `source_patch_sha256` accept 64 hexadecimal characters. For uncommitted
source use the base Git revision plus both fields: hash the actual producer
executable and a complete patch including all changed/untracked source used
to build it. A patch hash without the binary hash is rejected. Keep the
patch/source archive in the private run bundle for review. These identities
do not change the run's evidence type or establish its provenance by
themselves. `scorer_binary_sha256` identifies the separate scorer executable.

Only read relative audio paths within the consented session root; refuse
absolute paths, parent traversal and symlinks escaping that root.

Feed only `input` to the classifier, not manifest truth or holdout labels.
For addressed speech, `last_exchange` is the last eligible person/lamp
exchange selected by the frozen rule, or null; do not infer it from the
gold recipient label. Keep development/adaptation inputs separate from
sealed holdout inputs. For voice matching, reference audio belongs to the
manifest reference trial; its session can have a different capture clock.
Do not subtract times across those sessions.

The scorer validates its manifest/run schemas; the adapter owns sidecar
validation and private I/O. `src/schema.rs` is the executable public schema.
