//! Accepted-render continuity and bounded DSP call accounting. These cursors are
//! not physical capture timestamps: the two USB devices have separate clocks.
//! No missing packet is interpreted as silence, and no samples are invented.
//!
//! Two ways of keeping the reference beside the capture:
//!
//! - **Fixed margin**, the default. Every accepted block goes to the DSP as it
//!   arrives, and the count of render blocks must stay between one and eight
//!   ahead of the capture blocks. The clocks differ, so that count drifts and
//!   a long session ends in `ReferenceLate` or `ReferenceBacklog`.
//! - **Capture-clocked**, opt-in through [`ReferenceAssembly::capture_clocked`].
//!   Accepted frames wait in a bounded ring and one render block is read from
//!   it for each capture block, at a rate that follows the difference between
//!   the clocks within `lamp_audio::drift::MAX_PPM`. Packet order, cursor
//!   continuity, privacy, freshness and both bounds fault exactly as before.
//!
//! The capture-clocked reference can also be given bounded recovery,
//! [`ReferenceAssembly::recovering`]: see [`recovery`].
pub mod hold;
mod prime;
pub use prime::PrimeBarrier;
mod recovery;
pub use recovery::Policy as RecoveryPolicy;

use crate::wire::{
    ClockPrimed, PreparedPrime, PrimeDisposition, ReferenceDrift, ReferenceFault,
    ReferenceFaultKind as Kind, ReferenceTiming, RenderPayload, RenderReference,
};
use lamp_audio::{
    CAPTURE_RATE, EchoProcessor, RENDER_RATE, RENDER_SAMPLES, RenderBlock,
    drift::{Controller, Resampler, TARGET_FRAMES},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const PRIME_FRAMES: usize = RENDER_SAMPLES * 2;
const MAX_REFERENCE_AGE_US: u64 = 40_000;
const MAX_PARTIAL_US: u64 = 50_000;
const MAX_AHEAD_BLOCKS: u64 = 8;
const MAX_CAPTURE_BEFORE_START_NOTICE: usize = PRIME_FRAMES / RENDER_SAMPLES;
/// How many of the speaker's first queue looks are kept for the line a `ReferenceLate` fault says
/// (a diagnostic only: nothing decides on them). 32 looks are 32 accepted render blocks, 320 ms.
const QUEUE_LOOKS: usize = 32;

/// One accepted reference packet's queue look: when it was accepted, when the speaker looked at
/// its queue, and how many frames it held then (all host monotonic microseconds).
#[derive(Clone, Copy, Default)]
struct QueueLook {
    accepted_at_us: u64,
    observed_at_us: u64,
    queued_frames: usize,
}
/// Capture-clocked only: the ring starts with this much of the silence that
/// preceded the stream ahead of the speaker's prime, so that it starts at its
/// target. Before its first accepted frame the speaker put out nothing; the
/// frames are true history, not filler, and are never counted as accepted.
const LEAD_FRAMES: usize = TARGET_FRAMES - PRIME_FRAMES;

struct CaptureClock {
    resampler: Resampler,
    controller: Controller,
}

/// The capture-clocked reference at this moment. Evidence, not a verdict.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drift {
    /// Read rate in force, parts per million above one frame per frame.
    pub ppm: f64,
    /// The settled part: how much faster the speaker accepts than the
    /// microphone delivers, as far as it has been learned.
    pub steady_ppm: f64,
    /// Accepted frames waiting in the ring.
    pub unread_frames: usize,
    /// Render blocks read from the ring for the DSP.
    pub rendered_blocks: u64,
}

pub struct ReferenceAssembly {
    timing: ReferenceTiming,
    sequence: u64,
    privacy_generation: u64,
    samples: RenderBlock,
    len: usize,
    partial_since: u64,
    prime_observed_at_us: u64,
    // A restart does not stop the microphone. Keep only the two host read
    // completions permitted by the certified prime until ClockStarted arrives.
    // Classify them against the conservative start-call request bound, never
    // against call completion or message receipt time.
    captures_before_start_notice: [u64; MAX_CAPTURE_BEFORE_START_NOTICE],
    captures_before_start_notice_len: usize,
    last_capture_read_at_us: Option<u64>,
    clock: Option<Box<CaptureClock>>,
    /// Capture-clocked: the accepted-frame cursor of the first frame the
    /// ring was given since it was last emptied. Negative by the lead while
    /// the ring still starts with the silence before the stream.
    ring_origin: i64,
    /// Fixed margin with recovery: capture blocks by which the count the
    /// margin is measured against has been moved, in all. Every move is a
    /// reported reseed; no other counter changes.
    margin_rebase: i64,
    recovery: Option<Box<recovery::State>>,
    /// The first [`QUEUE_LOOKS`] queue looks, kept for [`Self::late`]'s line; never read otherwise.
    queue_looks: [QueueLook; QUEUE_LOOKS],
    queue_looks_len: usize,
}

impl Default for ReferenceAssembly {
    fn default() -> Self {
        Self {
            timing: ReferenceTiming::default(),
            sequence: 0,
            privacy_generation: 0,
            samples: [0; RENDER_SAMPLES],
            len: 0,
            partial_since: 0,
            prime_observed_at_us: 0,
            captures_before_start_notice: [0; MAX_CAPTURE_BEFORE_START_NOTICE],
            captures_before_start_notice_len: 0,
            last_capture_read_at_us: None,
            clock: None,
            ring_origin: 0,
            margin_rebase: 0,
            recovery: None,
            queue_looks: [QueueLook::default(); QUEUE_LOOKS],
            queue_looks_len: 0,
        }
    }
}

/// Post-write authority checks may physically discard an accepted write. Its
/// old cursors are evidence of loss, never permission to relabel it after reset.
pub fn accepted_write_survived(
    first: lamp_audio::ownership::PlaybackCursor,
    end: lamp_audio::ownership::PlaybackCursor,
    current: lamp_audio::ownership::QueuedRange,
    frames: usize,
) -> std::io::Result<bool> {
    if first.epoch == 0
        || first.epoch != end.epoch
        || frames > RENDER_SAMPLES
        || first.frame.checked_add(frames as u64) != Some(end.frame)
        || end.epoch > current.epoch
        || (end.epoch == current.epoch && end.frame > current.accepted_through)
    {
        return Err(std::io::Error::other(
            "invalid accepted-write cursor accounting",
        ));
    }
    Ok(end.epoch == current.epoch)
}

impl ReferenceAssembly {
    /// The opt-in reference that follows the capture clock. Call
    /// [`Self::render_for_capture`] between `before_capture` and the DSP's
    /// capture call.
    pub fn capture_clocked() -> Self {
        Self {
            clock: Some(Box::new(CaptureClock {
                resampler: Resampler::new(),
                controller: Controller::default(),
            })),
            ..Self::default()
        }
    }

    /// `None` for the fixed margin.
    pub fn drift(&self) -> Option<Drift> {
        self.clock.as_ref().map(|clock| Drift {
            ppm: clock.resampler.ppm(),
            steady_ppm: clock.controller.steady_ppm(),
            unread_frames: clock.resampler.unread(),
            rendered_blocks: clock.resampler.taken_blocks(),
        })
    }

