//! Bounded, metadata-only admission of one head-direction observation.
//!
//! The timestamp, identity and sequence checks reuse the camera `Capture`
//! contract. The eyes' direct `LinuxPort` reader does not run that wrapper, so
//! this observer checks its own metadata without changing legacy capture or
//! output. No image, landmarks, clock, I/O or publication live here.
//!
//! All host times must come from the same OS monotonic clock as the driver's
//! `HostMonotonic` timestamp. An end-of-frame time remains an end-of-frame
//! time; it is not renamed an exposure time. Unknown source time is rejected,
//! never replaced with the time a request, dequeue or inference finished.
//!
//! The caller samples permission before opening, after opening, around frame
//! acquisition and again before using a result. A generation must change on
//! every revocation/expiry, including close followed by immediate regrant.
//! This guard validates those samples; it cannot observe an unsampled change
//! or close a concurrent revoke/publish race. A publisher and its consumers
//! still need their own final authority fence. Acceptance is a read-only
//! check, not a reservation, one-shot delivery or cancellation of inference.

use lamp_camera::{
    CaptureId, DriverTimestamp, MAX_DEQUEUE_AGE_US, PortFrame, SourceId, TimestampDomain,
    TimestampPoint,
};

/// Closed failure vocabulary; no driver messages or caller text are retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    InvalidMaxAge,
    GenerationExhausted,
    NoCapture,
    PermissionMissing,
    PermissionChanged,
    ClockInvalid,
    CaptureMismatch,
    SourceMismatch,
    TimestampMissing,
    TimestampDomain,
    TimestampPoint,
    TimestampPointChanged,
    TimestampZero,
    TimestampBeforeOpen,
    TimestampFuture,
    TimestampRegressed,
    SequenceDuplicate,
    SequenceRegressed,
    Stale,
    Superseded,
}

impl Reason {
    /// Fixed diagnostic key. This never includes capture identities, source
    /// labels, timestamps or caller/driver text.
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidMaxAge => "invalid_max_age",
            Self::GenerationExhausted => "generation_exhausted",
            Self::NoCapture => "no_capture",
            Self::PermissionMissing => "permission_missing",
            Self::PermissionChanged => "permission_changed",
            Self::ClockInvalid => "clock_invalid",
            Self::CaptureMismatch => "capture_mismatch",
            Self::SourceMismatch => "source_mismatch",
            Self::TimestampMissing => "timestamp_missing",
            Self::TimestampDomain => "timestamp_domain",
            Self::TimestampPoint => "timestamp_point",
            Self::TimestampPointChanged => "timestamp_point_changed",
            Self::TimestampZero => "timestamp_zero",
            Self::TimestampBeforeOpen => "timestamp_before_open",
            Self::TimestampFuture => "timestamp_future",
            Self::TimestampRegressed => "timestamp_regressed",
            Self::SequenceDuplicate => "sequence_duplicate",
            Self::SequenceRegressed => "sequence_regressed",
            Self::Stale => "stale",
            Self::Superseded => "superseded",
        }
    }
}

/// An observation admitted by this guard, usable only while it is still the
/// latest attempt in the same capture and permission generation. Fields are
/// private so callers cannot change its age, sequence or authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ticket {
    generation: u64,
    capture: CaptureId,
    permission: u64,
    driver_sequence: u32,
    driver_timestamp: DriverTimestamp,
    dequeue_started_us: u64,
    dequeue_completed_us: u64,
}

/// Original source timing plus the time this acceptance check was made.
/// `source_age_us` dates the driver's stated point, not a guessed exposure
/// start. None of these fields renews a result's freshness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timing {
    pub driver_timestamp: DriverTimestamp,
    pub dequeue_started_us: u64,
    pub dequeue_completed_us: u64,
    pub ready_at_us: u64,
    pub source_age_us: u64,
}

#[derive(Debug)]
struct Active {
    capture: CaptureId,
    source: SourceId,
    opening_start_us: u64,
    permission: u64,
    last_driver_sequence: Option<u32>,
    last_driver_timestamp: Option<DriverTimestamp>,
}

