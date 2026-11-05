//! The journal the image keeps on the eMMC (`lamp-journal-keep`,
//! `docs/install.md`): a first-time setup's (#5144, "A first-time setup's
//! journal") and the last two boots' (#2602). Their lines can hold what the
//! person said, so the owner's own "forget everything about me" deletes them
//! all, on the same mark and rule as their place and their voice print (the
//! PM's privacy calls on #5342 and #5358). They also go by themselves 72
//! hours on, and at a reset.
//!
//! The voice runs as root, as the keep does. The copies go, and so do their
//! links where journalctl reads. Two marks are left for the keep: `.forgotten`
//! in the setup's folder, so nothing more of that walk is copied, and
//! `.forgotten-boot` with this boot's id, so the shutdown of this boot keeps
//! nothing of it.
use serde_json::{Value, json};
use std::{io, path::Path};

/// Where the keep puts its copies.
pub const KEPT: &str = "/var/lib/lamp-journal";
/// Where the keep links them for journalctl.
pub const LINKS: &str = "/run/log/journal";
/// This boot's id, as the keep reads it.
pub const BOOT_ID: &str = "/proc/sys/kernel/random/boot_id";
/// The setup's folder in [`KEPT`], and its mark.
const SETUP: &str = "setup";
const FORGOTTEN: &str = ".forgotten";
/// The boots' folders in [`KEPT`]: kept, and the keep's own in-between ones.
const BOOTS: [&str; 4] = ["last", "previous", ".old", ".new"];
/// The mark for the keep at this boot's shutdown.
const FORGOTTEN_BOOT: &str = ".forgotten-boot";
/// The keep's links' names begin so: the setup's and the boots'.
const LINK_PREFIXES: [&str; 3] = ["lamp-kept-setup-", "lamp-kept-last-", "lamp-kept-previous-"];
/// What the answer says when the copies could not all be removed.
const KEPT_SAID: &str = "kept: it could not be removed";

/// The owner's "forget everything about me", marked by the second route's
/// guard: the kept journal goes too, and the answer says so. Another
/// person's forget, the room's, an unmarked one, or one whose memory was not
/// emptied, leaves it.
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
        Path::new(BOOT_ID),
        told,
        lease.admitted_person(),
        &crate::person::owner_ids(),
    )
}

/// [`forget_with_everything`] once its mark is judged: the copies under
/// `kept` and their links in `links` removed when `admitted`, the line's
/// person, is one of `owners` and `told` says their memory was emptied.
/// `boot_id` is where this boot's id is read.
pub(crate) fn forget_if(
    kept: &Path,
    links: &Path,
    boot_id: &Path,
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
    match wipe(kept, links, boot_id) {
        Ok(true) => {
            eprintln!("lamp-voice: the kept journal was removed with everything about them");
            told.0["kept_logs"] = json!("forgotten too");
        }
        Ok(false) => {}
        Err(error) => {
            eprintln!(
                "lamp-voice: the kept journal could not be removed with everything about them: {}",
                error.kind()
            );
            told.0["everything"] = json!(false);
            told.0["kept_logs"] = json!(KEPT_SAID);
        }
    }
    told
}

/// Removes every copy under `kept` and their links in `links`, leaving the
/// keep's marks. Whether any was there; no folder is nothing to forget.
fn wipe(kept: &Path, links: &Path, boot_id: &Path) -> io::Result<bool> {
    // The marks first, so a keep running now copies nothing after them.
    // This boot's shutdown keeps nothing of it, even with nothing kept yet:
    // the keep makes the folder at shutdown (Twelve's P3 on #5365). No id:
    // nothing to mark, and the boot's lines rotate out of the RAM disk.
    if let Ok(id) = std::fs::read_to_string(boot_id) {
        std::fs::create_dir_all(kept)?;
        std::fs::write(kept.join(FORGOTTEN_BOOT), id.trim())?;
    }
    let setup = kept.join(SETUP);
    if setup.is_dir() {
        std::fs::write(setup.join(FORGOTTEN), b"")?;
    }
    let mut any = remove_journals(&setup)?;
    for slot in BOOTS {
        let folder = kept.join(slot);
        if folder.is_dir() {
            remove_journals(&folder)?;
            std::fs::remove_dir_all(&folder)?;
            any = true;
        }
    }
    if let Ok(entries) = std::fs::read_dir(links) {
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if LINK_PREFIXES.iter().any(|prefix| name.starts_with(prefix)) {
                std::fs::remove_file(entry.path())?;
                any = true;
            }
        }
    }
    Ok(any)
}

