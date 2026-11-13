//! A heard line that plainly asks for an installed skill by its name calls
//! that skill's function in code, on the second route; the model only
//! words the answer (#6772, the lead's rule on #6071). Off unless
//! [`ENVIRONMENT`] is `on`. On lamp-52e6 at 12587 (Seventh's photo-booth
//! row on #6413) the pushed `photo-booth` skill was on and offered, one of 48 functions, and "photo booth" was
//! answered "I'm Lamp — I don't have a camera, so I can't take photos…"
//! with no call: the model's own idea of what the lamp can do stood over a
//! skill that was installed and offered. Asked by its name, a skill is no
//! longer the model's to decide on: it runs, as the heard words read a
//! skill back in code (#6619, [`crate::make_skill_local::read_back_heard`]).
//!
//! How it runs: round 1 is that call, not the model's, and goes the way a
//! model's round-1 call goes (`second_provider::answer`): the rule's hold,
//! the Policy's for a spoken turn (`LAMP_POLICY_HOLDS_ACTIONS`, which ends
//! the turn held and runs nothing), the pause's grace, the turn's runner, a
//! recording offered, and what the skill's answer asks for after it (a look,
//! a search walked on). The model words its result in round 2.
//!
//! Which skills: those the lamp's skills offer the voice
//! ([`crate::tools::skill_tools`], through `use_skill` as it offers them
//! when it is on), but never a core skill's: the lamp's own skills are
//! told whole to the model and asked by their own words, some with a
//! choice the model makes (which scene, which dance), and
//! `LAMP_SKILL_ASKED` forces them ([`crate::tools::skill_asked`]). A skill
//! installed from the Store, pushed from a computer or made at the lamp is
//! the one the model talks itself out of. Only a skill that takes no choice
//! and no yes-or-no: one that does ("white noise", rain or ocean) is the
//! model's, which gives it one.
//!
//! Which lines: the skill's function's name said as words in a row,
//! loosely (`photo_booth` as "photo booth" or "photobooth", a number as a
//! word or a digit, [`crate::made_skill::name_digits`]), and nothing else
//! but what is said around a plain ask: a greeting, the lamp's name, a
//! "please" ([`crate::tools::skill_asked`]'s `AROUND`, but never "wait",
//! which may be a person stopping: [`NOT_AROUND`]), a request before it
//! ("can you", "start", "I want", its `REQUESTS`), or a word that asks to
//! run it ([`RUNS`]: "run", "use", "open", "try", "skill"). Any other word
//! leaves the line to the model, as before: a question about the skill
//! ("what does photo booth use?", which #6619 reads back), a no ("don't
//! run photo booth"), something about it ("I loved the photo booth"), and
//! a line that names no skill ("is there a camera in here?"). A missed
//! line only means the model answers as it did; a wrong one runs a skill
//! nobody asked for. Two skills named, none.
//!
//! "Take a photo" names no skill: a skill.toml has no words of its own
//! for what asks for it, only its name, its description for the model and
//! its `when`. Until it has, such a line is the model's.
use crate::known_faces::lower_words;
use crate::tools::skill_asked::{AROUND, REQUESTS, phrase_at};
use serde_json::{Value, json};

/// Only the exact value `on`; off unless set. The second route alone.
pub const ENVIRONMENT: &str = "LAMP_SKILL_NAMED";

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The id round 1's call is given when the heard words named its skill:
/// the round's call loop knows it by it.
pub const CALL_ID: &str = "skill_named_by_words";

/// Of `AROUND`'s words, those that never go around a skill's name here:
/// "Photo booth, wait" and "wait, photo booth" may be a person stopping or
/// changing their mind, so such a line is the model's (Twelve's P2 4 on
/// #6782). A skill named "wait" is still never matched ([`plain_word`]).
pub const NOT_AROUND: &[&str] = &["wait"];

/// Words a line that names a skill may hold anywhere besides those
/// `AROUND` it: what asks to run a skill, and the skill itself ("run the
/// photo booth skill", "use your photo booth", "photo booth for me").
pub const RUNS: &[&[&str]] = &[
    &["run"],
    &["use"],
    &["open"],
    &["launch"],
    &["try"],
    &["skill"],
    &["your"],
    &["for", "me"],
    &["for", "us"],
];

