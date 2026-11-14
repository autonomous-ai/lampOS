//! A playback queue with no device: [`SoftPcm`] is a [`PlaybackPcm`] that drains at the
//! rendering rate by the clock, as the board's queue drains into its amplifier, and hands each
//! block of sound it takes to a sink. The speaker worker plays into it exactly as it plays into
//! ALSA's `PCM` (its ownership, authority, privacy and underrun logic sit above the seam), so a
//! developer's computer beside a simulator can have the voice's sound on the bus instead of a
//! sound card (`--speaker-device bus:speaker1`, `lamp_voice::bus_speaker`).
//!
//! The queue is [`RENDER_SAMPLES`] times four frames deep, as the speaker negotiates of ALSA. A
//! write is taken up to the room there is, a full queue is `EAGAIN` (the speaker tries again at
//! its next tick), and what a write takes goes to the sink at once, in the one block the write
//! made: the sink is where the sound leaves, so it is told as the voice writes it, which is at
//! the pace the queue lets the voice go. The queue is drained only after [`PlaybackPcm::start`],
//! as the board's is under the speaker's clock mode. It never underruns (the software null
//! sink does not either): an empty queue that was started stays running.
//!
//! `discard` empties the queue and stops it, as `snd_pcm_drop` does; what the sink was already
//! given is out and is not recalled, which is at most the queue's depth, 40 ms.
use crate::{RENDER_RATE, RENDER_SAMPLES, pcm::PcmFormat, pcm::PlaybackPcm};
use alsa::pcm::State;
use std::sync::Mutex;

/// The queue's depth in frames: what the speaker asks of ALSA.
pub const DEPTH_FRAMES: i64 = (RENDER_SAMPLES * 4) as i64;

/// What the speaker needs to know of this queue, as it learns it of ALSA's.
pub const FORMAT: PcmFormat = PcmFormat {
    rate: RENDER_RATE,
    period_frames: RENDER_SAMPLES as i64,
    buffer_frames: DEPTH_FRAMES,
};

struct Inner {
    /// Frames queued as of `as_of_us`.
    queued: i64,
    as_of_us: u64,
    /// What is left of the time since the last whole frame, in microsecond-frames (a frame is
    /// [`MICROS`] of them), so many small looks drain exactly as one long one would.
    carry: u64,
    started: bool,
}

/// Microseconds a second: what one frame's worth of `time * rate` is counted in.
const MICROS: u64 = 1_000_000;

/// Where the accepted frames go: told once per write, with exactly what the queue took.
type Sink = Box<dyn FnMut(&[i16]) + Send>;

pub struct SoftPcm {
    inner: Mutex<Inner>,
    /// Monotonic microseconds.
    clock: Box<dyn Fn() -> u64 + Send>,
    /// Where taken sound goes: called with each write's accepted frames.
    sink: Mutex<Sink>,
}

fn error(func: &'static str, errno: rustix::io::Errno) -> alsa::Error {
    alsa::Error::new(func, errno.raw_os_error())
}

impl SoftPcm {
    /// A queue on the real clock.
    pub fn real_time(sink: impl FnMut(&[i16]) + Send + 'static) -> Self {
        Self::with_clock(lamp_ipc::monotonic_us, sink)
    }

    /// A queue on `clock` (monotonic microseconds), for tests and for a caller with its own.
    pub fn with_clock(
        clock: impl Fn() -> u64 + Send + 'static,
        sink: impl FnMut(&[i16]) + Send + 'static,
    ) -> Self {
        let now = clock();
        Self {
            inner: Mutex::new(Inner {
                queued: 0,
                as_of_us: now,
                carry: 0,
                started: false,
            }),
            clock: Box::new(clock),
            sink: Mutex::new(Box::new(sink)),
        }
    }

    /// The queue as of now: what the clock has drained is gone.
    fn settled(&self) -> std::sync::MutexGuard<'_, Inner> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let now = (self.clock)();
        if inner.started && inner.queued > 0 {
            let elapsed = now.saturating_sub(inner.as_of_us);
            let owed = elapsed * u64::from(RENDER_RATE) + inner.carry;
            let drained = (owed / MICROS) as i64;
            inner.carry = owed % MICROS;
            inner.as_of_us = now;
            inner.queued = (inner.queued - drained).max(0);
            if inner.queued == 0 {
                inner.carry = 0;
            }
        } else {
            inner.as_of_us = now;
            inner.carry = 0;
        }
        inner
    }
}

impl PlaybackPcm for SoftPcm {
    fn avail_delay(&self) -> alsa::Result<(i64, i64)> {
        let inner = self.settled();
        Ok((DEPTH_FRAMES - inner.queued, inner.queued))
    }

    fn start(&self) -> alsa::Result<()> {
        self.settled().started = true;
        Ok(())
    }

    fn writei(&self, samples: &[i16]) -> alsa::Result<usize> {
        let taken = {
            let mut inner = self.settled();
            let room = (DEPTH_FRAMES - inner.queued).max(0) as usize;
            let taken = room.min(samples.len());
            if taken == 0 && !samples.is_empty() {
                return Err(error("snd_pcm_writei", rustix::io::Errno::AGAIN));
            }
            inner.queued += taken as i64;
            taken
        };
        if taken > 0 {
            (self.sink.lock().unwrap_or_else(|p| p.into_inner()))(&samples[..taken]);
        }
        Ok(taken)
    }

