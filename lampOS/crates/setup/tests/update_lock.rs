//! Setup's helpers under lamp-update's lock (#4490, #4485, the app's restart
//! and shutdown, lamp-control's developer setting), in a binary of their
//! own. Each test lets go of the lock and has it taken again at once, by
//! itself or by the helper. In the `it`
//! binary, that second take failed now and then with "another lamp-update is
//! at work" (the `change::` set failed 3 of 4 runs on this Mac for
//! Reviewer-two and 4 of 4 for Tests, #3997, 30 Oct). Third measured why
//! (13:57): macOS's `flock(LOCK_EX|LOCK_NB)` turned down a lock nobody held,
//! with no holder in `lsof`, while the binary's other tests were busy with
//! files, and never alone; with no test starting a process it still failed 8
//! of 10. Cargo runs
//! test binaries one at a time, so here they run with nothing beside them.
//! Lamps and CI are Linux, and ARM64 hasn't shown it.
#[path = "it/change_rig.rs"]
mod change_rig;

use change_rig::{NEXT, Root, S, Script};
use lamp_msgs::Voice;
use lamp_setup::change::{self, answer};
use lamp_update::quiet::Heard;
use serde_json::json;
use std::{fs, path::PathBuf, sync::atomic::Ordering, time::Duration};

