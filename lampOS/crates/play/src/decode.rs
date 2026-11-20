//! MP3 in, mono S16 at 24 kHz out, as the stream arrives. MP3 only: the
//! decoder is symphonia's MPEG audio reader and decoder (pure Rust, no other
//! container or codec is built). Nothing here opens a file, a socket or the
//! bus: it reads a [`Read`] and hands PCM to a callback.
//!
//! It stops on the first thing it cannot decode (no resync, no skipped
//! frame), on a source that is not MP3, on more bytes than [`Limits`] allow
//! and on more audio than they allow.
use crate::resample::{OUT_RATE, Resampler};
use std::io::{self, Read};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{Decoder, DecoderOptions},
    errors::Error,
    formats::{FormatOptions, FormatReader},
    io::{MediaSourceStream, MediaSourceStreamOptions, ReadOnlySource},
};
use symphonia::default::{codecs::MpaDecoder, formats::MpaReader};

/// How a play ended: the words the app is told.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The whole stream played.
    Done,
    /// The callback said stop.
    Stopped,
    /// More bytes than the cap.
    TooBig,
    /// More audio than the cap.
    TooLong,
    /// Not MP3, or a frame it could not decode.
    BadAudio,
}

impl Outcome {
    /// The fixed word for the app and the journal.
    pub fn word(self) -> &'static str {
        match self {
            Outcome::Done => "done",
            Outcome::Stopped => "stopped",
            Outcome::TooBig => "too_big",
            Outcome::TooLong => "too_long",
            Outcome::BadAudio => "bad_audio",
        }
    }
}

/// What the callback answers for each block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Flow {
    Continue,
    Stop,
}

/// The caps on one play.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// The most bytes read from the source.
    pub most_bytes: u64,
    /// The most audio, in seconds of output.
    pub most_seconds: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            most_bytes: 128 * 1024 * 1024,
            most_seconds: 3 * 60 * 60,
        }
    }
}

/// The most samples handed to the callback at once: 200 ms at 24 kHz, a
/// whole number of the speaker's 10 ms chunks, and under the door's block
/// (9,600 bytes is 4,800 samples).
pub const BLOCK: usize = 4_800;

/// How a play went: the outcome, and what the journal may say of it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub outcome: Outcome,
    /// Samples handed to the callback, at 24 kHz.
    pub samples: u64,
    /// Bytes read from the source.
    pub bytes: u64,
}

/// A reader that stops at the byte cap and says so.
struct Counted<R> {
    inner: R,
    read: u64,
    most: u64,
    over: Arc<AtomicBool>,
    total: Arc<AtomicU64>,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let room = self.most.saturating_sub(self.read);
        if room == 0 {
            self.over.store(true, Ordering::Relaxed);
            return Err(io::Error::other("more bytes than the cap"));
        }
        let take = buffer
            .len()
            .min(usize::try_from(room).unwrap_or(usize::MAX));
        let count = self.inner.read(&mut buffer[..take])?;
        self.read += count as u64;
        self.total.store(self.read, Ordering::Relaxed);
        Ok(count)
    }
}

/// Plays `source` through `sink`, 24 kHz mono S16 in blocks of at most
/// [`BLOCK`] samples, until it ends or a cap or an error stops it.
pub fn decode<R, F>(source: R, limits: Limits, sink: F) -> Outcome
where
    R: Read + Send + Sync + 'static,
    F: FnMut(&[i16]) -> Flow,
{
    decode_report(source, limits, sink).outcome
}

/// [`decode`], with the counts the journal may keep.
pub fn decode_report<R, F>(source: R, limits: Limits, mut sink: F) -> Report
where
    R: Read + Send + Sync + 'static,
    F: FnMut(&[i16]) -> Flow,
{
    let over = Arc::new(AtomicBool::new(false));
    let bytes = Arc::new(AtomicU64::new(0));
    let samples = Arc::new(AtomicU64::new(0));
    let (counted_bytes, counted_samples) = (Arc::clone(&bytes), Arc::clone(&samples));
    let outcome = run(
        source,
        limits,
        &over,
        &counted_bytes,
        &counted_samples,
        &mut sink,
    );
    Report {
        outcome,
        samples: samples.load(Ordering::Relaxed),
        bytes: bytes.load(Ordering::Relaxed),
    }
}

