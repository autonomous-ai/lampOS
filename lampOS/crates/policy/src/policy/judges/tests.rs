use super::*;
use crate::{decide::Answer, gather::LampThen, policy::Timing};
use lamp_msgs::Face;

fn judgement(p: f32) -> Judgement {
    Judgement {
        probability: p,
        choice: Choice::at(p),
        from_rule: false,
    }
}

fn context() -> Context {
    Context {
        heard: "one word".into(),
        room: Room::Quiet,
        lamp_is_speaking: false,
        lamp_last_said: None,
        seconds_since_lamp_spoke: None,
        name_heard: None,
        like_asker: None,
    }
}

fn utterance(at: Instant) -> Utterance {
    Utterance {
        text: "one word".into(),
        pieces: 1,
        request: None,
        began: at,
        ended: at,
        sent: (at, at),
        lamp: LampThen {
            speaking: false,
            said: String::new(),
            spoke: None,
        },
        cut: false,
    }
}

#[test]
fn quiet_room_without_call_or_slow_answer_looks_up_even_if_local_is_confident() {
    let weights = local::Weights::seed();
    let cues = local::Cues {
        room: Some(local::RoomCue::Quiet),
        ..local::Cues::default()
    };
    for p in [0.0, 0.5, 1.0] {
        let result = combine(&weights, &cues, Some(judgement(p)), None);
        assert_eq!(result.judgement.choice, Choice::LookUp);
        assert_eq!(result.judgement.probability, p);
        assert!(!result.judgement.from_rule);
        assert_eq!(result.guard, Some("no_timely_slow_no_call"));
    }
    let result = combine(&weights, &cues, None, None);
    assert!(result.judgement.from_rule);
    assert_eq!(result.judgement.choice, Choice::LookUp);
}

#[test]
fn one_judge_failure_preserves_the_other_probability_and_both_use_separate_probabilities() {
    let weights = local::Weights::seed();
    let cues = local::Cues {
        text_called: Some(true),
        ..local::Cues::default()
    };
    let left = judgement(0.9);
    let right = judgement(0.3);
    assert_eq!(combine(&weights, &cues, Some(left), None).judgement, left);
    assert_eq!(combine(&weights, &cues, None, Some(right)).judgement, right);
    let both = combine(&weights, &cues, Some(left), Some(right));
    assert_eq!(
        both.judgement.probability,
        weights.local_share() * 0.9 + (1.0 - weights.local_share()) * 0.3
    );
    assert_eq!(both.judgement.choice, Choice::LookUp);
    assert_eq!(both.by, "local+jev");
}

#[test]
fn busy_weak_name_is_not_a_call_but_sure_name_conversation_text_and_fresh_pose_are() {
    let weights = local::Weights::seed();
    let base = local::Cues {
        room: Some(local::RoomCue::Busy),
        name_score: Some(0.993),
        ..local::Cues::default()
    };
    assert_eq!(
        combine(&weights, &base, Some(judgement(0.99)), None)
            .judgement
            .choice,
        Choice::LookUp
    );
    for cues in [
        local::Cues {
            name_score: Some(0.999),
            ..base.clone()
        },
        local::Cues {
            text_called: Some(true),
            ..base.clone()
        },
        local::Cues {
            conversation: Some(true),
            ..base.clone()
        },
        local::Cues {
            facing: Some(true),
            face_age_ms: Some(1999),
            ..base.clone()
        },
    ] {
        assert_eq!(
            combine(&weights, &cues, Some(judgement(0.99)), None)
                .judgement
                .choice,
            Choice::Answer
        );
    }
    for age in [None, Some(2000), Some(2001)] {
        let cues = local::Cues {
            facing: Some(true),
            face_age_ms: age,
            ..base.clone()
        };
        assert_eq!(
            combine(&weights, &cues, Some(judgement(0.99)), None)
                .judgement
                .choice,
            Choice::LookUp
        );
    }
}

fn face(facing: Option<bool>, height: f32) -> Face {
    Face {
        speaking: None,
        id: "authored".into(),
        xmin: 0.1,
        xmax: 0.4,
        ymin: 0.0,
        ymax: height,
        confidence: 1.0,
        yaw: None,
        facing,
    }
}

#[test]
fn missing_pose_is_not_away_and_an_empty_or_distant_view_is_unknown() {
    let mut judges = Judges::new(Options::default());
    let at = Instant::now();
    for (faces, expected) in [
        (vec![], None),
        (vec![face(None, 0.3)], None),
        (vec![face(Some(true), 0.1)], None),
        (vec![face(Some(false), 0.3)], Some(false)),
        (vec![face(Some(false), 0.3), face(None, 0.3)], None),
        (
            vec![face(Some(false), 0.3), face(Some(true), 0.3)],
            Some(true),
        ),
        (
            vec![face(Some(false), 0.3), face(Some(true), f32::NAN)],
            None,
        ),
    ] {
        judges.faces(&Faces { faces }, at);
        assert_eq!(
            judges
                .cues(&context(), &utterance(at), Timing::LAMP.seen_for)
                .facing,
            expected
        );
    }
}

