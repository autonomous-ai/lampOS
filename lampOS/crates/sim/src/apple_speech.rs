//! A resident speech synthesizer for `lamp-sim speech` on macOS (sim-3-P4
//! step 1b; the first builder on #6811, 13 Nov 21:28, option (c)): Apple's
//! `AVSpeechSynthesizer`, kept for the service's life, writing each line to
//! buffers (`writeUtterance:toBufferCallback:`) that are made into 24 kHz
//! S16LE as they come. Measured on macOS 26 (arm64) from Rust before this
//! module: the first buffer of a warm line in about 8 ms whatever its length,
//! the first line after start in about 330 ms; `say` takes 370 to 830 ms.
//!
//! Its callbacks come only while the **main thread's** run loop runs (a worker
//! thread's own run loop got none in 60 s), so [`serve_on_main_thread`] owns the
//! main thread: the HTTP side runs elsewhere and hands lines over a channel.
//!
//! The only `unsafe` in lamp-sim is here: every AVFAudio binding is an
//! `unsafe fn`. Each block says why its call is sound.
#![allow(unsafe_code)]

use crate::speech::{Engine, ToRoute};
use block2::RcBlock;
use objc2_avf_audio::{
    AVAudioBuffer, AVAudioPCMBuffer, AVSpeechSynthesisVoice, AVSpeechSynthesizer, AVSpeechUtterance,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop, NSString};
use std::{
    io,
    ptr::NonNull,
    sync::mpsc,
    time::{Duration, Instant},
};

/// The longest a line may take to be spoken whole.
const LINE_WAIT: Duration = Duration::from_secs(30);
/// How long the main thread's run loop runs between looks for a new line.
const PUMP: f64 = 0.005;

/// What the synthesizer hands back for a line.
enum Piece {
    Samples { rate: f64, samples: Vec<f32> },
    End,
    Failed(String),
}

/// A line to speak and where its pieces go.
pub struct Line {
    text: String,
    out: mpsc::Sender<Piece>,
}

/// The engine the HTTP side holds: it hands each line to the main thread.
pub struct Resident {
    lines: mpsc::Sender<Line>,
}

/// The resident engine, and the receiving end [`serve_on_main_thread`] takes.
pub fn resident() -> (Resident, mpsc::Receiver<Line>) {
    let (lines, receiver) = mpsc::channel();
    (Resident { lines }, receiver)
}

impl Engine for Resident {
    fn speak(&self, text: &str, out: &mut dyn FnMut(&[u8]) -> io::Result<()>) -> io::Result<()> {
        let (pieces, receiver) = mpsc::channel();
        self.lines
            .send(Line {
                text: text.to_owned(),
                out: pieces,
            })
            .map_err(|_| io::Error::other("the synthesizer has stopped"))?;
        let mut to_route: Option<ToRoute> = None;
        let until = Instant::now() + LINE_WAIT;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match receiver.recv_timeout(left) {
                Ok(Piece::Samples { rate, samples }) => {
                    let bytes = to_route
                        .get_or_insert_with(|| ToRoute::new(rate))
                        .piece(&samples);
                    out(&bytes)?;
                }
                Ok(Piece::End) => return Ok(()),
                Ok(Piece::Failed(why)) => return Err(io::Error::other(why)),
                Err(_) => {
                    return Err(io::Error::other(format!(
                        "no whole line from the synthesizer in {} s",
                        LINE_WAIT.as_secs()
                    )));
                }
            }
        }
    }
}

/// Owns the main thread: one synthesizer, each line written to buffers, and
/// the run loop kept running so their callbacks come. Returns when every
/// sender is gone.
pub fn serve_on_main_thread(lines: mpsc::Receiver<Line>, language: &str) {
    // SAFETY: `new` makes a fresh synthesizer; it is used on this thread only.
    let synthesizer = unsafe { AVSpeechSynthesizer::new() };
    // SAFETY: a plain class method taking a valid NSString; it returns a voice
    // or nil, and nil (no such language) leaves the system's default voice.
    let voice =
        unsafe { AVSpeechSynthesisVoice::voiceWithLanguage(Some(&NSString::from_str(language))) };
    loop {
        match lines.try_recv() {
            Ok(line) => speak_line(&synthesizer, voice.as_deref(), line),
            Err(mpsc::TryRecvError::Empty) => pump(),
            Err(mpsc::TryRecvError::Disconnected) => return,
        }
    }
}

