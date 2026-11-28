//! Inactive, bounded history for a future confirmation reader (#8971).
//!
//! This is not a presence, identity or liveness detector. It has no runtime
//! caller, default operating limits, model, I/O, bus output or retained pixels. Passing ids
//! come from an upstream one-to-one tracker, such as `bus_out::Ids`; there is
//! no second geometric matcher here. Even measured movement is not proof of
//! a person. A caller must bind captures/epochs and recheck current authority.

/// Work/storage bound, not a qualified number of people to recognize.
pub const MAX_TRACKS: usize = 8;

/// No defaults: these durations need a separately evaluated operating point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub window_us: u64,
    pub maximum_gap_us: u64,
    pub maximum_frame_age_us: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capture {
    /// A local increasing generation, changed on camera/privacy restart.
    /// This number is not a camera grant or proof of authorization.
    pub epoch: u64,
    pub sequence: u64,
    pub captured_us: u64,
}

/// Refers to the entire interval since the preceding capture, not just a
/// latest joint reading. A caller unable to establish it must use `Unknown`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CameraInterval {
    KnownStill,
    Moved,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub track_id: u32,
    /// Centre in the filled image's normalized coordinates, each in [0,1].
    pub centre: [f32; 2],
    /// Caller-supplied quality result, never a claim of physical presence.
    /// False breaks this track's window instead of bridging missing evidence.
    pub usable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rejection {
    InvalidLimits,
    Closed,
    Epoch,
    Time,
    Capacity,
    DuplicateTrack,
    Geometry,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub track_id: u32,
    pub samples: u32,
    pub span_us: u64,
    /// Current centre minus this window's first centre. Absent for the
    /// first sample: two faces in one capture cannot be a temporal delta.
    pub displacement: Option<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameHistory {
    pub capture: Capture,
    pub camera_interval: CameraInterval,
    /// Input order; unusable candidates and unused slots stay None.
    pub windows: [Option<Window>; MAX_TRACKS],
}

#[derive(Clone, Copy, Debug)]
struct Track {
    id: u32,
    origin: [f32; 2],
    origin_us: u64,
    seen_us: u64,
    samples: u32,
}

#[derive(Debug)]
pub struct History {
    limits: Limits,
    epoch: u64,
    open: bool,
    /// Last consumed capture and the local time at which it was processed.
    last: Option<(Capture, u64)>,
    tracks: [Option<Track>; MAX_TRACKS],
}

impl History {
    pub fn new(epoch: u64, limits: Limits) -> Result<Self, Rejection> {
        if limits.window_us == 0 || limits.maximum_gap_us == 0 || limits.maximum_frame_age_us == 0 {
            return Err(Rejection::InvalidLimits);
        }
        Ok(Self {
            limits,
            epoch,
            open: true,
            last: None,
            tracks: [None; MAX_TRACKS],
        })
    }

    /// Forget all evidence and stop accepting frames, including ones from
    /// the old epoch. Returned numbers are not revocable capabilities.
    pub fn close(&mut self) {
        self.tracks.fill(None);
        self.last = None;
        self.open = false;
    }

    /// Reopening requires a strictly newer epoch; replay/overflow never wraps.
    pub fn begin_epoch(&mut self, epoch: u64) -> Result<(), Rejection> {
        self.close();
        if epoch <= self.epoch {
            return Err(Rejection::Epoch);
        }
        self.epoch = epoch;
        self.open = true;
        Ok(())
    }

    fn reject<T>(&mut self, why: Rejection) -> Result<T, Rejection> {
        self.tracks.fill(None);
        Err(why)
    }

    /// One complete frame, with no partial updates or same-frame matching.
    /// Any invalid input breaks history. Missing/unusable ids break their own
    /// history; aliases/refound ids never inherit another id's observations.
    pub fn observe(
        &mut self,
        capture: Capture,
        processed_us: u64,
        camera_interval: CameraInterval,
        candidates: &[Candidate],
    ) -> Result<FrameHistory, Rejection> {
        if !self.open {
            return self.reject(Rejection::Closed);
        }
        if capture.epoch != self.epoch {
            return self.reject(Rejection::Epoch);
        }
        if capture.captured_us > processed_us
            || processed_us - capture.captured_us > self.limits.maximum_frame_age_us
            || self.last.is_some_and(|(last, processed)| {
                capture.sequence <= last.sequence
                    || capture.captured_us <= last.captured_us
                    || processed_us < processed
            })
        {
            return self.reject(Rejection::Time);
        }
        // A valid capture stamp is consumed even if its candidate list is bad.
        // The same frame cannot be replayed with different candidates later.
        self.last = Some((capture, processed_us));
        if candidates.len() > MAX_TRACKS {
            return self.reject(Rejection::Capacity);
        }
        for (index, candidate) in candidates.iter().enumerate() {
            if candidates[..index]
                .iter()
                .any(|other| other.track_id == candidate.track_id)
            {
                return self.reject(Rejection::DuplicateTrack);
            }
            if candidate
                .centre
                .iter()
                .any(|value| !(0.0..=1.0).contains(value))
            {
                return self.reject(Rejection::Geometry);
            }
        }
        if camera_interval != CameraInterval::KnownStill {
            self.tracks.fill(None);
        }
        for slot in &mut self.tracks {
            if slot.is_some_and(|track| {
                capture.captured_us - track.seen_us > self.limits.maximum_gap_us
                    || !candidates
                        .iter()
                        .any(|candidate| candidate.track_id == track.id && candidate.usable)
            }) {
                *slot = None;
            }
        }
        let mut result = FrameHistory {
            capture,
            camera_interval,
            windows: [None; MAX_TRACKS],
        };
        for (index, candidate) in candidates.iter().enumerate() {
            if !candidate.usable {
                continue;
            }
            let existing = self
                .tracks
                .iter()
                .position(|slot| slot.is_some_and(|track| track.id == candidate.track_id));
            // Each retained id is unique and appears in this bounded batch,
            // so every new id has an empty slot; no observed track is evicted.
            let at = existing
                .or_else(|| self.tracks.iter().position(Option::is_none))
                .expect("a bounded unique candidate batch has a free slot");
            let slot = &mut self.tracks[at];
            // Expire the origin BEFORE measuring displacement. In particular,
            // repeated stationary samples do not extend the old origin's life.
            if slot
                .is_none_or(|track| capture.captured_us - track.origin_us > self.limits.window_us)
            {
                *slot = Some(Track {
                    id: candidate.track_id,
                    origin: candidate.centre,
                    origin_us: capture.captured_us,
                    seen_us: capture.captured_us,
                    samples: 0,
                });
            }
            let track = slot.as_mut().expect("track was established");
            track.samples = track.samples.saturating_add(1);
            track.seen_us = capture.captured_us;
            let span_us = capture.captured_us - track.origin_us;
            result.windows[index] = Some(Window {
                track_id: track.id,
                samples: track.samples,
                span_us,
                displacement: (span_us > 0).then_some([
                    candidate.centre[0] - track.origin[0],
                    candidate.centre[1] - track.origin[1],
                ]),
            });
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> Limits {
        // Authored control durations, not proposed product settings.
        Limits {
            window_us: 10_000_000,
            maximum_gap_us: 5_000_000,
            maximum_frame_age_us: 1_000_000,
        }
    }

    fn capture(sequence: u64, captured_us: u64) -> Capture {
        Capture {
            epoch: 7,
            sequence,
            captured_us,
        }
    }

    fn candidate(id: u32, x: f32) -> Candidate {
        Candidate {
            track_id: id,
            centre: [x, 0.5],
            usable: true,
        }
    }

    fn observe(
        history: &mut History,
        sequence: u64,
        at: u64,
        values: &[Candidate],
    ) -> FrameHistory {
        history
            .observe(
                capture(sequence, at),
                at,
                CameraInterval::KnownStill,
                values,
            )
            .unwrap()
    }

    #[test]
    fn two_faces_in_one_frame_cannot_be_a_move_or_share_a_window() {
        let mut ids = crate::bus_out::Ids::default();
        let boxes = [[0.4, 0.4, 0.5, 0.6], [0.49, 0.4, 0.59, 0.6]];
        let tracked = ids.of(&boxes, 0);
        assert_ne!(tracked[0], tracked[1]);
        let faces = [candidate(tracked[0], 0.45), candidate(tracked[1], 0.54)];
        let mut history = History::new(7, limits()).unwrap();
        let first = observe(&mut history, 1, 0, &faces);
        for window in first.windows.into_iter().flatten() {
            assert_eq!(window.samples, 1);
            assert_eq!(window.displacement, None);
        }
        let second = observe(&mut history, 2, 250_000, &faces);
        for window in second.windows.into_iter().flatten() {
            assert_eq!(window.samples, 2);
            assert_eq!(window.displacement, Some([0.0, 0.0]));
        }
    }

    #[test]
    fn expired_origin_is_reset_before_late_movement() {
        let mut history = History::new(7, limits()).unwrap();
        for (seq, at) in [(1, 0), (2, 4_000_000), (3, 8_000_000)] {
            observe(&mut history, seq, at, &[candidate(1, 0.45)]);
        }
        let last = observe(&mut history, 4, 11_000_000, &[candidate(1, 0.51)]);
        let window = last.windows[0].unwrap();
        assert_eq!(window.samples, 1);
        assert_eq!(window.span_us, 0);
        assert_eq!(window.displacement, None);
    }

    #[test]
    fn positive_interval_and_exact_window_boundary_are_measured() {
        let mut history = History::new(7, limits()).unwrap();
        observe(&mut history, 1, 0, &[candidate(1, 0.25)]);
        observe(&mut history, 2, 5_000_000, &[candidate(1, 0.25)]);
        let exact = observe(&mut history, 3, 10_000_000, &[candidate(1, 0.5)]);
        assert_eq!(exact.windows[0].unwrap().displacement, Some([0.25, 0.0]));
        assert_eq!(exact.windows[0].unwrap().span_us, 10_000_000);
    }

    #[test]
    fn repeated_out_of_order_and_same_time_frames_break_history() {
        for bad in [capture(2, 101), capture(1, 102), capture(3, 100)] {
            let mut history = History::new(7, limits()).unwrap();
            observe(&mut history, 1, 0, &[candidate(1, 0.25)]);
            observe(&mut history, 2, 100, &[candidate(1, 0.25)]);
            assert_eq!(
                history.observe(bad, 200, CameraInterval::KnownStill, &[candidate(1, 0.5)]),
                Err(Rejection::Time)
            );
            assert_eq!(
                observe(&mut history, 4, 300, &[candidate(1, 0.5)]).windows[0]
                    .unwrap()
                    .samples,
                1
            );
        }
    }

    #[test]
    fn processing_clock_reversal_future_and_stale_captures_fail_closed() {
        for (bad, processed) in [
            (capture(2, 101), 100),
            (capture(2, 101), 1_000_102),
            (capture(2, 101), 150),
        ] {
            let mut history = History::new(7, limits()).unwrap();
            history
                .observe(
                    capture(1, 100),
                    200,
                    CameraInterval::KnownStill,
                    &[candidate(1, 0.25)],
                )
                .unwrap();
            assert_eq!(
                history.observe(
                    bad,
                    processed,
                    CameraInterval::KnownStill,
                    &[candidate(1, 0.5)]
                ),
                Err(Rejection::Time)
            );
            assert_eq!(
                observe(&mut history, 3, 1_000_200, &[candidate(1, 0.5)]).windows[0]
                    .unwrap()
                    .samples,
                1
            );
        }
    }

    #[test]
    fn maximum_age_is_inclusive() {
        let mut history = History::new(7, limits()).unwrap();
        assert!(
            history
                .observe(capture(1, 0), 1_000_000, CameraInterval::KnownStill, &[])
                .is_ok()
        );
    }

    #[test]
    fn privacy_close_requires_new_epoch_and_never_reuses_old_motion() {
        let mut history = History::new(7, limits()).unwrap();
        observe(&mut history, 1, 0, &[candidate(1, 0.25)]);
        history.close();
        assert_eq!(
            history.observe(capture(2, 1), 1, CameraInterval::KnownStill, &[]),
            Err(Rejection::Closed)
        );
        assert_eq!(history.begin_epoch(7), Err(Rejection::Epoch));
        history.begin_epoch(8).unwrap();
        assert_eq!(
            history.observe(capture(2, 2), 2, CameraInterval::KnownStill, &[]),
            Err(Rejection::Epoch)
        );
        let next = Capture {
            epoch: 8,
            sequence: 1,
            captured_us: 3,
        };
        let result = history
            .observe(next, 3, CameraInterval::KnownStill, &[candidate(1, 0.75)])
            .unwrap();
        assert_eq!(result.windows[0].unwrap().displacement, None);
    }

    #[test]
    fn epoch_overflow_cannot_reopen_with_an_old_generation() {
        let mut history = History::new(u64::MAX, limits()).unwrap();
        assert_eq!(history.begin_epoch(0), Err(Rejection::Epoch));
        assert_eq!(
            history.observe(
                Capture {
                    epoch: u64::MAX,
                    sequence: 1,
                    captured_us: 1
                },
                1,
                CameraInterval::KnownStill,
                &[]
            ),
            Err(Rejection::Closed)
        );
    }

    #[test]
    fn duplicate_ids_reject_whole_batch_and_consume_the_capture() {
        let mut history = History::new(7, limits()).unwrap();
        observe(&mut history, 1, 0, &[candidate(1, 0.25)]);
        assert_eq!(
            history.observe(
                capture(2, 1),
                1,
                CameraInterval::KnownStill,
                &[candidate(1, 0.25), candidate(1, 0.5)]
            ),
            Err(Rejection::DuplicateTrack)
        );
        assert_eq!(
            history.observe(
                capture(2, 1),
                1,
                CameraInterval::KnownStill,
                &[candidate(1, 0.5)]
            ),
            Err(Rejection::Time)
        );
        assert_eq!(
            observe(&mut history, 3, 2, &[candidate(1, 0.5)]).windows[0]
                .unwrap()
                .samples,
            1
        );
    }

    #[test]
    fn invalid_geometry_including_unused_input_rejects_whole_batch() {
        for x in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
            let mut history = History::new(7, limits()).unwrap();
            let mut bad = candidate(2, x);
            bad.usable = false;
            assert_eq!(
                history.observe(
                    capture(1, 0),
                    0,
                    CameraInterval::KnownStill,
                    &[candidate(1, 0.25), bad]
                ),
                Err(Rejection::Geometry)
            );
            assert!(history.tracks.iter().all(Option::is_none));
        }
    }

    #[test]
    fn too_many_candidates_are_not_truncated_or_partially_recorded() {
        let mut history = History::new(7, limits()).unwrap();
        let values: Vec<_> = (0..=MAX_TRACKS)
            .map(|id| candidate(id as u32, 0.5))
            .collect();
        assert_eq!(
            history.observe(capture(1, 0), 0, CameraInterval::KnownStill, &values),
            Err(Rejection::Capacity)
        );
        assert!(history.tracks.iter().all(Option::is_none));
    }

    #[test]
    fn input_reordering_keeps_ids_separate_at_full_capacity() {
        let mut history = History::new(7, limits()).unwrap();
        let mut values: Vec<_> = (0..MAX_TRACKS)
            .map(|id| candidate(id as u32, id as f32 / 8.0))
            .collect();
        observe(&mut history, 1, 0, &values);
        values.reverse();
        let next = observe(&mut history, 2, 1, &values);
        for (window, value) in next.windows.into_iter().flatten().zip(values) {
            assert_eq!(window.track_id, value.track_id);
            assert_eq!(window.displacement, Some([0.0, 0.0]));
        }
    }

    #[test]
    fn missing_unusable_or_refound_id_does_not_inherit_motion() {
        for mode in 0..3 {
            let mut history = History::new(7, limits()).unwrap();
            observe(&mut history, 1, 0, &[candidate(1, 0.25)]);
            match mode {
                0 => {
                    observe(&mut history, 2, 1, &[]);
                }
                1 => {
                    observe(
                        &mut history,
                        2,
                        1,
                        &[Candidate {
                            usable: false,
                            ..candidate(1, 0.25)
                        }],
                    );
                }
                _ => {
                    observe(&mut history, 2, 1, &[candidate(2, 0.25)]);
                }
            }
            let next = observe(&mut history, 3, 2, &[candidate(1, 0.75)]);
            assert_eq!(next.windows[0].unwrap().displacement, None);
        }
    }

    #[test]
    fn excessive_gap_forgets_motion_even_with_the_same_id() {
        let mut history = History::new(7, limits()).unwrap();
        observe(&mut history, 1, 0, &[candidate(1, 0.25)]);
        let next = observe(&mut history, 2, 5_000_001, &[candidate(1, 0.75)]);
        assert_eq!(next.windows[0].unwrap().displacement, None);
    }

    #[test]
    fn unknown_or_moving_camera_interval_starts_a_new_origin() {
        for interval in [CameraInterval::Moved, CameraInterval::Unknown] {
            let mut history = History::new(7, limits()).unwrap();
            observe(&mut history, 1, 0, &[candidate(1, 0.25)]);
            let next = history
                .observe(capture(2, 1), 1, interval, &[candidate(1, 0.75)])
                .unwrap();
            assert_eq!(next.windows[0].unwrap().displacement, None);
            assert_eq!(
                observe(&mut history, 3, 2, &[candidate(1, 0.75)]).windows[0]
                    .unwrap()
                    .displacement,
                Some([0.0, 0.0])
            );
        }
    }

    #[test]
    fn zero_duration_limits_are_invalid_and_no_default_exists() {
        for invalid in [
            Limits {
                window_us: 0,
                ..limits()
            },
            Limits {
                maximum_gap_us: 0,
                ..limits()
            },
            Limits {
                maximum_frame_age_us: 0,
                ..limits()
            },
        ] {
            assert!(matches!(
                History::new(7, invalid),
                Err(Rejection::InvalidLimits)
            ));
        }
    }
}