/// A zone's `lamp-schedule` restart waits for the quiet and then for no
/// update at work, holding lamp-update's lock through the restart: a round
/// puts its build back when a unit it restarted changes process in its
/// watch (`updates.md` step 9). Free, the lock comes after the quiet; held
/// all along, given up at the bound with why; let go meanwhile, taken after
/// the quiet again.
#[test]
fn a_zone_restart_waits_for_no_update_and_holds_its_lock() {
    use std::{cell::RefCell, rc::Rc};
    let folder = std::env::temp_dir().join(format!(
        "lsc-unlocked-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let records = lamp_update::records::Records::new(folder.clone());
    let resting =
        || -> Box<dyn Fn(u64) -> Vec<Heard>> { Box::new(|_| vec![Heard::State(Voice::Resting)]) };
    // No update: the lock after 45 s of quiet, held while the caller acts.
    let mut ears = Script {
        now: 0,
        at: resting(),
    };
    let held = change::quiet_and_unlocked(&mut ears, &records, change::QUIET_MOST).unwrap();
    assert_eq!(ears.now, 45 * S);
    assert!(records.lock().is_err());
    drop(held);
    // An update holding it all along: given up at the bound, with why.
    let update = records.lock().unwrap();
    let mut ears = Script {
        now: 0,
        at: resting(),
    };
    let Err(why) = change::quiet_and_unlocked(&mut ears, &records, Duration::from_secs(120)) else {
        panic!("taken while an update held it");
    };
    assert!(why.contains("update"), "{why}");
    assert!(ears.now >= 120 * S, "{}", ears.now);
    // Let go at 70 s: quiet at 45 s, the lock held, looked at again 10 s
    // later after another 45 s of quiet, at 100 s.
    let update = Rc::new(RefCell::new(Some(update)));
    let letting = Rc::clone(&update);
    let mut ears = Script {
        now: 0,
        at: Box::new(move |now| {
            if now >= 70 * S {
                letting.borrow_mut().take();
            }
            vec![Heard::State(Voice::Resting)]
        }),
    };
    let held = change::quiet_and_unlocked(&mut ears, &records, change::QUIET_MOST).unwrap();
    assert_eq!(ears.now, 100 * S);
    assert!(update.borrow().is_none());
    drop(held);
    let _ = fs::remove_dir_all(folder);
}

/// The app's restart and shutdown (#2193, #3697): off, refused as every
/// change is; on, answered first, holding lamp-update's lock to act on;
/// refused with why while an update holds it, and nothing to act on. A
/// line that asks neither is a change, not a power request.
#[test]
fn a_restart_or_shutdown_is_answered_first_and_refused_under_an_update() {
    use lamp_keeper::restart::{INSTALLING, REQUEST, SHUTDOWN_REQUEST};
    let folder = std::env::temp_dir().join(format!(
        "lsc-power-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let records = lamp_update::records::Records::new(folder.clone());
    assert!(change::power(r#"{"person_name":"Lan"}"#, true, &records).is_none());
    assert!(change::power(r#"{"shutdown":"machine"}"#, true, &records).is_none());
    for (line, power, done) in [
        (REQUEST, change::Power::Restart, json!({"restarting": true})),
        (
            SHUTDOWN_REQUEST,
            change::Power::Shutdown,
            json!({"shutting_down": true}),
        ),
    ] {
        let (asked, reply, lock) = change::power(line, false, &records).unwrap();
        assert_eq!(asked, power);
        assert_eq!(reply, json!({"refused": change::OFF}));
        assert!(lock.is_none());
        let held = records.lock().unwrap();
        let (_, reply, lock) = change::power(line, true, &records).unwrap();
        assert_eq!(reply, json!({ "refused": INSTALLING }), "{line}");
        assert!(lock.is_none());
        drop(held);
        let (_, reply, lock) = change::power(line, true, &records).unwrap();
        assert_eq!(reply, json!({ "done": done }), "{line}");
        // Held while it acts: an update that starts meanwhile can't have it.
        assert!(lock.is_some());
        assert!(records.lock().is_err());
    }
    let _ = fs::remove_dir_all(folder);
}

/// A clock for `answer_holding` under which nothing lets go of the lock.
fn idle() -> Script {
    Script {
        now: 0,
        at: Box::new(|_| Vec::new()),
    }
}

/// Fourth's P2 on #4485: a language takes lamp-update's lock before it is
/// written. Held past the wait, it is refused with words that fit an update
/// and a change before it alike (#5394), and the file is untouched;
/// otherwise the lock is held for the voice's restart and let go after it,
/// and the same language again holds none.
#[test]
fn a_language_takes_the_update_lock_before_it_is_written() {
    let root = Root::new();
    let config = root.config();
    let records = lamp_update::records::Records::new(root.0.join("update"));
    let held = records.lock().unwrap();
    let mut ears = idle();
    let (reply, done, lock) =
        change::answer_holding(r#"{"language": "ja"}"#, &config, true, &records, &mut ears);
    assert_eq!(reply, json!({ "refused": change::STILL_APPLYING }));
    assert_eq!(
        ears.now,
        change::CLOSE_LOCK_MOST.as_millis() as u64,
        "waited first"
    );
    assert!(done.is_none() && lock.is_none());
    assert!(!config.language.exists(), "nothing written");
    // Other changes don't wait on it.
    let mut ears = idle();
    let (reply, _, lock) = change::answer_holding(
        r#"{"person_name": "Lan"}"#,
        &config,
        true,
        &records,
        &mut ears,
    );
    assert_eq!(reply, json!({"done": {"name": "Lan"}}));
    assert!(lock.is_none());
    assert_eq!(ears.now, 0);
    drop(held);
    let (reply, done, lock) = change::answer_holding(
        r#"{"language": "ja"}"#,
        &config,
        true,
        &records,
        &mut idle(),
    );
    assert_eq!(reply, json!({"done": {"language": "ja"}}));
    assert!(done.unwrap().restart_voice);
    assert!(lock.is_some(), "held through the restart");
    assert!(records.lock().is_err(), "no update starts meanwhile");
    drop(lock);
    assert!(records.lock().is_ok(), "let go after the restart");
    let (_, done, lock) = change::answer_holding(
        r#"{"language": "ja"}"#,
        &config,
        true,
        &records,
        &mut idle(),
    );
    assert!(!done.unwrap().restart_voice);
    assert!(lock.is_none());
    assert!(records.lock().is_ok(), "the same language holds nothing");
    // A bad language under no update: refused, nothing held.
    let (reply, _, lock) = change::answer_holding(
        r#"{"language": "fr"}"#,
        &config,
        true,
        &records,
        &mut idle(),
    );
    assert_eq!(
        reply,
        json!({"refused": "language must be one of en, vi, ja, zh, ko"})
    );
    assert!(lock.is_none());
    assert_eq!(fs::read_to_string(&config.language).unwrap(), "ja\n");
}

/// #5394, seen on a lamp: a voice change held the lock 38 s while the voice
/// came to rest, and the changes made meanwhile were refused as if an update
/// were installing. One made while the last one holds it waits for it and
/// then applies, so the app hears `success`.
#[test]
fn a_change_made_while_the_last_one_holds_the_lock_waits_and_applies() {
    use std::{cell::RefCell, rc::Rc};
    let root = Root::new();
    let config = root.config();
    let records = lamp_update::records::Records::new(root.0.join("update"));
    let (_, done, first) = change::answer_holding(
        r#"{"voice": "Kanna"}"#,
        &config,
        true,
        &records,
        &mut idle(),
    );
    assert!(done.unwrap().restart_voice);
    assert!(first.is_some());
    // The first change's voice rests and restarts 3 s into the second's wait.
    let first = Rc::new(RefCell::new(first));
    let letting = Rc::clone(&first);
    let mut ears = Script {
        now: 0,
        at: Box::new(move |now| {
            if now >= 3 * S {
                letting.borrow_mut().take();
            }
            Vec::new()
        }),
    };
    let (reply, done, second) = change::answer_holding(
        r#"{"character": "calm"}"#,
        &config,
        true,
        &records,
        &mut ears,
    );
    assert_eq!(ears.now, 3 * S);
    assert!(first.borrow().is_none());
    assert!(reply.get("done").is_some(), "{reply}");
    assert!(done.unwrap().restart_voice);
    assert!(second.is_some(), "held through its own restart");
    assert_eq!(
        fs::read_to_string(change::beside_language(&config, "character")).unwrap(),
        "calm\n"
    );
}

fn records_folder(name: &str) -> (PathBuf, lamp_update::records::Records) {
    let folder = std::env::temp_dir().join(format!(
        "lsc-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    (folder.clone(), lamp_update::records::Records::new(folder))
}

/// systemd as the test sees it: each call, and whether lamp-update's lock
/// was held when it came; `stop` fails when told to.
struct Units<'a> {
    records: &'a lamp_update::records::Records,
    calls: Vec<(String, bool)>,
    stop_fails: bool,
}

impl change::Systemd for Units<'_> {
    fn systemctl(&mut self, args: &[&str]) -> Result<(), String> {
        let held = self.records.lock().is_err();
        self.calls.push((args.join(" "), held));
        if self.stop_fails && args[0] == "stop" {
            return Err("exit status: 1".into());
        }
        Ok(())
    }
}

/// lamp-update's lock alone, no quiet: a voice that never rests does not
/// hold it back; looked for every `every`, given up at the bound.
#[test]
fn control_waits_only_for_the_update_lock() {
    let (folder, records) = records_folder("ctl-unlocked");
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| vec![Heard::State(Voice::Speaking), Heard::Speech]),
    };
    let held = change::unlocked(
        &mut ears,
        &records,
        change::UPDATE_LOOK_EVERY,
        change::QUIET_MOST,
    )
    .unwrap();
    assert_eq!(ears.now, 0);
    drop(held);
    let update = records.lock().unwrap();
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| Vec::new()),
    };
    let Err(why) = change::unlocked(&mut ears, &records, S_1, Duration::from_secs(8)) else {
        panic!("taken while an update held it");
    };
    assert!(why.contains("update"), "{why}");
    assert_eq!(ears.now, 8 * S);
    drop(update);
    let _ = fs::remove_dir_all(folder);
}

const S_1: Duration = Duration::from_secs(1);

/// Off with the drop-in there: lamp-update's lock taken, the drop-in
/// removed, reload and stop under it, and only then the reply (returned
/// once they ran); a stop that fails is said, never claimed closed. Off
/// with none there is answered as any change, with no systemctl.
#[test]
fn off_holds_the_update_lock_and_stops_before_the_reply() {
    let root = Root::new();
    let config = root.config();
    let (folder, records) = records_folder("ctl-off");
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| Vec::new()),
    };
    let off = r#"{"control": false}"#;
    // None there: not this path.
    let mut units = Units {
        records: &records,
        calls: Vec::new(),
        stop_fails: false,
    };
    assert!(
        change::close_before_answer(off, &config, true, &records, &mut ears, &mut units).is_none()
    );
    assert!(units.calls.is_empty());
    answer(r#"{"control": true}"#, &config, true);
    // Without --enable, not this path either.
    assert!(
        change::close_before_answer(off, &config, false, &records, &mut ears, &mut units).is_none()
    );
    // On isn't this path.
    assert!(
        change::close_before_answer(
            r#"{"control": true}"#,
            &config,
            true,
            &records,
            &mut ears,
            &mut units
        )
        .is_none()
    );
    let (reply, done) =
        change::close_before_answer(off, &config, true, &records, &mut ears, &mut units).unwrap();
    // The reply exists only now, after both calls, each under the lock.
    assert_eq!(
        units.calls,
        vec![
            ("daemon-reload".to_owned(), true),
            ("stop lamp-control.service".to_owned(), true)
        ]
    );
    assert_eq!(reply, json!({"done": {"on": false, "closed": true}}));
    let done = done.unwrap();
    assert!(!done.restart_control && !done.restart_schedule);
    assert!(!config.control_dropin.exists());
    // Let go once answered.
    drop(records.lock().unwrap());
    // A stop that fails: still answered, saying the door may stay open.
    answer(r#"{"control": true}"#, &config, true);
    let mut failing = Units {
        records: &records,
        calls: Vec::new(),
        stop_fails: true,
    };
    let (reply, _) =
        change::close_before_answer(off, &config, true, &records, &mut ears, &mut failing).unwrap();
    assert_eq!(
        reply,
        json!({"done": {"on": false, "closed": false, "note": change::CLOSE_MAY_STAY_OPEN}})
    );
    assert!(!config.control_dropin.exists());
    let _ = fs::remove_dir_all(folder);
}

/// Off while an update holds the lock past about 8 s: refused, the drop-in
/// left in place, systemd not asked.
#[test]
fn off_while_an_update_holds_the_lock_refuses_and_keeps_the_drop_in() {
    let root = Root::new();
    let config = root.config();
    let (folder, records) = records_folder("ctl-busy");
    answer(r#"{"control": true}"#, &config, true);
    let update = records.lock().unwrap();
    let mut ears = Script {
        now: 0,
        at: Box::new(|_| Vec::new()),
    };
    let mut units = Units {
        records: &records,
        calls: Vec::new(),
        stop_fails: false,
    };
    let (reply, done) = change::close_before_answer(
        r#"{"control": false}"#,
        &config,
        true,
        &records,
        &mut ears,
        &mut units,
    )
    .unwrap();
    assert_eq!(reply, json!({"refused": change::CLOSE_INSTALLING}));
    assert_eq!(
        change::CLOSE_INSTALLING,
        "an update is installing: try again in a few minutes"
    );
    assert!(done.is_none());
    assert!(units.calls.is_empty());
    assert_eq!(ears.now, 8 * S);
    assert_eq!(
        fs::read_to_string(&config.control_dropin).unwrap(),
        change::CONTROL_DROPIN_TEXT
    );
    drop(update);
    let _ = fs::remove_dir_all(folder);
}

/// On: answered first (the drop-in written), then lamp-update's lock, with
/// no quiet, and reload and restart under it once an update lets it go.
#[test]
fn on_replies_then_restarts_once_unlocked() {
    use std::{cell::RefCell, rc::Rc};
    let root = Root::new();
    let config = root.config();
    let (folder, records) = records_folder("ctl-on");
    let (reply, done) = answer(r#"{"control": true}"#, &config, true);
    assert_eq!(reply, json!({"done": {"on": true}}));
    assert!(done.unwrap().restart_control);
    assert!(config.control_dropin.exists());
    // An update holds the lock until 25 s; the voice never rests.
    let update = Rc::new(RefCell::new(Some(records.lock().unwrap())));
    let letting = Rc::clone(&update);
    let mut ears = Script {
        now: 0,
        at: Box::new(move |now| {
            if now >= 25 * S {
                letting.borrow_mut().take();
            }
            vec![Heard::State(Voice::Speaking)]
        }),
    };
    let mut units = Units {
        records: &records,
        calls: Vec::new(),
        stop_fails: false,
    };
    let said = change::restart_control_when_unlocked(&mut ears, &records, &mut units);
    assert_eq!(said, "lamp-control restarted on the developer setting");
    // Looked at every 10 s: free at 30 s.
    assert_eq!(ears.now, 30 * S);
    assert_eq!(
        units.calls,
        vec![
            ("daemon-reload".to_owned(), true),
            ("restart lamp-control.service".to_owned(), true)
        ]
    );
    let _ = fs::remove_dir_all(folder);
}