/// The most words a skill's name is said in.
const NAME_WORDS: usize = 6;

/// A heard line longer than this is never a plain ask for a skill by its
/// name: no skills' file is read for it.
const MOST_WORDS: usize = 16;

/// A function's name as it is matched: number words as digits, and no
/// `_` or `-` between its words.
fn loose(name: &str) -> String {
    crate::made_skill::name_digits(&name.replace('_', "-")).replace('-', "")
}

/// Whether `name`, loosely, is a word or words a plain ask holds anyway
/// ([`AROUND`], [`REQUESTS`], [`RUNS`]): a skill named "hello" or "start"
/// is never run for a line that only greets or asks.
fn plain_word(name: &str) -> bool {
    static PLAIN: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
        [AROUND, REQUESTS, RUNS]
            .iter()
            .flat_map(|phrases| phrases.iter())
            .map(|phrase| loose(&phrase.join("-")))
            .collect()
    });
    PLAIN.iter().any(|plain| plain == name)
}

/// `functions` as [`named`] matches them: each loosely, with its own name,
/// none empty or a plain word.
fn wanted<'a>(functions: impl IntoIterator<Item = &'a str>) -> Vec<(String, &'a str)> {
    functions
        .into_iter()
        .map(|function| (loose(function), function))
        .filter(|(name, _)| !name.is_empty() && !plain_word(name))
        .collect()
}

/// The function among `functions` (skills' functions as their tools file
/// names them: `photo_booth`) that `heard` plainly asks for by its name,
/// if one ([`self`]): the name said as words in a row, the same name again
/// at most, requests only before it, and no word but those `AROUND` it or
/// [`RUNS`].
pub fn named<'a>(heard: &str, functions: &[&'a str]) -> Option<&'a str> {
    named_among(heard, &wanted(functions.iter().copied()))
}

/// [`named`] among functions already made [`wanted`].
fn named_among<'a>(heard: &str, wanted: &[(String, &'a str)]) -> Option<&'a str> {
    let words = lower_words(heard);
    let mut found: Option<&str> = None;
    let mut at = 0;
    while at < words.len() {
        // The longest name said from here.
        let name = (1..=NAME_WORDS.min(words.len() - at))
            .rev()
            .find_map(|length| {
                let said = loose(&words[at..at + length].join("-"));
                wanted
                    .iter()
                    .find(|(name, _)| *name == said)
                    .map(|(_, function)| (length, *function))
            });
        if let Some((length, function)) = name {
            if found.is_some_and(|before| before != function) {
                return None;
            }
            found = Some(function);
            at += length;
        } else if let Some(taken) = around_at(&words, at)
            .or_else(|| phrase_at(&words, at, RUNS))
            .or_else(|| {
                found
                    .is_none()
                    .then(|| phrase_at(&words, at, REQUESTS))
                    .flatten()
            })
        {
            at += taken;
        } else {
            return None;
        }
    }
    found
}

/// How many words from `at` an `AROUND` phrase takes, never one of
/// [`NOT_AROUND`].
fn around_at(words: &[String], at: usize) -> Option<usize> {
    if NOT_AROUND.contains(&words[at].as_str()) {
        None
    } else {
        phrase_at(words, at, AROUND)
    }
}

/// The call that runs `function`: through `use_skill`, its `skill` that
/// function, when the voice reaches skills by it (`LAMP_USE_SKILL`), else
/// the function itself, with no choice or yes-or-no: what the skill does
/// when it is asked for by name alone.
pub fn call(use_skill: bool, function: &str) -> (String, Value) {
    if use_skill {
        (
            crate::use_skill::NAME.to_owned(),
            json!({ "skill": function }),
        )
    } else {
        (function.to_owned(), json!({}))
    }
}

/// The skill's function a call (`name`, `args`) runs: `use_skill`'s
/// `skill`, or its own name.
pub fn function_of<'a>(name: &'a str, args: &'a Value) -> &'a str {
    if name == crate::use_skill::NAME {
        args["skill"].as_str().unwrap_or_default()
    } else {
        name
    }
}

/// Whether a model's call (`name`, `args`) runs the skill the heard words
/// ran already (`ran`, a call as [`call`] makes it), however its name is
/// written.
pub fn same_skill_again(name: &str, args: &Value, ran: &(String, Value)) -> bool {
    let asked = loose(function_of(&ran.0, &ran.1));
    !asked.is_empty() && loose(function_of(name, args)) == asked
}