#[test]
fn cue_receipt_history_excludes_future_faces_and_later_voice_knowledge() {
    let mut judges = Judges::new(Options::default());
    let at = Instant::now();
    judges.faces(
        &Faces {
            faces: vec![face(Some(false), 0.3)],
        },
        at,
    );
    judges.faces(
        &Faces {
            faces: vec![face(Some(true), 0.3)],
        },
        at + Duration::from_millis(5),
    );
    judges.voice_seen(at + Duration::from_millis(5));
    let frozen = judges.cues(&context(), &utterance(at), Timing::LAMP.seen_for);
    assert_eq!(frozen.facing, Some(false));
    assert_eq!(frozen.face_age_ms, Some(0));
    assert_eq!(frozen.conversation, None);
    assert_eq!(frozen.word_count, Some(2));
    assert_eq!(frozen.duration_ms, None);
    let later = at + Duration::from_millis(10);
    assert_eq!(
        judges
            .cues(&context(), &utterance(later), Timing::LAMP.seen_for)
            .conversation,
        Some(false)
    );
    judges.voice_gone(later);
    judges.eyes_gone(later);
    let gone = judges.cues(&context(), &utterance(later), Timing::LAMP.seen_for);
    assert_eq!(gone.conversation, None);
    assert_eq!(gone.facing, None);
}

#[test]
fn stale_and_evicted_cues_stay_unknown_and_cut_text_has_no_length() {
    let mut judges = Judges::new(Options::default());
    let at = Instant::now();
    judges.faces(
        &Faces {
            faces: vec![face(Some(true), 0.3)],
        },
        at,
    );
    let mut heard = utterance(at + Duration::from_millis(2001));
    heard.cut = true;
    let cues = judges.cues(&context(), &heard, Timing::LAMP.seen_for);
    assert_eq!(
        (cues.facing, cues.face_age_ms, cues.word_count),
        (None, None, None)
    );
    for n in 1..=40 {
        judges.voice_seen(at + Duration::from_millis(n));
    }
    assert_eq!(judges.tracker.voice.len(), 32);
    assert_eq!(
        judges
            .cues(&context(), &utterance(at), Timing::LAMP.seen_for)
            .conversation,
        None
    );
}

#[test]
fn malformed_answers_fail_whole_judge_and_notes_never_hold_service_text() {
    let asked = Asked {
        answers: [("calls_lamp".into(), Answer::Noul(f32::NAN))].into(),
        model: "secret-service-text".into(),
        took: Duration::ZERO,
        usage: None,
        connection: None,
    };
    let row = Answered::of(Ok(asked), Duration::ZERO, |_| {
        panic!("invalid answers must not be combined")
    });
    assert_eq!(row.error, Some("missing_or_invalid_question"));
    assert!(row.answers.is_empty() && row.judgement.is_none());
    let notice = row.notice(1, "slow", "timely", false).to_string();
    assert!(!notice.contains("secret-service-text"));
}

#[test]
fn invalid_budgets_are_rejected_before_startup() {
    for deadline in [Duration::ZERO, Duration::from_millis(1001)] {
        assert!(
            Options {
                deadline,
                ..Options::default()
            }
            .validate()
            .is_err()
        );
    }
    for local_deadline in [Duration::ZERO, Duration::from_millis(101)] {
        assert!(
            Options {
                local_deadline,
                ..Options::default()
            }
            .validate()
            .is_err()
        );
    }
    assert!(Options::default().validate().is_ok());
}

#[test]
fn both_completion_and_receipt_must_precede_the_original_deadline() {
    let at = Instant::now();
    let cutoff = at + Duration::from_millis(100);
    let before = cutoff - Duration::from_nanos(1);
    let flight = Flight {
        number: 1,
        context: context(),
        started: at,
        cutoff,
        closed: None,
        job: Some(Job {
            number: 1,
            waiting: Waiting {
                utterance: utterance(at),
                context: context(),
                eligible: at,
                seen: None,
                local: Some(local::Cues::default()),
            },
            local: Answered::absent("failed"),
        }),
    };
    assert!(flight.timely(before, before));
    assert!(!flight.timely(before, cutoff));
    assert!(!flight.timely(cutoff, before));
    assert!(!flight.timely(before, cutoff + Duration::from_secs(1)));
    let mut judges = Judges::new(Options::default());
    judges.flight = Some(flight);
    assert_eq!(judges.wait(at), Duration::from_millis(5));
    assert_eq!(judges.wait(before), Duration::from_nanos(1));
    assert_eq!(judges.wait(cutoff), Duration::ZERO);
    let flight = judges.flight.as_mut().unwrap();
    flight.job.take();
    assert!(
        !flight.timely(before, before),
        "an early closed choice is never revised"
    );
    assert_eq!(judges.wait(cutoff), Duration::from_millis(20));
}
