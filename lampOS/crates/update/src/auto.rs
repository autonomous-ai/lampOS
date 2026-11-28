//! The owner's Auto update (#8769, the product owner's rule through the
//! lead): on by default, turned off in the app (`update.auto`, which
//! setup's change helper keeps as [`OFF`]).
//!
//! - **On** (no [`OFF`]): a round does not wait for a quiet moment. It gives
//!   the voice its moment as the owner's Update now does
//!   (`round::Run::give_way`, at most `update_now::GIVE_WAY_MOST`), so a
//!   voice talking with someone can say it is updating, and then it
//!   switches, quiet or not.
//! - **Off** ([`OFF`] there): a round fetches and checks as before, but
//!   switches only for the owner's Update now; otherwise it ends
//!   `not-quiet` with [`OFF_WAITS`], and the next round looks again.
//!
//! Either way the first update after setup (#6284), the setup walk's hold,
//! a reset's hold and the watch with its put-back are as before. Read at
//! each round, so the owner's switch counts from the next one.

use std::path::Path;

/// The switch, `on` or `off` (`hermes_install::wanted`): unset, on on a
/// product lamp ([`PRODUCT_DEFAULT`]) and off elsewhere, as
/// `LAMP_FIRST_UPDATE` is, since lamp-update's own unit is never carried by
/// a round.
pub const SWITCH: &str = "LAMP_UPDATE_AUTO";

/// On by default on a product lamp: the product owner's rule (#8769).
pub const PRODUCT_DEFAULT: bool = true;

/// The mark of Auto update turned off, written by setup's change helper
/// (root's, 0644). Absent, as on a new lamp and after a reset, it is on.
pub const OFF: &str = lamp_msgs::AUTO_UPDATE_OFF;

/// Why a round with Auto update off did not switch.
pub const OFF_WAITS: &str = "Auto update is off: the new build waits for the owner's Update now";

/// Whether Auto update is on for the lamp whose root is `root`: anything
/// but [`OFF`] there (a file, or a link, read or not) is on. Only a plain
/// file turns it off, so a mark that can't be looked at never stops
/// updates.
pub fn on(root: &Path) -> bool {
    let off = root.join(OFF.trim_start_matches('/'));
    !std::fs::symlink_metadata(off).is_ok_and(|meta| meta.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_unless_the_off_mark_is_a_plain_file() {
        let root = tempfile::tempdir().unwrap();
        assert!(on(root.path()), "no mark: on");
        let off = root.path().join(OFF.trim_start_matches('/'));
        std::fs::create_dir_all(off.parent().unwrap()).unwrap();
        std::fs::write(&off, "").unwrap();
        assert!(!on(root.path()), "the mark: off");
        std::fs::remove_file(&off).unwrap();
        std::fs::create_dir(&off).unwrap();
        assert!(on(root.path()), "a folder in its place is no mark");
    }
}
