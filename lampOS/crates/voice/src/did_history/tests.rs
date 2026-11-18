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
    assert_eq!(TELLER, "skills");
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
