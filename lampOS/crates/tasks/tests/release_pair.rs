//! Manual, offline checks of the published 6408 and 6498 Tasks executables.
//! Run on Linux ARM64 with LAMP_COMPAT_TASKS_6408 and LAMP_COMPAT_TASKS_6498
//! pointing to the pinned artifacts. LAMP_COMPAT_OUTPUT optionally selects
//! the parent for retained evidence. No local Tasks binary writes the store.
//! Harness responses and Voice attestations are fixtures, not completed work
//! or real speech. These tests neither use a network nor prove power durability.

#[path = "release_pair/linux.rs"]
mod linux;

#[test]
#[ignore = "requires the pinned published Tasks executables on native Linux ARM64"]
fn release_6498_reads_writes_and_reopens_release_6408_tasks() {
    linux::run(true);
}

#[test]
#[ignore = "requires the pinned published Tasks executables on native Linux ARM64"]
fn release_6408_reads_writes_and_reopens_release_6498_tasks() {
    linux::run(false);
}
