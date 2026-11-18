//! The skills the person made, told by voice (lamp-9adf, 15 Nov, build
//! 13162: "which skills have you made for me?" called nothing and was
//! answered from the last few turns, two of 26; heard as "skills have you
//! made for me?" it called a read-back, which said one of them "doesn't
//! send or write anything", since `make_skill` named no list).
//!
//! - **One plain path, the model's to choose.** `make_skill` takes
//!   [`LIST`], `true`, given alone: whether a line asks which skills they
//!   made is the model's judgment, never a list of words (the owner's
//!   ruling, 13 Nov). A parameter of the function the voice has, as a
//!   remove is (`crate::made_remove`): the voice is at its 48.
//! - **Only the person's own, and on.** The made skills' index
//!   (`made.json`), those marked on whose folder is still on the lamp
//!   ([`made_skill::drop_gone`] first), made here (by voice or in the app)
//!   apart from those added from a file. Newest first.
//! - **Names to the owner.** How many always; the newest names, at most
//!   [`NAMED_MOST`] of each, only when the owner hears
//!   (`services::owner_hears`), as a read-back's and a remove's are: a
//!   name can carry a person's.
use crate::made_skill::{self, Keeping};
use serde_json::{Value, json};

/// The `make_skill` argument that asks which skills the person made:
/// `true`, given alone.
pub const LIST: &str = "made";

/// What the model is told of a list asked in a turn nobody spoke.
pub const NOT_ASKED_ALOUD: &str =
    "the skills they made are told only when the person asks out loud";

/// The most names said of each, newest first: a short answer, never all.
pub const NAMED_MOST: usize = 3;

/// The person's made skills on the lamp, newest first: those made here,
/// then those added from a file.
fn on(keeping: &Keeping) -> (Vec<String>, Vec<String>) {
    made_skill::drop_gone(keeping);
    let (mut made, mut added) = (Vec::new(), Vec::new());
    for entry in made_skill::entries(&keeping.directory).iter().rev() {
        let Some(name) = entry["name"].as_str().filter(|_| entry["on"] == true) else {
            continue;
        };
        let kind = if entry["added"] == "file" {
            &mut added
        } else {
            &mut made
        };
        if !kind.iter().any(|kept| kept == name) {
            kind.push(name.to_owned());
        }
    }
    (made, added)
}

/// The skills the person made on the lamp, newest first, as [`listed`]
/// counts them: every count said of them comes from here (lamp-9adf, 17
/// Nov: the list said eighteen, a remove that found none said nineteen,
/// counting those off and added from a file too).
pub fn made_on(keeping: &Keeping) -> Vec<String> {
    on(keeping).0
}

/// `names`' newest [`NAMED_MOST`], as said.
fn said(names: &[String]) -> Vec<String> {
    names
        .iter()
        .take(NAMED_MOST)
        .map(|name| made_skill::spoken_name(name))
        .collect()
}

/// A list asked by the model's `make_skill` call, in the voice's request
/// `request` (`heard`: someone spoke it): how many the person made and,
/// to the owner alone (`owner_hears`, asked only when there is a name to
/// say), the newest names. What the model is told.
pub fn listed(
    keeping: &Keeping,
    request: u64,
    heard: bool,
    owner_hears: impl FnOnce() -> bool,
) -> Value {
    if !heard {
        eprintln!("lamp-voice: request {request}: made skills not listed: no one asked aloud");
        return json!({ "error": NOT_ASKED_ALOUD });
    }
    let (made, added) = on(keeping);
    // Counts only: the journal holds no skill's name.
    eprintln!(
        "lamp-voice: request {request}: made skills listed: {} made, {} added from a file",
        made.len(),
        added.len()
    );
    if made.is_empty() && added.is_empty() {
        return json!({
            "made_count": 0,
            "next": "Say plainly, in a few words, that they haven't made any skill yet. Call \
                     nothing."
        });
    }
    if !owner_hears() {
        return json!({
            "made_count": made.len(),
            "added_from_a_file_count": added.len(),
            "next": "Say plainly, in a few words, how many skills they made, and how many were \
                     added from a file, if any. Name no skill. Call nothing."
        });
    }
    json!({
        "made_count": made.len(),
        "newest_made": said(&made),
        "added_from_a_file_count": added.len(),
        "newest_added_from_a_file": said(&added),
        "next": "Say plainly, in a few words, how many skills they made and name the newest \
                 ones given; then, if any, how many were added from a file and their newest. \
                 Name no other skill. Call nothing."
    })
}

