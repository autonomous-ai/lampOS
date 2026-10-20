# Pipecat reference fixture

`pipecat-reference.json` (SHA256
`e63375bd32ce098a9930019821e3db8c5aa9d0b09e5cd75777ca14b5d5518fae`) is the
oracle for `tests/it/parity.rs`. It is data only; no generator lives in lampOS.

## How it was generated

It was generated once, outside lampOS, by a private Python verification tool
(SHA256 `cf593e0af2e864f1d01e65b51aef9e770c5c234da077ebe44242b2495984288f`).
That tool was committed earlier as `make_reference.py` and removed under the
owner's Rust-only rule.

- **Inputs.**
  - Pipecat commit `53d313b176f65246e451282d5d03537574f54835`, file
    `src/pipecat/audio/turn/smart_turn/_whisper_features.py`. Its SHA256 is
    recorded in the JSON as `features_source_sha256`.
  - The pinned model `smart-turn-v3.2-gpu.onnx` (`model_sha256`).
  - ONNX Runtime 1.31.0 with one intra-op thread, and NumPy 2.5.4.
- **Signals.** The four signals are the integer xorshift32 sequences that
  `signal()` in `tests/it/parity.rs` regenerates bit for bit:
  - `noise_2s`: seed 1, 32,000 samples;
  - `voiced_9s`: seed 2, 144,000 samples, with a 3-sample pulse every 100
    samples;
  - `short_0p3s`: seed 7, 4,800 samples;
  - `silence`: empty.
- **Steps for each signal**, following Pipecat's `local_smart_turn_v3.py`:
  1. Convert the int16 PCM to float32 divided by 32,768.
  2. Keep the last 128,000 samples, left-padding with zeros to that length.
  3. Call `compute_whisper_log_mel_features(audio, do_normalize=True)`.
  4. Run the model on `features[None]`.
- **Stored values**, from the features as float64:
  - `sum` and `abs_sum`;
  - every 37th value in row-major order (`stride`, `sampled`);
  - the sample count;
  - the model's output `probability`.

## How the test uses it

- Rust features must match every sampled value within 1e-4, and both sums
  within 0.064 (a mean difference below 1e-6 over 64,000 values).
- With the model present, tract's probability must match within 1e-4, both in
  process and through `lamp-turn-end`.
- A changed model or Pipecat feature code invalidates this file. Regenerate it
  with the same inputs outside lampOS, and update the hashes recorded here.
