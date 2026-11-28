# Lamp-channel collection, 28 November 2026

This registers two recordings of the same labelled source clips for
[#8850](https://github.com/autonomous-ai/lampOS/issues/8850). It supplements
the [sensor evaluation protocol](sensor-evaluation.md); the v2 bars, frozen
scorer and real-person qualification requirements are unchanged. The morning
table retains its 06:56 cutoff. Later evidence has its own timestamp.

## Recording register

The team lead holds the originals on Mac 2 under
`~/lamp-voice-data/heylamp-8907/lamp-channel-1128/`. Keep every attempt,
correction, failed seal and output, with its original bytes, SHA-256 inventory
and README. No audio or private text belongs in this repository.

The [metadata handoff](https://github.com/autonomous-ai/lampOS/issues/8850#issuecomment-7235897159)
is frozen at
[`c625cb08f960435195b626a4030e76382cd1337e`](https://github.com/autonomous-ai/lampOS/tree/c625cb08f960435195b626a4030e76382cd1337e).
The Reviewer verified all **13 files** in its `SHA256SUMS.meta`. Both
captures are continuous `mic2_audio.jsonl` bus recordings (20 ms PCM16,
16 kHz mono frames), not 3,200 separately verified recorded audio files.

| Attempt | Registered collection | Intended use | Verification status |
|---|---|---|---|
| `recording/` (`session-1/` in the handoff) | 3,200 planned source slots; lamp output occurred during capture | Calls over lamp output, interruption development and self-wake diagnostics | Index, 32 playback blocks, correction, 1,509 sound intervals and both seal inventories checked; overlap membership unresolved below |
| `recording-r1/` (`r1/` in the handoff) | The same 3,200 slots; 11:27:31–14:18:00 GMT+7 | Matched clean comparator | All 32 blocks report complete on try 1; sound TSV has zero rows; holder reports no outputs/watchdog events after repairing the journal seal |

The r1 seal originally failed to fetch its journal because the start time was
parsed as the word `recording`. The holder retained that failed 20-byte gzip
and replaced the seal input with the correct 4,300-line window. Its README
records the repair. Empty intervals are therefore checked against the
**repaired metadata**, not proof from the failed empty export. The original
journal, watchdog and captured audio have not been read on this host; the
absence of acoustic lamp output and frame continuity are not independently
verified here.

| Bound item | SHA-256 |
|---|---|
| Metadata inventory | `a9c555316c104dcc10fd77a64241f3fe08ee62200bedf9ffed2b2d53a3913149` |
| `index-meta.tsv` | `7db13e50e451dc511a2b2b558ad64c0a142f8b059fbd332ee43f74a21828a841` |
| Initial `mic2_audio.jsonl` (both seals agree) | `5f159cb262968055ed7adce274a5d44cbf61a0f3fd2a54e4749a5a64b6b6f73a` |
| r1 `mic2_audio.jsonl` (both seals agree) | `fd7fc00bf75526a22f6222912921eb504eafac34d885112bb5fe59a555665894` |

The last two hashes are **verified agreement between recorded inventories**,
not a fresh hash of the inaccessible raw bytes. Every original file's hash
remains in the sealed inventories; these four anchors do not replace them.

## Source labels and overlap limits

The shared index has 3,200 distinct opaque IDs, 32 blocks of 100, and
**1,600 `call` / 1,600 `not-call`** labels. Source use is 2,400 train
(1,200/class), 600 test (300/class), and 200 test-extra (100/class). Source
`split=test` includes both test and test-extra; retain both columns rather
than silently pooling them. Map `not-call` to the scorer's `non_call`
explicitly when constructing an otherwise valid call manifest.

All source voices are generated: ElevenLabs 1,701 clips, Gemini 1,299 and
OpenAI 200, with 83 distinct provider/voice IDs across nine styles. None of
those IDs crosses the declared train/test split. This does not verify text
or template independence, model training exposure, or six real speakers.
Even blocks use playback gain 1.0 and odd blocks 0.5 (1,600 clips each);
these are different source clips, not every clip measured at both gains,
and the gains are not calibrated sound pressure levels.

**CORRECTION.md supersedes the initial README's claim of no lamp answers.**
Its 1,758 count means overlap with **any lamp sound**, not speech alone.
It reports 591 speech, 1,469 wait and 313 ring overlaps; these categories can
overlap and must not be added. The supplied interval table contains 331
speech intervals reconstructed from handed audio, 1,160 estimated wait
intervals and 18 ring upper bounds. The speaker level changed during the
session and was muted at 10:12:35.863. These are not measured acoustic
start/stop receipts. The reported 1,442 with no interval overlap are not
automatically verified quiet recordings.

The published clip counts do not reproduce from the supplied nominal
timestamps. Using half-open intervals from `mac_wall_ns_before + offset_s`
through that start plus `duration_s`, with no startup adjustment, gives
**1,767 any / 582 speech / 1,354 wait / 348 ring / 1,433 none**. Adding
0.1 or 0.2 seconds (the README's approximate afplay startup) does not
reconcile all counts. Preserve these as a sensitivity check, not corrected
labels. The exact helper, per-clip mask and any captured/chirp alignment
used by the producer are requested before accepting the slices as frozen
gold. Every planned row remains present in the provisional local report.

Lamp pre-play monotonic ns, Mac pre-play wall ns and lamp journal wall ms
are different clock domains. NTP plus a nominal offset does not establish
the v2 maximum 5 ms error; startup, drift and actual captured boundaries
remain unverified. Keep `clock_verified=false` and clock error unknown.
Do not turn an alignment fit residual into a complete clock-error bound.

## Admission and pairing

Before scoring, retain the bound metadata above and verify the actual
captured frames, source-audio content hashes, timing and overlap labels.
Record every missing, extra, duplicate or corrupt input; do not shrink the
planned denominator to rows that produced predictions. The metadata pairs
`(block, clip_in_block, clip_id)` across both attempts. `clip_id` is the first
16 hex characters of SHA-256 of the **source path**, not audio content.
Bind each captured segment to that row and its original rendered-audio
hash; neither folder order nor a transcript match substitutes for this.

Report 3,200 index pairs covered by both playback logs separately from the
number of captured segments independently verified; none was verified here.
Two captures of one stimulus are correlated observations, not 6,400 new
utterances or independent speakers. Keep each original source/voice/template
family and train/development/test role together. Preserve the original
label and every correction; record label disagreements before predictions.
Re-recording a generated voice on the lamp does not make it a real-person
holdout. These sets enter development accounting. Preserve all source roles
and report each separately; do not count train rows as held-out evaluation.

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
3. **Interruption.** The reported 1,758 overlap tags are not 1,758 successful or even
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
