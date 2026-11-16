//! A skill added from a file in the owner's app (`LAMP_SKILL_UPLOAD`, off
//! by default; the first builder on the Queue, 3 Nov 14:33, with Backend3's
//! rules). `lamp-cloud` fetches the file the app uploaded, checks its
//! SHA-256, refuses a Store-signed package and one with code, and hands
//! its two files here over the app-make socket ([`crate::app_make`]):
//! `{"upload": {"skill_md", "skill_toml"?, "request_id"?}}`, `skill_toml`
//! absent for a bare `SKILL.md`.
//!
//! The lamp holds it to a made skill's own rules, and more:
//! - **Never as the Store's.** It is marked `made: here` and
//!   [`made_skill::ADDED_FROM_FILE`], and said as "added from a file, not
//!   from the Skills Store" ([`on_line`]).
//! - **A name the lamp holds is refused, never replaced**: a skill on the
//!   lamp by that name (core, from the Store, pushed or made), or one kept
//!   here, made or added ([`taken`]). The manage helper holds it again at
//!   the install.
//! - **It never starts on its own**: no trigger, no `[ask]`; it runs only
//!   when the person asks, through its function
//!   ([`made_skill::capped_added`]). Otherwise a made skill's cap: the
//!   light, the head and the two topics, and no other permission.
//! - **A bare `SKILL.md`** becomes a written skill ([`written`]): a
//!   trigger-less skill whose function's note is the file's text, the
//!   model's instruction, with nothing but `speak`.
//! - **The same checks as a made skill**: [`made_skill::plainly`] says what
//!   it does, `lamp-skills pack` checks it, and it is kept beside the made
//!   skills, at most five made or added ([`made_skill::keep_added`]).
//! - **On at once, with no spoken yes** (#5977; the owner, 10 Nov: a made skill
//!   goes on with no spoken yes; the app's ask is the ask): once kept it
//!   is put on through [`made_skill::put_on`] and the manage helper, held
//!   again to the added cap there, as a skill typed in the app
//!   ([`crate::app_make`]), and the lamp says [`on_line`], or
//!   [`not_on_line`] with why it isn't on.
//!
//! The files' words are never in the journal.
use crate::made_skill::{self, Files, Keeping, Made, Refused};
use serde_json::{Value, json};
use std::path::Path;

/// The switch, in the coordinator's environment and `lamp-cloud`'s. `on`
/// only.
pub const ENVIRONMENT: &str = "LAMP_SKILL_UPLOAD";
/// What an added skill is kept as made from: this, and when it was taken.
pub const FROM: &str = "upload:";
/// `lamp-skills`' folder, core and installable skills side by side.
pub const SKILLS_FOLDER: &str = "/var/lib/lampOS/skills";
/// Skills pushed from a computer (`lamp_skills::PUSHED_DIR`).
pub const PUSHED_FOLDER: &str = "/var/lib/lampOS/skills-dev";
/// The most of a written skill's note: the file's text, the model's
/// instruction. The say service refuses longer words at every call, so a
/// longer note would be put on and never work (Thirteen's P2 on #5080).
pub const MOST_NOTE_BYTES: usize = crate::say::MOST_WORDS_BYTES;
/// What a refusal says with the switch off.
pub const OFF: &str = "not on this lamp: a skill added from a file";
/// What a refusal says without the say service, so nothing could be said
/// aloud of it.
pub const NO_ASK: &str = "the lamp can't ask about it aloud now";

/// Whether the switch is on in this process's environment.
pub fn from_environment() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// A bare `SKILL.md` as a written skill: a trigger-less skill started only
/// by its function, named for the skill (`-` as `_`), whose one step is
/// the function's note, the text after the front matter on one line. It
/// may speak (the note is the model's own answer) and nothing else.
pub fn written(card: &str) -> Result<Files, Refused> {
    written_with(
        card,
        &crate::upload_agent::asks_of(&crate::upload_agent::triggers(card)),
    )
}

