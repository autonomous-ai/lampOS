//! The owner's Update now (#7517, `LAMP_UPDATE_NOW=on`): a round the owner's
//! app asked for switches without waiting for a quiet moment.
//!
//! lamp-cloud takes `update.now` only from the lamp's own account channel
//! (`online::commanded`), writes its mark ([`lamp_msgs::UPDATE_NOW_FILE`],
//! in its own 0700 state folder) and says `update/check` with `now`. The bus
//! keeps no list of who may say a topic, so a `now` check counts only with
//! that mark beside it, at most [`lamp_msgs::UpdateCheck::NOW_WITHIN`] old,
//! and each mark only once ([`Marks::vouched`]). Any other `now` is a plain
//! check.
//!
//! The round it asks for, or a round it finds waiting for its quiet moment,
//! keeps every other rule (the lock, the signature, the room on the disk,
//! the reset hold, the setup walk, the minute's watch and the put-back).
//! Before its switch it gives the voice [`GIVE_WAY_QUIET`] of quiet, at
//! most [`GIVE_WAY_MOST`], so a voice talking with someone can say it is
//! updating and end its turn; then it switches, quiet or not.
use std::{
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::Duration,
};

/// The switch, in lamp-update's environment: `on` exactly. Off by default.
pub const SWITCH: &str = "LAMP_UPDATE_NOW";

/// The quiet the voice is given before an Update now's switch: time to end
/// a turn and say its one line.
pub const GIVE_WAY_QUIET: Duration = Duration::from_secs(3);

/// The most an Update now's switch waits for the voice: past it, it
/// switches anyway (the first builder's "the switch within 10 s").
pub const GIVE_WAY_MOST: Duration = Duration::from_secs(10);

/// The most bytes of the mark read: it is one small JSON object.
const MOST_MARK: u64 = 256;

/// Whether the switch is on, from the value the environment gives.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// What may vouch for a `now` check: lamp-cloud's mark at `file`, and the
/// last mark taken, so one tap is taken once.
#[derive(Debug)]
pub struct Marks {
    file: PathBuf,
    taken: Option<u64>,
}

impl Marks {
    /// The mark at [`lamp_msgs::UPDATE_NOW_FILE`] under `root`.
    pub fn under(root: &Path) -> Self {
        Self::at(root.join(lamp_msgs::UPDATE_NOW_FILE.trim_start_matches('/')))
    }

    /// The mark at `file`.
    pub fn at(file: PathBuf) -> Self {
        Self { file, taken: None }
    }

    /// Whether a `now` check heard at `now_unix_ms` is the owner's: the
    /// mark is there, a plain file in a folder only its owner may write,
    /// the file its folder owner's and no one else's to write, and its
    /// `at_unix_ms` at most [`lamp_msgs::UpdateCheck::NOW_WITHIN`] before
    /// now (and not more than a second after, a clock's step), and not the
    /// mark taken last. Taken, it is never taken again.
    pub fn vouched(&mut self, now_unix_ms: u64) -> Result<u64, String> {
        let at = read_mark(&self.file)?;
        let within = lamp_msgs::UpdateCheck::NOW_WITHIN.as_millis() as u64;
        if at > now_unix_ms.saturating_add(1_000) {
            return Err("its mark is dated ahead of the lamp's clock".into());
        }
        if now_unix_ms.saturating_sub(at) > within {
            return Err(format!(
                "its mark is {} ms old, more than {within} ms",
                now_unix_ms.saturating_sub(at)
            ));
        }
        if self.taken == Some(at) {
            return Err("its mark was taken already".into());
        }
        self.taken = Some(at);
        Ok(at)
    }
}