/// Runs the main thread's run loop for a moment, so callbacks come.
fn pump() {
    // SAFETY: called on the main thread; `currentRunLoop` is that thread's run
    // loop, and `runMode:beforeDate:` with the default mode and a date a few
    // milliseconds on returns once that date passes or a source fires.
    unsafe {
        let date = NSDate::dateWithTimeIntervalSinceNow(PUMP);
        NSRunLoop::currentRunLoop().runMode_beforeDate(NSDefaultRunLoopMode, &date);
    }
}

fn speak_line(
    synthesizer: &AVSpeechSynthesizer,
    voice: Option<&AVSpeechSynthesisVoice>,
    line: Line,
) {
    // SAFETY: a class method making an utterance from a valid NSString.
    let utterance =
        unsafe { AVSpeechUtterance::speechUtteranceWithString(&NSString::from_str(&line.text)) };
    // SAFETY: sets a property of the utterance just made, to a voice from
    // `voiceWithLanguage` or none.
    unsafe { utterance.setVoice(voice) };
    let (done, finished) = mpsc::channel::<()>();
    let out = line.out.clone();
    let block = RcBlock::new(move |buffer: NonNull<AVAudioBuffer>| {
        // SAFETY: AVFoundation passes a valid buffer, alive for this call.
        let buffer: &AVAudioBuffer = unsafe { buffer.as_ref() };
        let Some(pcm) = buffer.downcast_ref::<AVAudioPCMBuffer>() else {
            return;
        };
        // SAFETY: reads properties of a valid PCM buffer.
        let (frames, rate) = unsafe { (pcm.frameLength() as usize, pcm.format().sampleRate()) };
        if frames == 0 {
            // The last buffer of a line is empty.
            let _ = out.send(Piece::End);
            let _ = done.send(());
            return;
        }
        // SAFETY: for a PCM buffer with `frames` frames, a non-null
        // `floatChannelData` (or `int16ChannelData`) points to one pointer per
        // channel, each to at least `frames` samples, valid for this call; the
        // first channel is read and copied out before returning.
        let samples = unsafe {
            let floats = pcm.floatChannelData();
            let shorts = pcm.int16ChannelData();
            if !floats.is_null() {
                std::slice::from_raw_parts((*floats).as_ptr(), frames).to_vec()
            } else if !shorts.is_null() {
                std::slice::from_raw_parts((*shorts).as_ptr(), frames)
                    .iter()
                    .map(|sample| f32::from(*sample) / 32_768.0)
                    .collect()
            } else {
                let _ = out.send(Piece::Failed("a buffer with no samples".to_owned()));
                let _ = done.send(());
                return;
            }
        };
        let _ = out.send(Piece::Samples { rate, samples });
    });
    // SAFETY: `block` is a valid heap block and the utterance is valid; the
    // synthesizer retains (copies) the block for as long as it may call it, so
    // a call after this function has returned still finds it alive (and sends
    // to a channel nobody reads, which is harmless).
    unsafe {
        synthesizer.writeUtterance_toBufferCallback(&utterance, &*block as *const _ as *mut _);
    }
    let until = Instant::now() + LINE_WAIT;
    while finished.try_recv().is_err() {
        if Instant::now() >= until {
            // SAFETY: stops the synthesizer this thread owns, at once.
            unsafe {
                synthesizer.stopSpeakingAtBoundary(objc2_avf_audio::AVSpeechBoundary::Immediate);
            }
            let _ = line
                .out
                .send(Piece::Failed("the line did not end".to_owned()));
            return;
        }
        pump();
    }
}
