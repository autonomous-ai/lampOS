use super::*;
use crate::own::{self, BINARY, CURRENT, FOLDER};
use std::os::unix::fs::{PermissionsExt, symlink};

const BUILD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NOW: u64 = 1_800_000_000_000;

struct Lamp {
    root: tempfile::TempDir,
    records: PathBuf,
}

/// A lamp running build BUILD of lamp-update, holding "new", and the
/// writer at its fixed path holding "old".
fn lamp() -> Lamp {
    let root = tempfile::tempdir().unwrap();
    let update = on_root(root.path(), FOLDER);
    for (build, bytes) in [(BUILD, "new"), (OTHER, "old")] {
        fs::create_dir_all(update.join(build)).unwrap();
        fs::write(update.join(build).join(BINARY), bytes).unwrap();
    }
    symlink(BUILD, on_root(root.path(), CURRENT)).unwrap();
    let back = on_root(root.path(), BACK);
    fs::create_dir_all(back.parent().unwrap()).unwrap();
    fs::write(&back, "old").unwrap();
    fs::set_permissions(&back, fs::Permissions::from_mode(0o755)).unwrap();
    let records = root.path().join("var/lib/lampos-update");
    fs::create_dir_all(&records).unwrap();
    Lamp { root, records }
}

fn good(path: &Path) -> Result<String, String> {
    Ok(fixed_copy::answer(&sha256_hex(&fs::read(path).unwrap())))
}

fn writer(lamp: &Lamp) -> String {
    fs::read_to_string(on_root(lamp.root.path(), BACK)).unwrap()
}

fn prev(lamp: &Lamp) -> Option<String> {
    fs::read_to_string(on_root(lamp.root.path(), PREV)).ok()
}

#[test]
fn a_round_asks_only_when_the_writer_is_not_the_running_build_and_it_is_ready() {
    let lamp = lamp();
    let held = Records::new(lamp.records.clone()).lock().unwrap();
    assert!(ask(lamp.root.path(), &held, b"new", NOW).unwrap());
    let request: Request =
        serde_json::from_slice(&fs::read(lamp.records.join(REQUEST)).unwrap()).unwrap();
    assert_eq!(
        request,
        Request {
            build: sha256_hex(b"new"),
            at_unix_ms: NOW,
            op: Op::Refresh
        }
    );
    // The writer already the running build: nothing asked.
    fs::remove_file(lamp.records.join(REQUEST)).unwrap();
    fs::write(on_root(lamp.root.path(), BACK), "new").unwrap();
    assert!(!ask(lamp.root.path(), &held, b"new", NOW).unwrap());
    assert!(!lamp.records.join(REQUEST).exists());
    // A switch of lamp-update's own waiting: not ready, nothing asked.
    fs::write(on_root(lamp.root.path(), BACK), "old").unwrap();
    own::begin(&lamp.records, OTHER, BUILD).unwrap();
    let why = ask(lamp.root.path(), &held, b"new", NOW).unwrap_err();
    assert!(why.contains("waits to be confirmed"), "{why}");
    assert!(!lamp.records.join(REQUEST).exists());
}

#[test]
fn the_units_program_acts_on_the_rounds_fresh_request_while_the_round_holds_the_lock() {
    let lamp = lamp();
    let held = Records::new(lamp.records.clone()).lock().unwrap();
    assert!(ask(lamp.root.path(), &held, b"new", NOW).unwrap());
    let done = refresh(
        lamp.root.path(),
        &lamp.records,
        b"new",
        &|| NOW + 1_000,
        &mut good,
    )
    .unwrap();
    assert_eq!(writer(&lamp), "new");
    assert_eq!(prev(&lamp).as_deref(), Some("old"));
    assert_eq!(
        done,
        Done {
            build: sha256_hex(b"new"),
            before: Some(sha256_hex(b"old")),
            synced: true,
            at_unix_ms: NOW + 1_000,
            proven: false,
            put_back: None,
            for_good: false,
        }
    );
    assert_eq!(super::done(&lamp.records), Some(done));
    assert!(!lamp.records.join(REQUEST).exists());
    drop(held);
}

