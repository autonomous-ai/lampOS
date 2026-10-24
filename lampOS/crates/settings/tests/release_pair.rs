//! Actual CLI/store compatibility; released payloads run only by explicit native opt-in.
#[path = "release_pair/fixture.rs"]
mod fixture;

#[test]
fn current_settings_cli_store_smoke() {
    fixture::run("current_settings_cli_store_smoke", None);
}

#[test]
#[ignore = "manual native Linux/aarch64 released Settings pair; retained private evidence"]
fn release_6498_settings_reads_6408_and_6408_reads_return() {
    fixture::run(
        "release_6498_settings_reads_6408_and_6408_reads_return",
        Some((6408, 6498)),
    );
}

#[test]
#[ignore = "manual native Linux/aarch64 released Settings pair; retained private evidence"]
fn release_6408_settings_reads_6498_and_6498_reads_return() {
    fixture::run(
        "release_6408_settings_reads_6498_and_6498_reads_return",
        Some((6498, 6408)),
    );
}
