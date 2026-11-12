//! A switch cut short (the lamp lost power, rebooted or was reset, or
//! lamp-update was stopped mid-switch) says nothing about its build: it is
//! put back at the next start but not refused, and the next round tries the
//! build again (#6266). Only three cut-shorts in a row of the same build
//! refuse it, so a build that itself reboots the lamp mid-switch is still
//! stopped. `state.json`'s `cut_short` counts them.
use super::*;

/// Build `sequence`: a new voice and its worker; the body and the controls
/// as they are.
fn publish_voice(lamp: &mut Lamp, sequence: u64) {
    lamp.publish(
        sequence,
        &[
            ("lamp-voice", &format!("voice {sequence}")),
            ("lamp-turn-end", &format!("turn-end {sequence}")),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
}

/// A round to the published build, cut at its reload (the power goes), then
/// the next start, which puts the old build back.
fn cut_and_recover(lamp: &mut Lamp, sequence: u64) {
    lamp.systemd.cut_at_reload = true;
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lamp.round()));
    assert!(cut.is_err(), "the round was not cut");
    assert!(lamp.exists("/var/lib/lampos-update/journal.json"));
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: s, why, after: Ok(()), .. } if *s == sequence && why.contains("cut short")),
        "{outcome:?}"
    );
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

fn refused(lamp: &Lamp) -> Vec<u64> {
    lamp.state()["refused"]
        .as_array()
        .map(|refused| {
            refused
                .iter()
                .map(|refusal| refusal["sequence"].as_u64().unwrap())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_switch_cut_short_is_put_back_not_refused_and_the_next_round_takes_the_build() {
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    cut_and_recover(&mut lamp, 2);
    assert_eq!(refused(&lamp), Vec::<u64>::new());
    assert_eq!(lamp.state()["cut_short"]["sequence"], 2);
    assert_eq!(lamp.state()["cut_short"]["times"], 1);
    assert_eq!(running(&lamp)["lamp-voice.service"], "voice 1");

    // The next round tries build 2 again, and it is taken.
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(running(&lamp)["lamp-voice.service"], "voice 2");
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
    // A finished switch ends the count.
    assert!(lamp.state().get("cut_short").is_none(), "{}", lamp.state());
}

#[test]
fn three_cut_shorts_in_a_row_of_the_same_build_refuse_it() {
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    cut_and_recover(&mut lamp, 2);
    cut_and_recover(&mut lamp, 2);
    assert_eq!(refused(&lamp), Vec::<u64>::new());
    assert_eq!(lamp.state()["cut_short"]["times"], 2);
    cut_and_recover(&mut lamp, 2);
    assert_eq!(refused(&lamp), vec![2]);
    assert_eq!(lamp.state()["refused"][0]["kind"], "cut_short");
    let why = lamp.state()["refused"][0]["why"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        why.contains("cut short") && why.contains("3 times"),
        "{why}"
    );
    assert!(lamp.state().get("cut_short").is_none(), "{}", lamp.state());

    // From then on the build is not tried again.
    let calls = lamp.calls().len();
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Refused(why) if why.contains("was put back")),
        "{outcome:?}"
    );
    assert_eq!(lamp.calls().len(), calls);
}

#[test]
fn a_different_build_starts_the_count_again() {
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    cut_and_recover(&mut lamp, 2);
    cut_and_recover(&mut lamp, 2);
    // A newer build is published: its cuts are its own.
    publish_voice(&mut lamp, 3);
    cut_and_recover(&mut lamp, 3);
    assert_eq!(lamp.state()["cut_short"]["sequence"], 3);
    assert_eq!(lamp.state()["cut_short"]["times"], 1);
    cut_and_recover(&mut lamp, 3);
    assert_eq!(refused(&lamp), Vec::<u64>::new());
    assert_eq!(lamp.round(), Outcome::Switched(3));
}

#[test]
fn a_finished_switch_starts_the_count_again() {
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    cut_and_recover(&mut lamp, 2);
    cut_and_recover(&mut lamp, 2);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert!(lamp.state().get("cut_short").is_none(), "{}", lamp.state());
    publish_voice(&mut lamp, 3);
    cut_and_recover(&mut lamp, 3);
    assert_eq!(lamp.state()["cut_short"]["times"], 1);
    assert_eq!(refused(&lamp), Vec::<u64>::new());
}

/// A real refusal is unchanged by the count: a build that comes up unwell
/// after a cut-short is refused at once.
#[test]
fn a_part_not_well_after_a_cut_short_is_still_refused_at_once() {
    let mut lamp = Lamp::new();
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 2, which crashes"),
            ("lamp-turn-end", "turn-end 2"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
    cut_and_recover(&mut lamp, 2);
    assert_eq!(refused(&lamp), Vec::<u64>::new());
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, .. } if why.contains("lamp-voice.service ended")),
        "{outcome:?}"
    );
    assert_eq!(refused(&lamp), vec![2]);
    assert_eq!(lamp.state()["refused"][0]["kind"], "part_unwell");
    assert!(matches!(lamp.round(), Outcome::Refused(why) if why.contains("was put back")));
}

