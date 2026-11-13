//! A heard line that plainly asks for an installed skill by its name calls
//! that skill's function in code, on the second route; the model only
//! words the answer (#6772, the lead's rule on #6071). On lamp-52e6 at
//! 12587 (Seventh's photo-booth row on #6413) the pushed `photo-booth`
//! skill was on and offered, one of 48 functions, and "photo booth" was
//! answered "I'm Lamp — I don't have a camera, so I can't take photos…"
//! with no call: the model's own idea of what the lamp can do stood over a
//! skill that was installed and offered. Asked by its name, a skill is no
//! longer the model's to decide on: it runs, as the heard words read a
//! skill back in code (#6619, [`crate::make_skill_local::read_back_heard`]).
//!
//! Which skills: those the lamp's skills offer the voice
//! ([`crate::tools::skill_tools`], through `use_skill` as it offers them
//! when it is on), but never a core skill's: the lamp's own skills are
//! told whole to the model and asked by their own words, some with a
//! choice the model makes (which scene, which dance), and
//! `LAMP_SKILL_ASKED` forces them ([`crate::tools::skill_asked`]). A skill
//! installed from the Store, pushed from a computer or made at the lamp is
//! the one the model talks itself out of.
//!
//! Which lines: the skill's function's name said as words in a row,
//! loosely (`photo_booth` as "photo booth" or "photobooth", a number as a
//! word or a digit, [`crate::made_skill::name_digits`]), and nothing else
//! but what is said around a plain ask: a greeting, the lamp's name, a
//! "please" ([`crate::tools::skill_asked`]'s `AROUND`), a request before it
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
    [AROUND, REQUESTS, RUNS]
        .iter()
        .flat_map(|phrases| phrases.iter())
        .any(|phrase| loose(&phrase.join("-")) == name)
}

/// The function among `functions` (skills' functions as their tools file
/// names them: `photo_booth`) that `heard` plainly asks for by its name,
/// if one ([`self`]): the name said as words in a row, the same name again
/// at most, requests only before it, and no word but those `AROUND` it or
/// [`RUNS`].
pub fn named<'a>(heard: &str, functions: &[&'a str]) -> Option<&'a str> {
    let words = lower_words(heard);
    let wanted: Vec<(String, &str)> = functions
        .iter()
        .map(|function| (loose(function), *function))
        .filter(|(name, _)| !name.is_empty() && !plain_word(name))
        .collect();
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
        } else if let Some(taken) = phrase_at(&words, at, AROUND)
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
/// skills `settings` offers the voice now, never a core skill's: what the
/// second route runs before round 1. The skills' files are read only for a
/// line of at most [`MOST_WORDS`] words.
pub fn heard(settings: &crate::tools::Settings, heard: &str) -> Option<(String, Value)> {
    let count = lower_words(heard).len();
    if count == 0 || count > MOST_WORDS {
        return None;
    }
    let skills = crate::tools::skill_tools(settings);
    let reached = if settings.use_skill {
        crate::use_skill::offered(&skills)
    } else {
        &skills[..]
    };
    let core = crate::tools::core_skills(settings);
    let functions: Vec<&str> = reached
        .iter()
        .map(|tool| tool.name.as_str())
        .filter(|name| !core.contains(*name))
        .collect();
    named(heard, &functions).map(|function| call(settings.use_skill, function))
}

/// The note round 1 gets once the heard words ran the skill `spoken` (its
/// function's name as words), with its result `told`.
pub fn ran_note(spoken: &str, told: &Value) -> String {
    format!(
        "(For you to know: they asked for {spoken} by its name, one of your skills, so it was run \
         for them now, with this result: {told}. Do as its \"next\" says, in a few words, as \
         yourself. Never say you can't do it or don't have what it uses: it is running on you. Do \
         not call it again.)"
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
            "photo",
            "booth",
            "hello",
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
}
