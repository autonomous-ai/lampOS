//! The functions a product lamp's voice tells each session of, with every
//! product skill installed. A session takes at most `lamp_gemini::MAX_TOOLS`
//! (32), and what is past it is left out by order, without a word: the
//! voice's own first, then the actions part's, then the skills' (the
//! coordinator, 19 Oct: lamp-4ace sat at exactly 32, and switching
//! onboarding on there pushed one of the actions part's out). So the
//! product's count is pinned here, worked out by each part's own code: the
//! voice's own from the product voice unit's environment, the actions
//! part's whole catalog (its sound, its light and Schedule's), and the tools
//! of `install::PRODUCT_SKILLS` as `lamp-skills` writes them.
use lamp_update::install;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const VOICE: &str = include_str!("../../../deploy/product/lamp-voice.service");
/// Set only for the run in a process of its own: the folder of the
/// catalogs it reads, there as the product's `LAMP_ACTIONS_DIR`.
const CATALOGS: &str = "LAMP_TEST_PRODUCT_CATALOGS";
/// The test run in a process of its own.
const VOICE_SIDE: &str = "the_product_voice_s_functions_in_a_process_of_their_own";
/// How that process says each function given, and each left out: found
/// anywhere in a line, as the test runner may begin the first with the
/// test's name.
const DECLARED: &str = "declared: ";
const LEFT_OUT: &str = "left out: ";

/// What a product lamp declares now: the voice's own 13, the actions part's
/// 17 and the skills' 1 (Dance's), one under the bound. A change to it is a
/// change to what every session is told: say in its pull request what is
/// left out past the bound, if anything.
const PRODUCT_FUNCTIONS: usize = 31;

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

/// Every `Environment=` of the product voice unit's `[Service]`.
fn voice_environment() -> Vec<(String, String)> {
    let mut inside = false;
    let mut set = Vec::new();
    for line in VOICE.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[Service]";
        } else if inside && let Some(pair) = line.strip_prefix("Environment=") {
            let (name, value) = pair.split_once('=').unwrap();
            set.push((name.to_owned(), value.to_owned()));
        }
    }
    set
}

/// The voice's side, run in a process of its own by
/// [`a_product_lamp_declares_every_function_it_has_within_the_bound`] with
/// the product voice unit's environment and no other `LAMP_` setting, as
/// the voice's provider worker works it out: its flags from that
/// environment, its memory started (the rules' functions are told only
/// then), a session's functions, and what the link would leave out of
/// them. Does nothing in any other run.
#[test]
fn the_product_voice_s_functions_in_a_process_of_their_own() {
    if std::env::var_os(CATALOGS).is_none() {
        return;
    }
    let flags = lamp_voice::tools::arguments_from_environment();
    let (settings, rest) = lamp_voice::tools::take(&flags).unwrap();
    assert!(rest.is_empty(), "{rest:?}");
    if let Some(socket) = &settings.memory {
        lamp_voice::remember::start(socket.clone(), None);
    }
    let (kept, left_out) = lamp_voice::tools::admitted(lamp_voice::tools::declared(&settings));
    for tool in kept {
        println!("{DECLARED}{}", tool.name);
    }
    for why in left_out {
        println!("{LEFT_OUT}{why}");
    }
}

/// What the product voice declares, run in a process of its own with the
/// product voice unit's environment and `extra` on top: the actions part's
/// whole catalog, the product skills' tools, and the names given and left
/// out by the link.
struct Product {
    executive: Vec<lamp_actions::Function>,
    skill_tools: Vec<String>,
    declared: Vec<String>,
    left_out: Vec<String>,
}

