use lamp_eyes::who::{MOST_PRINTS, Taking, ask_of};

#[test]
fn a_speaker_bound_enrollment_request_is_understood() {
    let ask = br#"{"remember_speaker":{"name":"Fixture Speaker","face":"boot/f3","began_us":1000000,"ended_us":3000000,"replace":false,"apart":false}}"#;
    assert!(
        ask_of(ask).is_some(),
        "the guarded enrollment protocol is absent"
    );
}

#[test]
fn the_legacy_capture_contract_stays_available_when_not_guarded() {
    let mut taking = Taking::new("Fixture Speaker".into(), 1);
    for _ in 0..MOST_PRINTS {
        taking.look(&[0.2], Some(vec![1.0; lamp_eyes::who::PRINT]));
    }
    assert_eq!(taking.taken().unwrap().len(), MOST_PRINTS);
}

fn print(index: usize) -> Vec<f32> {
    let mut p = vec![0.0; lamp_eyes::who::PRINT];
    p[index] = 1.0;
    p
}
fn guarded() -> Taking {
    use lamp_eyes::remember_speaker::{Anchors, Request};
    let mut anchors = Anchors::default();
    anchors.refresh(1, Some(7));
    anchors.observe("boot/f3", &print(0), 2_000_000, 2_010_000, Some(7), Some(7));
    let request = Request {
        name: "Fixture Speaker".into(),
        face: "boot/f3".into(),
        began_us: 1_000_000,
        ended_us: 3_000_000,
        replace: false,
        apart: false,
    };
    let mut taking = Taking::new(request.name.clone(), 3_100_000);
    taking.one_face = Some(false);
    taking.speaker = Some(anchors.begin(&request, 3_100_000, Some(7)).unwrap());
    taking
}

#[test]
fn the_real_capture_collector_never_mixes_in_a_replacement_under_the_same_id() {
    let mut taking = guarded();
    for n in 0..MOST_PRINTS {
        let at = 3_200_000 + n as u64 * 100_000;
        taking.look_seen(
            &[0.2],
            Some(print(usize::from(n > 0))),
            Some("boot/f3"),
            at,
            at + 10_000,
            Some(7),
        );
    }
    assert!(taking.taken().is_none());
    assert_eq!(
        taking
            .speaker
            .as_ref()
            .unwrap()
            .checked(4_000_000, Some(7), false),
        Err("not_that_face")
    );
}

#[test]
fn five_fresh_prints_of_the_requesting_speaker_reach_the_final_fence() {
    let mut taking = guarded();
    for n in 0..MOST_PRINTS {
        let at = 3_200_000 + n as u64 * 100_000;
        taking.look_seen(
            &[0.2],
            Some(print(0)),
            Some("boot/f3"),
            at,
            at + 10_000,
            Some(7),
        );
    }
    assert_eq!(taking.taken().unwrap().len(), MOST_PRINTS);
    let guard = taking.speaker.as_ref().unwrap();
    assert!(guard.checked(4_000_000, Some(7), false).is_ok());
    assert_eq!(guard.checked(4_000_000, Some(7), true), Err("peer_closed"));
    assert_eq!(guard.checked(4_000_000, Some(8), false), Err("eyes_shut"));
}

#[test]
fn missing_or_ambiguous_frames_end_a_capture_and_a_return_cannot_rescue_it() {
    for heights in [vec![], vec![0.2, 0.2]] {
        let mut taking = guarded();
        taking.look_seen(&heights, None, None, 3_200_000, 3_210_000, Some(7));
        for n in 0..MOST_PRINTS {
            let at = 3_300_000 + n as u64 * 100_000;
            taking.look_seen(
                &[0.2],
                Some(print(0)),
                Some("boot/f3"),
                at,
                at + 10_000,
                Some(7),
            );
        }
        assert!(taking.taken().is_none());
    }
}

#[test]
fn guarded_proof_cannot_be_mixed_with_a_legacy_operation_or_unknown_fields() {
    for line in [
        r#"{"remember":"Fixture Speaker","speaker":{"face":"boot/f3"}}"#,
        r#"{"remember_speaker":{"name":"Fixture Speaker","face":"boot/f3","began_us":1,"ended_us":2,"replace":false,"apart":false},"remember":"Other"}"#,
        r#"{"remember_speaker":{"name":"Fixture Speaker","face":"boot/f3","began_us":1,"ended_us":2,"replace":false,"apart":false,"fallback":true}}"#,
        r#"{"remember_speaker":{"name":"Fixture Speaker","face":"boot/f3","began_us":1,"ended_us":2}}"#,
    ] {
        assert!(ask_of(line.as_bytes()).is_none(), "{line}");
    }
}
