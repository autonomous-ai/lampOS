//! #7685: what a person's body sounds say is theirs. `ears/body` and
//! `BodySound` are read by the lamp's skills and the local door, and by
//! nothing that leaves the lamp or lasts: not the cloud, not telemetry or the
//! usage reports, not memory or the records, and not the voice (which never
//! answers a cough as if spoken to). A part that should read them is added
//! here on purpose, by the one who adds it, with the reason.
use std::fs;
use std::path::Path;

/// Crates that may name the topic or the type, and why.
const MAY_NAME_IT: &[(&str, &str)] = &[
    ("ears", "publishes them, on the lamp"),
    ("msgs", "the type and its topic"),
    ("sim", "its stimuli, no lamp"),
    (
        "voice-eval",
        "names the topic only to count a left-out message by it (BUS_TOPICS), never reads one",
    ),
];

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
            .is_some_and(|ext| ext == "rs" || ext == "toml")
        {
            found.push(path);
        }
    }
}

#[test]
fn no_part_that_leaves_the_lamp_or_lasts_reads_a_persons_body_sounds() {
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
            if text.contains("ears/body")
                || text.contains("BodySound")
                || text.contains("BodyClass")
            {
                reached.push(format!("{name}: {}", file.display()));
            }
        }
    }
    assert!(
        reached.is_empty(),
        "a part reads a person's body sounds without being named in MAY_NAME_IT: {reached:?}"
    );
    // The cloud, the reports, memory and the voice are the ones that must never be added.
    for never in ["cloud", "records", "memory", "voice", "reports", "update"] {
        assert!(
            MAY_NAME_IT.iter().all(|(name, _)| *name != never),
            "{never}"
        );
    }
}
