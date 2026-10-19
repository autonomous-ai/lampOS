//! The ARM64 workflow skips a pull request that changes only docs a build
//! never reads (the project lead on #1711, #1791): its `pull_request` paths
//! leave out `lampOS/docs/**/*.md` and `lampOS/builders/**`, and name back
//! the docs that code or tests read. A doc a test reads but the workflow
//! leaves out would let a change to it merge without the run that tests it.
//! This holds the list to the code: every `docs/…/*.md` or `builders/…` path
//! named in a string in a crate's code (`include_str!`, a path a test opens)
//! must be named back after the line that leaves it out.
//!
//! A path put together from pieces (`join("docs").join("x.md")`) is not
//! seen: name such a path in one string.
use std::{
    fs,
    path::{Path, PathBuf},
};

const WORKFLOW: &str = "../.github/workflows/lampos-arm64.yml";

/// Each Rust file under `dir`.
fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The paths of `docs/…/*.md` and `builders/…` named in strings in `text`'s
/// code, its line comments left out, from `docs/` or `builders/` on.
fn named_in(text: &str) -> Vec<String> {
    let mut named = Vec::new();
    for line in text.lines() {
        let code = line.split("//").next().unwrap_or_default();
        for (index, part) in code.split('"').enumerate() {
            // The odd parts are inside quotes.
            if index % 2 == 0 {
                continue;
            }
            for start in ["docs/", "builders/"] {
                let Some(at) = part.find(start) else {
                    continue;
                };
                if at > 0 && !part[..at].ends_with('/') {
                    continue;
                }
                let path = &part[at..];
                let plain = path
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c));
                if plain && (start == "builders/" || path.ends_with(".md")) {
                    named.push(path.to_owned());
                }
            }
        }
    }
    named
}

/// The workflow's `pull_request` paths, in their order.
fn pull_request_paths(workflow: &str) -> Vec<String> {
    let after = workflow
        .split("\n  pull_request:\n")
        .nth(1)
        .expect("a pull_request trigger");
    let block = after
        .split("\n  workflow_dispatch:")
        .next()
        .unwrap_or(after);
    block
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- '")?.strip_suffix('\''))
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_doc_a_crate_reads_is_named_back_in_the_workflows_pull_request_paths() {
    let lampos = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // A copy of lampOS alone (a podman export) has no workflow; GitHub's
    // checkout always has it.
    let Ok(workflow) = fs::read_to_string(lampos.join(WORKFLOW)) else {
        assert!(
            std::env::var_os("GITHUB_ACTIONS").is_none(),
            "no {WORKFLOW} on GitHub Actions"
        );
        eprintln!("no {WORKFLOW} beside this lampOS: not checked");
        return;
    };
    let paths = pull_request_paths(&workflow);
    let mut files = Vec::new();
    sources(&lampos.join("crates"), &mut files);
    assert!(files.len() > 100, "{} files", files.len());
    let mut read = Vec::new();
    for file in &files {
        // This file's own examples, in the test below, are not reads.
        if file.ends_with("update/tests/ci_paths.rs") {
            continue;
        }
        let text = fs::read_to_string(file).unwrap_or_default();
        for path in named_in(&text) {
            read.push((path, file.strip_prefix(&lampos).unwrap_or(file).to_owned()));
        }
    }
    assert!(
        read.iter().any(|(path, _)| path == "docs/switches.md"),
        "the scan found what it should: {read:?}"
    );
    for (path, file) in &read {
        let left_out = if path.starts_with("builders/") {
            "!lampOS/builders/**"
        } else {
            "!lampOS/docs/**/*.md"
        };
        // Without the line that leaves it out, every change to it runs.
        let Some(out) = paths.iter().position(|named| named == left_out) else {
            continue;
        };
        let back = paths
            .iter()
            .rposition(|named| *named == format!("lampOS/{path}"));
        assert!(
            back.is_some_and(|back| back > out),
            "{} reads {path}, which the ARM64 workflow's pull_request paths leave out \
             ({left_out}) and don't name back after it: add '- 'lampOS/{path}'' below that \
             line in {WORKFLOW}",
            file.display()
        );
    }
}

#[test]
fn the_scan_sees_paths_in_strings_and_not_in_comments_or_prose() {
    let text = r#"
        let page = include_str!("../../../docs/install.md");
        let other = fs::read_to_string(root.join("docs/verification/x/README.md"));
        // See docs/memory.md for why.
        #[ignore = "requires the pinned model; see docs/endpoint-pilot.md"]
        let role = Path::new("../builders/tests.md");
        let picture = include_bytes!("../../../docs/verification/x/face.png");
    "#;
    assert_eq!(
        named_in(text),
        [
            "docs/install.md",
            "docs/verification/x/README.md",
            "builders/tests.md"
        ]
    );
    let workflow = "on:\n  pull_request:\n    paths:\n      - 'lampOS/**'\n      - '!lampOS/docs/**/*.md'\n      - 'lampOS/docs/install.md'\n  workflow_dispatch:\n";
    assert_eq!(
        pull_request_paths(workflow),
        [
            "lampOS/**",
            "!lampOS/docs/**/*.md",
            "lampOS/docs/install.md"
        ]
    );
}
