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
