use super::*;
use lamp_camera::{DriverTimestamp, TimestampDomain};
use lamp_turn_owner::BootId;
use std::num::NonZeroU64;

const PERMISSION: Option<u64> = Some(7);

fn capture() -> CaptureId {
    CaptureId {
        worker: BootId::new([19; 16]).unwrap(),
        epoch: NonZeroU64::new(23).unwrap(),
    }
}

fn source() -> SourceId {
    SourceId::new("private_source_canary").unwrap()
}

fn frame(sequence: u32, at: u64, point: TimestampPoint) -> PortFrame {
    PortFrame {
        capture: capture(),
        source: source(),
        bytes_used: 4567,
        driver_sequence: sequence,
        timestamp: Some(DriverTimestamp {
            micros: at,
            domain: TimestampDomain::HostMonotonic,
            point,
        }),
    }
}

fn observer() -> Observer {
    let mut observer = Observer::new(1);
    observer.begin(capture(), source(), 100, 110, PERMISSION, PERMISSION);
    observer
}

fn ready(observer: &mut Observer, sequence: u32, at: u64, point: TimestampPoint) {
    observer.frame(
        &frame(sequence, at, point),
        at + 1,
        at + 2,
        PERMISSION,
        PERMISSION,
    );
    observer.look();
}

fn face() -> Face {
    Face {
        score: 0.9,
        x: 80.0,
        y: 40.0,
        width: 120.0,
        height: 160.0,
        marks: [
            (100.0, 100.0),
            (180.0, 100.0),
            (140.0, 130.0),
            (115.0, 160.0),
            (165.0, 160.0),
        ],
    }
}

fn mesh() -> Vec<f32> {
    let mut points = vec![0.0; crate::mesh::LANDMARKS * 3];
    // Synthetic planar face; geometry unit tests, not this fixture, test angles.
    for (index, x, y) in [
        (33, 60., 100.),
        (263, 180., 100.),
        (133, 90., 100.),
        (362, 150., 100.),
        (129, 90., 140.),
        (358, 150., 140.),
        (2, 120., 160.),
        (9, 120., 50.),
        (94, 120., 160.),
        (151, 120., 40.),
    ] {
        points[index * 3] = x;
        points[index * 3 + 1] = y;
    }
    points
}

#[test]
fn existing_look_denominator_includes_missing_and_ambiguous_inputs() {
    let picture = Picture {
        planes: Vec::new(),
        filled: (320, 240),
    };
    let faces = [face()];
    let meshes = [(0, mesh())];
    let mut look = Observation {
        detected: true,
        raw_faces: 1,
        faces: &faces,
        picture: &picture,
        mesh_configured: true,
        mesh_wanted: true,
        meshes: &meshes,
    };
    assert_eq!(look.outcome(), Outcome::Available);
    look.detected = false;
    assert_eq!(look.outcome(), Outcome::DetectionSkipped);
    look.detected = true;
    look.raw_faces = 0;
    look.faces = &[];
    assert_eq!(look.outcome(), Outcome::NoFace);
    look.raw_faces = 2;
    look.faces = &faces; // The still-face filter removing one must not pick a subject.
    assert_eq!(look.outcome(), Outcome::MultipleFaces);
    look.raw_faces = 1;
    look.faces = &[];
    assert_eq!(look.outcome(), Outcome::FilteredFace);
    look.faces = &faces;
    look.mesh_configured = false;
    assert_eq!(look.outcome(), Outcome::MeshNotConfigured);
    look.mesh_configured = true;
    look.mesh_wanted = false;
    assert_eq!(look.outcome(), Outcome::MeshDeferred);
    look.mesh_wanted = true;
    look.meshes = &[];
    assert_eq!(look.outcome(), Outcome::MeshUnavailable);
    let wrong = [(1, mesh())];
    look.meshes = &wrong;
    assert_eq!(look.outcome(), Outcome::MeshUnavailable);
    let invalid = [(0, vec![0.0; crate::mesh::LANDMARKS * 3])];
    look.meshes = &invalid;
    assert_eq!(look.outcome(), Outcome::InvalidGeometry);
}

