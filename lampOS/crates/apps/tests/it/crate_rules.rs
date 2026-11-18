//! Rules over the crate's own source (no uid, no user made, nothing recursive).
use std::fs;

fn sources() -> Vec<(String, String)> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| {
            (
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            )
        })
        .collect()
}

#[test]
fn no_uid_is_stored_or_compared_and_no_user_is_made() {
    // Spelled in pieces so this file does not trip its own rule.
    let words = [
        ["getpw", "nam"].concat(),
        ["uid", "_of"].concat(),
        ["ch", "own"].concat(),
        ["user", "add"].concat(),
        ["user", "del"].concat(),
        ["group", "add"].concat(),
    ];
    for (path, text) in sources() {
        for word in &words {
            let hits: Vec<&str> = text
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .filter(|line| line.contains(word.as_str()))
                .collect();
            assert!(hits.is_empty(), "{path}: `{word}`: {hits:?}");
        }
    }
}
