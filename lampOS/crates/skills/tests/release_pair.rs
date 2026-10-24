//! Actual published Skills readers and writers, in both release directions.
//! These manual native ARM64 cases cover daily claims and on/off records only.
//! Policy never approves an output; all output paths are private traps.
//! `rested` and actuator behavior remain separate follow-up work.

#[path = "release_pair/fixture.rs"]
mod fixture;

use fixture::{Release, daily, on_off};

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
