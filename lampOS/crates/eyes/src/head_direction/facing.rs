//! Fixed, observation-only facing proxies; no confidence or addressed-person claim.
//! The legacy edge allowance and pitch constants are frozen before evaluation.
//! Missing or invalid geometry is unknown, never evidence of looking away.

use crate::facing::{MAX_PITCH_DOWN_DEG, MAX_PITCH_UP_DEG, allowed_deg};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Class {
    Facing,
    NotFacing,
    Unknown,
}

/// Apply the existing yaw cone to either geometry method, with no pitch gate.
/// The caller separately establishes face visibility, size and association.
pub fn yaw_only(yaw_deg: Option<f32>, from_middle: f32) -> Class {
    let Some(yaw) = yaw_deg.filter(|yaw| yaw.is_finite() && (-90.0..=90.0).contains(yaw)) else {
        return Class::Unknown;
    };
    if !from_middle.is_finite() || from_middle < 0.0 {
        return Class::Unknown;
    }
    if yaw.abs() <= allowed_deg(from_middle) {
        Class::Facing
    } else {
        Class::NotFacing
    }
}

/// The same yaw allowance plus inclusive pitch [-20,+25] degrees, down positive.
/// A missing pitch remains unknown even when the yaw alone would be outside.
pub fn yaw_and_pitch(yaw_deg: Option<f32>, pitch_deg: Option<f32>, from_middle: f32) -> Class {
    let Some(pitch) = pitch_deg.filter(|pitch| pitch.is_finite() && (-90.0..=90.0).contains(pitch))
    else {
        return Class::Unknown;
    };
    match yaw_only(yaw_deg, from_middle) {
        Class::Unknown => Class::Unknown,
        Class::Facing if (-MAX_PITCH_UP_DEG..=MAX_PITCH_DOWN_DEG).contains(&pitch) => Class::Facing,
        Class::Facing | Class::NotFacing => Class::NotFacing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_yaw_boundary_and_existing_edge_allowance_apply_to_both_signs() {
        assert_eq!(crate::facing::MAX_YAW_DEG, 60.0);
        assert_eq!(crate::facing::EDGE_SCALE, 1.8);
        for sign in [-1.0, 1.0] {
            assert_eq!(yaw_only(Some(sign * 60.0), 0.0), Class::Facing);
            assert_eq!(yaw_only(Some(sign * 60.001), 0.0), Class::NotFacing);
            assert_eq!(yaw_only(Some(sign * 84.0), 0.5), Class::Facing);
            assert_eq!(yaw_only(Some(sign * 84.001), 0.5), Class::NotFacing);
            assert_eq!(yaw_only(Some(sign * 90.0), 1.0), Class::Facing);
            assert_eq!(yaw_only(Some(sign * 90.0), 7.0), Class::Facing);
        }
    }

    #[test]
    fn pitch_is_inclusive_and_changes_only_the_pitch_candidate() {
        assert_eq!(MAX_PITCH_UP_DEG, 20.0);
        assert_eq!(MAX_PITCH_DOWN_DEG, 25.0);
        for pitch in [-20.0, 0.0, 25.0] {
            assert_eq!(yaw_and_pitch(Some(60.0), Some(pitch), 0.0), Class::Facing);
        }
        for pitch in [-20.001, 25.001] {
            assert_eq!(
                yaw_and_pitch(Some(60.0), Some(pitch), 0.0),
                Class::NotFacing
            );
            assert_eq!(yaw_only(Some(60.0), 0.0), Class::Facing);
        }
        assert_eq!(yaw_and_pitch(Some(61.0), Some(0.0), 0.0), Class::NotFacing);
        assert_eq!(yaw_and_pitch(Some(80.0), Some(0.0), 1.0), Class::Facing);
    }

    #[test]
    fn missing_nonfinite_and_out_of_range_inputs_are_unknown() {
        for angle in [
            None,
            Some(f32::NAN),
            Some(f32::INFINITY),
            Some(-90.001),
            Some(90.001),
        ] {
            assert_eq!(yaw_only(angle, 0.0), Class::Unknown);
            assert_eq!(yaw_and_pitch(angle, Some(0.0), 0.0), Class::Unknown);
            assert_eq!(yaw_and_pitch(Some(89.0), angle, 0.0), Class::Unknown);
        }
        for edge in [f32::NAN, f32::INFINITY, -0.001] {
            assert_eq!(yaw_only(Some(0.0), edge), Class::Unknown);
            assert_eq!(yaw_and_pitch(Some(0.0), Some(0.0), edge), Class::Unknown);
        }
    }

    #[test]
    fn the_shared_yaw_comparison_matches_the_existing_measurable_rule() {
        for yaw in [0.0, 59.0, 60.0, 61.0, 84.0, 90.0] {
            for edge in [0.0, 0.2, 0.5, 1.0] {
                let old = crate::facing::facing(Some(yaw), crate::facing::LEAST_WIDTH, edge);
                assert_eq!(yaw_only(Some(yaw), edge) == Class::Facing, old);
            }
        }
    }

    #[test]
    #[ignore = "explicit host-only measurement, no device or hard deadline claim"]
    fn measure_incremental_classification_elapsed() {
        use std::{hint::black_box, time::Instant};
        let marks = [
            (100.0, 100.0),
            (180.0, 100.0),
            (140.0, 130.0),
            (115.0, 160.0),
            (165.0, 160.0),
        ];
        let one = || {
            let at = Instant::now();
            let legacy = crate::facing::yaw_deg(black_box(&marks));
            black_box((
                yaw_only(legacy, black_box(0.25)),
                yaw_only(Some(black_box(60.5)), black_box(0.25)),
                yaw_and_pitch(
                    Some(black_box(60.5)),
                    Some(black_box(24.0)),
                    black_box(0.25),
                ),
            ));
            at.elapsed().as_nanos()
        };
        let first = one();
        let mut times = (0..1000).map(|_| one()).collect::<Vec<_>>();
        times.sort_unstable();
        eprintln!(
            "classification_host_ns first={first} median={} p95={} max={} n=1000 boundary=legacy_yaw_plus_three_classifiers target_ns=1000000",
            times[500], times[950], times[999]
        );
    }
}
