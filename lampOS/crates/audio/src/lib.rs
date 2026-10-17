//! Bounded audio processing for the Lamp microphone process.
//!
//! The caller supplies the speaker's actual accepted PCM as render reference.
//! Provider-generated but unplayed audio must never enter this reference path.
//! Capture and render are independent 10 ms streams. Device discontinuities
//! must be surfaced and the processor reset; they must not be hidden as silence.
use sonora::config::{EchoCanceller, NoiseSuppression};
use sonora::{AudioProcessing, Config, StreamConfig};
use std::io;

#[cfg(target_os = "linux")]
mod alsa_device;
#[cfg(unix)]
pub mod capture;
#[cfg(unix)]
pub mod device;
pub mod drift;
pub mod duplex;
pub mod evidence;
#[cfg(unix)]
pub mod notice;
pub mod ownership;
#[cfg(target_os = "linux")]
pub mod pcm;
pub mod playout;
pub mod qualification;
#[cfg(unix)]
pub mod replay;
pub mod steady;
#[cfg(unix)]
pub mod testing;

pub const CAPTURE_RATE: u32 = 16_000;
pub const RENDER_RATE: u32 = 24_000;
pub const CAPTURE_SAMPLES: usize = 160;
pub const RENDER_SAMPLES: usize = 240;
pub const BLOCK_MS: u32 = 10;

pub type CaptureBlock = [i16; CAPTURE_SAMPLES];
pub type RenderBlock = [i16; RENDER_SAMPLES];
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// How the echo canceller is given the speaker's sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reference {
    /// As the speaker accepted it. The default.
    #[default]
    AsGiven,
    /// Through a [`steady::SteadyReference`], which holds the lamp's echo in
    /// one place as the canceller sees it (docs/steady-reference.md).
    Steady,
}

/// A single microphone worker owns this processor; no shared mutex or network I/O.
pub struct EchoProcessor {
    inner: AudioProcessing,
    observer: evidence::EchoObserver,
    steady: Option<Box<steady::SteadyReference>>,
    noise_suppression: bool,
    /// Whether the canceller keeps its linear output ([`Self::with_linear_output`]).
    export_linear: bool,
    render_input: [f32; RENDER_SAMPLES],
    render_output: [f32; RENDER_SAMPLES],
    capture_input: [f32; CAPTURE_SAMPLES],
    capture_output: [f32; CAPTURE_SAMPLES],
    linear_output: [[f32; CAPTURE_SAMPLES]; 1],
    linear: Option<CaptureBlock>,
}

impl EchoProcessor {
    pub fn new(noise_suppression: bool) -> Self {
        Self::with_reference(noise_suppression, Reference::AsGiven)
    }

    pub fn with_reference(noise_suppression: bool, reference: Reference) -> Self {
        Self {
            inner: Self::build(noise_suppression, false),
            observer: evidence::EchoObserver::new(),
            steady: (reference == Reference::Steady).then(Box::default),
            noise_suppression,
            export_linear: false,
            render_input: [0.0; RENDER_SAMPLES],
            render_output: [0.0; RENDER_SAMPLES],
            capture_input: [0.0; CAPTURE_SAMPLES],
            capture_output: [0.0; CAPTURE_SAMPLES],
            linear_output: [[0.0; CAPTURE_SAMPLES]; 1],
            linear: None,
        }
    }

    /// Also keep the canceller's linear output for [`Self::linear`]: the
    /// microphone, high-pass filtered, less the echo the canceller's
    /// adaptive filter predicts, before its residual echo suppressor and
    /// before noise suppression. The suppressor takes speech over the lamp
    /// away with the echo; the linear output keeps that speech, and keeps
    /// the echo the filter does not predict. Off by default. On,
    /// [`Self::capture`] returns exactly the blocks it returns off.
    pub fn with_linear_output(mut self) -> Self {
        self.export_linear = true;
        self.reset();
        self
    }

    fn build(noise_suppression: bool, export_linear: bool) -> AudioProcessing {
        AudioProcessing::builder()
            .config(Config {
                echo_canceller: Some(EchoCanceller {
                    export_linear_aec_output: export_linear,
                    ..EchoCanceller::default()
                }),
                noise_suppression: noise_suppression.then(NoiseSuppression::default),
                // Do not silently change mic gain or boost background speech.
                ..Config::default()
            })
            .capture_config(StreamConfig::new(CAPTURE_RATE, 1))
            .render_config(StreamConfig::new(RENDER_RATE, 1))
            .build()
    }

    /// Reinitialize after lost reference/capture samples, device reopen or privacy.
    /// Reset is a cold operation and must be recorded as a discontinuity.
    pub fn reset(&mut self) {
        self.inner = Self::build(self.noise_suppression, self.export_linear);
        self.observer.reset();
        if let Some(steady) = self.steady.as_mut() {
            steady.reset();
        }
        self.render_input.fill(0.0);
        self.render_output.fill(0.0);
        self.capture_input.fill(0.0);
        self.capture_output.fill(0.0);
        self.linear_output = [[0.0; CAPTURE_SAMPLES]; 1];
        self.linear = None;
    }