#[test]
fn visibility_keeps_small_outside_and_clipped_faces_unknown() {
    let picture = Picture {
        planes: Vec::new(),
        filled: (320, 240),
    };
    let meshes = [(0, mesh())];
    let classify = |face| {
        Observation {
            detected: true,
            raw_faces: 1,
            faces: &[face],
            picture: &picture,
            mesh_configured: true,
            mesh_wanted: true,
            meshes: &meshes,
        }
        .outcome()
    };
    assert_eq!(
        classify(Face {
            width: 1.0,
            ..face()
        }),
        Outcome::TooSmall
    );
    assert_eq!(
        classify(Face {
            marks: [(-1., 100.); 5],
            ..face()
        }),
        Outcome::OutsideFrame
    );
    assert_eq!(classify(Face { x: -1., ..face() }), Outcome::ClippedFace);
    assert_eq!(classify(Face { y: 120., ..face() }), Outcome::ClippedFace);
}

#[test]
fn late_results_stay_in_the_denominator_and_never_in_available_timing() {
    let mut observer = observer();
    ready(&mut observer, 1, 1_000, TimestampPoint::StartOfExposure);
    observer.finish(Outcome::Available, Crop::Working, 101_000, PERMISSION);
    ready(&mut observer, 2, 201_000, TimestampPoint::StartOfExposure);
    observer.finish(Outcome::Available, Crop::Full, 201_100, PERMISSION);
    ready(&mut observer, 3, 300_000, TimestampPoint::StartOfExposure);
    observer.finish(Outcome::Available, Crop::Working, 550_000, PERMISSION);
    let report = observer.report(REPORT_EVERY_US + 1).unwrap();
    assert_eq!(report.counts.looks, 3);
    assert_eq!(report.counts.outcomes.0[Outcome::Available as usize], 2);
    assert_eq!(report.counts.outcomes.0[Outcome::Rejected as usize], 1);
    assert_eq!(report.counts.look_rejections.0[Reason::Stale as usize], 1);
    assert_eq!(
        report.counts.working_crop_source_age_us.start_of_exposure,
        [0, 0, 0, 1, 0, 0]
    );
    assert_eq!(
        report.counts.full_crop_source_age_us.start_of_exposure,
        [1, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        report.counts.working_crop_source_age_us.end_of_frame,
        [0; 6]
    );
}

#[test]
fn end_of_frame_is_not_reported_as_exposure_and_point_changes_are_rejected() {
    let mut observer = observer();
    ready(&mut observer, 1, 1_000, TimestampPoint::EndOfFrame);
    observer.finish(Outcome::Available, Crop::Working, 1_100, PERMISSION);
    ready(&mut observer, 2, 2_000, TimestampPoint::StartOfExposure);
    observer.finish(Outcome::Available, Crop::Working, 2_100, PERMISSION);
    let report = observer.report(REPORT_EVERY_US + 1).unwrap();
    assert_eq!(report.counts.outcomes.0[Outcome::Available as usize], 1);
    assert_eq!(
        report.counts.frame_rejections.0[Reason::TimestampPointChanged as usize],
        1
    );
    assert_eq!(
        report.counts.look_rejections.0[Reason::TimestampPointChanged as usize],
        1
    );
    assert_eq!(
        report.counts.working_crop_source_age_us.end_of_frame,
        [1, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        report.counts.working_crop_source_age_us.start_of_exposure,
        [0; 6]
    );
}

#[test]
fn revoke_during_read_or_inference_cannot_count_as_available() {
    for (before, after, finish) in [
        (None, PERMISSION, PERMISSION),
        (PERMISSION, Some(8), Some(8)),
        (PERMISSION, PERMISSION, None),
        (PERMISSION, PERMISSION, Some(8)),
    ] {
        let mut observer = observer();
        observer.frame(
            &frame(1, 1_000, TimestampPoint::StartOfExposure),
            1_001,
            1_002,
            before,
            after,
        );
        observer.look();
        observer.finish(Outcome::Available, Crop::Working, 1_100, finish);
        let report = observer.report(REPORT_EVERY_US + 1).unwrap();
        assert_eq!(report.counts.looks, 1);
        assert_eq!(report.counts.outcomes.0[Outcome::Available as usize], 0);
        assert_eq!(report.counts.outcomes.0[Outcome::Rejected as usize], 1);
    }
}

#[test]
fn missing_superseded_and_cancelled_looks_cannot_disappear_or_be_double_counted() {
    let mut observer = observer();
    observer.look();
    observer.finish(Outcome::DecodeFailed, Crop::Working, 200, PERMISSION);
    ready(&mut observer, 1, 1_000, TimestampPoint::StartOfExposure);
    observer.frame(
        &frame(2, 1_100, TimestampPoint::StartOfExposure),
        1_101,
        1_102,
        PERMISSION,
        PERMISSION,
    );
    observer.finish(Outcome::Available, Crop::Working, 1_200, PERMISSION);
    observer.look();
    observer.clear();
    observer.finish(Outcome::Available, Crop::Working, 1_300, PERMISSION);
    let report = observer.report(REPORT_EVERY_US + 1).unwrap();
    assert_eq!(report.counts.looks, 3);
    assert_eq!(report.counts.outcomes.0.iter().sum::<u64>(), 3);
    assert_eq!(report.counts.outcomes.0[Outcome::Cancelled as usize], 1);
    assert_eq!(
        report.counts.look_rejections.0[Reason::NoCapture as usize],
        1
    );
    assert_eq!(
        report.counts.look_rejections.0[Reason::Superseded as usize],
        1
    );
}

#[test]
fn processing_failures_and_unavailable_geometry_are_not_successes() {
    let mut observer = observer();
    for (index, outcome) in [
        Outcome::DecodeFailed,
        Outcome::DetectorFailed,
        Outcome::NoFace,
        Outcome::DetectionSkipped,
        Outcome::MeshDeferred,
        Outcome::InvalidGeometry,
    ]
    .into_iter()
    .enumerate()
    {
        let at = 1_000 + index as u64 * 1_000;
        ready(
            &mut observer,
            index as u32 + 1,
            at,
            TimestampPoint::StartOfExposure,
        );
        observer.finish(outcome, Crop::Working, at + 100, PERMISSION);
        observer.finish(Outcome::Available, Crop::Working, at + 150, PERMISSION);
    }
    let report = observer.report(REPORT_EVERY_US + 1).unwrap();
    assert_eq!(report.counts.looks, 6);
    assert_eq!(report.counts.outcomes.0.iter().sum::<u64>(), 6);
    assert_eq!(report.counts.outcomes.0[Outcome::Available as usize], 0);
    assert_eq!(
        report.counts.working_crop_source_age_us.start_of_exposure,
        [0; 6]
    );
}

#[test]
fn reports_are_bounded_reset_once_and_never_export_private_metadata() {
    let mut observer = observer();
    assert!(observer.report(REPORT_EVERY_US).is_none());
    ready(
        &mut observer,
        987_654_321,
        7_654_321,
        TimestampPoint::StartOfExposure,
    );
    assert!(observer.report(REPORT_EVERY_US + 1).is_none()); // Don't split an in-progress look.
    observer.finish(Outcome::Available, Crop::Full, 7_654_400, PERMISSION);
    let json = serde_json::to_value(observer.report(REPORT_EVERY_US + 1).unwrap()).unwrap();
    let text = serde_json::to_string(&json).unwrap();
    assert!(text.len() < 4096);
    for private in [
        "private_source_canary",
        "987654321",
        "7654321",
        "7654400",
        "landmarks",
        "yaw_deg",
        "pitch_deg",
        "worker",
    ] {
        assert!(!text.contains(private), "{private}");
    }
    fn only_closed_strings(value: &serde_json::Value) {
        match value {
            serde_json::Value::String(text) => assert!(
                [
                    "lamp-head-direction-observer/1",
                    "mesh_head_3a9ae0692_checked"
                ]
                .contains(&text.as_str())
            ),
            serde_json::Value::Array(values) => values.iter().for_each(only_closed_strings),
            serde_json::Value::Object(values) => values.values().for_each(only_closed_strings),
            serde_json::Value::Number(value) => assert!(value.as_u64().is_some()),
            serde_json::Value::Bool(_) => {}
            serde_json::Value::Null => panic!("unexpected absent aggregate"),
        }
    }
    only_closed_strings(&json);
    assert!(observer.report(REPORT_EVERY_US + 2).is_none());
    let empty = observer.report(2 * REPORT_EVERY_US + 1).unwrap();
    assert_eq!(empty.counts.looks, 0);
    assert_eq!(empty.counts.frames, 0);
    assert_eq!(empty.counts.outcomes.0.iter().sum::<u64>(), 0);
}

#[test]
fn counter_overflow_is_visible_and_closed_vocabularies_match_storage_indices() {
    for (index, (outcome, _)) in OUTCOMES.iter().enumerate() {
        assert_eq!(*outcome as usize, index);
    }
    for (index, reason) in REASONS.iter().enumerate() {
        assert_eq!(*reason as usize, index);
    }
    let mut observer = observer();
    observer.counts.looks = u64::MAX;
    observer.look();
    observer.finish(Outcome::Available, Crop::Working, 200, PERMISSION);
    let report = observer.report(REPORT_EVERY_US + 1).unwrap();
    assert!(report.counts.saturated);
    assert_eq!(report.counts.looks, u64::MAX);
}