/// A state written before the count (no `cut_short`) still loads, and a
/// cut-short recovered over it starts the count at one.
#[test]
fn a_state_from_before_the_count_still_loads() {
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    let state = lamp.root.join("var/lib/lampos-update/state.json");
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    fs::write(
        &state,
        r#"{"installed":null,"previous":null,"refused":[],"made":[]}"#,
    )
    .unwrap();
    cut_and_recover(&mut lamp, 2);
    assert_eq!(lamp.state()["cut_short"]["times"], 1);
    assert_eq!(lamp.round(), Outcome::Switched(2));
}

/// Build 2, whose voice crashes: a fault of the build, put back and refused.
fn publish_crashing_voice(lamp: &mut Lamp) {
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 2, which crashes"),
            ("lamp-turn-end", "turn-end 2"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
}

/// Thirteen's P3 on #6416: a power cut while a fault of the build is being
/// put back leaves the journal saying why, so the next start refuses the
/// build at once and does not count it as a cut short to try again.
#[test]
fn a_cut_while_a_fault_is_put_back_still_refuses_the_build() {
    let mut lamp = Lamp::new();
    publish_crashing_voice(&mut lamp);
    // The switch's reload passes; the put-back's own reload, with the old
    // units half written back, is where the power goes.
    let mut reloads = 0;
    lamp.systemd.at_call = Some(Box::new(move |call| {
        reloads += usize::from(call == "reload");
        reloads == 2
    }));
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lamp.round()));
    assert!(cut.is_err(), "the put-back was not cut: {:?}", lamp.calls());
    lamp.systemd.at_call = None;
    assert!(lamp.exists("/var/lib/lampos-update/journal.json"));

    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, .. } if why.contains("lamp-voice.service ended")),
        "{outcome:?}"
    );
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    assert_eq!(refused(&lamp), vec![2]);
    assert!(lamp.state().get("cut_short").is_none(), "{}", lamp.state());
    assert_eq!(running(&lamp)["lamp-voice.service"], "voice 1");
    assert!(matches!(lamp.round(), Outcome::Refused(why) if why.contains("was put back")));
}

/// As above, with the files all put back and the state not yet written: the
/// next start still refuses the build.
#[test]
fn a_fault_put_back_whose_state_is_not_written_still_refuses_the_build() {
    let mut lamp = Lamp::new();
    publish_crashing_voice(&mut lamp);
    // At the put-back's restart of the old voice (the second restart of
    // lamp-voice), the files are back; the state then cannot be written.
    let beside = lamp
        .root
        .join("var/lib/lampos-update/.state.json.lamp-update");
    let blocker = beside.clone();
    let mut restarts = 0;
    lamp.systemd.at_call = Some(Box::new(move |call| {
        restarts += usize::from(call == "restart lamp-voice.service");
        if restarts == 2 {
            fs::create_dir_all(blocker.join("in the way")).unwrap();
        }
        false
    }));
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Failed(why) if why.contains("the state cannot be written")),
        "{outcome:?}"
    );
    lamp.systemd.at_call = None;
    assert_eq!(running(&lamp)["lamp-voice.service"], "voice 1");
    assert!(lamp.exists("/var/lib/lampos-update/journal.json"));
    fs::remove_dir_all(&beside).unwrap();

    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, .. } if why.contains("lamp-voice.service ended")),
        "{outcome:?}"
    );
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    assert_eq!(refused(&lamp), vec![2]);
    assert!(lamp.state().get("cut_short").is_none(), "{}", lamp.state());
    assert!(matches!(lamp.round(), Outcome::Refused(why) if why.contains("was put back")));
}

