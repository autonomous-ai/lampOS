use super::*;
use crate::keep::{self, Keep};
use crate::watch::{OnTrack, Track};

const S: u64 = 1_000_000;
const CLOSE: Option<f32> = Some(0.25);
const FAR: Option<f32> = Some(0.08);

/// The gate open on a face.
fn open() -> Inputs {
    Inputs {
        opted_in: true,
        said_on: true,
        privacy_open: Some(true),
        someone: Some(true),
        faces: Some(1),
        asleep: Some(false),
    }
}

fn face_lost() -> Inputs {
    Inputs {
        faces: Some(0),
        ..open()
    }
}

fn now(now_us: u64, inputs: &Inputs) -> Now<'_> {
    Now {
        now_us,
        inputs,
        stood_up: false,
        forgot: false,
        recent_answer: None,
        posture: true,
        face: None,
    }
}

fn track() -> OnTrack {
    OnTrack {
        track: Some(Track {
            id: "f1".into(),
            edges: [0.3, 0.3, 0.5, 0.6],
        }),
    }
}

/// A hold begun at 0 with one close answer.
fn held() -> Hold {
    let mut hold = Hold::default();
    assert!(!hold.step(&now(0, &open())), "the gate opens itself");
    assert!(hold.held());
    hold.answer(CLOSE);
    assert_eq!(hold.notes(), [Note::Began]);
    hold
}

#[test]
fn the_shoulders_share_needs_both_confident() {
    let joint = |x: f32, conf: f32| Joint { x, y: 50.0, conf };
    let both = [joint(100.0, 0.9), joint(148.0, 0.3)];
    assert!((share(Some(&both), 320).unwrap() - 0.15).abs() < 1e-6);
    assert!(close(share(Some(&both), 320)));
    assert!(
        !close(share(Some(&both), 640)),
        "half as wide: across the room"
    );
    let weak = [joint(100.0, 0.9), joint(200.0, 0.29)];
    assert_eq!(share(Some(&weak), 320), None);
    assert_eq!(share(None, 320), None);
    assert_eq!(share(Some(&both), 0), None);
    assert!(!close(None));
}

#[test]
fn held_while_the_face_is_lost_and_a_close_person_is_seen() {
    let mut hold = held();
    for at in [1, 30, 200, 590] {
        assert!(hold.step(&now(at * S, &face_lost())), "{at}");
        if at % 30 == 0 {
            hold.answer(CLOSE);
        }
    }
    let notes = hold.notes();
    assert_eq!(
        notes[0],
        Note::GoesOn {
            share: CLOSE,
            far_in_row: 0
        }
    );
    assert_eq!(
        notes[0].line(),
        "held sitting goes on (close person, shoulders 25% of width)"
    );
    // Nobody heard and the faces not known don't end it.
    let unheard = Inputs {
        someone: Some(false),
        faces: None,
        ..open()
    };
    assert!(hold.step(&now(591 * S, &unheard)));
    assert!(hold.held());
}

#[test]
fn two_answers_without_a_close_person_end_it() {
    let mut hold = held();
    assert!(hold.step(&now(S, &face_lost())));
    hold.answer(FAR);
    assert!(hold.step(&now(31 * S, &face_lost())), "one is forgiven");
    hold.answer(None);
    assert!(!hold.step(&now(61 * S, &face_lost())));
    assert!(!hold.held());
    let notes = hold.notes();
    assert_eq!(notes.last(), Some(&Note::Ended(End::NoClosePerson)));
    assert_eq!(
        notes[1].line(),
        "held sitting goes on (no close person in 1 answer of 2, shoulders 8% of width)"
    );
    // Not again without the owner's face.
    assert!(!hold.step(&now(62 * S, &face_lost())));
    // A close answer between resets the count.
    let mut hold = held();
    for share in [FAR, CLOSE, FAR, CLOSE] {
        hold.answer(share);
        assert!(hold.step(&now(S, &face_lost())));
    }
}

#[test]
fn it_ends_10_min_after_the_gate_was_last_open_on_a_face() {
    let mut hold = held();
    assert!(hold.step(&now(S, &face_lost())));
    // Open again at 5 min: the 10 min run from there.
    assert!(!hold.step(&now(300 * S, &open())));
    hold.answer(CLOSE);
    assert!(hold.step(&now(899 * S, &face_lost())));
    hold.answer(CLOSE);
    assert!(!hold.step(&now(900 * S, &face_lost())));
    assert_eq!(hold.notes().last(), Some(&Note::Ended(End::TooLong)));
    assert_eq!(
        Note::Ended(End::TooLong).line(),
        "held sitting ended (10 min since the gate was open on a face)"
    );
}

