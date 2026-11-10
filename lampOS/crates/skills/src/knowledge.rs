//! A tool's product knowledge (#5790, the core skill `faq`): the lamp tells a
//! person how to use it, from one file in the skill's own folder,
//! `knowledge = "knowledge.md"` in its `skill.toml`. The file is split into
//! sections, one for each word of the skill's `[choice]`, under a heading
//! `## WORD`; a call is told its note, then the section its choice names,
//! then, for each skill the section names in backticks (`` `guard-mode` ``),
//! whether that skill is on this lamp now. Nothing in it is anyone's: the
//! same words for everyone at the lamp.
//!
//! What a person reads and the model never does: everything above the first
//! section (a title, how the file is kept), and each line that is wholly an
//! HTML comment (`<!-- ... -->`), where a fact names the switch it needs.
use std::{fs, path::Path};

/// The most the file may be.
pub const FILE_MOST_BYTES: u64 = 32_768;
/// The most one section may be, as told.
pub const SECTION_MOST_BYTES: usize = 2_400;
/// The most one call's answer may be as JSON (`{"next": ...}`, a look's
/// words included), the skills' line at its longest included: under the
/// link's bound for an answer (`lamp_gemini::MAX_TOOL_JSON_BYTES`, 4,096,
/// less the 16 `lamp_voice::tools::sendable` keeps), with room for the
/// note `use_skill` adds (`lamp_voice::use_skill::ran`, about 210 bytes
/// for a skill name of the most, 64).
pub const TOLD_MOST_BYTES: usize = 3_700;

/// The file as loaded: each section's words, in the file's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Knowledge {
    sections: Vec<(String, Section)>,
}

/// One section: what the model is told of it, and the skills it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub text: String,
    /// Each skill named in backticks, once, in the order named.
    pub skills: Vec<String>,
}

/// Whether a skill a section names is on this lamp now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Here {
    /// Installed, enabled, and not turned off or snoozed.
    On,
    /// Installed and enabled, but turned off or snoozed (in the app or by
    /// voice).
    Off,
    /// Not installed, or installed disabled (`enabled = false`, as the
    /// product's install leaves one of `PRODUCT_SKILLS_OFF`): the engine
    /// holds enabled skills only.
    Missing,
}

impl Here {
    fn words(self) -> &'static str {
        match self {
            Here::On => "is on",
            Here::Off => "is installed but off",
            Here::Missing => "is not on this lamp",
        }
    }
}

impl Knowledge {
    /// The section for `topic`, a word of the skill's choice.
    pub fn section(&self, topic: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|(name, _)| name == topic)
            .map(|(_, section)| section)
    }

    /// Every section, by its topic, in the file's order.
    pub fn sections(&self) -> impl Iterator<Item = (&str, &Section)> {
        self.sections
            .iter()
            .map(|(name, section)| (name.as_str(), section))
    }

    /// Why a call with `note` could be told more than [`TOLD_MOST_BYTES`],
    /// if it could: each section with every skill it names not on this
    /// lamp, its longest line, in the answer as [`crate::told`] makes it,
    /// with a look's words when the skill `looks`. Nothing else is added to
    /// a knowledge skill's answer before `use_skill`'s note: a `[history]`
    /// beside it is refused at load.
    pub fn too_long(&self, note: &str, looks: bool) -> Option<String> {
        self.sections.iter().find_map(|(topic, section)| {
            let mut calls = vec![crate::Call::Say(told(note, section, |_| Here::Off))];
            if looks {
                calls.push(crate::Call::Look);
            }
            let bytes = crate::told(&calls).to_string().len();
            (bytes > TOLD_MOST_BYTES).then(|| {
                format!(
                    "knowledge section {topic} with the note is {bytes} bytes as told, over {TOLD_MOST_BYTES}"
                )
            })
        })
    }
}

/// The file `name` in `dir`, for a choice of `topics`: a plain file of at
/// most [`FILE_MOST_BYTES`], never a link, read as [`parse`] reads it.
pub fn read(dir: &Path, name: &str, topics: &[String]) -> Result<Knowledge, String> {
    let stem = name.strip_suffix(".md").unwrap_or_default();
    if stem.is_empty()
        || !stem
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(format!(
            "knowledge {name:?} is a file name in the skill's folder: lowercase letters, digits and hyphens, then .md"
        ));
    }
    let path = dir.join(name);
    let meta = fs::symlink_metadata(&path).map_err(|error| format!("{name}: {error}"))?;
    if !meta.is_file() {
        return Err(format!("{name} is a plain file, not a link or a folder"));
    }
    if meta.len() > FILE_MOST_BYTES {
        return Err(format!("{name} is over {FILE_MOST_BYTES} bytes"));
    }
    let text = fs::read_to_string(&path).map_err(|error| format!("{name}: {error}"))?;
    parse(&text, topics).map_err(|why| format!("{name}: {why}"))
}