/// A switch cut short is put back with no `refusing` in its journal: a cut
/// during that put-back is still a cut short, counted and tried again.
#[test]
fn a_cut_while_a_cut_short_is_put_back_is_still_only_counted() {
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    lamp.systemd.cut_at_reload = true;
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lamp.round()));
    assert!(cut.is_err());
    // The next start's put-back is cut at its reload in turn.
    lamp.systemd.cut_at_reload = true;
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lamp.round()));
    assert!(cut.is_err(), "the put-back was not cut: {:?}", lamp.calls());
    let journal: serde_json::Value =
        serde_json::from_str(&lamp.read("/var/lib/lampos-update/journal.json")).unwrap();
    assert!(journal.get("refusing").is_none(), "{journal}");
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, .. } if why.contains("cut short")),
        "{outcome:?}"
    );
    assert_eq!(refused(&lamp), Vec::<u64>::new());
    assert_eq!(lamp.round(), Outcome::Switched(2));
}

/// A refused build is said to the app by a fixed name, never its text, and the
/// owner's retry drops the refusal so the next round takes the build (#6266);
/// a retry of a build that is not refused changes nothing.
#[test]
fn a_refused_build_is_said_by_a_fixed_name_and_the_owners_retry_drops_it() {
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    cut_and_recover(&mut lamp, 2);
    cut_and_recover(&mut lamp, 2);
    cut_and_recover(&mut lamp, 2);
    assert_eq!(refused(&lamp), vec![2]);
    let said = |lamp: &Lamp| {
        let state: lamp_update::records::State = serde_json::from_value(lamp.state()).unwrap();
        lamp_update::status::refused(&state)
    };
    let refused_now = said(&lamp).expect("the blocked update is said");
    assert_eq!(
        (refused_now.sequence, refused_now.reason.as_str()),
        (2, "cut_short")
    );
    assert!(refused_now.check());
    // A round now refuses and says so, as before.
    assert!(matches!(lamp.round(), Outcome::Refused(_)));
    let records = lamp_update::records::Records::new(lamp.records());
    let mut told = Vec::new();
    // Not refused: a plain check, nothing dropped.
    lamp_update::app_check::drop_refusals(&records, &[9], &mut |line| told.push(line.to_owned()));
    assert_eq!(refused(&lamp), vec![2], "{told:?}");
    assert!(
        told.iter().any(|line| line.contains("not refused")),
        "{told:?}"
    );
    // The owner's retry of build 2: dropped, nothing else.
    lamp_update::app_check::drop_refusals(&records, &[2], &mut |line| told.push(line.to_owned()));
    assert_eq!(refused(&lamp), Vec::<u64>::new());
    assert!(said(&lamp).is_none());
    assert_eq!(lamp.round(), Outcome::Switched(2));
}

/// A refusal of a build the lamp already runs or is past blocks nothing and
/// is not said.
#[test]
fn only_a_refusal_newer_than_the_running_build_is_said() {
    let mut state = lamp_update::records::State::default();
    let build = |sequence| lamp_update::records::Build {
        sequence,
        commit: format!("c{sequence}"),
    };
    state.installed = Some(build(5));
    state.refuse_as(&build(3), "old", Some("other"), None);
    assert!(lamp_update::status::refused(&state).is_none());
    state.refuse_as(&build(6), "a part", Some("part_unwell"), None);
    state.refuse_as(
        &build(7),
        "lamp-body held, nothing moves: unplug",
        None,
        None,
    );
    let said = lamp_update::status::refused(&state).unwrap();
    // The newest, by its fixed name (read from the text for an entry with none).
    assert_eq!((said.sequence, said.reason.as_str()), (7, "held"));
}