#[test]
fn a_refresh_is_timed_once_the_copy_is_in_place_not_when_asked() {
    // The probe stands in for the place's writes, syncs and probe, which
    // may take seconds: a `back` the timer begins in them is still the copy
    // before's, so the refresh is timed after (Reviewer-three's P3 on
    // #4190).
    let lamp = lamp();
    let held = Records::new(lamp.records.clone()).lock().unwrap();
    assert!(ask(lamp.root.path(), &held, b"new", NOW).unwrap());
    let now = std::cell::Cell::new(NOW + 1_000);
    let mut slow = |path: &Path| {
        now.set(now.get() + 1_200);
        good(path)
    };
    let done = refresh(
        lamp.root.path(),
        &lamp.records,
        b"new",
        &|| now.get(),
        &mut slow,
    )
    .unwrap();
    assert_eq!(writer(&lamp), "new");
    assert_eq!(done.at_unix_ms, NOW + 2_200);
    drop(held);
}

#[test]
fn with_the_lock_held_and_no_fresh_request_for_these_bytes_nothing_is_replaced() {
    let lamp = lamp();
    let _held = Records::new(lamp.records.clone()).lock().unwrap();
    let run = |lamp: &Lamp, at: u64| {
        refresh(lamp.root.path(), &lamp.records, b"new", &|| at, &mut good).unwrap_err()
    };
    // No request: another lamp-update at work asked nothing.
    assert!(run(&lamp, NOW).contains("asked nothing"));
    // A request for other bytes.
    let other = Request {
        build: sha256_hex(b"other"),
        at_unix_ms: NOW,
        op: Op::Refresh,
    };
    fs::write(
        lamp.records.join(REQUEST),
        serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    assert!(run(&lamp, NOW).contains("another"));
    // A stale one, and one from the future.
    for at in [NOW - FRESH.as_millis() as u64 - 1, NOW + 1] {
        let stale = Request {
            build: sha256_hex(b"new"),
            at_unix_ms: at,
            op: Op::Refresh,
        };
        fs::write(
            lamp.records.join(REQUEST),
            serde_json::to_vec(&stale).unwrap(),
        )
        .unwrap();
        assert!(run(&lamp, NOW).contains("not fresh"));
    }
    assert_eq!(writer(&lamp), "old");
    assert_eq!(prev(&lamp), None);
    assert!(super::done(&lamp.records).is_none());
}

#[test]
fn by_hand_with_no_round_at_work_it_asks_ready_itself_under_the_lock() {
    let lamp = lamp();
    let done = refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
    assert_eq!(done.before, Some(sha256_hex(b"old")));
    assert_eq!(writer(&lamp), "new");
    // Not the build update/current names: not ready, nothing replaced.
    let lamp = super::tests::lamp();
    let why = refresh(
        lamp.root.path(),
        &lamp.records,
        b"stale",
        &|| NOW,
        &mut good,
    )
    .unwrap_err();
    assert!(why.contains("not the build"), "{why}");
    assert_eq!(writer(&lamp), "old");
    // A copy that does not answer: nothing replaced either.
    let mut bad = |_: &Path| Err("exit status: 1".to_owned());
    let why = refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut bad).unwrap_err();
    assert!(why.contains("nothing was replaced"), "{why}");
    assert_eq!(writer(&lamp), "old");
}

/// The round's request for `op`, then the unit's program run on it.
fn op(lamp: &Lamp, op: Op, at: u64) -> Result<Done, String> {
    let held = Records::new(lamp.records.clone()).lock().unwrap();
    request(&held, b"new", op, at).unwrap();
    let done = refresh(lamp.root.path(), &lamp.records, b"new", &|| at, &mut good);
    drop(held);
    done
}

#[test]
fn a_new_writer_whose_back_ended_well_since_is_proved_and_its_prev_removed() {
    let lamp = lamp();
    refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
    assert!(unproven(lamp.root.path(), &lamp.records));
    let done = super::done(&lamp.records).unwrap();
    // A `back` before the refresh, or by another writer, proves nothing.
    note_back(&lamp.records, b"new", NOW - 1).unwrap();
    assert!(!proved(&lamp.records, &done));
    note_back(&lamp.records, b"old", NOW + 1).unwrap();
    assert!(!proved(&lamp.records, &done));
    note_back(&lamp.records, b"new", NOW + 1).unwrap();
    assert!(proved(&lamp.records, &done));
    let proven = op(&lamp, Op::Proven, NOW + 2).unwrap();
    assert!(proven.proven);
    assert_eq!(prev(&lamp), None);
    assert_eq!(writer(&lamp), "new");
    assert!(!unproven(lamp.root.path(), &lamp.records));
}

