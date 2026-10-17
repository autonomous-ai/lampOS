# Sonora

[![crate][crate-image]][crate-link]
[![docs][docs-image]][docs-link]
![BSD-3-Clause licensed][license-image]
![Rust Version][rustc-image]
[![Build Status][build-image]][build-link]

Pure Rust implementation of [WebRTC] audio processing -- the full pipeline with
echo cancellation (AEC3), noise suppression, automatic gain control (AGC2),
and high-pass filtering.

Provides a Rust API. A C API is available via the separate [`sonora-ffi`](https://crates.io/crates/sonora-ffi) crate.

## Usage

```rust
use sonora::{AudioProcessing, Config, StreamConfig};
use sonora::config::{EchoCanceller, NoiseSuppression, GainController2};

let config = Config {
    echo_canceller: Some(EchoCanceller::default()),
    noise_suppression: Some(NoiseSuppression::default()),
    gain_controller2: Some(GainController2::default()),
    ..Default::default()
};

let mut apm = AudioProcessing::builder()
    .config(config)
    .capture_config(StreamConfig::new(48000, 1))
    .render_config(StreamConfig::new(48000, 1))
    .build();

// Process 10ms frames (48kHz * 10ms = 480 samples)
let src = vec![0.0f32; 480];
let mut dest = vec![0.0f32; 480];
apm.process_capture_f32(&[&src], &mut [&mut dest]).unwrap();
```

See the [workspace README] for benchmarks, platform support, and the full crate listing.

## License

BSD-3-Clause. See [LICENSE] for details.

[//]: # (badges)

[crate-image]: https://img.shields.io/crates/v/sonora.svg
[crate-link]: https://crates.io/crates/sonora
[docs-image]: https://docs.rs/sonora/badge.svg
[docs-link]: https://docs.rs/sonora/
[license-image]: https://img.shields.io/badge/license-BSD--3--Clause-blue.svg
[rustc-image]: https://img.shields.io/badge/rustc-1.91+-blue.svg
[build-image]: https://github.com/dignifiedquire/sonora/actions/workflows/ci.yml/badge.svg?branch=main
[build-link]: https://github.com/dignifiedquire/sonora/actions/workflows/ci.yml?query=branch:main

[//]: # (general links)

[WebRTC]: https://webrtc.org
[workspace README]: https://github.com/dignifiedquire/sonora#readme
[LICENSE]: https://github.com/dignifiedquire/sonora/blob/main/LICENSE