    pub fn timing(&self) -> ReferenceTiming {
        ReferenceTiming {
            recovery: self.recovery_counts(),
            drift: self.drift().map(|drift| ReferenceDrift {
                rate_ppm: drift.ppm as f32,
                learned_ppm: drift.steady_ppm as f32,
                unread_frames: u32::try_from(drift.unread_frames).unwrap_or(u32::MAX),
            }),
            ..self.timing
        }
    }

    fn note_queue_look(&mut self) {
        if self.queue_looks_len < QUEUE_LOOKS {
            self.queue_looks[self.queue_looks_len] = QueueLook {
                accepted_at_us: self.timing.accepted_at_us,
                observed_at_us: self.timing.queue_observed_at_us,
                queued_frames: self.timing.queued_frames,
            };
            self.queue_looks_len += 1;
        }
    }

    /// The speaker's first queue looks as one line, in ms from the first accept: `accepted/observed/queued`
    /// for each. What a `ReferenceLate` on a Mac needs read (the queue ran out: how deep did it sit?).
    fn queue_looks_line(&self) -> Option<String> {
        let first = self
            .queue_looks
            .first()
            .filter(|_| self.queue_looks_len > 0)?
            .accepted_at_us;
        let looks: Vec<String> = self.queue_looks[..self.queue_looks_len]
            .iter()
            .map(|look| {
                format!(
                    "{:.1}/{:.1}/{}",
                    look.accepted_at_us.saturating_sub(first) as f64 / 1000.0,
                    look.observed_at_us.saturating_sub(first) as f64 / 1000.0,
                    look.queued_frames
                )
            })
            .collect();
        Some(format!(
            "lamp-voice: reference queue looks (ms from the first accept, accepted/observed/queued frames; the first {}): {}",
            self.queue_looks_len,
            looks.join(" ")
        ))
    }

    /// A `ReferenceLate` fault, and on stderr the queue looks that led to it: a diagnostic, no
    /// decision depends on them.
    fn late(&self, now: u64) -> ReferenceFault {
        if let Some(line) = self.queue_looks_line() {
            eprintln!("{line}");
        }
        self.fault(Kind::ReferenceLate, now)
    }

    pub fn fault(&self, reason: Kind, now: u64) -> ReferenceFault {
        ReferenceFault {
            reason,
            at_us: now,
            expected_sequence: self.sequence.saturating_add(1),
            received_sequence: None,
            received_first_sample: None,
            timing: self.timing(),
        }
    }

    pub fn prime(
        &mut self,
        prime: ClockPrimed,
        now: u64,
        privacy: u64,
        echo: &mut EchoProcessor,
    ) -> Result<PreparedPrime> {
        let previous_epoch = self.timing.playback_epoch;
        let reset_valid = match (self.timing.playback_epoch, prime.discarded) {
            (0, None) => prime.playback_epoch == 1,
            (epoch, Some(old)) => {
                old.epoch == epoch
                    && old.retired_through <= old.accepted_through
                    && old.accepted_through >= self.timing.accepted_through
                    && epoch.checked_add(1) == Some(prime.playback_epoch)
            }
            _ => false,
        };
        if !reset_valid
            || privacy == 0
            || prime.privacy_generation != privacy
            || prime.accepted_zero_frames != PRIME_FRAMES
            || prime.queued_frames != PRIME_FRAMES
            || prime.accepted_at_us > prime.queue_observed_at_us
            || !fresh(prime.accepted_at_us, now, 100_000)
            || !fresh(prime.queue_observed_at_us, now, 100_000)
        {
            return Err(self.fault(Kind::InvalidPrime, now).into());
        }
        // Only the speaker's certified, actually accepted prime reaches here.
        // This resets acoustic adaptation; silence does not train room response.
        echo.reset();
        let analysed = match self.clock.as_mut() {
            None => {
                for _ in 0..2 {
                    echo.render(&[0; RENDER_SAMPLES])?;
                }
                PRIME_FRAMES as u64
            }
            Some(clock) => {
                // The prime waits in the ring with the frames that follow it.
                self.ring_origin = -(LEAD_FRAMES as i64);
                clock.resampler.reset();
                clock.controller.reset();
                clock
                    .resampler
                    .push(&[0; LEAD_FRAMES + PRIME_FRAMES])
                    .map_err(|_| self.fault(Kind::InvalidPrime, now))?;
                0
            }
        };
        self.timing = ReferenceTiming {
            playback_epoch: prime.playback_epoch,
            accepted_through: PRIME_FRAMES as u64,
            analysed_through: analysed,
            accepted_at_us: prime.accepted_at_us,
            analysed_at_us: now,
            queue_observed_at_us: prime.queue_observed_at_us,
            queued_frames: prime.queued_frames,
            ..ReferenceTiming::default()
        };
        self.sequence = 0;
        self.margin_rebase = 0;
        self.privacy_generation = privacy;
        self.prime_observed_at_us = prime.queue_observed_at_us;
        self.captures_before_start_notice.fill(0);
        self.captures_before_start_notice_len = 0;
        // The looks are of this epoch's start, as the sequence is.
        self.queue_looks_len = 0;
        self.clear_partial();
        if self.recovery.is_some() {
            self.primed_recovering(previous_epoch, now)?;
        }
        Ok(PreparedPrime {
            playback_epoch: prime.playback_epoch,
            privacy_generation: privacy,
            prepared_through: self.timing.accepted_through,
            analysed_through: self.timing.analysed_through,
            disposition: if self.clock.is_some() {
                PrimeDisposition::BufferedForCapture
            } else {
                PrimeDisposition::Analysed
            },
        })
    }

    pub fn started(
        &mut self,
        epoch: u64,
        requested_at: u64,
        completed_at: u64,
        now: u64,
    ) -> Result<()> {
        if epoch == 0
            || epoch != self.timing.playback_epoch
            || self.timing.clock_started_at_us.is_some()
            || requested_at < self.prime_observed_at_us
            || requested_at > completed_at
            || !fresh(requested_at, now, MAX_REFERENCE_AGE_US)
            || !fresh(completed_at, now, MAX_REFERENCE_AGE_US)
        {
            return Err(self.fault(Kind::InvalidStart, now).into());
        }
        let origin = self.captures_before_start_notice[..self.captures_before_start_notice_len]
            .iter()
            .filter(|read_at| **read_at < requested_at)
            .count() as u64;
        // Capture-clocked, the ring's own capacity is the backlog bound.
        if self.clock.is_none()
            && (self.timing.accepted_through / RENDER_SAMPLES as u64)
                .saturating_sub(self.timing.capture_blocks_processed - origin)
                > MAX_AHEAD_BLOCKS
        {
            return Err(self.fault(Kind::ReferenceBacklog, now).into());
        }
        self.timing.capture_blocks_before_clock_start = origin;
        self.timing.clock_start_requested_at_us = Some(requested_at);
        self.timing.clock_started_at_us = Some(completed_at);
        self.captures_before_start_notice.fill(0);
        self.captures_before_start_notice_len = 0;
        Ok(())
    }