/// [`written`], with its function's `asks` given (`LAMP_MADE_SKILL_ASKS`:
/// its trigger phrases as [`crate::upload_agent::asks_checked`] keeps
/// them, so `LAMP_SKILL_NAMED` calls it in code when one is heard); none,
/// and its `skill.toml` is as before, byte for byte.
pub fn written_with(card: &str, asks: &[String]) -> Result<Files, Refused> {
    let unread = || Refused(made_skill_md_unread(), None);
    let name = made_skill::card_name(card).ok_or_else(unread)?;
    let tool = name.replace('-', "_");
    if !tool.starts_with(|c: char| c.is_ascii_lowercase()) || tool.len() > 48 {
        return Err(Refused(
            "its name can't be a function's: it begins with a letter and is at most 48 long".into(),
            None,
        ));
    }
    let body = card
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---"))
        .map(|(_, body)| body.trim_start_matches('-'))
        .ok_or_else(unread)?;
    let note: String = body
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if note.is_empty() {
        return Err(Refused(
            "its SKILL.md says nothing after its name and description".into(),
            None,
        ));
    }
    if note.len() > MOST_NOTE_BYTES {
        return Err(Refused(
            format!("its SKILL.md's text is longer than {MOST_NOTE_BYTES} bytes"),
            None,
        ));
    }
    let toml = format!(
        "kind = \"installable\"\nversion = \"1.0.0\"\nneeds = [\"voice\"]\ntool = \"{tool}\"\n{}\n\
         [permissions]\nspeak = true\n\n[[do]]\nsay = {}\n",
        crate::upload_agent::asks_line(asks),
        toml_string(&note)
    );
    Ok(Files {
        toml,
        card: card.to_owned(),
        left_out: None,
        scripts: Vec::new(),
    })
}

fn made_skill_md_unread() -> String {
    "its SKILL.md has no name and description".into()
}

/// `text` as a TOML basic string.
fn toml_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// Why `name` can't be added: a skill on the lamp by that name, in
/// `skills` or `pushed`, or one kept in `made` (made or added). None when
/// it is free.
pub fn taken(name: &str, skills: &Path, pushed: &Path, made: &Path) -> Option<String> {
    let on_the_lamp = |folder: &Path| std::fs::symlink_metadata(folder.join(name)).is_ok();
    (on_the_lamp(skills)
        || on_the_lamp(pushed)
        || made_skill::kept_names(made).iter().any(|kept| kept == name))
    .then(|| {
        format!(
            "a skill called {name} is already on this lamp, and one added from a file never takes its place"
        )
    })
}

/// Where an added skill is held to the lamp's names, and kept.
#[derive(Clone, Debug)]
pub struct Place {
    pub skills: std::path::PathBuf,
    pub pushed: std::path::PathBuf,
}

impl Place {
    /// The product's folders.
    pub fn product() -> Self {
        Self {
            skills: SKILLS_FOLDER.into(),
            pushed: PUSHED_FOLDER.into(),
        }
    }
}

/// The two files taken, checked and kept from `upload:<at_unix_ms>`: a
/// package's `skill.toml` and `SKILL.md`, or a bare `SKILL.md`'s written
/// skill. Its name must be free ([`taken`]).
pub fn take(
    place: &Place,
    keeping: &Keeping,
    skill_toml: Option<&str>,
    skill_md: &str,
    at_unix_ms: u64,
) -> Result<Made, Refused> {
    take_telling(place, keeping, skill_toml, skill_md, at_unix_ms).map(|(made, _)| made)
}