#[test]
fn no_close_answer_within_45_s_when_the_face_is_lost_ends_it() {
    let mut hold = Hold::default();
    hold.step(&now(0, &open()));
    // The face lost seconds after the beginning (lamp-06c1, run 4): held,
    // waiting for the first answer.
    for at in [1, 10, 44] {
        assert!(hold.step(&now(at * S, &face_lost())), "{at} s");
    }
    assert!(!hold.step(&now(45 * S, &face_lost())));
    assert!(!hold.held());
    assert_eq!(hold.notes().last(), Some(&Note::Ended(End::NoAnswerYet)));
    assert_eq!(
        Note::Ended(End::NoAnswerYet).line(),
        "held sitting ended (the face lost and no close posture answer within 45 s)"
    );
    // A far answer only, then the 45 s: ended too.
    let mut hold = Hold::default();
    hold.step(&now(0, &open()));
    assert!(hold.step(&now(S, &face_lost())));
    hold.answer(FAR);
    assert!(hold.step(&now(30 * S, &face_lost())), "one far is forgiven");
    assert!(!hold.step(&now(45 * S, &face_lost())));
    assert_eq!(hold.notes().last(), Some(&Note::Ended(End::NoAnswerYet)));
    // An answer before any hold is not taken.
    let mut hold = Hold::default();
    hold.answer(CLOSE);
    assert!(!hold.step(&now(0, &face_lost())));
    assert!(!hold.held());
}

#[test]
fn without_posture_a_lost_face_ends_it_at_once() {
    let mut hold = Hold::default();
    hold.step(&now(0, &open()));
    let lost = face_lost();
    let step = Now {
        posture: false,
        ..now(S, &lost)
    };
    assert!(!hold.step(&step));
    assert_eq!(hold.notes().last(), Some(&Note::Ended(End::NoAnswerYet)));
}

#[test]
fn a_close_first_answer_within_45_s_keeps_it_on() {
    let mut hold = Hold::default();
    hold.step(&now(0, &open()));
    assert!(hold.step(&now(2 * S, &face_lost())));
    hold.answer(CLOSE);
    for at in [32, 45, 120] {
        assert!(hold.step(&now(at * S, &face_lost())), "{at} s");
        hold.answer(CLOSE);
    }
    assert!(hold.held());
    // Open again past 45 s: the grace is the beginning's only.
    let mut hold = Hold::default();
    hold.step(&now(0, &open()));
    assert!(!hold.step(&now(40 * S, &open())));
    assert!(!hold.step(&now(46 * S, &face_lost())));
    assert_eq!(hold.notes().last(), Some(&Note::Ended(End::NoAnswerYet)));
}

#[test]
fn a_hold_begins_with_an_answer_under_30_s_old() {
    let mut hold = Hold::default();
    let opened = open();
    let begin = Now {
        recent_answer: Some(CLOSE),
        ..now(0, &opened)
    };
    assert!(!hold.step(&begin));
    assert_eq!(hold.notes(), [Note::Began]);
    // The face lost at once, then 45 s on: the reused close answer holds.
    assert!(hold.step(&now(S, &face_lost())));
    assert!(hold.step(&now(50 * S, &face_lost())));
    // Taken only at the beginning: a held hold ignores it.
    let begin_far = Now {
        recent_answer: Some(FAR),
        ..now(51 * S, &opened)
    };
    assert!(!hold.step(&begin_far));
    assert!(hold.step(&now(52 * S, &face_lost())));
    assert!(hold.held());
}

#[test]
fn stood_up_privacy_sleep_opt_in_and_forget_end_it_at_once() {
    let cases: [(Inputs, bool, bool, End); 6] = [
        (face_lost(), true, false, End::StoodUp),
        (face_lost(), false, true, End::Forgot),
        (
            Inputs {
                privacy_open: Some(false),
                ..face_lost()
            },
            false,
            false,
            End::Gate(Closed::PrivacyClosed),
        ),
        (
            Inputs {
                asleep: Some(true),
                ..face_lost()
            },
            false,
            false,
            End::Gate(Closed::Asleep),
        ),
        (
            Inputs {
                opted_in: false,
                ..face_lost()
            },
            false,
            false,
            End::Gate(Closed::NoOptIn),
        ),
        (
            Inputs {
                said_on: false,
                ..face_lost()
            },
            false,
            false,
            End::Gate(Closed::NotSaidYet),
        ),
    ];
    for (inputs, stood_up, forgot, end) in cases {
        let mut hold = held();
        assert!(hold.step(&now(S, &face_lost())));
        let step = Now {
            now_us: 2 * S,
            inputs: &inputs,
            stood_up,
            forgot,
            recent_answer: None,
            posture: true,
            face: None,
        };
        assert!(!hold.step(&step), "{end:?}");
        assert!(!hold.held(), "{end:?}");
        assert_eq!(hold.notes().last(), Some(&Note::Ended(end)));
    }
}

