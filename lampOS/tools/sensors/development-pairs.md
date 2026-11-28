# Raw development voice pairs

This v2 mode scores known-identity digital voice experiments, including a
probe deliberately compared with its caller and another reference. Its unit
is an **ordered reference/probe source-segment pair**, not an independent
episode or real-person holdout. It always reports **NOT QUALIFIED**, even
with perfect raw classification. The ordinary physical rubric is unchanged.

Use the same `score-voice-match.sh MANIFEST RUN NEW_OUT` command. Add this
explicit manifest field:

```json
"evaluation_mode": "raw_development_pairs"
```

The entire manifest must be version 2, rubric `sensor-team-2026-11-27-v2`,
with only `development` sessions of origin `public`, `generated` or
`invented`, and only primary (`fault:false`) `voice_match` cases. Run evidence
must be `replay`. Mixing holdout, real-person collection sessions, another
sensor, fault exclusions or live readiness is rejected. Keep physical and
raw-pair reports separate, with their actual source/input/executable hashes.
Here `development` means evidence outside the physical qualification collection.
It does not authorize training on a provider/public test set: preserve its
original train/validation/test partition and freeze in the private README.

## Encode the inputs once

1. Each trial identifies a source waveform segment exactly once. Its session's
   `recording_sha256` is the SHA-256 of the actual waveform whose offsets
   the trial uses, **not the corpus inventory hash**. Keep inventory and README
   hashes in their existing separate fields. Independent clip files can each
   use a session/clock ID and offsets starting at zero. These are file time
   domains, not proof of independent physical recording sessions.
2. `start_us` and `end_us` are measured offsets in those source bytes. The
   original acquisition `started_unix_s` may be null when unavailable; do not
   substitute download, adaptation or scoring time. Unknown consent, geometry,
   alignment and environment use the limited external-development encoding
   in [adapter-v2.md](adapter-v2.md). Keep real human/public speech local.
3. Assign known anonymous speaker identities consistently and namespace them
   across corpora/providers. A generated voice identity is not a real holdout
   participant. Reference speech need not have said “Hey Lamp” or addressed
   the lamp: leave `call_end_us:null` and `recipient:"unknown"` when true.
   This exception is only for raw-pair mode, not physical admission references.
4. Give each ordered comparison a unique case ID, its shared probe trial ID,
   reference trial ID and known `same`/`different` label. Reuse the probe ID;
   do not clone audio/epochs to manufacture new independent episodes.

For example, two cases may use the same probe:

```json
[
  {
    "id": "pair-same", "sensor": "voice_match",
    "trial_id": "probe01", "reference_trial_id": "reference01",
    "expected": "same", "fault": false
  },
  {
    "id": "pair-different", "sensor": "voice_match",
    "trial_id": "probe01", "reference_trial_id": "reference02",
    "expected": "different", "fault": false
  }
]
```

The corresponding `reference01` and `probe01` speaker IDs must agree, while
`reference02` differs. Each prediction uses its case ID and the probe's
source clock. In this mode `input_end_us` equals the probe's `end_us`: it is
the last recorded sample offset, **not a measured live submission time**.
Keep `ready_us:null`, and no live association/admission/framing/playback
timestamps. Optional `replay_elapsed_us` is a separate measured host duration
around enrollment/classification. Retain its boundary and exclusions in the
source README; do not add it to a file timestamp.

## What the scorer checks and reports

Source identity is canonical lowercase recording hash plus start/end offsets.
The same ordered source pair cannot appear twice under different IDs or hash
letter case. Aliases of one segment must keep speaker/source/origin labels
consistent. Reference and probe segments from the same recording may not
overlap: self-comparisons or shared audio samples cannot inflate accuracy.
Disjoint segments from one recording remain correlated development evidence.
Role-reversed comparisons are distinct ordered pairs, still correlated.

Every planned pair remains in the raw denominator; missing and unknown outputs
fail. `raw_by_split.development` contains class/confusion counts and missing
metadata. The top-level `development_pairs` reports planned pairs, unique
probe/reference/input segments, unique recording hashes, reused probes,
additional pairs on reused probes, maximum pairs per probe and unknown
original session starts. Per-row JSON/TSV preserves `reference_trial_id`.
`qualification_evaluation_mode` explicitly fails; holdout counts stay zero.

These unique counts describe **processed input bytes**, not independent original
utterances. Quiet/noisy crops or other augmentations can have distinct hashes
while sharing original speech. Retain that lineage and original-versus-derived
counts in the README and EVAL data limits. Pair-level accuracy is descriptive;
do not present a confidence interval assuming independent pairs or claim six
physical speakers from generated/provider identities.

Without this mode, the normal v2 rules still require one voice case per probe,
eligible call references and at least two hours between same-person reference
and probe sessions. V1 remains unchanged. Unknown original times are accepted
for external v2 development generally, but cannot satisfy a spacing rule.
