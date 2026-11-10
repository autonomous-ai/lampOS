//! The core FAQ skill (#5790): the lamp tells a person how to use it from
//! one product-knowledge file, `skills/faq/knowledge.md`. Held here: each
//! switch the file names is in `docs/switches.md`, each skill it names is a
//! folder of `skills/`; each call is told its section and whether the skills
//! it names are on this lamp, within what the link carries through
//! `use_skill`; and the eval set, `skills/faq/tests/evals.toml`, maps each
//! owner-style question to a topic whose section holds its answer.
use lamp_skills::{Engine, load, told};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn faq() -> lamp_skills::Skill {
    load(&root().join("skills/faq")).unwrap()
}

fn knowledge_text() -> String {
    std::fs::read_to_string(root().join("skills/faq/knowledge.md")).unwrap()
}

/// What the model is told of `lamp_help` with `args`, through `use_skill`,
/// on a lamp with `skills` loaded beside it and `off` turned off.
fn told_of(args: Value, beside: &[&str], off: &[&str]) -> Value {
    let mut skills = vec![faq()];
    skills.extend(
        beside
            .iter()
            .map(|name| load(&root().join("skills").join(name)).unwrap()),
    );
    let mut engine = Engine::new(skills, true);
    if !off.is_empty() {
        let state = tempfile::tempdir().unwrap();
        std::fs::write(state.path().join("off"), off.join("\n") + "\n").unwrap();
        engine.keep_off_in(state.path()).unwrap();
    }
    let run = engine.tool("lamp_help", &args, 1_000_000).unwrap();
    let mut answer = told(&run.calls);
    lamp_voice::use_skill::ran("lamp_help", &mut answer);
    answer
}

/// Each `LAMP_*` name in `text`.
fn switches(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut at = 0;
    while let Some(start) = text[at..].find("LAMP_").map(|offset| at + offset) {
        let end = start
            + text[start..]
                .bytes()
                .take_while(|byte| {
                    byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_'
                })
                .count();
        let whole =
            start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
        let name = &text[start..end];
        if whole && name.len() > "LAMP_".len() && !found.iter().any(|seen| seen == name) {
            found.push(name.to_owned());
        }
        at = end.max(start + 1);
    }
    found
}

/// Every switch the knowledge file names is one `docs/switches.md` lists:
/// the file is kept true to the code (#5790's rule).
#[test]
fn every_switch_the_faq_names_is_in_switches_md() {
    let census = std::fs::read_to_string(root().join("docs/switches.md")).unwrap();
    let listed = switches(&census);
    let named = switches(&knowledge_text());
    assert!(!named.is_empty());
    let missing: Vec<&String> = named.iter().filter(|name| !listed.contains(name)).collect();
    assert!(missing.is_empty(), "not in docs/switches.md: {missing:?}");
}

/// Every skill the knowledge file names in backticks is a folder of
/// `skills/`, so its state line names a real skill.
#[test]
fn every_skill_the_faq_names_is_a_skill_folder() {
    let skill = faq();
    let knowledge = skill.knowledge.as_ref().unwrap();
    for (topic, section) in knowledge.sections() {
        for name in &section.skills {
            assert!(
                root()
                    .join("skills")
                    .join(name)
                    .join("skill.toml")
                    .is_file(),
                "{topic} names {name}, which is no skill folder"
            );
        }
    }
}

/// The FAQ is a core skill reached by its tool alone, reading nothing,
/// lighting and moving nothing: it only tells.
#[test]
fn the_faq_only_tells() {
    let skill = faq();
    assert_eq!(skill.kind, lamp_skills::SkillKind::Core);
    assert_eq!(skill.tool.as_deref(), Some("lamp_help"));
    assert!(skill.triggers.is_empty());
    assert!(skill.permissions.speak);
    assert!(!skill.permissions.light && !skill.permissions.body && !skill.permissions.look);
    assert!(!skill.permissions.camera && !skill.permissions.messages);
    assert!(skill.permissions.topics.is_empty() && skill.permissions.actions.is_empty());
    assert!(skill.description.len() <= 1_024);
}

