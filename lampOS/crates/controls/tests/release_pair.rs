//! Actual published Controls writers/readers; ordinary execution uses today's
//! CLI only to check the fixture. Neither path serves hardware or opens a bus.
#[path = "release_pair/fixture.rs"]
mod fixture;

#[test]
fn current_cli_exercises_the_controls_pair_fixture() {
    fixture::timeout_control();
    fixture::run(
        false,
        false,
        "current_cli_exercises_the_controls_pair_fixture",
    );
}

#[test]
#[ignore = "requires pinned 6408/6498 artifacts on native Linux ARM64"]
fn release_6498_reads_6408_calibration_and_6408_reads_returned_pins() {
    fixture::run(
        true,
        false,
        "release_6498_reads_6408_calibration_and_6408_reads_returned_pins",
    );
}

#[test]
#[ignore = "requires pinned 6408/6498 artifacts on native Linux ARM64"]
fn release_6408_reads_6498_calibration_and_6498_reads_returned_pins() {
    fixture::run(
        true,
        true,
        "release_6408_reads_6498_calibration_and_6498_reads_returned_pins",
    );
}