/// The sections of `text`: one `## TOPIC` heading for each of `topics` and
/// no other, each with 1 to [`SECTION_MOST_BYTES`] of words told, and no
/// other heading inside one.
pub fn parse(text: &str, topics: &[String]) -> Result<Knowledge, String> {
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    for line in text.lines() {
        if let Some(topic) = line.strip_prefix("## ") {
            let topic = topic.trim();
            if !topics.iter().any(|one| one == topic) {
                return Err(format!(
                    "section {topic:?} is not one of the choice's words {topics:?}"
                ));
            }
            if sections.iter().any(|(name, _)| name == topic) {
                return Err(format!("section {topic:?} is there twice"));
            }
            sections.push((topic.to_owned(), Vec::new()));
            continue;
        }
        let Some((topic, lines)) = sections.last_mut() else {
            // Above the first section: for people, never told.
            continue;
        };
        let trimmed = line.trim();
        if trimmed.starts_with("<!--") && trimmed.ends_with("-->") {
            continue;
        }
        if trimmed.starts_with('#') {
            return Err(format!(
                "section {topic:?} has a heading of its own, {trimmed:?}: a section is words"
            ));
        }
        if line.contains("<!--") || line.contains("-->") {
            return Err(format!(
                "section {topic:?}: a comment is a whole line of its own"
            ));
        }
        if line.chars().any(char::is_control) {
            return Err(format!("section {topic:?} has a control character"));
        }
        lines.push(line.trim_end());
    }
    if let Some(missing) = topics
        .iter()
        .find(|topic| !sections.iter().any(|(name, _)| name == *topic))
    {
        return Err(format!("no section for the choice's word {missing:?}"));
    }
    let sections = sections
        .into_iter()
        .map(|(topic, lines)| {
            let text = lines.join("\n").trim().to_owned();
            if text.is_empty() || text.len() > SECTION_MOST_BYTES {
                return Err(format!(
                    "section {topic:?} is 1 to {SECTION_MOST_BYTES} bytes of words, not {}",
                    text.len()
                ));
            }
            let skills = named_skills(&text);
            Ok((topic, Section { text, skills }))
        })
        .collect::<Result<_, String>>()?;
    Ok(Knowledge { sections })
}

/// Each skill a section names: a word in backticks of lowercase letters,
/// digits and hyphens (a skill's folder name), once each.
fn named_skills(text: &str) -> Vec<String> {
    let mut skills: Vec<String> = Vec::new();
    for (index, piece) in text.split('`').enumerate() {
        let quoted = index % 2 == 1;
        let skill_name = (1..=64).contains(&piece.len())
            && piece
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && piece.as_bytes()[0].is_ascii_lowercase();
        if quoted && skill_name && !skills.iter().any(|named| named == piece) {
            skills.push(piece.to_owned());
        }
    }
    skills
}

/// What the model is told of a call: the note, the section, and whether
/// each skill it names is on this lamp now (`here`).
pub fn told(note: &str, section: &Section, here: impl Fn(&str) -> Here) -> String {
    let mut told = format!("{note}\n\n{}", section.text);
    if !section.skills.is_empty() {
        let states: Vec<String> = section
            .skills
            .iter()
            .map(|skill| format!("{skill} {}", here(skill).words()))
            .collect();
        told.push_str(&format!(
            "\n\nOn this lamp now, of the skills named above: {}.",
            states.join("; ")
        ));
    }
    told
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topics(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn a_section_is_told_without_its_title_or_comments() {
        let knowledge = parse(
            "# Title\n\nFor people.\n\n## lights\n<!-- LAMP_X, on in the product -->\nAsk for a `light-scenes` scene.\n\n## care\nSay stop the nudges.\n",
            &topics(&["lights", "care"]),
        )
        .unwrap();
        let lights = knowledge.section("lights").unwrap();
        assert_eq!(lights.text, "Ask for a `light-scenes` scene.");
        assert_eq!(lights.skills, ["light-scenes"]);
        assert_eq!(
            knowledge.section("care").unwrap().skills,
            Vec::<String>::new()
        );
        let told = told("Answer from this.", lights, |_| Here::Missing);
        assert_eq!(
            told,
            "Answer from this.\n\nAsk for a `light-scenes` scene.\n\nOn this lamp now, of the skills named above: light-scenes is not on this lamp."
        );
    }

    #[test]
    fn a_switch_in_backticks_is_no_skill() {
        assert_eq!(
            named_skills("`LAMP_USE_SKILL` and `guard-mode` and `guard-mode` and `x y`"),
            ["guard-mode"]
        );
    }

    #[test]
    fn every_word_has_one_section_and_no_other() {
        let words = topics(&["a", "b"]);
        assert!(parse("## a\nx\n", &words).unwrap_err().contains("\"b\""));
        assert!(
            parse("## a\nx\n## b\ny\n## c\nz\n", &words)
                .unwrap_err()
                .contains("\"c\"")
        );
        assert!(
            parse("## a\nx\n## a\nx\n## b\ny\n", &words)
                .unwrap_err()
                .contains("twice")
        );
        assert!(
            parse("## a\n\n## b\ny\n", &words)
                .unwrap_err()
                .contains("1 to")
        );
        assert!(
            parse("## a\n### more\n## b\ny\n", &words)
                .unwrap_err()
                .contains("heading")
        );
        assert!(
            parse("## a\nx <!-- y -->\n## b\ny\n", &words)
                .unwrap_err()
                .contains("comment")
        );
        let long = format!("## a\n{}\n## b\ny\n", "w".repeat(SECTION_MOST_BYTES + 1));
        assert!(parse(&long, &words).is_err());
    }

    #[test]
    fn a_note_and_a_section_too_long_to_tell_are_found() {
        let words = topics(&["a"]);
        let knowledge = parse(&format!("## a\n{}\n", "w".repeat(2_000)), &words).unwrap();
        assert!(knowledge.too_long("short", false).is_none());
        assert!(knowledge.too_long(&"n".repeat(2_000), false).is_some());
        // A look's words count too: just under the bound without them.
        let section = &knowledge.section("a").unwrap().text;
        let room = TOLD_MOST_BYTES - r#"{"next":"\n\n"}"#.len() - section.len();
        assert!(knowledge.too_long(&"n".repeat(room), false).is_none());
        assert!(knowledge.too_long(&"n".repeat(room), true).is_some());
    }
}
