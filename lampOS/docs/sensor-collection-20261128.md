# Lamp-channel collection, 28 November 2026

This registers two recordings of the same labelled source clips for
[#8850](https://github.com/autonomous-ai/lampOS/issues/8850). It supplements
the [sensor evaluation protocol](sensor-evaluation.md); the v2 bars, frozen
scorer and real-person qualification requirements are unchanged. The morning
table retains its 06:56 cutoff. Later evidence has its own timestamp.

## Recording register

The team lead holds the original audio, `index.tsv`, `CORRECTION.md` and
`lamp-speech-session-1.tsv` locally. Keep every attempt and correction, with
its original bytes, SHA-256 inventory and README. No audio or private text
belongs in this repository.

| Attempt | Holder-reported collection | Intended use | Verification status |
|---|---|---|---|
| `recording/` | 3,200 clips; lamp speech over 1,758 | Calls over lamp speech, interruption development and self-wake diagnostics | Index, correction, overlap intervals and seal requested; not independently checked |
| `recording-r1/` | The same 3,200 source clips, recorded again clean | Matched clean comparator | Seal expected around 14:20 GMT+7; completion and absence of lamp speech not yet verified |

The remaining 1,442 first-attempt clips are a derived remainder, not verified
clean recordings. A coarse clip overlap tag does not establish that lamp
speech overlaps the relevant stimulus or its full decision window.

## Admission and pairing

Before scoring, retain and verify the exact index, correction, overlap TSV,
each recording inventory, README and seal. Record every missing, extra,
duplicate or corrupt file; do not shrink the planned denominator to files
that produced predictions. Bind each captured file to the original index
row and original rendered/source-audio hash. Pair the two attempts by that
source key, never by folder order or a new transcript match.

Report 3,200 planned pairs separately from the number actually verified.
Two captures of one stimulus are correlated observations, not 6,400 new
utterances or independent speakers. Keep each original source/voice/template
family and train/development/test role together. Preserve the original
label and every correction; record label disagreements before predictions.
Re-recording a generated voice on the lamp does not make it a real-person
holdout. These sets enter development accounting; source origin and lineage
must be verified before generating a v2 manifest.

The local collection sidecar binds the two attempt IDs, source keys,
recording hashes, source split and overlap intervals to their clock/epoch.
It is separate from the strict scorer JSON: do not add unrecognized fields
to v2. Missing clocks, distance or consent stay unknown under the
[external-development adapter](../tools/sensors/adapter-v2.md); do not invent
measured geometry or a zero clock error to satisfy a schema.

## Three distinct measurements

1. **Call detection with lamp speech.** Keep the original call/non-call gold.
   Report each class for both attempts and for the predeclared overlap and
   non-overlap slices. For verified pairs, report correct in both, gained,
   lost and wrong/unknown in both, with missing captures/predictions retained.
   The standing call bar is at least 98% call recall and 95% non-call
   rejection, at least 120 of each, plus the existing latency/holdout gates.
2. **Self-wake.** Use independently labelled lamp-only, no-external-call
   exposure. A call intentionally played over the lamp is not a self-wake
   negative. A false activation during mixed non-call audio does not, by
   itself, identify the lamp's own speech as the cause. Retain the exposure
   duration, activation count and exact observation boundary. Report offline
   detector activations separately from observed product wakes. Any additional
   windows keep their parent source identity and are not pooled as new
   independent trials.
3. **Interruption.** The 1,758 overlap tags are not 1,758 successful or even
   fully labelled interruptions. Independently label `interrupt`,
   `backchannel`, `side_talk` and `noise`; record each actual acoustic onset,
   planned lamp playback interval and observation horizon. Only rows with
   those facts can enter the corresponding v2 barge manifest. Missing labels
   remain an explicit coverage gap. The bar stays at least 114/120 in each
   class; true interruption requires actual early audible stop within
   400 ms including clock error. Negative cases require no stop/restart
   through the full planned playback and declared horizon.

An offline replay cannot stop the original recorded playback. Keep
`ready_us` null and host replay duration separate; never populate actual
stop/readiness fields from host timing or natural recording completion.
Measured lamp-clock mapping, first use, slow dependencies and complete
physical stop observations remain required for a later live qualification.
Use the existing [call](../tools/sensors/score-call.sh) and
[barge-in](../tools/sensors/score-barge-in.sh) wrappers only with valid
manifests and actual producer outputs. An inventory or empty prediction run
is not a measured sensor result.

Freeze the candidate source/binary/full patch, model, configuration, labels
and scoring inputs before comparison. A development result from either set
cannot replace the shared real-person collection or the separate whole-bench
A1/B1/B2/A2 gate. Keep the sensor standalone/default-off; no merge or train.
