//! A subscriber that drops is said on standard error at most once a minute,
//! by the publisher's own cadence clock, and its total since the publisher
//! began is kept in the publisher's entry for `Bus::drops` to read.
use super::*;
use crate::Kind;
use serde_json::json;
use std::sync::{Arc, Mutex};

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from(format!(
            "/tmp/bdr-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }

    fn bus(&self) -> Bus {
        Bus::new(self.0.join("b")).unwrap()
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
struct Clock(Arc<Mutex<Instant>>);

impl Clock {
    fn reader(&self) -> impl Fn() -> Instant + Send + Sync + 'static + use<> {
        let at = self.0.clone();
        move || *at.lock().unwrap()
    }

    fn set(&self, at: Instant) {
        *self.0.lock().unwrap() = at;
    }
}

/// The lines a publisher says about drops, kept instead of printed.
fn listen(publisher: &mut Publisher) -> Arc<Mutex<Vec<String>>> {
    let said = Arc::new(Mutex::new(Vec::new()));
    let kept = said.clone();
    publisher.say_drop = Box::new(move |line| kept.lock().unwrap().push(line.into()));
    said
}

const PRIVATE: &str = "words nobody else should read";

/// Publish until the subscriber that never reads has its first message
/// dropped.
fn fill(publisher: &mut Publisher) {
    for _ in 0..1_000_000 {
        let outcome = publisher
            .publish_json(1, &json!({ "text": PRIVATE }))
            .unwrap();
        if outcome.dropped == 1 {
            return;
        }
        assert_eq!(outcome.delivered, 1);
    }
    panic!("the queue never filled");
}

fn line(socket: &str, dropped: u64, total: u64) -> String {
    format!(
        "lamp-bus: drops topic=people publisher=eyes subscriber={socket} dropped={dropped} \
         total={total} (dropped since this subscriber's last line; at most one line a minute)"
    )
}

#[test]
fn a_full_queue_is_said_once_a_minute_with_its_count_and_never_the_payload() {
    let root = Root::new();
    let bus = root.bus();
    let slow = bus.subscribe("people", "slow").unwrap();
    let start = Instant::now();
    let clock = Clock(Arc::new(Mutex::new(start)));
    let mut publisher = bus
        .publisher_with_cadence_clock("people", Kind::Event, "eyes", clock.reader())
        .unwrap();
    let said = listen(&mut publisher);
    fill(&mut publisher);
    let socket = publisher.subscribers()[0].socket.clone();
    assert!(socket.starts_with("slow."), "{socket}");
    assert_eq!(*said.lock().unwrap(), [line(&socket, 1, 1)]);

    // Nine more within the minute are counted and not said.
    for step in 1..=9u64 {
        clock.set(start + Duration::from_millis(59_990 * step / 9));
        assert_eq!(publisher.publish_json(1, &PRIVATE).unwrap().dropped, 1);
    }
    assert_eq!(said.lock().unwrap().len(), 1);

    // The minute has passed: the next publish says the nine, whether or
    // not it drops itself, and then nothing more for a minute.
    clock.set(start + Duration::from_secs(60));
    assert_eq!(publisher.publish_json(1, &PRIVATE).unwrap().dropped, 1);
    assert_eq!(
        *said.lock().unwrap(),
        [line(&socket, 1, 1), line(&socket, 10, 11)]
    );
    clock.set(start + Duration::from_millis(119_999));
    assert_eq!(publisher.publish_json(1, &PRIVATE).unwrap().dropped, 1);
    assert_eq!(said.lock().unwrap().len(), 2);
    for line in said.lock().unwrap().iter() {
        assert!(!line.contains("words"), "{line}");
    }

    // Its total since the publisher began is in the publisher's entry, for
    // anyone to read, from its next refresh.
    clock.set(start + Duration::from_secs(121));
    publisher.alive().unwrap();
    let drops = bus.drops("people").unwrap();
    assert_eq!(drops.len(), 1, "{drops:?}");
    assert_eq!(drops[0].subscriber, socket);
    assert_eq!(drops[0].dropped, 12);
    assert_eq!(drops[0].publisher.name, "eyes");
    assert_eq!(drops[0].publisher.boot, publisher.boot());
    assert_eq!(publisher.totals().dropped, 12);
    // Written beside and put in place: nothing else is left in `pubs/`.
    assert_eq!(fs::read_dir(&publisher.pubs).unwrap().count(), 1);
    drop(slow);
}

#[test]
fn a_subscriber_that_keeps_up_changes_nothing_and_is_never_said() {
    let root = Root::new();
    let bus = root.bus();
    let mut fast = bus.subscribe("people", "fast").unwrap();
    let start = Instant::now();
    let clock = Clock(Arc::new(Mutex::new(start)));
    let mut publisher = bus
        .publisher_with_cadence_clock("people", Kind::Event, "eyes", clock.reader())
        .unwrap();
    let said = listen(&mut publisher);
    for n in 1..=50u64 {
        clock.set(start + Duration::from_millis(100 * n));
        assert_eq!(
            publisher.publish_json(1, &n).unwrap(),
            Published {
                delivered: 1,
                dropped: 0,
                removed: 0
            }
        );
        assert_eq!(fast.recv(Duration::ZERO).unwrap().unwrap().header.seq, n);
    }
    assert!(said.lock().unwrap().is_empty());
    // The entry is the empty file it always was.
    assert_eq!(fs::metadata(&publisher.entry).unwrap().len(), 0);
    assert!(bus.drops("people").unwrap().is_empty());
}

#[test]
fn guarded_drops_are_said_by_the_next_preparation_not_while_sending() {
    let root = Root::new();
    let bus = root.bus();
    let slow = bus.subscribe("mic1/audio", "slow").unwrap();
    let clock = Clock(Arc::new(Mutex::new(Instant::now())));
    let mut publisher = bus
        .publisher_with_cadence_clock("mic1/audio", Kind::Event, "eyes", clock.reader())
        .unwrap();
    let said = listen(&mut publisher);
    for _ in 0..1_000_000 {
        publisher.prepare_guarded_event().unwrap();
        let outcome = publisher
            .publish_bytes_guarded(1, "json+pcm", PRIVATE.as_bytes(), || true)
            .unwrap();
        if outcome.dropped == 1 {
            break;
        }
    }
    assert_eq!(publisher.totals().dropped, 1);
    assert!(said.lock().unwrap().is_empty());
    publisher.prepare_guarded_event().unwrap();
    let said = said.lock().unwrap();
    assert_eq!(said.len(), 1);
    assert!(
        said[0].starts_with("lamp-bus: drops topic=mic1/audio publisher=eyes subscriber=slow."),
        "{said:?}"
    );
    assert!(said[0].contains(" dropped=1 total=1 "), "{said:?}");
    drop(slow);
}
