//! The keep's rules, read from the bus as `serve` reads it, with the pause
//! stepped as `serve` steps it ([`crate::pause::tests::Scene`]).
use super::*;
use crate::pause::tests::{MS, S, Scene};

/// One step of `serve`'s keep at `at`, with posture said and a sitting
/// under way.
fn plan(scene: &mut Scene, keep: &mut Keep, at: u64, stood_up: bool, forgot: bool) -> Plan {
    let verdict = scene.step(at);
    let verdict = pause::after_stood_up(&mut scene.pause, verdict, stood_up);
    let inputs = scene.readings.inputs(scene.opted_in, true, at);
    let seen = scene.readings.on_track(at);
    let gate = crate::gate::may_request(&inputs);
    let desk = crate::gate::may_request(&Inputs {
        opted_in: true,
        said_on: true,
        privacy_open: Some(true),
        asleep: Some(false),
        ..inputs
    });
    keep.step(&Now {
        now_us: at,
        inputs: &inputs,
        seen: &seen,
        verdict,
        pause_lost: scene.pause.lost(),
        stood_up,
        forgot,
        posture_gate: gate,
        desk,
        watching: true,
        sitting: true,
    })
}

fn owner(scene: &mut Scene, keep: &mut Keep, at: u64) -> Plan {
    scene.faces(&[("f1", 0.4)], at);
    scene.known(at);
    plan(scene, keep, at, false, false)
}

#[test]
fn the_face_lost_30_s_then_back_and_recognised_goes_on() {
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    assert_eq!(owner(&mut scene, &mut keep, S).posture, Step::Open);
    // The face lost at 2 s: nobody in view.
    scene.faces(&[], 2 * S);
    let lost = plan(&mut scene, &mut keep, 2 * S, false, false);
    assert_eq!(lost.hard, None);
    assert_eq!((lost.posture, lost.sitting), (Step::Began, Step::Began));
    for at in [10 * S, 20 * S, 31 * S] {
        scene.faces(&[], at);
        let still = plan(&mut scene, &mut keep, at, false, false);
        assert_eq!((still.posture, still.sitting), (Step::Kept, Step::Kept));
        assert!(still.posture.holds() && still.sitting.holds());
    }
    // Back with a new passing id: closed until the recogniser says a level.
    scene.faces(&[("f7", 0.4)], 32 * S);
    let back = plan(&mut scene, &mut keep, 32 * S, false, false);
    assert_eq!(scene.gate(32 * S), Err(crate::gate::Closed::Stranger));
    assert_eq!((back.posture, back.sitting), (Step::Kept, Step::Kept));
    scene.known(32 * S + 500 * MS);
    let open = plan(&mut scene, &mut keep, 32 * S + 500 * MS, false, false);
    assert_eq!(scene.gate(32 * S + 500 * MS), Ok(()));
    assert_eq!((open.posture, open.sitting), (Step::Resumed, Step::Resumed));
    assert_eq!(
        open.posture.line("posture's window", "open").unwrap(),
        "posture's window goes on: the gate opened again within 120 s"
    );
    assert_eq!(owner(&mut scene, &mut keep, 33 * S).posture, Step::Open);
}

#[test]
fn closed_past_120_s_wipes() {
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    owner(&mut scene, &mut keep, S);
    scene.faces(&[], 2 * S);
    let began = plan(&mut scene, &mut keep, 2 * S, false, false);
    assert_eq!(
        began.posture.line("posture's window", "nobody").unwrap(),
        "posture's window kept unsaid for up to 120 s: the gate closed (nobody)"
    );
    scene.faces(&[], 121 * S);
    assert_eq!(
        plan(&mut scene, &mut keep, 121 * S, false, false).posture,
        Step::Kept
    );
    scene.faces(&[], 132 * S);
    let gone = plan(&mut scene, &mut keep, 132 * S, false, false);
    assert_eq!(gone.posture, Step::Wiped(Why::Expired));
    assert_eq!(gone.sitting, Step::Wiped(Why::Expired));
    assert_eq!(
        gone.sitting.line("the sitting's timers", "nobody").unwrap(),
        "the sitting's timers wiped: the gate stayed closed 120 s (nobody)"
    );
}

#[test]
fn the_privacy_slide_wipes_posture_at_once() {
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    owner(&mut scene, &mut keep, S);
    scene.privacy = false;
    let shut = owner(&mut scene, &mut keep, 2 * S);
    assert_eq!(shut.hard, Some(Hard::PrivacyClosed));
    assert_eq!(shut.posture, Step::Wiped(Why::Hard(Hard::PrivacyClosed)));
    // The doing timers never ended on the privacy alone: the desk is open.
    assert_eq!(shut.sitting, Step::Open);
    // Sleep and the opt-in off wipe at once too.
    for (asleep, opted_in, hard) in [(true, true, Hard::Asleep), (false, false, Hard::OptInOff)] {
        let mut scene = Scene::owner_at(0);
        let mut keep = Keep::default();
        scene.asleep = asleep;
        scene.opted_in = opted_in;
        let closed = owner(&mut scene, &mut keep, S);
        assert_eq!(closed.posture, Step::Wiped(Why::Hard(hard)));
    }
}

