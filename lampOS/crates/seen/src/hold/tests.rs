use super::*;
use crate::keep::{self, Keep};
use crate::pause::Pause;
use crate::watch::Track;

const S: u64 = 1_000_000;
const CLOSE: Option<f32> = Some(0.25);
const FAR: Option<f32> = Some(0.08);

/// The gate open on the owner's face.
fn open() -> Inputs {
    Inputs {
        opted_in: true,
        said_on: true,
        privacy_open: Some(true),
        someone: Some(true),
        faces: Some(1),
        familiar: Some(true),
        asleep: Some(false),
    }
}

fn face_lost() -> Inputs {
    Inputs {
        faces: Some(0),
        familiar: None,
        ..open()
    }
}

/// No one face in view with a reading on it.
static NO_TRACK: OnTrack = OnTrack {
    track: None,
    stranger: false,
    reading_at: None,
};

fn now(now_us: u64, inputs: &Inputs) -> Now<'_> {
    seeing(now_us, inputs, &NO_TRACK)
}

fn seeing<'a>(now_us: u64, inputs: &'a Inputs, seen: &'a OnTrack) -> Now<'a> {
    Now {
        now_us,
        inputs,
        stood_up: false,
        forgot: false,
        seen,
        recent_answer: None,
        posture: true,
    }
}

