//! Actual published Skills readers and writers, in both release directions.
//! Eight manual native ARM64 cases cover daily/on-off state, rested nights
//! and local package installation/loading.
//! Rested cases use synthetic Expression receipts and one private stretch
//! datagram; no actuator or provider process runs.

#[path = "release_pair/fixture.rs"]
mod fixture;

use fixture::{Release, daily, on_off, packages, rested_night};

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6498_keeps_6408_daily_claims_and_6408_reads_them_back() {
    daily(Release::Old, Release::Current);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6408_keeps_6498_daily_claims_and_6498_reads_them_back() {
    daily(Release::Current, Release::Old);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6498_reads_6408_on_off_and_6408_reads_the_changes_back() {
    on_off(Release::Old, Release::Current);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6408_reads_6498_on_off_and_6498_reads_the_changes_back() {
    on_off(Release::Current, Release::Old);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6498_reads_6408_rested_night_and_6408_keeps_its_morning_spent() {
    rested_night(Release::Old, Release::Current);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6408_reads_6498_rested_night_and_6498_keeps_its_morning_spent() {
    rested_night(Release::Current, Release::Old);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6498_installs_6408_packages_and_6408_loads_the_replacement() {
    packages(Release::Old, Release::Current);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6408_installs_6498_packages_and_6498_loads_the_replacement() {
    packages(Release::Current, Release::Old);
}