    /// Returns false only for a packet from a previous, explicitly discarded
    /// epoch that was already queued on the separate data socket at reset time.
    pub fn accept(
        &mut self,
        reference: RenderReference,
        now: u64,
        privacy: u64,
        echo: &mut EchoProcessor,
    ) -> Result<bool> {
        if reference.playback_epoch > 0 && reference.playback_epoch < self.timing.playback_epoch {
            self.timing.ignored_old_epoch_packets =
                self.timing.ignored_old_epoch_packets.saturating_add(1);
            return Ok(false);
        }
        // Starting a stream is not recovered: until the clock has started
        // every rule below is in force.
        if self.recovery.is_some()
            && self.clock.is_some()
            && self.timing.clock_started_at_us.is_some()
        {
            return self.accept_recovering(reference, now, privacy);
        }
        let count = reference.payload.frames();
        let reason = if reference.playback_epoch == 0
            || reference.playback_epoch != self.timing.playback_epoch
            || count == 0
            || count > RENDER_SAMPLES
        {
            Some(Kind::InvalidReference)
        } else if reference.privacy_generation != self.privacy_generation
            || privacy != self.privacy_generation
        {
            Some(Kind::PrivacyMismatch)
        } else if self.sequence.checked_add(1) != Some(reference.sequence) {
            Some(Kind::SequenceGap)
        } else if reference.first_sample != self.timing.accepted_through {
            Some(Kind::CursorGap)
        } else if reference.queued_frames > 960
            || reference.accepted_at_us < self.timing.accepted_at_us
            || reference.accepted_at_us > reference.queue_observed_at_us
            || !fresh(reference.accepted_at_us, now, MAX_REFERENCE_AGE_US)
            || !fresh(reference.queue_observed_at_us, now, MAX_REFERENCE_AGE_US)
        {
            Some(Kind::QueueTiming)
        } else {
            None
        };
        if let Some(reason) = reason {
            let mut fault = self.fault(reason, now);
            fault.received_sequence = Some(reference.sequence);
            fault.received_first_sample = Some(reference.first_sample);
            return Err(fault.into());
        }
        let accepted = self
            .timing
            .accepted_through
            .checked_add(count as u64)
            .ok_or_else(|| self.fault(Kind::CursorGap, now))?;
        let zeros = [0; RENDER_SAMPLES];
        let samples = match &reference.payload {
            RenderPayload::Silence { .. } => &zeros[..count],
            RenderPayload::Speech { samples, .. } => samples.as_slice(),
        };
        if let Some(clock) = self.clock.as_mut() {
            // Every accepted frame waits for the capture block that reads it.
            if clock.resampler.push(samples).is_err() {
                return Err(self.fault(Kind::ReferenceBacklog, now).into());
            }
            self.sequence = reference.sequence;
            self.timing.accepted_through = accepted;
            self.timing.accepted_at_us = reference.accepted_at_us;
            self.timing.queue_observed_at_us = reference.queue_observed_at_us;
            self.timing.queued_frames = reference.queued_frames;
            self.note_queue_look();
            return Ok(true);
        }
        let full_blocks = accepted / RENDER_SAMPLES as u64;
        let ahead = full_blocks.saturating_sub(self.running_capture_blocks());
        // With recovery the bound may be passed by a few blocks until the
        // next capture block, which is the only place it can be told whether
        // the clocks did it or the capture loop fell behind. Nothing is
        // decided here on what an earlier capture block said.
        if ahead > MAX_AHEAD_BLOCKS && !self.awaits_capture(ahead) {
            return Err(self.fault(Kind::ReferenceBacklog, now).into());
        }
        let mut offset = 0;
        while offset < samples.len() {
            if self.len == 0 {
                self.partial_since = now;
            }
            let copied = (samples.len() - offset).min(RENDER_SAMPLES - self.len);
            self.samples[self.len..self.len + copied]
                .copy_from_slice(&samples[offset..offset + copied]);
            self.len += copied;
            offset += copied;
            if self.len == RENDER_SAMPLES {
                echo.render(&self.samples)?;
                self.remember_rendered();
                self.timing.analysed_through += RENDER_SAMPLES as u64;
                self.timing.analysed_at_us = now;
                self.clear_partial();
            }
        }
        self.sequence = reference.sequence;
        self.timing.accepted_through = accepted;
        self.timing.accepted_at_us = reference.accepted_at_us;
        self.timing.queue_observed_at_us = reference.queue_observed_at_us;
        self.timing.queued_frames = reference.queued_frames;
        self.note_queue_look();
        Ok(true)
    }

    /// Call after draining direct reference, immediately before processing every
    /// retained capture block. This never sleeps for reference or pads a gap.
    /// The balance is a software buffer invariant, not oscillator synchronization.
    pub fn before_capture(&self, capture_delayed_frames: i64, now: u64) -> Result<u16> {
        let latest_clock_evidence = self
            .timing
            .accepted_at_us
            .max(self.timing.clock_started_at_us.unwrap_or(0));
        let supplied = match &self.clock {
            None => {
                self.timing.analysed_through / RENDER_SAMPLES as u64 > self.running_capture_blocks()
            }
            Some(clock) => clock.resampler.ready(),
        };
        if self.timing.playback_epoch == 0
            || !fresh(latest_clock_evidence, now, MAX_REFERENCE_AGE_US)
            || !supplied
            || (self.timing.clock_started_at_us.is_none()
                && self.captures_before_start_notice_len == MAX_CAPTURE_BEFORE_START_NOTICE)
        {
            return Err(self.late(now).into());
        }
        if self.len > 0 && now.saturating_sub(self.partial_since) >= MAX_PARTIAL_US {
            return Err(self.fault(Kind::PartialExpired, now).into());
        }
        if !(0..=640).contains(&capture_delayed_frames) {
            return Err(self.fault(Kind::QueueTiming, now).into());
        }
        // Nominal-rate interpolation of the last queue observation, explicitly
        // not measured speaker/ADC time or proof of two-USB-clock synchronization.
        let elapsed = now.saturating_sub(
            self.timing
                .queue_observed_at_us
                .max(self.timing.clock_start_requested_at_us.unwrap_or(0)),
        );
        let consumed = (u128::from(elapsed) * u128::from(RENDER_RATE) / 1_000_000)
            .min(usize::MAX as u128) as usize;
        let render_frames = self.timing.queued_frames.saturating_sub(consumed);
        if self.timing.clock_started_at_us.is_some() && render_frames == 0 {
            // An origin offset is not credit for an exhausted observed queue.
            // Fail on nominal starvation; never manufacture a silence block.
            return Err(self.late(now).into());
        }
        // Capture-clocked, the block about to be rendered was accepted a
        // ring's length ago and is that much nearer to being heard.
        let waited = self
            .clock
            .as_ref()
            .map_or(0, |clock| clock.resampler.unread());
        let delay = render_frames.saturating_sub(waited) as u64 * 1_000 / u64::from(RENDER_RATE)
            + capture_delayed_frames as u64 * 1_000 / u64::from(CAPTURE_RATE);
        Ok(delay as u16)
    }

