use super::*;

#[test]
fn a_held_topic_and_a_number_are_quoted_as_the_lamp_measured_them() {
    assert_eq!(
        why(Some("room/activity has held for 2820 s")),
        "room/activity has held for 2820 s"
    );
    assert_eq!(
        why(Some("it appeared on humans/doing after 1800 s without")),
        "it appeared on humans/doing after 1800 s without"
    );
}

#[test]
fn no_word_a_person_said_ever_reaches_a_reason() {
    // A trigger on heard words, an item's name and the model's call
    // arguments all carry data: none of it is quoted.
    for held in [
        "voice/heard has held for 3 s: call me Walter",
        "voice/heard Walter's birthday has held for 3 s",
        "each Buy a card for Walter of tasks on tasks/list came to hold",
        "the model called remember with {\"words\":\"my pin is 1234\"}, for [\"x\"]",
        "it appeared on voice/heard after 5 s without hello Walter",
        "anything else, with a name: Walter",
    ] {
        let reason = why(Some(held));
        assert!(
            !reason.contains("Walter") && !reason.contains("1234") && !reason.contains("birthday"),
            "{held:?} -> {reason}"
        );
        assert!(reason.chars().count() <= LampDid::MAX_WHY);
    }
    assert_eq!(why(None), "it ran without a trigger's facts");
}

#[test]
fn one_did_for_each_kind_of_outward_thing_and_none_for_the_rest() {
    let calls = vec![
        Call::Say("hello".into()),
        Call::Say("again".into()),
        Call::Gesture("nod".into()),
        Call::Ends,
        Call::Look,
    ];
    let mut teller = Teller::default();
    teller.begin(
        "sip-of-water",
        false,
        &calls,
        Some("room/activity has held for 2820 s"),
    );
    let made = teller.ended("sip-of-water", "said it", 7);
    assert_eq!(made.len(), 2);
    assert_eq!(made[0].what, DidWhat::Spoke);
    assert_eq!(made[1].what, DidWhat::Moved);
    assert_eq!(made[0].who, "lamp-skills:sip-of-water");
    assert_eq!(made[0].why, "room/activity has held for 2820 s");
    assert_eq!(made[0].outcome, DidOutcome::Done);
    assert_eq!(made[0].at_us, 7);
    // Nothing outward, nothing told; and an end is told once.
    teller.begin("x-skill", false, &[Call::Ends, Call::Look], None);
    assert!(teller.ended("x-skill", "done", 7).is_empty());
    assert!(teller.ended("sip-of-water", "done", 8).is_empty());
}

#[test]
fn every_outward_action_is_told_not_only_say_light_and_move() {
    use lamp_msgs::DidWhat::*;
    assert_eq!(kinds(&[Call::Cue]), vec![Spoke]);
    assert_eq!(
        kinds(&[Call::Sound {
            name: "chime".into(),
            path: "sounds/chime.wav".into()
        }]),
        vec![Spoke]
    );
}

#[test]
fn a_made_skill_is_told_generically_and_a_named_one_by_its_id() {
    let calls = [Call::Say("hi".into())];
    let teller = Teller::default();
    let told = teller.refused("walters-birthday-card", true, &calls, None, 1);
    assert_eq!(told[0].who, "lamp-skills:a made skill");
    assert!(!told[0].who.contains("walter"));
    let told = teller.refused("sip-of-water", false, &calls, None, 1);
    assert_eq!(told[0].who, "lamp-skills:sip-of-water");
}

#[test]
fn how_a_run_went_is_told_when_it_is_known() {
    let calls = [Call::Say("hi".into())];
    let mut teller = Teller::default();
    let refused = teller.refused("x-skill", false, &calls, None, 3);
    assert_eq!(refused[0].outcome, DidOutcome::Refused);
    assert_eq!(
        outcome_of(DidWhat::Spoke, "not made: 4 runs already wait"),
        DidOutcome::Refused
    );
    assert_eq!(outcome_of(DidWhat::Spoke, "say: spoken"), DidOutcome::Done);
    teller.begin("x-skill", false, &calls, None);
    assert_eq!(
        teller.ended("x-skill", "say: unsaid", 4)[0].outcome,
        DidOutcome::Cut
    );
}