#[test]
fn a_lease_or_state_not_known_closes_the_step_and_keeps_the_hold() {
    let mut hold = held();
    for inputs in [
        Inputs {
            privacy_open: None,
            ..face_lost()
        },
        Inputs {
            asleep: None,
            ..face_lost()
        },
    ] {
        assert!(!hold.step(&now(S, &inputs)));
        assert!(hold.held());
    }
    assert!(hold.step(&now(2 * S, &face_lost())));
}

#[test]
fn switched_off_the_inputs_go_on_as_they_came() {
    let lost = face_lost();
    let inputs = wire(None, &[CLOSE], &now(S, &lost));
    assert_eq!(inputs, lost);
    assert_eq!(
        gate::may_request(&inputs),
        Err(Closed::Nobody),
        "closed as before"
    );
}

#[test]
fn held_a_second_face_or_a_guest_goes_on_and_the_keep_wipes_nothing() {
    // No stranger check (the owner's word on #6543; lamp-06c1, run 6: the
    // owner read as a stranger three times, a drink lost): a second face
    // or another face alone in the seat neither ends the hold nor wipes.
    let mut hold = Hold::default();
    let opened = open();
    wire(Some(&mut hold), &[], &now(0, &opened));
    let guest = OnTrack {
        track: Some(Track {
            id: "f2".into(),
            edges: [0.3, 0.3, 0.5, 0.6],
        }),
    };
    for (at, inputs, seen) in [
        (S, open(), guest.clone()),
        (
            2 * S,
            Inputs {
                faces: Some(2),
                ..open()
            },
            OnTrack::default(),
        ),
        (30 * S, open(), guest),
        (31 * S, open(), track()),
    ] {
        let inputs = wire(Some(&mut hold), &[CLOSE], &now(at, &inputs));
        assert_eq!(gate::may_request(&inputs), Ok(()));
        let plan = Keep::default().step(
            &keep::Now {
                now_us: at,
                inputs: &inputs,
                seen: &seen,
                stood_up: false,
                forgot: false,
                posture_gate: Ok(()),
                desk: Ok(()),
                watching: true,
            },
            true,
            false,
        );
        assert_eq!(plan.hard, None);
        assert_eq!(plan.posture, keep::Step::Open);
    }
    assert!(hold.held());
    assert!(
        !hold
            .notes()
            .iter()
            .any(|note| matches!(note, Note::Ended(_)))
    );
}