#[test]
fn a_new_writer_that_failed_is_put_back_and_never_asked_again() {
    let lamp = lamp();
    refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
    let done = op(&lamp, Op::PutBack, NOW + 1).unwrap();
    assert_eq!(writer(&lamp), "old");
    assert_eq!(prev(&lamp), None);
    assert_eq!(done.build, sha256_hex(b"old"));
    assert_eq!(done.put_back, Some(sha256_hex(b"new")));
    assert!(!unproven(lamp.root.path(), &lamp.records));
    let held = Records::new(lamp.records.clone()).lock().unwrap();
    assert!(!ask(lamp.root.path(), &held, b"new", NOW + 2).unwrap());
    // A later build of lamp-update may be asked.
    fs::write(
        on_root(lamp.root.path(), FOLDER).join(BUILD).join(BINARY),
        "newer",
    )
    .unwrap();
    assert!(ask(lamp.root.path(), &held, b"newer", NOW + 2).unwrap());
    // Refreshed, it is unproven, though the put-back of "new" is carried
    // forward (Reviewer-two's P2 on #4178), and it is proved as any
    // refresh is.
    let done = refresh(
        lamp.root.path(),
        &lamp.records,
        b"newer",
        &|| NOW + 3,
        &mut good,
    )
    .unwrap();
    assert_eq!(done.put_back, Some(sha256_hex(b"new")));
    assert_eq!(writer(&lamp), "newer");
    assert_eq!(prev(&lamp).as_deref(), Some("old"));
    assert!(unproven(lamp.root.path(), &lamp.records));
    note_back(&lamp.records, b"newer", NOW + 4).unwrap();
    assert!(proved(&lamp.records, &done));
    request(&held, b"newer", Op::Proven, NOW + 5).unwrap();
    let proven = refresh(
        lamp.root.path(),
        &lamp.records,
        b"newer",
        &|| NOW + 5,
        &mut good,
    )
    .unwrap();
    assert!(proven.proven);
    assert_eq!(prev(&lamp), None);
    assert!(!unproven(lamp.root.path(), &lamp.records));
}

#[test]
fn nothing_is_unproven_before_a_refresh_or_when_the_writer_was_already_the_build() {
    let lamp = lamp();
    assert!(!unproven(lamp.root.path(), &lamp.records));
    fs::write(on_root(lamp.root.path(), BACK), "new").unwrap();
    let done = refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
    assert_eq!(done.before, None);
    assert!(!unproven(lamp.root.path(), &lamp.records));
}

/// lamp-06c1's record (#5539), as a lamp-update from before #5539 wrote it
/// when the first apply after a refresh failed: the copy before is the
/// writer again, and the running build is named as put back, with no word
/// on whether it was for good.
fn put_back_before_5539(lamp: &Lamp) {
    let record = serde_json::json!({
        "build": sha256_hex(b"old"),
        "before": sha256_hex(b"new"),
        "synced": true,
        "at_unix_ms": NOW,
        "proven": true,
        "put_back": sha256_hex(b"new"),
    });
    fs::write(lamp.records.join(DONE), record.to_string()).unwrap();
}

#[test]
fn a_new_writer_whose_first_apply_failed_is_put_back_and_asked_again() {
    let lamp = lamp();
    refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
    let done = op(&lamp, Op::PutBackForApply, NOW + 1).unwrap();
    assert_eq!(writer(&lamp), "old");
    assert_eq!(prev(&lamp), None);
    assert_eq!(done.build, sha256_hex(b"old"));
    assert_eq!(done.before, Some(sha256_hex(b"new")));
    assert_eq!(done.put_back, None);
    assert!(!unproven(lamp.root.path(), &lamp.records));
    let held = Records::new(lamp.records.clone()).lock().unwrap();
    assert!(ask(lamp.root.path(), &held, b"new", NOW + 2).unwrap());
}

#[test]
fn a_put_back_for_an_apply_carries_a_build_put_back_before_forward() {
    let lamp = lamp();
    refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
    let mut done = super::done(&lamp.records).unwrap();
    done.put_back = Some(sha256_hex(b"older"));
    done.for_good = true;
    fs::write(lamp.records.join(DONE), serde_json::to_vec(&done).unwrap()).unwrap();
    let done = op(&lamp, Op::PutBackForApply, NOW + 1).unwrap();
    assert_eq!(done.put_back, Some(sha256_hex(b"older")));
    assert!(done.for_good);
}