/// [`take`], and a bare `SKILL.md`'s trigger phrases left out of its
/// function's `asks` with why (`LAMP_MADE_SKILL_ASKS`,
/// [`crate::upload_agent::asks_checked`]: not plain words the lamp can
/// start it by, or words that would start another skill on the lamp,
/// [`made_skill::ask_taken`]), for the app; the rest still goes on.
pub fn take_telling(
    place: &Place,
    keeping: &Keeping,
    skill_toml: Option<&str>,
    skill_md: &str,
    at_unix_ms: u64,
) -> Result<(Made, Vec<crate::upload_agent::LeftOut>), Refused> {
    // A file saved with Windows line endings reads as any other.
    let skill_md = &skill_md.replace("\r\n", "\n");
    let skill_toml = skill_toml.map(|toml| toml.replace("\r\n", "\n"));
    let skill_toml = skill_toml.as_deref();
    let name =
        made_skill::card_name(skill_md).ok_or_else(|| Refused(made_skill_md_unread(), None))?;
    if !(1..=64).contains(&name.len())
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(Refused("its name isn't a skill's name".into(), None));
    }
    // A made or added skill removed since (by the app or `lamp-skills
    // remove`) is dropped from the kept ones first, as a voice make's name
    // is (`made_skill::free_name`): on lamp-9adf, 15 Nov 16:14, a file
    // added again 3 minutes after the app removed its skill was refused as
    // "already on this lamp" until the record was dropped at 16:23.
    made_skill::drop_gone(keeping);
    if let Some(why) = taken(&name, &place.skills, &place.pushed, &keeping.directory) {
        return Err(Refused(why, None));
    }
    // An agent skill (`LAMP_UPLOAD_TO_AGENT`): the file whole, run by the
    // lamp's agent ([`crate::upload_agent`]).
    // Its trigger phrases as its function's asks, each checked as a made
    // skill's are, none another skill's (`LAMP_MADE_SKILL_ASKS`).
    let (asks, left_out) =
        crate::upload_agent::asks_checked(&crate::upload_agent::triggers(skill_md), |ask| {
            made_skill::ask_taken(&place.skills, &keeping.directory, &name, ask)
        });
    if !left_out.is_empty() {
        eprintln!(
            "lamp-voice: a skill added from a file: {} of its trigger phrases left out of its asks",
            left_out.len()
        );
    }
    if skill_toml.is_none() && crate::upload_agent::on() {
        let (files, _) = crate::upload_agent::files_with(skill_md, &asks)?;
        return made_skill::keep_agent(&files, &format!("{FROM}{at_unix_ms}"), keeping)
            .map(|made| (made, left_out));
    }
    let (files, left_out) = match skill_toml {
        Some(toml) => (
            Files {
                toml: toml.to_owned(),
                card: skill_md.to_owned(),
                left_out: None,
                scripts: Vec::new(),
            },
            Vec::new(),
        ),
        None => (written_with(skill_md, &asks)?, left_out),
    };
    made_skill::keep_added(&files, &format!("{FROM}{at_unix_ms}"), keeping)
        .map(|made| (made, left_out))
}

/// What the lamp says aloud once a skill added from a file is on: its
/// name, that it was added from a file and not from the Skills Store, and
/// that it's on, then what it does and uses, as a
/// made skill's line says (Thirteen's P3 on #5985: no yes is asked).
pub fn on_line(made: &Made) -> String {
    let made_line = made_skill::on_line(made);
    let said = made_skill::spoken_name(&made.name);
    let rest = made_line
        .strip_prefix(&format!("I made {said}, and it's on"))
        .unwrap_or(".");
    format!("I added {said} from a file, not from the Skills Store, and it's on{rest}")
}

/// A kept skill added from a file put on at once ([`made_skill::put_on`]),
/// and what the lamp says then: [`on_line`], or for an agent skill
/// [`crate::upload_agent::went_on`] (its task's yes recorded at once when
/// its grants allow), or [`not_on_line`] with why it isn't on.
pub fn put_on_said(keeping: &Keeping, made: &Made) -> String {
    match made_skill::put_on(keeping, made) {
        Ok(()) => match made_skill::agent_grants(&keeping.directory, &made.sha256) {
            Some(grants) => crate::upload_agent::went_on(made, &grants, |name| {
                crate::upload_agent::record_by(
                    &keeping.manage,
                    Path::new(crate::skill_task_yes::CONTROL_SOCKET),
                    name,
                )
            }),
            None => on_line(made),
        },
        Err(why) => not_on_line(&why),
    }
}

/// What the lamp says aloud when a skill added from a file couldn't be put
/// on: why, in the plain words [`made_skill::put_on`] gives.
pub fn not_on_line(why: &str) -> String {
    format!("I added a skill from a file but couldn't put it on: {why}.")
}

/// What became of the skill added from a file `sha256`, kept in
/// `directory`: `kept` (kept, and being put on), `on`, `left_off` (kept
/// and not put on: its `not_on` says why; or asked about before 10 Nov and
/// never said yes to) or `none` (not kept, or not one added from a file).
/// `lamp-cloud` asks it until the skill is on or left off, so the backend
/// is told only then (`skills.upload.<upload_id>`'s second reply).
pub fn state(directory: &Path, sha256: &str) -> &'static str {
    let entries = made_skill::entries(directory);
    let Some(entry) = entries
        .iter()
        .find(|entry| entry["sha256"] == sha256 && entry["added"] == "file")
    else {
        return "none";
    };
    if entry["on"] == true {
        return "on";
    }
    if entry.get("not_on").is_some() || entry["offered_at_unix_ms"].as_u64().is_some() {
        return "left_off";
    }
    "kept"
}

