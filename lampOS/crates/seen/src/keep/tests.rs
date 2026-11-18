//! The keep's rules, read from the bus as `serve` reads it
//! ([`crate::scene::Scene`]).
use super::*;
use crate::scene::{S, Scene};

/// One step of `serve`'s keep at `at`, with posture said and a sitting
/// under way.
fn plan(scene: &mut Scene, keep: &mut Keep, at: u64, stood_up: bool, forgot: bool) -> Plan {
    scene.renew(at);
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
    keep.step(
        &Now {
            now_us: at,
            inputs: &inputs,
            seen: &seen,
            stood_up,
            forgot,
            posture_gate: gate,
            desk,
            watching: true,
        },
        true,
        false,
    )
}

fn owner(scene: &mut Scene, keep: &mut Keep, at: u64) -> Plan {
    owner_as(scene, keep, "f1", at)
}

/// A face `id` alone in view at `at`.
fn owner_as(scene: &mut Scene, keep: &mut Keep, id: &str, at: u64) -> Plan {
    scene.faces(&[(id, 0.4)], at);
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
    // Back on the same passing id: open at once, no recogniser's level
    // waited for.
    scene.faces(&[("f1", 0.4)], 32 * S);
    let open = plan(&mut scene, &mut keep, 32 * S, false, false);
    assert_eq!(scene.gate(32 * S), Ok(()));
    assert_eq!((open.posture, open.sitting), (Step::Resumed, Step::Resumed));
    assert_eq!(
        open.posture.line("posture's window", "open").unwrap(),
        "posture's window goes on: the gate opened again within 120 s"
    );
    assert_eq!(owner(&mut scene, &mut keep, 33 * S).posture, Step::Open);
}

#[test]
fn a_second_remembered_person_within_120_s_starts_afresh() {
    // The owner (f1) leaves; another person (f7) sits down 30 s later: the
    // window and timers kept for f1 are wiped, never taken on by f7.
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    owner(&mut scene, &mut keep, S);
    scene.faces(&[], 2 * S);
    let lost = plan(&mut scene, &mut keep, 2 * S, false, false);
    assert_eq!((lost.posture, lost.sitting), (Step::Began, Step::Began));
    scene.faces(&[("f7", 0.4)], 32 * S);
    let other = plan(&mut scene, &mut keep, 32 * S, false, false);
    assert_eq!(scene.gate(32 * S), Ok(()));
    assert_eq!(other.hard, None);
    assert_eq!(
        (other.posture, other.sitting),
        (Step::Wiped(Why::OtherFace), Step::Wiped(Why::OtherFace))
    );
    assert_eq!(
        other.sitting.line("the sitting's timers", "open").unwrap(),
        "the sitting's timers wiped: the gate opened again on another face than the one it was kept for"
    );
    // f7's own sitting from here, open as any new one; f7 then lost and
    // back on f7 goes on as f7's.
    assert_eq!(
        owner_as(&mut scene, &mut keep, "f7", 33 * S).posture,
        Step::Open
    );
    scene.faces(&[], 34 * S);
    assert!(
        plan(&mut scene, &mut keep, 34 * S, false, false)
            .posture
            .holds()
    );
    let again = owner_as(&mut scene, &mut keep, "f7", 40 * S);
    assert_eq!(
        (again.posture, again.sitting),
        (Step::Resumed, Step::Resumed)
    );
}

