//! Manual actual-release persistence checks. The harness is a native Linux
//! ARM64 protocol client; only pinned released lamp-schedule binaries write
//! stores. Boot IDs and wall trust are explicit fixture inputs, not reboots or
//! evidence of NTP, speech delivery, systemd or physical flash durability.
//! Both published releases carry the same Schedule bytes; this is payload
//! continuity evidence, not a Schedule implementation-delta qualification.
#[path = "release_pair/driver.rs"]
mod driver;

macro_rules! pair {
    ($name:ident, $first:literal, $second:literal, $changed:literal) => {
        #[test]
        #[ignore = "manual pinned released binary paths; native Linux ARM64 only"]
        fn $name() {
            driver::scenario($first, $second, $changed);
        }
    };
}
pair!(
    schedule_6408_to_6498_to_6408_same_boot,
    "6408",
    "6498",
    false
);
pair!(
    schedule_6498_to_6408_to_6498_same_boot,
    "6498",
    "6408",
    false
);
pair!(
    schedule_6408_to_6498_to_6408_changed_boot_identity,
    "6408",
    "6498",
    true
);
pair!(
    schedule_6498_to_6408_to_6498_changed_boot_identity,
    "6498",
    "6408",
    true
);
