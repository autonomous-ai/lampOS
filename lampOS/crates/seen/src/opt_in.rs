//! The owner's opt-in for `lamp-seen` (#6543, phase 0's first line): off by
//! default, set in the app or by the owner's spoken yes, and off again on
//! forget everything.
//!
//! It is read as the app's two yeses are (#5884, `lamp_msgs::consent`): a
//! file in the consent folder, present only while the yes stands, whose key
//! is exactly `"yes"`. Missing, unreadable, too long, not JSON or anything
//! but `"yes"` is off.
//!
//! The key and its file are not decided yet: Fourteen, who owns the app,
//! is choosing them. Until then they are the placeholders [`KEY`] and
//! [`FILE`], read through [`Reader`] so the store can change without the
//! gate changing. Nothing writes them yet, and no key is added to
//! `lamp-cloud`'s `consent.set`.
// TODO(#6543): take the key and the file Fourteen chooses for the app's
// opt-in, and add it to lamp_msgs::consent::FILES and lamp-cloud's
// consent.set beside face expressions and room sounds.
use lamp_msgs::consent::{self, Kept};
use std::path::{Path, PathBuf};

/// The placeholder key, in the file and (later) on the wire. TBD with
/// Fourteen (#6543).
pub const KEY: &str = "seen_activity";

/// The placeholder file's name in the consent folder. TBD with Fourteen
/// (#6543).
pub const FILE: &str = "seen-activity.json";

/// Where the opt-in is kept.
pub trait Reader {
    /// What the kept opt-in says now. Anything missing or unreadable is
    /// [`Kept::default`]: no yes.
    fn read(&self) -> Kept;
}

/// The opt-in kept as a file in a consent folder, as the app's two yeses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub path: PathBuf,
}

impl File {
    /// The product's: [`FILE`] in `lamp_msgs::consent::FOLDER`.
    pub fn product() -> Self {
        Self::in_folder(Path::new(consent::FOLDER))
    }

    /// [`FILE`] in `folder`.
    pub fn in_folder(folder: &Path) -> Self {
        Self {
            path: folder.join(FILE),
        }
    }
}

impl Reader for File {
    fn read(&self) -> Kept {
        consent::read(&self.path, KEY)
    }
}

/// The opt-in as the gate takes it: a kept yes, and no forget since it was
/// given.
///
/// Forget everything (`memory/forgot`) turns the opt-in off (phase 0's
/// fifth line). Until the key is chosen this part cannot remove the kept
/// yes itself (only root's helper writes the consent folder), so it holds
/// the yes as off from the forget on, and takes a yes again only when it
/// was given after the forget (its `at_unix_s` later). A yes with no time
/// cannot be shown to be later, so it stays off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OptIn {
    /// When the last forget came, in Unix seconds, if one has since start.
    forgot_at_unix_s: Option<u64>,
}

impl OptIn {
    /// A forget came at `now_unix_s`: the opt-in is off from now on until a
    /// later yes.
    pub fn forgot(&mut self, now_unix_s: u64) {
        self.forgot_at_unix_s = Some(
            self.forgot_at_unix_s
                .map_or(now_unix_s, |at| at.max(now_unix_s)),
        );
    }

    /// Whether `kept` is a yes the gate takes.
    pub fn on(&self, kept: &Kept) -> bool {
        if !kept.yes {
            return false;
        }
        match self.forgot_at_unix_s {
            None => true,
            Some(forgot) => kept.at_unix_s.is_some_and(|given| given > forgot),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("seen-opt-in-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn off_unless_a_kept_yes() {
        let folder = folder("off");
        let file = File::in_folder(&folder);
        let latch = OptIn::default();
        assert!(!latch.on(&file.read()), "missing");
        for text in [
            "",
            "yes",
            "[1]",
            r#"{"seen_activity":"no"}"#,
            r#"{"seen_activity":true}"#,
            r#"{"seen_activity":"YES"}"#,
            r#"{"face_expressions":"yes"}"#,
        ] {
            std::fs::write(&file.path, text).unwrap();
            assert!(!latch.on(&file.read()), "{text}");
        }
        std::fs::write(&file.path, vec![b' '; consent::MOST_BYTES as usize + 1]).unwrap();
        assert!(!latch.on(&file.read()), "too long");
        std::fs::write(&file.path, consent::bytes(KEY, 7, consent::BY_APP)).unwrap();
        assert!(latch.on(&file.read()), "a kept yes");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn the_product_reads_the_consent_folder() {
        assert_eq!(File::product().path, Path::new(consent::FOLDER).join(FILE));
    }

    #[test]
    fn a_forget_turns_it_off_until_a_later_yes() {
        let yes_at = |at: Option<u64>| Kept {
            yes: true,
            by: Some("app".into()),
            at_unix_s: at,
        };
        let mut latch = OptIn::default();
        assert!(latch.on(&yes_at(Some(100))));
        latch.forgot(200);
        assert!(!latch.on(&yes_at(Some(100))), "given before the forget");
        assert!(!latch.on(&yes_at(Some(200))), "given in the same second");
        assert!(!latch.on(&yes_at(None)), "a yes with no time");
        assert!(latch.on(&yes_at(Some(201))), "given again after");
        // An earlier forget never moves it back.
        latch.forgot(150);
        assert!(!latch.on(&yes_at(Some(200))));
        assert!(!latch.on(&Kept::default()));
    }
}