#[test]
fn a_keep_begun_with_no_face_known_never_goes_on() {
    let mut away = Away::default();
    // Held from its first step with no face in view: whose it is not known.
    assert_eq!(away.step(S, true, None, true, None), Step::Began);
    assert_eq!(
        away.step(2 * S, false, None, true, Some("f1")),
        Step::Wiped(Why::OtherFace)
    );
    // Open with f1, kept, reopened with no face in view: not known either.
    assert_eq!(away.step(3 * S, false, None, true, Some("f1")), Step::Open);
    assert_eq!(away.step(4 * S, true, None, true, None), Step::Began);
    assert_eq!(
        away.step(5 * S, false, None, true, None),
        Step::Wiped(Why::OtherFace)
    );
    // A face lost for a step while open keeps whose it is.
    assert_eq!(away.step(6 * S, false, None, true, Some("f2")), Step::Open);
    assert_eq!(away.step(7 * S, false, None, true, None), Step::Open);
    assert_eq!(away.step(8 * S, true, None, true, None), Step::Began);
    assert_eq!(
        away.step(9 * S, false, None, true, Some("f2")),
        Step::Resumed
    );
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
fn a_second_face_or_a_guest_goes_on() {
    // No stranger check (the owner's word on #6543): a second face beside
    // the owner closes and wipes nothing.
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    owner(&mut scene, &mut keep, S);
    scene.faces(&[("f1", 0.4), ("f2", 0.1)], 2 * S);
    let two = plan(&mut scene, &mut keep, 2 * S, false, false);
    assert_eq!(two.hard, None);
    assert_eq!((two.posture, two.sitting), (Step::Open, Step::Open));
    // The owner turned away and read under a new id in the same step: the
    // gate stays open, so nothing kept and nothing wiped.
    scene.faces(&[("f9", 0.4)], 3 * S);
    let them = plan(&mut scene, &mut keep, 3 * S, false, false);
    assert_eq!(them.hard, None);
    assert_eq!((them.posture, them.sitting), (Step::Open, Step::Open));
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
    let open_up = plan(&mut scene, &mut keep, S, true, false);
    assert!(open_up.sitting.wipes());
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    scene.faces(&[("f1", 0.4)], S);
    let forgot = plan(&mut scene, &mut keep, S, false, true);
    assert_eq!(forgot.sitting, Step::Wiped(Why::Hard(Hard::Forgot)));
}

#[test]
fn nothing_to_keep_is_as_before() {
    let mut away = Away::default();
    assert_eq!(away.step(0, true, None, false, None), Step::Open);
    assert_eq!(
        away.step(0, true, Some(Hard::Asleep), false, None),
        Step::Open
    );
    assert_eq!(away.step(S, true, None, true, None), Step::Began);
    assert_eq!(
        away.step(S + KEEP_US - 1, true, None, true, None),
        Step::Kept
    );
    assert_eq!(
        away.step(S + KEEP_US, true, None, true, None),
        Step::Wiped(Why::Expired)
    );
    assert_eq!(away.step(S + KEEP_US, true, None, false, None), Step::Open);
}

#[test]
fn a_hold_ended_on_another_face_wipes_both_as_another_face() {
    let mut scene = Scene::owner_at(0);
    let mut keep = Keep::default();
    assert_eq!(owner(&mut scene, &mut keep, S).posture, Step::Open);
    scene.faces(&[("f2", 0.4)], 2 * S);
    scene.renew(2 * S);
    let inputs = scene.readings.inputs(scene.opted_in, true, 2 * S);
    let seen = scene.readings.on_track(2 * S);
    let gate = crate::gate::may_request(&inputs);
    let step = |keep: &mut Keep, sitting: bool, watching: bool| {
        keep.step(
            &Now {
                now_us: 2 * S,
                inputs: &inputs,
                seen: &seen,
                stood_up: false,
                forgot: false,
                posture_gate: gate,
                desk: gate,
                watching,
            },
            sitting,
            true,
        )
    };
    let plan = step(&mut keep, true, true);
    assert_eq!(plan.hard, None);
    assert_eq!(
        (plan.posture, plan.sitting),
        (Step::Wiped(Why::OtherFace), Step::Wiped(Why::OtherFace))
    );
    // Nothing under way: nothing to wipe.
    let plan = step(&mut Keep::default(), false, false);
    assert_eq!((plan.posture, plan.sitting), (Step::Open, Step::Open));
    // The new face's id is kept: lost and back under it, it goes on.
    scene.faces(&[], 3 * S);
    let lost = plan_at(&mut scene, &mut keep, 3 * S);
    assert_eq!((lost.posture, lost.sitting), (Step::Began, Step::Began));
    let back = owner_as(&mut scene, &mut keep, "f2", 10 * S);
    assert_eq!((back.posture, back.sitting), (Step::Resumed, Step::Resumed));
}

fn plan_at(scene: &mut Scene, keep: &mut Keep, at: u64) -> Plan {
    plan(scene, keep, at, false, false)
}

/// One step of `serve`'s seat at `at` ([`Seat::step`]), with posture said,
/// `room/activity`'s arrival at 0, and the break a stand-up.
fn seat_step(
    scene: &mut Scene,
    seat: &mut Seat,
    at: u64,
    stood_up: Option<u64>,
    hold_notes: &[crate::hold::Note],
) -> Plan {
    scene.renew(at);
    let inputs = scene.readings.inputs(scene.opted_in, true, at);
    let seen = scene.readings.on_track(at);
    let gate = crate::gate::may_request(&inputs);
    seat.step(
        &Now {
            now_us: at,
            inputs: &inputs,
            seen: &seen,
            stood_up: stood_up == Some(at),
            forgot: false,
            posture_gate: gate,
            desk: gate,
            watching: true,
        },
        hold_notes,
        &Begin {
            arrived_us: 0,
            more: false,
            break_is_standing: true,
            stood_up_us: stood_up,
        },
    )
}

/// What `humans/doing` says at `at`: break, drink and eating, in seconds.
fn timers(seat: &Seat, at: u64) -> (u32, u32, u32) {
    let doing = seat.sitting().expect("a sitting").doing(at);
    (doing.break_s_ago, doing.drank_s_ago, doing.ate_s_ago)
}

#[test]
fn seat_a_held_sitting_ended_on_another_face_begins_the_next_from_then() {
    // lamp-06c1, run 8 (#7506), through `serve`'s step: `room/activity`
    // still says the sitting began at 0 when another face ends the hold
    // at 2_100 s; the next person's timers count from then.
    use crate::hold::{End, Note};
    let mut scene = Scene::owner_at(0);
    let mut seat = Seat::default();
    seat_step(&mut scene, &mut seat, S, None, &[Note::Began]);
    assert_eq!(timers(&seat, 60 * S), (60, 60, 60), "from the arrival");
    scene.faces(&[("f2", 0.4)], 2_100 * S);
    let plan = seat_step(
        &mut scene,
        &mut seat,
        2_100 * S,
        None,
        &[Note::Ended(End::OtherFace), Note::Began],
    );
    assert_eq!(
        (plan.posture, plan.sitting),
        (Step::Wiped(Why::OtherFace), Step::Wiped(Why::OtherFace))
    );
    assert_eq!(timers(&seat, 2_160 * S), (60, 60, 60));
    // Later wipes of other kinds keep that floor.
    scene.faces(&[], 2_200 * S);
    seat_step(&mut scene, &mut seat, 2_200 * S, None, &[]);
    scene.faces(&[("f2", 0.4)], 2_210 * S);
    seat_step(&mut scene, &mut seat, 2_210 * S, None, &[]);
    assert_eq!(timers(&seat, 2_260 * S), (160, 160, 160));
}

#[test]
fn seat_120_s_with_nobody_begins_the_next_from_the_wipe() {
    let mut scene = Scene::owner_at(0);
    let mut seat = Seat::default();
    seat_step(&mut scene, &mut seat, S, None, &[]);
    for at in [2 * S, 60 * S] {
        scene.faces(&[], at);
        seat_step(&mut scene, &mut seat, at, None, &[]);
    }
    scene.faces(&[], 122 * S);
    let plan = seat_step(&mut scene, &mut seat, 122 * S, None, &[]);
    assert_eq!(plan.sitting, Step::Wiped(Why::Expired));
    scene.faces(&[("f2", 0.4)], 130 * S);
    seat_step(&mut scene, &mut seat, 130 * S, None, &[]);
    assert_eq!(timers(&seat, 182 * S), (60, 60, 60));
}

#[test]
fn seat_a_stand_up_begins_from_the_arrival_as_before() {
    // Twelve's P2 on #7635: one person's ~30 s stand-up keeps the drink
    // and eating timers from the first arrival; the break counts from
    // the return (`LAMP_SEEN_BREAK_IS_STANDING`).
    let mut scene = Scene::owner_at(0);
    let mut seat = Seat::default();
    seat_step(&mut scene, &mut seat, S, None, &[]);
    let plan = seat_step(&mut scene, &mut seat, 2_072 * S, Some(2_072 * S), &[]);
    assert_eq!(plan.sitting, Step::Wiped(Why::Hard(Hard::StoodUp)));
    scene.faces(&[], 2_073 * S);
    seat_step(&mut scene, &mut seat, 2_073 * S, Some(2_072 * S), &[]);
    scene.faces(&[("f4", 0.4)], 2_100 * S);
    seat_step(&mut scene, &mut seat, 2_100 * S, Some(2_072 * S), &[]);
    assert_eq!(timers(&seat, 3_000 * S), (900, 3_000, 3_000));
}

#[test]
fn seat_a_plain_new_id_without_the_hold_begins_from_the_arrival_as_before() {
    // The keep's own OtherFace: the gate open again on a new passing id,
    // most often the same person looking back up. The arrival stands.
    let mut scene = Scene::owner_at(0);
    let mut seat = Seat::default();
    seat_step(&mut scene, &mut seat, S, None, &[]);
    scene.faces(&[], 2_090 * S);
    seat_step(&mut scene, &mut seat, 2_090 * S, None, &[]);
    scene.faces(&[("f3", 0.4)], 2_100 * S);
    let plan = seat_step(&mut scene, &mut seat, 2_100 * S, None, &[]);
    assert_eq!(plan.sitting, Step::Wiped(Why::OtherFace));
    assert_eq!(timers(&seat, 2_160 * S), (2_160, 2_160, 2_160));
}
