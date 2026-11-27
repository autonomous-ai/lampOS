//! Default-off observation of geometry already computed by the eyes.
//!
//! This module never opens a camera, runs inference, changes look cadence,
//! publishes pose, or decides which person was addressed. It borrows an
//! existing look's landmarks, discards the angles, and retains bounded counts.
//! Availability is not accuracy. The denominator is existing processing
//! looks, not head turns, utterances, or independently labelled opportunities.

pub mod freshness;
pub mod geometry;

use crate::{faces::Face, facing, picture::Picture};
use freshness::{Guard, Reason, Ticket};
use lamp_camera::{CaptureId, PortFrame, SourceId, TimestampPoint};
use serde::{Serialize, Serializer, ser::SerializeMap};

pub const REPORT_EVERY_US: u64 = 60_000_000;
pub const MAX_AGE_US: u64 = lamp_camera::MAX_DEQUEUE_AGE_US;
/// Exclusive upper edges. Only fresh available geometry enters these bins;
/// late and rejected looks remain in outcomes, not in a truncated p95 claim.
const AGE_BOUNDS_US: [u64; 6] = [25_000, 50_000, 100_000, 150_000, 200_000, MAX_AGE_US];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Crop {
    Working,
    Full,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Available,
    NoFace,
    MultipleFaces,
    FilteredFace,
    DetectionSkipped,
    DecodeFailed,
    DetectorFailed,
    TooSmall,
    OutsideFrame,
    ClippedFace,
    MeshNotConfigured,
    MeshDeferred,
    MeshUnavailable,
    InvalidGeometry,
    Rejected,
    Cancelled,
}

const OUTCOMES: [(Outcome, &str); 16] = [
    (Outcome::Available, "available"),
    (Outcome::NoFace, "no_face"),
    (Outcome::MultipleFaces, "multiple_faces"),
    (Outcome::FilteredFace, "filtered_face"),
    (Outcome::DetectionSkipped, "detection_skipped"),
    (Outcome::DecodeFailed, "decode_failed"),
    (Outcome::DetectorFailed, "detector_failed"),
    (Outcome::TooSmall, "too_small"),
    (Outcome::OutsideFrame, "outside_frame"),
    (Outcome::ClippedFace, "clipped_face"),
    (Outcome::MeshNotConfigured, "mesh_not_configured"),
    (Outcome::MeshDeferred, "mesh_deferred"),
    (Outcome::MeshUnavailable, "mesh_unavailable"),
    (Outcome::InvalidGeometry, "invalid_geometry"),
    (Outcome::Rejected, "rejected"),
    (Outcome::Cancelled, "cancelled"),
];
const REASONS: [Reason; 20] = [
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

/// Borrowed outputs from the existing pipeline. There is no new model read.
pub struct Observation<'a> {
    pub detected: bool,
    /// Count before the existing still-face filter. Two detections must not
    /// become an unambiguous subject merely because one was filtered out.
    pub raw_faces: usize,
    pub faces: &'a [Face],
    pub picture: &'a Picture,
    pub mesh_configured: bool,
    pub mesh_wanted: bool,
    pub meshes: &'a [(usize, Vec<f32>)],
}

impl Observation<'_> {
    pub fn outcome(&self) -> Outcome {
        if !self.detected {
            return Outcome::DetectionSkipped;
        }
        match self.raw_faces {
            0 => return Outcome::NoFace,
            1 => {}
            _ => return Outcome::MultipleFaces,
        }
        let [face] = self.faces else {
            return Outcome::FilteredFace;
        };
        let (width, height) = self.picture.filled;
        if face.width / (width.max(1) as f32) < facing::LEAST_WIDTH {
            return Outcome::TooSmall;
        }
        if !facing::marks_seen(&face.marks, width as f32, height as f32) {
            return Outcome::OutsideFrame;
        }
        if face.x < 0.0
            || face.y < 0.0
            || face.x + face.width > width as f32
            || face.y + face.height > height as f32
        {
            return Outcome::ClippedFace;
        }
        if !self.mesh_configured {
            return Outcome::MeshNotConfigured;
        }
        if !self.mesh_wanted {
            return Outcome::MeshDeferred;
        }
        let Some((_, landmarks)) = self.meshes.iter().find(|(index, _)| *index == 0) else {
            return Outcome::MeshUnavailable;
        };
        if geometry::mesh_head(landmarks).is_some() {
            Outcome::Available
        } else {
            Outcome::InvalidGeometry
        }
    }
}

#[derive(Debug, Default)]
struct Outcomes([u64; OUTCOMES.len()]);

impl Serialize for Outcomes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(OUTCOMES.len()))?;
        for ((_, name), value) in OUTCOMES.iter().zip(self.0) {
            map.serialize_entry(name, &value)?;
        }
        map.end()
    }
}

#[derive(Debug, Default)]
struct Rejections([u64; REASONS.len()]);

impl Serialize for Rejections {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(REASONS.len()))?;
        for (reason, value) in REASONS.iter().zip(self.0) {
            map.serialize_entry(reason.code(), &value)?;
        }
        map.end()
    }
}

#[derive(Debug, Default, Serialize)]
struct Ages {
    start_of_exposure: [u64; AGE_BOUNDS_US.len()],
    end_of_frame: [u64; AGE_BOUNDS_US.len()],
}

