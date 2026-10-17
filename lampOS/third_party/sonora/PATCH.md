# Sonora dependency snapshot

Sonora 0.2.0 from crates.io (`sonora-0.2.0.crate`, SHA-256
`2e1ce442ab5cb392c4067b8f2cced20be4cf12e479f6dfbf44acbe9b14ac9250`, the
checksum the workspace lock pinned before this snapshot; upstream
<https://github.com/dignifiedquire/sonora>, commit
`a024d6ef8351add55be5e8b1d6cc35f555787660`, `crates/sonora`), under its
BSD-3-Clause license (`LICENSE`).

The files are the crate archive's, unchanged, except:

- `Cargo.lock` (upstream's own lockfile) is removed; a path dependency does
  not use it.
- `PATCH.md` (this file) is added.
- The patch below, in `src/config.rs`, `src/audio_processing.rs`,
  `src/audio_processing_impl.rs` and `src/echo_canceller3.rs`; each change
  is marked `lampOS patch`.

The workspace uses it through `[patch.crates-io]` in `lampOS/Cargo.toml`
and excludes it from workspace membership, as `vendor/tough`. The other
Sonora crates (`sonora-aec3`, `sonora-agc2`, `sonora-ns`, ...) still come
from crates.io at the versions `Cargo.lock` pins.

Not run here: upstream's own test suite, examples and benches (their
dev-dependencies are not in the workspace lock).

## Patch: AEC3's linear output

WebRTC's `AudioProcessing::GetLinearAecOutput` gives AEC3's linear output:
the capture less the echo its adaptive filter predicts, before the
residual echo suppressor. Sonora 0.2.0 ports AEC3's side of it but cannot
reach it: `AudioProcessingImpl` passes `None` for it, nothing sets
`filter.export_linear_aec_output`, and `EchoCanceller3::process_capture`
never copies the linear sub-frames out (in C++ the sub-frame view points
into the output buffer; in the port it is an owned copy).

The patch adds, off by default:

- `config::EchoCanceller::export_linear_aec_output: bool` (default
  `false`). It sets `filter.export_linear_aec_output` in both of AEC3's
  configs and allocates a 16 kHz buffer for the output. `apply_config`
  re-initializes AEC3 when it changes.
- `AudioProcessing::linear_aec_output(&self, &mut [[f32; 160]]) -> bool`,
  as `GetLinearAecOutput`: the last capture frame's linear output, one
  array per AEC3 capture channel, in `[-1, 1]`, at 16 kHz.
- In `EchoCanceller3::process_capture`, the copy of each linear sub-frame
  into the output buffer.

Off, the processing path is unchanged: `None` is passed and no buffer
exists, as before. On, AEC3 only copies its linear error signal out, so
the processed capture is unchanged; `lamp-audio`'s tests check both
outputs block by block. Sonora's `analyze_linear_aec_output_when_available`
still analyses the processed capture, as in 0.2.0, not the linear output.

Replace this snapshot when an upstream Sonora release exposes the linear
output, and check that its scale and timing match this one first.