fn product_with(extra: &[(&str, &str)]) -> Product {
    let folder = tempfile::tempdir().unwrap();
    // The actions part's catalog with all it offers: sound and light, both
    // there, and Schedule's (the product's LAMP_SCHEDULE_TOOLS=on).
    let mut executive = lamp_actions::catalog(true, true);
    executive.extend(lamp_actions::schedule::catalog());
    std::fs::write(
        folder.path().join(lamp_voice::tools::CATALOG_FILE),
        lamp_actions::catalog_file(&executive),
    )
    .unwrap();
    // The skills' tools beside it, as `lamp-skills --tools-dir` writes them.
    let skills: Vec<lamp_skills::Skill> = install::PRODUCT_SKILLS
        .iter()
        .map(|name| lamp_skills::load(&skills_root().join(name)).unwrap())
        .collect();
    let beside = folder.path().join(lamp_voice::tools::SKILLS_DIR);
    std::fs::create_dir(&beside).unwrap();
    std::fs::write(
        beside.join(lamp_voice::tools::CATALOG_FILE),
        lamp_skills::tools_file(&skills).to_string(),
    )
    .unwrap();

    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([VOICE_SIDE, "--exact", "--nocapture", "--test-threads=1"]);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("LAMP_") {
            command.env_remove(name);
        }
    }
    for (name, value) in voice_environment() {
        if name == "LAMP_ACTIONS_DIR" {
            command.env(name, folder.path());
        } else {
            command.env(name, value);
        }
    }
    for (name, value) in extra {
        command.env(name, value);
    }
    let output = command.env(CATALOGS, folder.path()).output().unwrap();
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let found = |mark: &str| -> Vec<String> {
        said.lines()
            .filter_map(|line| line.split_once(mark).map(|(_, rest)| rest.to_owned()))
            .collect()
    };
    Product {
        executive,
        skill_tools: skills
            .iter()
            .filter_map(|skill| skill.tool.clone())
            .collect(),
        declared: found(DECLARED),
        left_out: found(LEFT_OUT),
    }
}

/// Every function the actions part and the product skills offer reaches a
/// product lamp's sessions, none left out by order or by the link, and the
/// count is [`PRODUCT_FUNCTIONS`], within `lamp_gemini::MAX_TOOLS`.
#[test]
fn a_product_lamp_declares_every_function_it_has_within_the_bound() {
    let Product {
        executive,
        skill_tools,
        declared,
        left_out,
    } = product_with(&[]);
    assert!(left_out.is_empty(), "left out by the link: {left_out:?}");
    for name in executive
        .iter()
        .map(|function| function.name)
        .chain(skill_tools.iter().map(String::as_str))
    {
        assert!(
            declared.iter().any(|told| told == name),
            "{name} is left out of a product lamp's sessions, past {}: {declared:?}",
            lamp_gemini::MAX_TOOLS
        );
    }
    println!(
        "A product lamp declares {} functions of at most {}: the voice's own {}, the actions part's {}, the skills' {} ({})",
        declared.len(),
        lamp_gemini::MAX_TOOLS,
        declared.len() - executive.len() - skill_tools.len(),
        executive.len(),
        skill_tools.len(),
        skill_tools.join(", ")
    );
    assert!(declared.len() <= lamp_gemini::MAX_TOOLS);
    assert_eq!(declared.len(), PRODUCT_FUNCTIONS, "{declared:?}");
    assert!(
        !declared
            .iter()
            .any(|name| name == lamp_voice::reports_stop::NAME
                || name == lamp_voice::reports_stop::STATE_NAME)
    );
}

/// The reviewer's P3 on #1906: with `LAMP_REPORTS=on` the product voice
/// declares its two reports functions too, `reports_stop` and
/// `reports_state`, before the actions part's and the skills'. The voice's
/// own 15 and the actions part's 17 are the 32, so Dance's, the one
/// product skill's function, is the one cut, and the link leaves out nothing.
#[test]
fn with_reports_on_a_product_lamp_is_at_32_and_dance_is_cut() {
    let Product {
        executive,
        skill_tools,
        declared,
        left_out,
    } = product_with(&[(lamp_voice::reports_stop::ENVIRONMENT, "on")]);
    assert!(left_out.is_empty(), "left out by the link: {left_out:?}");
    assert_eq!(declared.len(), lamp_gemini::MAX_TOOLS, "{declared:?}");
    assert_eq!(declared.len(), PRODUCT_FUNCTIONS + 1, "{declared:?}");
    for name in [
        lamp_voice::reports_stop::NAME,
        lamp_voice::reports_stop::STATE_NAME,
    ]
    .into_iter()
    .chain(executive.iter().map(|function| function.name))
    {
        assert!(
            declared.iter().any(|told| told == name),
            "{name}: {declared:?}"
        );
    }
    assert_eq!(skill_tools, ["dance"]);
    assert!(!declared.iter().any(|told| told == "dance"), "{declared:?}");
}
