//! Explicit native ARM64 checks of the published Voice delivery-ledger reader.
//! The released `schedule-consumer` writes and reopens every live ledger. The
//! fixture supplies Schedule replies and an authored whole-reply completion;
//! it never opens capture, a provider, a speaker, or an actuator. This establishes
//! persisted consumer continuity, not acoustic completion or Schedule durability.
//! See `release_pair/README.md` for the exact manual boundary and artifact pins.

#[path = "release_pair/fixture.rs"]
mod fixture;

use fixture::{Cut, Release, check};

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6498_reads_6408_unfinished_delivery_and_completes_it() {
    check(Release::Old, Release::Current, Cut::Unfinished);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6408_reads_6498_unfinished_delivery_and_completes_it() {
    check(Release::Current, Release::Old, Cut::Unfinished);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6498_reads_6408_completed_delivery_without_another_offer() {
    check(Release::Old, Release::Current, Cut::Completed);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6408_reads_6498_completed_delivery_without_another_offer() {
    check(Release::Current, Release::Old, Cut::Completed);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6498_replays_6408_exact_ack_intent_without_another_offer() {
    check(Release::Old, Release::Current, Cut::AckIntent);
}

#[test]
#[ignore = "requires pinned published executables on isolated native Linux ARM64"]
fn release_6408_replays_6498_exact_ack_intent_without_another_offer() {
    check(Release::Current, Release::Old, Cut::AckIntent);
}
