# Sensor 6 paired evaluation

The simple base admits every ordinary in-window follow-up. The percentages below describe the **sensor’s own explicit classifications**. “Kept” means a same-caller pair classified as same. “Rejected” means another speaker classified as different. Wrong and unknown answers both count as misses. Each comparator receives identical PCM.

## ECAPA on the same two sets

At the frozen median frame cosine threshold of 0.25, the ECAPA sensor missed the 95% caller-retention target on both sets. Its other-voice rejection reached 94.6% on provider audio and 67.9% on the physical microphone. The shared reviewer verified all 1,152 decisions. [EVAL and retained data](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7236567550) · [Registered rule](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7235630577)

| Set and condition | Same caller kept | Other voice rejected |
|---|---:|---:|
| Provider, all | 130/554 (23.5%) | 524/554 (94.6%) |
| Provider, 1–3 words, quiet | 36/121 | 113/121 |
| Provider, 4+ words, quiet | 36/156 | 149/156 |
| Provider, 1–3 words, +5 dB | 27/121 | 112/121 |
| Provider, 4+ words, +5 dB | 31/156 | 150/156 |
| dbda microphone, all | 10/16 (62.5%) | 19/28 (67.9%) |
| dbda microphone, 1–3 words, quiet | 7/9 | 12/18 |
| dbda microphone, 4+ words, quiet | 3/7 | 7/10 |
| dbda microphone, background, either length | Unmeasured | Unmeasured |

Five provider trials per class returned unknown. The 44 physical pairs all yielded a decision. The ECAPA provider reports at the registered snapshot and published implementation revision had byte-identical frame scores across 211,994 frame positions. On Mac 2, the physical manifest used 46 waveforms joined to their six original synthetic voices using the sealed scene plan. The collector retains the source recordings, per-frame decisions, earlier attempts, two separate source inventories and the shared reviewer's report. [Reproduction and local data locations](../../../tools/caller-frame-bench/strong-replay.md)

### Latency on lamp-dbda

The team lead ran the native ARM64 child on lamp-dbda with its regular services active. The eight synthetic 16 kHz fixtures included quiet and +5 dB speech. Every timed request sent both the “Hey Lamp” reference and the line from memory and ended at the decision newline. The timer excludes microphone capture, ASR and audible reply. Its three process starts and 24 warm requests all completed. [Lead’s board receipt](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7236523867)

| Warm request on lamp-dbda | Lines | p50 | p95 |
|---|---:|---:|---:|
| All fixtures | 24 | 398 ms | 2,082 ms |
| 1–3 words | 12 | 375 ms | 415 ms |
| 6–7 words | 12 | 601 ms | 2,122 ms |

The overall warm p95 exceeded the 500 ms target. Across three fresh processes, median spawn to ready was 2,031 ms, the first one-word request after ready took 302 ms at the median, and median spawn to first decision was 2,334 ms. All six timing summaries were reproduced from the physical board’s raw TSV, SHA-256 `09adb90d3a6c641c048dd99f2220498c60fdc528658c1921b7363910deb3ac7e`. The model runs asynchronously and the simple window admits lines independently.

<a id="held-out-elevenlabs-and-gemini-voices"></a>

## Earlier ST trial on held-out ElevenLabs and Gemini voices

On 1,108 paired comparisons the frame model kept **457/554 same callers (82.5%)** and rejected **88/554 other voices (15.9%)**. The guarded whole-line model kept 0/554 and rejected 84/554 (15.2%). The frame model rejected **0/277 other voices in +5 dB noise**. Sensor 6 stays off and outside the combined judge. [EVAL, raw counts and report hashes](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7224796496)

| Follow-up and condition | Guarded CAM++ same / other correct | Frames same kept | Frames other rejected |
|---|---|---:|---:|
| 1–3 words, quiet | 0/121 · 12/121 | 71/121 (58.7%) | 34/121 (28.1%) |
| 1–3 words, +5 dB | 0/121 · 12/121 | 120/121 (99.2%) | 0/121 (0%) |
| 4+ words, quiet | 0/156 · 31/156 | 111/156 (71.2%) | 54/156 (34.6%) |
| 4+ words, +5 dB | 0/156 · 29/156 | 155/156 (99.4%) | 0/156 (0%) |

