# V2 adapter contract

Use `version: 2` in both manifest and run, and
`rubric_id: "sensor-team-2026-11-27-v2"` in the manifest. The executable schema
is [schema.rs](src/schema.rs); all unknown JSON fields are rejected. The
[protocol](../../docs/sensor-evaluation.md) owns denominators and boundaries.
V1 still scores old inputs without reinterpreting them as the new jobs.

## Minimal runnable development example

[example-head-v2-manifest.json](example-head-v2-manifest.json) and
[example-head-v2-run.json](example-head-v2-run.json) are invented numeric
data, with dummy provenance hashes and no raw recording. The run pins the
exact bytes of its example manifest. It demonstrates one correct raw replay
classification, **not a holdout or latency pass**:

```sh
tools/sensors/run.sh validate ABSOLUTE_PATH/example-head-v2-manifest.json
tools/sensors/score-head.sh ABSOLUTE_PATH/example-head-v2-manifest.json ABSOLUTE_PATH/example-head-v2-run.json NEW_ABSOLUTE_OUTPUT
```

The second command exits 1 and still writes a useful report. V2 rows include
development and holdout, with `split`, `data_origin` and `applicable` in TSV.
`raw_by_split` separates raw counts by class and origin; top-level `counts`
and `raw_classification` remain primary **holdout-only**. A missing v2 live
`ready_us` is a counted failure, not a fabricated timestamp.

## V1 to V2 fields

| Object | Current contract |
|---|---|
| Session | Remove `delete_after_unix_s`; add `retention:"keep_all"`, exact `inventory_sha256`, `readme_sha256`, and `data_origin`. Keep consent, recording hash, split, source clock and measured error. `clock_verified:true` and error <=5000 us are needed to qualify. |
| Data origin | `real_people`, `generated`, `public`, `invented`; origin is distinct from run evidence/location. Generated/public data never establish the team's real-person holdout. |
| Head trial | `head_target` and `shoulders_target` each lamp/away/unknown; keep separate `look_target`, actual subject track and eyes epoch. |
| Head case | `expected:facing` only when both known toward lamp; otherwise known away. Optional `head_gold_adjudicated:true` permits independent gold when a component is unknown. Without that gold use expected unknown + fault diagnostic. |
| Speaking face | Visible temporary track, `no_visible`, or `unknown`; applicability is >=2 visible faces. No upper two-face restriction in v2; report actual face-count slices and limits. |
| Barge trial | `overlap:true`, `lamp_audio_start_us` before acoustic `start_us`, `planned_lamp_audio_end_us` after it. Source may also be `noise`. |
| Barge case | Sensor `barge_in`; expected `interrupt`, `backchannel`, `side_talk` or `noise`; `observation_end_us` reaches both the full planned playback and stimulus end. |
| Barge prediction | Output `stop`, `continue` or `unknown`; actual `last_lamp_audio_us`, `observed_until_us`, `playback_stopped`. Interrupt success requires actual early stop <=400000 us from acoustic trial start, including clock error. Natural completion is not a stop. Negative success requires full playback and observation with no stop. |
| Run provenance | 40-hex `build_sha` names source revision/base. SHA-256 `producer_binary_sha256` is required to qualify v2. `source_patch_sha256` identifies the complete uncommitted source, including untracked source; omit only for the exact committed source. Model/config hashes are separate. |

### Unknown public or generated development metadata

Public benchmarks do not imply measured desk distance or collection-specific
consent. For **v2 development sessions only**, with origin `public`,
`generated` or `invented`, these unmeasured fields may honestly be null:
session `consent`, original `started_unix_s` and `sync_error_us`, trial
`distance_mm` and `bearing_deg`.
Use `light:"unknown"` and `noise:"unknown"` when unlabelled. Keep
`clock_verified:false` if no measured mapping exists. A zero distance is
invalid; never fill unknowns with a fabricated 600 mm or consent assertion.

The private source README records the actual dataset/access basis and limits;
null consent makes no claim of collection consent or permission to reuse data.
Real-person sessions and every holdout still require affirmative consent and
known collection fields. Unknown clock error cannot coexist with a verified
clock or produce a numeric latency upper bound. V1 remains strict.

These inputs report raw development classification and `metadata_unknown`
counts by field, including rows outside the primary task. They cannot fill
any holdout distance/light cell or qualify a sensor. Missing/unknown predicted
speaker labels still fail raw accuracy on every applicable primary line.
Retain all known label provenance and keep replay readiness null. Earlier
scorer binaries may reject this development-only extension; pin the updated
source and actual executable in the EVAL rather than changing the data.

For repeated voice reference/probe comparisons without observed physical
session spacing, use the explicit [raw development pair mode](development-pairs.md).
It reports correlated pairs and source reuse; it never qualifies a sensor.

Keep original source, complete patch, binary, model and configuration privately
available for hash verification. A patch hash does not replace the base Git
revision or binary hash. A source READ-OK does not establish measurement quality.

All seven sensors retain `input_end_us`, `ready_us`, `added_wait_us`,
`held_line` and `safety_violation`. Replay has null readiness and may supply
host `replay_elapsed_us`; it forbids live association, framing, admission and
playback outcome fields. Do not add host duration to recorded ASR/capture time.
Use the [historical adapter examples](adapter-examples.md) for the unchanged
voice/addressed timestamp meanings and private trial-ID joins, with v2
session/rubric fields above. The 30 ms late voice arrival remains late for
actual admission even if within the separate line-end+100 ms budget.

Primary unknowns/missing/late results fail. `safe_abstentions` counts safe fault
behavior separately. Raw classification does not check physical stop/motion,
clock readiness or qualification. Timings retain raw values and mapped-clock
upper-bound p50/p95; host frame work remains its actual elapsed duration.

## Whole comparison

[whole.rs](src/whole.rs) is a separate strict v2 input. `purpose` is
`baseline_characterization` (C/D) or `one_sensor_addition` (A1/B1/B2/A2).
Its `rubric` field uses the same v2 identifier. Cases are an ordered typed
array. Get **canonical typed** `cases_sha256` with:

```sh
tools/sensors/run.sh whole-cases-hash ABSOLUTE_CASE_ARRAY_JSON
```

This is not the raw JSON file hash: whitespace is discarded and every typed
field, including nulls, consent and origin, is serialized in schema order.
Keep the exact input file hash separately in the local inventory. Case order
is preserved and must be frozen before predictions.

Pin baseline/candidate source, binary, full patch when needed, config and
model-manifest hashes; unchanged settings, rendered kit, collection, scorer
source/binary and case-vector hashes are separate. Each run pins its own
identity, distinct raw inventory and measured source clock. Never reuse an
absolute timestamp from a different leg's new capture.

For one addition, `accepted_sensors` plus exactly `added_sensor` defines B;
A contains the accepted set. `combined_judge:true` is required on all legs.
`added_component_model_sha256` and `added_component_config_sha256` match the
prerequisite's model/config hashes, which match the actual standalone report.
The CLI hashes that report, **re-scores its original manifest and run**, and
requires exact report equality using the current frozen scorer binary.
An asserted `qualified:true` is insufficient. The lead still verifies that
these hashes identify the component actually running in the whole build.
If a standalone report was produced with another platform's executable,
re-score its original inputs on the comparison host with the frozen scorer
and retain both reports; reference the newly verified report's exact hash.

C/D uses no added component or prerequisite and passes `- - -` in place of
the three sensor input files. It reports gaps and numeric observations but
cannot return a promotion pass. Full whole input, output and per-person rows
stay local; publish aggregates using the EVAL template.
