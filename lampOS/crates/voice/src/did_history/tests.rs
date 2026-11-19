use super::*;
use lamp_msgs::{DidOutcome, DidWhat};

const MINUTE: u64 = 60_000_000;

fn did(at_us: u64, why: &str) -> LampDid {
    LampDid {
        who: "lamp-skills:sip-of-water".into(),
        what: DidWhat::Spoke,
        why: why.into(),
        at_us,
        outcome: DidOutcome::Done,
    }
}

#[test]
fn the_201st_action_drops_the_first() {
    let mut history = DidHistory::default();
    for n in 1..=201u64 {
        history.keep(did(n * 1_000, &format!("action {n}")), n * 1_000);
    }
    assert_eq!(history.len(), MOST);
    let newest = history.newest(MOST, 201_000);
    assert_eq!(newest.first().unwrap().why, "action 201");
    assert_eq!(newest.last().unwrap().why, "action 2", "the first is gone");
}

#[test]
fn an_action_older_than_an_hour_is_gone() {
    let mut history = DidHistory::default();
    history.keep(did(MINUTE, "early"), MINUTE);
    history.keep(did(30 * MINUTE, "middle"), 30 * MINUTE);
    // 61 minutes after the first: only the first has aged out.
    let now = 62 * MINUTE;
    let kept: Vec<_> = history.newest(10, now).into_iter().map(|d| d.why).collect();
    assert_eq!(kept, ["middle"]);
    // And all of them an hour and a half on.
    assert!(history.newest(10, 200 * MINUTE).is_empty());
    assert!(history.is_empty());
}

#[test]
fn newest_first_and_at_most_n() {
    let mut history = DidHistory::default();
    for n in 1..=5u64 {
        history.keep(did(n * MINUTE, &format!("a{n}")), n * MINUTE);
    }
    let two: Vec<_> = history
        .newest(2, 6 * MINUTE)
        .into_iter()
        .map(|d| d.why)
        .collect();
    assert_eq!(two, ["a5", "a4"]);
}

#[test]
fn a_did_that_fails_its_check_is_never_kept() {
    let mut history = DidHistory::default();
    history.keep(did(MINUTE, ""), MINUTE);
    history.keep(did(MINUTE, &"x".repeat(121)), MINUTE);
    history.keep(did(0, "no time"), MINUTE);
    assert!(history.is_empty());
}

#[test]
fn the_switch_and_the_one_teller_are_named() {
    assert_eq!(ENVIRONMENT, "LAMP_DID_KEEP");
    assert_eq!(TELLERS, ["skills", "body", "ring", "voice"]);
}