/// Each topic's call is told the note, its section and no other, within
/// what the link carries with `use_skill`'s note (`tools::sendable`); a
/// call naming none is told the default, `abilities`.
#[test]
fn each_topic_is_told_its_section_within_the_link() {
    let skill = faq();
    let knowledge = skill.knowledge.as_ref().unwrap();
    let mut topics = 0;
    for (topic, section) in knowledge.sections() {
        let answer = told_of(json!({ "topic": topic }), &[], &[]);
        assert!(
            lamp_voice::tools::sendable(&answer),
            "{topic}: {}",
            answer.to_string().len()
        );
        let next = answer["next"].as_str().unwrap();
        assert!(next.starts_with("Answer their question from the lamp's own notes below"));
        assert!(next.contains(&section.text), "{topic}");
        assert!(
            !next.contains("<!--"),
            "{topic}: a maintainer's comment told"
        );
        for (other, theirs) in knowledge.sections() {
            if other != topic {
                assert!(
                    !next.contains(&theirs.text),
                    "{topic} told {other}'s section"
                );
            }
        }
        topics += 1;
    }
    assert_eq!(topics, 12);
    let none = told_of(Value::Null, &[], &[]);
    assert!(
        none["next"]
            .as_str()
            .unwrap()
            .contains(&knowledge.section("abilities").unwrap().text)
    );
}

/// A skill a section names is told as on, off or not on this lamp, as
/// lamp-skills has it: never described as working when it is not here.
#[test]
fn a_skill_named_is_told_as_on_off_or_not_on_this_lamp() {
    let answer = told_of(json!({ "topic": "guard_mode" }), &[], &[]);
    let next = answer["next"].as_str().unwrap();
    assert!(next.contains("guard-mode is not on this lamp"), "{next}");
    let answer = told_of(json!({ "topic": "guard_mode" }), &["guard-mode"], &[]);
    assert!(
        answer["next"]
            .as_str()
            .unwrap()
            .contains("guard-mode is on")
    );
    let answer = told_of(
        json!({ "topic": "guard_mode" }),
        &["guard-mode"],
        &["guard-mode"],
    );
    assert!(
        answer["next"]
            .as_str()
            .unwrap()
            .contains("guard-mode is installed but off")
    );
}

/// One question of the eval set.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ask {
    /// What the person says.
    said: String,
    /// The topic `use_skill` should be called with for `faq`'s tool.
    topic: String,
    /// Words the section must hold for the right answer (any case).
    #[serde(default)]
    holds: Vec<String>,
    /// An ask the lamp can't do yet: its answer says so honestly.
    #[serde(default)]
    cannot: bool,
    /// Words the section must never hold: a feature it must not claim.
    #[serde(default)]
    never: Vec<String>,
    /// A skill the answer speaks of, turned off on this lamp (in the app or
    /// by voice): told it is installed but off, never as working.
    off: Option<String>,
    /// A skill the answer speaks of that is not on this lamp: not
    /// installed, or installed disabled, as `install::PRODUCT_SKILLS_OFF`
    /// leaves one. Told so, never as working.
    missing: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evals {
    ask: Vec<Ask>,
}