#[test]
fn a_build_put_back_by_a_record_from_before_5539_is_asked_once_more_then_never_again() {
    for again in [Op::PutBackForApply, Op::PutBack] {
        let lamp = lamp();
        put_back_before_5539(&lamp);
        let held = Records::new(lamp.records.clone()).lock().unwrap();
        // Offered once more.
        assert!(ask(lamp.root.path(), &held, b"new", NOW + 1).unwrap());
        drop(held);
        let refreshed = op(&lamp, Op::Refresh, NOW + 2).unwrap();
        assert_eq!(writer(&lamp), "new");
        assert_eq!(prev(&lamp).as_deref(), Some("old"));
        assert!(unproven(lamp.root.path(), &lamp.records));
        assert_eq!(refreshed.put_back, Some(sha256_hex(b"new")));
        assert!(refreshed.for_good);
        // Put back again, whatever for: never asked again.
        op(&lamp, again, NOW + 3).unwrap();
        assert_eq!(writer(&lamp), "old");
        let held = Records::new(lamp.records.clone()).lock().unwrap();
        assert!(
            !ask(lamp.root.path(), &held, b"new", NOW + 4).unwrap(),
            "{again:?}"
        );
    }
}

/// lamp-52e6's state (#5539): a `.prev` no record pairs with, another copy
/// than the writer in place, and no `writer-refresh.done` at all.
fn prev_no_record_pairs_with(lamp: &Lamp) {
    let path = on_root(lamp.root.path(), PREV);
    fs::write(&path, "oldest").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(super::done(&lamp.records).is_none());
}

#[test]
fn a_prev_no_record_pairs_with_is_cleared_once_the_writer_in_place_has_proved_itself() {
    let lamp = lamp();
    prev_no_record_pairs_with(&lamp);
    note_back(&lamp.records, b"old", NOW - 1_000).unwrap();
    let done = op(&lamp, Op::Refresh, NOW).unwrap();
    assert_eq!(writer(&lamp), "new");
    assert_eq!(prev(&lamp).as_deref(), Some("old"));
    assert_eq!(done.before, Some(sha256_hex(b"old")));
    assert!(unproven(lamp.root.path(), &lamp.records));
}

#[test]
fn a_prev_no_record_pairs_with_is_kept_while_the_writer_in_place_has_not_proved_itself() {
    // No `back` noted, or one by another writer.
    for noted in [None, Some(&b"oldest"[..]), Some(&b"new"[..])] {
        let lamp = lamp();
        prev_no_record_pairs_with(&lamp);
        if let Some(own) = noted {
            note_back(&lamp.records, own, NOW - 1_000).unwrap();
        }
        let why = op(&lamp, Op::Refresh, NOW).unwrap_err();
        assert!(why.contains("no refresh in the records kept it"), "{why}");
        assert!(why.contains("nothing was replaced"), "{why}");
        assert_eq!(writer(&lamp), "old");
        assert_eq!(prev(&lamp).as_deref(), Some("oldest"));
    }
}

#[test]
fn a_prev_an_unproven_refresh_kept_is_never_cleared() {
    // A refresh not proved yet pairs with its `.prev`: even with the writer
    // in place noted, that `.prev` may be the only good copy.
    let lamp = lamp();
    refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
    note_back(&lamp.records, b"new", NOW + 1).unwrap();
    let held = Records::new(lamp.records.clone()).lock().unwrap();
    request(&held, b"newer", Op::Refresh, NOW + 2).unwrap();
    let why = refresh(
        lamp.root.path(),
        &lamp.records,
        b"newer",
        &|| NOW + 2,
        &mut good,
    )
    .unwrap_err();
    drop(held);
    assert!(why.contains("holds another copy"), "{why}");
    assert_eq!(writer(&lamp), "new");
    assert_eq!(prev(&lamp).as_deref(), Some("old"));
}

/// What [`forget_writer`] leaves: whether PREV, the request and the record
/// are there, in that order.
fn left(lamp: &Lamp) -> [bool; 3] {
    [
        fs::symlink_metadata(on_root(lamp.root.path(), PREV)).is_ok(),
        lamp.records.join(REQUEST).exists(),
        lamp.records.join(DONE).exists(),
    ]
}