/// The keeper reads the bus on a thread of its own and a turn only the
/// memory: a did published on `lamp/did` is kept, with nothing written.
#[test]
fn a_did_published_on_the_bus_is_kept() {
    use lamp_bus::Bus;
    let dir = std::env::temp_dir().join(format!("dh{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bus = Bus::new(&dir).unwrap();
    let mut out = bus
        .publisher_declared(LampDid::TOPIC, &LampDid::declaration(), "skills")
        .unwrap();
    let history = watch(&bus).expect("a keeper on a real bus");
    std::thread::sleep(Duration::from_millis(200));
    let now = lamp_ipc::monotonic_us();
    out.publish_json(now, &did(now, "sat 47 min, no drink seen"))
        .unwrap();
    let end = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let found = history
            .lock()
            .unwrap()
            .newest(5, lamp_ipc::monotonic_us())
            .iter()
            .any(|d| d.why == "sat 47 min, no drink seen");
        if found {
            break;
        }
        assert!(std::time::Instant::now() < end, "not kept within 5 s");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(kept().is_some());
    // A did from any other publisher is not kept; the next from lamp-skills is.
    let mut other = bus
        .publisher_declared("lamp/did", &LampDid::declaration(), "voice_part")
        .map(Some)
        .unwrap_or(None);
    if let Some(other) = other.as_mut() {
        let now = lamp_ipc::monotonic_us();
        other
            .publish_json(now, &did(now, "words from elsewhere"))
            .unwrap();
    }
    out.publish_json(now, &did(now, "the one that follows"))
        .unwrap();
    let end = std::time::Instant::now() + Duration::from_secs(5);
    while !history
        .lock()
        .unwrap()
        .newest(5, lamp_ipc::monotonic_us())
        .iter()
        .any(|d| d.why == "the one that follows")
    {
        assert!(std::time::Instant::now() < end, "not kept within 5 s");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !history
            .lock()
            .unwrap()
            .newest(5, lamp_ipc::monotonic_us())
            .iter()
            .any(|d| d.why == "words from elsewhere")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Heron on #7812: with the presence on the body makes about 90 shifts an
/// hour and a turn to a call every 10 s; its dids must not push the skills'
/// out of the shared 200.
#[test]
fn a_body_flood_never_pushes_the_skills_dids_out() {
    let mut history = DidHistory::default();
    for at in 0..120 {
        history.keep(did(MINUTE + at, "a skill's"), MINUTE + at);
    }
    for at in 0..400 {
        history.keep_part(
            did(MINUTE + 200 + at, "the body's"),
            Part::Body,
            MINUTE + 200 + at,
        );
    }
    let all = history.newest(MOST * 2, MINUTE + 700);
    let body = all.iter().filter(|d| d.why == "the body's").count();
    let skills = all.len() - body;
    assert_eq!(body, BODY_MOST, "the body keeps at most its share");
    assert_eq!(skills, MOST - BODY_MOST, "the skills keep at least theirs");
    assert_eq!(all.len(), MOST);
    // The body's own oldest went first: its newest are all there.
    assert!(all.iter().take(BODY_MOST).all(|d| d.why == "the body's"));
}

#[test]
fn skills_alone_still_fill_all_two_hundred() {
    let mut history = DidHistory::default();
    for at in 0..300 {
        history.keep(did(MINUTE + at, "a skill's"), MINUTE + at);
    }
    assert_eq!(history.len(), MOST);
    // And a body that tells a little leaves the rest to the skills.
    for at in 0..10 {
        history.keep_part(
            did(MINUTE + 400 + at, "the body's"),
            Part::Body,
            MINUTE + 400 + at,
        );
    }
    let all = history.newest(MOST, MINUTE + 500);
    assert_eq!(all.iter().filter(|d| d.why == "the body's").count(), 10);
    assert_eq!(all.len(), MOST);
}

/// Told by the body on the bus, a did takes the body's share: 150 of them keep
/// 100, and the skills' did before them stays.
#[test]
fn dids_told_by_the_body_on_the_bus_take_the_bodys_share() {
    use lamp_bus::Bus;
    let dir = std::env::temp_dir().join(format!("dhb{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bus = Bus::new(&dir).unwrap();
    let mut skills = bus
        .publisher_declared(LampDid::TOPIC, &LampDid::declaration(), "skills")
        .unwrap();
    let mut body = bus
        .publisher_declared(LampDid::TOPIC, &LampDid::declaration(), "body")
        .unwrap();
    let history = watch_into(&bus, Arc::default()).expect("a keeper on a real bus");
    std::thread::sleep(Duration::from_millis(200));
    let now = lamp_ipc::monotonic_us();
    skills.publish_json(now, &did(now, "the skill's")).unwrap();
    for _ in 0..150 {
        let at = lamp_ipc::monotonic_us();
        body.publish_json(at, &did(at, "the body's")).unwrap();
    }
    let end = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let all = history
            .lock()
            .unwrap()
            .newest(MOST, lamp_ipc::monotonic_us());
        let body_kept = all.iter().filter(|d| d.why == "the body's").count();
        if body_kept == BODY_MOST && all.iter().any(|d| d.why == "the skill's") {
            break;
        }
        assert!(
            std::time::Instant::now() < end,
            "kept {body_kept} of the body's"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The ring tells a few faces a day: its share is at most RING_MOST, and
/// neither a body flood nor a skills flood pushes its dids out.
#[test]
fn the_rings_share_is_its_own_and_a_body_flood_does_not_push_it_out() {
    let mut history = DidHistory::default();
    for at in 0..5 {
        history.keep_part(did(MINUTE + at, "the ring's"), Part::Ring, MINUTE + at);
    }
    for at in 0..120 {
        history.keep(did(MINUTE + 10 + at, "a skill's"), MINUTE + 10 + at);
    }
    for at in 0..400 {
        history.keep_part(
            did(MINUTE + 200 + at, "the body's"),
            Part::Body,
            MINUTE + 200 + at,
        );
    }
    let all = history.newest(MOST * 2, MINUTE + 700);
    let count = |why: &str| all.iter().filter(|d| d.why == why).count();
    assert_eq!(
        count("the ring's"),
        5,
        "the ring's five are all still there"
    );
    assert!(
        count("a skill's") >= MOST - BODY_MOST,
        "{}",
        count("a skill's")
    );
    assert!(count("the body's") <= BODY_MOST - 5);
    assert_eq!(all.len(), MOST);
    // The ring's own oldest goes first past its share.
    for at in 0..40 {
        history.keep_part(
            did(MINUTE + 700 + at, "the ring's"),
            Part::Ring,
            MINUTE + 700 + at,
        );
    }
    let all = history.newest(MOST * 2, MINUTE + 800);
    assert_eq!(
        all.iter().filter(|d| d.why == "the ring's").count(),
        RING_MOST
    );
}

/// The voice tells the lines it begins itself: its share is at most
/// VOICE_MOST, and a body flood or a skills flood does not push it out.
#[test]
fn the_voices_share_is_its_own_and_a_flood_does_not_push_it_out() {
    let mut history = DidHistory::default();
    for at in 0..5 {
        history.keep_part(did(MINUTE + at, "the voice's"), Part::Voice, MINUTE + at);
    }
    for at in 0..120 {
        history.keep(did(MINUTE + 10 + at, "a skill's"), MINUTE + 10 + at);
    }
    for at in 0..400 {
        history.keep_part(
            did(MINUTE + 200 + at, "the body's"),
            Part::Body,
            MINUTE + 200 + at,
        );
    }
    let all = history.newest(MOST * 2, MINUTE + 700);
    let count = |why: &str| all.iter().filter(|d| d.why == why).count();
    assert_eq!(count("the voice's"), 5);
    assert!(count("a skill's") >= MOST - BODY_MOST);
    assert_eq!(all.len(), MOST);
    for at in 0..40 {
        history.keep_part(
            did(MINUTE + 700 + at, "the voice's"),
            Part::Voice,
            MINUTE + 700 + at,
        );
    }
    let all = history.newest(MOST * 2, MINUTE + 800);
    assert_eq!(
        all.iter().filter(|d| d.why == "the voice's").count(),
        VOICE_MOST
    );
}

#[test]
fn only_the_named_parts_have_a_share() {
    assert_eq!(Part::of("skills"), Some(Part::Skills));
    assert_eq!(Part::of("body"), Some(Part::Body));
    assert_eq!(Part::of("ring"), Some(Part::Ring));
    assert_eq!(Part::of("voice"), Some(Part::Voice));
    assert_eq!(Part::of("lamp-voice"), None);
    for source in TELLERS {
        assert!(Part::of(source).is_some(), "{source}");
    }
}
