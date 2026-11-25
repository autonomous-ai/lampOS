use super::*;

#[test]
fn each_moment_is_a_spoke_did_with_fixed_words_and_nothing_said() {
    for (moment, who, why) in [
        (
            Moment::TaskNews,
            "lamp-voice:task-news",
            "a task it handed over has news to tell",
        ),
        (
            Moment::Onboarding(Some(3)),
            "lamp-voice:onboarding",
            "the first minutes with a new person: step 3",
        ),
        (
            Moment::Onboarding(None),
            "lamp-voice:onboarding",
            "the first minutes with a new person",
        ),
        (
            Moment::UpdateLine,
            "lamp-voice:update-line",
            "the lamp's software is being updated",
        ),
        (
            Moment::AgentSwitch,
            "lamp-voice:agent-switch",
            "the lamp's agent is being switched",
        ),
    ] {
        let told = did(moment, 7);
        assert_eq!(told.who, who);
        assert_eq!(told.why, why);
        assert_eq!(told.what, DidWhat::Spoke);
        assert_eq!(told.outcome, DidOutcome::Done);
        assert_eq!(told.at_us, 7);
        assert!(told.check().is_ok(), "{who}");
    }
}

#[test]
fn the_switch_is_one_name_and_off_tells_nothing() {
    assert_eq!(ENVIRONMENT, "LAMP_DID_VOICE");
    assert_eq!(SOURCE, "voice");
    if !on() {
        told(Moment::TaskNews);
        assert!(OUT.get().is_none());
    }
}

/// Read back whole from a real bus as the voice's; a second within [`EVERY`]
/// is let go.
#[test]
fn a_moment_handed_over_is_published_as_the_voice_and_a_burst_is_let_go() {
    let dir = std::env::temp_dir().join(format!("dv{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bus = Bus::new(&dir).unwrap();
    let mut heard = bus
        .subscribe_typed(LampDid::TOPIC, "test", LampDid::TYPE)
        .unwrap();
    let (to_thread, from_callers) = mpsc::sync_channel::<Moment>(8);
    let before = let_go();
    let thread = {
        let root = dir.clone();
        std::thread::spawn(move || {
            let bus = Bus::new(&root).unwrap();
            publish_on(&bus, from_callers);
        })
    };
    to_thread.send(Moment::Onboarding(Some(2))).unwrap();
    to_thread.send(Moment::TaskNews).unwrap();
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
            ..did(Moment::Onboarding(Some(2)), 0)
        }
    );
    assert!(heard.recv(Duration::from_millis(300)).unwrap().is_none());
    assert!(let_go() > before, "the second came within two seconds");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Rule 7: the edge costs the voice's loop one `try_send`; with the switch off
/// it reads one variable. 20,000 each, the mean and slowest printed, bounded.
#[test]
fn the_edge_costs_one_try_send_and_off_one_variable() {
    let (to_thread, _from_callers) = mpsc::sync_channel::<Moment>(8);
    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    for _ in 0..20_000 {
        let call = Instant::now();
        let _ = to_thread.try_send(Moment::TaskNews);
        slowest = slowest.max(call.elapsed());
    }
    let mean = started.elapsed() / 20_000;
    eprintln!("did-voice edge, 20,000 try_send: mean {mean:?} slowest {slowest:?}");
    assert!(mean < Duration::from_micros(20), "{mean:?}");
    assert!(slowest < Duration::from_millis(5), "{slowest:?}");

    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    for _ in 0..20_000 {
        let call = Instant::now();
        if !on() {
            told(Moment::TaskNews);
        }
        slowest = slowest.max(call.elapsed());
    }
    let off_mean = started.elapsed() / 20_000;
    eprintln!("did-voice told() switch off, 20,000: mean {off_mean:?} slowest {slowest:?}");
    assert!(off_mean < Duration::from_micros(50), "{off_mean:?}");
    assert!(slowest < Duration::from_millis(5), "{slowest:?}");
}
