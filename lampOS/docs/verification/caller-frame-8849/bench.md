# Sensor 6 paired evaluation

The simple base admits every ordinary in-window follow-up. The percentages below describe the **sensor’s own explicit classifications**. “Kept” means a same-caller pair classified as same. “Rejected” means another speaker classified as different. Wrong and unknown answers both count as misses. Each comparator receives identical PCM.

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

Both development sets use mono 16 kHz PCM and digitally mixed stationary noise at +5 dB referenced to active speech. Neither is a recording through the team lamp. All 144 noisy other-caller trials across the two sets received a false same-caller classification. The candidate stays out of the combined judge.

## Latency and parity

| Local paired replay | First frame decision | Warm frame p50 / p95 | Warm unguarded CAM++ p50 / p95 |
|---|---:|---:|---:|
| Six synthetic voices | 1.695 ms | 2.827 / 4.108 ms | 56.882 / 63.295 ms |
| LibriSpeech | 1.489 ms | 2.988 / 6.481 ms | 57.314 / 63.227 ms |

These Mac CPU timings start with both arrays in memory and end after enrollment, follow-up inference and classification. There were 287 warm observations per run. For the first run, cold model-file load took 2.111 ms for Personal VAD and 866.047 ms for CAM++. The guarded whole-line classifier had no available pair to time. A TensorFlow reference check covered the first synthetic case in each of the eight class/length/condition cells. All 1,176 frame labels matched the Rust implementation. The maximum absolute posterior difference was 0.00000415. Lamp readiness and audible response require a separate capture. [Frozen model](https://huggingface.co/wq2012/personal-vad-v1-st-ce/tree/56f67ae31897518a994dec0cb3fd283fd9db027e)

The real model child returned the same classifications on all 288 synthetic pairs. The first process launch took 636.918 ms from spawn to its ready handshake. The next 24 process restarts, with the executable recently used by the OS, had median 4.583 ms and p95 5.114 ms to ready. In the 288-pair persistent process, the first write-to-class round trip was 2.359 ms and warm p95 was 4.246 ms. These figures include pipe transfer of both clips and native inference. WAV reads were outside the timer.

Builder #2 keeps archive `sensor6-8849-20261127` locally on the builder’s Mac. The synthetic and public-reader runs have separate frozen inventories and READMEs. The team lead is custodian of the physical recording on Mac 2. The v2 #8885 scorer distinguishes raw development counts from the verified real-person holdout. Model posteriors are frame observations. The denominators in the table count paired utterances. [Scoring contract](../../../tools/caller-frame-bench/scoring.md) · [Team evaluation](https://github.com/autonomous-ai/lampOS/issues/8849) · [Frozen reviewer rubric](https://github.com/autonomous-ai/lampOS/issues/8850#issuecomment-7221984822)