fn track() -> OnTrack {
    OnTrack {
        track: Some(Track {
            id: "f1".into(),
            edges: [0.3, 0.3, 0.5, 0.6],
        }),
        stranger: false,
        reading_at: Some(0),
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
    // A misread, a stranger reading and a second face don't end it.
    for inputs in [
        Inputs {
            familiar: Some(false),
            ..open()
        },
        Inputs {
            faces: Some(2),
            familiar: None,
            ..open()
        },
        Inputs {
            someone: Some(false),
            faces: None,
            ..open()
        },
    ] {
        assert!(hold.step(&now(591 * S, &inputs)), "{inputs:?}");
    }
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
fn it_ends_10_min_after_the_face_was_last_recognised() {
    let mut hold = held();
    assert!(hold.step(&now(S, &face_lost())));
    // Recognised again at 5 min: the 10 min run from there.
    assert!(!hold.step(&now(300 * S, &open())));
    hold.answer(CLOSE);
    assert!(hold.step(&now(899 * S, &face_lost())));
    hold.answer(CLOSE);
    assert!(!hold.step(&now(900 * S, &face_lost())));
    assert_eq!(hold.notes().last(), Some(&Note::Ended(End::TooLong)));
    assert_eq!(
        Note::Ended(End::TooLong).line(),
        "held sitting ended (10 min since the owner's face was recognised)"
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
    // Recognised again past 45 s: the grace is the beginning's only.
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
            seen: &NO_TRACK,
            recent_answer: None,
            posture: true,
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
    let stranger = OnTrack {
        stranger: true,
        ..track()
    };
    let (inputs, seen) = wire(None, &[CLOSE], &seeing(S, &lost, &stranger));
    assert_eq!(inputs, lost);
    assert_eq!(seen, stranger);
    assert_eq!(
        gate::may_request(&inputs),
        Err(Closed::Nobody),
        "closed as before"
    );
}

#[test]
fn held_open_the_pause_and_the_keep_see_the_gate_open() {
    let mut hold = Hold::default();
    let opened = open();
    wire(Some(&mut hold), &[], &seeing(0, &opened, &track()));
    // A stranger reading on the same face, a second face: the pause and
    // the keep would pause or wipe; held, they see the gate open.
    for (at, inputs) in [
        (
            S,
            Inputs {
                familiar: Some(false),
                ..open()
            },
        ),
        (
            2 * S,
            Inputs {
                faces: Some(2),
                familiar: None,
                ..open()
            },
        ),
    ] {
        let stranger = OnTrack {
            stranger: true,
            ..track()
        };
        let (inputs, seen) = wire(Some(&mut hold), &[CLOSE], &seeing(at, &inputs, &stranger));
        assert_eq!(gate::may_request(&inputs), Ok(()));
        let mut pause = Pause::default();
        let verdict = pause.step(at, &inputs, &seen);
        assert!(!verdict.paused && !verdict.wipe);
        let plan = Keep::default().step(&keep::Now {
            now_us: at,
            inputs: &inputs,
            seen: &seen,
            verdict,
            pause_lost: false,
            stood_up: false,
            forgot: false,
            posture_gate: Ok(()),
            desk: Ok(()),
            watching: true,
            sitting: true,
        });
        assert_eq!(plan.hard, None);
        assert_eq!(plan.posture, keep::Step::Open);
    }
}

/// One face alone in view, read as a stranger.
fn stranger_alone() -> Inputs {
    Inputs {
        familiar: Some(false),
        ..open()
    }
}

fn stranger_track() -> OnTrack {
    OnTrack {
        stranger: true,
        ..track()
    }
}

#[test]
fn a_close_stranger_alone_for_10_s_ends_it_at_once() {
    let mut hold = held();
    let (inputs, seen) = (stranger_alone(), stranger_track());
    // The count runs from the first reading, at 1 s.
    for at in 1..=10 {
        assert!(hold.step(&seeing(at * S, &inputs, &seen)), "{at} s");
    }
    assert!(hold.held());
    assert!(!hold.step(&seeing(11 * S, &inputs, &seen)));
    assert!(!hold.held());
    assert_eq!(hold.notes().last(), Some(&Note::Ended(End::StrangerAlone)));
    assert_eq!(
        Note::Ended(End::StrangerAlone).line(),
        "held sitting ended (another person alone in the seat for 10 s)"
    );
    // Then the gate is closed as a stranger's, so #7251's wipe applies.
    let (inputs, _) = wire(Some(&mut hold), &[CLOSE], &seeing(12 * S, &inputs, &seen));
    assert_eq!(gate::may_request(&inputs), Err(Closed::Stranger));
}

#[test]
fn nine_s_then_an_owner_reading_starts_the_count_again() {
    let mut hold = held();
    let inputs = stranger_alone();
    // Another person on "f2"; the owner's own "f1" never counts.
    let seen = OnTrack {
        track: Some(Track {
            id: "f2".into(),
            edges: [0.3, 0.3, 0.5, 0.6],
        }),
        ..stranger_track()
    };
    for at in 1..=9 {
        assert!(hold.step(&seeing(at * S, &inputs, &seen)), "{at} s");
    }
    // The owner recognised on "f1": the gate opens itself.
    assert!(!hold.step(&seeing(10 * S, &open(), &track())));
    for at in 11..=19 {
        assert!(hold.step(&seeing(at * S, &inputs, &seen)), "{at} s");
    }
    assert!(hold.held());
    // Nine seconds more: the count runs from 11 s.
    assert!(!hold.step(&seeing(21 * S, &inputs, &seen)));
    assert_eq!(hold.notes().last(), Some(&Note::Ended(End::StrangerAlone)));
}

#[test]
fn a_far_stranger_alone_does_not_end_it() {
    let mut hold = held();
    hold.answer(FAR);
    let (inputs, seen) = (stranger_alone(), stranger_track());
    for at in 1..=29 {
        assert!(hold.step(&seeing(at * S, &inputs, &seen)), "{at} s");
    }
    assert!(hold.held());
    assert!(!hold.notes().contains(&Note::Ended(End::StrangerAlone)));
}

#[test]
fn two_faces_one_a_stranger_do_not_end_it() {
    let mut hold = held();
    // A second face beside the seated person: no one face alone, so the
    // eyes' track is none (`Readings::on_track`).
    let two = Inputs {
        faces: Some(2),
        familiar: Some(false),
        ..open()
    };
    for at in 1..=29 {
        assert!(hold.step(&seeing(at * S, &two, &NO_TRACK)), "{at} s");
        if at % 15 == 0 {
            hold.answer(CLOSE);
        }
    }
    // A stranger alone for 9 s, a second face for a step, then alone again:
    // the count starts again.
    let (alone, seen) = (stranger_alone(), stranger_track());
    for at in 30..=38 {
        assert!(hold.step(&seeing(at * S, &alone, &seen)));
    }
    assert!(hold.step(&seeing(39 * S, &two, &NO_TRACK)));
    for at in 40..=48 {
        assert!(hold.step(&seeing(at * S, &alone, &seen)), "{at} s");
    }
    assert!(hold.held());
    assert!(!hold.notes().contains(&Note::Ended(End::StrangerAlone)));
}

#[test]
fn a_misread_of_1_to_3_s_does_not_end_it() {
    let mut hold = held();
    let (inputs, seen) = (stranger_alone(), stranger_track());
    let mut at = 0;
    for misread in [1, 2, 3, 3, 2] {
        for _ in 0..misread {
            at += 1;
            assert!(hold.step(&seeing(at * S, &inputs, &seen)), "{at} s");
        }
        at += 1;
        assert!(!hold.step(&seeing(at * S, &open(), &track())));
    }
    assert!(hold.held());
    // A new id (the face gone, another come) starts the count again too.
    let on = |id: &str| OnTrack {
        track: Some(Track {
            id: id.into(),
            edges: [0.3, 0.3, 0.5, 0.6],
        }),
        ..stranger_track()
    };
    let (first, other) = (on("f2"), on("f3"));
    for step in 1..=9 {
        assert!(hold.step(&seeing((at + step) * S, &inputs, &first)));
    }
    assert!(hold.step(&seeing((at + 10) * S, &inputs, &other)));
    assert!(hold.held());
}

#[test]
fn switched_off_a_stranger_alone_is_as_it_came() {
    let (inputs, seen) = (stranger_alone(), stranger_track());
    let (wired, wired_seen) = wire(None, &[CLOSE], &seeing(11 * S, &inputs, &seen));
    assert_eq!(wired, inputs);
    assert_eq!(wired_seen, seen);
    assert_eq!(gate::may_request(&wired), Err(Closed::Stranger));
}

/// The owner seen on "f1", then the face alone read as a stranger on it.
fn held_on_the_owners_face() -> Hold {
    let mut hold = Hold::default();
    let opened = open();
    assert!(!hold.step(&seeing(0, &opened, &track())));
    hold.answer(CLOSE);
    assert_eq!(hold.notes(), [Note::Began]);
    hold
}

#[test]
fn a_stranger_reading_on_the_owners_own_id_never_ends_it() {
    let mut hold = held_on_the_owners_face();
    let (inputs, seen) = (stranger_alone(), stranger_track());
    // The owner slumped or turned (lamp-06c1, run 4, 15:42:29): the same
    // id read as a stranger for a minute.
    for at in 1..=60 {
        assert!(hold.step(&seeing(at * S, &inputs, &seen)), "{at} s");
        if at % 30 == 0 {
            hold.answer(CLOSE);
        }
    }
    assert!(hold.held());
    assert!(!hold.notes().contains(&Note::Ended(End::StrangerAlone)));
    // A new id read as a stranger for 10 s alone still ends it.
    let other = OnTrack {
        track: Some(Track {
            id: "f2".into(),
            edges: [0.3, 0.3, 0.5, 0.6],
        }),
        ..stranger_track()
    };
    for at in 61..=70 {
        assert!(hold.step(&seeing(at * S, &inputs, &other)), "{at} s");
    }
    assert!(!hold.step(&seeing(71 * S, &inputs, &other)));
    assert_eq!(hold.notes().last(), Some(&Note::Ended(End::StrangerAlone)));
}

#[test]
fn held_a_stranger_reading_on_the_owners_id_wipes_nothing() {
    let mut hold = held_on_the_owners_face();
    let seen = stranger_track();
    // A lease not known this step: the hold opens nothing, yet the
    // misread on the owner's id is not a stranger to the pause or the keep.
    let unknown = Inputs {
        privacy_open: None,
        ..stranger_alone()
    };
    for (at, inputs) in [(S, stranger_alone()), (2 * S, unknown)] {
        let (inputs, wired) = wire(Some(&mut hold), &[], &seeing(at, &inputs, &seen));
        assert!(!wired.stranger, "{at}");
        let mut pause = Pause::default();
        let verdict = pause.step(at, &inputs, &wired);
        let plan = Keep::default().step(&keep::Now {
            now_us: at,
            inputs: &inputs,
            seen: &wired,
            verdict,
            pause_lost: false,
            stood_up: false,
            forgot: false,
            posture_gate: gate::may_request(&inputs),
            desk: Ok(()),
            watching: true,
            sitting: true,
        });
        assert_ne!(plan.hard, Some(keep::Hard::Stranger), "{at}");
    }
    assert!(hold.held());
    // On another id, unheld, the stranger is seen as it came.
    let mut idle = Hold::default();
    let (_, wired) = wire(
        Some(&mut idle),
        &[],
        &seeing(3 * S, &stranger_alone(), &seen),
    );
    assert!(wired.stranger);
}