#[test]
fn forget_writer_clears_what_is_there_with_any_of_its_files_missing() {
    for there in 0..8u8 {
        let lamp = lamp();
        let prev = on_root(lamp.root.path(), PREV);
        if there & 4 != 0 {
            refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
        }
        if there & 1 != 0 {
            fs::write(&prev, "oldest").unwrap();
        } else if prev.exists() {
            fs::remove_file(&prev).unwrap();
        }
        if there & 2 != 0 {
            let held = Records::new(lamp.records.clone()).lock().unwrap();
            request(&held, b"new", Op::Proven, NOW).unwrap();
        }
        let build = sha256_hex(b"newer");
        let acted = forget_writer(lamp.root.path(), &lamp.records, &build, true, NOW).unwrap();
        assert_eq!(acted, there != 0, "{there}");
        assert_eq!(left(&lamp), [false, false, false], "{there}");
        // Run again, it changes nothing.
        assert!(!forget_writer(lamp.root.path(), &lamp.records, &build, true, NOW).unwrap());
    }
}

#[test]
fn forget_writer_leaves_all_as_it_is_when_the_writer_is_unchanged_and_named_by_the_record() {
    let lamp = lamp();
    refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
    let before = super::done(&lamp.records);
    let build = sha256_hex(b"new");
    assert!(!forget_writer(lamp.root.path(), &lamp.records, &build, false, NOW + 1).unwrap());
    assert_eq!(left(&lamp), [true, false, true]);
    assert_eq!(super::done(&lamp.records), before);
    assert!(unproven(lamp.root.path(), &lamp.records));
}

#[test]
fn forget_writer_acts_on_unchanged_bytes_when_the_record_names_another_build() {
    // A run cut after it placed the writer and before it cleared.
    let lamp = lamp();
    refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
    let build = sha256_hex(b"newer");
    assert!(forget_writer(lamp.root.path(), &lamp.records, &build, false, NOW + 1).unwrap());
    assert_eq!(left(&lamp), [false, false, false]);
    assert!(!unproven(lamp.root.path(), &lamp.records));
}

#[test]
fn forget_writer_keeps_a_build_put_back_in_a_fresh_proven_record() {
    for for_good in [true, false] {
        let lamp = lamp();
        if for_good {
            refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
            op(&lamp, Op::PutBack, NOW + 1).unwrap();
        } else {
            // A record from before #5539 carries as it is: for_good unset.
            put_back_before_5539(&lamp);
            fs::write(on_root(lamp.root.path(), PREV), "oldest").unwrap();
        }
        let build = sha256_hex(b"newer");
        assert!(forget_writer(lamp.root.path(), &lamp.records, &build, true, NOW + 2).unwrap());
        assert_eq!(left(&lamp), [false, false, true]);
        assert_eq!(
            super::done(&lamp.records),
            Some(Done {
                build: build.clone(),
                before: None,
                synced: true,
                at_unix_ms: NOW + 2,
                proven: true,
                put_back: Some(sha256_hex(b"new")),
                for_good,
            })
        );
        assert!(!unproven(lamp.root.path(), &lamp.records));
        assert_eq!(
            put_back_for_good(&lamp.records, &sha256_hex(b"new")),
            for_good
        );
        // Run again, the writer now this build, it changes nothing.
        let kept = super::done(&lamp.records);
        assert!(!forget_writer(lamp.root.path(), &lamp.records, &build, false, NOW + 3).unwrap());
        assert_eq!(super::done(&lamp.records), kept);
    }
}

#[test]
fn forget_writer_that_cannot_remove_the_prev_says_so_and_keeps_the_record() {
    let lamp = lamp();
    refresh(lamp.root.path(), &lamp.records, b"new", &|| NOW, &mut good).unwrap();
    let prev = on_root(lamp.root.path(), PREV);
    fs::remove_file(&prev).unwrap();
    fs::create_dir_all(prev.join("inside")).unwrap();
    let held = Records::new(lamp.records.clone()).lock().unwrap();
    request(&held, b"new", Op::Proven, NOW).unwrap();
    drop(held);
    let why = forget_writer(
        lamp.root.path(),
        &lamp.records,
        &sha256_hex(b"newer"),
        true,
        NOW,
    )
    .unwrap_err();
    assert!(why.contains("lamp-update-back.prev"), "{why}");
    assert_eq!(left(&lamp), [true, true, true]);
}
