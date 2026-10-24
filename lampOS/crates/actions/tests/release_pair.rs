//! Explicit native qualification with pinned published executables.
//! Ordinary tests compile this driver but do not execute its external artifacts.
#[path = "release_pair/fixture.rs"]
mod fixture;

#[test]
#[ignore = "requires the pinned 6408/6498 ELF files on native Linux ARM64"]
fn release_6498_keeps_6408_caps_and_6408_reads_its_changes() {
    fixture::run(
        fixture::Release::Old,
        fixture::Release::Current,
        "release_6498_keeps_6408_caps_and_6408_reads_its_changes",
    );
}

#[test]
#[ignore = "requires the pinned 6408/6498 ELF files on native Linux ARM64"]
fn release_6408_keeps_6498_caps_and_6498_reads_its_changes() {
    fixture::run(
        fixture::Release::Current,
        fixture::Release::Old,
        "release_6408_keeps_6498_caps_and_6498_reads_its_changes",
    );
}
