//! A first-time setup's journal, kept on the eMMC past the 20M cap by the
//! image's `lamp-journal-keep setup` (#5144, `docs/install.md`, "A
//! first-time setup's journal"). Its lines can hold what the person said at
//! setup, so the owner's own "forget everything about me" deletes it, on the
//! same mark and rule as their place and their voice print (the PM's privacy
//! call on #5342). It goes by itself 72 hours after the walk began, and at a
//! redo or a reset.
//!
//! The voice runs as root, as the keep does. The copies go, and so do their
//! links where journalctl reads; a `.forgotten` mark is left beside them so
//! the keep copies nothing more of the same walk (`lamp-journal-keep`).
use serde_json::{Value, json};
use std::{io, path::Path};

/// Where the keep puts the setup's copies.
pub const KEPT: &str = "/var/lib/lamp-journal/setup";
/// Where the keep links them for journalctl.
pub const LINKS: &str = "/run/log/journal";
/// The keep's links' names begin so.
const LINK_PREFIX: &str = "lamp-kept-setup-";
/// The mark the keep reads: this walk's journal was forgotten.
const FORGOTTEN: &str = ".forgotten";
/// What the answer says when the copies could not all be removed.
const KEPT_SAID: &str = "kept: it could not be removed";

/// The owner's "forget everything about me", marked by the second route's
/// guard: the setup's kept journal goes too, and the answer says so.
/// Another person's forget, the room's, an unmarked one, or one whose
/// memory was not emptied, leaves it.
pub fn forget_with_everything(
    told: (Value, bool),
    lease: &crate::memory_scope::ScopeLease,
) -> (Value, bool) {
    if !lease.everything_marked() {
        return told;
    }
    forget_if(
        Path::new(KEPT),
        Path::new(LINKS),
        told,
        lease.admitted_person(),
        &crate::person::owner_ids(),
    )
}

/// [`forget_with_everything`] once its mark is judged: the copies in `kept`
/// and their links in `links` removed when `admitted`, the line's person,
/// is one of `owners` and `told` says their memory was emptied.
pub(crate) fn forget_if(
    kept: &Path,
    links: &Path,
    mut told: (Value, bool),
    admitted: Option<&str>,
    owners: &[String],
) -> (Value, bool) {
    let emptied =
        told.0["forgotten"] == true || told.0["note"] == json!("I was keeping nothing of them.");
    let theirs = admitted.is_some_and(|admitted| owners.iter().any(|owner| owner == admitted));
    if !emptied || !theirs {
        return told;
    }
    match wipe(kept, links) {
        Ok(true) => {
            eprintln!(
                "lamp-voice: the setup's kept journal was removed with everything about them"
            );
            told.0["setup_log"] = json!("forgotten too");
        }
        Ok(false) => {}
        Err(error) => {
            eprintln!(
                "lamp-voice: the setup's kept journal could not be removed with everything about them: {}",
                error.kind()
            );
            told.0["everything"] = json!(false);
            told.0["setup_log"] = json!(KEPT_SAID);
        }
    }
    told
}

/// Removes the copies in `kept` and their links in `links`, leaving the
/// keep's mark that this walk was forgotten. Whether any was there; no
/// folder is nothing to forget.
fn wipe(kept: &Path, links: &Path) -> io::Result<bool> {
    let entries = match std::fs::read_dir(kept) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    // The mark first, so a keep running now copies nothing after it.
    std::fs::write(kept.join(FORGOTTEN), b"")?;
    let mut any = false;
    for entry in entries {
        let entry = entry?;
        if entry.file_name().to_string_lossy().ends_with(".journal") {
            std::fs::remove_file(entry.path())?;
            any = true;
        }
    }
    if let Ok(entries) = std::fs::read_dir(links) {
        for entry in entries {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with(LINK_PREFIX) {
                std::fs::remove_file(entry.path())?;
                any = true;
            }
        }
    }
    Ok(any)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamp(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "lamp-voice-setup-log-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let (kept, links) = (root.join("setup"), root.join("run"));
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::create_dir_all(&links).unwrap();
        for name in ["system@a-1.journal", ".system@a-2.journal"] {
            std::fs::write(kept.join(name), "j").unwrap();
        }
        std::fs::write(kept.join(".started"), "").unwrap();
        std::os::unix::fs::symlink(
            kept.join("system@a-1.journal"),
            links.join("lamp-kept-setup-system.a-1.journal"),
        )
        .unwrap();
        // The last boots' links are #2602's, not the setup's: kept.
        std::fs::write(links.join("lamp-kept-last-system.b.journal"), "").unwrap();
        (kept, links)
    }

    fn names(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn emptied() -> (Value, bool) {
        (json!({"ok": true, "forgotten": true}), true)
    }

    /// The PM's privacy call on #5342 (#5186's rule): the owner's own
    /// forget everything, once their memory is emptied, removes the setup's
    /// kept journal and its links, leaves the keep its mark, and says so.
    #[test]
    fn the_owners_forget_everything_removes_the_setups_kept_journal() {
        let (kept, links) = lamp("owner");
        let owners = ["p-owner".to_owned()];
        let told = forget_if(&kept, &links, emptied(), Some("p-owner"), &owners);
        assert_eq!(told.0["setup_log"], "forgotten too");
        assert_eq!(told.0["everything"], Value::Null);
        assert_eq!(names(&kept), [".forgotten", ".started"]);
        assert_eq!(names(&links), ["lamp-kept-last-system.b.journal"]);
        // Again: nothing left to remove, nothing said.
        let told = forget_if(&kept, &links, emptied(), Some("p-owner"), &owners);
        assert_eq!(told.0["setup_log"], Value::Null);
        // No setup kept at all: nothing said, nothing made.
        let none = kept.join("none");
        let told = forget_if(&none, &links, emptied(), Some("p-owner"), &owners);
        assert_eq!(told.0["setup_log"], Value::Null);
        assert!(!none.exists());
    }

    /// Another person's forget, the room's, or one that didn't empty the
    /// owner's memory, leaves it.
    #[test]
    fn only_the_owners_emptied_forget_removes_it() {
        let (kept, links) = lamp("others");
        let owners = ["p-owner".to_owned()];
        for (told, admitted) in [
            (emptied(), Some("p-guest")),
            (emptied(), None),
            (
                (json!({"ok": true, "forgotten": false}), true),
                Some("p-owner"),
            ),
        ] {
            let told = forget_if(&kept, &links, told, admitted, &owners);
            assert_eq!(told.0["setup_log"], Value::Null);
        }
        assert_eq!(
            names(&kept),
            [".started", ".system@a-2.journal", "system@a-1.journal"]
        );
        assert_eq!(names(&links).len(), 2);
    }

    /// A copy that can't be removed: the answer says it is kept, and that
    /// not everything was forgotten.
    #[test]
    fn a_copy_that_cant_be_removed_is_said() {
        let (kept, links) = lamp("stuck");
        // A folder named as a copy: remove_file refuses it.
        std::fs::create_dir_all(kept.join("stuck.journal")).unwrap();
        let owners = ["p-owner".to_owned()];
        let told = forget_if(&kept, &links, emptied(), Some("p-owner"), &owners);
        assert_eq!(told.0["everything"], false);
        assert_eq!(told.0["setup_log"], KEPT_SAID);
    }
}