The complete release contains 44,981 original 16 kHz mono WAVs. Its plan lists 19 additional Gemini renders that were never delivered. The primary used 68 separate literal “Hey Lamp” openings and 277 ordinary probes, including 60 one-word, eight two-word and 53 three-word recordings. Both ends of every pair came from held-out ElevenLabs or Gemini voices. Each probe used its true enrollment and an impostor under the same quiet or noisy condition. The microphone and room response were simulated with the repository acoustic tools. All 4,500 already-rendered OpenAI files and all training voices remain in the original archive. [Provider data and release digest](https://github.com/autonomous-ai/lampOS/issues/8907#issuecomment-7224058425)

All eight ElevenLabs test voices and five of six Gemini test voices supplied bare openings. The remaining voice supplied 19 ordinary probes in a separate other-voice check. Frames rejected 10/76 of its impostor comparisons, including 0/38 in noise. Two independently verified wake-only cuts from its compound openings gave 35/38 same-caller comparisons correct. Guarded CAM++ rejected 12/76 impostors and kept 0/38 same callers on that supplement. Those opportunities have separate denominators. In a separate whole-line control, longer same-voice, same-style compound references gave guarded CAM++ 0/554 callers kept and 252/554 other speakers rejected (45.5%).

On the benchmark Mac, the frame model’s first paired inference took 3.062 ms and warm p50/p95 were 2.915/7.918 ms (1,107 warm pairs). Guarded CAM++ reached inference on 210 pairs. Its first eligible result took 79.338 ms and warm p50/p95 were 76.652/86.900 ms. Model-file load took 2.158 ms for frames and 866.426 ms for CAM++. These timings start with both PCM buffers available. The team lead measured the physical microphone separately in run D. The digital run contributes zero qualifying real-person pairs. [Run D handoff](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7224605726)

The official #8885 scorer reconciled all 2,216 method-by-pair predictions. The private corpus inventory SHA-256 is `907573417595c3347c56f9a152fb00a0dc5b9ac3f46014bf92071af7d16f873a`. Archive `sensor6-8849-20261127` holds the complete provider tar, original and processed WAVs, acoustic stages and noise stems, source metadata, posteriors, timing, executables and all failed attempts. Real people’s recordings stay on their collecting Mac.

<a id="played-through-lamp-dbdas-microphone"></a>

## Earlier ST trial through lamp-dbda’s microphone

The team lead played labelled synthetic voices into the lamp’s microphone and scored the recorded 16 kHz audio on Mac 2. Frames kept **16/16** same-caller follow-ups and rejected **1/28 (3.6%)** constructed impostor comparisons. Of the remaining impostors, 25 were classified as the caller and two were unknown. Guarded CAM++ abstained on all 44 measured pairs. Each wake-only cut was 0.55–0.81 s. [Lead’s physical EVAL and archive](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7226611814)

| Recorded cell | Frames same kept | Frames impostors rejected | Guarded CAM++ same / other correct |
|---|---:|---:|---|
| 1–3 words, quiet | 9/9 | 1/18 (5.6%) | 0/9 · 0/18 |
| 4+ words, quiet | 7/7 | 0/10 | 0/7 · 0/10 |
| 1–3 words, background | Unmeasured | Unmeasured | Unmeasured |
| 4+ words, background | Unmeasured | Unmeasured | Unmeasured |

The recorder stopped at 00:42:56 GMT+7 and restarted at 01:36:09. All three natural other-voice lines and both background lines occurred in that gap. The ledger retains **53 missing planned pairs** (28 follow-ups plus 25 associated impostors). The lead could not verify wake boundaries for 31 of 91 calls. Twelve follow-ups without their own verified opening were recorded as uncertain. A separate comparison against another verified call by the same voice kept 8/9 short and 3/3 longer follow-ups.

First frame inference on the busy replay Mac took 6.6 ms and warm p50/p95 were 5.6/10.5 ms over 43 pairs. The frame model loaded in 3.3 ms. CAM++ loaded in 3.9 s. Forcing it past its duration guard gave warm p50/p95 of 246/372 ms. Live lamp readiness remains unmeasured. All 6,211 physical files stay with the lead on Mac 2. The folder seal is `fe2e0021f56f6512d51ab5165cd661d18440ad19846883890bb0636f06209130`.

A separate provider diagnostic submitted each saved 300 ms additive-noise stem alone. With noisy enrollments, frames returned same caller on 550/554 comparisons. With quiet enrollments, the same stems gave 19/554 same-caller results. All 1,108 digital-zero controls gave unknown. These controls carry zero weight in the accuracy tables. Their verified local inventory is `623e254bc21db9793ea516d25ea605d42267f539df1209136e9b55ab71e3cc9c`.

## Development runs before the provider holdout

Each row has 36 same-caller and 36 other-caller pairs. The whole-line comparator abstained on all 576 opportunities across both runs because every isolated enrollment was shorter than its 1.5-second minimum. The unguarded diagnostic shows the classifications the same CAM++ weights produce if forced to process those short clips.

| Local source | Follow-up | Setting | Whole-line correct, same / other | Frame same kept | Frame other rejected | Unguarded CAM++ correct, same / other |
|---|---|---|---|---:|---:|---|
| Six synthetic Mac voices | 1–3 words | Quiet | 0 / 0 | 32/36 (88.9%) | 21/36 (58.3%) | 0 / 34 |
| Six synthetic Mac voices | 1–3 words | +5 dB | 0 / 0 | 36/36 (100%) | 0/36 (0%) | 0 / 33 |
| Six synthetic Mac voices | 4+ words | Quiet | 0 / 0 | 30/36 (83.3%) | 19/36 (52.8%) | 0 / 31 |
| Six synthetic Mac voices | 4+ words | +5 dB | 0 / 0 | 36/36 (100%) | 0/36 (0%) | 0 / 36 |
| LibriSpeech test-clean | 1–3 words | Quiet | 0 / 0 | 26/36 (72.2%) | 22/36 (61.1%) | 0 / 34 |
| LibriSpeech test-clean | 1–3 words | +5 dB | 0 / 0 | 36/36 (100%) | 0/36 (0%) | 0 / 36 |
| LibriSpeech test-clean | 4+ words | Quiet | 0 / 0 | 26/36 (72.2%) | 20/36 (55.6%) | 0 / 35 |
| LibriSpeech test-clean | 4+ words | +5 dB | 0 / 0 | 36/36 (100%) | 0/36 (0%) | 0 / 34 |

The first run has 72 independently rendered follow-ups and 12 literal “Hey Lamp” openings from six verified voices. Each quiet/noisy follow-up was paired with its own caller and with an impostor. A first fixture generation exposed two macOS voice names that produced identical PCM. That generation and its raw files were retained and excluded. On the verified set, the frame check kept 134/144 (93.1%) and rejected 40/144 (27.8%). The manifest SHA-256 is `c52ca2a4b91fb34a598fed8c9109e37f1a279e6f2fdf94656b1f6b39fb067fa3`.

The second run uses 12 real LibriSpeech test-clean readers and 84 distinct source utterances, with different enrollment/follow-up chapters. Each reference contains two transcript-aligned words serving as a surrogate for “Hey Lamp”. The reader says the same words under both comparisons. The local corpus retains the original FLAC, source download, labels, alignments, derived WAVs and noise. The frame check kept 124/144 (86.1%) and rejected 42/144 (29.2%). The manifest SHA-256 is `34b3a7ea7257611e70425b2ce6a50b58eb28a1df9b8deea0f31ab0510dd3aaa8`. [LibriSpeech](https://www.openslr.org/12/) · [Word alignments](https://doi.org/10.5281/zenodo.2619474)

The open-source checkpoint publisher drew its English training speakers from LibriSpeech train-clean-100 and its English evaluation speakers from test-clean. Our 12 readers come from test-clean. This run measures transfer to short prompts and added noise. [Publisher’s data preparation](https://github.com/wq2012/personal_vad/blob/03be700243b1d6241a8903b0ebff6814476d6853/scripts/download_mls.py#L247-L273)

Both development sets use mono 16 kHz PCM and digitally mixed stationary noise at +5 dB referenced to active speech. Neither is a recording through the team lamp. All 144 noisy other-caller trials across the two sets received a false same-caller classification. On each of the 144 noisy follow-up WAVs, substituting the impostor reference for the true reference produced the same maximum target-frame fraction of 1.00. A threshold on this fraction cannot separate these pairs. In quiet, the true reference ranked higher on 56/72 synthetic and 46/72 public follow-ups. The candidate stays out of the combined judge.

### What the model did before the speaker started

We kept each enrollment and submitted just the first 300 ms of its follow-up. In both corpora the quiet prefix contained 4,800 zero PCM samples. Every corresponding noisy prefix matched the saved additive noise samples byte for byte. The frozen native child returned unknown on all 288 silent requests. It classified all 144 distinct noise clips as the caller with both the true and impostor enrollments, for 288/288 same-caller results with a target-frame fraction of 1.00. An independent check reconciled every prefix and response. These are speech-free controls and contribute zero trials to the accuracy table. The local archive inventory SHA-256 is `62a8fa9d836e31a47d7dcf831d8b09abf8b1996a9f54fc63ac05d23e3239c16b`.

### Whole-line control with a longer opening

To measure how much the 1.5-second guard affected the comparison, a separate replay kept all 288 LibriSpeech probes, pair labels and conditions. It extended each two-word enrollment through the end of the same recorded utterance when more speech was available. Eight of the twelve enrollments contained further speech. In total, 118 pairs met the duration minimum for both inputs.

Guarded CAM++ correctly kept 0/144 same-caller pairs and rejected 56/144 other-caller pairs (38.9%). It classified 14 same-caller pairs as other. The remaining 130 same-caller and 88 other-caller pairs were unknown. Other voices rejected in the short quiet, short noisy, long quiet and long noisy cells were 5/36, 5/36, 23/36 and 23/36. The Mac took 78.081 ms for the first eligible pair and 78.031/80.413 ms at warm p50/p95, including two uncached embedding requests and comparison. The primary frame and whole-line results above still use the two-word enrollment.

## Latency and parity

| Local paired replay | First frame decision | Warm frame p50 / p95 | Warm unguarded CAM++ p50 / p95 |
|---|---:|---:|---:|
| Six synthetic voices | 1.695 ms | 2.827 / 4.108 ms | 56.882 / 63.295 ms |
| LibriSpeech | 1.489 ms | 2.988 / 6.481 ms | 57.314 / 63.227 ms |

These Mac CPU timings start with both arrays in memory and end after enrollment, follow-up inference and classification. There were 287 warm observations per run. For the first run, cold model-file load took 2.111 ms for Personal VAD and 866.047 ms for CAM++. The guarded whole-line classifier had no available pair to time. A TensorFlow reference check covered the first synthetic case in each of the eight class/length/condition cells. All 1,176 frame labels matched the Rust implementation. The maximum absolute posterior difference was 0.00000415. Lamp readiness and audible response require a separate capture. [Frozen model](https://huggingface.co/wq2012/personal-vad-v1-st-ce/tree/56f67ae31897518a994dec0cb3fd283fd9db027e)

The real model child returned the same classifications on all 288 synthetic pairs. The first process launch took 636.918 ms from spawn to its ready handshake. The next 24 process restarts, with the executable recently used by the OS, had median 4.583 ms and p95 5.114 ms to ready. In the 288-pair persistent process, the first write-to-class round trip was 2.359 ms and warm p95 was 4.246 ms. These figures include pipe transfer of both clips and native inference. WAV reads were outside the timer.

Builder #2 keeps archive `sensor6-8849-20261127` locally on the builder’s Mac. The synthetic and public-reader runs have separate frozen inventories and READMEs. The team lead is custodian of the physical recording on Mac 2. The v2 #8885 scorer distinguishes raw development counts from the verified real-person holdout. Model posteriors are frame observations. The denominators in the table count paired utterances. [Scoring contract](../../../tools/caller-frame-bench/scoring.md) · [Team evaluation](https://github.com/autonomous-ai/lampOS/issues/8849) · [Frozen reviewer rubric](https://github.com/autonomous-ai/lampOS/issues/8850#issuecomment-7221984822)

A repeat from clean commit `1b3fc5360b9dc9d70064c52acccebb62fa3d4351` reproduced every classification in the two development runs. Warm p50/p95 was 2.834/4.155 ms on generated speech and 3.124/6.987 ms on public speech. The issue records both inventories, reports, model hashes and the exact limits of the shared scorer. [EVAL on #8849](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7222294409)

The shared #8885 scorer reproduced all 1,152 method-by-pair rows using v2 `raw_development_pairs` at `bfb0a95a9da96740f77923b76fd8547fecbb7ffd`. Each corpus has 288 ordered comparisons, 144 condition-specific follow-up WAVs and 24 condition-specific enrollment WAVs. Its physical holdout has zero verified pairs. The official reports retain raw development classification and mark the sensor unqualified. [Reviewer’s raw-pair contract](https://github.com/autonomous-ai/lampOS/blob/bfb0a95a9da96740f77923b76fd8547fecbb7ffd/lampOS/tools/sensors/development-pairs.md) · [Official EVAL with report hashes](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7222967382)