    /// Capture-clocked: read this capture block's render block from the ring
    /// and give it to the DSP, then let the controller see what is left.
    /// With the fixed margin the DSP already has it and this does nothing.
    /// Call after `before_capture` has succeeded.
    pub fn render_for_capture(&mut self, now: u64, echo: &mut EchoProcessor) -> Result<()> {
        let Some(clock) = self.clock.as_mut() else {
            return Ok(());
        };
        let mut block = [0; RENDER_SAMPLES];
        if clock.resampler.take(&mut block).is_err() {
            return Err(self.late(now).into());
        }
        echo.render(&block)?;
        // Frames the speaker has accepted since its last packet, at the
        // nominal rate and for at most two blocks: with them the fill is a
        // steady figure and not a sawtooth one packet wide.
        let since = now.saturating_sub(self.timing.accepted_at_us).min(20_000);
        let owed = since as f64 * f64::from(RENDER_RATE) / 1e6;
        let ppm = clock
            .controller
            .observe(clock.resampler.unread() as f64 + owed);
        clock.resampler.set_ppm(ppm);
        // Accepted frames read past. The lead was never an accepted frame.
        self.timing.analysed_through = clock
            .resampler
            .consumed()
            .saturating_sub(LEAD_FRAMES as u64);
        self.timing.analysed_at_us = now;
        Ok(())
    }

    pub fn captured(&mut self, read_completed_at_us: u64, now: u64) -> Result<()> {
        if self.timing.playback_epoch == 0
            || read_completed_at_us > now
            || self
                .last_capture_read_at_us
                .is_some_and(|previous| read_completed_at_us <= previous)
        {
            return Err(self.fault(Kind::CaptureTiming, now).into());
        }
        let processed = self
            .timing
            .capture_blocks_processed
            .checked_add(1)
            .ok_or_else(|| self.fault(Kind::CaptureTiming, now))?;
        if self.timing.clock_started_at_us.is_none() {
            if self.captures_before_start_notice_len == MAX_CAPTURE_BEFORE_START_NOTICE {
                return Err(self.late(now).into());
            }
            self.captures_before_start_notice[self.captures_before_start_notice_len] =
                read_completed_at_us;
            self.captures_before_start_notice_len += 1;
        }
        self.timing.capture_blocks_processed = processed;
        self.last_capture_read_at_us = Some(read_completed_at_us);
        Ok(())
    }

    fn running_capture_blocks(&self) -> u64 {
        // This is a one-time phase origin, not a claim that the USB clocks are
        // synchronized. Total DSP calls remain separately observable and drift
        // after start still consumes the existing bounded reference lead.
        // With recovery, each reported re-centre moves the count by what it
        // says; without, the rebase is zero.
        (self.timing.capture_blocks_processed - self.timing.capture_blocks_before_clock_start)
            .saturating_add_signed(self.margin_rebase)
    }

    fn clear_partial(&mut self) {
        self.samples.fill(0);
        self.len = 0;
        self.partial_since = 0;
    }
}

