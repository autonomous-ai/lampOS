//! Since #1806 a crate's integration tests in `tests/it/` run as one process,
//! as its unit tests in `src/` always have. A test's scratch folder named by
//! the process (`/tmp/lpp-act-<pid>`) is then one folder for every file that
//! builds that name, and each file's tests hear or delete the other's (the
//! first builder on #1722, 21 Oct 13:1x: lamp-policy's `program.rs` and
//! `progress.rs`, lamp-cloud's `ring.rs` and `app_contract.rs`). Only plain
//! `cargo test` shows it: nextest gives each test its own process.
//!
//! This holds each such name to one file of a test binary. A name is a
//! `format!` string that starts `/tmp/` or is joined to `/tmp` or
//! `temp_dir()`, with `process::id()` among its arguments, its `{…}` parts
//! read as one. Within a crate, one file of `src/` and one of `tests/it/` may
//! build it. Give each file a prefix of its own.
//!
//! Every `{…}` reads as one, so `/tmp/x-{name}-{}` and `/tmp/x-{}-{n}` count
//! as one name though they can't meet: a rename clears it. A folder inside
//! another file's (`/tmp/a-{}` beside `/tmp/a-{}/b`) isn't seen.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

/// Each Rust file under `dir`.
fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The test binary `file` of a crate is built into, as `src`, `it` or the
/// file's own name, or `None` for a file outside the crate's tests (a
/// helper another binary includes, a fixture).
fn binary(within: &Path) -> Option<String> {
    let parts: Vec<&str> = within.iter().filter_map(|part| part.to_str()).collect();
    match parts.as_slice() {
        ["src", "main.rs"] => Some("main".into()),
        ["src", "bin", name, ..] => Some(format!("bin/{name}")),
        ["src", ..] => Some("src".into()),
        ["tests", "it", ..] => Some("it".into()),
        ["tests", name] => Some((*name).into()),
        _ => None,
    }
}

/// The rest of a `format!` call after its string: up to the parenthesis that
/// closes the call, strings inside it passed over.
fn arguments(rest: &str) -> &str {
    let (mut depth, mut quoted, mut escaped) = (1, false, false);
    for (index, c) in rest.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return &rest[..index];
                }
            }
            _ => {}
        }
    }
    rest
}

/// Whether the code `before` a `format!` joins it to `/tmp` or `temp_dir()`:
/// it ends in `.join(`, with either in the same statement within the 200
/// characters before, white space left out. So `PathBuf::from("/tmp").join(`, `fs::canonicalize("/tmp")
/// .unwrap().join(` and a `temp_dir()` with `.join(` on the next line all
/// count (the reviewer-lead's P2 on #2501).
fn joined_to_a_scratch_root(before: &str) -> bool {
    let mut start = before.len().saturating_sub(200);
    while !before.is_char_boundary(start) {
        start -= 1;
    }
    let code: String = before[start..]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let code = code.rsplit([';', '}']).next().unwrap_or_default();
    code.ends_with(".join(")
        && (code.contains("\"/tmp\"") || code.contains("\"/tmp/") || code.contains("temp_dir()"))
}

/// The scratch folder names `text` builds, each with its `{…}` parts as `{}`.
fn scratch_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find("format!(") {
        let at = from + found;
        from = at + "format!(".len();
        let line = text[..at].rfind('\n').map_or(0, |start| start + 1);
        if text[line..at].trim_start().starts_with("//") {
            continue;
        }
        let rest = text[from..].trim_start();
        let Some(literal) = rest.strip_prefix('"') else {
            continue;
        };
        let Some(end) = literal.find('"') else {
            continue;
        };
        let name = &literal[..end];
        if !arguments(&literal[end + 1..]).contains("process::id()")
            || !(name.starts_with("/tmp/") || joined_to_a_scratch_root(&text[..at]))
        {
            continue;
        }
        let mut read = String::new();
        let mut inside = false;
        for c in name.chars() {
            match c {
                '{' => {
                    inside = true;
                    read.push_str("{}");
                }
                '}' => inside = false,
                c if !inside => read.push(c),
                _ => {}
            }
        }
        names.push(read);
    }
    names
}

#[test]
fn no_two_files_of_a_test_binary_build_the_same_scratch_folder() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut built: BTreeMap<(String, String, String), BTreeSet<String>> = BTreeMap::new();
    let mut seen = 0;
    for entry in fs::read_dir(&crates).unwrap().flatten() {
        let root = entry.path();
        let Some(name) = root.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let mut files = Vec::new();
        sources(&root.join("src"), &mut files);
        sources(&root.join("tests"), &mut files);
        for file in files {
            let within = file.strip_prefix(&root).unwrap();
            let Some(binary) = binary(within) else {
                continue;
            };
            let text = fs::read_to_string(&file).unwrap();
            for scratch in scratch_names(&text) {
                seen += 1;
                built
                    .entry((name.to_owned(), binary.clone(), scratch))
                    .or_default()
                    .insert(within.display().to_string());
            }
        }
    }
    // The workspace builds many: an empty read means this one went wrong.
    assert!(
        seen > 300,
        "only {seen} scratch names found under {crates:?}"
    );
    let shared: Vec<String> = built
        .iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|((name, binary, scratch), files)| {
            format!("{name} ({binary}): {scratch:?} in {files:?}")
        })
        .collect();
    assert!(
        shared.is_empty(),
        "one scratch folder for two files of a test binary; give each file a prefix of its own:\n{}",
        shared.join("\n")
    );
}

#[test]
fn the_names_read_are_the_scratch_folders_only() {
    let text = r#"
        let a = PathBuf::from(format!("/tmp/lcr-{}", std::process::id()));
        let b = std::env::temp_dir().join(format!("lm-{}-s-{name}", std::process::id()));
        // let c = PathBuf::from(format!("/tmp/commented-{}", std::process::id()));
        let d = format!("{source}.{}.{boot}", process::id());
        let e = PathBuf::from(format!("/tmp/no-pid-{name}"));
        let f = std::path::PathBuf::from(format!(
            "/tmp/lvo-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let g = PathBuf::from("/tmp").join(format!("lsn-{what}-{}", std::process::id()));
        let h = fs::canonicalize("/tmp").unwrap().join(format!("lvr-{}", std::process::id()));
        let i = std::env::temp_dir()
            .join(format!("jl-{}", std::process::id()));
        let j = root.join(format!("not-scratch-{}", std::process::id()));
    "#;
    assert_eq!(
        scratch_names(text),
        [
            "/tmp/lcr-{}",
            "lm-{}-s-{}",
            "/tmp/lvo-{}-{}",
            "lsn-{}-{}",
            "lvr-{}",
            "jl-{}"
        ]
    );
    assert_eq!(binary(Path::new("src/a/b.rs")).as_deref(), Some("src"));
    assert_eq!(binary(Path::new("tests/it/x.rs")).as_deref(), Some("it"));
    assert_eq!(
        binary(Path::new("tests/wake.rs")).as_deref(),
        Some("wake.rs")
    );
    assert_eq!(binary(Path::new("tests/support/x.rs")), None);
}
