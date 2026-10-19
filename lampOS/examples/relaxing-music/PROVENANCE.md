# Provenance and license

SPDX-License-Identifier: Apache-2.0

The score in `score.rs` is an original 48-second instrumental composition
created for the lampOS relaxing-music example on 2026-10-19. It was written
directly as eight chord voicings and sixteen upper notes. It is not a
transcription or adaptation of an existing piece.

The renderer in `crates/skills/examples/render_relaxing_music.rs` generates
every sample from integer oscillators and envelopes. It uses no recordings,
samples, soundfonts, speech engine, network service or external decoder.
Equal-tempered tuning and elementary oscillator mathematics are its only
musical building blocks.

The score, renderer, example text and generated `relaxing.wav` are provided
under the Apache License, Version 2.0, reproduced in `LICENSE`. The generated
recording carries the same license as its source. No third-party recording
rights are asserted or required by this example.

To reproduce the recording, use the renderer from the same lampOS source
revision. Its sample arithmetic and WAV encoding use integers and explicit
little-endian bytes. The renderer's tests pin the recording's byte fingerprint;
the package manifest independently records its size and SHA-256.

Generated WAVs and package archives are build artifacts and must not be
committed to source control. This provenance does not establish acoustic
loudness, musical quality on a particular speaker, or successful device
playback; those require separate listening and device checks.
