//! Manual qualification with actual published Skills writers and Cloud readers.
#[path = "release_pair/fixture.rs"]
mod fixture;

#[test]
#[ignore = "requires pinned releases in a fresh isolated native ARM64 container"]
fn release_6498_cloud_reads_6408_packages_and_6408_reads_the_replacement() {
    fixture::run(
        6408,
        6498,
        "release_6498_cloud_reads_6408_packages_and_6408_reads_the_replacement",
    );
}

#[test]
#[ignore = "requires pinned releases in a fresh isolated native ARM64 container"]
fn release_6408_cloud_reads_6498_packages_and_6498_reads_the_replacement() {
    fixture::run(
        6498,
        6408,
        "release_6408_cloud_reads_6498_packages_and_6498_reads_the_replacement",
    );
}
