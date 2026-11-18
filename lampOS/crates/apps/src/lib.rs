//! The pure parts of the apps' manage helper (#7415, piece 2).
//!
//! An app is a Store package (FORMAT 2) the lamp installs as a static
//! systemd instance, `lampapp@<name>.service` or, with the manifest's
//! top-level `net`, `lampapp-net@<name>.service`. The helper writes no unit
//! and makes no user: systemd makes the user (`DynamicUser=yes`) and the data
//! folder (`StateDirectory=`). What this crate holds is what can be tested
//! with no root and no systemd: the names, the one manifest-derived line
//! (`entry.env`), the order a remove runs in, the sweep after an interrupted
//! install or remove, the rule for a staged package, and the launcher.
//!
//! Nothing here stores or compares a uid: a dynamic user's is not stable.
pub mod effects;
pub mod entry;
pub mod facts;
pub mod layout;
pub mod name;
pub mod remove;
pub mod staging;
pub mod state;
pub mod sweep;
#[doc(hidden)]
pub mod testing;

/// The fixed launcher, root's, shipped beside the helper (never in a
/// package): `python3 -I` drops the app's `src/` from `sys.path`, so this
/// puts it back and runs the entry named in `entry.env`.
pub const LAUNCHER: &str = include_str!("../launcher/run.py");