#[test]
fn the_owner_leaving_and_a_guest_sitting_within_1_5_s_ends_at_the_10_min_re_check() {
    // The Reviewer's P3 on #7476: with two faces in view no id is matched
    // (`on_track` none), so the keep holds the owner's id from before. The
    // owner leaves unseen by `room/stood_up` and a guest sits within 1.5 s,
    // close but showing no face: the hold carries the owner's window and
    // timers under the guest until 10 min since the gate was last open on
    // a face, then the guest's own face wipes them as another face.
    fn keep_step(keep: &mut Keep, at: u64, inputs: &Inputs, seen: &OnTrack) -> keep::Plan {
        let gate = gate::may_request(inputs);
        keep.step(
            &keep::Now {
                now_us: at,
                inputs,
                seen,
                stood_up: false,
                forgot: false,
                posture_gate: gate,
                desk: gate,
                watching: true,
            },
            true,
            false,
        )
    }
    let mut hold = Hold::default();
    let mut keep = Keep::default();
    // The owner alone, then a guest beside them: two faces, no id.
    let opened = wire(Some(&mut hold), &[], &now(0, &open()));
    assert_eq!(
        keep_step(&mut keep, 0, &opened, &track()).posture,
        keep::Step::Open
    );
    let two = Inputs {
        faces: Some(2),
        ..open()
    };
    let both = wire(Some(&mut hold), &[CLOSE], &now(S, &two));
    let plan = keep_step(&mut keep, S, &both, &OnTrack::default());
    assert_eq!(
        (plan.posture, plan.sitting),
        (keep::Step::Open, keep::Step::Open)
    );
    // The owner leaves at 2 s; the guest sits by 3.5 s, face not shown,
    // shoulders close on every answer.
    for at in std::iter::once(2 * S).chain((1..=20).map(|step| step * 30 * S)) {
        let answers: &[Option<f32>] = if at == 2 * S { &[] } else { &[CLOSE] };
        let inputs = wire(Some(&mut hold), answers, &now(at, &face_lost()));
        assert_eq!(gate::may_request(&inputs), Ok(()), "held at {at}");
        let plan = keep_step(&mut keep, at, &inputs, &OnTrack::default());
        assert_eq!(
            (plan.posture, plan.sitting),
            (keep::Step::Open, keep::Step::Open)
        );
    }
    // 10 min since the gate was last open on a face (1 s): the re-check.
    let lost = wire(Some(&mut hold), &[CLOSE], &now(601 * S, &face_lost()));
    assert_eq!(hold.notes().last(), Some(&Note::Ended(End::TooLong)));
    assert_eq!(gate::may_request(&lost), Err(Closed::Nobody));
    let plan = keep_step(&mut keep, 601 * S, &lost, &OnTrack::default());
    assert_eq!(
        (plan.posture, plan.sitting),
        (keep::Step::Began, keep::Step::Began)
    );
    // The guest's own face: another face than the owner's id kept, wiped.
    let guest = OnTrack {
        track: Some(Track {
            id: "f2".into(),
            edges: [0.3, 0.3, 0.5, 0.6],
        }),
    };
    let theirs = wire(Some(&mut hold), &[], &now(610 * S, &open()));
    let plan = keep_step(&mut keep, 610 * S, &theirs, &guest);
    assert_eq!(
        (plan.posture, plan.sitting),
        (
            keep::Step::Wiped(keep::Why::OtherFace),
            keep::Step::Wiped(keep::Why::OtherFace)
        )
    );
}

/// One step at `at` with `face` the one face in view's passing id.
fn with_face<'a>(at: u64, inputs: &'a Inputs, face: Option<&'a str>) -> Now<'a> {
    Now {
        face,
        ..now(at, inputs)
    }
}

/// The keep's step, as `serve` steps it after the hold.
fn keep_after(
    keep: &mut Keep,
    at: u64,
    inputs: &Inputs,
    seen: &OnTrack,
    hold: &[Note],
) -> keep::Plan {
    let gate = gate::may_request(inputs);
    keep.step(
        &keep::Now {
            now_us: at,
            inputs,
            seen,
            stood_up: false,
            forgot: false,
            posture_gate: gate,
            desk: gate,
            watching: true,
        },
        true,
        ended_on_other_face(hold),
    )
}

fn guest() -> OnTrack {
    OnTrack {
        track: Some(Track {
            id: "f2".into(),
            edges: [0.3, 0.3, 0.5, 0.6],
        }),
    }
}

#[test]
fn held_after_the_face_lost_and_a_far_answer_another_face_ends_it_and_wipes_the_sitting() {
    // lamp-06c1, run 8 (#7506): the owner hid, the hold went on on a
    // stale close answer, then one answer had no close person (07:20:33),
    // and another face came into view under its own passing id: it took
    // on the owner's sitting. The first builder's rule on #6543: the next
    // person's timers never count the first one's time.
    let mut hold = Hold::default();
    let mut keep = Keep::default();
    let opened = open();
    let inputs = wire(Some(&mut hold), &[], &with_face(0, &opened, Some("f1")));
    let notes = hold.notes();
    assert_eq!(notes, [Note::Began]);
    let plan = keep_after(&mut keep, 0, &inputs, &track(), &notes);
    assert_eq!(
        (plan.posture, plan.sitting),
        (keep::Step::Open, keep::Step::Open)
    );
    // The owner hides: no face, a close answer, then one far one.
    let lost = face_lost();
    for (at, answers) in [(S, &[CLOSE][..]), (29 * S, &[FAR][..])] {
        let inputs = wire(Some(&mut hold), answers, &with_face(at, &lost, None));
        assert_eq!(gate::may_request(&inputs), Ok(()), "held at {at}");
        let notes = hold.notes();
        assert!(!ended_on_other_face(&notes));
        let plan = keep_after(&mut keep, at, &inputs, &OnTrack::default(), &notes);
        assert_eq!(
            (plan.posture, plan.sitting),
            (keep::Step::Open, keep::Step::Open)
        );
    }
    // Another face comes into view at 41 s: the hold ends on it, and the
    // sitting and posture's window are wiped; a hold begins afresh for it.
    let inputs = wire(
        Some(&mut hold),
        &[],
        &with_face(41 * S, &opened, Some("f2")),
    );
    assert_eq!(gate::may_request(&inputs), Ok(()), "open on its own");
    let notes = hold.notes();
    assert_eq!(notes, [Note::Ended(End::OtherFace), Note::Began]);
    assert_eq!(
        notes[0].line(),
        "held sitting ended (another face in view than the one it was held for)"
    );
    assert!(ended_on_other_face(&notes));
    let plan = keep_after(&mut keep, 41 * S, &inputs, &guest(), &notes);
    assert_eq!(
        (plan.posture, plan.sitting),
        (
            keep::Step::Wiped(keep::Why::OtherFace),
            keep::Step::Wiped(keep::Why::OtherFace)
        )
    );
}

