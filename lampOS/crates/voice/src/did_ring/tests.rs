use super::*;

#[test]
fn each_face_is_a_lit_did_with_its_fixed_words_and_no_heard_text() {
    for (face, who) in [
        (Face::Asleep, "lamp-ring:asleep"),
        (Face::NightSleep, "lamp-ring:night-sleep"),
        (Face::Unplug, "lamp-ring:unplug"),
        (Face::PrivacyMuted, "lamp-ring:privacy-muted"),
    ] {
        let told = did(face, 7);
        assert_eq!(told.who, who);
        assert_eq!(told.what, DidWhat::Lit);
        assert_eq!(told.outcome, DidOutcome::Done);
        assert_eq!(told.at_us, 7);
        assert!(told.check().is_ok(), "{who}");
        assert!(told.why.chars().count() <= LampDid::MAX_WHY);
    }
    // The privacy one says only THAT the lamp was private.
    assert_eq!(
        did(Face::PrivacyMuted, 1).why,
        "the microphone is off at the switch"
    );
    assert_eq!(
        did(Face::Asleep, 1).why,
        "the lamp cannot hear: it is muted or the privacy switch is closed"
    );
}

#[test]
fn a_face_drawn_again_is_one_showing_and_after_a_gap_a_new_one() {
    let mut episode = Episode::default();
    assert!(episode.seen(1_000_000));
    // Drawn every frame: the same showing.
    for step in 1..=200 {
        assert!(!episode.seen(1_000_000 + step * 25_000), "{step}");
    }
    // Gone for more than the gap, then drawn: a new one.
    let later = 1_000_000 + 200 * 25_000 + GAP_US + 1;
    assert!(episode.seen(later));
    assert!(!episode.seen(later + 25_000));
}

#[test]
fn the_switch_is_one_name_and_off_tells_nothing() {
    assert_eq!(ENVIRONMENT, "LAMP_DID_RING");
    assert_eq!(SOURCE, "ring");
    // Off in a test process: told() does nothing and starts no thread.
    if !on() {
        told(Face::Asleep);
        assert!(OUT.get().is_none());
    }
}

/// The thread makes a did for the face handed over and publishes it as the
/// ring's: read back whole from a real bus; a second within [`EVERY`] is let go.
#[test]
fn a_face_handed_over_is_published_as_the_ring_and_a_burst_is_let_go() {
    let dir = std::env::temp_dir().join(format!("dr{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bus = Bus::new(&dir).unwrap();
    let mut heard = bus
        .subscribe_typed(LampDid::TOPIC, "test", LampDid::TYPE)
        .unwrap();
    let (to_thread, from_callers) = mpsc::sync_channel::<Face>(8);
    let before = let_go();
    // publish() opens the bus from the environment: point it here.
    let thread = {
        let root = dir.clone();
        std::thread::spawn(move || {
            let bus = Bus::new(&root).unwrap();
            publish_on(&bus, from_callers);
        })
    };
    to_thread.send(Face::NightSleep).unwrap();
    to_thread.send(Face::Unplug).unwrap();
    drop(to_thread);
    thread.join().unwrap();
    let first = heard
        .recv(Duration::from_secs(2))
        .unwrap()
        .expect("a did is told");
    assert_eq!(first.header.source, SOURCE);
    let told = first.parse::<LampDid>().unwrap();
    assert_eq!(
        told,
        LampDid {
            at_us: told.at_us,
            ..did(Face::NightSleep, 0)
        }
    );
    assert!(heard.recv(Duration::from_millis(300)).unwrap().is_none());
    assert_eq!(let_go(), before + 1, "the second came within two seconds");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Rule 7: what a face's edge costs the ring's loop is one `try_send`:
/// 20,000 of them, the mean and the slowest, with the switch on and nothing
/// reading; and the same with it off. Printed, and bounded.
#[test]
fn the_edge_costs_the_loop_one_try_send() {
    let (to_thread, _from_callers) = mpsc::sync_channel::<Face>(8);
    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    for _ in 0..20_000 {
        let call = Instant::now();
        let _ = to_thread.try_send(Face::Asleep);
        slowest = slowest.max(call.elapsed());
    }
    let mean = started.elapsed() / 20_000;
    eprintln!("did-ring edge, 20,000 try_send: mean {mean:?} slowest {slowest:?}");
    assert!(mean < Duration::from_micros(20), "{mean:?}");
    assert!(slowest < Duration::from_millis(5), "{slowest:?}");
}

/// Rule 7, the rest of what the ring's loop does for this: per frame, the
/// unplug and the privacy ring ask [`Episode::seen`] (integer arithmetic);
/// at an edge with the switch off, [`told`] reads one variable. 20,000 each,
/// the mean and the slowest printed and bounded.
#[test]
fn per_frame_and_with_the_switch_off_the_cost_is_arithmetic_and_one_variable() {
    let mut episode = Episode::default();
    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    for step in 0..20_000u64 {
        let call = Instant::now();
        let _ = episode.seen(1_000_000 + step * 25_000);
        slowest = slowest.max(call.elapsed());
    }
    let seen_mean = started.elapsed() / 20_000;
    let seen_slowest = slowest;
    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    for _ in 0..20_000 {
        let call = Instant::now();
        if !on() {
            told(Face::Asleep);
        }
        slowest = slowest.max(call.elapsed());
    }
    let off_mean = started.elapsed() / 20_000;
    eprintln!(
        "did-ring per frame, 20,000: Episode::seen mean {seen_mean:?} slowest {seen_slowest:?}; told() with the switch off mean {off_mean:?} slowest {slowest:?}"
    );
    assert!(seen_mean < Duration::from_micros(5), "{seen_mean:?}");
    assert!(off_mean < Duration::from_micros(50), "{off_mean:?}");
    assert!(slowest < Duration::from_millis(5), "{slowest:?}");
}