/// One line on the app-make socket, if it is an upload's (`upload`) or
/// asks what became of one (`{"added": {"sha256"}}`, [`state`]): the answer, and
/// once kept, the skill to put on at once. `on` is the switch; `can_ask`,
/// whether the say service is there to say it aloud. None for a line that
/// is not an upload's.
pub fn line(
    asked: &Value,
    on: bool,
    can_ask: bool,
    place: &Place,
    keeping: &Keeping,
    now_unix_ms: u64,
) -> Option<(Value, Option<Made>)> {
    let asked = asked.as_object().filter(|asked| asked.len() == 1)?;
    if let Some(added) = asked.get("added") {
        if !on {
            return Some((json!({"refused": OFF}), None));
        }
        let sha256 = added["sha256"].as_str().unwrap_or_default();
        return Some((json!({"state": state(&keeping.directory, sha256)}), None));
    }
    let upload = asked.get("upload")?;
    if !on {
        return Some((json!({"refused": OFF}), None));
    }
    if !can_ask {
        return Some((json!({"refused": NO_ASK}), None));
    }
    let Some(skill_md) = upload["skill_md"].as_str() else {
        return Some((json!({"refused": "an upload names its SKILL.md"}), None));
    };
    let skill_toml = match upload.get("skill_toml") {
        None | Some(Value::Null) => None,
        Some(Value::String(toml)) => Some(toml.as_str()),
        Some(_) => return Some((json!({"refused": "its skill.toml is text"}), None)),
    };
    Some(
        match take_telling(place, keeping, skill_toml, skill_md, now_unix_ms) {
            Ok((made, left_out)) => {
                eprintln!("lamp-voice: a skill added from a file: kept; put on now");
                let mut kept = json!({
                    "name": made.name,
                    "description": made.description,
                    "version": made.version,
                    "sha256": made.sha256,
                    "uses": {"light": made.uses.light, "body": made.uses.body, "topics": made.uses.topics},
                    // An agent skill's grants, said back in the app.
                    "grants": made_skill::agent_grants(&keeping.directory, &made.sha256),
                });
                // The keys' names it needs, for the app to ask for
                // (`LAMP_SKILL_SECRETS`); never a value.
                if let Some(secrets) = made_skill::agent_secrets(&keeping.directory, &made.sha256) {
                    kept["secrets"] = json!(secrets);
                }
                // Its trigger phrases left out of its asks, and why
                // (`LAMP_MADE_SKILL_ASKS`): absent with none, as before.
                if !left_out.is_empty() {
                    kept["asks_left_out"] = json!(
                        left_out
                            .iter()
                            .map(|(phrase, why)| json!({"phrase": phrase, "why": why}))
                            .collect::<Vec<_>>()
                    );
                }
                (json!({ "kept": kept }), Some(made))
            }
            Err(Refused(why, _)) => {
                eprintln!("lamp-voice: a skill added from a file was not kept");
                (json!({"refused": why}), None)
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_skill_local::tests::keeping;

    const CARD: &str = "---\nname: tea-tip\ndescription: Call when someone asks for a tea tip.\n---\n\n# Tea tip\n\nGive one short tip about brewing green tea. SECRET-WORDS-IN-THE-FILE\n";

    fn place(root: &Path) -> Place {
        let place = Place {
            skills: root.join("skills"),
            pushed: root.join("pushed"),
        };
        std::fs::create_dir_all(&place.skills).unwrap();
        std::fs::create_dir_all(&place.pushed).unwrap();
        place
    }

    const NOW: u64 = 1_800_000_000_000;

    /// A bare SKILL.md becomes a written skill: no trigger, its function
    /// named for it, its text the note, nothing but `speak`; it passes the
    /// added cap and is said plainly as asked for only.
    #[test]
    fn a_bare_skill_md_is_taken_as_a_trigger_less_written_skill() {
        let files = written(CARD).unwrap();
        let table: toml::Table = toml::from_str(&files.toml).unwrap();
        assert!(table.get("trigger").is_none(), "{}", files.toml);
        assert_eq!(table["tool"].as_str(), Some("tea_tip"));
        assert_eq!(table["permissions"]["speak"].as_bool(), Some(true));
        let note = table["do"][0]["say"].as_str().unwrap();
        assert!(note.starts_with("# Tea tip Give one short tip"), "{note}");
        let (version, uses) = made_skill::capped_added(&files.toml).unwrap();
        assert_eq!(version, "1.0.0");
        assert_eq!(uses, made_skill::Uses::default());
        let said = made_skill::plainly(&files.toml).unwrap();
        assert!(said.starts_with("Only when you ask for it"), "{said}");
        assert!(said.contains("never on its own"), "{said}");
        // Kept, marked added, and told as not the Store's.
        let (root, keeping) = keeping("upload-bare");
        let made = take(&place(&root), &keeping, None, CARD, NOW).unwrap();
        assert_eq!(made.name, "tea-tip");
        let entry = &made_skill::entries(&keeping.directory)[0];
        assert_eq!(entry["added"], "file");
        assert_eq!(entry["from"], format!("upload:{NOW}"));
        let line = on_line(&made);
        assert_eq!(
            line,
            "I added tea tip from a file, not from the Skills Store, and it's on: only when you ask for it, it answers in its own words, as its file tells it, and never on its own."
        );
        // Its words are the lamp's plain ones, never the file's.
        assert!(!line.contains("SECRET-WORDS"), "{line}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A note the say service would refuse at every call is refused here,
    /// and a file with Windows line endings reads as any other (Thirteen's
    /// P2 and P3 on #5080).
    #[test]
    fn a_note_past_the_say_bound_is_refused_and_crlf_is_read() {
        let body = "a".repeat(crate::say::MOST_WORDS_BYTES + 1);
        let long = format!("---\nname: tea-tip\ndescription: A tip.\n---\n{body}\n");
        let why = written(&long).unwrap_err().0;
        assert!(why.contains("longer than 900 bytes"), "{why}");
        let fits = format!(
            "---\nname: tea-tip\ndescription: A tip.\n---\n{}\n",
            "a".repeat(crate::say::MOST_WORDS_BYTES)
        );
        assert!(written(&fits).is_ok());
        let (root, keeping) = keeping("upload-crlf");
        let crlf = CARD.replace('\n', "\r\n");
        let made = take(&place(&root), &keeping, None, &crlf, NOW).unwrap();
        assert_eq!(made.name, "tea-tip");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A package's skill that would start on its own is refused: a
    /// trigger, an `[ask]`, a `[[while]]`; so is one with no function to be
    /// asked for, and one past a made skill's cap.
    #[test]
    fn a_trigger_no_function_or_more_than_a_made_skill_may_use_is_refused() {
        let (root, keeping) = keeping("upload-trigger");
        let place = place(&root);
        let card = "---\nname: glow-back\ndescription: Glows.\n---\n";
        let refused = |toml: &str| take(&place, &keeping, Some(toml), card, NOW).unwrap_err().0;
        let triggered = "kind = \"installable\"\nversion = \"1.0.0\"\ntool = \"glow_back\"\n\
                         [permissions]\nlight = true\ntopics = [\"room/presence\"]\n\
                         [trigger]\ntopic = \"room/presence\"\nfield = \"someone\"\nequals = true\n\
                         [[do]]\nlight = { colour = \"cyan\" }\n";
        assert!(refused(triggered).contains("starts on its own"));
        let no_tool = "kind = \"installable\"\nversion = \"1.0.0\"\n[permissions]\nlight = true\n\
                       [[do]]\nlight = { colour = \"cyan\" }\n";
        assert!(refused(no_tool).contains("no tool"));
        let wider = "kind = \"installable\"\nversion = \"1.0.0\"\ntool = \"glow_back\"\n\
                     [permissions]\nlight = true\nmessages = true\n[[do]]\nlight = { colour = \"cyan\" }\n";
        // A skill added from a file keeps the cap from before #6066 (App
        // Only's P2 on #6103): the lift is for a skill made on the lamp.
        assert!(refused(wider).contains("more than the light and the head"));
        assert!(made_skill::entries(&keeping.directory).is_empty());
        // Within the cap, asked for only: kept, its light said plainly.
        let good = "kind = \"installable\"\nversion = \"1.0.0\"\ntool = \"glow_back\"\n\
                    [permissions]\nlight = true\n[[do]]\nlight = { colour = \"cyan\", for = \"20s\" }\n";
        let made = take(&place, &keeping, Some(good), card, NOW).unwrap();
        assert_eq!(
            made.description,
            "Only when you ask for it, it glows cyan for 20 seconds, and never on its own."
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A name the lamp holds is refused, never replaced: on the lamp (core,
    /// Store or made), pushed, or kept here, made or added.
    #[test]
    fn a_name_the_lamp_holds_is_refused() {
        let (root, keeping) = keeping("upload-taken");
        let place = place(&root);
        std::fs::create_dir_all(place.skills.join("tea-tip")).unwrap();
        let why = take(&place, &keeping, None, CARD, NOW).unwrap_err().0;
        assert!(why.contains("already on this lamp"), "{why}");
        std::fs::remove_dir_all(place.skills.join("tea-tip")).unwrap();
        std::fs::create_dir_all(place.pushed.join("tea-tip")).unwrap();
        assert!(take(&place, &keeping, None, CARD, NOW).is_err());
        std::fs::remove_dir_all(place.pushed.join("tea-tip")).unwrap();
        take(&place, &keeping, None, CARD, NOW).unwrap();
        // Kept here once: a second upload by that name is refused too.
        let why = take(&place, &keeping, None, CARD, NOW + 1).unwrap_err().0;
        assert!(why.contains("already on this lamp"), "{why}");
        assert_eq!(made_skill::entries(&keeping.directory).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// One added, put on and then removed from the lamp is free to add
    /// again at once: its record is dropped before the name is checked
    /// (lamp-9adf, 15 Nov 16:14). While its folder is on the lamp, it is
    /// still refused.
    #[test]
    fn one_removed_since_can_be_added_again() {
        use crate::make_skill_local::tests::manage_helper;
        let (root, keeping) = keeping("upload-again");
        let place = place(&root);
        let made = take(&place, &keeping, None, CARD, NOW).unwrap();
        let asked = manage_helper(&keeping.manage);
        made_skill::put_on(&keeping, &made).unwrap();
        asked.join().unwrap();
        let folder = keeping.installed.join("tea-tip");
        std::fs::create_dir_all(&folder).unwrap();
        let why = take(&place, &keeping, None, CARD, NOW + 1).unwrap_err().0;
        assert!(why.contains("already on this lamp"), "{why}");
        // Removed: the folder gone, the same file is taken again.
        std::fs::remove_dir_all(&folder).unwrap();
        let again = take(&place, &keeping, None, CARD, NOW + 2).unwrap();
        assert_eq!(again.name, "tea-tip");
        assert_eq!(made_skill::entries(&keeping.directory).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Kept, it goes on at once with no yes (the owner, 10 Nov): the
    /// manage helper is asked for its SHA-256, and it reads `on`. With no
    /// helper it stays off, reads `left_off`, and why is said.
    #[test]
    fn it_goes_on_at_once_with_no_yes_or_stays_off_with_why() {
        use crate::make_skill_local::tests::manage_helper;
        let (root, keeping) = keeping("upload-on");
        let made = take(&place(&root), &keeping, None, CARD, NOW).unwrap();
        let directory = &keeping.directory;
        assert_eq!(state(directory, &made.sha256), "kept");
        let why = made_skill::put_on(&keeping, &made).unwrap_err();
        assert_eq!(state(directory, &made.sha256), "left_off");
        assert_eq!(
            not_on_line(&why),
            "I added a skill from a file but couldn't put it on: the lamp's skills couldn't be \
             reached."
        );
        let asked = manage_helper(&keeping.manage);
        made_skill::put_on(&keeping, &made).unwrap();
        assert_eq!(asked.join().unwrap()["install_made"], made.sha256.as_str());
        assert_eq!(state(directory, &made.sha256), "on");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The socket's line: refused with the switch off or no say service;
    /// kept, given back to be put on; anything not an upload's is not taken
    /// here.
    #[test]
    fn the_sockets_upload_line_is_switched_and_answered() {
        let (root, keeping) = keeping("upload-line");
        let place = place(&root);
        let asked = json!({"upload": {"skill_md": CARD, "request_id": "r1"}});
        let (told, ask) = line(&asked, false, true, &place, &keeping, NOW).unwrap();
        assert_eq!((told, ask), (json!({"refused": OFF}), None));
        let (told, ask) = line(&asked, true, false, &place, &keeping, NOW).unwrap();
        assert_eq!((told, ask), (json!({"refused": NO_ASK}), None));
        assert!(made_skill::entries(&keeping.directory).is_empty());
        let (told, ask) = line(&asked, true, true, &place, &keeping, NOW).unwrap();
        assert_eq!(told["kept"]["name"], "tea-tip", "{told}");
        assert!(!told.to_string().contains("SECRET-WORDS"), "{told}");
        let made = ask.unwrap();
        let sha = made.sha256.clone();
        assert_eq!(told["kept"]["sha256"], sha);
        assert!(line(&json!({"made": {}}), true, true, &place, &keeping, NOW).is_none());
        // What became of it, as lamp-cloud asks until it is on or left off.
        let added = json!({"added": {"sha256": sha.clone()}});
        let state_of = |now| line(&added, true, true, &place, &keeping, now).unwrap().0;
        assert_eq!(state_of(NOW), json!({"state": "kept"}));
        let asked = crate::make_skill_local::tests::manage_helper(&keeping.manage);
        made_skill::put_on(&keeping, &made).unwrap();
        asked.join().unwrap();
        assert_eq!(state_of(NOW + 2_000), json!({"state": "on"}));
        let missing = json!({"added": {"sha256": "0".repeat(64)}});
        assert_eq!(
            line(&missing, true, true, &place, &keeping, NOW).unwrap().0,
            json!({"state": "none"})
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The first builder's answer 3 on #7249 (`LAMP_MADE_SKILL_ASKS`, with
    /// `LAMP_UPLOAD_TO_AGENT` off): a bare `SKILL.md`'s trigger phrases
    /// are its written function's `asks`, each through the same check as
    /// a made skill's; one refused (a question, one word of its own, or
    /// words that would start another skill on the lamp) is left out and
    /// the app is told which and why, while the rest still goes in and the
    /// skill is kept. Off, its `skill.toml` is byte for byte as before and
    /// the app is told nothing more.
    #[test]
    fn a_bare_files_trigger_phrases_are_its_asks_and_the_app_is_told_those_left_out() {
        crate::upload_agent::set_on_here(Some(false));
        let card = "---\nname: tea-tip\ndescription: A tea tip.\n---\n\n# Tea tip\n\n\
                    Trigger phrases: \"Give me a tea tip\", \"tea please\", \
                    \"what tea is best?\", \"take a photo\"\n\nGive one short tip.\n";
        made_skill::set_asks_here(None);
        let unset = written(card).unwrap();
        made_skill::set_asks_here(Some(false));
        assert_eq!(written(card).unwrap().toml, unset.toml, "off, as before");
        assert!(!unset.toml.contains("asks"), "{}", unset.toml);
        let (root, kept_off) = keeping("upload-asks-off");
        let asked = json!({"upload": {"skill_md": card}});
        let (told, _) = line(&asked, true, true, &place(&root), &kept_off, NOW).unwrap();
        assert!(told["kept"].get("asks_left_out").is_none(), "{told}");
        let _ = std::fs::remove_dir_all(&root);
        made_skill::set_asks_here(Some(true));
        let (root, keeping) = keeping("upload-asks-on");
        let place = place(&root);
        let booth = place.skills.join("photo-booth");
        std::fs::create_dir_all(&booth).unwrap();
        std::fs::write(
            booth.join("skill.toml"),
            "tool = \"photo_booth\"\nasks = [\"take a photo\"]\n",
        )
        .unwrap();
        let (told, made) = line(&asked, true, true, &place, &keeping, NOW).unwrap();
        assert!(made.is_some(), "{told}");
        let left_out = told["kept"]["asks_left_out"].as_array().unwrap();
        let phrases: Vec<&str> = left_out
            .iter()
            .map(|left| left["phrase"].as_str().unwrap())
            .collect();
        assert_eq!(
            phrases,
            ["tea please", "what tea is best?", "take a photo"],
            "{told}"
        );
        assert!(
            left_out[2]["why"]
                .as_str()
                .unwrap()
                .contains("and another skill, photo booth, so neither would start"),
            "{told}"
        );
        let files = written_with(card, &["give me a tea tip".to_owned()]).unwrap();
        assert!(
            files.toml.contains("asks = [\"give me a tea tip\"]\n"),
            "{}",
            files.toml
        );
        assert!(made_skill::capped_added(&files.toml).is_ok());
        made_skill::set_asks_here(Some(false));
        assert!(
            made_skill::capped_added(&files.toml).is_err(),
            "off, an added skill gives no asks"
        );
        made_skill::set_asks_here(None);
        crate::upload_agent::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