#[test]
fn the_privacy_after_the_face_was_lost_still_wipes_at_once() {
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    owner(&mut scene, &mut keep, S);
    scene.faces(&[], 2 * S);
    assert!(
        plan(&mut scene, &mut keep, 2 * S, false, false)
            .posture
            .holds()
    );
    scene.privacy = false;
    scene.faces(&[], 3 * S);
    let shut = plan(&mut scene, &mut keep, 3 * S, false, false);
    assert_eq!(shut.posture, Step::Wiped(Why::Hard(Hard::PrivacyClosed)));
}

#[test]
fn a_stranger_confirmed_wipes_at_once() {
    // A second face in view.
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    owner(&mut scene, &mut keep, S);
    scene.faces(&[("f1", 0.4), ("f2", 0.1)], 2 * S);
    let two = plan(&mut scene, &mut keep, 2 * S, false, false);
    assert_eq!(two.hard, Some(Hard::Stranger));
    assert_eq!(two.posture, Step::Wiped(Why::Hard(Hard::Stranger)));
    assert_eq!(two.sitting, Step::Wiped(Why::Hard(Hard::Stranger)));
    // The face lost, then another sits down and is read as nobody
    // remembered: kept while not known, wiped once the reading says so.
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    owner(&mut scene, &mut keep, S);
    scene.faces(&[], 2 * S);
    assert!(
        plan(&mut scene, &mut keep, 2 * S, false, false)
            .sitting
            .holds()
    );
    scene.faces(&[("f9", 0.4)], 3 * S);
    assert!(
        plan(&mut scene, &mut keep, 3 * S, false, false)
            .sitting
            .holds()
    );
    scene.stranger(4 * S);
    scene.faces(&[("f9", 0.4)], 4 * S);
    let them = plan(&mut scene, &mut keep, 4 * S, false, false);
    assert_eq!(them.sitting, Step::Wiped(Why::Hard(Hard::Stranger)));
    assert_eq!(them.posture, Step::Wiped(Why::Hard(Hard::Stranger)));
}

#[test]
fn a_misread_past_5_s_still_wipes_at_once() {
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    owner(&mut scene, &mut keep, S);
    scene.faces(&[("f1", 0.4)], 2 * S);
    scene.stranger(2 * S);
    let paused = plan(&mut scene, &mut keep, 2 * S, false, false);
    assert_eq!((paused.posture, paused.sitting), (Step::Open, Step::Open));
    scene.faces(&[("f1", 0.4)], 7 * S);
    let past = plan(&mut scene, &mut keep, 7 * S, false, false);
    assert!(past.posture.wipes() && past.sitting.wipes(), "{past:?}");
}

#[test]
fn the_face_lost_during_a_pause_is_kept() {
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    owner(&mut scene, &mut keep, S);
    scene.faces(&[("f1", 0.4)], 2 * S);
    scene.stranger(2 * S);
    plan(&mut scene, &mut keep, 2 * S, false, false);
    assert!(scene.pause.paused());
    // pause.rs: no face wipes what the pause kept; the outer keep holds it.
    scene.faces(&[], 3 * S);
    let lost = plan(&mut scene, &mut keep, 3 * S, false, false);
    assert!(scene.pause.lost());
    assert_eq!(lost.hard, None);
    assert_eq!((lost.posture, lost.sitting), (Step::Began, Step::Began));
}

#[test]
fn standing_up_or_a_forget_wipes_at_once() {
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    owner(&mut scene, &mut keep, S);
    scene.faces(&[], 2 * S);
    plan(&mut scene, &mut keep, 2 * S, false, false);
    scene.faces(&[], 3 * S);
    let up = plan(&mut scene, &mut keep, 3 * S, true, false);
    assert_eq!(up.posture, Step::Wiped(Why::Hard(Hard::StoodUp)));
    assert_eq!(up.sitting, Step::Wiped(Why::Hard(Hard::StoodUp)));
    assert_eq!(
        up.posture.line("posture's window", "nobody").unwrap(),
        "posture's window wiped at once (stood_up)"
    );
    // Standing up with the gate open ends the sitting too.
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    scene.faces(&[("f1", 0.4)], S);
    scene.known(S);
    let open_up = plan(&mut scene, &mut keep, S, true, false);
    assert!(open_up.sitting.wipes());
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    scene.faces(&[("f1", 0.4)], S);
    scene.known(S);
    let forgot = plan(&mut scene, &mut keep, S, false, true);
    assert_eq!(forgot.sitting, Step::Wiped(Why::Hard(Hard::Forgot)));
}

#[test]
fn nothing_to_keep_is_as_before() {
    let mut away = Away::default();
    assert_eq!(away.step(0, true, None, false), Step::Open);
    assert_eq!(away.step(0, true, Some(Hard::Asleep), false), Step::Open);
    assert_eq!(away.step(S, true, None, true), Step::Began);
    assert_eq!(away.step(S + KEEP_US - 1, true, None, true), Step::Kept);
    assert_eq!(
        away.step(S + KEEP_US, true, None, true),
        Step::Wiped(Why::Expired)
    );
    assert_eq!(away.step(S + KEEP_US, true, None, false), Step::Open);
}