/// A state with build 2 refused as `kind`, at `at_unix_ms`, on boot `boot`.
fn refused_state(lamp: &Lamp, kind: &str, at_unix_ms: u64, boot: Option<&str>, retried: bool) {
    let state = lamp.root.join("var/lib/lampos-update/state.json");
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    let mut refusal = serde_json::json!({
        "sequence": 2, "commit": "c2", "why": "lamp-body held, nothing moves: kept unplug request present, and was not before the switch",
        "at_unix_ms": at_unix_ms, "kind": kind,
    });
    if let Some(boot) = boot {
        refusal["boot_id"] = boot.into();
    }
    if retried {
        refusal["retried"] = true.into();
    }
    let state_json = serde_json::json!({
        "installed": null, "previous": null, "refused": [refusal], "made": [],
    });
    fs::write(&state, state_json.to_string()).unwrap();
}

fn this_boot(lamp: &Lamp, id: &str) {
    let path = lamp.root.join("proc/sys/kernel/random/boot_id");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("{id}\n")).unwrap();
}

/// A build refused because a part was held by a passing local state (a kept
/// unplug request) is no fault of its own (#6266, 5a): it is tried once more on
/// another boot or after 24 hours, and stays refused before, whatever else.
#[test]
fn a_held_refusal_gets_one_more_try_after_a_reboot_or_24_hours() {
    let hour_ms = 60 * 60 * 1000;
    let long_ago = records_now() - 25 * hour_ms;
    let just_now = records_now() - hour_ms;
    // Made now, on this boot: refused still.
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    this_boot(&lamp, "boot-b");
    refused_state(&lamp, "held", just_now, Some("boot-b"), false);
    assert!(matches!(lamp.round(), Outcome::Refused(why) if why.contains("was put back")));
    // On another boot: once more, and taken.
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    this_boot(&lamp, "boot-b");
    refused_state(&lamp, "held", just_now, Some("boot-a"), false);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(refused(&lamp), Vec::<u64>::new());
    // 24 hours on the same boot: once more.
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    this_boot(&lamp, "boot-b");
    refused_state(&lamp, "held", long_ago, Some("boot-b"), false);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    // A boot that is not known (no id, or none noted) is never "another boot".
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    refused_state(&lamp, "held", just_now, Some("boot-a"), false);
    assert!(matches!(lamp.round(), Outcome::Refused(_)));
    // Already tried once more: final, however old.
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    this_boot(&lamp, "boot-b");
    refused_state(&lamp, "held", long_ago, Some("boot-a"), true);
    assert!(matches!(lamp.round(), Outcome::Refused(_)));
    // A real fault is refused for good, however old and on any boot.
    for kind in ["part_unwell", "cut_short", "other"] {
        let mut lamp = Lamp::new();
        publish_voice(&mut lamp, 2);
        this_boot(&lamp, "boot-b");
        refused_state(&lamp, kind, long_ago, Some("boot-a"), false);
        assert!(matches!(lamp.round(), Outcome::Refused(_)), "{kind}");
    }
}

/// The one more try is only one: a build whose held refusal was lapsed and is
/// refused again is final; and a try that is cut short does not use it up.
#[test]
fn the_one_more_try_of_a_held_refusal_is_only_one() {
    let build = lamp_update::records::Build {
        sequence: 2,
        commit: "c2".into(),
    };
    let hours = |h: u64| h * 60 * 60 * 1000;
    let mut state = lamp_update::records::State::default();
    let held = "lamp-body held, nothing moves: unplug";
    state.refuse_as(&build, held, Some("held"), Some("boot-a"));
    let at = state.refused[0].at_unix_ms;
    // Not yet: same boot, within the day.
    assert_eq!(state.lapse_held(2, at + hours(1), Some("boot-a")), None);
    assert_eq!(state.refused.len(), 1);
    // Another boot: lapsed, the refusal dropped and the try noted.
    assert_eq!(
        state.lapse_held(2, at + hours(1), Some("boot-b")),
        Some("a reboot")
    );
    assert!(state.refused.is_empty());
    assert_eq!(state.held_retried, vec![2]);
    // The try is cut short (nothing refused): still no refusal, the next round tries it again.
    assert_eq!(state.lapse_held(2, at + hours(2), Some("boot-b")), None);
    // The try is refused as held again: final, whatever the clock and the boot.
    state.refuse_as(&build, held, Some("held"), Some("boot-b"));
    assert!(state.refused[0].retried);
    assert_eq!(state.lapse_held(2, at + hours(100), Some("boot-c")), None);
    assert_eq!(state.refused.len(), 1);
}