fn run<R, F>(
    source: R,
    limits: Limits,
    over: &Arc<AtomicBool>,
    bytes: &Arc<AtomicU64>,
    handed: &Arc<AtomicU64>,
    sink: &mut F,
) -> Outcome
where
    R: Read + Send + Sync + 'static,
    F: FnMut(&[i16]) -> Flow,
{
    let counted = Counted {
        inner: source,
        read: 0,
        most: limits.most_bytes,
        over: Arc::clone(over),
        total: Arc::clone(bytes),
    };
    let stream = MediaSourceStream::new(
        Box::new(ReadOnlySource::new(counted)),
        MediaSourceStreamOptions::default(),
    );
    let ended_badly = |over: &AtomicBool| {
        if over.load(Ordering::Relaxed) {
            Outcome::TooBig
        } else {
            Outcome::BadAudio
        }
    };
    let Ok(mut format) = MpaReader::try_new(stream, &FormatOptions::default()) else {
        return ended_badly(over);
    };
    let Some(track) = format.default_track() else {
        return Outcome::BadAudio;
    };
    let (track_id, params) = (track.id, track.codec_params.clone());
    let Ok(mut decoder) = MpaDecoder::try_new(&params, &DecoderOptions::default()) else {
        return Outcome::BadAudio;
    };
    let most_samples = limits.most_seconds * u64::from(OUT_RATE);
    let mut resampler: Option<(u32, Resampler)> = None;
    let mut mono: Vec<f32> = Vec::new();
    let mut pending: Vec<i16> = Vec::new();
    let mut played: u64 = 0;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(error))
                if error.kind() == io::ErrorKind::UnexpectedEof
                    && !over.load(Ordering::Relaxed) =>
            {
                // The end of the stream. Nothing decoded at all is no MP3.
                if played == 0 && pending.is_empty() {
                    return Outcome::BadAudio;
                }
                if let Some((_, resampler)) = &mut resampler {
                    resampler.finish(&mut pending);
                }
                return flush(sink, &pending, handed);
            }
            Err(_) => return ended_badly(over),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(_) => return ended_badly(over),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        if channels == 0 || channels > 2 || spec.rate == 0 {
            return Outcome::BadAudio;
        }
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        samples.copy_interleaved_ref(decoded);
        mono.clear();
        mono.extend(
            samples
                .samples()
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        // The rate is the first frame's; a stream that changes it is refused
        // rather than played at the wrong speed.
        let resampler = match &mut resampler {
            Some((rate, resampler)) if *rate == spec.rate => resampler,
            Some(_) => return Outcome::BadAudio,
            None => {
                let Some(made) = Resampler::new(spec.rate) else {
                    return Outcome::BadAudio;
                };
                &mut resampler.insert((spec.rate, made)).1
            }
        };
        resampler.push(&mono, &mut pending);
        while pending.len() >= BLOCK {
            let block: Vec<i16> = pending.drain(..BLOCK).collect();
            played += BLOCK as u64;
            if played > most_samples {
                return Outcome::TooLong;
            }
            handed.fetch_add(block.len() as u64, Ordering::Relaxed);
            if sink(&block) == Flow::Stop {
                return Outcome::Stopped;
            }
        }
    }
}

/// The last, short block, then the end.
fn flush<F: FnMut(&[i16]) -> Flow>(sink: &mut F, pending: &[i16], handed: &AtomicU64) -> Outcome {
    if !pending.is_empty() {
        handed.fetch_add(pending.len() as u64, Ordering::Relaxed);
        if sink(pending) == Flow::Stop {
            return Outcome::Stopped;
        }
    }
    Outcome::Done
}

#[cfg(test)]
mod tests;
