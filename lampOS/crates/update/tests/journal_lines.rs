//! What a round writes to the journal, as `docs/updates.md`'s "Reading a
//! round from the journal" gives it (#4598): run sheets grep these words
//! (#4561, #4370), so a change to one of them fails here first, not on a
//! lamp. The new updater's confirm line is `main.rs`'s, pinned by
//! `self_cli_exec` on Linux.

use lamp_update::manifest::{File, TooOld};
use lamp_update::records::Records;
use lamp_update::round::Told;
use lamp_update::{Outcome, walk};
use std::{fs, path::Path};

const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NEW: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn said(outcome: Outcome) -> (String, &'static str) {
    let kind = outcome.kind();
    (outcome.to_string(), kind)
}

#[test]
fn every_round_end_says_the_words_the_table_gives_beside_its_name() {
    let rows = [
        (
            Outcome::UpToDate(8925),
            "up to date: build 8925",
            "up-to-date",
        ),
        (
            Outcome::Unreachable("timed out".into()),
            "nothing changed: ",
            "unreachable",
        ),
        (
            Outcome::NothingPublished,
            "nothing changed: ",
            "nothing-published",
        ),
        (
            Outcome::Refused("why".into()),
            "refused, nothing switched: ",
            "refused",
        ),
        (
            Outcome::NotQuiet("why".into()),
            "not switched: ",
            "not-quiet",
        ),
        (
            Outcome::Switched(8925),
            "switched to build 8925; every part is well",
            "switched",
        ),
        (
            Outcome::SelfSwitched {
                sequence: 8925,
                from: OLD.into(),
                to: NEW.into(),
            },
            "build 8925 runs; lamp-update switches from its build aaaaaaaaaaaa to bbbbbbbbbbbb, which must confirm itself",
            "self-switched",
        ),
        (
            Outcome::Climbs {
                sequence: 8925,
                needed: 9,
                from: OLD.into(),
                to: NEW.into(),
            },
            "build 8925 needs updater 9; lamp-update switches from its build aaaaaaaaaaaa to bbbbbbbbbbbb, which the build carries, and takes nothing else of it until that one has confirmed itself",
            "self-switched",
        ),
        (Outcome::Failed("why".into()), "failed: ", "failed"),
    ];
    for (outcome, begins, name) in rows {
        let (text, kind) = said(outcome);
        assert!(text.starts_with(begins), "{text:?} begins {begins:?}");
        assert_eq!(kind, name, "{text}");
        // The name is the log's and the status's, never the journal's.
        assert!(!text.contains("self-switched") && !text.contains("self-confirmed"));
    }
    let told = || Told {
        new: String::new(),
        old: String::new(),
    };
    for (after, ends) in [
        (Ok(()), "; the previous build is well"),
        (
            Err("why".to_owned()),
            "; the previous build is not well either: why",
        ),
    ] {
        let (text, kind) = said(Outcome::RolledBack {
            sequence: 8925,
            why: "lamp-voice failed".into(),
            after,
            told: told(),
        });
        assert!(text.starts_with("build 8925 put back: "), "{text}");
        assert!(text.ends_with(ends), "{text}");
        assert_eq!(kind, "rolled-back");
    }
}

/// The refusal that means no climb, in both its endings, and the climb's
/// own line never says it.
#[test]
fn a_build_for_a_newer_updater_is_refused_in_the_words_that_mean_no_climb() {
    let too_old = |carried: Option<File>| TooOld {
        channel: Some("lamp-voice".into()),
        sequence: Some(8925),
        needed: 9,
        updater: 8,
        files_url: None,
        carried,
    };
    let carried = File {
        name: "lamp-update".into(),
        sha256: NEW.into(),
        size: 1,
    };
    assert_eq!(
        too_old(Some(carried)).why(),
        "build 8925 needs updater 9 or newer, and this is updater 8; waiting for a newer updater"
    );
    assert_eq!(
        too_old(None).why(),
        "build 8925 needs updater 9 or newer, and this is updater 8; it carries no lamp-update, so waiting for a build that does"
    );
}

/// `not switched: …`'s reasons: the walk's hold and the lock.
#[test]
fn a_round_not_switched_says_the_walk_or_the_lock_as_the_table_gives_them() {
    assert_eq!(
        walk::held(3),
        "the setup walk is under way: held, round 3 of 8"
    );
    let folder = tempfile::tempdir().unwrap();
    let records = Records::new(folder.path().to_owned());
    let _held = records.lock().unwrap();
    let Err(error) = records.lock() else {
        panic!("the lock was taken twice");
    };
    assert_eq!(error.to_string(), "another lamp-update is at work");
}

/// The words of a `self.say(` call at `at` in `source`: its string literal,
/// directly or as `format!`'s, or none for a line built before the call
/// (`self.say(&line)`), which the table names by hand.
fn said_words(source: &str, at: usize) -> Option<String> {
    let rest = source[at + "self.say(".len()..].trim_start();
    let rest = rest.strip_prefix("&format!(").unwrap_or(rest).trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut words = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(words),
            '\\' => match chars.next()? {
                // A line continued: the newline and the next line's indent.
                '\n' => {
                    let tail = chars.as_str().trim_start();
                    chars = tail.chars();
                }
                escaped => words.push(escaped),
            },
            c => words.push(c),
        }
    }
    None
}

/// Every line a round says with words of its own (`Round::say` in
/// `round.rs`) has a row in `docs/updates.md`'s "Every line a round says":
/// the row holds its longest fixed piece, the words between its
/// placeholders. Run sheets grep these lines on lamps (#4672, #4680), and on
/// 31 Oct 34 of the round's 41 had no row.
#[test]
fn every_line_a_round_says_has_a_row_in_updates_md() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = fs::read_to_string(crate_dir.join("src/round.rs")).unwrap();
    let docs = fs::read_to_string(crate_dir.join("../../docs/updates.md")).unwrap();
    let table = docs
        .split_once("### Every line a round says")
        .map(|(_, after)| after.split("\n### ").next().unwrap_or(after))
        .expect("updates.md has \"Every line a round says\"");
    let mut lines = 0;
    let mut unnamed = Vec::new();
    for (at, _) in source.match_indices("self.say(") {
        let Some(words) = said_words(&source, at) else {
            continue;
        };
        lines += 1;
        let piece = words
            .split(['{', '}'])
            .step_by(2)
            .map(|piece| piece.trim_matches(|c: char| c.is_whitespace() || ",;:".contains(c)))
            .max_by_key(|piece| piece.len())
            .unwrap_or_default();
        if !table.contains(piece) {
            unnamed.push(words);
        }
    }
    assert!(lines >= 40, "only {lines} said lines found in round.rs");
    assert!(
        unnamed.is_empty(),
        "updates.md's \"Every line a round says\" has no row for {unnamed:#?}"
    );
}