/// Constant-size state for one capture and at most one pending result.
///
/// Retain one guard for the observer's lifetime. Clear it on stop, camera
/// faults or permission loss. Beginning another capture, clearing, and every
/// observation attempt invalidate the previous ticket, even if IDs are
/// reused or the new attempt fails. The host clock high-water mark survives
/// clearing and reopening. Generation exhaustion is permanent.
#[derive(Debug)]
pub struct Guard {
    max_age_us: u64,
    generation: u64,
    exhausted: bool,
    last_host_us: u64,
    active: Option<Active>,
    pending: Option<Ticket>,
}

impl Guard {
    /// `max_age_us` is an exclusive freshness ceiling, bounded by the camera
    /// contract. It is separate from any measured inference-latency target.
    pub fn new(max_age_us: u64) -> Result<Self, Reason> {
        if max_age_us == 0 || max_age_us > MAX_DEQUEUE_AGE_US {
            return Err(Reason::InvalidMaxAge);
        }
        Ok(Self {
            max_age_us,
            generation: 0,
            exhausted: false,
            last_host_us: 0,
            active: None,
            pending: None,
        })
    }

    /// Begin only after an opening whose original permission still holds.
    /// `permission_before` must actually have been sampled before opening;
    /// supplying the post-open sample twice cannot establish continuity.
    /// A failed begin also invalidates the previous capture and ticket. Both
    /// host samples must follow the lifetime high-water mark; a valid start
    /// still advances it when the following end or permission is rejected.
    pub fn begin(
        &mut self,
        capture: CaptureId,
        source: SourceId,
        opening_start_us: u64,
        opening_end_us: u64,
        permission_before: Option<u64>,
        permission_after: Option<u64>,
    ) -> Result<(), Reason> {
        self.active = None;
        self.invalidate()?;
        self.check_host_time(opening_start_us)?;
        self.check_host_time(opening_end_us)?;
        let permission = permission_before.ok_or(Reason::PermissionMissing)?;
        check_permission(permission, permission_after)?;
        self.active = Some(Active {
            capture,
            source,
            opening_start_us,
            permission,
            last_driver_sequence: None,
            last_driver_timestamp: None,
        });
        Ok(())
    }

    /// Drop capture metadata and invalidate all tickets without resetting the
    /// host clock high-water mark. Overflow seals the guard permanently;
    /// subsequent methods return `GenerationExhausted`.
    pub fn clear(&mut self) {
        self.active = None;
        let _ = self.invalidate();
    }

    /// Observe metadata from one completed dequeue, without copying image
    /// bytes. The caller must also check permission before the read; the
    /// unchanged generation here proves that check was not superseded.
    ///
    /// Every attempt invalidates the prior ticket. Once capture, source,
    /// permission and host timing are valid, its forward sequence is consumed
    /// even if the timestamp is rejected. An unusable newer frame therefore
    /// cannot be followed by replaying its sequence as a different result.
    /// Observed permission loss/change disarms the capture until `begin`.
    pub fn observe(
        &mut self,
        frame: &PortFrame,
        dequeue_start_us: u64,
        dequeue_end_us: u64,
        current_permission: Option<u64>,
    ) -> Result<Ticket, Reason> {
        self.invalidate()?;
        self.check_host_time(dequeue_start_us)?;
        self.check_host_time(dequeue_end_us)?;
        let active = self.active.as_mut().ok_or(Reason::NoCapture)?;
        if let Err(reason) = check_permission(active.permission, current_permission) {
            self.active = None;
            return Err(reason);
        }
        if frame.capture != active.capture {
            return Err(Reason::CaptureMismatch);
        }
        if frame.source != active.source {
            return Err(Reason::SourceMismatch);
        }
        if let Some(previous) = active.last_driver_sequence {
            let delta = frame.driver_sequence.wrapping_sub(previous);
            if delta == 0 {
                return Err(Reason::SequenceDuplicate);
            }
            if delta >= (1 << 31) {
                return Err(Reason::SequenceRegressed);
            }
        }
        active.last_driver_sequence = Some(frame.driver_sequence);
        let timestamp = frame.timestamp.ok_or(Reason::TimestampMissing)?;
        if timestamp.domain != TimestampDomain::HostMonotonic {
            return Err(Reason::TimestampDomain);
        }
        if timestamp.point == TimestampPoint::Unknown {
            return Err(Reason::TimestampPoint);
        }
        if timestamp.micros == 0 {
            return Err(Reason::TimestampZero);
        }
        if timestamp.micros < active.opening_start_us {
            return Err(Reason::TimestampBeforeOpen);
        }
        if timestamp.micros > dequeue_end_us {
            return Err(Reason::TimestampFuture);
        }
        if let Some(previous) = active.last_driver_timestamp {
            if timestamp.point != previous.point {
                return Err(Reason::TimestampPointChanged);
            }
            if timestamp.micros < previous.micros {
                return Err(Reason::TimestampRegressed);
            }
        }
        active.last_driver_timestamp = Some(timestamp);
        if dequeue_end_us - timestamp.micros >= self.max_age_us {
            return Err(Reason::Stale);
        }
        let ticket = Ticket {
            generation: self.generation,
            capture: active.capture,
            permission: active.permission,
            driver_sequence: frame.driver_sequence,
            driver_timestamp: timestamp,
            dequeue_started_us: dequeue_start_us,
            dequeue_completed_us: dequeue_end_us,
        };
        self.pending = Some(ticket);
        Ok(ticket)
    }