#[derive(Debug, Default, Serialize)]
struct Counts {
    frames: u64,
    looks: u64,
    outcomes: Outcomes,
    frame_rejections: Rejections,
    look_rejections: Rejections,
    working_crop_source_age_us: Ages,
    full_crop_source_age_us: Ages,
    saturated: bool,
}

/// Numbers and fixed vocabulary only. No frame clocks/IDs, passing face IDs,
/// angle, landmark, image, path, words, or raw error can be serialized here.
#[derive(Debug, Serialize)]
pub struct Report {
    schema: &'static str,
    method: &'static str,
    window_us: u64,
    age_bin_exclusive_upper_us: [u64; AGE_BOUNDS_US.len()],
    #[serde(flatten)]
    counts: Counts,
}

fn increment(value: &mut u64, saturated: &mut bool) {
    match value.checked_add(1) {
        Some(next) => *value = next,
        None => *saturated = true,
    }
}

/// One capture receipt and one in-progress look; all counters are fixed size.
pub struct Observer {
    guard: Guard,
    latest: Option<Result<Ticket, Reason>>,
    pending: Option<Result<Ticket, Reason>>,
    counts: Counts,
    report_at_us: u64,
}

impl Observer {
    pub fn new(now_us: u64) -> Self {
        Self {
            guard: Guard::new(MAX_AGE_US).expect("constant camera age ceiling"),
            latest: None,
            pending: None,
            counts: Counts::default(),
            report_at_us: now_us,
        }
    }

    pub fn begin(
        &mut self,
        capture: CaptureId,
        source: SourceId,
        started_us: u64,
        ended_us: u64,
        permission_before: Option<u64>,
        permission_after: Option<u64>,
    ) {
        self.clear();
        self.latest = self
            .guard
            .begin(
                capture,
                source,
                started_us,
                ended_us,
                permission_before,
                permission_after,
            )
            .err()
            .map(Err);
    }

    pub fn clear(&mut self) {
        if self.pending.take().is_some() {
            self.count_outcome(Outcome::Cancelled);
        }
        self.latest = None;
        self.guard.clear();
    }

    pub fn frame(
        &mut self,
        frame: &PortFrame,
        started_us: u64,
        ended_us: u64,
        permission_before: Option<u64>,
        permission_after: Option<u64>,
    ) {
        increment(&mut self.counts.frames, &mut self.counts.saturated);
        let observed = self
            .guard
            .observe(frame, started_us, ended_us, permission_before)
            .and_then(|ticket| {
                self.guard
                    .accept(ticket, ended_us, permission_after)
                    .map(|_| ticket)
            });
        if let Err(reason) = observed {
            increment(
                &mut self.counts.frame_rejections.0[reason as usize],
                &mut self.counts.saturated,
            );
        }
        self.latest = Some(observed);
    }

    pub fn look(&mut self) {
        if self.pending.take().is_some() {
            self.count_outcome(Outcome::Cancelled);
        }
        increment(&mut self.counts.looks, &mut self.counts.saturated);
        self.pending = Some(self.latest.take().unwrap_or(Err(Reason::NoCapture)));
    }

    /// Final permission and source-age check after the borrowed geometry.
    /// It authorizes no publication; only the aggregate counters change.
    /// Calling again after a completed look is a no-op, so the caller can
    /// finish decode/detector failures without double-counting completed work.
    pub fn finish(&mut self, outcome: Outcome, crop: Crop, now_us: u64, permission: Option<u64>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let timing = pending.and_then(|ticket| self.guard.accept(ticket, now_us, permission));
        match timing {
            Err(reason) => {
                self.count_outcome(Outcome::Rejected);
                increment(
                    &mut self.counts.look_rejections.0[reason as usize],
                    &mut self.counts.saturated,
                );
            }
            Ok(timing) => {
                self.count_outcome(outcome);
                if outcome == Outcome::Available {
                    let ages = match crop {
                        Crop::Working => &mut self.counts.working_crop_source_age_us,
                        Crop::Full => &mut self.counts.full_crop_source_age_us,
                    };
                    let bins = match timing.driver_timestamp.point {
                        TimestampPoint::StartOfExposure => &mut ages.start_of_exposure,
                        TimestampPoint::EndOfFrame => &mut ages.end_of_frame,
                        TimestampPoint::Unknown => {
                            unreachable!("guard refuses an unknown source point")
                        }
                    };
                    let index = AGE_BOUNDS_US
                        .iter()
                        .position(|upper| timing.source_age_us < *upper)
                        .expect("accepted age is below the exclusive ceiling");
                    increment(&mut bins[index], &mut self.counts.saturated);
                }
            }
        }
    }

    pub fn report(&mut self, now_us: u64) -> Option<Report> {
        let window_us = now_us.checked_sub(self.report_at_us)?;
        if window_us < REPORT_EVERY_US || self.pending.is_some() {
            return None;
        }
        self.report_at_us = now_us;
        Some(Report {
            schema: "lamp-head-direction-observer/1",
            method: "mesh_head_3a9ae0692_checked",
            window_us,
            age_bin_exclusive_upper_us: AGE_BOUNDS_US,
            counts: std::mem::take(&mut self.counts),
        })
    }

    fn count_outcome(&mut self, outcome: Outcome) {
        increment(
            &mut self.counts.outcomes.0[outcome as usize],
            &mut self.counts.saturated,
        );
    }
}

#[cfg(test)]
mod tests;