/// Removes the journal files in `folder` (a copy being written, dot-named,
/// too). Whether any was there.
fn remove_journals(folder: &Path) -> io::Result<bool> {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let mut any = false;
    for entry in entries {
        let entry = entry?;
        if entry.file_name().to_string_lossy().ends_with(".journal") {
            std::fs::remove_file(entry.path())?;
            any = true;
        }
    }
    Ok(any)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A lamp's kept journal: the setup's copies, the last two boots', and
    /// every one linked where journalctl reads, plus a link that isn't the
    /// keep's.
    struct Lamp {
        kept: PathBuf,
        links: PathBuf,
        boot_id: PathBuf,
    }

    fn lamp(name: &str) -> Lamp {
        let root = std::env::temp_dir().join(format!(
            "lamp-voice-kept-journal-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let lamp = Lamp {
            kept: root.join("lamp-journal"),
            links: root.join("run"),
            boot_id: root.join("boot_id"),
        };
        std::fs::create_dir_all(&lamp.links).unwrap();
        std::fs::write(&lamp.boot_id, "0123-boot\n").unwrap();
        for (slot, names) in [
            ("setup", &["system@a-1.journal", ".system@a-2.journal"][..]),
            ("last", &["system@b-1.journal"][..]),
            (
                "previous",
                &["system@c-1.journal", "system@c-2.journal"][..],
            ),
        ] {
            let folder = lamp.kept.join(slot);
            std::fs::create_dir_all(&folder).unwrap();
            for name in names {
                std::fs::write(folder.join(name), "j").unwrap();
                if !name.starts_with('.') {
                    std::os::unix::fs::symlink(
                        folder.join(name),
                        lamp.links
                            .join(format!("lamp-kept-{slot}-{}", name.replace('@', "."))),
                    )
                    .unwrap();
                }
            }
        }
        std::fs::write(lamp.kept.join("setup/.started"), "").unwrap();
        // journald's own folder in /run is never the keep's.
        std::fs::create_dir_all(lamp.links.join("0123456789abcdef")).unwrap();
        lamp
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

    fn forget(lamp: &Lamp, told: (Value, bool), admitted: Option<&str>) -> (Value, bool) {
        forget_if(
            &lamp.kept,
            &lamp.links,
            &lamp.boot_id,
            told,
            admitted,
            &["p-owner".to_owned()],
        )
    }

    /// The PM's privacy calls on #5342 and #5358 (#5186's rule): the owner's
    /// own forget everything, once their memory is emptied, removes the
    /// setup's kept journal and the last two boots', with their links;
    /// leaves the keep its marks (this walk forgotten, this boot's shutdown
    /// to keep nothing); and says so.
    #[test]
    fn the_owners_forget_everything_removes_every_kept_journal() {
        let lamp = lamp("owner");
        let told = forget(&lamp, emptied(), Some("p-owner"));
        assert_eq!(told.0["kept_logs"], "forgotten too");
        assert_eq!(told.0["everything"], Value::Null);
        assert_eq!(names(&lamp.kept), [".forgotten-boot", "setup"]);
        assert_eq!(
            std::fs::read_to_string(lamp.kept.join(".forgotten-boot")).unwrap(),
            "0123-boot"
        );
        assert_eq!(names(&lamp.kept.join("setup")), [".forgotten", ".started"]);
        assert_eq!(names(&lamp.links), ["0123456789abcdef"]);
        // Again: nothing left to remove, nothing said.
        let told = forget(&lamp, emptied(), Some("p-owner"));
        assert_eq!(told.0["kept_logs"], Value::Null);
    }

    /// Only the boots kept (no setup's, as on a lamp set up before #5144):
    /// they go, and no setup folder is made.
    #[test]
    fn the_boots_alone_are_forgotten_too() {
        let lamp = lamp("boots");
        std::fs::remove_dir_all(lamp.kept.join("setup")).unwrap();
        let told = forget(&lamp, emptied(), Some("p-owner"));
        assert_eq!(told.0["kept_logs"], "forgotten too");
        assert_eq!(names(&lamp.kept), [".forgotten-boot"]);
        // Nothing kept at all: nothing said, but this boot is still marked,
        // since its shutdown would make the folder and keep it.
        let none = lamp.kept.join("none");
        let told = forget_if(
            &none,
            &lamp.links,
            &lamp.boot_id,
            emptied(),
            Some("p-owner"),
            &["p-owner".to_owned()],
        );
        assert_eq!(told.0["kept_logs"], Value::Null);
        assert_eq!(names(&none), [".forgotten-boot"]);
    }

    /// Another person's forget, the room's, or one that didn't empty the
    /// owner's memory, leaves all of it.
    #[test]
    fn only_the_owners_emptied_forget_removes_it() {
        let lamp = lamp("others");
        for (told, admitted) in [
            (emptied(), Some("p-guest")),
            (emptied(), None),
            (
                (json!({"ok": true, "forgotten": false}), true),
                Some("p-owner"),
            ),
        ] {
            let told = forget(&lamp, told, admitted);
            assert_eq!(told.0["kept_logs"], Value::Null);
        }
        assert_eq!(names(&lamp.kept), ["last", "previous", "setup"]);
        assert_eq!(names(&lamp.links).len(), 5);
    }

    /// A copy that can't be removed: the answer says it is kept, and that
    /// not everything was forgotten.
    #[test]
    fn a_copy_that_cant_be_removed_is_said() {
        let lamp = lamp("stuck");
        // A folder named as a copy: remove_file refuses it.
        std::fs::create_dir_all(lamp.kept.join("setup/stuck.journal")).unwrap();
        let told = forget(&lamp, emptied(), Some("p-owner"));
        assert_eq!(told.0["everything"], false);
        assert_eq!(told.0["kept_logs"], KEPT_SAID);
    }
}