#[test]
fn held_with_a_close_person_throughout_the_owner_back_on_a_new_id_goes_on() {
    // Heron's P1 on #7635: the eyes give a face turned away and back a new
    // passing id as a rule. The owner reads face-down as f1, close in
    // every answer, and looks up as f3: the same body, the hold goes on.
    let mut hold = Hold::default();
    let mut keep = Keep::default();
    let opened = open();
    let lost = face_lost();
    let inputs = wire(Some(&mut hold), &[], &with_face(0, &opened, Some("f1")));
    keep_after(&mut keep, 0, &inputs, &track(), &hold.notes());
    for at in [S, 30 * S, 60 * S] {
        let inputs = wire(Some(&mut hold), &[CLOSE], &with_face(at, &lost, None));
        let plan = keep_after(&mut keep, at, &inputs, &OnTrack::default(), &hold.notes());
        assert_eq!(
            (plan.posture, plan.sitting),
            (keep::Step::Open, keep::Step::Open)
        );
    }
    let up = OnTrack {
        track: Some(Track {
            id: "f3".into(),
            edges: [0.3, 0.3, 0.5, 0.6],
        }),
    };
    let inputs = wire(
        Some(&mut hold),
        &[CLOSE],
        &with_face(70 * S, &opened, Some("f3")),
    );
    let notes = hold.notes();
    assert!(
        !notes.iter().any(|note| matches!(note, Note::Ended(_))),
        "{notes:?}"
    );
    assert!(hold.held());
    let plan = keep_after(&mut keep, 70 * S, &inputs, &up, &notes);
    assert_eq!(
        (plan.posture, plan.sitting),
        (keep::Step::Open, keep::Step::Open)
    );
    // Two faces in view after a far answer: no one face to match, nothing
    // ends; the hold now keeps f3, so f3 alone again goes on too.
    let two = Inputs {
        faces: Some(2),
        ..open()
    };
    wire(Some(&mut hold), &[FAR], &with_face(80 * S, &lost, None));
    wire(Some(&mut hold), &[CLOSE], &with_face(85 * S, &two, None));
    wire(
        Some(&mut hold),
        &[],
        &with_face(90 * S, &opened, Some("f3")),
    );
    assert!(!ended_on_other_face(&hold.notes()));
    assert!(hold.held());
    // A far answer after the next loss, then another id: ended.
    wire(Some(&mut hold), &[CLOSE], &with_face(91 * S, &lost, None));
    wire(Some(&mut hold), &[FAR], &with_face(120 * S, &lost, None));
    wire(
        Some(&mut hold),
        &[],
        &with_face(125 * S, &opened, Some("f2")),
    );
    assert!(ended_on_other_face(&hold.notes()));
}

#[test]
fn held_with_no_one_face_kept_any_face_after_a_far_answer_ends_it() {
    // Begun on two faces, no id known: as the keep, a face after the loss
    // is another face than one it was held for, once the chair may have
    // emptied; with only close answers it goes on.
    let two = Inputs {
        faces: Some(2),
        ..open()
    };
    for (answer, ends) in [(FAR, true), (CLOSE, false)] {
        let mut hold = Hold::default();
        wire(Some(&mut hold), &[], &with_face(0, &two, None));
        hold.answer(CLOSE);
        wire(Some(&mut hold), &[], &with_face(S, &face_lost(), None));
        wire(
            Some(&mut hold),
            &[answer],
            &with_face(20 * S, &face_lost(), None),
        );
        wire(
            Some(&mut hold),
            &[],
            &with_face(21 * S, &open(), Some("f1")),
        );
        assert_eq!(ended_on_other_face(&hold.notes()), ends, "{answer:?}");
    }
}
