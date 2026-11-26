//! The model, behind [`Speaker`] (#8754): Phonon 31m on the CPU, its q8
//! weights, English text rules, and the voices kept beside it.
use std::path::Path;

/// What speaks a sentence: the model, or a test's stand-in.
pub trait Speaker: Send + Sync {
    /// Says `text` in `voice`, handing each piece of 24 kHz mono samples in
    /// `[-1, 1]` to `out` as it is made. `out` returning `false` (the voice
    /// went away) stops the sentence there, which is not a failure.
    fn speak(
        &self,
        text: &str,
        voice: &str,
        out: &mut dyn FnMut(&[f32]) -> bool,
    ) -> Result<(), String>;
}

/// Phonon 31m, loaded once and reused.
pub struct Phonon {
    synth: ptts::synth::Synth,
}

impl Phonon {
    /// Loads the model kept in `folder` ([`crate::fetch::folder`]) with
    /// every voice under its `voices/`, its tensor work on `threads`
    /// threads.
    pub fn load(folder: &Path, threads: usize) -> Result<Self, String> {
        use ptts::checkpoint::{Checkpoint, ResolveOptions};
        use ptts::synth::{DeviceKind, Quant};
        xn::set_num_threads(threads);
        let checkpoint = Checkpoint::resolve(
            folder,
            ResolveOptions {
                quant: Quant::Q80,
                weights: None,
            },
        )
        .map_err(|e| format!("the model: {e}"))?;
        let mut synth = checkpoint
            .builder(ptts::preprocess::Lang::En)
            .device(DeviceKind::Cpu)
            .build()
            .map_err(|e| format!("the model: {e}"))?;
        checkpoint.register_voices(&mut synth);
        Ok(Self { synth })
    }

    /// The voices the model has, by name.
    pub fn voices(&self) -> Vec<String> {
        self.synth.voices()
    }
}

impl Speaker for Phonon {
    fn speak(
        &self,
        text: &str,
        voice: &str,
        out: &mut dyn FnMut(&[f32]) -> bool,
    ) -> Result<(), String> {
        let options = ptts::synth::SpeechOptions::default().voice(voice);
        let stream = self
            .synth
            .stream_with(text, &options)
            .map_err(|e| e.to_string())?;
        // Dropping the stream, at the end or when `out` says stop, stops
        // and joins its generation threads.
        for piece in stream {
            let piece = piece.map_err(|e| e.to_string())?;
            if !piece.is_empty() && !out(&piece) {
                break;
            }
        }
        Ok(())
    }
}
