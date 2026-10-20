//! A skill made by voice (#1685, `lamp_voice::made_skill`), checked by the
//! real `lamp-skills pack`, the checker an install uses (#1698): what passes
//! is packed and kept in a 0700 folder with its SHA-256, at most five;
//! what does more than it declares, or more than a made skill may, is not.
use lamp_voice::made_skill::{KEPT_MOST, Keeping, Made, Refused, keep};
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

const TOML: &str = r#"kind = "installable"
version = "1.0.0"
needs = ["light"]

[permissions]
topics = ["voice/state"]
light = true

[trigger]
topic = "voice/state"
field = "state"
equals = "resting"
cooldown = "12h"

[quiet]
from = "22:30"
to = "22:00"

[ask]
policy = "nudge"

[[do]]
light = { dim = 1 }
"#;

fn card(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Dims the light a shade at 10 pm.\n---\n\n# Evening dim\n\nAt ten it dims the light a shade.\n"
    )
}

fn answer(toml: &str, name: &str) -> String {
    format!(
        "Here is the skill.\n\n```toml skill.toml\n{toml}```\n\n```markdown SKILL.md\n{}```\n",
        card(name)
    )
}

fn keeping(dir: &Path) -> Keeping {
    Keeping {
        directory: dir.join("made"),
        program: env!("CARGO_BIN_EXE_lamp-skills").into(),
    }
}

fn tars(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".tar"))
        .collect();
    names.sort();
    names
}

/// The skill passes the cap and the checker: packed, kept in the voice's
/// own 0700 folder by its SHA-256, marked `made: here`, with what it uses.
#[test]
fn a_skill_made_by_voice_is_checked_packed_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let made: Made = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    assert_eq!(
        (made.name.as_str(), made.version.as_str()),
        ("evening-dim", "1.0.0")
    );
    assert_eq!(made.description, "Dims the light a shade at 10 pm.");
    assert_eq!(made.uses.spoken(), "its light and whether it is talking");
    let bytes = fs::read(&made.package).unwrap();
    let sha: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(made.sha256, sha);
    assert!(
        String::from_utf8_lossy(&bytes).contains("made: here"),
        "the card in the package says it was made here"
    );
    let mode = fs::metadata(&keeping.directory)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
    assert_eq!(tars(&keeping.directory).len(), 1);
    // Nothing is left beside it, and nothing went into a skills folder.
    let left: Vec<_> = fs::read_dir(&keeping.directory)
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with(".making"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    // The same hand-over again: the kept one.
    let again = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    assert_eq!(again, made);
    assert_eq!(tars(&keeping.directory).len(), 1);
}

/// What the checker refuses is not kept: a step the skill's permissions
/// don't give (a gesture, with only the light), as an install would refuse
/// it. What the cap refuses never reaches the checker.
#[test]
fn a_made_skill_that_does_more_than_it_declares_or_may_is_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let gesture = TOML.replace(
        "light = { dim = 1 }",
        "light = { dim = 1 }\nbody = { gesture = \"curious\" }",
    );
    let refused = keep(&answer(&gesture, "evening-dim"), false, "s1:8:1", &keeping);
    assert_eq!(
        refused,
        Err(Refused("it doesn't pass the lamp's check".into()))
    );
    let speaking = TOML.replace("light = true\n", "light = true\nspeak = true\n");
    let capped = keep(&answer(&speaking, "evening-dim"), false, "s1:9:1", &keeping);
    assert!(capped.unwrap_err().0.contains("more than the light"));
    assert!(tars(&keeping.directory).is_empty());
}

/// At most five made skills are kept: the sixth lets the oldest go, its
/// package with it.
#[test]
fn at_most_five_made_skills_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    for made in 1..=KEPT_MOST + 1 {
        keep(
            &answer(TOML, &format!("evening-dim-{made}")),
            false,
            &format!("s1:{made}:1"),
            &keeping,
        )
        .unwrap();
    }
    let kept = tars(&keeping.directory);
    assert_eq!(kept.len(), KEPT_MOST, "{kept:?}");
    assert!(
        !kept.iter().any(|name| name.starts_with("evening-dim-1-")),
        "{kept:?}"
    );
    assert!(kept.iter().any(|name| name.starts_with("evening-dim-6-")));
}
