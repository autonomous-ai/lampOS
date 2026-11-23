//! The speaker's one-shot start barrier, shared by the Linux worker and tests.
use super::PRIME_FRAMES;
use crate::wire::{ClockPrimed, PreparedPrime, PrimeDisposition, ReferenceFaultKind as Kind};
use lamp_audio::ownership::QueuedRange;

const MAX_PRIME_US: u64 = 100_000;
/// The first start after the voice starts (epoch 1) may be held up on a cold
/// boot, while the whole board is starting: on 52e6 it once took more than
/// 100 ms and the voice exited (#8391). Later epochs keep 100 ms.
const FIRST_PRIME_US: u64 = 1_000_000;

/// The start barrier's bound for a playback epoch, from the first prime write
/// to the clock's start. Speaker and capture both use it.
pub fn barrier_us(playback_epoch: u64) -> u64 {
    if playback_epoch == 1 {
        FIRST_PRIME_US
    } else {
        MAX_PRIME_US
    }
}

#[derive(Default)]
pub struct PrimeBarrier {
    pending: Option<(ClockPrimed, u64)>,
}

impl PrimeBarrier {
    /// Arm only after the certified prime was sent on the live control link.
    /// `began_at` is the original first-prime-write attempt, never receipt time.
    pub fn sent(&mut self, prime: ClockPrimed, began_at: u64) -> Result<(), Kind> {
        if self.pending.is_some()
            || prime.playback_epoch == 0
            || prime.privacy_generation == 0
            || prime.accepted_zero_frames != PRIME_FRAMES
            || prime.queued_frames != PRIME_FRAMES
            || began_at > prime.accepted_at_us
            || prime.accepted_at_us > prime.queue_observed_at_us
        {
            return Err(Kind::InvalidPrime);
        }
        self.pending = Some((prime, began_at));
        Ok(())
    }

    pub fn is_waiting(&self) -> bool {
        self.pending.is_some()
    }

    pub fn reset(&mut self) {
        self.pending = None;
    }

    /// Validate preparation, unchanged hardware cursors and current privacy
    /// before ALSA start. Success consumes this exact prime, so duplicate or
    /// unsolicited acknowledgements cannot start another clock. This does not
    /// authorize ALSA: the worker still obtains and checks its live zero token.
    pub fn acknowledge(
        &mut self,
        prepared: PreparedPrime,
        current: QueuedRange,
        privacy_generation: u64,
        now: u64,
    ) -> Result<u64, Kind> {
        let (prime, began_at) = self.pending.ok_or(Kind::InvalidStart)?;
        if now < prime.queue_observed_at_us
            || now
                .checked_sub(began_at)
                .is_none_or(|age| age >= barrier_us(prime.playback_epoch))
        {
            return Err(Kind::BarrierTimeout);
        }
        if privacy_generation == 0
            || privacy_generation != prime.privacy_generation
            || prepared.privacy_generation != prime.privacy_generation
        {
            return Err(Kind::PrivacyMismatch);
        }
        if prepared.playback_epoch != prime.playback_epoch
            || current.epoch != prime.playback_epoch
            || current.accepted_through != PRIME_FRAMES as u64
            || current.retired_through != 0
            || prepared.prepared_through != PRIME_FRAMES as u64
            || match prepared.disposition {
                PrimeDisposition::Analysed => prepared.analysed_through != PRIME_FRAMES as u64,
                PrimeDisposition::BufferedForCapture => prepared.analysed_through != 0,
            }
        {
            return Err(Kind::InvalidStart);
        }
        self.pending = None;
        Ok(prime.playback_epoch)
    }
}