fn fresh(at: u64, now: u64, maximum: u64) -> bool {
    now.checked_sub(at).is_some_and(|age| age < maximum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_audio::ownership::QueuedRange;

    /// The diagnostic line a `ReferenceLate` says: the first 32 looks only, in ms from the first
    /// accept, and nothing at all before the first look. It decides nothing.
    #[test]
    fn the_queue_looks_line_holds_the_first_looks_in_ms_from_the_first_accept() {
        let mut state = ReferenceAssembly::default();
        assert!(state.queue_looks_line().is_none());
        for look in 0..40u64 {
            state.timing.accepted_at_us = 5_000_000 + look * 10_000;
            state.timing.queue_observed_at_us = 5_000_000 + look * 10_000 + 2_500;
            state.timing.queued_frames = 240 + look as usize;
            state.note_queue_look();
        }
        let line = state.queue_looks_line().unwrap();
        assert!(line.contains("the first 32)"), "{line}");
        let looks: Vec<&str> = line.rsplit(": ").next().unwrap().split(' ').collect();
        assert_eq!(looks.len(), 32);
        assert_eq!(looks[0], "0.0/2.5/240");
        assert_eq!(looks[31], "310.0/312.5/271");
    }

    #[test]
    fn a_write_discarded_during_its_post_write_check_is_never_new_epoch_reference() {
        use lamp_audio::ownership::PlaybackCursor;
        let first = PlaybackCursor {
            epoch: 1,
            frame: 480,
        };
        let end = PlaybackCursor {
            epoch: 1,
            frame: 720,
        };
        let after = QueuedRange {
            epoch: 2,
            retired_through: 0,
            accepted_through: 480,
        };
        assert!(!accepted_write_survived(first, end, after, 240).unwrap());
        let current = QueuedRange {
            epoch: 1,
            retired_through: 500,
            accepted_through: 720,
        };
        assert!(accepted_write_survived(first, end, current, 240).unwrap());
        assert!(accepted_write_survived(first, end, current, 239).is_err());
        assert!(
            accepted_write_survived(
                first,
                PlaybackCursor {
                    epoch: 2,
                    frame: 720
                },
                after,
                240
            )
            .is_err()
        );
    }
    fn prime(epoch: u64, discarded: Option<QueuedRange>) -> ClockPrimed {
        ClockPrimed {
            playback_epoch: epoch,
            privacy_generation: 2,
            accepted_zero_frames: 480,
            accepted_at_us: 10_000,
            queue_observed_at_us: 10_001,
            queued_frames: 480,
            discarded,
        }
    }
    fn reference(sequence: u64, first: u64, frames: u16) -> RenderReference {
        RenderReference {
            playback_epoch: 1,
            privacy_generation: 2,
            sequence,
            first_sample: first,
            accepted_at_us: 10_100,
            queue_observed_at_us: 10_101,
            queued_frames: 480,
            payload: RenderPayload::Silence { frames },
        }
    }
    #[test]
    fn no_reference_without_certified_prime_and_empty_link_is_not_silence() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        assert!(state.before_capture(0, 10_002).is_err());
        let mut wrong = prime(1, None);
        wrong.accepted_zero_frames = 240;
        assert!(state.prime(wrong, 10_002, 2, &mut echo).is_err());
        assert!(state.prime(prime(1, None), 10_002, 3, &mut echo).is_err());
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        state.started(1, 10_003, 10_003, 10_004).unwrap();
        for index in 0..2 {
            state.before_capture(0, 10_010 + index).unwrap();
            state.captured(10_010 + index, 10_010 + index).unwrap();
        }
        let error = state.before_capture(0, 10_011).unwrap_err();
        assert_eq!(
            error.downcast_ref::<ReferenceFault>().unwrap().reason,
            Kind::ReferenceLate
        );
        assert_eq!(state.timing.analysed_through, 480);
    }
    #[test]
    fn partial_writes_keep_exact_cursor_and_duplicate_overlap_gap_never_reach_dsp() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        state
            .accept(reference(1, 480, 73), 10_102, 2, &mut echo)
            .unwrap();
        assert_eq!(state.len, 73);
        assert_eq!(state.timing.analysed_through, 480);
        for packet in [
            reference(1, 553, 167),
            reference(2, 552, 167),
            reference(3, 553, 167),
        ] {
            assert!(state.accept(packet, 10_102, 2, &mut echo).is_err());
        }
        assert_eq!(state.len, 73);
        state
            .accept(reference(2, 553, 167), 10_102, 2, &mut echo)
            .unwrap();
        assert_eq!(state.timing.analysed_through, 720);
        assert_eq!(state.len, 0);
    }
    #[test]
    fn reset_drops_unplayed_tail_and_delayed_old_packets_cannot_enter_new_epoch() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        state
            .accept(reference(1, 480, 73), 10_102, 2, &mut echo)
            .unwrap();
        state.captured(10_102, 10_102).unwrap();
        let old = QueuedRange {
            epoch: 1,
            retired_through: 100,
            accepted_through: 553,
        };
        let mut reset = prime(2, Some(old));
        reset.accepted_at_us = 10_200;
        reset.queue_observed_at_us = 10_201;
        state.prime(reset, 10_202, 2, &mut echo).unwrap();
        assert_eq!(state.len, 0);
        assert_eq!(state.timing.capture_blocks_processed, 0);
        assert!(
            !state
                .accept(reference(2, 553, 100), 10_203, 2, &mut echo)
                .unwrap()
        );
        assert_eq!(state.timing.accepted_through, 480);
        assert_eq!(state.timing.ignored_old_epoch_packets, 1);
        assert!(state.started(1, 10_204, 10_204, 10_205).is_err());
        state.started(2, 10_204, 10_204, 10_205).unwrap();
    }
    #[test]
    fn excess_reference_backlog_and_nominal_clock_starvation_are_explicit_faults() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        for sequence in 1..=6 {
            state
                .accept(
                    reference(sequence, 480 + (sequence - 1) * 240, 240),
                    10_102,
                    2,
                    &mut echo,
                )
                .unwrap();
        }
        let error = state
            .accept(reference(7, 1920, 240), 10_102, 2, &mut echo)
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<ReferenceFault>().unwrap().reason,
            Kind::ReferenceBacklog
        );
        assert!(state.before_capture(0, 50_100).is_err());
        assert!(state.before_capture(641, 10_102).is_err());
    }

    fn assert_fault(result: Result<impl Sized>, reason: Kind) {
        let error = match result {
            Ok(_) => panic!("expected {reason:?}"),
            Err(error) => error,
        };
        assert_eq!(
            error.downcast_ref::<ReferenceFault>().unwrap().reason,
            reason
        );
    }

    fn capture(state: &mut ReferenceAssembly, read_at: u64) {
        state.before_capture(0, read_at).unwrap();
        state.captured(read_at, read_at + 1).unwrap();
    }

    /// The looks are of the epoch's start: a new prime (a reference restart) clears them, so a late
    /// fault hours into a run says the looks of the epoch it ends, never of the first one.
    #[test]
    fn a_new_epochs_prime_clears_the_queue_looks() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        let mut packet = reference(1, 480, 240);
        packet.playback_epoch = 1;
        packet.accepted_at_us = 10_010;
        packet.queue_observed_at_us = 10_011;
        packet.queued_frames = 337;
        state.accept(packet, 10_012, 2, &mut echo).unwrap();
        assert_eq!(state.queue_looks_len, 1);
        let mut reset = prime(
            2,
            Some(QueuedRange {
                epoch: 1,
                retired_through: 100,
                accepted_through: 720,
            }),
        );
        reset.accepted_at_us = 10_020;
        reset.queue_observed_at_us = 10_021;
        state.prime(reset, 10_022, 2, &mut echo).unwrap();
        assert_eq!(state.queue_looks_len, 0);
        assert!(state.queue_looks_line().is_none());
    }

    #[test]
    fn restart_origin_preserves_the_observed_queue_at_the_second_cancellation() {
        // Model the relevant host/driver chronology from directed-pilot-fvackatg.
        // That run recorded only start completion. The request below is a
        // simulated bound, not a historical measurement or acoustic timestamp.
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        let mut reset = prime(
            2,
            Some(QueuedRange {
                epoch: 1,
                retired_through: 100,
                accepted_through: 480,
            }),
        );
        reset.accepted_at_us = 177_656_668_319;
        reset.queue_observed_at_us = 177_656_668_320;
        state.prime(reset, 177_656_673_021, 2, &mut echo).unwrap();
        capture(&mut state, 177_656_673_103);
        state
            .started(2, 177_656_674_000, 177_656_674_519, 177_656_674_520)
            .unwrap();
        capture(&mut state, 177_656_680_247);
        for block in 3..=256 {
            let accepted_at = 177_659_215_892 - (256 - block) * 10_000;
            let mut packet = reference(block - 2, (block - 1) * 240, 240);
            packet.playback_epoch = 2;
            packet.accepted_at_us = accepted_at;
            packet.queue_observed_at_us = accepted_at + 8;
            packet.queued_frames = 456;
            state
                .accept(packet, accepted_at + 10, 2, &mut echo)
                .unwrap();
            capture(&mut state, accepted_at + 4_048);
        }
        let before = state.timing();
        assert_eq!(before.capture_blocks_processed, 256);
        assert_eq!(before.capture_blocks_before_clock_start, 1);
        assert_eq!(before.analysed_through, 61_440);
        assert_eq!(state.running_capture_blocks(), 255);

        // The last queue observation still has 112 frames at the nominal rate:
        // 456 - floor(14,342 us * 24,000 / 1,000,000). No new render is accepted.
        let at = 177_659_230_242;
        assert_eq!(at - before.queue_observed_at_us, 14_342);
        assert_eq!(
            before.queued_frames
                - ((at - before.queue_observed_at_us) * 24_000 / 1_000_000) as usize,
            112
        );
        assert_eq!(state.before_capture(0, at).unwrap(), 4);
        state.captured(at, at + 1).unwrap();
        assert_eq!(state.timing().capture_blocks_processed, 257);
        assert_eq!(state.timing().accepted_through, before.accepted_through);
        assert_eq!(state.timing().analysed_through, before.analysed_through);
        // This grants one demonstrated phase correction, never recurring credit.
        assert_fault(state.before_capture(0, at + 1), Kind::ReferenceLate);
    }

    #[test]
    fn delayed_start_notice_classifies_read_completion_against_start_request() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        capture(&mut state, 11_000);
        capture(&mut state, 15_000);
        // Arrival is later than both reads; only the first preceded the request.
        state.started(1, 12_000, 12_000, 20_000).unwrap();
        assert_eq!(state.timing().capture_blocks_processed, 2);
        assert_eq!(state.timing().capture_blocks_before_clock_start, 1);
        assert_eq!(state.running_capture_blocks(), 1);
        capture(&mut state, 21_000);
        assert_eq!(state.timing().capture_blocks_processed, 3);
        assert_fault(state.before_capture(0, 21_001), Kind::ReferenceLate);
        assert_eq!(state.timing().analysed_through, 480);
    }

    #[test]
    fn captures_at_or_after_start_request_never_receive_a_late_notice_exemption() {
        for first_read in [12_000, 13_000] {
            let mut state = ReferenceAssembly::default();
            let mut echo = EchoProcessor::new(false);
            state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
            capture(&mut state, first_read);
            capture(&mut state, 15_000);
            state.started(1, 12_000, 12_000, 20_000).unwrap();
            assert_eq!(state.timing().capture_blocks_before_clock_start, 0);
            assert_fault(state.before_capture(0, 20_001), Kind::ReferenceLate);
        }
    }

    #[test]
    fn future_wrong_epoch_and_duplicate_start_do_not_rebase_counters() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        assert_fault(state.started(0, 10_000, 10_000, 10_001), Kind::InvalidStart);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        capture(&mut state, 11_000);
        assert_fault(state.started(1, 12_000, 12_000, 11_999), Kind::InvalidStart);
        assert_fault(state.started(2, 12_000, 12_000, 12_001), Kind::InvalidStart);
        assert_fault(state.started(1, 9_999, 9_999, 12_001), Kind::InvalidStart);
        assert_eq!(state.timing().clock_started_at_us, None);
        assert_eq!(state.timing().capture_blocks_before_clock_start, 0);
        state.started(1, 12_000, 12_000, 12_001).unwrap();
        capture(&mut state, 13_000);
        for at in [12_000, 14_000] {
            assert_fault(state.started(1, at, at, 14_001), Kind::InvalidStart);
        }
        assert_eq!(state.timing().clock_started_at_us, Some(12_000));
        assert_eq!(state.timing().capture_blocks_before_clock_start, 1);
        assert_eq!(state.timing().capture_blocks_processed, 2);
    }

    #[test]
    fn ambiguous_reads_during_the_start_call_receive_no_exemption() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        capture(&mut state, 11_000);
        capture(&mut state, 13_000);
        state.started(1, 12_000, 18_000, 20_000).unwrap();
        assert_eq!(state.timing().capture_blocks_before_clock_start, 1);
        assert_eq!(state.timing().capture_blocks_processed, 2);
        assert_eq!(state.timing().clock_start_requested_at_us, Some(12_000));
        assert_eq!(state.timing().clock_started_at_us, Some(18_000));
        // Queue aging also uses the earliest possible start bound, not the
        // later completion (which would overstate the remaining queue by 6 ms).
        assert_eq!(state.before_capture(0, 21_000).unwrap(), 11);
        assert_fault(state.before_capture(0, 32_000), Kind::ReferenceLate);
    }

    #[test]
    fn failed_or_late_start_cannot_publish_a_valid_origin_or_readiness() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        capture(&mut state, 11_000);
        for (request, completion, receipt) in [
            (12_001, 12_000, 14_000), // Reversed method-call interval.
            (12_000, 16_000, 15_999), // Completion in the future.
            (9_999, 13_000, 14_000),  // Request before the actual prime.
            (12_000, 50_000, 52_000), // Request is already 40 ms old.
        ] {
            assert_fault(
                state.started(1, request, completion, receipt),
                Kind::InvalidStart,
            );
            assert_eq!(state.timing().clock_start_requested_at_us, None);
            assert_eq!(state.timing().clock_started_at_us, None);
            assert_eq!(state.timing().capture_blocks_before_clock_start, 0);
        }
        // A failed speaker start sends no successful notice. Its absence never
        // grants an origin: only the existing two retained blocks are allowed.
        capture(&mut state, 23_000);
        assert_fault(state.before_capture(0, 24_000), Kind::ReferenceLate);
    }

    #[test]
    fn more_than_two_capture_calls_without_a_start_notice_still_fault() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        capture(&mut state, 11_000);
        capture(&mut state, 12_000);
        // Even extra actual reference cannot extend the bounded unknown phase.
        let mut packet = reference(1, 480, 240);
        packet.accepted_at_us = 12_500;
        packet.queue_observed_at_us = 12_501;
        state.accept(packet, 12_502, 2, &mut echo).unwrap();
        assert_fault(state.before_capture(0, 13_000), Kind::ReferenceLate);
        assert_fault(state.captured(13_000, 13_001), Kind::ReferenceLate);
        assert_eq!(state.timing().capture_blocks_processed, 2);
    }

    #[test]
    fn each_explicit_reset_gets_its_own_bounded_immutable_phase_origin() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        for epoch in 1..=3 {
            let base = epoch * 100_000;
            let old = (epoch > 1).then(|| QueuedRange {
                epoch: epoch - 1,
                retired_through: 100,
                accepted_through: state.timing().accepted_through,
            });
            let mut next = prime(epoch, old);
            next.accepted_at_us = base;
            next.queue_observed_at_us = base + 1;
            state.prime(next, base + 2, 2, &mut echo).unwrap();
            assert_eq!(state.timing().capture_blocks_processed, 0);
            assert_eq!(state.timing().capture_blocks_before_clock_start, 0);
            capture(&mut state, base + 1_000);
            if epoch == 2 {
                capture(&mut state, base + 2_000);
            }
            state
                .started(epoch, base + 3_000, base + 3_000, base + 3_001)
                .unwrap();
            let origin = if epoch == 2 { 2 } else { 1 };
            assert_eq!(state.timing().capture_blocks_before_clock_start, origin);
            capture(&mut state, base + 4_000);
            capture(&mut state, base + 5_000);
            assert_eq!(state.timing().capture_blocks_processed, origin + 2);
            assert_fault(state.before_capture(0, base + 5_001), Kind::ReferenceLate);
        }
    }

    #[test]
    fn capture_clock_drift_cannot_rebase_the_start_origin() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        capture(&mut state, 11_000);
        state.started(1, 12_000, 12_000, 12_001).unwrap();
        capture(&mut state, 13_000);
        for block in 1..=4 {
            let at = 14_000 + block * 1_000;
            let mut packet = reference(block, 480 + (block - 1) * 240, 240);
            packet.accepted_at_us = at;
            packet.queue_observed_at_us = at;
            state.accept(packet, at, 2, &mut echo).unwrap();
            capture(&mut state, at + 1);
        }
        // An independent faster capture clock consumes the remaining lead.
        capture(&mut state, 19_000);
        assert_eq!(state.timing().capture_blocks_before_clock_start, 1);
        assert_eq!(state.timing().capture_blocks_processed, 7);
        assert_fault(state.before_capture(0, 19_001), Kind::ReferenceLate);
    }

    #[test]
    fn phase_origin_does_not_hide_a_truly_exhausted_observed_queue() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        capture(&mut state, 11_000);
        state.started(1, 12_000, 12_000, 12_001).unwrap();
        assert_eq!(state.running_capture_blocks(), 0);
        assert!(state.before_capture(0, 31_999).is_ok());
        // 480 actual prime frames have nominally elapsed, still within 40 ms.
        assert_fault(state.before_capture(0, 32_000), Kind::ReferenceLate);
        assert_eq!(state.timing().accepted_through, 480);
        assert_eq!(state.timing().analysed_through, 480);
    }

    #[test]
    fn origin_classification_cannot_expand_the_eight_block_backlog_bound() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        capture(&mut state, 11_000);
        capture(&mut state, 12_000);
        for sequence in 1..=7 {
            let mut packet = reference(sequence, 480 + (sequence - 1) * 240, 240);
            packet.accepted_at_us = 14_000 + sequence;
            packet.queue_observed_at_us = packet.accepted_at_us;
            state.accept(packet, 15_000, 2, &mut echo).unwrap();
        }
        // Control was delayed behind nine actual reference blocks; excluding
        // both pre-start reads cannot make that excessive backlog acceptable.
        assert_fault(
            state.started(1, 13_000, 13_000, 15_001),
            Kind::ReferenceBacklog,
        );
        assert_eq!(state.timing().clock_started_at_us, None);
        assert_eq!(state.timing().capture_blocks_before_clock_start, 0);
        assert_eq!(state.timing().capture_blocks_processed, 2);
    }

    #[test]
    fn a_delayed_start_notice_may_follow_newer_accepted_reference() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        capture(&mut state, 11_000);
        let mut packet = reference(1, 480, 240);
        packet.accepted_at_us = 13_000;
        packet.queue_observed_at_us = 13_001;
        state.accept(packet, 13_002, 2, &mut echo).unwrap();
        capture(&mut state, 14_000);
        state.started(1, 12_000, 12_000, 15_000).unwrap();
        assert_eq!(state.timing().capture_blocks_before_clock_start, 1);
        assert_eq!(state.timing().accepted_at_us, 13_000);
        assert_eq!(state.timing().clock_started_at_us, Some(12_000));
        assert_eq!(state.running_capture_blocks(), 1);
        assert_eq!(state.timing().analysed_through, 720);
    }

    #[test]
    fn reference_and_start_notice_freshness_remain_strictly_under_40_ms() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        capture(&mut state, 11_000);
        assert_fault(state.started(1, 12_000, 12_000, 52_000), Kind::InvalidStart);
        state.started(1, 12_000, 12_000, 12_001).unwrap();
        let mut packet = reference(1, 480, 240);
        packet.accepted_at_us = 12_000;
        packet.queue_observed_at_us = 12_001;
        assert_fault(
            state.accept(packet, 52_000, 2, &mut echo),
            Kind::QueueTiming,
        );
        assert_fault(state.before_capture(0, 52_000), Kind::ReferenceLate);
        assert_eq!(state.timing().capture_blocks_before_clock_start, 1);
    }

    #[test]
    fn invalid_capture_timestamps_cannot_create_a_start_exemption() {
        let mut state = ReferenceAssembly::default();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        assert_fault(state.captured(12_000, 11_000), Kind::CaptureTiming);
        capture(&mut state, 11_000);
        for at in [10_999, 11_000] {
            assert_fault(state.captured(at, 12_000), Kind::CaptureTiming);
        }
        state.started(1, 12_000, 12_000, 12_001).unwrap();
        assert_eq!(state.timing().capture_blocks_processed, 1);
        assert_eq!(state.timing().capture_blocks_before_clock_start, 1);
    }

    /// Two clocks in virtual time, through the production calls. The capture
    /// clock is the time base: one block every 10 ms. The speaker accepts one
    /// block every 10 ms of its own clock, `skew_ppm` faster. `deliver` says
    /// whether the speaker's packet number `n` reaches the capture worker.
    /// Returns the capture blocks processed and the fault that ended the run.
    fn session(
        state: &mut ReferenceAssembly,
        skew_ppm: f64,
        blocks: u64,
        deliver: impl FnMut(u64) -> bool,
    ) -> (u64, Option<Kind>) {
        session_at(state, skew_ppm, blocks, deliver, |_| 0)
    }

    /// `late(block)` is how long after its time the capture loop reaches a
    /// block, in microseconds: zero for a loop that is never kept waiting.
    fn session_at(
        state: &mut ReferenceAssembly,
        skew_ppm: f64,
        blocks: u64,
        mut deliver: impl FnMut(u64) -> bool,
        late: impl Fn(u64) -> u64,
    ) -> (u64, Option<Kind>) {
        const START: u64 = 1_000_000;
        let mut echo = EchoProcessor::new(false);
        let mut first = prime(1, None);
        first.accepted_at_us = START;
        first.queue_observed_at_us = START + 1;
        state.prime(first, START + 2, 2, &mut echo).unwrap();
        state
            .started(1, START + 10, START + 20, START + 30)
            .unwrap();
        let period = 10_000.0 / (1.0 + skew_ppm * 1e-6);
        let mut packet = 0_u64;
        let mut sequence = 0_u64;
        for block in 0..blocks {
            // The capture reads half a block after the speaker's first write.
            let now = START + 5_000 + block * 10_000 + late(block);
            loop {
                let accepted_at = START + ((packet + 1) as f64 * period) as u64;
                if accepted_at > now {
                    break;
                }
                packet += 1;
                if !deliver(packet) {
                    continue;
                }
                sequence += 1;
                let mut reference = reference(sequence, 480 + (packet - 1) * 240, 240);
                reference.accepted_at_us = accepted_at;
                reference.queue_observed_at_us = accepted_at;
                reference.queued_frames = 456;
                if let Err(error) = state.accept(reference, now, 2, &mut echo) {
                    return (
                        block,
                        Some(error.downcast_ref::<ReferenceFault>().unwrap().reason),
                    );
                }
            }
            let step = state
                .before_capture(16, now)
                .and_then(|_| state.render_for_capture(now, &mut echo))
                .and_then(|()| state.captured(now, now + 100));
            if let Err(error) = step {
                return (
                    block,
                    Some(error.downcast_ref::<ReferenceFault>().unwrap().reason),
                );
            }
        }
        (blocks, None)
    }

    #[test]
    fn the_fixed_margin_is_used_up_by_the_clock_difference_measured_on_the_lamps() {
        // The speaker 98 ppm fast gains a block every 102 s. The margin is two
        // blocks at the start and eight at most: eleven minutes.
        let mut state = ReferenceAssembly::default();
        let (blocks, fault) = session(&mut state, 98.0, 100_000, |_| true);
        assert_eq!(fault, Some(Kind::ReferenceBacklog));
        assert!((60_000..=70_000).contains(&blocks), "after {blocks} blocks");
        // The speaker 98 ppm slow loses the two it started with sooner.
        let mut state = ReferenceAssembly::default();
        let (blocks, fault) = session(&mut state, -98.0, 100_000, |_| true);
        assert_eq!(fault, Some(Kind::ReferenceLate));
        assert!((5_000..=25_000).contains(&blocks), "after {blocks} blocks");
    }

    #[test]
    fn the_capture_clocked_reference_follows_that_difference() {
        for skew in [98.0, -98.0] {
            // Twenty minutes: twice as long as the fixed margin lasted.
            let mut state = ReferenceAssembly::capture_clocked();
            let (blocks, fault) = session(&mut state, skew, 120_000, |_| true);
            assert_eq!((blocks, fault), (120_000, None), "{skew}");
            let drift = state.drift().unwrap();
            assert!((drift.steady_ppm - skew).abs() < 10.0, "{skew}: {drift:?}");
            assert!(
                (TARGET_FRAMES - 240..=TARGET_FRAMES + 240).contains(&drift.unread_frames),
                "{skew}: {drift:?}"
            );
            // One render block for each capture block, and every counter is
            // still the count it was: nothing was rebased.
            let timing = state.timing();
            assert_eq!(drift.rendered_blocks, 120_000);
            assert_eq!(timing.capture_blocks_processed, 120_000);
            let expected = (120_000.0 * 240.0 * (1.0 + skew * 1e-6)) as u64;
            assert!(timing.accepted_through.abs_diff(480 + expected) <= 720);
            assert_eq!(timing.accepted_through % 240, 0);
            // What is accepted and not yet read is what waits in the ring.
            assert_eq!(
                timing.accepted_through - timing.analysed_through,
                drift.unread_frames as u64
            );
            // The periodic report carries the same state to the trace.
            let reported = timing.drift.unwrap();
            assert_eq!(reported.unread_frames as usize, drift.unread_frames);
            assert_eq!(reported.learned_ppm, drift.steady_ppm as f32);
            assert_eq!(reported.rate_ppm, drift.ppm as f32);
        }
    }

    #[test]
    fn the_fixed_reference_reports_exactly_what_it_did() {
        let mut state = ReferenceAssembly::default();
        let (blocks, fault) = session(&mut state, 0.0, 500, |_| true);
        assert_eq!((blocks, fault), (500, None));
        assert!(state.drift().is_none() && state.timing().drift.is_none());
        let report = serde_json::to_value(state.timing()).unwrap();
        assert!(report.get("drift").is_none(), "{report}");
        // And a report without the field still reads back.
        let read: ReferenceTiming = serde_json::from_value(report).unwrap();
        assert!(read.drift.is_none());
        // A fault from the capture-clocked reference says where it stood.
        let clocked = ReferenceAssembly::capture_clocked();
        assert!(clocked.fault(Kind::ReferenceLate, 1).timing.drift.is_some());
    }

    #[test]
    fn lost_packets_are_a_fault_within_the_same_bound_either_way() {
        for clocked in [false, true] {
            let mut state = if clocked {
                ReferenceAssembly::capture_clocked()
            } else {
                ReferenceAssembly::default()
            };
            // Ten seconds of steady supply, then nothing arrives.
            let (blocks, fault) = session(&mut state, 98.0, 2_000, |packet| packet <= 1_000);
            assert_eq!(fault, Some(Kind::ReferenceLate), "clocked {clocked}");
            // The last packet was accepted 10 s in. Its 40 ms of freshness and
            // the frames in hand allow two to four more capture blocks.
            assert!(
                (1_002..=1_005).contains(&blocks),
                "clocked {clocked}: {blocks}"
            );
        }
    }

    /// A capture loop held up at block 3,000 for `stall_us`, which then
    /// works through what it missed without waiting: each block it owes is
    /// read 200 us after the one before, until it is back on time.
    fn stall(stall_us: u64) -> impl Fn(u64) -> u64 {
        move |block| match block.checked_sub(3_000) {
            Some(behind) => (stall_us + behind * 200).saturating_sub(behind * 10_000),
            None => 0,
        }
    }

    #[test]
    fn a_capture_loop_kept_waiting_catches_up_without_a_fault_in_either_mode() {
        // The paced check woke the capture loop 27 ms late. It found the
        // packets of that time waiting, and a ring of eight blocks with five
        // in hand had no room for them. The same 27 ms, thirty seconds in.
        for clocked in [false, true] {
            let mut state = if clocked {
                ReferenceAssembly::capture_clocked()
            } else {
                ReferenceAssembly::default()
            };
            let (blocks, fault) = session_at(&mut state, 98.0, 6_000, |_| true, stall(27_000));
            assert_eq!((blocks, fault), (6_000, None), "clocked {clocked}");
            if let Some(drift) = state.drift() {
                // And it settles again where it was.
                assert!(
                    (TARGET_FRAMES - 240..=TARGET_FRAMES + 240).contains(&drift.unread_frames),
                    "{drift:?}"
                );
            }
        }
    }

    #[test]
    fn a_capture_loop_kept_waiting_longer_than_a_reference_keeps_is_a_fault_either_way() {
        // Thirty-five milliseconds: the oldest packet waiting was accepted a
        // block before that, and 45 ms is older than a reference may be.
        // Neither mode pretends otherwise.
        let mut faults = Vec::new();
        for clocked in [false, true] {
            let mut state = if clocked {
                ReferenceAssembly::capture_clocked()
            } else {
                ReferenceAssembly::default()
            };
            let (blocks, fault) = session_at(&mut state, 98.0, 6_000, |_| true, stall(35_000));
            assert_eq!(blocks, 3_000, "clocked {clocked}");
            faults.push(fault.expect("a stale reference was used"));
        }
        assert_eq!(faults[0], faults[1]);
    }

    #[test]
    fn a_skipped_packet_is_a_gap_and_never_a_clock_difference() {
        for clocked in [false, true] {
            let mut state = if clocked {
                ReferenceAssembly::capture_clocked()
            } else {
                ReferenceAssembly::default()
            };
            // Packet 500 never arrives. Packet 501 does, with its own cursor.
            let (blocks, fault) = session(&mut state, 98.0, 2_000, |packet| packet != 500);
            assert_eq!(fault, Some(Kind::CursorGap), "clocked {clocked}");
            assert!((499..=501).contains(&blocks), "clocked {clocked}: {blocks}");
        }
    }

    #[test]
    fn a_flood_of_reference_is_a_backlog_in_the_ring_too() {
        let mut state = ReferenceAssembly::capture_clocked();
        let mut echo = EchoProcessor::new(false);
        state.prime(prime(1, None), 10_002, 2, &mut echo).unwrap();
        // The ring holds ten blocks: the lead, the prime and five more.
        for sequence in 1..=5 {
            let packet = reference(sequence, 480 + (sequence - 1) * 240, 240);
            state.accept(packet, 10_102, 2, &mut echo).unwrap();
        }
        assert_fault(
            state.accept(reference(6, 1_680, 240), 10_102, 2, &mut echo),
            Kind::ReferenceBacklog,
        );
        // The refused packet changed nothing.
        assert_eq!(state.timing().accepted_through, 1_680);
        assert_eq!(state.drift().unwrap().unread_frames, 2_400);
    }

    #[test]
    fn a_clock_no_crystal_has_is_not_followed() {
        // 2,000 ppm: the rate stops at its limit and the ring fills.
        let mut state = ReferenceAssembly::capture_clocked();
        let (blocks, fault) = session(&mut state, 2_000.0, 100_000, |_| true);
        assert_eq!(fault, Some(Kind::ReferenceBacklog));
        assert!(blocks < 10_000, "{blocks}");
        assert!((state.drift().unwrap().ppm - 500.0).abs() < 0.01);
    }

    /// Four hours of virtual time through the real DSP render path. Slow in a
    /// debug build: run with `--release -- --ignored`.
    #[test]
    #[ignore = "four virtual hours; run in release"]
    fn four_virtual_hours_capture_clocked() {
        for skew in [98.0, -66.0] {
            let mut state = ReferenceAssembly::capture_clocked();
            let (blocks, fault) = session(&mut state, skew, 4 * 360_000, |_| true);
            assert_eq!((blocks, fault), (4 * 360_000, None), "{skew}");
            let drift = state.drift().unwrap();
            println!("{skew} ppm for four hours: {drift:?}");
            assert!((drift.steady_ppm - skew).abs() < 5.0);
            assert!((drift.ppm - skew).abs() < 10.0);
        }
    }
}
