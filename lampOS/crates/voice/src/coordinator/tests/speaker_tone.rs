//! The self-test's speaker check over the bus, fed the evidence of the
//! lamp's own speaking turns; nothing is played.
use super::*;
use crate::coordinator::speaker_tone::SpeakerEcho;
use crate::speaker_check::{ROOM_BLOCKS, TURN_BLOCKS};
use lamp_audio::evidence::{EchoEvidence, Level, QUIET_BLOCKS_CEILING};
use lamp_bus::Bus;
use lamp_msgs::{SpeakerCheck, SpeakerCheckRequest};

fn bus(name: &str) -> (Bus, std::path::PathBuf) {
    let root = PathBuf::from(format!("/tmp/lv-echo-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    (Bus::new(&root).unwrap(), root)
}

fn answer(bus: &Bus) -> SpeakerCheck {
    bus.latest(SpeakerCheck::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap()
}

fn ask(bus: &Bus, id: &str, occasion: &str) {
    bus.publisher_declared(
        SpeakerCheckRequest::TOPIC,
        &SpeakerCheckRequest::declaration(),
        "selftest",
    )
    .unwrap()
    .publish_json(
        1,
        &SpeakerCheckRequest {
            id: id.into(),
            occasion: occasion.into(),
        },
    )
    .unwrap();
}

fn evidence(quiet: u8, capture_db: i16, residual_db: i16) -> EchoEvidence {
    EchoEvidence {
        blocks: 1,
        render_active_blocks: 0,
        render_quiet_blocks: quiet,
        capture_level: Level::from_tenths_of_db(capture_db * 10),
        residual_level: Level::from_tenths_of_db(residual_db * 10),
        echo: None,
        silent_at_echo: None,
        echo_delay_blocks: None,
    }
}

/// The room, the lamp speaking long enough to be measured, then quiet.
fn speak(check: &mut SpeakerEcho) {
    for _ in 0..ROOM_BLOCKS {
        check.observe(Some(&evidence(QUIET_BLOCKS_CEILING, -60, -61)), 0);
    }
    for _ in 0..TURN_BLOCKS {
        check.observe(Some(&evidence(0, -15, -70)), 1_000_000);
    }
    for quiet in 1..=QUIET_BLOCKS_CEILING {
        check.observe(Some(&evidence(quiet, -60, -61)), 2_000_000);
    }
}

#[test]
fn a_request_is_answered_at_once_from_the_last_speaking_turn() {
    let (bus, root) = bus("answered");
    let mut check = SpeakerEcho::on(&bus).unwrap();
    ask(&bus, "early", "on_demand");
    check.answer_requests(monotonic_us());
    let early = answer(&bus);
    assert_eq!(
        (early.id.as_str(), early.verdict.as_str()),
        ("early", "not_checked")
    );
    assert!(early.reason.contains("not spoken"), "{early:?}");
    speak(&mut check);
    // Frames without evidence change nothing.
    check.observe(None, 3_000_000);
    ask(&bus, "after", "factory");
    check.answer_requests(61_000_000);
    let after = answer(&bus);
    assert_eq!(after.id, "after");
    assert_eq!(after.verdict, "pass", "{after:?}");
    assert_eq!(after.cancelled_db, Some(55.0));
    assert_eq!(after.cancelled_median_db, Some(55.0));
    // The turn's last speaking block was read a minute before the answer.
    assert_eq!(after.turn_age_s, Some(60));
    assert_eq!(after.rise_db, Some(45.0));
    assert_eq!(after.cancelled_lower_bound, Some(false));
    assert_eq!(after.delay_ms, None);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn without_the_switch_on_there_is_no_check_and_no_bus_is_opened() {
    for switch in [None, Some("off"), Some("")] {
        assert!(
            SpeakerEcho::from_switch(switch, || panic!("the bus was opened for {switch:?}"))
                .is_none()
        );
    }
    let (bus, root) = bus("switch");
    let mut check = SpeakerEcho::from_switch(Some("on"), || Ok(bus.clone())).unwrap();
    speak(&mut check);
    ask(&bus, "on", "on_demand");
    check.answer_requests(monotonic_us());
    assert_eq!(answer(&bus).verdict, "pass");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_request_not_for_the_factory_or_on_demand_is_not_checked() {
    let (bus, root) = bus("occasion");
    let mut check = SpeakerEcho::on(&bus).unwrap();
    speak(&mut check);
    ask(&bus, "boot", "first_boot");
    check.answer_requests(monotonic_us());
    let refused = answer(&bus);
    assert_eq!(
        (refused.id.as_str(), refused.verdict.as_str()),
        ("boot", "not_checked")
    );
    assert_eq!(refused.cancelled_db, None);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_answer_that_cannot_be_written_switches_the_check_off_and_the_voice_goes_on() {
    let (bus, root) = bus("broken");
    let mut check = SpeakerEcho::on(&bus).unwrap();
    // The answer topic's kept value is a directory: every publication fails.
    let kept = root.join(SpeakerCheck::TOPIC).join("latest");
    let _ = std::fs::remove_file(&kept);
    std::fs::create_dir_all(kept.join("in-the-way")).unwrap();
    ask(&bus, "r1", "on_demand");
    check.answer_requests(monotonic_us());
    assert!(check.is_off());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn requests_that_cannot_be_read_switch_the_check_off() {
    let (bus, root) = bus("unreadable");
    let mut check = SpeakerEcho::on(&bus).unwrap();
    check.take(
        Err(io::Error::other("the socket went away")),
        monotonic_us(),
    );
    assert!(check.is_off());
    std::fs::remove_dir_all(root).unwrap();
}