/// The eval set (#5790's "done when"): about 30 owner-style questions, each
/// mapped to the FAQ's tool and the topic whose section answers it, and a
/// few it must answer with an honest "I can't do that yet". Held here
/// without a model: each topic is the choice's, each answer's words are in
/// that section, and a can't-do ask's section claims none of what it asks.
/// Whether the model calls `use_skill` for each is a played row's.
#[test]
fn every_eval_question_is_answered_by_its_section() {
    let text = std::fs::read_to_string(root().join("skills/faq/tests/evals.toml")).unwrap();
    let evals: Evals = toml::from_str(&text).unwrap();
    assert!(evals.ask.len() >= 30, "{}", evals.ask.len());
    assert!(evals.ask.iter().filter(|ask| ask.cannot).count() >= 3);
    let skill = faq();
    let knowledge = skill.knowledge.as_ref().unwrap();
    for (topic, _) in knowledge.sections() {
        assert!(
            evals.ask.iter().any(|ask| ask.topic == topic),
            "no eval asks about {topic}"
        );
    }
    for ask in &evals.ask {
        let section = knowledge
            .section(&ask.topic)
            .unwrap_or_else(|| panic!("{:?}: no topic {}", ask.said, ask.topic));
        let lower = section.text.to_lowercase();
        assert!(ask.cannot || !ask.holds.is_empty(), "{:?}", ask.said);
        for word in &ask.holds {
            assert!(
                lower.contains(&word.to_lowercase()),
                "{:?}: {} holds no {word:?}",
                ask.said,
                ask.topic
            );
        }
        for word in &ask.never {
            assert!(
                !lower.contains(&word.to_lowercase()),
                "{:?}: {} claims {word:?}",
                ask.said,
                ask.topic
            );
        }
        if ask.cannot {
            assert!(!ask.never.is_empty(), "{:?}", ask.said);
        }
        // Asked about a skill that is off, or not here: the call is told
        // so, with the note's "say it isn't on for this lamp".
        let topic = json!({ "topic": ask.topic });
        let told_as = [
            ask.off.as_ref().map(|skill| {
                let next = told_of(topic.clone(), &[skill], &[skill])["next"].take();
                (skill, next, "is installed but off")
            }),
            ask.missing.as_ref().map(|skill| {
                let next = told_of(topic.clone(), &[], &[])["next"].take();
                (skill, next, "is not on this lamp")
            }),
        ];
        for (skill, next, state) in told_as.into_iter().flatten() {
            let next = next.as_str().unwrap();
            assert!(section.skills.contains(skill), "{:?}: {skill}", ask.said);
            assert!(
                next.contains(&format!("{skill} {state}")),
                "{:?}: {next}",
                ask.said
            );
            assert!(next.contains("say it isn't on for this lamp"), "{next}");
        }
    }
    assert!(evals.ask.iter().any(|ask| ask.off.is_some()));
    assert!(evals.ask.iter().any(|ask| ask.missing.is_some()));
    // The note tells the model to say so when the notes don't.
    let note = told_of(json!({ "topic": "abilities" }), &[], &[]);
    assert!(
        note["next"]
            .as_str()
            .unwrap()
            .contains("say plainly that you can't do that yet")
    );
}

/// A skill folder `a-help` in `root` with `toml` and, if given, its
/// `knowledge.md`.
fn folder(root: &Path, toml: &str, knowledge: Option<&str>) -> PathBuf {
    let dir = root.join("a-help");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: a-help\ndescription: Call when someone asks how to use the lamp.\n---\n",
    )
    .unwrap();
    std::fs::write(dir.join("skill.toml"), toml).unwrap();
    if let Some(text) = knowledge {
        std::fs::write(dir.join("knowledge.md"), text).unwrap();
    }
    dir
}

const HELP: &str = r#"tool = "a_help"
knowledge = "knowledge.md"
[permissions]
speak = true
[choice]
name = "topic"
about = "What they asked about."
one_of = ["one", "two"]
default = "one"
[[do]]
say = "Answer from the notes below."
"#;

