# Comparing caller-voice replay results

Score both classifiers on one frozen list of call/follow-up pairs. The caller’s voice is the positive class. The other speaker is the negative class. Use these commands in a private local directory. The report keeps the same denominator for each classifier.

```sh
ruby prepare-heldout.rb "$SOURCE_TAR" "$PUBLISHED_SHA256" "$NEW_PRIMARY_ARCHIVE"
```

This prepares the #8907 **ElevenLabs and Gemini test voices**. It verifies and retains the original tar plus every member, including excluded OpenAI audio, and every derived file. It reads `manifest.tsv` and `voices.tsv` inside the tar, pairs separate bare “Hey Lamp” calls with ordinary lines, and records the selected providers, voices, source paths, speaking styles and reuse counts. The accepted provider spellings after lowercasing and trimming are `elevenlabs`, `gemini`, `gemini live`, `gemini-live` and `gemini_live`. `source-scope.tsv` gives counts for every excluded voice and source provider. The primary scorer checks both ends of each pair against the checksummed test-voice metadata and emits per-provider results. [Owner’s provider scope](https://github.com/autonomous-ai/lampOS/issues/8907#issuecomment-7221499098)

The final provider release confirms ElevenLabs rendered genuine whispers. The preparer keeps both providers’ declared styles when matching enrollments. [Published release](https://github.com/autonomous-ai/lampOS/issues/8907#issuecomment-7224058425)

The quiet and +5 dB pink-noise cells run through the repository’s microphone and channel functions. Supply `--channel measured.npz` when the team has the measured impulse response and floor spectrum. The default uses the repository’s seeded synthetic room response. Ruby imports the archived modules by absolute file path using PyCall 1.5.5. The existing `channel.py apply` CLI applies the impulse response and floor. Supply `--acoustic-dir DIR` for another checkout’s `lampOS/training` or the archived flat `provenance/` directory. The default uses this checkout. Its regressions run with `ruby prepare-heldout_test.rb`. [Primary dataset specification](https://github.com/autonomous-ai/lampOS/issues/8907)

Use the same Python interpreter for the binding and the CLI. It needs NumPy, SciPy and a shared libpython. Install PyCall and its Fiddle dependency in a local benchmark tool directory on the Mac. The tested versions are Ruby 4.0.6, CPython 3.14.7, NumPy 2.4.6 and SciPy 1.18.0. The actual run records its versions in `provenance/channel-run.txt`. [PyCall usage](https://github.com/red-data-tools/pycall.rb) · [PyCall 1.5.5](https://rubygems.org/gems/pycall/versions/1.5.5)

```sh
export CALLER_FRAME_GEMS="$TOOLS/pycall-gems"
export PYTHON="$(command -v python3)"
gem fetch fiddle -v 1.1.8
gem fetch pycall -v 1.5.5
printf '%s\n' '7fa8ee3627271497f3add5503acdbc3f40b32f610fc1cf49634f083ef3f32eee  fiddle-1.1.8.gem' \
  '6552170dc6723be73004fb7baf6af7f55d7eb06724d4ca18e44957444323d6e1  pycall-1.5.5.gem' | shasum -a 256 -c -
gem install --local --no-document --install-dir "$CALLER_FRAME_GEMS" ./fiddle-1.1.8.gem ./pycall-1.5.5.gem
```

```sh
ruby score.rb compare synthetic "$ARCHIVE/pairs.tsv" "$RUN/whole.tsv" "$RUN/frame.tsv" "$RUN/paired"
```

Use `synthetic-heldout` for provider-declared held-out synthetic voices, `public-speech` for public natural speech recorded elsewhere, and `lamp-replay` for local replay of labelled lamp recordings. All four modes measure classification and host inference time. Audio capture, lamp readiness, added admission wait and speech-end to audible response require observations on the device.

## Inputs

`pairs.tsv` has a header with the tab-separated columns below. Use an opaque case ID, two paths relative to the archive directory, `same` or `different`, a positive integer word count, and `quiet` or `noisy`. List every planned pair once. A follow-up can be paired with different opening callers. Include the TSV and all source files in its sibling `audio-SHA256SUMS`, with one SHA-256, two spaces and a relative path on each line. The comparison verifies the full inventory.

```text
case_id	reference	followup	label	words	condition
```

Each producer TSV starts with `# pairs_sha256=` followed by the 64-digit digest of those exact `pairs.tsv` bytes. Its next line is the tab-separated header below. Emit `same`, `different` or `unknown` and the finite score used by the classifier. Leave the score empty when unavailable. Measure host inference using one monotonic clock and record microseconds. Mark first and warm invocations as `first` and `warm`. Use `unmeasured` with an empty duration for a cached result with no measured invocation. Document exactly what is included in that host interval and measure model initialization and wake enrollment separately.

```text
case_id	output	score	replay_elapsed_us	phase
```

The report counts explicit `same` on a same-caller pair as kept, and explicit `different` on an other-source pair as rejected. Each missing row, explicit abstention and wrong-direction answer stays in the denominator. `report.json` gives each of those counts, the source hashes, and nearest-rank host timings. `README.md` gives the overall and four crossed length/noise rows. Only the new destination directory is created. Keep the producer’s TSVs, model versions and configuration, audio, hashes, and measurements together locally.

## Shared reviewer scorer

Use frozen #8885 v2 at `bfb0a95a9da96740f77923b76fd8547fecbb7ffd`. Both the manifest and candidate run use version 2. The manifest names rubric `sensor-team-2026-11-27-v2`. Each session declares its actual origin, collection consent when known, `retention: "keep_all"`, recording/inventory/README hashes and capture clock. Build with `cargo +1.96.0 build --offline --locked` in the frozen checkout’s `lampOS/tools/sensors` directory. [V2 adapter](https://github.com/autonomous-ai/lampOS/blob/bfb0a95a9da96740f77923b76fd8547fecbb7ffd/lampOS/tools/sensors/adapter-v2.md) · [Reviewer’s raw-pair handoff](https://github.com/autonomous-ai/lampOS/issues/8849#issuecomment-7222729467)

Seal each private raw session, then verify every raw byte and the generated README. Supply a new inventory directory outside the raw root. Keep all originals, run outputs, failed attempts, traces, initial inventories and subsequent versions locally.

```sh
"$SCORER" archive "$SESSION" "$NEW_INVENTORY"
"$SCORER" verify-archive "$SESSION" "$NEW_INVENTORY/inventory.json"
```

For a lead-supplied annotated capture, use the genuine source-clock manifest, its manifest-bound trial sidecar, the empty `voice_match` replay header for that candidate, the actual submission and policy audit, and its private bindings JSON. The manifest and audit must preserve the actual opportunity for each method. Whole-line CAM++ requires both captured inputs to reach 24,000 samples and processes up to 28,800 samples. The frame model emits a posterior every 10 ms starting at 25 ms of PCM and needs eight winning speech frames to classify a line. A frame offset gives a position within the waveform. Acoustic opportunity, child submission, readiness and actual first admission have separate clock boundaries. [Measurement protocol](https://github.com/autonomous-ai/lampOS/blob/bfb0a95a9da96740f77923b76fd8547fecbb7ffd/lampOS/docs/sensor-evaluation.md)

```sh
ruby reviewer_replay.rb "$ARCHIVE/pairs.tsv" "$RUN/frame.tsv" \
  "$CAPTURE/frame-manifest.json" "$CAPTURE/frame-sidecar.json" \
  "$RUN/frame-header.json" "$CAPTURE/frame-audit.json" "$RUN/frame-bindings.json" \
  "$SCORER" "$NEW_OFFICIAL_FRAME"
```

Repeat with the whole-line predictions, manifest, header, bindings and observed submission audit for that method. `pairs.tsv` must cover every nonfault voice-match case in each split it includes. The private sidecar binds the reference and probe paths, trial IDs, source clock, worker epoch and request. The audit contains the actual `input_end_us`, `added_wait_us`, `held_line` and `safety_violation` for each submitted case. An unobserved submission or missing prediction leaves that case in the denominator. The adapter passes through observed host inference time and writes null lamp readiness.

The bindings JSON has `build_sha` (the 40-hex producer revision or parent base), `exact_commit` (boolean), `prediction_sha256` (the candidate TSV), and absolute private paths named `producer_binary`, `model` and `config`. For uncommitted source, supply `source_patch` pointing to the complete retained patch, including untracked inputs. Its digest must match `source_patch_sha256` in the candidate header. For each session, `sessions[session_id]` supplies absolute `root` and `inventory` paths for `verify-archive`. The recording hash must occur in that verified inventory. The driver records the input digests, official validation/verification and scoring exit statuses, and output hashes.

The official `raw_by_split` includes development and holdout class counts by origin. Top-level `raw_classification` is holdout-only. A valid replay writes an unqualified report and the scorer exits 1. Malformed or unrepresentable evidence exits 2. Verified source clock uncertainty of at most 5 ms and on-lamp behavior are needed for the real-person qualification gate.

### Digital voice pairs

Use the official `raw_development_pairs` mode for generated speech, public LibriSpeech and #8907. It scores every planned ordered caller/probe pair. Shared probes and quiet/noisy renderings are correlated. The resulting raw counts appear in `raw_by_split.development`; `development_pairs` gives unique probe/reference segments and reuse. Physical holdout counts remain zero. Per-row source and capture paths stay private. [Reviewer’s raw-pair contract](https://github.com/autonomous-ai/lampOS/blob/bfb0a95a9da96740f77923b76fd8547fecbb7ffd/lampOS/tools/sensors/development-pairs.md)

```sh
"$SCORER" archive "$CORPUS" "$NEW_SOURCE_INVENTORY"
ruby reviewer_raw_pairs.rb synthetic-heldout "$CORPUS/pairs.tsv" "$BENCHMARK/run" \
  "$NEW_SOURCE_INVENTORY/inventory.json" "$PRIVATE_BINDINGS" "$SCORER" "$NEW_OFFICIAL_RUN"
"$SCORER" archive "$NEW_OFFICIAL_RUN" "$NEW_RESULT_INVENTORY"
"$SCORER" verify-archive "$NEW_OFFICIAL_RUN" "$NEW_RESULT_INVENTORY/inventory.json"
```

Use `synthetic` for the native six-voice development corpus and `public-speech` for the LibriSpeech test-clean transfer corpus. Use `synthetic-heldout` only for the eligible ElevenLabs/Gemini test voices. It enforces the original provider test split at both ends of each pair and records accuracy by follow-up provider. The original synthetic or public test partition stays in the source README. Keep all unselected provider files under the original archive.

The canonical producer’s parent directory contains `SHA256SUMS`, `source-revision.txt` and `source.patch`. Its `run/` directory contains the candidate TSVs and `provenance.tsv` from the Rust benchmark. The raw-pair bindings JSON uses `build_sha`, `exact_commit`, `producer_binary`, `scorer_revision` and `scorer_sha256`. Its `whole` and `frame` objects each supply the retained absolute `model` and `config` paths and the predeclared `config_sha256`. Use the original run’s source and full patch for a dirty build, supplied as `source_patch` and `source_patch_sha256`. The adapter checks each candidate’s bytes, exact model and source against the sealed run. It requires a new destination and keeps failed attempts.

The adapter gives each distinct complete model-input WAV its own file clock. It records the real waveform SHA-256 and sample offsets from zero to the exclusive PCM end. Original acquisition time, consent, clock mapping, distance, bearing and lighting can remain unknown. For these offline replays, `input_end_us` is the source file endpoint and readiness is null. Zero added wait and false hold/safety values describe the isolated classifier process. Host timing covers the actual local inference. Files prepared from the same original utterance are still correlated across conditions. The adapter checks each official output and each class denominator, then writes the four length/noise cells, method-specific hashes, official input/output digests and original partition to `summary.json`. An unqualified exit 1 from each official classifier run is the expected result. [Measurement rules](https://github.com/autonomous-ai/lampOS/blob/bfb0a95a9da96740f77923b76fd8547fecbb7ffd/lampOS/tools/sensors/adapter-v2.md)

The public EVAL reports each required class and four length/noise cells with their denominators, wrong/unknown/missing counts, actual independent source and pairing counts, method-specific opportunities, host and device timing boundaries, build/model/config/scorer digests and local inventory verification status. Keep private words, participant identifiers, paths and case rows local. [Public EVAL template](https://github.com/autonomous-ai/lampOS/blob/bfb0a95a9da96740f77923b76fd8547fecbb7ffd/lampOS/tools/sensors/eval-template.md)

Run `ruby score_test.rb` to check paired counting, provenance, v2 joins and archive verification.