/// The call a heard line asks for by a skill's name ([`named`]), among the
/// skills `settings` offers the voice now that take no choice and no
/// yes-or-no, never a core skill's: what the second route makes its first
/// round. The skills' files are read only for a line of at most
/// [`MOST_WORDS`] words.
pub fn heard(settings: &crate::tools::Settings, heard: &str) -> Option<(String, Value)> {
    if !short(heard) {
        return None;
    }
    let functions = functions(settings);
    let functions: Vec<&str> = functions.iter().map(String::as_str).collect();
    named(heard, &functions).map(|function| call(settings.use_skill, function))
}

/// Whether `heard` is a line short enough to be a plain ask by a name.
fn short(heard: &str) -> bool {
    let count = lower_words(heard).len();
    count > 0 && count <= MOST_WORDS
}

/// The functions [`heard`] matches a line against, read from the files.
fn functions(settings: &crate::tools::Settings) -> Vec<String> {
    let skills = crate::tools::skill_tools(settings);
    let reached = if settings.use_skill {
        crate::use_skill::offered(&skills)
    } else {
        &skills[..]
    };
    let core = crate::tools::core_skills(settings);
    reached
        .iter()
        .filter(|tool| crate::use_skill::takes_nothing(tool))
        .map(|tool| tool.name.clone())
        .filter(|name| !core.contains(name))
        .collect()
}

/// A file's time and length as last changed, or none for no file.
type Stamp = Option<(std::time::SystemTime, u64)>;

/// The files' stamps, and the functions read then, each loosely and with
/// its own name ([`wanted`]).
type Kept = (Vec<Stamp>, Vec<(String, String)>);

/// [`heard`], with the functions it matches kept while the files they are
/// read from are unchanged ([`Heard::stamps`]): reading and parsing the
/// tools files and making their names loose on every short line cost
/// 0.65 ms (3.9 ms in a debug build) for 45 skills on a Mac (Twelve's P3 1
/// on #6782); kept, a line looks only at the files' times: 7 µs (34 µs).
pub struct Heard {
    settings: crate::tools::Settings,
    kept: std::sync::Mutex<Option<Kept>>,
}

impl Heard {
    pub fn new(settings: crate::tools::Settings) -> Self {
        Self {
            settings,
            kept: std::sync::Mutex::new(None),
        }
    }