/// [`listed`] on the lamp, by `make_skill`'s call: the made skills' own
/// folder (`LAMP_MAKE_SKILL`), the owner's hearing as a read-back's
/// (`services::owner_hears`).
pub fn listed_here(
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    let Some(keeping) = Keeping::when(true) else {
        return crate::tools::unavailable();
    };
    listed(&keeping, request, heard, || {
        crate::services::owner_hears(request, heard, lease)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_skill_local::tests::keeping;

    /// `made` kept in `keeping`'s index, oldest first, by name, whether it
    /// is on and whether it was added from a file; an on one's folder is on
    /// the lamp as it went on.
    fn kept(keeping: &Keeping, made: &[(&str, bool, bool)]) {
        std::fs::create_dir_all(&keeping.directory).unwrap();
        let entries: Vec<Value> = made
            .iter()
            .enumerate()
            .map(|(at, (name, on, added))| {
                let sha256 = format!("{at:x}").repeat(64);
                if *on {
                    let folder = keeping.installed.join(name);
                    std::fs::create_dir_all(&folder).unwrap();
                    std::fs::write(
                        folder.join(".package.json"),
                        json!({"sha256": sha256}).to_string(),
                    )
                    .unwrap();
                }
                let mut entry = json!({"name": name, "sha256": sha256, "on": on});
                if *added {
                    entry["added"] = json!("file");
                }
                entry
            })
            .collect();
        std::fs::write(
            keeping.directory.join(made_skill::INDEX),
            serde_json::to_vec(&entries).unwrap(),
        )
        .unwrap();
    }

    /// lamp-9adf, 15 Nov: "which skills have you made for me?" is told
    /// from the made skills on the lamp, never from the thread: how many,
    /// the newest three named to the owner, those added from a file apart,
    /// one not on or taken off the lamp left out, and no name to anyone
    /// else.
    #[test]
    fn the_skills_they_made_are_told_from_the_lamp_newest_first() {
        let (root, keeping) = keeping("made-list");
        kept(
            &keeping,
            &[
                ("evening-dim", true, false),
                ("photo-booth", true, true),
                ("waits-for-yes", false, false),
                ("unread-mail-subjects", true, false),
                ("glow-green-on-return", true, false),
                ("study-coach", true, true),
                ("back-to-desk-email-lamp-test", true, false),
                ("taken-off", true, false),
            ],
        );
        // `lamp-skills remove taken-off`.
        std::fs::remove_dir_all(keeping.installed.join("taken-off")).unwrap();
        let told = listed(&keeping, 1, true, || true);
        assert_eq!(told["made_count"], 4, "{told}");
        assert_eq!(
            told["newest_made"],
            json!([
                "back to desk email lamp test",
                "glow green on return",
                "unread mail subjects"
            ]),
            "{told}"
        );
        assert_eq!(told["added_from_a_file_count"], 2, "{told}");
        assert_eq!(
            told["newest_added_from_a_file"],
            json!(["study coach", "photo booth"]),
            "{told}"
        );
        // Not the owner: how many alone.
        let told = listed(&keeping, 2, true, || false);
        assert_eq!(told["made_count"], 4, "{told}");
        assert!(told.get("newest_made").is_none(), "{told}");
        assert!(told["next"].as_str().unwrap().contains("Name no skill"));
        // Unheard: nothing told, the owner never asked of.
        let told = listed(&keeping, 3, false, || panic!("owner asked"));
        assert_eq!(told["error"], NOT_ASKED_ALOUD);
        // None made: said so, the owner never asked of.
        std::fs::remove_file(keeping.directory.join(made_skill::INDEX)).unwrap();
        let told = listed(&keeping, 4, true, || panic!("owner asked"));
        assert_eq!(told["made_count"], 0, "{told}");
        assert!(told["next"].as_str().unwrap().contains("haven't made any"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