    pub fn render(&mut self, samples: &RenderBlock) -> Result<()> {
        let steadied;
        let samples = match self.steady.as_mut() {
            Some(steady) => {
                steadied = *steady.render(samples);
                &steadied
            }
            None => samples,
        };
        pcm_to_float(samples, &mut self.render_input);
        self.inner
            .process_render_f32(&[&self.render_input], &mut [&mut self.render_output])
            .map_err(|e| io::Error::other(format!("AEC render: {e:?}")))?;
        self.observer.render(samples);
        Ok(())
    }

    /// The queue-derived delay is a hint, not a forced acoustic alignment.
    /// Sonora's default AEC3 uses its internal adaptive delay estimator; the
    /// external hint can seed the render buffer after an internal reset.
    pub fn capture(
        &mut self,
        samples: &CaptureBlock,
        device_delay_ms: u16,
    ) -> Result<CaptureBlock> {
        self.linear = None;
        if device_delay_ms > 500 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AEC device delay exceeds 500 ms",
            )
            .into());
        }
        // With the steady reference, the canceller is handed the speaker's
        // sound that much later, and the delay it is told shrinks to match.
        let device_delay_ms = match self.steady.as_mut() {
            Some(steady) => {
                steady.capture(samples);
                let applied = steady.steadiness().delay_ms.round() as u16;
                device_delay_ms.saturating_sub(applied)
            }
            None => device_delay_ms,
        };
        self.inner
            .set_stream_delay_ms(i32::from(device_delay_ms))
            .map_err(|e| io::Error::other(format!("AEC delay: {e:?}")))?;
        pcm_to_float(samples, &mut self.capture_input);
        self.inner
            .process_capture_f32(&[&self.capture_input], &mut [&mut self.capture_output])
            .map_err(|e| io::Error::other(format!("AEC capture: {e:?}")))?;
        if self.capture_output.iter().any(|sample| !sample.is_finite()) {
            return Err(io::Error::other("AEC returned non-finite audio").into());
        }
        let clean = self.capture_output.map(float_to_pcm);
        self.observer.capture(samples, &clean);
        // Only for `linear`: it never fails the block above.
        if self.export_linear
            && self.inner.linear_aec_output(&mut self.linear_output)
            && self.linear_output[0]
                .iter()
                .all(|sample| sample.is_finite())
        {
            self.linear = Some(self.linear_output[0].map(float_to_pcm));
        }
        Ok(clean)
    }

    /// Skip the echo evidence's search while the speaker has been given only
    /// silence ([`evidence::EchoObserver::skip_while_render_silent`]).
    pub fn skip_evidence_while_render_silent(&mut self, on: bool) {
        self.observer.skip_while_render_silent(on);
    }

    /// What the echo evidence's skip did since construction.
    pub fn evidence_skipped(&self) -> evidence::Skipped {
        self.observer.skipped()
    }

    /// The canceller's linear output for the block [`Self::capture`] last
    /// returned (see [`Self::with_linear_output`]), 160 samples at the
    /// capture rate. It is one 64-sample canceller block (4 ms) behind the
    /// microphone, and one block ahead of the processed output, which the
    /// residual echo suppressor delays by another. `None` without
    /// `with_linear_output`, when that call failed or the canceller gave no
    /// finite linear output, and until a capture has run since the last
    /// reset. Before the canceller has had any render it passes the
    /// microphone through and this is silence.
    pub fn linear(&self) -> Option<&CaptureBlock> {
        self.linear.as_ref()
    }

    /// What the render explains of the capture block just processed, before
    /// and after cancellation. `None` until a capture has run since the last
    /// reset. These are measurements for an admission decision made
    /// elsewhere; see [`evidence`].
    pub fn evidence(&self) -> Option<evidence::EchoEvidence> {
        self.observer.latest()
    }

    /// Cached AEC render-buffer alignment, in milliseconds. Reading it does not
    /// run DSP or enable the optional residual-echo detector. `Some` is not
    /// convergence or echo-free evidence: initial and retained alignment also
    /// have values. A new/reset processor reports `None` until capture runs.
    pub fn internal_alignment_ms(&self) -> Option<i32> {
        self.inner.statistics().delay_ms
    }

    /// What the steady reference is doing, when it was chosen.
    pub fn steadiness(&self) -> Option<steady::Steadiness> {
        self.steady.as_ref().map(|steady| steady.steadiness())
    }
}

fn pcm_to_float<const N: usize>(samples: &[i16; N], output: &mut [f32; N]) {
    for (source, target) in samples.iter().zip(output.iter_mut()) {
        *target = f32::from(*source) / 32768.0;
    }
}

pub fn float_to_pcm(sample: f32) -> i16 {
    // Rust's float-to-int conversion saturates; caller-generated audio is finite.
    (sample.clamp(-1.0, 1.0) * 32768.0).round() as i16
}
