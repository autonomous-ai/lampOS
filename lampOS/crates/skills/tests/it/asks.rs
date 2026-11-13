//! A skill's own ask phrases (#6772, the first builder's ruling on #6059,
//! 13 Nov 10:15): an optional `asks = [...]` in `skill.toml`, checked as it
//! loads (so at push, like any field), and written to the tools file beside
//! its tool, where the voice reads them (`skill_named`).
use serde_json::json;
use std::path::PathBuf;

/// A skill folder named `name` with this `skill.toml`, in a folder of its
/// own; the folder is kept while the returned guard lives.
fn folder(name: &str, toml: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(name);
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Take a photo of whoever is at the lamp.\n---\n"),
    )
    .unwrap();
    std::fs::write(dir.join("skill.toml"), toml).unwrap();
    (root, dir)
}

const BOOTH: &str =
    "tool = \"photo_booth\"\n[permissions]\nspeak = true\n[[do]]\nsay = \"Say: smile.\"\n";

/// Seventh's "take a photo" (#6413) as the photo booth's ask: it loads, and
/// the tools file names it beside the tool.
#[test]
fn asks_load_and_are_written_beside_the_tool() {
    let toml = format!("asks = [\"take a photo\", \"Take a picture\", \"snap a pic\"]\n{BOOTH}");
    let (_root, dir) = folder("photo-booth", &toml);
    let mut skill = lamp_skills::load(&dir).unwrap();
    skill.enabled = true;
    assert_eq!(skill.asks, ["take a photo", "Take a picture", "snap a pic"]);
    let file = lamp_skills::tools_file(&[skill]);
    assert_eq!(
        file["asks"],
        json!({"photo_booth": ["take a photo", "Take a picture", "snap a pic"]})
    );
    // None given, and the tools file is as before: no `asks`.
    let (_root, dir) = folder("photo-booth", BOOTH);
    let mut skill = lamp_skills::load(&dir).unwrap();
    skill.enabled = true;
    assert!(skill.asks.is_empty());
    assert!(lamp_skills::tools_file(&[skill]).get("asks").is_none());
}

/// At most 8 phrases, each 2 to 8 words of letters, digits and apostrophes
/// with one space between, none listed twice; and only beside a tool that
/// takes no choice and no flag.
#[test]
fn asks_out_of_bounds_are_refused() {
    let nine: Vec<String> = (1..=9).map(|at| format!("\"take photo {at}\"")).collect();
    for asks in [
        "[\"photo\"]".to_owned(),
        "[\"one two three four five six seven eight nine\"]".to_owned(),
        "[\"take a photo!\"]".to_owned(),
        "[\"take  a photo\"]".to_owned(),
        "[\" take a photo\"]".to_owned(),
        "[\"take-a photo\"]".to_owned(),
        "[\"take a photo\", \"Take A Photo\"]".to_owned(),
        format!("[{}]", nine.join(", ")),
    ] {
        let (_root, dir) = folder("photo-booth", &format!("asks = {asks}\n{BOOTH}"));
        assert!(lamp_skills::load(&dir).is_err(), "{asks}");
    }
    let (_root, dir) = folder(
        "photo-booth",
        &format!("asks = [\"let's go\", \"it's 4 o'clock\"]\n{BOOTH}"),
    );
    assert!(lamp_skills::load(&dir).is_ok());
    for toml in [
        // No tool.
        "asks = [\"take a photo\"]\n[trigger]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n[ask]\npolicy = \"nudge\"\n[permissions]\ntopics = [\"voice/state\"]\nspeak = true\n[[do]]\nsay = \"Say: hi.\"\n".to_owned(),
        // A choice.
        format!("asks = [\"take a photo\"]\n{BOOTH}[choice]\nname = \"style\"\none_of = [\"color\", \"mono\"]\nabout = \"Which.\"\n"),
        // A flag.
        format!("asks = [\"take a photo\"]\n{BOOTH}[flag]\nname = \"flash\"\nabout = \"With the light.\"\n"),
    ] {
        let (_root, dir) = folder("photo-booth", &toml);
        assert!(lamp_skills::load(&dir).is_err(), "{toml}");
    }
}
