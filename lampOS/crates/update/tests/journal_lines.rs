//! What a round writes to the journal, as `docs/updates.md`'s "Reading a
//! round from the journal" gives it (#4598): run sheets grep these words
//! (#4561, #4370), so a change to one of them fails here first, not on a
//! lamp. The new updater's confirm line is `main.rs`'s, pinned by
//! `self_cli_exec` on Linux.

use lamp_update::manifest::{File, TooOld};
use lamp_update::records::Records;
use lamp_update::round::Told;
use lamp_update::{Outcome, walk};

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
