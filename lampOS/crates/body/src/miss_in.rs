//! Missed servo reads from a test input (`lamp-body --miss-from FILE`,
//! #8266): for tests on a lamp only, so that a row about a gesture that
//! meets a missed read need not wait for the wire to miss one by itself (0
//! in a night's run, 3 in 17,000 steps). No product unit passes the flag (a
//! test checks every unit under `deploy/`).
//!
//! The file is read at most every [`crate::hold_in::LOOK_EVERY`]. Each time
//! it is written with `miss N` as its first two words (N from 1 to
//! [`MOST`]), the body's next N reads of the servos fail where a read that
//! got no reply fails, and nothing else changes. Any other text, an empty
//! file or none at all asks nothing; a file found already there at start
//! asks nothing either, so a body started again does not miss for an old
//! test. A write is told from the last by its time or its text: the same
//! text written again within the clock's tick is not seen.

use std::{
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

use crate::hold_in::LOOK_EVERY;

/// The most reads one write may miss: the body's own `WIRE_STEPS`, at which
/// the run ends as on a wire that stays silent.
pub const MOST: u8 = 25;

/// What one read of the file asks: how many reads to miss, if any.
pub fn said_misses(text: &str) -> Option<u8> {
    let mut words = text.split_whitespace();
    if words.next() != Some("miss") {
        return None;
    }
    let count = words.next()?.parse::<u8>().ok()?;
    (1..=MOST).contains(&count).then_some(count)
}

/// The body's end of `--miss-from`.
pub struct MissIn {
    path: PathBuf,
    looked: Option<Instant>,
    /// The last write seen: its time and its text.
    seen: Option<(Option<SystemTime>, String)>,
}

impl MissIn {
    pub fn from_file(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            looked: None,
            seen: None,
        }
    }

    /// The file it reads.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Option<(Option<SystemTime>, String)> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        let modified = std::fs::metadata(&self.path)
            .and_then(|meta| meta.modified())
            .ok();
        Some((modified, text))
    }

    /// How many reads a new write of the file asks to miss from now: 0 but
    /// once per write. Never waits longer than a read of a small file, and
    /// reads it at most every [`LOOK_EVERY`].
    pub fn misses(&mut self, now: Instant) -> u8 {
        if self.looked.is_some_and(|at| now - at < LOOK_EVERY) {
            return 0;
        }
        let first = self.looked.is_none();
        self.looked = Some(now);
        let read = self.read();
        if read == self.seen {
            return 0;
        }
        self.seen = read;
        match &self.seen {
            Some((_, text)) if !first => said_misses(text).unwrap_or(0),
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_miss_and_a_count_from_one_to_twenty_five_asks() {
        assert_eq!(said_misses("miss 1"), Some(1));
        assert_eq!(said_misses("miss 5\n"), Some(5));
        assert_eq!(said_misses("  miss 25 now"), Some(25));
        for text in [
            "", "miss", "miss 0", "miss 26", "miss -1", "miss x", "MISS 3", "free", "held",
            "3 miss",
        ] {
            assert_eq!(said_misses(text), None, "{text:?}");
        }
    }

    #[test]
    fn each_write_asks_once_and_one_there_at_start_asks_nothing() {
        let root = std::env::temp_dir().join(format!("lamp-body-miss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("miss");
        std::fs::write(&file, "miss 3\n").unwrap();
        let mut miss = MissIn::from_file(&file);
        let start = Instant::now();
        assert_eq!(miss.misses(start), 0, "there at start: asks nothing");
        assert_eq!(miss.misses(start + LOOK_EVERY), 0, "not written again");
        std::fs::write(&file, "miss 2\n").unwrap();
        // Not read again before its time.
        assert_eq!(miss.misses(start + LOOK_EVERY * 3 / 2), 0);
        assert_eq!(miss.misses(start + LOOK_EVERY * 2), 2);
        assert_eq!(miss.misses(start + LOOK_EVERY * 3), 0, "once per write");
        std::fs::write(&file, "miss 26").unwrap();
        assert_eq!(miss.misses(start + LOOK_EVERY * 4), 0, "out of range");
        std::fs::write(&file, "miss 5").unwrap();
        assert_eq!(miss.misses(start + LOOK_EVERY * 5), 5);
        std::fs::remove_file(&file).unwrap();
        assert_eq!(miss.misses(start + LOOK_EVERY * 6), 0, "removed");
        std::fs::write(&file, "miss 5").unwrap();
        assert_eq!(
            miss.misses(start + LOOK_EVERY * 7),
            5,
            "written after a removal"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