/// `knowledge` loads only as the FAQ uses it: a tool alone starts the
/// skill, its choice names the sections with a default, its one say is a
/// note, and the file is there with a section for each word.
#[test]
fn knowledge_is_refused_out_of_its_place() {
    let root = tempfile::tempdir().unwrap();
    let both = "## one\nThe first.\n## two\nThe second.\n";
    assert!(load(&folder(root.path(), HELP, Some(both))).is_ok());
    let why = load(&folder(root.path(), HELP, Some("## one\nThe first.\n"))).unwrap_err();
    assert!(why.contains("no section"), "{why}");
    let why = load(&folder(root.path(), HELP, None)).unwrap_err();
    assert!(why.contains("knowledge.md"), "{why}");
    let no_default = HELP.replace("default = \"one\"\n", "");
    let why = load(&folder(root.path(), &no_default, Some(both))).unwrap_err();
    assert!(why.contains("default"), "{why}");
    let path = HELP.replace("\"knowledge.md\"", "\"../knowledge.md\"");
    let why = load(&folder(root.path(), &path, Some(both))).unwrap_err();
    assert!(why.contains("file name"), "{why}");
    let two_says = format!("{HELP}[[do]]\nif_choice = [\"two\"]\nsay = \"Also this.\"\n");
    assert!(load(&folder(root.path(), &two_says, Some(both))).is_err());
}

