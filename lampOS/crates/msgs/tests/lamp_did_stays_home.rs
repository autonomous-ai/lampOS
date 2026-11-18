//! #7757: what the lamp did, and why, stays on the lamp. `lamp/did` and
//! `LampDid` are published by lamp-skills (and the parts that tell their own
//! actions, each in a small PR of its own), kept in memory for an hour by the
//! voice, and named by voice-eval only to count a left-out message by it. No
//! other part reads it: not the cloud, telemetry, the usage reports, memory,
//! the records or any skill. A part that should is added here on purpose,
//! by the one who adds it, with the reason.
use std::fs;
use std::path::Path;

/// Crates that may name the topic or the type, and why.
const MAY_NAME_IT: &[(&str, &str)] = &[
    ("msgs", "the type and its topic"),
    (
        "skills",
        "tells each outward run it makes (LAMP_DID_PUBLISH)",
    ),
    (
        "voice",
        "keeps the last hour in memory for the look-up (LAMP_DID_KEEP)",
    ),
    (
        "voice-eval",
        "names the topic only to count a left-out message by it (BUS_TOPICS), never reads one",
    ),
];

const NAMES: &[&str] = &["lamp/did", "LampDid", "did_history", "DidHistory"];

fn sources(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            sources(&path, found);
        } else if path
            .extension()
            .is_some_and(|ext| matches!(ext.to_str(), Some("rs" | "toml" | "py" | "sh" | "json")))
        {
            found.push(path);
        }
    }
}

#[test]
fn no_part_that_leaves_the_lamp_or_lasts_reads_what_the_lamp_did() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut reached = Vec::new();
    for entry in fs::read_dir(&crates).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if MAY_NAME_IT.iter().any(|(allowed, _)| *allowed == name) || !entry.path().is_dir() {
            continue;
        }
        let mut files = Vec::new();
        sources(&entry.path(), &mut files);
        for file in files {
            let text = fs::read_to_string(&file).unwrap_or_default();
            if NAMES.iter().any(|name| text.contains(name)) {
                reached.push(format!("{name}: {}", file.display()));
            }
        }
    }
    assert!(
        reached.is_empty(),
        "a part reads what the lamp did without being named in MAY_NAME_IT: {reached:?}"
    );
    for never in [
        "cloud",
        "records",
        "memory",
        "reports",
        "update",
        "telemetry",
    ] {
        assert!(
            MAY_NAME_IT.iter().all(|(name, _)| *name != never),
            "{never}"
        );
    }
}

/// The skills and the tools' scripts, outside `crates/`, never name them.
#[test]
fn no_skill_or_script_outside_the_crates_names_what_the_lamp_did() {
    let lamp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut reached = Vec::new();
    for dir in [
        "../skills",
        "../scripts",
        "skills",
        "sdk",
        "tools",
        "examples",
        "deploy",
        "training",
    ] {
        let mut files = Vec::new();
        sources(&lamp.join(dir), &mut files);
        for file in files {
            let text = fs::read_to_string(&file).unwrap_or_default();
            if NAMES.iter().any(|name| text.contains(name)) {
                reached.push(file.display().to_string());
            }
        }
    }
    assert!(reached.is_empty(), "{reached:?}");
}
