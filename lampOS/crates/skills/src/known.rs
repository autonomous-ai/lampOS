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

/// Where Who was here's names are switched on (#1997, the lead's yes 20
/// Oct): beside [`ENVIRONMENT`], a visit by someone who said yes to being
/// named to others is told with their name. `on` only.
pub const NAMES_ENVIRONMENT: &str = "LAMP_WHO_WAS_HERE_NAMES";

/// Whether Who was here's names are switched on.
pub fn naming() -> bool {
    std::env::var(NAMES_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The eyes' file of names Who was here and guard mode may say, and how it
/// is read: one copy, in `lamp_msgs::known`, which `lamp-cloud` reads too
/// as it sends guard mode's named text (#2562).
pub use lamp_msgs::known::{tellable_in, tellable_of};

/// The name in the known file at `path`, if it is no older than `fresh`.
/// A memory id on the line after it (`LAMP_MEMORY_STABLE_ID=on` in the
/// eyes, #2176) is passed over: never kept here.
pub fn name_in(path: &Path, fresh: Duration) -> Option<String> {
    let age = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())?;
    if age > fresh {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let (name, _) = lamp_msgs::known::read_known(&text)?;
    (name.len() <= MOST_NAME_BYTES).then(|| name.to_owned())
}

/// The memory's id of the known person in the known file at `path`, if
/// it is no older than `fresh`: the eyes' id on its second line when it is
/// one a skill's ask may carry (`p-` and 16 hex digits), else
/// [`lamp_msgs::known::person_id`] of the name, as memory keys them. Never
/// the name: the Policy is told whom a hello is for, not who they are.
pub fn person_in(path: &Path, fresh: Duration) -> Option<String> {
    let age = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())?;
    if age > fresh {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let (name, id) = lamp_msgs::known::read_known(&text)?;
    Some(match id {
        Some(id) if askable(id) => id.to_owned(),
        _ => lamp_msgs::known::person_id(name),
    })
}

/// Whether `id` is one a skill's ask may carry, as `lamp_msgs::SkillAsk`'s
/// check wants it: `p-` and 16 lowercase hex digits, without the eyes' `-N`
/// for a shared one. Any other shape would be dropped by the Policy, and
/// with it the hello.
fn askable(id: &str) -> bool {
    id.len() == 18
        && id.starts_with("p-")
        && id[2..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
        // The eyes' memory id after the name (#2176): the name is read.
        std::fs::write(&known, "Alpha\np-be9688998f085098\n").unwrap();
        assert_eq!(name_in(&known, fresh).as_deref(), Some("Alpha"));
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

    /// The first builder, 30 Oct: a hello is asked for whom it is, by the
    /// memory's id, never the name: the eyes' id when there is one an ask
    /// may carry, else the id of the name; nothing for nobody known.
    #[test]
    fn the_person_a_hello_is_for_is_the_memorys_id_never_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let known = dir.path().join("known");
        let fresh = Duration::from_secs(5);
        assert_eq!(person_in(&known, fresh), None, "no file: nobody known");
        std::fs::write(&known, "Alpha\n").unwrap();
        assert_eq!(
            person_in(&known, fresh),
            Some(lamp_msgs::known::person_id("Alpha"))
        );
        std::fs::write(&known, "Alpha\np-be9688998f085098\n").unwrap();
        assert_eq!(
            person_in(&known, fresh).as_deref(),
            Some("p-be9688998f085098")
        );
        // A shared id's `-2` is not an ask's shape: the name's id instead.
        std::fs::write(&known, "Alpha\np-be9688998f085098-2\n").unwrap();
        assert_eq!(
            person_in(&known, fresh),
            Some(lamp_msgs::known::person_id("Alpha"))
        );
        // Nor is any other 18 bytes the ask's check would drop (the
        // Reviewer-lead on #4625): the name's id instead, never no hello.
        for odd in [
            "p-BE9688998F085098",
            "q-be9688998f085098",
            "p-be9688998f08509z",
        ] {
            std::fs::write(&known, format!("Alpha\n{odd}\n")).unwrap();
            let person = person_in(&known, fresh);
            assert_ne!(person.as_deref(), Some(odd));
            assert!(person.is_none_or(|id| askable(&id)), "{odd}");
        }
        let old = SystemTime::now() - Duration::from_secs(10);
        std::fs::File::options()
            .write(true)
            .open(&known)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(person_in(&known, fresh), None, "stale: nobody known");
    }

    /// #1997: the names Who was here may say are the eyes' `tellable`
    /// beside the known file, one a line; none without the file, or with one
    /// larger than twenty names.
    #[test]
    fn the_tellable_file_gives_the_names_who_said_yes() {
        let dir = tempfile::tempdir().unwrap();
        let known = dir.path().join("known");
        let tellable = tellable_of(&known);
        assert_eq!(tellable, dir.path().join("tellable"));
        assert!(tellable_in(&tellable).is_empty(), "no file: nobody");
        std::fs::write(&tellable, "Alpha\nBeta\n\n").unwrap();
        assert_eq!(tellable_in(&tellable), ["Alpha", "Beta"]);
        std::fs::write(&tellable, "x".repeat(20 * 161 + 1)).unwrap();
        assert!(tellable_in(&tellable).is_empty(), "too large: nobody");
    }
}
