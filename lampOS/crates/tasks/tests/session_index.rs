//! #6274: the index of the sessions the lamp itself started or handed work
//! to (`lamp_tasks::session_index`): their id, name and last use only.
use lamp_tasks::session_index::{self, MOST_INDEXED};
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt};

const MACHINE: &str = "0123456789abcdef0123456789abcdef";
const LAPTOP: &str = "fedcba9876543210fedcba9876543210";

fn id(n: usize) -> String {
    format!("{n:08x}-1111-4111-8111-111111111111")
}

/// Kept on disk, each session is its id, its name and when it was last
/// used, by machine: nothing of what it was asked, answered or is about,
/// nor its folder.
#[test]
fn the_index_holds_only_id_name_and_last_use() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("sessions-started.json");
    session_index::record(&file, MACHINE, &id(1), Some("Christmas dates"), 1_000);
    session_index::record(&file, LAPTOP, &id(2), None, 2_000);
    let kept: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    let mut top: Vec<&str> = kept
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    top.sort_unstable();
    assert_eq!(top, ["machines", "version"]);
    for (machine, entries) in kept["machines"].as_object().unwrap() {
        for entry in entries.as_array().unwrap() {
            let mut keys: Vec<&str> = entry
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(keys, ["id", "last_used", "name"], "{machine}");
        }
    }
    assert_eq!(kept["machines"][MACHINE][0]["name"], "Christmas dates");
    assert_eq!(kept["machines"][LAPTOP][0]["name"], "");
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // Used again: its time moves on, and a name not given keeps the one kept.
    session_index::record(&file, MACHINE, &id(1), None, 3_000);
    let read = session_index::read(&file);
    assert_eq!(read[MACHINE].len(), 1);
    assert_eq!(read[MACHINE][0].name, "Christmas dates");
    assert_eq!(read[MACHINE][0].last_used, 3_000);
}

/// At most [`MOST_INDEXED`] sessions in all, on every machine together:
/// one more lets the least lately used go.
#[test]
fn the_index_keeps_the_most_lately_used() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("sessions-started.json");
    for n in 0..MOST_INDEXED {
        let machine = if n % 2 == 0 { MACHINE } else { LAPTOP };
        session_index::record(&file, machine, &id(n), Some("work"), 1_000 + n as u64);
    }
    // The oldest is used again, so the second oldest is the one to go.
    session_index::record(&file, MACHINE, &id(0), None, 10_000);
    session_index::record(&file, LAPTOP, &id(MOST_INDEXED), Some("new"), 10_001);
    let read = session_index::read(&file);
    let all: Vec<&str> = read
        .values()
        .flatten()
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(all.len(), MOST_INDEXED);
    assert!(all.contains(&id(0).as_str()));
    assert!(!all.contains(&id(1).as_str()));
    assert!(all.contains(&id(MOST_INDEXED).as_str()));
}

/// Forgotten, the file is gone; forgetting again finds nothing. A file
/// that isn't the index is read as none and written over.
#[test]
fn forgetting_removes_the_index() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("sessions-started.json");
    assert!(!session_index::forget(&file).unwrap());
    session_index::record(&file, MACHINE, &id(1), Some("Christmas dates"), 1_000);
    assert!(session_index::forget(&file).unwrap());
    assert!(!file.exists());
    assert!(session_index::read(&file).is_empty());
    fs::write(&file, b"not the index").unwrap();
    assert!(session_index::read(&file).is_empty());
    session_index::record(&file, MACHINE, &id(1), Some("Christmas dates"), 1_000);
    assert_eq!(session_index::read(&file)[MACHINE].len(), 1);
}

/// Signed out (the Reviewer-lead's P3 on #6447): only the machines kept
/// stay; nothing is written when none goes.
#[test]
fn forgetting_machines_keeps_only_those_kept() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("sessions-started.json");
    session_index::record(&file, "account", "a1", Some("Christmas dates"), 1);
    session_index::record(&file, "here", "h1", Some("Login fix"), 2);
    session_index::forget_machines(&file, |machine| machine == "here");
    let kept = session_index::read(&file);
    assert_eq!(kept.keys().collect::<Vec<_>>(), ["here"]);
    let before = std::fs::metadata(&file).unwrap().modified().unwrap();
    session_index::forget_machines(&file, |machine| machine == "here");
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        before
    );
}