    /// [`heard`] for these settings.
    pub fn heard(&self, heard: &str) -> Option<(String, Value)> {
        if !short(heard) {
            return None;
        }
        let stamps = self.stamps();
        let mut kept = self
            .kept
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if kept.as_ref().is_none_or(|(at, _)| *at != stamps) {
            let read = functions(&self.settings);
            let wanted = wanted(read.iter().map(String::as_str))
                .into_iter()
                .map(|(name, function)| (name, function.to_owned()))
                .collect();
            *kept = Some((stamps, wanted));
        }
        let wanted: Vec<(String, &str)> = kept
            .as_ref()
            .map(|(_, wanted)| {
                wanted
                    .iter()
                    .map(|(name, function)| (name.clone(), function.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        named_among(heard, &wanted).map(|function| call(self.settings.use_skill, function))
    }

    /// What the functions are read from: each skills folder's tools file
    /// and call socket (whose presence says which folder is read,
    /// `tools::skills_folders`), and the executive's tools file, whose
    /// names a skill's never takes.
    fn stamps(&self) -> Vec<Stamp> {
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        for folder in crate::tools::every_skills_folder(&self.settings) {
            files.push(folder.join(crate::tools::CATALOG_FILE));
            files.push(folder.join(crate::tools::CALL_SOCKET));
        }
        if let Some(exec) = &self.settings.exec {
            files.push(exec.join(crate::tools::CATALOG_FILE));
        }
        files
            .iter()
            .map(|file| {
                let meta = std::fs::metadata(file).ok()?;
                Some((meta.modified().ok()?, meta.len()))
            })
            .collect()
    }
}

/// The note the turn is asked with when the heard words named the skill
/// `spoken` (its function's name as words): round 1 calls it, and the
/// round after words its answer, whatever it is (the Reviewer-lead's P3 1
/// on #6782: a run that failed is not said as running).
pub fn ran_note(spoken: &str) -> String {
    format!(
        "(For you to know: they asked for {spoken} by its name, one of your skills, so it is called \
         for them in this turn. Then say what its answer tells you, in a few words, as yourself; if \
         it was not done or failed, say so plainly. Never say you don't have what it uses: the lamp \
         has a camera, a light and a body. Do not call it again.)"
    )
}

/// What a model's call of the skill the heard words ran already is told
/// ([`same_skill_again`]): not run twice, and the first run's result.
pub fn ran_already(told: &Value) -> Value {
    json!({
        "error": "Not run again: they asked for this skill by its name, and it was run for them already.",
        "ran": told,
        "next": "Say only what its result above tells you to say. Call nothing."
    })
}

/// A function's name as it is said: `photo_booth` as "photo booth".
pub fn spoken(function: &str) -> String {
    function.replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON_THE_LAMP: [&str; 4] = ["photo_booth", "desk_check", "skill_2", "hello"];

    /// Seventh's line on lamp-52e6 at 12587 and the plain ways to ask for a
    /// skill by its name name it; a question, a no, a remark and a line
    /// with no skill's name are the model's.
    #[test]
    fn a_plain_ask_by_a_skills_name_names_it() {
        for (heard, function) in [
            ("photo booth", "photo_booth"),
            ("Photo booth.", "photo_booth"),
            ("Lamp, photo booth please.", "photo_booth"),
            ("Hey Lamp, run photo booth.", "photo_booth"),
            ("Can you do the photo booth?", "photo_booth"),
            ("Use your photo booth skill.", "photo_booth"),
            ("Start the photobooth for me", "photo_booth"),
            ("I want the photo booth, thanks.", "photo_booth"),
            ("photo booth, photo booth!", "photo_booth"),
            ("run skill two", "skill_2"),
            ("Desk check, please.", "desk_check"),
        ] {
            assert_eq!(named(heard, &ON_THE_LAMP), Some(function), "{heard}");
        }
        for heard in [
            "is there a camera in here?",
            "Do you have a camera?",
            "what does photo booth use?",
            "is photo booth on?",
            "don't run photo booth",
            "turn off photo booth",
            "remove the photo booth skill",
            "I loved the photo booth yesterday.",
            "photo booth and desk check",
            "Photo booth, wait.",
            "wait, photo booth",
            "Wait. Photo booth.",
            "photo",
            "booth",
            "hello",
            "wait",
            "Hello lamp.",
            "please",
            "",
        ] {
            assert_eq!(named(heard, &ON_THE_LAMP), None, "{heard}");
        }
    }

    /// Through `use_skill` when the voice reaches skills by it, else the
    /// function itself; a model's call of it after is the same skill,
    /// however written.
    #[test]
    fn the_call_and_the_same_skill_again() {
        let through = call(true, "photo_booth");
        assert_eq!(
            through,
            (
                crate::use_skill::NAME.to_owned(),
                json!({"skill": "photo_booth"})
            )
        );
        let own = call(false, "photo_booth");
        assert_eq!(own, ("photo_booth".to_owned(), json!({})));
        for ran in [&through, &own] {
            assert!(same_skill_again(
                crate::use_skill::NAME,
                &json!({"skill": "photo_booth"}),
                ran
            ));
            assert!(same_skill_again(
                crate::use_skill::NAME,
                &json!({"skill": "photo-booth"}),
                ran
            ));
            assert!(same_skill_again("photo_booth", &json!({}), ran));
            assert!(!same_skill_again(
                crate::use_skill::NAME,
                &json!({"skill": "dance"}),
                ran
            ));
            assert!(!same_skill_again("take_a_look", &json!({}), ran));
            assert!(!same_skill_again(crate::use_skill::NAME, &json!({}), ran));
        }
    }

    /// A folder of the test's own, with `tools` as its tools file.
    fn skills_folder(name: &str, tools: Value) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let base = if std::path::Path::new("/private/tmp").is_dir() {
            std::path::PathBuf::from("/private/tmp")
        } else {
            std::env::temp_dir()
        };
        let dir = base.join(format!(
            "lvs-named-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(crate::tools::CATALOG_FILE), tools.to_string()).unwrap();
        dir
    }

    /// The skills on a lamp: two that take nothing, one with a choice, one
    /// with a yes-or-no, and a core skill.
    fn on_a_lamp() -> Value {
        json!({"tools":[
            {"name":"photo_booth","description":"Take a photo."},
            {"name":"desk_check","description":"Check the desk."},
            {"name":"white_noise","description":"Play a sound.",
             "parameters":{"type":"object","properties":{
                "sound":{"type":"string","enum":["rain","ocean"],"description":"Which."}}}},
            {"name":"focus_timer","description":"A focus timer.",
             "parameters":{"type":"object","properties":{
                "long":{"type":"boolean","description":"A long one."}}}},
            {"name":"light_scene","description":"A light scene."}],
          "core":["light_scene"]})
    }

    /// Twelve's P2 1 on #6782: a skill that takes a choice or a yes-or-no
    /// is never run by its name with none ("white noise", rain or ocean):
    /// the model gives it one. A core skill is the model's too. With
    /// `use_skill` and without it.
    #[test]
    fn only_a_skill_that_takes_nothing_is_run_by_its_name() {
        let dir = skills_folder("takes", on_a_lamp());
        for use_skill in [true, false] {
            let settings = crate::tools::Settings {
                skills: Some(dir.clone()),
                use_skill,
                ..Default::default()
            };
            assert_eq!(
                heard(&settings, "photo booth"),
                Some(call(use_skill, "photo_booth"))
            );
            assert_eq!(
                heard(&settings, "Desk check, please."),
                Some(call(use_skill, "desk_check"))
            );
            for line in [
                "white noise",
                "play white noise",
                "focus timer",
                "light scene",
            ] {
                assert_eq!(heard(&settings, line), None, "{line}");
            }
        }
        std::fs::remove_dir_all(dir).ok();
    }

    /// Twelve's P3 1 on #6782: what reading the skills' files costs a short
    /// line, as `heard` reads them on every spoken line of at most 16 words.
    /// Printed with `--nocapture`; the bound only catches a reading gone
    /// far wrong on a slow runner.
    #[test]
    fn what_reading_the_skills_costs_a_short_line() {
        let mut tools = on_a_lamp();
        // A lamp with 40 skills of its own beside them.
        for at in 0..40 {
            tools["tools"].as_array_mut().unwrap().push(json!({
                "name": format!("store_skill_{at}"),
                "description": "A skill from the Store, with a description of about the length a skill's has. "
                    .repeat(3),
            }));
        }
        let dir = skills_folder("cost", tools);
        let settings = crate::tools::Settings {
            skills: Some(dir.clone()),
            use_skill: true,
            ..Default::default()
        };
        const LINES: u32 = 200;
        let began = std::time::Instant::now();
        for _ in 0..LINES {
            assert!(heard(&settings, "Lamp, photo booth please.").is_some());
        }
        let each = began.elapsed() / LINES;
        let kept = Heard::new(settings);
        let began = std::time::Instant::now();
        for _ in 0..LINES {
            assert!(kept.heard("Lamp, photo booth please.").is_some());
        }
        let each_kept = began.elapsed() / LINES;
        eprintln!(
            "skill_named over 45 skills: {each:?} a line read each time, {each_kept:?} kept by the files' times"
        );
        assert!(each < std::time::Duration::from_millis(50), "{each:?}");
        assert!(
            each_kept < std::time::Duration::from_millis(50),
            "{each_kept:?}"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// The functions kept are read again once the tools file changes: a
    /// skill installed is matched on the next line, one removed is not.
    #[test]
    fn the_functions_kept_follow_the_tools_file() {
        let dir = skills_folder("kept", on_a_lamp());
        let kept = Heard::new(crate::tools::Settings {
            skills: Some(dir.clone()),
            use_skill: true,
            ..Default::default()
        });
        assert!(kept.heard("photo booth").is_some());
        assert_eq!(kept.heard("dance party"), None);
        std::fs::write(
            dir.join(crate::tools::CATALOG_FILE),
            json!({"tools":[{"name":"dance_party","description":"Dance, with lights, for a while."}]})
                .to_string(),
        )
        .unwrap();
        assert_eq!(kept.heard("dance party"), Some(call(true, "dance_party")));
        assert_eq!(kept.heard("photo booth"), None);
        std::fs::remove_dir_all(dir).ok();
    }
}