    /// Recheck the original metadata immediately before using a result.
    /// This does not consume the ticket, modify state or advance any clock.
    /// The supplied `now_us` must be a current clock sample; this read-only
    /// method cannot compare it with a previous call's acceptance time.
    pub fn accept(
        &self,
        ticket: Ticket,
        now_us: u64,
        current_permission: Option<u64>,
    ) -> Result<Timing, Reason> {
        if self.exhausted {
            return Err(Reason::GenerationExhausted);
        }
        if self.pending != Some(ticket) || ticket.generation != self.generation {
            return Err(Reason::Superseded);
        }
        let active = self.active.as_ref().ok_or(Reason::NoCapture)?;
        check_permission(active.permission, current_permission)?;
        if ticket.capture != active.capture
            || ticket.permission != active.permission
            || Some(ticket.driver_sequence) != active.last_driver_sequence
        {
            return Err(Reason::Superseded);
        }
        if now_us < self.last_host_us || now_us < ticket.dequeue_completed_us {
            return Err(Reason::ClockInvalid);
        }
        let source_age_us = now_us
            .checked_sub(ticket.driver_timestamp.micros)
            .ok_or(Reason::ClockInvalid)?;
        if source_age_us >= self.max_age_us {
            return Err(Reason::Stale);
        }
        Ok(Timing {
            driver_timestamp: ticket.driver_timestamp,
            dequeue_started_us: ticket.dequeue_started_us,
            dequeue_completed_us: ticket.dequeue_completed_us,
            ready_at_us: now_us,
            source_age_us,
        })
    }

    /// Like camera Capture's clock check: a rejected sample cannot lower the
    /// last accepted host time, including across capture lifecycle changes.
    fn check_host_time(&mut self, now_us: u64) -> Result<(), Reason> {
        if now_us < self.last_host_us {
            return Err(Reason::ClockInvalid);
        }
        self.last_host_us = now_us;
        Ok(())
    }

    fn invalidate(&mut self) -> Result<(), Reason> {
        self.pending = None;
        if !self.exhausted
            && let Some(next) = self.generation.checked_add(1)
        {
            self.generation = next;
            return Ok(());
        }
        self.exhausted = true;
        self.active = None;
        Err(Reason::GenerationExhausted)
    }
}

