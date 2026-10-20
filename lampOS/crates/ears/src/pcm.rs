//! Opt-in bounded PCM handoff for the privacy-gated Ears runtime (#1669).
//! The capture owner and Bus pump share one slot. The handoff has no I/O;
//! publisher setup is a separate pump-side operation.
//! A block keeps its original permit: a later lease never renews queued audio.
use crate::{
    BLOCK, RATE,
    privacy::{Permit, PublicationGate},
};
use lamp_msgs::Audio;
use std::{io, sync::Mutex};

/// Only Ears mic1 expires after 50 ms. The Bus receiver additionally applies
/// its existing 50 ms transport grace; this is not strict 50 ms delivery.
pub fn audio_publisher(bus: &lamp_bus::Bus, source: &str) -> io::Result<lamp_bus::Publisher> {
    let lifetime = std::time::Duration::from_millis(50);
    let mut publisher = bus.publisher_declared(
        Audio::TOPICS[0],
        &Audio::declaration().lasting(lifetime),
        source,
    )?;
    publisher.expires_after(Some(lifetime));
    Ok(publisher)
}

/// Bounded drain: false means the budget was consumed without observing empty.
/// The caller must fence privacy on false, and must not publish PCM that tick.
pub fn drain_privacy(mut next: impl FnMut() -> io::Result<bool>) -> io::Result<bool> {
    for _ in 0..16 {
        if !next()? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Failure policy for the optional audio publisher, independent of capture and
/// metadata publication. No failure changes microphone authority by itself.
#[derive(Default)]
pub struct ExportRetry {
    retry_at_us: u64,
    failed: bool,
}
impl ExportRetry {
    pub fn ready(&self, now_us: u64) -> bool {
        now_us >= self.retry_at_us
    }
    /// Whether this is the first failure worth reporting, until recovery.
    pub fn failed(&mut self, now_us: u64) -> bool {
        self.retry_at_us = now_us.saturating_add(2_000_000);
        !std::mem::replace(&mut self.failed, true)
    }
    pub fn recovered(&mut self) -> bool {
        self.retry_at_us = 0;
        std::mem::take(&mut self.failed)
    }
}

/// One owned block, deliberately neither Clone nor Debug. Drop fills this
/// buffer with zero; this is not a guarantee about compiler copies, allocator,
/// kernel buffers, or physical memory erasure.
pub struct Pcm {
    samples: [i16; BLOCK],
    permit: Permit,
    fence: u64,
    sequence: u64,
    captured_at_us: u64,
}

impl Drop for Pcm {
    fn drop(&mut self) {
        self.samples.fill(0);
    }
}

impl Pcm {
    pub fn captured_at_us(&self) -> u64 {
        self.captured_at_us
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn permit(&self) -> Permit {
        self.permit
    }

    pub fn fence(&self) -> u64 {
        self.fence
    }

    /// Recheck permission before packing. The eventual publisher must check
    /// again at its final boundary: this check grants no future publication.
    pub fn pack(&self, gate: &PublicationGate, now_us: u64) -> io::Result<Option<Packet>> {
        if !gate.allows(self.permit, self.fence, now_us) {
            return Ok(None);
        }
        let mut bytes = [0u8; BLOCK * 2];
        for (sample, pair) in self.samples.iter().zip(bytes.chunks_exact_mut(2)) {
            pair.copy_from_slice(&sample.to_le_bytes());
        }
        let result = Audio {
            sample_rate: RATE,
            channels: 1,
            sample_format: Audio::S16LE.to_owned(),
            sequence: self.sequence,
        }
        .block(&bytes);
        bytes.fill(0);
        let packet = Packet {
            bytes: result?,
            permit: self.permit,
            fence: self.fence,
        };
        // Revocation can race encoding. A fresh clock check still belongs at
        // the final publication boundary, not at this earlier snapshot.
        Ok(packet.allows(gate, now_us).then_some(packet))
    }
}

/// Owned encoded header and PCM. No Clone/Debug, and its bytes are filled with
/// zero on drop, with the same erasure limits as [`Pcm`].
pub struct Packet {
    bytes: Vec<u8>,
    permit: Permit,
    fence: u64,
}

impl Packet {
    pub fn permit(&self) -> Permit {
        self.permit
    }

    pub fn fence(&self) -> u64 {
        self.fence
    }

    /// Use the packet's original authority in the publisher's final callback.
    /// A current lease supplied by a caller must never replace this permit.
    pub fn allows(&self, gate: &PublicationGate, now_us: u64) -> bool {
        gate.allows(self.permit, self.fence, now_us)
    }

    /// Borrow only while this packet's original authority still holds. The
    /// reference itself cannot enforce future revocation: do not retain it or
    /// publish it without another check at the final output boundary.
    pub fn bytes_if_allowed(&self, gate: &PublicationGate, now_us: u64) -> Option<&[u8]> {
        self.allows(gate, now_us).then_some(self.bytes.as_slice())
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

/// A single slot; callers provide synchronization if crossing threads. Off by
/// default. There is no queue, inference, allocation on offer, or Bus work.
#[derive(Default)]
pub struct Mailbox {
    enabled: bool,
    slot: Option<Pcm>,
}

impl Mailbox {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            slot: None,
        }
    }

    /// The producer supplies its capture sequence, advancing it for *every*
    /// capture block, including dropped offers. Full means drop incoming; the
    /// valid retained block is never overwritten. An expired or revoked slot is
    /// cleared first, so it cannot indefinitely block a fresh capture era.
    /// Disabled or unauthorized offers never copy. Admission checks the current
    /// publication fence, epoch and lease as well as the original deadline.
    pub fn offer(
        &mut self,
        samples: &[i16; BLOCK],
        gate: &PublicationGate,
        permit: Permit,
        fence: u64,
        sequence: u64,
        now_us: u64,
    ) -> bool {
        self.offer_captured(
            samples,
            gate,
            CaptureStamp {
                permit,
                fence,
                sequence,
                captured_at_us: now_us,
            },
            now_us,
        )
    }

    pub fn offer_captured(
        &mut self,
        samples: &[i16; BLOCK],
        gate: &PublicationGate,
        stamp: CaptureStamp,
        now_us: u64,
    ) -> bool {
        let CaptureStamp {
            permit,
            fence,
            sequence,
            captured_at_us,
        } = stamp;
        if captured_at_us > now_us {
            return false;
        }
        if !self.enabled {
            return false;
        }
        if self
            .slot
            .as_ref()
            .is_some_and(|block| !gate.allows(block.permit, block.fence, now_us))
        {
            self.invalidate();
        }
        if self.slot.is_some() || !gate.allows(permit, fence, now_us) {
            return false;
        }
        self.slot = Some(Pcm {
            samples: *samples,
            permit,
            fence,
            sequence,
            captured_at_us,
        });
        // If a fence changed while copying, discard rather than retaining the
        // just-revoked block. Final consumers still recheck independently.
        if !gate.allows(permit, fence, now_us) {
            self.invalidate();
            return false;
        }
        true
    }

    /// Consume the slot exactly once. An invalid block is dropped, never left
    /// waiting for a newer permit that might appear to authorize it again.
    pub fn take(&mut self, gate: &PublicationGate, now_us: u64) -> Option<Pcm> {
        let block = self.slot.take()?;
        gate.allows(block.permit, block.fence, now_us)
            .then_some(block)
    }

    /// Destroy pending PCM on capture discontinuity or mute.
    /// This does not enable a disabled mailbox or change publication authority.
    pub fn invalidate(&mut self) {
        self.slot = None;
    }
}

/// Original capture metadata, never replaced by a publication timestamp.
#[derive(Clone, Copy)]
pub struct CaptureStamp {
    pub permit: Permit,
    pub fence: u64,
    pub sequence: u64,
    pub captured_at_us: u64,
}

/// One cross-thread slot. Contention drops work, never waits for the bus pump.
/// Revocation is enforced by PublicationGate even when cleanup cannot lock.
pub struct SharedMailbox {
    enabled: bool,
    slot: Mutex<Mailbox>,
}
impl SharedMailbox {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            slot: Mutex::new(Mailbox::new(enabled)),
        }
    }
    pub fn offer(
        &self,
        samples: &[i16; BLOCK],
        gate: &PublicationGate,
        stamp: CaptureStamp,
        now_us: u64,
    ) -> bool {
        self.enabled
            && self
                .slot
                .try_lock()
                .is_ok_and(|mut slot| slot.offer_captured(samples, gate, stamp, now_us))
    }
    pub fn take(&self, gate: &PublicationGate, now_us: u64) -> Option<Pcm> {
        if !self.enabled {
            return None;
        }
        self.slot.try_lock().ok()?.take(gate, now_us)
    }
    pub fn discard_invalid(&self, gate: &PublicationGate, now_us: u64) {
        if self.enabled
            && let Ok(mut slot) = self.slot.try_lock()
            && slot
                .slot
                .as_ref()
                .is_some_and(|pcm| !gate.allows(pcm.permit, pcm.fence, now_us))
        {
            slot.invalidate();
        }
    }
    pub fn invalidate(&self) {
        if self.enabled
            && let Ok(mut slot) = self.slot.try_lock()
        {
            slot.invalidate();
        }
    }
}

/// PCM never selects the compatibility path that ignores privacy leases.
pub fn export_enabled(audio: Option<&str>, privacy: Option<&str>) -> io::Result<bool> {
    if audio != Some("on") {
        return Ok(false);
    }
    if !crate::privacy::enabled(privacy) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Ears LAMP_BUS_AUDIO=on requires LAMP_PRIVACY_CONTROLS=on or no-slide",
        ));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permit(epoch: u64, until_us: u64) -> Permit {
        Permit { epoch, until_us }
    }

    #[test]
    fn privacy_redrain_is_bounded_and_errors_never_report_an_empty_queue() {
        let mut reads = 0;
        assert!(
            !drain_privacy(|| {
                reads += 1;
                Ok(true)
            })
            .unwrap()
        );
        assert_eq!(reads, 16);
        assert!(drain_privacy(|| Ok(false)).unwrap());
        assert!(drain_privacy(|| Err(io::Error::other("privacy lost"))).is_err());
    }

    #[test]
    fn optional_audio_failure_retries_after_two_seconds_and_reports_once() {
        let mut retry = ExportRetry::default();
        assert!(retry.ready(0));
        assert!(retry.failed(100));
        assert!(!retry.ready(2_000_099));
        assert!(retry.ready(2_000_100));
        assert!(!retry.failed(2_000_100));
        assert!(!retry.ready(4_000_099));
        assert!(retry.recovered());
        assert!(!retry.recovered());
        assert!(retry.ready(4_000_100));
        assert!(retry.failed(4_000_100));
    }

    #[test]
    fn host_handoff_cost_is_measured_separately_from_capture_and_bus_io() {
        let shared = SharedMailbox::new(true);
        let gate = PublicationGate::default();
        gate.update(Some(permit(1, 50)), 0);
        let samples = [1; BLOCK];
        let mut times = Vec::new();
        for sequence in 0..1000 {
            let started = std::time::Instant::now();
            assert!(shared.offer(
                &samples,
                &gate,
                CaptureStamp {
                    permit: permit(1, 50),
                    fence: 0,
                    sequence,
                    captured_at_us: 1
                },
                2
            ));
            times.push(started.elapsed().as_nanos());
            assert!(shared.take(&gate, 3).is_some());
        }
        times.sort_unstable();
        eprintln!(
            "ears PCM host handoff only: p50={}ns p95={}ns max={}ns; 1000 blocks; target <1ms per20msblock; excludes ALSA, bus IO and target hardware",
            times[499], times[949], times[999]
        );
    }

    #[test]
    fn export_is_exact_opt_in_and_refuses_the_nonprivacy_capture_path() {
        for audio in [None, Some("off"), Some("ON"), Some("")] {
            assert!(!export_enabled(audio, None).unwrap());
        }
        assert!(export_enabled(Some("on"), Some("on")).unwrap());
        assert!(export_enabled(Some("on"), Some("no-slide")).unwrap());
        for privacy in [None, Some("off"), Some("ON"), Some("NO-SLIDE")] {
            assert!(export_enabled(Some("on"), privacy).is_err());
        }
    }

    #[test]
    fn a_stalled_slot_never_blocks_capture_and_drops_expose_the_original_sequence() {
        let shared = SharedMailbox::new(true);
        let gate = PublicationGate::default();
        gate.update(Some(permit(1, 50)), 0);
        let stamp = |sequence| CaptureStamp {
            permit: permit(1, 50),
            fence: 0,
            sequence,
            captured_at_us: 1,
        };
        let locked = shared.slot.lock().unwrap();
        assert!(!shared.offer(&[1; BLOCK], &gate, stamp(0), 2));
        shared.invalidate(); // Also does not wait on the pump.
        assert!(shared.take(&gate, 2).is_none());
        drop(locked);
        assert!(shared.offer(&[2; BLOCK], &gate, stamp(1), 2));
        let pcm = shared.take(&gate, 3).unwrap();
        assert_eq!(pcm.sequence(), 1);
        assert_eq!(pcm.captured_at_us(), 1);
        assert!(shared.offer(&[3; BLOCK], &gate, stamp(2), 4));
        gate.update(Some(permit(1, 100)), 0);
        shared.discard_invalid(&gate, 50);
        assert!(shared.take(&gate, 51).is_none());
    }

    #[test]
    fn default_and_explicit_off_do_not_retain_audio() {
        let gate = PublicationGate::default();
        gate.update(Some(permit(1, 50)), 0);
        for mut mailbox in [Mailbox::default(), Mailbox::new(false)] {
            assert!(!mailbox.offer(&[7; BLOCK], &gate, permit(1, 50), 0, 0, 1));
            assert!(mailbox.slot.is_none());
            assert!(mailbox.take(&gate, 2).is_none());
            mailbox.invalidate();
            assert!(!mailbox.offer(&[7; BLOCK], &gate, permit(1, 50), 0, 1, 3));
        }
    }

    #[test]
    fn a_full_slot_drops_incoming_and_the_producers_sequence_exposes_the_gap() {
        let gate = PublicationGate::default();
        let permit = permit(1, 50);
        gate.update(Some(permit), 0);
        let mut mailbox = Mailbox::new(true);
        assert!(mailbox.offer(&[1; BLOCK], &gate, permit, 0, 0, 1));
        assert!(!mailbox.offer(&[2; BLOCK], &gate, permit, 0, 1, 2));
        let first = mailbox.take(&gate, 3).unwrap();
        assert_eq!(first.samples, [1; BLOCK]);
        assert_eq!(first.sequence(), 0);
        assert!(mailbox.take(&gate, 3).is_none());
        assert!(mailbox.offer(&[3; BLOCK], &gate, permit, 0, 2, 4));
        assert_eq!(mailbox.take(&gate, 5).unwrap().sequence(), 2);
    }

    #[test]
    fn fences_epochs_and_original_deadlines_are_checked_at_take() {
        for (held, fence, current, current_fence, now) in [
            (permit(1, 50), 0, permit(1, 50), 1, 2),
            (permit(1, 50), 0, permit(2, 50), 0, 2),
            (permit(1, 50), 0, permit(1, 100), 0, 50),
            (permit(1, 100), 0, permit(1, 50), 0, 50),
        ] {
            let mut mailbox = Mailbox::new(true);
            let gate = PublicationGate::default();
            gate.update(Some(held), fence);
            assert!(mailbox.offer(&[1; BLOCK], &gate, held, fence, 0, 1));
            if current_fence == 1 {
                gate.fence();
            }
            gate.update(Some(current), current_fence);
            assert!(mailbox.take(&gate, now).is_none());
            gate.update(Some(permit(1, 200)), current_fence);
            assert!(mailbox.take(&gate, now).is_none());
        }
    }

    #[test]
    fn expiry_at_offer_and_explicit_invalidation_leave_nothing_to_publish() {
        let gate = PublicationGate::default();
        gate.update(Some(permit(1, 50)), 0);
        let mut mailbox = Mailbox::new(true);
        assert!(!mailbox.offer(&[1; BLOCK], &gate, permit(1, 50), 0, 0, 50));
        assert!(!mailbox.offer(&[1; BLOCK], &gate, permit(1, 50), u64::MAX, 0, 1));
        assert!(mailbox.offer(&[1; BLOCK], &gate, permit(1, 50), 0, 1, 1));
        mailbox.invalidate();
        gate.update(Some(permit(1, 100)), 0);
        assert!(mailbox.take(&gate, 2).is_none());
    }

    #[test]
    fn unauthorized_offers_never_retain_pcm_even_before_take() {
        let gate = PublicationGate::default();
        let held = permit(1, 50);
        let mut mailbox = Mailbox::new(true);
        // No current grant, then the wrong epoch, then a revoked grant.
        assert!(!mailbox.offer(&[5; BLOCK], &gate, held, 0, 0, 1));
        assert!(mailbox.slot.is_none());
        gate.update(Some(permit(2, 50)), 0);
        assert!(!mailbox.offer(&[5; BLOCK], &gate, held, 0, 1, 1));
        assert!(mailbox.slot.is_none());
        gate.update(Some(held), 0);
        gate.update(None, 0);
        assert!(!mailbox.offer(&[5; BLOCK], &gate, held, 0, 2, 1));
        assert!(mailbox.slot.is_none());
        let fresh_fence = gate.fence();
        gate.update(Some(held), fresh_fence);
        assert!(!mailbox.offer(&[5; BLOCK], &gate, held, 0, 3, 1));
        assert!(mailbox.slot.is_none());
        // Both the original and current grant must still be live.
        gate.update(Some(permit(1, 2)), fresh_fence);
        assert!(!mailbox.offer(&[5; BLOCK], &gate, held, fresh_fence, 4, 2));
        assert!(mailbox.slot.is_none());
        gate.update(Some(permit(1, 100)), fresh_fence);
        assert!(!mailbox.offer(&[5; BLOCK], &gate, held, fresh_fence, 5, 50));
        assert!(mailbox.slot.is_none());
        assert!(mailbox.offer(&[5; BLOCK], &gate, permit(1, 100), fresh_fence, 6, 51));
        assert_eq!(mailbox.take(&gate, 52).unwrap().sequence(), 6);
    }

    #[test]
    fn an_expired_full_slot_cannot_starve_a_new_valid_offer() {
        let gate = PublicationGate::default();
        gate.update(Some(permit(1, 20)), 0);
        let mut mailbox = Mailbox::new(true);
        assert!(mailbox.offer(&[1; BLOCK], &gate, permit(1, 20), 0, 10, 1));
        assert!(!mailbox.offer(&[2; BLOCK], &gate, permit(1, 20), 0, 11, 2));
        gate.update(Some(permit(1, 100)), 0);
        assert!(mailbox.offer(&[3; BLOCK], &gate, permit(1, 100), 0, 12, 20));
        let current = mailbox.take(&gate, 21).unwrap();
        assert_eq!(current.sequence(), 12);
        assert_eq!(current.samples, [3; BLOCK]);
    }

    #[test]
    fn a_new_capture_era_replaces_only_the_revoked_slot_and_not_its_permit() {
        let gate = PublicationGate::default();
        gate.update(Some(permit(1, 100)), 0);
        let mut mailbox = Mailbox::new(true);
        assert!(mailbox.offer(&[1; BLOCK], &gate, permit(1, 100), 0, 0, 1));
        let fence = gate.fence();
        gate.update(Some(permit(2, 100)), fence);
        // Even an invalid incoming offer destroys the obsolete retained audio.
        assert!(!mailbox.offer(&[2; BLOCK], &gate, permit(1, 100), 0, 1, 2));
        assert!(mailbox.slot.is_none());
        assert!(mailbox.offer(&[3; BLOCK], &gate, permit(2, 100), fence, 2, 3));
        let next = mailbox.take(&gate, 4).unwrap();
        assert_eq!(next.permit(), permit(2, 100));
        assert_eq!(next.fence(), fence);
        assert_eq!(next.sequence(), 2);
    }

    #[test]
    fn encoded_packets_keep_the_original_authority_after_the_pcm_is_dropped() {
        for transition in 0..4 {
            let gate = PublicationGate::default();
            gate.update(Some(permit(1, 50)), 0);
            let mut mailbox = Mailbox::new(true);
            assert!(mailbox.offer(&[7; BLOCK], &gate, permit(1, 50), 0, 99, 1));
            let pcm = mailbox.take(&gate, 2).unwrap();
            let packet = pcm.pack(&gate, 3).unwrap().unwrap();
            drop(pcm);
            assert_eq!(packet.permit(), permit(1, 50));
            assert_eq!(packet.fence(), 0);
            let at = match transition {
                0 => {
                    gate.update(Some(permit(1, 100)), 0);
                    50
                }
                1 => {
                    gate.update(Some(permit(2, 100)), 0);
                    4
                }
                2 => {
                    let fence = gate.fence();
                    gate.update(Some(permit(1, 100)), fence);
                    4
                }
                _ => {
                    gate.update(None, 0);
                    4
                }
            };
            assert!(!packet.allows(&gate, at));
            assert!(packet.bytes_if_allowed(&gate, at).is_none());
        }
    }

    #[test]
    fn a_final_callback_can_refuse_after_an_earlier_payload_borrow() {
        let gate = PublicationGate::default();
        gate.update(Some(permit(1, 50)), 0);
        let mut mailbox = Mailbox::new(true);
        assert!(mailbox.offer(&[7; BLOCK], &gate, permit(1, 50), 0, 0, 1));
        let packet = mailbox
            .take(&gate, 2)
            .unwrap()
            .pack(&gate, 3)
            .unwrap()
            .unwrap();
        assert!(packet.bytes_if_allowed(&gate, 4).is_some());
        gate.fence();
        // The only authority used by a future publisher must be the original
        // packet's, checked at its final callback with the then-current clock.
        let sent = if packet.allows(&gate, 5) { 1 } else { 0 };
        assert_eq!(sent, 0);
    }

    #[test]
    fn packing_is_320_little_endian_samples_under_the_existing_audio_header() {
        let gate = PublicationGate::default();
        let permit = permit(9, 50);
        gate.update(Some(permit), 0);
        let samples = std::array::from_fn(|i| match i % 4 {
            0 => i16::MIN,
            1 => i16::MAX,
            2 => -2,
            _ => 0x1234,
        });
        let mut mailbox = Mailbox::new(true);
        assert!(mailbox.offer(&samples, &gate, permit, 0, 12, 1));
        let pcm = mailbox.take(&gate, 2).unwrap();
        assert_eq!(pcm.permit(), permit);
        assert_eq!(pcm.fence(), 0);
        let packet = pcm.pack(&gate, 3).unwrap().unwrap();
        let (header, bytes) = Audio::split(packet.bytes_if_allowed(&gate, 3).unwrap()).unwrap();
        assert_eq!(
            (header.sample_rate, header.channels, header.sequence),
            (16_000, 1, 12)
        );
        assert_eq!(header.sample_format, Audio::S16LE);
        assert_eq!(bytes.len(), BLOCK * 2);
        for (sample, pair) in samples.iter().zip(bytes.chunks_exact(2)) {
            assert_eq!(*sample, i16::from_le_bytes([pair[0], pair[1]]));
        }
        // Renewing the gate does not renew a block already taken either.
        gate.update(
            Some(Permit {
                epoch: 9,
                until_us: 100,
            }),
            0,
        );
        assert!(pcm.pack(&gate, 50).unwrap().is_none());
        // Taking a block is not permission to publish after revocation.
        gate.fence();
        assert!(pcm.pack(&gate, 4).unwrap().is_none());
    }
}
