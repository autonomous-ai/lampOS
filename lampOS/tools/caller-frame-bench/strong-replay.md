# Replaying the 28 November caller model

Use the same sealed pairs as the previous frame model. The provider set contains 1,108 comparisons. The separate Mac 2 capture contains 44 scored comparisons from synthetic speech played into the lamp microphone. Run the physical commands on Mac 2. Keep WAVs, per-case scores, frame traces, official run/report rows and recordings on the collecting computer. Only aggregate counts, latency distributions and artifact hashes belong in the team handoff.

## Inputs

Use the SpeechBrain ECAPA graph from revision `0f99f2d0ebe89ac095bcc5903c4dd8f72b367286`, file SHA-256 `d8c32b0dbc17e31c7b13c9d7910f0adaca2ea0b0b3351da05041c9168e741994`. The compiled policy is in `lampOS/crates/same-voice/src/strong_caller.rs`. The registered decision is a median active-frame cosine strictly greater than 0.25. Unknown earns zero credit. The comparison script independently recomputes every decision from the signed 25 ms frame scores.

| Source | Pair SHA-256 | Audio inventory SHA-256 |
| --- | --- | --- |
| Provider test voices | `540b13ff4457aa569a9f8499b37fa8a90de9a820ded8935eb280eda205287172` | `b3a66082354b7b0181de89261c9d2eaadbfdca6f156c117a60e40a2e745f2dbf` |
| Mac 2 physical playback | `ca526e1a10942223c61736dab329894647a2a4b68e608ef0304aeb9ffa9ec0e7` | `c84b0ef3499bfdf8722200512fd01f4a455a1a7f779735d3f926981c910de074` |

Use Ruby with its standard JSON and CSV libraries, the existing local reviewer binary built from `bfb0a95a9da96740f77923b76fd8547fecbb7ffd`, the exact native example and the corresponding source revision. From an exact clean source checkout run these commands. For a patched build, retain and apply the entire source patch, then set `PATCH` to its absolute filename.

```sh
T=lampOS/tools/caller-frame-bench
K=lampOS/crates/same-voice/src/strong_caller.rs
REV=$(git rev-parse HEAD)
PATCH=-
cargo test --release --locked --manifest-path lampOS/Cargo.toml -p lamp-same-voice --lib strong_caller::tests
cargo build --release --locked --manifest-path lampOS/Cargo.toml -p lamp-same-voice --example strong_caller_bench
B=lampOS/target/release/examples/strong_caller_bench
ruby "$T/strong-score-test.rb"
```

## Provider set

Point `H` at the morning handoff’s `data` directory, `M` at the local `ecapa-voxceleb.onnx` and `O` at a new private output directory. Create `O` once. The tools create fresh subdirectories and retain earlier runs.

```sh
C="$H/provider-7224058425/corpus"
J="$H/provider-7224058425/official-corpus-inventory/inventory.json"
V="$H/provider-7224058425/official-paired/manifest.json"
OLD="$H/provider-7224058425/result/run/frame.tsv"
S="$H/reviewer-bfb0/scorer/lamp-sensor-score"
mkdir -m 700 "$O"
ruby "$T/strong-replay.rb" prepare provider "$C/pairs.tsv" "$J" "$O/plan"
ruby "$T/strong-replay.rb" bind "$REV" "$PATCH" "$B" "$M" "$K" "$S" "$O/freeze.json"
ruby "$T/strong-replay.rb" run "$O/plan" "$C" "$B" "$M" "$K" "$O/freeze.json" all "$O/run"
ruby "$T/strong-score.rb" "$O/plan" "$C" "$O/run" "$OLD" "$S" "$J" "$V" "$O/score"
```

## Mac 2 physical playback

Use the unchanged local baseline `frame.tsv` as `OLD`. Set `S` to Mac 2’s original reviewer executable and set `M` and `O` as above. The prior collection also carries local inventory hash `97ecd42f5d512bd93c300db032645a98d4ce26e9fd7bd3bb1cdbbdb95433b65e` and folder seal `fe2e0021f56f6512d51ab5165cd661d18440ad19846883890bb0636f06209130`.

```sh
D="$HOME/lamp-voice-data/sensor6-D-1128"
C="$D/09-corpus-primary"
mkdir -m 700 "$O"
ruby "$T/strong-replay.rb" prepare physical "$C/pairs.tsv" "$D/SHA256SUMS" "$O/plan"
ruby "$T/strong-replay.rb" bind "$REV" "$PATCH" "$B" "$M" "$K" "$S" "$O/freeze.json"
ruby "$T/strong-replay.rb" run "$O/plan" "$C" "$B" "$M" "$K" "$O/freeze.json" all "$O/run"
ruby "$T/strong-score.rb" "$O/plan" "$C" "$O/run" "$OLD" - - - "$O/paired"
```

For official raw-pair scoring use the existing verified corpus `inventory.json` as `J` and its existing v2 raw-pair `manifest.json` as `V`. If the original capture has no compatible manifest, supply `V=@/absolute/path/speaker-map.tsv`. Its four tab-separated columns are `wav_sha256`, `speaker_id`, `source_path` and `source_sha256`. Every used waveform needs exactly one mapping from the original collection’s speaker metadata. Each source path and digest must belong to `J`. Keep any newly prepared map outside the original sealed input directory. Run `lamp-sensor-score archive "$C" "$O/reviewer-inventory"` if an inventory of this exact source directory is needed. The new inventory and map must derive from the retained original evidence.

```sh
ruby "$T/strong-score.rb" "$O/plan" "$C" "$O/run" "$OLD" "$S" "$J" "$V" "$O/reviewed"
```

The physical result retains 16 same-caller and 28 constructed other-voice comparisons. The primary recorder gap accounts for 28 missing follow-ups and 25 missing additional impostor comparisons. A separate 12 follow-ups lack a verified cut of their own caller. Both background cells have zero measured pairs.

Physical preparation verifies the outer folder seal, links the exact inner audio inventory and pair list to their entries in that seal, and checks each input waveform against both inventories. The plan records separate folder and audio inventory hashes.

## Reading the output

The scorer emits before/after counts for all pairs and each short/long, quiet/noisy cell, plus provider groups for the digital set. Every row belongs to its planned class, including wrong, unknown and missing baseline outputs. The program refuses an incomplete new candidate run. Pair-level classifications are reconstructed from saved active-frame raw cosines. Identical follow-ups paired with different callers retain their own decision and frame trace. The official development-only rubric emits null live readiness and uses waveform-relative end points.

Use `all` for reported latency on both sets. Every pair then recomputes its own enrollment, probe and decision from in-memory PCM on the host. A second pass compares the independently cached features against every fresh decision, raw cosine and frame. Model initialization is separate. `sample` measures a deterministic prespecified subset and marks other elapsed times unmeasured. Application admission, device readiness and audible response require separate measurements.
