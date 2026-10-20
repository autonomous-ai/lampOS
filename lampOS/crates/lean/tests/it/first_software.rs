//! The first lamp software's body description, in this check's form
//! (`deploy/lamp-4ace/lean-first-software.json`), must give the numbers the
//! first software's own function gave for the same motor readings. The
//! expected distances below were computed by `cog_offset_mm` in
//! `hal/drivers/motors/recording_stability.py` on 2026-10-14, given true
//! angles; the first two poses are what lamp-4ace's motors read at rest and
//! standing that morning. Agreement shows the conversion, not the lamp: the
//! masses are estimates (docs/lean.md, "lamp-4ace, 14 October").

use lamp_lean::{Angles, Model};

const FILE: &str = include_str!("../../../../deploy/lamp-4ace/lean-first-software.json");

/// Motor readings for servos 1..5 and the first software's distance, in mm,
/// of the centre of mass from the base's axis.
const POSES: [([u16; 5], f64); 4] = [
    ([2023, 2052, 2047, 2053, 1950], 17.851),
    ([2023, 2558, 2503, 2053, 1820], 1.553),
    ([2209, 1265, 1933, 2499, 2030], 26.363),
    ([1731, 1458, 2096, 1902, 1516], 36.670),
];

fn off_axis_mm(model: &Model, counts: [u16; 5]) -> (f64, f64) {
    let check = model.check_pose(Angles::from_encoder_counts(counts).unwrap());
    let [x, y, _] = check.centre_of_mass_m;
    (x.hypot(y) * 1000.0, check.clearance_m * 1000.0)
}

#[test]
fn the_file_is_not_a_measurement_and_qualifies_nothing() {
    let model = Model::from_json(FILE).unwrap();
    assert!(!model.has_measurements());
    assert_eq!(model.margin_m(), 0.0);
    assert!((model.total_mass_kg() - 1.49).abs() < 1e-9);
}

#[test]
fn the_centre_of_mass_agrees_with_the_first_software_on_the_same_readings() {
    let model = Model::from_json(FILE).unwrap();
    for (counts, first_mm) in POSES {
        let (mm, _) = off_axis_mm(&model, counts);
        // The first software divides a turn into 4095 steps and this check
        // into 4096; over these poses that is under 0.03 mm.
        assert!(
            (mm - first_mm).abs() < 0.03,
            "{counts:?}: {mm} against {first_mm}"
        );
    }
}

#[test]
fn the_footprint_is_the_circle_inside_the_200_mm_square_of_the_lamps_specification() {
    let model = Model::from_json(FILE).unwrap();
    for (counts, _) in POSES {
        let (mm, clearance_mm) = off_axis_mm(&model, counts);
        // A 32-sided shape inside a 100 mm circle is at most 0.482 mm
        // stricter than the circle, and never looser.
        let against_circle = 100.0 - mm;
        assert!(clearance_mm <= against_circle + 1e-6, "{counts:?}");
        assert!(clearance_mm >= against_circle - 0.482, "{counts:?}");
        assert!(clearance_mm > 0.0, "{counts:?}");
    }
}