    fn drain(&self) -> alsa::Result<()> {
        if self.settled().queued > 0 {
            return Err(error("snd_pcm_drain", rustix::io::Errno::AGAIN));
        }
        Ok(())
    }

    fn prepare(&self) -> alsa::Result<()> {
        let mut inner = self.settled();
        inner.queued = 0;
        inner.started = false;
        Ok(())
    }

    fn state(&self) -> State {
        if self.settled().started {
            State::Running
        } else {
            State::Prepared
        }
    }

    fn discard(&self) -> alsa::Result<()> {
        let mut inner = self.settled();
        inner.queued = 0;
        inner.started = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    fn queue() -> (SoftPcm, Arc<AtomicU64>, Arc<Mutex<Vec<usize>>>) {
        let now = Arc::new(AtomicU64::new(1_000_000));
        let blocks = Arc::new(Mutex::new(Vec::new()));
        let (clock, seen) = (now.clone(), blocks.clone());
        let pcm = SoftPcm::with_clock(
            move || clock.load(Ordering::SeqCst),
            move |frames| seen.lock().unwrap().push(frames.len()),
        );
        (pcm, now, blocks)
    }

    #[test]
    fn a_write_is_taken_to_the_room_there_is_and_told_to_the_sink_once() {
        let (pcm, _now, blocks) = queue();
        assert_eq!(pcm.writei(&[7; 240]).unwrap(), 240);
        assert_eq!(pcm.writei(&[7; 240]).unwrap(), 240);
        assert_eq!(pcm.writei(&[7; 240]).unwrap(), 240);
        assert_eq!(pcm.writei(&[7; 240]).unwrap(), 240);
        // Full: not a fault, the speaker's `EAGAIN` to try again.
        let full = pcm.writei(&[7; 240]).unwrap_err();
        assert_eq!(full.errno(), rustix::io::Errno::AGAIN.raw_os_error());
        assert_eq!(*blocks.lock().unwrap(), vec![240, 240, 240, 240]);
        assert_eq!(pcm.avail_delay().unwrap(), (0, DEPTH_FRAMES));
    }

    #[test]
    fn a_started_queue_drains_at_the_rendering_rate_and_an_unstarted_one_does_not() {
        let (pcm, now, _blocks) = queue();
        pcm.writei(&[1; 480]).unwrap();
        now.fetch_add(100_000, Ordering::SeqCst);
        // Not started: nothing drains, however long it waits.
        assert_eq!(pcm.avail_delay().unwrap().1, 480);
        assert_eq!(pcm.state(), State::Prepared);
        pcm.start().unwrap();
        assert_eq!(pcm.state(), State::Running);
        now.fetch_add(10_000, Ordering::SeqCst);
        // 10 ms at 24 kHz is 240 frames.
        assert_eq!(pcm.avail_delay().unwrap(), (DEPTH_FRAMES - 240, 240));
        // `drain` is `EAGAIN` while it plays and done when it is empty.
        assert_eq!(
            pcm.drain().unwrap_err().errno(),
            rustix::io::Errno::AGAIN.raw_os_error()
        );
        now.fetch_add(10_000, Ordering::SeqCst);
        pcm.drain().unwrap();
        // Empty and started stays running: it never underruns.
        now.fetch_add(1_000_000, Ordering::SeqCst);
        assert_eq!(pcm.state(), State::Running);
        assert_eq!(pcm.avail_delay().unwrap(), (DEPTH_FRAMES, 0));
    }

    #[test]
    fn a_fraction_of_a_frame_is_never_lost_to_many_small_looks() {
        let (pcm, now, _blocks) = queue();
        pcm.writei(&[1; 480]).unwrap();
        pcm.start().unwrap();
        // 1 ms is 24 frames; 7 ms in 700 looks of 10 us each is 168 frames, however it is cut up.
        for _ in 0..700 {
            now.fetch_add(10, Ordering::SeqCst);
            pcm.avail_delay().unwrap();
        }
        assert_eq!(pcm.avail_delay().unwrap().1, 480 - 168);
    }

    #[test]
    fn discard_and_prepare_empty_and_stop_it() {
        let (pcm, _now, _blocks) = queue();
        pcm.writei(&[1; 480]).unwrap();
        pcm.start().unwrap();
        pcm.discard().unwrap();
        assert_eq!(pcm.avail_delay().unwrap(), (DEPTH_FRAMES, 0));
        assert_eq!(pcm.state(), State::Prepared);
        pcm.writei(&[1; 240]).unwrap();
        pcm.prepare().unwrap();
        assert_eq!(pcm.avail_delay().unwrap(), (DEPTH_FRAMES, 0));
    }

    #[test]
    fn its_format_is_one_the_speaker_accepts_of_alsa() {
        // The speaker's own bounds on what ALSA negotiates (`open_mode`): the rate, a period no
        // longer than a block, a buffer of at most four and, for the clock mode, at least two.
        assert_eq!(FORMAT.rate, RENDER_RATE);
        assert!(FORMAT.period_frames > 0 && FORMAT.period_frames <= RENDER_SAMPLES as i64);
        assert!(FORMAT.buffer_frames >= (RENDER_SAMPLES * 2) as i64);
        assert!(FORMAT.buffer_frames <= (RENDER_SAMPLES * 4) as i64);
    }
}
