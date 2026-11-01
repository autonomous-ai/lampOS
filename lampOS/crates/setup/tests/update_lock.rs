//! Setup's helpers under lamp-update's lock (#4490, #4485, the app's restart
//! and shutdown), in a binary of their own. Each test lets go of the lock and
//! has it taken again at once, by itself or by the helper. In the `it`
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
use lamp_setup::change;
use lamp_update::quiet::Heard;
use serde_json::json;
use std::{fs, sync::atomic::Ordering, time::Duration};

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

/// Fourth's P2 on #4485: a language takes lamp-update's lock before it is
/// written. Under an update it is refused with the restart's words and the
/// file is untouched; otherwise the lock is held for the voice's restart and
/// let go after it, and the same language again holds none.
#[test]
fn a_language_takes_the_update_lock_before_it_is_written() {
    use lamp_keeper::restart::INSTALLING;
    let root = Root::new();
    let config = root.config();
    let records = lamp_update::records::Records::new(root.0.join("update"));
    let held = records.lock().unwrap();
    let (reply, done, lock) =
        change::answer_holding(r#"{"language": "ja"}"#, &config, true, &records);
    assert_eq!(reply, json!({ "refused": INSTALLING }));
    assert!(done.is_none() && lock.is_none());
    assert!(!config.language.exists(), "nothing written");
    // Other changes don't wait on it.
    let (reply, _, lock) =
        change::answer_holding(r#"{"person_name": "Lan"}"#, &config, true, &records);
    assert_eq!(reply, json!({"done": {"name": "Lan"}}));
    assert!(lock.is_none());
    drop(held);
    let (reply, done, lock) =
        change::answer_holding(r#"{"language": "ja"}"#, &config, true, &records);
    assert_eq!(reply, json!({"done": {"language": "ja"}}));
    assert!(done.unwrap().restart_voice);
    assert!(lock.is_some(), "held through the restart");
    assert!(records.lock().is_err(), "no update starts meanwhile");
    drop(lock);
    assert!(records.lock().is_ok(), "let go after the restart");
    let (_, done, lock) = change::answer_holding(r#"{"language": "ja"}"#, &config, true, &records);
    assert!(!done.unwrap().restart_voice);
    assert!(lock.is_none());
    assert!(records.lock().is_ok(), "the same language holds nothing");
    // A bad language under no update: refused, nothing held.
    let (reply, _, lock) = change::answer_holding(r#"{"language": "fr"}"#, &config, true, &records);
    assert_eq!(
        reply,
        json!({"refused": "language must be one of en, vi, ja, zh, ko"})
    );
    assert!(lock.is_none());
    assert_eq!(fs::read_to_string(&config.language).unwrap(), "ja\n");
}