fn check_permission(expected: u64, current: Option<u64>) -> Result<(), Reason> {
    match current {
        None => Err(Reason::PermissionMissing),
        Some(current) if current != expected => Err(Reason::PermissionChanged),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_turn_owner::BootId;
    use std::num::NonZeroU64;

    const HOLD: Option<u64> = Some(7);

    fn capture(worker: u8, epoch: u64) -> CaptureId {
        CaptureId {
            worker: BootId::new([worker; 16]).unwrap(),
            epoch: NonZeroU64::new(epoch).unwrap(),
        }
    }

    fn source() -> SourceId {
        SourceId::new("eyes").unwrap()
    }

    fn frame(sequence: u32, micros: u64) -> PortFrame {
        PortFrame {
            capture: capture(1, 1),
            source: source(),
            bytes_used: 4,
            driver_sequence: sequence,
            timestamp: Some(DriverTimestamp {
                micros,
                domain: TimestampDomain::HostMonotonic,
                point: TimestampPoint::StartOfExposure,
            }),
        }
    }

    fn opened_guard(max_age_us: u64) -> Guard {
        let mut guard = Guard::new(max_age_us).unwrap();
        guard
            .begin(capture(1, 1), source(), 100, 110, HOLD, HOLD)
            .unwrap();
        guard
    }

    fn first(guard: &mut Guard) -> Ticket {
        guard.observe(&frame(1, 120), 125, 130, HOLD).unwrap()
    }

    #[test]
    fn the_age_ceiling_is_bounded_and_is_not_a_latency_target() {
        for age in [0, MAX_DEQUEUE_AGE_US + 1, u64::MAX] {
            assert_eq!(Guard::new(age).unwrap_err(), Reason::InvalidMaxAge);
        }
        assert!(Guard::new(1).is_ok());
        assert!(Guard::new(MAX_DEQUEUE_AGE_US).is_ok());
    }

    #[test]
    fn diagnostic_codes_are_fixed_distinct_keys_without_source_metadata() {
        let reasons = [
            Reason::InvalidMaxAge,
            Reason::GenerationExhausted,
            Reason::NoCapture,
            Reason::PermissionMissing,
            Reason::PermissionChanged,
            Reason::ClockInvalid,
            Reason::CaptureMismatch,
            Reason::SourceMismatch,
            Reason::TimestampMissing,
            Reason::TimestampDomain,
            Reason::TimestampPoint,
            Reason::TimestampPointChanged,
            Reason::TimestampZero,
            Reason::TimestampBeforeOpen,
            Reason::TimestampFuture,
            Reason::TimestampRegressed,
            Reason::SequenceDuplicate,
            Reason::SequenceRegressed,
            Reason::Stale,
            Reason::Superseded,
        ];
        for (index, reason) in reasons.into_iter().enumerate() {
            let code = reason.code();
            assert!(!code.is_empty() && code.len() <= 32);
            assert!(
                code.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            );
            assert!(reasons[..index].iter().all(|before| before.code() != code));
        }
        let mut guard = opened_guard(100);
        let mut frame = frame(1, 120);
        frame.source = SourceId::new("private-source-canary").unwrap();
        assert_eq!(
            guard.observe(&frame, 125, 130, HOLD).unwrap_err().code(),
            "source_mismatch"
        );
    }

    #[test]
    fn both_source_points_remain_exact_at_receipt_and_acceptance() {
        for point in [TimestampPoint::StartOfExposure, TimestampPoint::EndOfFrame] {
            let mut guard = opened_guard(100);
            let mut frame = frame(1, 120);
            frame.timestamp.as_mut().unwrap().point = point;
            let ticket = guard.observe(&frame, 125, 130, HOLD).unwrap();
            assert_eq!(
                guard.accept(ticket, 150, HOLD),
                Ok(Timing {
                    driver_timestamp: frame.timestamp.unwrap(),
                    dequeue_started_us: 125,
                    dequeue_completed_us: 130,
                    ready_at_us: 150,
                    source_age_us: 30,
                })
            );
        }
    }

    #[test]
    fn missing_zero_or_mixed_source_clocks_never_become_host_now() {
        let original = frame(1, 120);
        let timestamp = original.timestamp.unwrap();
        let cases = [
            (None, Reason::TimestampMissing),
            (
                Some(DriverTimestamp {
                    micros: 0,
                    ..timestamp
                }),
                Reason::TimestampZero,
            ),
            (
                Some(DriverTimestamp {
                    domain: TimestampDomain::Realtime,
                    ..timestamp
                }),
                Reason::TimestampDomain,
            ),
            (
                Some(DriverTimestamp {
                    domain: TimestampDomain::Unknown,
                    ..timestamp
                }),
                Reason::TimestampDomain,
            ),
            (
                Some(DriverTimestamp {
                    point: TimestampPoint::Unknown,
                    ..timestamp
                }),
                Reason::TimestampPoint,
            ),
        ];
        for (timestamp, reason) in cases {
            let mut guard = opened_guard(100);
            let frame = PortFrame {
                timestamp,
                ..original
            };
            assert_eq!(guard.observe(&frame, 125, 130, HOLD), Err(reason));
            assert!(guard.pending.is_none());
        }
    }

    #[test]
    fn capture_time_must_be_after_open_start_and_not_after_dequeue_end() {
        for (micros, wanted) in [
            (99, Some(Reason::TimestampBeforeOpen)),
            (100, None),
            (130, None),
            (131, Some(Reason::TimestampFuture)),
            (u64::MAX, Some(Reason::TimestampFuture)),
        ] {
            let result = opened_guard(100).observe(&frame(1, micros), 125, 130, HOLD);
            assert_eq!(result.err(), wanted);
        }
    }

    #[test]
    fn receipt_and_completion_share_the_original_exclusive_age_ceiling() {
        let mut guard = opened_guard(100);
        let ticket = first(&mut guard);
        assert_eq!(guard.accept(ticket, 219, HOLD).unwrap().source_age_us, 99);
        assert_eq!(guard.accept(ticket, 220, HOLD), Err(Reason::Stale));
        assert_eq!(guard.accept(ticket, u64::MAX, HOLD), Err(Reason::Stale));

        let mut guard = opened_guard(100);
        assert_eq!(
            guard.observe(&frame(1, 120), 210, 220, HOLD),
            Err(Reason::Stale)
        );
        assert!(guard.pending.is_none());
    }

    #[test]
    fn acceptance_is_read_only_and_does_not_refresh_age_or_consume_a_ticket() {
        let mut guard = opened_guard(100);
        let ticket = first(&mut guard);
        let before = format!("{guard:?}");
        let first = guard.accept(ticket, 180, HOLD).unwrap();
        assert_eq!(guard.accept(ticket, 180, HOLD), Ok(first));
        assert_eq!(guard.accept(ticket, 200, HOLD).unwrap().source_age_us, 80);
        assert_eq!(guard.accept(ticket, 220, HOLD), Err(Reason::Stale));
        assert_eq!(format!("{guard:?}"), before);
    }

    #[test]
    fn permission_is_required_and_unchanged_across_opening() {
        for (before, after, reason) in [
            (None, None, Reason::PermissionMissing),
            (None, HOLD, Reason::PermissionMissing),
            (HOLD, None, Reason::PermissionMissing),
            (HOLD, Some(8), Reason::PermissionChanged),
        ] {
            let mut guard = opened_guard(100);
            let ticket = first(&mut guard);
            assert_eq!(
                guard.begin(capture(1, 2), source(), 140, 150, before, after),
                Err(reason)
            );
            assert_eq!(guard.accept(ticket, 150, HOLD), Err(Reason::Superseded));
            assert_eq!(
                guard.observe(&frame(2, 160), 165, 170, HOLD),
                Err(Reason::NoCapture)
            );
        }
    }

    #[test]
    fn revoke_expiry_or_close_regrant_during_read_drops_the_old_result() {
        for (permission, reason) in [
            (None, Reason::PermissionMissing),
            (Some(8), Reason::PermissionChanged),
        ] {
            let mut guard = opened_guard(100);
            let ticket = first(&mut guard);
            assert_eq!(
                guard.observe(&frame(2, 140), 145, 150, permission),
                Err(reason)
            );
            assert_eq!(guard.accept(ticket, 150, HOLD), Err(Reason::Superseded));
            // Even a caller that supplies the old generation again cannot
            // revive this capture after the guard observed permission loss.
            assert_eq!(
                guard.observe(&frame(3, 160), 165, 170, HOLD),
                Err(Reason::NoCapture)
            );
        }
    }

    #[test]
    fn permission_is_rechecked_after_inference_without_mutating_state() {
        let mut guard = opened_guard(100);
        let ticket = first(&mut guard);
        let before = format!("{guard:?}");
        assert_eq!(
            guard.accept(ticket, 150, None),
            Err(Reason::PermissionMissing)
        );
        assert_eq!(
            guard.accept(ticket, 150, Some(8)),
            Err(Reason::PermissionChanged)
        );
        assert_eq!(format!("{guard:?}"), before);
    }

    #[test]
    fn opening_and_dequeue_clock_boundaries_are_checked() {
        let mut guard = opened_guard(100);
        let ticket = first(&mut guard);
        assert_eq!(
            guard.begin(capture(1, 2), source(), 151, 150, HOLD, HOLD),
            Err(Reason::ClockInvalid)
        );
        assert_eq!(guard.accept(ticket, 160, HOLD), Err(Reason::Superseded));

        for (start, end) in [(109, 130), (125, 124)] {
            assert_eq!(
                opened_guard(100).observe(&frame(1, 120), start, end, HOLD),
                Err(Reason::ClockInvalid)
            );
        }
        let mut guard = opened_guard(100);
        let ticket = first(&mut guard);
        assert_eq!(guard.accept(ticket, 129, HOLD), Err(Reason::ClockInvalid));
        assert_eq!(
            guard.observe(&frame(2, 140), 129, 150, HOLD),
            Err(Reason::ClockInvalid)
        );
        assert_eq!(guard.accept(ticket, 150, HOLD), Err(Reason::Superseded));
    }

    #[test]
    fn source_worker_and_capture_epoch_must_match_the_opening() {
        for (frame, reason) in [
            (
                PortFrame {
                    source: SourceId::new("other").unwrap(),
                    ..frame(2, 140)
                },
                Reason::SourceMismatch,
            ),
            (
                PortFrame {
                    capture: capture(2, 1),
                    ..frame(2, 140)
                },
                Reason::CaptureMismatch,
            ),
            (
                PortFrame {
                    capture: capture(1, 2),
                    ..frame(2, 140)
                },
                Reason::CaptureMismatch,
            ),
        ] {
            let mut guard = opened_guard(100);
            let ticket = first(&mut guard);
            assert_eq!(guard.observe(&frame, 145, 150, HOLD), Err(reason));
            assert_eq!(guard.accept(ticket, 150, HOLD), Err(Reason::Superseded));
        }
    }

    #[test]
    fn driver_sequence_accepts_forward_wrap_but_not_duplicates_or_regression() {
        let mut guard = opened_guard(100);
        let mut previous = None;
        for (index, sequence) in [u32::MAX - 1, u32::MAX, 0, 1].into_iter().enumerate() {
            let micros = 120 + index as u64 * 20;
            let ticket = guard
                .observe(&frame(sequence, micros), micros + 5, micros + 10, HOLD)
                .unwrap();
            if let Some(old) = previous {
                assert_eq!(
                    guard.accept(old, micros + 10, HOLD),
                    Err(Reason::Superseded)
                );
            }
            assert!(guard.accept(ticket, micros + 10, HOLD).is_ok());
            previous = Some(ticket);
        }
        for (index, (sequence, reason)) in [
            (1, Reason::SequenceDuplicate),
            (0, Reason::SequenceRegressed),
            (1_u32.wrapping_add(1 << 31), Reason::SequenceRegressed),
        ]
        .into_iter()
        .enumerate()
        {
            let micros = 200 + index as u64 * 20;
            assert_eq!(
                guard.observe(&frame(sequence, micros), micros + 5, micros + 10, HOLD),
                Err(reason)
            );
        }
    }

    #[test]
    fn timestamp_point_and_order_cannot_change_inside_one_capture() {
        let mut guard = opened_guard(100);
        first(&mut guard);
        assert_eq!(
            guard.observe(&frame(2, 119), 145, 150, HOLD),
            Err(Reason::TimestampRegressed)
        );
        let mut changed = frame(3, 160);
        changed.timestamp.as_mut().unwrap().point = TimestampPoint::EndOfFrame;
        assert_eq!(
            guard.observe(&changed, 165, 170, HOLD),
            Err(Reason::TimestampPointChanged)
        );
        // Equal timestamp values with a forward sequence are allowed, as in
        // camera Capture; an earlier value was not installed by the failure.
        assert!(guard.observe(&frame(4, 120), 175, 180, HOLD).is_ok());
    }

    #[test]
    fn every_failed_newer_observation_supersedes_a_pending_result() {
        let mut guard = opened_guard(100);
        let ticket = first(&mut guard);
        assert_eq!(
            guard.observe(&frame(2, 140), 230, 240, HOLD),
            Err(Reason::Stale)
        );
        assert_eq!(guard.accept(ticket, 240, HOLD), Err(Reason::Superseded));
        assert_eq!(
            guard.observe(&frame(2, 250), 255, 260, HOLD),
            Err(Reason::SequenceDuplicate)
        );
        assert!(guard.observe(&frame(3, 270), 275, 280, HOLD).is_ok());
    }

    #[test]
    fn timestamp_failure_consumes_sequence_but_does_not_replace_valid_source_time() {
        let mut guard = opened_guard(100);
        first(&mut guard);
        assert_eq!(
            guard.observe(&frame(2, u64::MAX), 145, 150, HOLD),
            Err(Reason::TimestampFuture)
        );
        assert_eq!(
            guard.observe(&frame(2, 160), 165, 170, HOLD),
            Err(Reason::SequenceDuplicate)
        );
        assert!(guard.observe(&frame(3, 180), 185, 190, HOLD).is_ok());
    }

    #[test]
    fn clear_and_begin_invalidate_tickets_even_when_ids_are_reused() {
        let mut guard = opened_guard(100);
        let old = first(&mut guard);
        guard.clear();
        assert_eq!(guard.accept(old, 150, HOLD), Err(Reason::Superseded));
        assert_eq!(
            guard.observe(&frame(1, 120), 145, 150, HOLD),
            Err(Reason::NoCapture)
        );
        guard
            .begin(capture(1, 1), source(), 200, 210, HOLD, HOLD)
            .unwrap();
        assert_eq!(
            guard.observe(&frame(1, 120), 215, 220, HOLD),
            Err(Reason::TimestampBeforeOpen)
        );
        let newer = guard.observe(&frame(2, 230), 235, 240, HOLD).unwrap();
        assert_ne!(old, newer);
        assert_eq!(guard.accept(old, 250, HOLD), Err(Reason::Superseded));
        assert!(guard.accept(newer, 250, HOLD).is_ok());

        guard
            .begin(capture(1, 1), source(), 300, 310, HOLD, HOLD)
            .unwrap();
        let newest = guard.observe(&frame(1, 320), 325, 330, HOLD).unwrap();
        assert_eq!(guard.accept(newer, 350, HOLD), Err(Reason::Superseded));
        assert!(guard.accept(newest, 350, HOLD).is_ok());
    }

    #[test]
    fn host_clock_survives_clear_failed_open_and_permission_loss() {
        for clear_first in [false, true] {
            let mut guard = opened_guard(100);
            let old = first(&mut guard);
            if clear_first {
                guard.clear();
                guard.clear();
            }
            // Reusing the capture identity does not allow the opening clock
            // to return to the old opening's times after a dequeue at 130.
            assert_eq!(
                guard.begin(capture(1, 1), source(), 100, 110, HOLD, HOLD),
                Err(Reason::ClockInvalid)
            );
            assert_eq!(guard.accept(old, 150, HOLD), Err(Reason::Superseded));
            assert_eq!(
                guard.begin(capture(1, 1), source(), 129, 200, HOLD, HOLD),
                Err(Reason::ClockInvalid)
            );
            // A valid start is retained even if the later end regresses.
            assert_eq!(
                guard.begin(capture(1, 1), source(), 140, 139, HOLD, HOLD),
                Err(Reason::ClockInvalid)
            );
            assert_eq!(
                guard.begin(capture(1, 1), source(), 139, 150, HOLD, HOLD),
                Err(Reason::ClockInvalid)
            );
            // Failed authorization still leaves the coherent host samples.
            assert_eq!(
                guard.begin(capture(1, 1), source(), 140, 150, HOLD, None),
                Err(Reason::PermissionMissing)
            );
            assert_eq!(
                guard.begin(capture(1, 1), source(), 149, 160, HOLD, HOLD),
                Err(Reason::ClockInvalid)
            );
            guard
                .begin(capture(1, 1), source(), 150, 160, HOLD, HOLD)
                .unwrap();
            assert_eq!(
                guard.observe(&frame(1, 170), 175, 180, None),
                Err(Reason::PermissionMissing)
            );
            assert_eq!(
                guard.begin(capture(1, 1), source(), 170, 190, HOLD, HOLD),
                Err(Reason::ClockInvalid)
            );
            guard
                .begin(capture(1, 1), source(), 180, 190, HOLD, HOLD)
                .unwrap();
        }
    }

    #[test]
    fn rejected_dequeue_clocks_cannot_lower_the_lifetime_high_water_mark() {
        let mut guard = opened_guard(100);
        let old = first(&mut guard);
        assert_eq!(
            guard.observe(&frame(2, 150), 160, 159, HOLD),
            Err(Reason::ClockInvalid)
        );
        assert_eq!(guard.accept(old, 170, HOLD), Err(Reason::Superseded));
        assert_eq!(
            guard.observe(&frame(3, 150), 159, 170, HOLD),
            Err(Reason::ClockInvalid)
        );
        let ticket = guard.observe(&frame(4, 165), 170, 180, HOLD).unwrap();
        assert!(guard.accept(ticket, 180, HOLD).is_ok());
        guard.clear();
        assert_eq!(
            guard.begin(capture(1, 1), source(), 175, 190, HOLD, HOLD),
            Err(Reason::ClockInvalid)
        );
        guard
            .begin(capture(1, 1), source(), 180, 190, HOLD, HOLD)
            .unwrap();
    }

    #[test]
    fn a_new_capture_can_restart_sequence_time_point_and_permission() {
        let mut guard = opened_guard(100);
        let old = first(&mut guard);
        guard
            .begin(capture(2, 1), source(), 200, 210, Some(8), Some(8))
            .unwrap();
        let mut newer = frame(0, 220);
        newer.capture = capture(2, 1);
        newer.timestamp.as_mut().unwrap().point = TimestampPoint::EndOfFrame;
        let ticket = guard.observe(&newer, 225, 230, Some(8)).unwrap();
        assert_eq!(guard.accept(old, 240, Some(8)), Err(Reason::Superseded));
        assert_eq!(
            guard.accept(ticket, 240, HOLD),
            Err(Reason::PermissionChanged)
        );
        assert!(guard.accept(ticket, 240, Some(8)).is_ok());
    }

    #[test]
    fn generation_exhaustion_seals_all_entry_points_permanently() {
        for overflow_at_clear in [false, true] {
            let mut guard = opened_guard(100);
            guard.generation = u64::MAX - 1;
            let ticket = first(&mut guard);
            assert!(guard.accept(ticket, 150, HOLD).is_ok());
            if overflow_at_clear {
                guard.clear();
            } else {
                assert_eq!(
                    guard.observe(&frame(2, 160), 165, 170, HOLD),
                    Err(Reason::GenerationExhausted)
                );
            }
            assert!(guard.active.is_none());
            assert!(guard.pending.is_none());
            assert_eq!(
                guard.accept(ticket, 180, HOLD),
                Err(Reason::GenerationExhausted)
            );
            assert_eq!(
                guard.begin(capture(1, 2), source(), 200, 210, HOLD, HOLD),
                Err(Reason::GenerationExhausted)
            );
            guard.clear();
            assert_eq!(
                guard.observe(&frame(1, 220), 225, 230, HOLD),
                Err(Reason::GenerationExhausted)
            );
        }
    }

    #[test]
    fn fixed_state_stays_bounded_across_many_attempts() {
        assert!(std::mem::size_of::<Guard>() <= 1_024);
        assert!(std::mem::size_of::<Ticket>() <= 128);
        assert!(!std::mem::needs_drop::<Guard>());
        let mut guard = opened_guard(100);
        for sequence in 0..10_000 {
            let micros = 120 + u64::from(sequence) * 20;
            let ticket = guard
                .observe(&frame(sequence, micros), micros + 5, micros + 10, HOLD)
                .unwrap();
            assert!(guard.accept(ticket, micros + 10, HOLD).is_ok());
        }
    }

    #[test]
    fn large_monotonic_values_do_not_wrap_source_age() {
        let mut guard = Guard::new(100).unwrap();
        guard
            .begin(
                capture(1, 1),
                source(),
                u64::MAX - 100,
                u64::MAX - 90,
                HOLD,
                HOLD,
            )
            .unwrap();
        let ticket = guard
            .observe(&frame(1, u64::MAX - 80), u64::MAX - 70, u64::MAX - 60, HOLD)
            .unwrap();
        assert_eq!(
            guard.accept(ticket, u64::MAX, HOLD).unwrap().source_age_us,
            80
        );
        assert_eq!(guard.accept(ticket, 0, HOLD), Err(Reason::ClockInvalid));
    }
}