/// On a product lamp each skill the knowledge names is told as the
/// product's install leaves it: on, or not on this lamp, whether not one of
/// `install::PRODUCT_SKILLS` (Guard mode's case) or installed disabled, by
/// `install::PRODUCT_SKILLS_OFF` or its own `enabled = false` (Comfort
/// break without `LAMP_NUDGES_PRESENT`). So a skill the product installs
/// off (#5804's light skills) is told as not on, never offered.
#[test]
fn a_product_lamp_tells_each_named_skill_as_its_install_leaves_it() {
    use lamp_update::install::{PRODUCT_SKILLS, PRODUCT_SKILLS_OFF};
    let installed_off = |name: &str| PRODUCT_SKILLS_OFF.iter().any(|(off, _)| *off == name);
    let skills: Vec<lamp_skills::Skill> = PRODUCT_SKILLS
        .iter()
        .map(|name| {
            let mut skill = load(&root().join("skills").join(name)).unwrap();
            // As install's `OFF_LINE` above its `skill.toml` leaves it.
            skill.enabled &= !installed_off(name);
            skill
        })
        .collect();
    let on: Vec<String> = skills
        .iter()
        .filter(|skill| skill.enabled)
        .map(|skill| skill.name.clone())
        .collect();
    let mut engine = Engine::new(skills, true);
    let faq = faq();
    let knowledge = faq.knowledge.as_ref().unwrap();
    let mut checked = 0;
    for (topic, section) in knowledge.sections() {
        if section.skills.is_empty() {
            continue;
        }
        let run = engine
            .tool("lamp_help", &json!({ "topic": topic }), 1_000_000)
            .unwrap();
        let next = told(&run.calls)["next"].take();
        let next = next.as_str().unwrap();
        for name in &section.skills {
            let state = if on.contains(name) {
                "is on"
            } else {
                "is not on this lamp"
            };
            assert!(
                next.contains(&format!("{name} {state}")),
                "{topic}: {name} not told as {state}: {next}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 10, "{checked}");
}

/// The words of a skill's name in `title`, lowercased: a leading "a" left
/// out ("A sip of water").
fn name_words(title: &str) -> Vec<String> {
    let words: Vec<String> = title
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '\'')
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    match words.split_first() {
        Some((first, rest)) if first == "a" && !rest.is_empty() => rest.to_vec(),
        _ => words,
    }
}

/// Whether `words` hold `phrase` in a row, each word as it is or with a
/// plural's `s` ("a light scene" speaks of Light scenes).
fn speaks_of(words: &[String], phrase: &[String]) -> bool {
    let same = |a: &str, b: &str| {
        a == b || a.strip_suffix('s') == Some(b) || b.strip_suffix('s') == Some(a)
    };
    !phrase.is_empty()
        && words
            .windows(phrase.len())
            .any(|run| run.iter().zip(phrase).all(|(a, b)| same(a, b)))
}

/// Every skill `abilities`, the default topic and "what can you do?", speaks
/// of by its name (its folder's words or its SKILL.md title, "Light show",
/// "Point the light") is named in backticks on that line, so the call is
/// told whether it is on this lamp and never offers one that is off
/// (Twelve and Thirteen on #5822: the light skills were said there in plain
/// words, offered whatever their state).
#[test]
fn every_skill_abilities_speaks_of_is_named_so_its_state_is_told() {
    let skill = faq();
    let abilities = &skill
        .knowledge
        .as_ref()
        .unwrap()
        .section("abilities")
        .unwrap()
        .text;
    let mut names: Vec<(String, Vec<Vec<String>>)> = Vec::new();
    for entry in std::fs::read_dir(root().join("skills")).unwrap() {
        let dir = entry.unwrap().path();
        let Some(name) = dir.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == "faq" || !dir.join("skill.toml").is_file() {
            continue;
        }
        let mut phrases = vec![name_words(name)];
        let about = std::fs::read_to_string(dir.join("SKILL.md")).unwrap_or_default();
        if let Some(title) = about.lines().find_map(|line| line.strip_prefix("# ")) {
            phrases.push(name_words(title));
        }
        names.push((name.to_owned(), phrases));
    }
    assert!(names.len() > 20, "{}", names.len());
    let mut spoken = 0;
    for line in abilities.lines() {
        // What the line says aloud: its backticked names left out.
        let plain: String = line
            .split('`')
            .enumerate()
            .filter(|(index, _)| index % 2 == 0)
            .map(|(_, piece)| piece)
            .collect::<Vec<_>>()
            .join(" ");
        let words = name_words(&plain);
        for (name, phrases) in &names {
            if phrases.iter().any(|phrase| speaks_of(&words, phrase)) {
                assert!(
                    line.contains(&format!("`{name}`")),
                    "abilities speaks of {name} without naming it: {line:?}"
                );
                spoken += 1;
            }
        }
    }
    assert!(spoken >= 4, "{spoken}");
}

// Review regression (Reviewer-three on #5822): a 2,400-byte section with a
// permitted hour of room presence loaded, and forty presence spells told
// beside it made a `use_skill` answer of 4,988 bytes, past what the link
// carries (`lamp_voice::tools::sendable`). Knowledge and `[history]` are
// now refused together at load, so what loads stays sendable.
#[test]
fn reviewer_three_history_and_knowledge_answer_stays_sendable() {
    let root = tempfile::tempdir().unwrap();
    let manifest = HELP.replace(
        "[permissions]",
        "[history]\ntopic = \"room/presence\"\nwithin = \"1h\"\n[permissions]\ntopics = [\"room/presence\"]",
    );
    let section = "w".repeat(2_400);
    let text = format!("## one\n{section}\n## two\nThe second.\n");
    let why = load(&folder(root.path(), &manifest, Some(&text))).unwrap_err();
    assert!(why.contains("[history]"), "{why}");
    // The same skill without its history loads, and its answer, the longest
    // section told after the same presence, is sendable through `use_skill`.
    let skill = load(&folder(root.path(), HELP, Some(&text))).unwrap();
    assert!(
        skill
            .knowledge
            .as_ref()
            .unwrap()
            .too_long("Answer from the notes below.", false)
            .is_none()
    );
    let mut engine = Engine::new(vec![skill], true);
    for at_s in (0..800_u64).step_by(2) {
        let phase = at_s % 20;
        let someone = phase < 10;
        let nobody_s = if someone { 0 } else { phase - 8 };
        engine.observe(
            "room/presence",
            at_s * 1_000_000,
            format!(r#"{{"someone":{someone},"nobody_s":{nobody_s}}}"#).as_bytes(),
            Some(3_000),
        );
    }
    let run = engine
        .tool("a_help", &json!({"topic":"one"}), 800_000_000)
        .unwrap();
    let mut answer = told(&run.calls);
    lamp_voice::use_skill::ran("a_help", &mut answer);
    assert!(
        lamp_voice::tools::sendable(&answer),
        "accepted knowledge exceeds tool transport: {} bytes",
        answer.to_string().len()
    );
}

/// What the load accepts is sendable at its most: a note and section just
/// within `knowledge::TOLD_MOST_BYTES`, with skills it names, a look's
/// words, and `use_skill`'s note for a skill name of the most, 64 bytes.
#[test]
fn the_longest_knowledge_answer_the_load_accepts_is_sendable() {
    let root = tempfile::tempdir().unwrap();
    let name = format!("a{}", "b".repeat(63));
    let dir = root.path().join(&name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: Call when someone asks how to use the lamp.\n---\n"
        ),
    )
    .unwrap();
    let looking = HELP
        .replace("speak = true", "speak = true\ncamera = true")
        .replace("[[do]]\n", "[[do]]\nlook = true\n");
    std::fs::write(dir.join("skill.toml"), &looking).unwrap();
    let named = (0..20)
        .map(|index| format!("`skill-number-{index:02}`"))
        .collect::<Vec<_>>()
        .join(" ");
    let text = |words: usize| {
        format!(
            "## one\n{named} {}\n## two\nThe second.\n",
            "w".repeat(words)
        )
    };
    let fits = |words: usize| {
        std::fs::write(dir.join("knowledge.md"), text(words)).unwrap();
        load(&dir).is_ok()
    };
    // The longest section that still loads, found by halving.
    let (mut low, mut high) = (0, 2_400);
    assert!(fits(low));
    while low + 1 < high {
        let middle = (low + high) / 2;
        if fits(middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    assert!(!fits(low + 1) && fits(low));
    let skill = load(&dir).unwrap();
    let mut engine = Engine::new(vec![skill], true);
    let run = engine
        .tool("a_help", &json!({"topic":"one"}), 1_000_000)
        .unwrap();
    let mut answer = told(&run.calls);
    assert_eq!(answer["look"], true, "{answer}");
    assert!(
        answer["next"]
            .as_str()
            .unwrap()
            .contains("skill-number-19 is not on this lamp")
    );
    lamp_voice::use_skill::ran(&name, &mut answer);
    assert!(
        lamp_voice::tools::sendable(&answer),
        "{} bytes",
        answer.to_string().len()
    );
}

/// Seventh's P2 on #5822: the setup section tells the walk's steps as the
/// onboarding skill's `order` has them, each once and in that order, and no
/// step the walk doesn't offer, so the text can't drift from the walk again.
#[test]
fn the_setup_section_tells_the_walks_steps_in_its_order() {
    let toml: toml::Table = std::fs::read_to_string(root().join("skills/onboarding/skill.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let order: Vec<&str> = toml["onboarding"]["order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step.as_str().unwrap())
        .collect();
    // How the setup section names each step of the walk.
    let said = |step: &str| match step {
        "where_you_sit" => "where you sit",
        "who_you_are" => "your name and face",
        "touch" => "a pet on top of my head",
        "how_you_sound" => "saying \"Hey Lamp\"",
        other => panic!("the setup section doesn't name the walk's {other:?} step"),
    };
    let text = knowledge_text();
    let setup = text.split("## setup").nth(1).unwrap();
    let setup = &setup[..setup.find("\n## ").unwrap_or(setup.len())];
    let steps = setup
        .split("short steps: ")
        .nth(1)
        .and_then(|rest| rest.split(". Say \"skip\"").next())
        .unwrap();
    let mut at = 0;
    for step in &order {
        let found = steps[at..]
            .find(said(step))
            .unwrap_or_else(|| panic!("{step} missing or out of order in {steps:?}"));
        at += found + said(step).len();
    }
    let count = ["four", "three", "five", "two"]
        .iter()
        .position(|word| setup.contains(&format!("{word} short steps")))
        .map(|at| [4, 3, 5, 2][at]);
    assert_eq!(count, Some(order.len()), "{setup}");
    assert!(!setup.contains("privacy switch"), "{setup}");
}
