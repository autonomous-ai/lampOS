//! docs/msgs.md names every message type this crate declares, so a type
//! added without its row is caught here: six were missing on 1 Nov 2026
//! (#4809).
use std::{fs, path::Path};

/// Every `lamp_msgs/<Name>` a `const TYPE` in `src/` declares, by name.
fn declared(dir: &Path, names: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            declared(&path, names);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            for line in fs::read_to_string(&path).unwrap().lines() {
                let Some((_, after)) = line.split_once("const TYPE:") else {
                    continue;
                };
                let Some((_, name)) = after.split_once("\"lamp_msgs/") else {
                    continue;
                };
                let name = name.split('"').next().unwrap().to_owned();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
}

#[test]
fn msgs_md_names_every_message_type_the_crate_declares() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut names = Vec::new();
    declared(&root.join("src"), &mut names);
    assert!(names.len() >= 80, "only {} types found", names.len());
    let doc = fs::read_to_string(root.join("../../docs/msgs.md")).unwrap();
    let missing: Vec<&String> = names
        .iter()
        .filter(|name| !doc.contains(&format!("`{name}`")))
        .collect();
    assert!(missing.is_empty(), "msgs.md names none of {missing:?}");
}
