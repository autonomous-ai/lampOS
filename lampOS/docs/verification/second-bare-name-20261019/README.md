# Bare "Lamp": supplemental synthetic evidence, 19 October 2026

This [#694](https://github.com/autonomous-ai/lampOS/issues/694) measurement used **new synthetic direct PCM**. The referenced played/acoustic recordings and onset manifest were unavailable. No speaker playback, microphone capture, speech-recognition service, or lamp was used. This evidence does not establish an acoustic/STT result, false-alarm rate, or latency result.

Three installed English TTS configurations, anonymized as **A/B/C**, each generated four positive forms and eight controls. Four leading-silence variants repeat those same 36 utterances; they are not 144 independent examples. All 144 offline scorer processes exited successfully. The public primary model was used; neither the unavailable local rescue model nor the separate #725 candidate was evaluated.

## Observed scores

The checker emits `heard` events at its fixed 0.9 threshold:

| Added leading silence | Positive lines with an event | Controls with an event |
|---|---:|---:|
| 0 s | 0/12 | 0/24 |
| 0.5 s | 0/12 | 0/24 |
| 1 s | 2/12 | 0/24 |
| 2.5 s | 6/12 | 0/24 |

Maximum scores in the 2.5-second warm-up arm:

| Synthetic phrase | A | B | C |
|---|---:|---:|---:|
| Lamp | 0.8404 | 0.4747 | 0.9272 |
| Lamp, set a timer for five minutes | 0.9537 | 0.9222 | 0.5701 |
| Thank you, Lamp | 0.0078 | 0.0042 | 0.0967 |
| Hey Lamp | 1.0000 | 1.0000 | 0.9104 |

The controls cover near words, ordinary mentions of a lamp, and the timer/thanks phrases without the name. Their highest score across every voice and padding arm was 0.0780. They contain only 33.24 seconds of distinct trimmed speech spans; zero detections here is insufficient to qualify a lower threshold or a room-talk false-alarm rate.

Lowering the detector threshold to its allowed minimum of 0.5 would cover 8/12 warm positive maxima instead of 6/12, but would still miss every trailing name. These are maximum-score comparisons, not events recomputed at 0.5. The existing [rescue rule](../../../crates/name/src/rescue.rs) at gate 0.5 cannot activate on the three trailing fixtures because their primary maxima stay below 0.10; this inference does not measure the unavailable second model. Voice's separate final admission conditions were not exercised.

## Exact score method

1. Generate each fixed phrase with macOS `say` at 175 words/minute, writing only to an AIFF file. Convert with `afconvert` to 16 kHz, mono signed PCM16.
2. Parse the WAV data chunk and remove only literal zero samples at its beginning/end. Do not normalize, amplify, denoise, or trim nonzero onset samples.
3. Prefix 0, 8000, 16000 or 40000 zero samples and append 51200 zeros (3.2 seconds). Feed raw little-endian PCM16 to the existing native debug `lamp-name check`, one input per process. Raw PCM avoids the checker's fixed 44-byte RIFF-header skip.
4. Keep the default hearing threshold 0.9; disable rescue, stop scoring and quiet resting. Record every maximum and `heard` event, with the checker's four-decimal score precision.

The [checker](../../../crates/name/src/main.rs) resets state per file and produces no score before 2.03 seconds. Padding tests **cold-checker/onset sensitivity**, not live-service pre-roll. The 0.5- and 2.5-second arms differ by exactly 25 80-ms scoring steps, preserving their relative phase; other padding comparisons also change phase.

CSV times are positions in the input signal. `maximum_from_phrase_onset_s` subtracts the added leading silence from the maximum's file position; it is relative to the phrase's first retained nonzero sample, not an independently aligned name word. A `heard` timestamp is the closed hearing's first threshold crossing, not publication or delivery time. None is an end-to-end latency measurement.

## Evidence and limits

- [per-line.csv](per-line.csv): all 144 rows, synthetic text, scores, signal positions, events and padded-PCM hashes. Host performance fields are omitted.
- [source-hashes.csv](source-hashes.csv): hashes and trimming/frame counts for the 36 generated source utterances.
- [provenance.json](provenance.json): model/tool/scorer hashes, audited source hashes and configuration. Model assets matched the [product registry](../../../deploy/product/models.json). Source hashes are not an independent reproducible-build attestation for the existing executable.
- [SHA256SUMS](SHA256SUMS): checksums of these four evidence files.

No audio, binaries, scripts, raw command logs, local machine paths, or human voice recordings are included. Voice identifiers remain anonymized, so this text-only bundle cannot regenerate byte-identical speech by itself. One earlier synthesis attempt produced zero frames; it was rejected and is excluded from every count.

**Proposal before code:** retain current thresholds. Compare the actual played recording and exact STT input with real detector context and trial flags. Fix the line-start/pre-roll path only if that establishes onset loss or reset exposure. If full-context scores remain weak, evaluate a model candidate covering isolated, initial and trailing calls against matched negatives. This supplemental evidence does not qualify a runtime change.