/// The mark's `at_unix_ms`, if the file and its folder are as only
/// lamp-cloud leaves them.
fn read_mark(file: &Path) -> Result<u64, String> {
    let folder = file.parent().ok_or("its mark has no folder")?;
    let folder_meta =
        std::fs::symlink_metadata(folder).map_err(|error| format!("no mark: {error}"))?;
    if !folder_meta.is_dir() || folder_meta.mode() & 0o022 != 0 {
        return Err("its mark's folder is not one only its owner may write".into());
    }
    let meta = std::fs::symlink_metadata(file).map_err(|error| format!("no mark: {error}"))?;
    if !meta.is_file() {
        return Err("its mark is not a plain file".into());
    }
    if meta.mode() & 0o022 != 0 || meta.uid() != folder_meta.uid() {
        return Err("its mark is not its folder owner's alone".into());
    }
    let mut text = String::new();
    std::fs::File::open(file)
        .and_then(|opened| opened.take(MOST_MARK).read_to_string(&mut text))
        .map_err(|error| format!("its mark cannot be read: {error}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "its mark is not one lamp-cloud writes")?;
    value
        .get("at_unix_ms")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "its mark is not one lamp-cloud writes".into())
}

/// The round's side of an Update now (`round::World::now`): whether one was
/// heard since the last look, and what the round says of it on
/// `update/status`.
pub trait NowAsk {
    /// Whether an Update now, vouched for, was heard since the last look
    /// (or started this round). Each is told once.
    fn heard(&mut self) -> bool;
    /// What the round waits for now (`lamp_msgs::UpdateRound::WAITING_*`),
    /// or none.
    fn waiting(&mut self, waiting: Option<&'static str>);
    /// The round is one of Update now's.
    fn asked(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mark(folder: &Path, at: u64) -> PathBuf {
        let file = folder.join("update-now");
        std::fs::write(&file, format!("{{\"at_unix_ms\":{at}}}")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        file
    }

    fn folder() -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        std::fs::set_permissions(folder.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        folder
    }

    #[test]
    fn the_switch_is_on_only_when_set_on_exactly() {
        assert!(on(Some("on")));
        for value in [None, Some(""), Some("ON"), Some("1"), Some("off")] {
            assert!(!on(value), "{value:?}");
        }
    }

    #[test]
    fn a_now_check_counts_only_within_five_seconds_of_lamp_clouds_mark_and_once() {
        let folder = folder();
        let file = mark(folder.path(), 100_000);
        let mut marks = Marks::at(file.clone());
        // Within 5 s: taken, once.
        assert_eq!(marks.vouched(104_000), Ok(100_000));
        assert!(
            marks
                .vouched(104_500)
                .unwrap_err()
                .contains("taken already")
        );
        // A new tap's mark is taken again.
        mark(folder.path(), 110_000);
        assert_eq!(marks.vouched(110_200), Ok(110_000));
        // Older than 5 s: a plain check.
        mark(folder.path(), 120_000);
        assert!(marks.vouched(125_001).unwrap_err().contains("ms old"));
        // Dated ahead of the clock by more than a second.
        mark(folder.path(), 140_000);
        assert!(marks.vouched(138_000).unwrap_err().contains("ahead"));
    }

    #[test]
    fn a_bare_now_from_anywhere_else_is_a_plain_check() {
        // No mark at all: nobody's tap vouches for it.
        let folder = folder();
        let mut marks = Marks::at(folder.path().join("update-now"));
        assert!(marks.vouched(1_000).unwrap_err().starts_with("no mark"));
        // A mark others may write, or in a folder others may write.
        let file = mark(folder.path(), 1_000);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o622)).unwrap();
        assert!(marks.vouched(1_500).unwrap_err().contains("alone"));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(folder.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(marks.vouched(1_500).unwrap_err().contains("folder"));
        std::fs::set_permissions(folder.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        // A link in its place, and words that are not a mark.
        std::fs::remove_file(&file).unwrap();
        let elsewhere = folder.path().join("elsewhere");
        std::fs::write(&elsewhere, "{\"at_unix_ms\":1000}").unwrap();
        std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
        assert!(marks.vouched(1_500).unwrap_err().contains("plain file"));
        std::fs::remove_file(&file).unwrap();
        std::fs::write(&file, "now").unwrap();
        assert!(marks.vouched(1_500).unwrap_err().contains("not one"));
    }
}