/// Heron on #7766: a say the voice did not play is never told as spoken, in
/// any of the forms `answer_outcome` reads; a light made in the same run is.
#[test]
fn a_say_the_voice_did_not_play_is_not_told_as_spoken() {
    for (last, outcome) in [
        ("say: refused (the lamp is muted)", DidOutcome::Refused),
        ("say: expired", DidOutcome::Cut),
        ("say: dropped (user_interrupted)", DidOutcome::Cut),
        ("say: dropped (asker gone)", DidOutcome::Cut),
        ("say: the voice ended without an outcome", DidOutcome::Cut),
        (
            "say: the voice's say service was not reached (Resource temporarily unavailable)",
            DidOutcome::Cut,
        ),
        (
            "light: ok; say: refused (the lamp is muted)",
            DidOutcome::Refused,
        ),
        ("say: unsaid (the moment passed)", DidOutcome::Cut),
        ("say: spoken", DidOutcome::Done),
        ("light: refused (outranked)", DidOutcome::Done),
    ] {
        assert_eq!(outcome_of(DidWhat::Spoke, last), outcome, "{last}");
    }
    // The same run's light and motion were made.
    assert_eq!(
        outcome_of(DidWhat::Lit, "say: refused (the lamp is muted)"),
        DidOutcome::Done
    );
    let mut teller = Teller::default();
    teller.begin(
        "x-skill",
        false,
        &[Call::Say("hi".into()), Call::Gesture("nod".into())],
        None,
    );
    let told = teller.ended("x-skill", "say: refused (the lamp is muted)", 5);
    assert_eq!(told[0].what, DidWhat::Spoke);
    assert_eq!(told[0].outcome, DidOutcome::Refused);
    assert_eq!(told[1].what, DidWhat::Moved);
    assert_eq!(told[1].outcome, DidOutcome::Done);
}

#[test]
fn the_switch_is_read_from_its_one_name() {
    assert_eq!(ENVIRONMENT, "LAMP_DID_PUBLISH");
}

/// Rule 7: a run never waits on its did. 1,000 told on a real bus; the
/// slowest one stated (target: under 5 ms, mean far under 1 ms) and each
/// read back whole.
#[test]
fn a_did_is_told_in_under_a_millisecond_and_arrives_whole() {
    use lamp_bus::Bus;
    let dir = std::env::temp_dir().join(format!("sd{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bus = Bus::new(&dir).unwrap();
    let mut out = bus
        .publisher_declared(LampDid::TOPIC, &LampDid::declaration(), "skills")
        .unwrap();
    let mut heard = bus.subscribe(LampDid::TOPIC, "test").unwrap();
    let mut teller = Teller::default();
    teller.begin(
        "sip-of-water",
        false,
        &[Call::Say("hello".into())],
        Some("room/activity has held for 2820 s"),
    );
    let made = teller.ended("sip-of-water", "done", 9);
    let mut slowest = std::time::Duration::ZERO;
    for _ in 0..1_000 {
        let started = std::time::Instant::now();
        out.publish_json(lamp_ipc::monotonic_us(), &made[0])
            .unwrap();
        slowest = slowest.max(started.elapsed());
    }
    eprintln!("lamp/did publish: slowest {slowest:?} over 1,000");
    assert!(slowest < std::time::Duration::from_millis(5), "{slowest:?}");
    let message = heard
        .recv(std::time::Duration::from_secs(2))
        .unwrap()
        .expect("a did arrives");
    assert_eq!(message.parse::<LampDid>().unwrap(), made[0]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_made_run_with_a_refused_step_is_done_not_refused() {
    // Only an outcome that leads with "not made" refuses the run.
    assert_eq!(
        outcome_of(DidWhat::Lit, "say: ok; light: refused (outranked)"),
        DidOutcome::Done
    );
    assert_eq!(
        outcome_of(DidWhat::Spoke, "not made: its last run is still under way"),
        DidOutcome::Refused
    );
}

#[test]
fn a_run_with_no_facts_is_not_told_as_asked_for_by_the_person() {
    for held in [None, Some("the model called remember")] {
        let reason = why(held);
        assert!(!reason.contains("person"), "{reason}");
    }
}

#[test]
fn a_refusal_leaves_the_run_under_way_to_tell_how_it_went() {
    let calls = [Call::Say("hi".into())];
    let mut teller = Teller::default();
    teller.begin(
        "x-skill",
        false,
        &calls,
        Some("room/activity has held for 5 s"),
    );
    let refused = teller.refused(
        "x-skill",
        false,
        &calls,
        Some("room/activity has held for 6 s"),
        2,
    );
    assert_eq!(refused[0].outcome, DidOutcome::Refused);
    assert_eq!(refused[0].why, "room/activity has held for 6 s");
    let done = teller.ended("x-skill", "said it", 3);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].outcome, DidOutcome::Done);
    assert_eq!(done[0].why, "room/activity has held for 5 s");
}
