//! A hand on the head from a test input (`lamp-body --hold-from FILE`,
//! #2668): for tests on a lamp only, so a row that needs the head held by a
//! hand, once proven by hand, runs again with no person there. No product
//! unit passes the flag (a test checks every unit under `deploy/`).
//!
//! The file's first word is read at most every [`LOOK_EVERY`]: `held` is the
//! head's pitch held by a hand ([`crate::body::Asked::held_by_test`]);
//! `free`, any other word, an empty file or none at all is no hand. Whoever
//! writes the file puts the hand on and takes it off.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// How often the file is read: five motor steps, soon enough for a hand.
pub const LOOK_EVERY: Duration = Duration::from_millis(100);

/// What one read of the file says: whether the head is held.
pub fn said_held(text: &str) -> bool {
    text.split_whitespace().next() == Some("held")
}

/// The body's end of `--hold-from`.
pub struct HoldIn {
    path: PathBuf,
    looked: Option<Instant>,
    held: bool,
}

impl HoldIn {
    pub fn from_file(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            looked: None,
            held: false,
        }
    }

    /// The file it reads.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the test input holds the head now. Never waits longer than a
    /// read of a small file, and reads it at most every [`LOOK_EVERY`].
    pub fn held(&mut self, now: Instant) -> bool {
        if self.looked.is_none_or(|at| now - at >= LOOK_EVERY) {
            self.looked = Some(now);
            self.held = std::fs::read_to_string(&self.path).is_ok_and(|text| said_held(&text));
        }
        self.held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_held_as_the_first_word_holds_the_head() {
        assert!(said_held("held"));
        assert!(said_held("held\n"));
        assert!(said_held("  held pitch"));
        for word in [
            "",
            "free",
            "HELD",
            "hold",
            "{\"held\":\"pitch\"}",
            "free held",
        ] {
            assert!(!said_held(word), "{word:?}");
        }
    }

    #[test]
    fn the_file_is_read_at_most_every_tenth_of_a_second_and_none_is_free() {
        let root = std::env::temp_dir().join(format!("lamp-body-hold-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("hold");
        let mut hold = HoldIn::from_file(&file);
        let start = Instant::now();
        assert!(!hold.held(start), "no file: no hand");
        std::fs::write(&file, "held\n").unwrap();
        // Not read again before its time.
        assert!(!hold.held(start + LOOK_EVERY / 2));
        assert!(hold.held(start + LOOK_EVERY));
        std::fs::write(&file, "free").unwrap();
        assert!(hold.held(start + LOOK_EVERY * 3 / 2));
        assert!(!hold.held(start + LOOK_EVERY * 2));
        std::fs::write(&file, "held").unwrap();
        assert!(hold.held(start + LOOK_EVERY * 3));
        std::fs::remove_file(&file).unwrap();
        assert!(!hold.held(start + LOOK_EVERY * 4), "removed: let go");
        let _ = std::fs::remove_dir_all(&root);
    }
}
