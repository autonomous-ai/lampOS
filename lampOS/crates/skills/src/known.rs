//! The eyes' known file (`lamp-eyes --known-file`), read only: the name of
//! the known person at the lamp while they are there, renewed every 2 s,
//! removed when nobody is near. Only someone who asked the lamp to remember
//! their face is ever in it.
//!
//! Who was here (#1685; Third's privacy read and the first builder's (a), 20
//! Oct 00:15 on #1997) reads it with `LAMP_WHO_WAS_HERE_KNOWN=on` for one
//! mark per spell: someone the lamp knows was there. The name is read and
//! let go at once, never kept or said: being remembered for a greeting
//! isn't agreeing to have your visits told to whoever asks.
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Where Who was here's switch is read. `on` only.
pub const ENVIRONMENT: &str = "LAMP_WHO_WAS_HERE_KNOWN";
/// Who was here's bound: the eyes' own renewal period, so a stranger who
/// sits down just after the known person left is not marked for them.
pub const MARK_FRESH: Duration = Duration::from_secs(2);
/// The longest name the eyes keep (`who::MOST_NAME_CHARS`), in bytes with
/// room for any letters.
const MOST_NAME_BYTES: usize = 160;

/// Whether Who was here's switch is on.
pub fn marking() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The name in the known file at `path`, if it is no older than `fresh`.
pub fn name_in(path: &Path, fresh: Duration) -> Option<String> {
    let age = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())?;
    if age > fresh {
        return None;
    }
    let name = std::fs::read_to_string(path).ok()?;
    let name = name.trim();
    (!name.is_empty() && name.len() <= MOST_NAME_BYTES && !name.contains('\n'))
        .then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_known_file_gives_the_name_only_while_fresh_and_plain() {
        let dir = tempfile::tempdir().unwrap();
        let known = dir.path().join("known");
        let fresh = Duration::from_secs(5);
        assert_eq!(name_in(&known, fresh), None, "no file: nobody known");
        std::fs::write(&known, "Alpha\n").unwrap();
        assert_eq!(name_in(&known, fresh).as_deref(), Some("Alpha"));
        std::fs::write(&known, "\n").unwrap();
        assert_eq!(name_in(&known, fresh), None, "empty");
        std::fs::write(&known, "Alpha\nBeta\n").unwrap();
        assert_eq!(name_in(&known, fresh), None, "two lines");
        std::fs::write(&known, "Alpha\n").unwrap();
        let old = SystemTime::now() - Duration::from_secs(3);
        std::fs::File::options()
            .write(true)
            .open(&known)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(name_in(&known, MARK_FRESH), None, "past Who was here's 2 s");
        assert_eq!(
            name_in(&known, fresh).as_deref(),
            Some("Alpha"),
            "within 5 s"
        );
    }
}
