//! The bus between a publisher and its subscribers, through real sockets in
//! a directory of the test's own.
//!
//! How much a queue holds is the operating system's business and differs
//! between the lamp and the machine these run on, so nothing here assumes a
//! number: a test that needs a full queue fills one until a send is dropped.
use lamp_bus::{Bus, Declaration, Kind, MOST_BYTES, Notice, Published, Statistics, Topic, Totals};
use serde_json::json;
use std::{
    fs, io,
    os::unix::{fs::PermissionsExt, net::UnixDatagram},
    path::{Path, PathBuf},
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Long enough for a message that has been sent to be there.
const WAIT: Duration = Duration::from_secs(5);
/// Long enough to say that nothing is coming.
const BRIEF: Duration = Duration::from_millis(30);
/// A publisher that looks at its subscribers once and never again, so that
/// a test decides what it knows.
const NEVER: Duration = Duration::from_secs(3_600);

/// A root of one test's own, removed when the test ends however it ends.
struct Root(PathBuf);

/// A number in base 36, the shortest a file name can carry it.
fn short(mut number: u32) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit(number % 36, 36).unwrap());
        number /= 36;
        if number == 0 {
            break digits.iter().rev().collect();
        }
    }
}

impl Root {
    fn new() -> Self {
        static MADE: AtomicU32 = AtomicU32::new(0);
        // Short: a socket's whole path has to fit in 103 bytes, and on
        // macOS the temporary directory alone is 49. With the process and
        // the count in decimal the name came to ten bytes, and a subscriber
        // called `follow` on `gemini/transcript` to 104 whenever the
        // process's number had five digits: eight at most this way.
        let path = std::env::temp_dir().join(format!(
            "b{}_{}",
            short(std::process::id()),
            short(MADE.fetch_add(1, Ordering::Relaxed))
        ));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn bus(&self) -> Bus {
        Bus::new(&self.0).unwrap()
    }

    /// Everything under the root, directories with a `/` after them.
    fn contents(&self) -> Vec<String> {
        fn walk(directory: &Path, prefix: &str, found: &mut Vec<String>) {
            let Ok(entries) = fs::read_dir(directory) else {
                return;
            };
            for entry in entries.flatten() {
                let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
                if entry.file_type().unwrap().is_dir() {
                    found.push(format!("{name}/"));
                    walk(&entry.path(), &format!("{name}/"), found);
                } else {
                    found.push(name);
                }
            }
        }
        let mut found = Vec::new();
        walk(&self.0, "", &mut found);
        found.sort();
        found
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sockets(subs: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(subs)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    found.sort();
    found
}

/// The first message waiting, which has to be the publisher's first, and
/// how many bytes it was on the wire.
fn subscriber_bytes(subscriber: &mut lamp_bus::Subscriber) -> usize {
    let message = subscriber.recv(WAIT).unwrap().expect("a held message");
    assert_eq!((message.header.seq, message.gap), (1, 0));
    serde_json::to_vec(&message.header).unwrap().len() + 1 + message.payload.len()
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

/// Wait for something another moment brings: a lease running out, a period
/// passing. What is asserted of time is then a least time, never a most.
fn until(what: &str, mut there: impl FnMut() -> bool) {
    let started = Instant::now();
    while !there() {
        assert!(started.elapsed() < WAIT, "{what} did not come");
        thread::sleep(Duration::from_millis(2));
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// A message made by hand, as a publisher in another language would make
/// it, so that a test says exactly what its header carries: `extra` is
/// more of the header's JSON, each field with a comma before it.
fn by_hand(topic: &str, seq: u64, at_unix_ms: u64, extra: &str) -> Vec<u8> {
    format!(
        "{{\"topic\":\"{topic}\",\"kind\":\"event\",\"enc\":\"json\",\"source\":\"hand\",\
         \"boot\":\"0123456789abcdef\",\"seq\":{seq},\"at_us\":{seq},\
         \"at_unix_ms\":{at_unix_ms}{extra}}}\n{seq}"
    )
    .into_bytes()
}

/// Twenty milliseconds of sound as the microphones give it, every byte
/// value in it, line ends and zeros too.
fn sound(n: u64) -> Vec<u8> {
    (0..640u64).map(|index| (index * (n + 1)) as u8).collect()
}

const ONE: Published = Published {
    delivered: 1,
    dropped: 0,
    removed: 0,
};

#[test]
fn one_publisher_reaches_two_subscribers_with_every_message_in_order_json_and_bytes() {
    let root = Root::new();
    let bus = root.bus();
    let mut follow = bus.subscribe("servos", "follow").unwrap();
    let mut eyes = bus.subscribe("servos", "eyes").unwrap();
    let mut publisher = bus.publisher("servos", Kind::Event, "body").unwrap();
    publisher.expires_after(Some(Duration::from_millis(500)));
    let both = Published {
        delivered: 2,
        dropped: 0,
        removed: 0,
    };
    // By turns, so that nothing depends on how much a queue holds.
    for n in 1..=60u64 {
        let outcome = if n % 2 == 1 {
            publisher.publish_json(n * 1_000, &json!({"yaw": n, "note": "a\nb"}))
        } else {
            publisher.publish_bytes(n * 1_000, "pcm_s16le_16000_mono", &sound(n))
        };
        assert_eq!(outcome.unwrap(), both);
        for subscriber in [&mut follow, &mut eyes] {
            let message = subscriber.recv(WAIT).unwrap().expect("a message");
            assert_eq!(message.header.topic, "servos");
            assert_eq!(message.header.kind, Kind::Event);
            assert_eq!(message.header.source, "body");
            assert_eq!(message.header.boot, publisher.boot());
            assert_eq!(message.header.seq, n);
            assert_eq!(message.header.at_us, n * 1_000);
            assert_eq!(message.header.expires_ms, Some(500));
            assert!(message.header.at_unix_ms > 0);
            assert_eq!((message.gap, message.publisher_restarted), (0, false));
            if n % 2 == 1 {
                assert_eq!(message.header.enc, "json");
                assert_eq!(message.json().unwrap(), json!({"yaw": n, "note": "a\nb"}));
                assert_eq!(message.parse::<serde_json::Value>().unwrap()["yaw"], n);
            } else {
                assert_eq!(message.header.enc, "pcm_s16le_16000_mono");
                assert_eq!(message.payload, sound(n));
                assert!(message.json().is_err());
            }
        }
    }
    // Several waiting at once come out in the order they went in.
    for n in 61..=68u64 {
        assert_eq!(publisher.publish_json(n, &n).unwrap(), both);
    }
    for subscriber in [&mut follow, &mut eyes] {
        for n in 61..=68u64 {
            let message = subscriber.recv(WAIT).unwrap().expect("a message");
            assert_eq!((message.header.seq, message.gap), (n, 0));
            assert_eq!(message.parse::<u64>().unwrap(), n);
        }
        assert!(subscriber.recv(BRIEF).unwrap().is_none());
        assert!(subscriber.recv(Duration::ZERO).unwrap().is_none());
        assert_eq!((subscriber.malformed(), subscriber.repeated()), (0, 0));
    }
    assert_eq!(
        publisher.totals(),
        Totals {
            published: 68,
            delivered: 136,
            dropped: 0
        }
    );
    // A message with nothing to carry, and labels that are not labels.
    publisher.expires_after(None);
    assert_eq!(publisher.publish_bytes(7, "none", b"").unwrap(), both);
    let bare = follow.recv(WAIT).unwrap().unwrap();
    assert_eq!((bare.header.enc.as_str(), bare.payload.len()), ("none", 0));
    assert_eq!(bare.header.expires_ms, None);
    for enc in ["none", "", "PCM", "a b"] {
        let error = publisher.publish_bytes(8, enc, b"x").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{enc:?}");
    }
    assert_eq!(publisher.totals().published, 69);
}

#[test]
fn a_late_subscriber_reads_the_current_value_of_a_latest_topic_and_nothing_of_an_event_topic() {
    let root = Root::new();
    let bus = root.bus().rescan_every(Duration::ZERO);
    let mut pose = bus.publisher("servos", Kind::Latest, "body").unwrap();
    let mut words = bus
        .publisher("gemini/transcript", Kind::Event, "voice")
        .unwrap();
    for n in 1..=3u64 {
        pose.publish_json(n, &json!({"yaw": 1_990 + n})).unwrap();
        words.publish_json(n, &json!({"text": "hello"})).unwrap();
    }
    let current = bus.latest("servos").unwrap().expect("the current value");
    assert_eq!(current.header.seq, 3);
    assert_eq!(current.header.kind, Kind::Latest);
    assert_eq!(current.json().unwrap(), json!({"yaw": 1_993}));
    assert_eq!((current.gap, current.publisher_restarted), (0, false));
    assert!(bus.latest("gemini/transcript").unwrap().is_none());
    assert!(bus.latest("mic2").unwrap().is_none());

    let mut late = bus.subscribe("servos", "late").unwrap();
    assert_eq!(late.latest().unwrap().expect("the value").header.seq, 3);
    // Asked for again it is still the value, and it is not handed over by
    // `recv` as if it had just arrived.
    assert_eq!(late.latest().unwrap().expect("the value").header.seq, 3);
    assert!(late.recv(BRIEF).unwrap().is_none());
    let mut late_words = bus.subscribe("gemini/transcript", "late").unwrap();
    assert!(late_words.latest().unwrap().is_none());
    assert!(late_words.recv(BRIEF).unwrap().is_none());

    // From here on both are subscribers like any other.
    assert_eq!(pose.publish_json(4, &json!({"yaw": 2_000})).unwrap(), ONE);
    assert_eq!(
        words.publish_json(4, &json!({"text": "lamp"})).unwrap(),
        ONE
    );
    let next = late.recv(WAIT).unwrap().expect("the next value");
    assert_eq!((next.header.seq, next.gap), (4, 0));
    let word = late_words.recv(WAIT).unwrap().expect("the next word");
    // The first three words were said before it listened: that is not a gap.
    assert_eq!((word.header.seq, word.gap), (4, 0));
    assert_eq!(bus.latest("servos").unwrap().unwrap().header.seq, 4);
}

#[test]
fn a_subscriber_that_never_reads_does_not_slow_the_publisher_or_break_the_one_that_reads() {
    const MESSAGES: u64 = 20_000;
    let root = Root::new();
    let bus = root.bus();
    let slow = bus.subscribe("mic2", "slow").unwrap();
    let mut fast = bus.subscribe("mic2", "fast").unwrap();
    let mut publisher = bus.publisher("mic2", Kind::Event, "voice").unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let reader = thread::spawn({
        let done = Arc::clone(&done);
        move || {
            let mut got = Vec::new();
            loop {
                match fast.recv(BRIEF).unwrap() {
                    Some(message) => {
                        assert_eq!(message.parse::<u64>().unwrap(), message.header.seq);
                        got.push((message.header.seq, message.gap));
                    }
                    // Nothing waiting and nothing more to come.
                    None if done.load(Ordering::SeqCst) => break got,
                    None => {}
                }
            }
        }
    });
    let started = Instant::now();
    for n in 1..=MESSAGES {
        let outcome = publisher.publish_json(n, &n).unwrap();
        assert_eq!(outcome.delivered + outcome.dropped, 2, "message {n}");
        assert_eq!(outcome.removed, 0);
    }
    let took = started.elapsed();
    done.store(true, Ordering::SeqCst);
    let got = reader.join().unwrap();
    assert!(took < Duration::from_secs(5), "publishing took {took:?}");

    // The one that read: in order, and every hole in it announced.
    assert_eq!(got[0], (1, 0));
    let mut last = 0;
    for &(seq, gap) in &got {
        assert!(seq > last, "{seq} after {last}");
        assert_eq!(gap, seq - last - 1, "at {seq}");
        last = seq;
    }
    let missed: u64 = got.iter().map(|&(_, gap)| gap).sum();
    assert_eq!(got.len() as u64 + missed, last);
    assert!(last <= MESSAGES);

    let totals = publisher.totals();
    assert_eq!(totals.published, MESSAGES);
    assert_eq!(totals.delivered + totals.dropped, 2 * MESSAGES);
    let subscribers = publisher.subscribers();
    let of = |name: &str| {
        subscribers
            .iter()
            .find(|subscriber| subscriber.socket.starts_with(name))
            .unwrap()
    };
    // Everything the reader's queue took, the reader got.
    assert_eq!(of("fast.").delivered, got.len() as u64);
    assert_eq!(of("fast.").delivered + of("fast.").dropped, MESSAGES);
    assert!(of("slow.").dropped > 0);
    assert!(of("slow.").delivered > 0);
    assert_eq!(of("slow.").delivered + of("slow.").dropped, MESSAGES);
    println!(
        "{MESSAGES} messages in {took:?}: the reader got {} and was told of {missed} missed; \
         the one that never read was sent {} and dropped {}",
        got.len(),
        of("slow.").delivered,
        of("slow.").dropped
    );
    drop(slow);
}

#[test]
fn a_full_queue_at_one_subscriber_takes_nothing_from_another() {
    let root = Root::new();
    let bus = root.bus();
    let slow = bus.subscribe("mic2", "slow").unwrap();
    let mut fast = bus.subscribe("mic2", "fast").unwrap();
    let mut publisher = bus.publisher("mic2", Kind::Event, "voice").unwrap();
    // One at a time, each read before the next is sent: the reader's queue
    // never holds more than one, and whatever it misses is the other's doing.
    for n in 1..=3_000u64 {
        let outcome = publisher
            .publish_bytes(n, "pcm_s16le_16000_mono", &sound(n))
            .unwrap();
        assert_eq!(outcome.delivered + outcome.dropped, 2);
        let message = fast.recv(WAIT).unwrap().expect("a message");
        assert_eq!((message.header.seq, message.gap), (n, 0));
        assert_eq!(message.payload, sound(n));
    }
    let subscribers = publisher.subscribers();
    for subscriber in &subscribers {
        if subscriber.socket.starts_with("fast.") {
            assert_eq!((subscriber.delivered, subscriber.dropped), (3_000, 0));
        } else {
            assert!(subscriber.dropped > 0, "the queue never filled");
            assert_eq!(subscriber.delivered + subscriber.dropped, 3_000);
            println!(
                "a subscriber that does not read held {} pieces of sound of 640 bytes",
                subscriber.delivered
            );
        }
    }
    assert_eq!(subscribers.len(), 2);
    drop(slow);
}

#[test]
fn messages_lost_to_a_full_queue_show_as_a_gap_on_the_next_one_that_arrives() {
    let root = Root::new();
    let bus = root.bus();
    let mut subscriber = bus.subscribe("people", "follow").unwrap();
    let mut publisher = bus.publisher("people", Kind::Event, "eyes").unwrap();
    // Nobody reads: at some point the queue takes no more.
    let mut sent = 0u64;
    loop {
        let outcome = publisher.publish_json(7, &json!({"n": 1})).unwrap();
        sent += 1;
        if outcome.dropped == 1 {
            break;
        }
        assert_eq!(outcome, ONE);
        assert!(sent < 1_000_000, "the queue never filled");
    }
    let held = sent - 1;
    assert!(held >= 1);
    for _ in 0..7 {
        let outcome = publisher.publish_json(7, &json!({"n": 1})).unwrap();
        assert_eq!((outcome.delivered, outcome.dropped), (0, 1));
    }
    // What was taken comes out whole and in order, with nothing missing.
    let size = subscriber_bytes(&mut subscriber);
    println!(
        "a subscriber that does not read held {held} messages of {size} bytes before one was dropped"
    );
    for seq in 2..=held {
        let message = subscriber.recv(WAIT).unwrap().expect("a held message");
        assert_eq!((message.header.seq, message.gap), (seq, 0));
    }
    assert!(subscriber.recv(BRIEF).unwrap().is_none());
    // The eight that were dropped are said with the next one, once.
    assert_eq!(publisher.publish_json(7, &json!({"n": 1})).unwrap(), ONE);
    let message = subscriber.recv(WAIT).unwrap().expect("the next message");
    assert_eq!((message.header.seq, message.gap), (held + 9, 8));
    assert!(!message.publisher_restarted);
    assert_eq!(publisher.publish_json(7, &json!({"n": 1})).unwrap(), ONE);
    let message = subscriber.recv(WAIT).unwrap().expect("the one after");
    assert_eq!((message.header.seq, message.gap), (held + 10, 0));
    assert_eq!(
        publisher.totals(),
        Totals {
            published: held + 10,
            delivered: held + 2,
            dropped: 8
        }
    );
    // An event topic keeps no value. There is the topic, its subscriber's
    // socket and its publisher's entry, each in its directory.
    assert_eq!(root.contents().len(), 5, "{:?}", root.contents());
    assert!(!root.0.join("people/latest").exists());
}

#[test]
fn a_second_publisher_instance_under_the_same_name_shows_as_a_restart_and_not_as_a_gap() {
    let root = Root::new();
    let bus = root.bus();
    let mut subscriber = bus.subscribe("servos", "follow").unwrap();
    let mut first = bus.publisher("servos", Kind::Event, "body").unwrap();
    for seq in 1..=5u64 {
        assert_eq!(first.publish_json(seq, &seq).unwrap(), ONE);
        let message = subscriber.recv(WAIT).unwrap().unwrap();
        assert_eq!((message.header.seq, message.gap), (seq, 0));
        assert!(!message.publisher_restarted);
    }
    let was = first.boot().to_string();
    drop(first);
    // The body's process ended and was started again.
    let mut second = bus.publisher("servos", Kind::Event, "body").unwrap();
    assert_ne!(second.boot(), was);
    assert_eq!(second.publish_json(1, &1).unwrap(), ONE);
    let message = subscriber.recv(WAIT).unwrap().unwrap();
    assert_eq!(message.header.boot, second.boot());
    assert_eq!((message.header.seq, message.gap), (1, 0));
    assert!(message.publisher_restarted);
    // Said once: the next is an ordinary message of the new instance.
    assert_eq!(second.publish_json(2, &2).unwrap(), ONE);
    let message = subscriber.recv(WAIT).unwrap().unwrap();
    assert_eq!((message.header.seq, message.gap), (2, 0));
    assert!(!message.publisher_restarted);
}

#[test]
fn a_dropped_subscriber_removes_its_socket_and_a_dead_ones_is_removed_by_the_next_publish() {
    let root = Root::new();
    let bus = root.bus().rescan_every(NEVER);
    let subs = root.0.join("servos/subs");
    let subscriber = bus.subscribe("servos", "follow").unwrap();
    let bound = sockets(&subs);
    assert_eq!(bound.len(), 1);
    let name = bound[0].file_name().unwrap().to_str().unwrap();
    assert!(
        name.starts_with(&format!("follow.{}.", std::process::id())) && name.ends_with(".sock"),
        "{name}"
    );
    drop(subscriber);
    assert!(sockets(&subs).is_empty());

    // A subscriber whose process died: the kernel closed its socket and
    // nothing removed the file.
    let ghost = subs.join("ghost.1.0.sock");
    drop(UnixDatagram::bind(&ghost).unwrap());
    assert!(ghost.exists());
    let mut alive = bus.subscribe("servos", "alive").unwrap();
    let mut publisher = bus.publisher("servos", Kind::Event, "body").unwrap();
    let outcome = publisher.publish_json(1, &1).unwrap();
    assert_eq!(
        outcome,
        Published {
            delivered: 1,
            dropped: 0,
            removed: 1
        }
    );
    assert!(!ghost.exists());
    assert_eq!(alive.recv(WAIT).unwrap().unwrap().header.seq, 1);
    // Forgotten: it is not found dead a second time.
    assert_eq!(publisher.publish_json(2, &2).unwrap(), ONE);
    assert_eq!(publisher.subscribers().len(), 1);

    // One that dies while the publisher knows it, with another already in
    // its place: the publisher has not looked since its first message, and
    // finding the dead one makes it look, in time for this same message.
    let dying = subs.join("dying.1.0.sock");
    let held = UnixDatagram::bind(&dying).unwrap();
    drop(alive);
    let outcome = publisher.publish_json(3, &3).unwrap();
    assert_eq!(
        (outcome.delivered, outcome.removed),
        (1, 1),
        "alive went, dying came"
    );
    drop(held);
    let mut successor = bus.subscribe("servos", "next").unwrap();
    let outcome = publisher.publish_json(4, &4).unwrap();
    assert_eq!(
        outcome,
        Published {
            delivered: 1,
            dropped: 0,
            removed: 1
        }
    );
    assert!(!dying.exists());
    let message = successor.recv(WAIT).unwrap().expect("the very message");
    assert_eq!((message.header.seq, message.gap), (4, 0));
    assert_eq!(sockets(&subs).len(), 1);
    drop(successor);
    // The publisher takes its entry with it as the subscriber its socket.
    assert_eq!(root.contents().len(), 4, "{:?}", root.contents());
    drop(publisher);
    assert_eq!(root.contents(), ["servos/", "servos/pubs/", "servos/subs/"]);
}

#[test]
fn publishing_to_nobody_succeeds_and_leaves_only_the_latest_value_and_the_publishers_entry() {
    let root = Root::new();
    let bus = root.bus().rescan_every(Duration::ZERO);
    let entry = |topic: &str, source: &str, publisher: &lamp_bus::Publisher| {
        format!(
            "{topic}/pubs/{source}.{}.{}",
            std::process::id(),
            publisher.boot()
        )
    };
    let mut sound_of = bus.publisher("mic2", Kind::Event, "voice").unwrap();
    let mut pose = bus.publisher("servos", Kind::Latest, "body").unwrap();
    // A publisher that has said nothing has touched nothing.
    assert!(!root.0.exists());
    for n in 1..=100u64 {
        let outcome = sound_of.publish_bytes(n, "pcm_s16le_16000_mono", &sound(n));
        assert_eq!(outcome.unwrap(), Published::default());
    }
    // Events to nobody: nothing of them is kept. What there is, is the
    // publisher saying that it is there.
    let voice = entry("mic2", "voice", &sound_of);
    assert_eq!(root.contents(), ["mic2/", "mic2/pubs/", voice.as_str()]);
    let body = entry("servos", "body", &pose);
    for n in 1..=100u64 {
        let outcome = pose.publish_json(n, &json!({"yaw": n}));
        assert_eq!(outcome.unwrap(), Published::default());
        assert_eq!(
            root.contents()[3..],
            ["servos/", "servos/latest", "servos/pubs/", body.as_str()]
        );
    }
    let expected = Totals {
        published: 100,
        delivered: 0,
        dropped: 0,
    };
    assert_eq!((sound_of.totals(), pose.totals()), (expected, expected));
    assert!(pose.subscribers().is_empty());
    let current = bus.latest("servos").unwrap().unwrap();
    assert_eq!(current.header.seq, 100);
    assert_eq!(current.json().unwrap(), json!({"yaw": 100}));
    // The value outlives its publisher: it is what the next subscriber
    // reads. The publishers' entries go with them.
    drop(pose);
    drop(sound_of);
    assert_eq!(
        root.contents(),
        [
            "mic2/",
            "mic2/pubs/",
            "servos/",
            "servos/latest",
            "servos/pubs/"
        ]
    );
}

#[test]
fn invalid_topic_and_subscriber_names_are_refused_before_anything_is_created() {
    let outer = Root::new();
    fs::create_dir(&outer.0).unwrap();
    let bus = Bus::new(outer.0.join("r")).unwrap();
    let long = "x".repeat(97);
    let absolute = outer.0.join("elsewhere");
    for topic in [
        "../x",
        "a//b",
        "A",
        long.as_str(),
        absolute.to_str().unwrap(),
        "/x",
        "",
        "..",
        "a/../b",
        "a/",
        "servos/subs",
        "latest",
        "servos/pubs",
        "declaration",
        // What the rule before this one took: a dot, a hyphen, a digit first.
        "room.state",
        "light.wanted",
        "lamp-body/pose",
        "2d/pose",
    ] {
        let refused = [
            bus.publisher(topic, Kind::Latest, "body").err(),
            bus.subscribe(topic, "follow").err(),
            bus.latest(topic).err(),
        ];
        for error in refused {
            let error = error.unwrap_or_else(|| panic!("{topic:?} was taken"));
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{topic:?}");
            assert!(error.to_string().starts_with("topic "), "{error}");
        }
    }
    let long = "n".repeat(33);
    for name in [
        "",
        "a.b",
        "a/b",
        "../x",
        "A",
        "a b",
        "lamp-body",
        "_a",
        "2a",
        long.as_str(),
    ] {
        let error = bus.subscribe("servos", name).err().expect("refused");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{name:?}");
        assert!(error.to_string().starts_with("subscriber name "), "{error}");
        let error = bus.publisher("servos", Kind::Latest, name).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{name:?}");
        assert!(error.to_string().starts_with("publisher name "), "{error}");
    }
    // Not under the root, and not beside it where `../x` would have gone.
    assert!(outer.contents().is_empty(), "{:?}", outer.contents());
    // A root has to be a place every process calls the same.
    let error = Bus::new("run/lampos-bus").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("not an absolute path"));
}

#[test]
fn the_first_topics_are_named_by_the_rule_and_nest_as_directories() {
    let root = Root::new();
    let bus = root.bus();
    let names = [
        "mic1/audio",
        "mic2/audio",
        "camera/image",
        "body/joint_states",
        "humans/faces/tracked",
        "voice/heard",
        "voice/said",
        "voice/state",
        "voice/link",
        "room/state",
        "light/command",
        "light/state",
        "diagnostics",
    ];
    for name in names {
        let mut publisher = bus.publisher(name, Kind::Latest, "body").unwrap();
        publisher.publish_json(1, &json!({"of": name})).unwrap();
        let kept = bus.latest(name).unwrap().expect("the value");
        assert_eq!(kept.header.topic, name);
        assert_eq!(kept.json().unwrap(), json!({"of": name}));
    }
    let mut expected: Vec<&str> = names.to_vec();
    expected.sort_unstable();
    let listed = bus.topics().unwrap();
    let listed: Vec<&str> = listed.iter().map(|topic| topic.name.as_str()).collect();
    // A token that only groups topics, `voice` or `humans/faces`, is not one.
    assert_eq!(listed, expected);
    assert!(root.0.join("humans/faces/tracked/latest").is_file());
    // Through a socket three directories down.
    let mut subscriber = bus.subscribe("humans/faces/tracked", "f").unwrap();
    let mut eyes = bus
        .publisher("humans/faces/tracked", Kind::Latest, "eyes")
        .unwrap();
    assert_eq!(eyes.publish_json(2, &json!({"faces": []})).unwrap(), ONE);
    let message = subscriber.recv(WAIT).unwrap().expect("the faces");
    assert_eq!(message.header.source, "eyes");
    assert_eq!(message.json().unwrap(), json!({"faces": []}));
}

#[test]
fn a_message_over_the_limit_is_refused_and_nothing_is_sent_or_kept() {
    let root = Root::new();
    let bus = root.bus();
    let mut subscriber = bus.subscribe("camera", "follow").unwrap();
    let mut publisher = bus.publisher("camera", Kind::Latest, "eyes").unwrap();
    // The payload alone is the limit: the header takes it over.
    for attempt in [
        publisher.publish_bytes(1, "jpeg", &vec![0xff; MOST_BYTES]),
        publisher.publish_json(1, &"x".repeat(MOST_BYTES)),
        publisher.publish_bytes(1, "jpeg", &vec![0xff; 4 * MOST_BYTES]),
    ] {
        let error = attempt.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            error.to_string().contains("over the bus's 60000"),
            "{error}"
        );
    }
    assert!(subscriber.recv(BRIEF).unwrap().is_none());
    assert!(bus.latest("camera").unwrap().is_none());
    assert_eq!(root.contents().len(), 3, "{:?}", root.contents());
    assert_eq!(publisher.totals(), Totals::default());
    // A frame goes by reference, and the count did not move for the three
    // refused: this is the publisher's first message.
    let reference = json!({"file": "/run/lampos-eyes/sight.jpg", "bytes": 61_234});
    assert_eq!(publisher.publish_json(2, &reference).unwrap(), ONE);
    let message = subscriber.recv(WAIT).unwrap().unwrap();
    assert_eq!((message.header.seq, message.gap), (1, 0));
    assert_eq!(message.json().unwrap(), reference);
    assert_eq!(bus.latest("camera").unwrap().unwrap().header.seq, 1);
}

#[test]
fn a_message_of_exactly_the_limit_goes_through_whole_where_the_system_takes_one_that_long() {
    let root = Root::new();
    let bus = root.bus();
    let mut subscriber = bus.subscribe("mic1", "follow").unwrap();
    let mut publisher = bus.publisher("mic1", Kind::Event, "voice").unwrap();
    assert_eq!(publisher.publish_bytes(1, "x", b"").unwrap(), ONE);
    let first = subscriber.recv(WAIT).unwrap().unwrap();
    // The next header is as long as this one: the count stays one digit.
    let header = serde_json::to_vec(&first.header).unwrap().len() + 1;
    let payload: Vec<u8> = (0..MOST_BYTES - header).map(|index| index as u8).collect();
    let one_more = [&payload[..], &[0]].concat();
    let error = publisher.publish_bytes(1, "x", &one_more).unwrap_err();
    assert!(error.to_string().contains("of 60001 bytes"), "{error}");
    let whole = publisher.publish_bytes(1, "x", &payload);
    if cfg!(target_os = "linux") {
        assert_eq!(whole.unwrap(), ONE);
        let message = subscriber.recv(WAIT).unwrap().expect("the whole message");
        assert_eq!((message.header.seq, message.gap), (2, 0));
        assert_eq!(message.payload.len() + header, MOST_BYTES);
        assert!(message.payload == payload);
    } else {
        // macOS gives a local datagram socket 2,048 bytes to send with and
        // the standard library has no way to ask for more: the refusal is
        // the system's, is said so, and leaves the count where it was.
        let error = whole.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("operating system"), "{error}");
        assert!(subscriber.recv(BRIEF).unwrap().is_none());
        assert_eq!(
            publisher.publish_bytes(1, "x", &payload[..1_500]).unwrap(),
            ONE
        );
        let message = subscriber.recv(WAIT).unwrap().expect("a shorter message");
        assert_eq!((message.header.seq, message.gap), (2, 0));
        assert!(message.payload == payload[..1_500]);
    }
    assert_eq!(publisher.totals().published, 2);
}

#[test]
fn two_publishers_on_one_topic_keep_their_own_count_and_their_own_gaps() {
    let root = Root::new();
    let bus = root.bus();
    let mut subscriber = bus.subscribe("people", "follow").unwrap();
    let mut eyes = bus.publisher("people", Kind::Event, "eyes").unwrap();
    let mut voice = bus.publisher("people", Kind::Event, "voice").unwrap();
    assert_ne!(eyes.boot(), voice.boot());
    // What was published, in order: by whom, with what count, and whether
    // the subscriber's queue took it.
    let mut log: Vec<(&str, u64, bool)> = Vec::new();
    let mut publish = |publisher: &mut lamp_bus::Publisher, source: &'static str| {
        let outcome = publisher.publish_json(1, &json!({"by": source})).unwrap();
        assert_eq!(outcome.delivered + outcome.dropped, 1);
        log.push((source, publisher.totals().published, outcome.delivered == 1));
        outcome.delivered == 1
    };
    let mut got: Vec<(String, u64, u64)> = Vec::new();
    let mut read = |subscriber: &mut lamp_bus::Subscriber, wait: Duration| {
        let Some(message) = subscriber.recv(wait).unwrap() else {
            return false;
        };
        assert!(!message.publisher_restarted);
        assert_eq!(
            message.json().unwrap()["by"],
            message.header.source.as_str()
        );
        got.push((message.header.source, message.header.seq, message.gap));
        true
    };
    // By turns and read at once, twice as many from the eyes.
    for _ in 0..4 {
        assert!(publish(&mut eyes, "eyes") && read(&mut subscriber, WAIT));
        assert!(publish(&mut voice, "voice") && read(&mut subscriber, WAIT));
        assert!(publish(&mut eyes, "eyes") && read(&mut subscriber, WAIT));
    }
    // Nobody reads until the eyes have lost five, the voice speaking in
    // between. Whether the voice loses any is the system's affair: on Linux
    // each publisher has an allowance of its own.
    let mut lost = 0;
    while lost < 5 {
        if !publish(&mut eyes, "eyes") {
            lost += 1;
        }
        if lost % 2 == 1 {
            publish(&mut voice, "voice");
        }
        assert!(
            eyes.totals().published < 1_000_000,
            "the queue never filled"
        );
    }
    while read(&mut subscriber, BRIEF) {}
    // One more from each, which says what each of them lost.
    assert!(publish(&mut voice, "voice") && read(&mut subscriber, WAIT));
    assert!(publish(&mut eyes, "eyes") && read(&mut subscriber, WAIT));

    // What should have been seen: every message taken, in the order sent,
    // each with the number of its own publisher's dropped since the last.
    let mut expected: Vec<(String, u64, u64)> = Vec::new();
    let mut lost_by = [("eyes", 0u64), ("voice", 0u64)];
    for &(source, seq, taken) in &log {
        let lost = &mut lost_by.iter_mut().find(|(by, _)| *by == source).unwrap().1;
        if taken {
            expected.push((source.into(), seq, *lost));
            *lost = 0;
        } else {
            *lost += 1;
        }
    }
    assert_eq!(got, expected);
    let last_of = |source: &str| got.iter().rev().find(|(by, ..)| by == source).unwrap();
    assert!(last_of("eyes").2 >= 1, "the eyes' loss was not said");
    assert_eq!(last_of("eyes").1, eyes.totals().published);
    assert_eq!(last_of("voice").1, voice.totals().published);
    assert_ne!(eyes.totals().published, voice.totals().published);
}

#[test]
fn a_root_that_cannot_be_used_is_an_error_said_once_and_never_a_panic() {
    let outer = Root::new();
    fs::create_dir(&outer.0).unwrap();
    // A file where the root's parent should be.
    let blocked = outer.0.join("f");
    fs::write(&blocked, b"in the way").unwrap();
    let bus = Bus::new(blocked.join("bus"))
        .unwrap()
        .rescan_every(Duration::ZERO);
    assert!(bus.subscribe("servos", "follow").is_err());
    assert!(bus.latest("servos").is_err());
    assert!(bus.topics().is_err());

    let mut events = bus.publisher("mic2", Kind::Event, "voice").unwrap();
    let error = events.publish_json(1, &1).unwrap_err();
    assert!(error.to_string().starts_with("bus topic "), "{error}");
    for n in 2..=20u64 {
        assert_eq!(events.publish_json(n, &n).unwrap(), Published::default());
    }
    assert_eq!(events.totals().published, 20);
    let mut pose = bus.publisher("servos", Kind::Latest, "body").unwrap();
    assert!(pose.publish_json(1, &1).is_err());
    for n in 2..=20u64 {
        assert_eq!(pose.publish_json(n, &n).unwrap(), Published::default());
    }

    // The root becomes usable: the same publishers carry on, without a word.
    fs::remove_file(&blocked).unwrap();
    assert_eq!(pose.publish_json(21, &21).unwrap(), Published::default());
    assert_eq!(bus.latest("servos").unwrap().unwrap().header.seq, 21);
    let mut subscriber = bus.subscribe("mic2", "follow").unwrap();
    assert_eq!(events.publish_json(21, &21).unwrap(), ONE);
    assert_eq!(subscriber.recv(WAIT).unwrap().unwrap().header.seq, 21);
    drop(subscriber);
    assert_eq!(events.publish_json(22, &22).unwrap(), Published::default());

    // And when it breaks again that is said again, once.
    fs::remove_dir_all(&blocked).unwrap();
    fs::write(&blocked, b"in the way again").unwrap();
    assert!(pose.publish_json(22, &22).is_err());
    assert!(events.publish_json(23, &23).is_err());
    for n in 23..=30u64 {
        assert_eq!(pose.publish_json(n, &n).unwrap(), Published::default());
        assert_eq!(events.publish_json(n, &n).unwrap(), Published::default());
    }
    assert_eq!(pose.totals().published, 30);
}

#[test]
fn a_socket_path_too_long_for_the_system_is_refused_in_plain_words_and_nothing_is_created() {
    let root = Root::new();
    let bus = root.bus();
    let topic = ["a".repeat(32), "b".repeat(32), "c".repeat(30)].join("/");
    assert_eq!(topic.len(), lamp_bus::MOST_TOPIC_BYTES);
    let error = bus.subscribe(&topic, "follow").err().expect("refused");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    let text = error.to_string();
    assert!(
        text.contains("a Unix socket's address holds 103") && text.contains("use a shorter root"),
        "{text}"
    );
    assert!(!root.0.exists());
    // Nothing of a publisher is bound, so the same topic can be published
    // and its value kept and read.
    let mut publisher = bus.publisher(&topic, Kind::Latest, "body").unwrap();
    assert_eq!(publisher.publish_json(1, &1).unwrap(), Published::default());
    assert_eq!(bus.latest(&topic).unwrap().unwrap().header.seq, 1);
    // A topic as long as leaves room for the socket's name is taken.
    let room = lamp_bus::MOST_SOCKET_PATH_BYTES
        - root.0.as_os_str().len()
        - "/".len()
        - "/subs/".len()
        - format!("f.{}.999.sock", std::process::id()).len();
    let fits = "t".repeat(room.min(32));
    let subscriber = bus.subscribe(&fits, "f");
    assert!(subscriber.is_ok(), "{room} bytes of topic");
    let bound = sockets(&root.0.join(&fits).join("subs")).remove(0);
    assert!(bound.as_os_str().len() <= lamp_bus::MOST_SOCKET_PATH_BYTES);
}

#[test]
fn what_is_not_a_message_of_the_topic_is_passed_over_and_counted() {
    let root = Root::new();
    let bus = root.bus();
    let mut subscriber = bus.subscribe("servos", "follow").unwrap();
    let path = sockets(&root.0.join("servos/subs")).remove(0);
    let stranger = UnixDatagram::unbound().unwrap();
    let mut publisher = bus.publisher("servos", Kind::Event, "body").unwrap();
    stranger.send_to(b"hello", &path).unwrap();
    stranger.send_to(b"{}\n{}", &path).unwrap();
    stranger.send_to(b"", &path).unwrap();
    assert_eq!(publisher.publish_json(1, &1).unwrap(), ONE);
    let message = subscriber
        .recv(WAIT)
        .unwrap()
        .expect("the message behind them");
    assert_eq!((message.header.seq, message.gap), (1, 0));
    assert_eq!(subscriber.malformed(), 3);

    // A message made by hand, as a publisher in another language would
    // make it: taken where it belongs, and not a message at the socket of
    // another topic.
    let by_hand = b"{\"topic\":\"mic2\",\"kind\":\"event\",\"enc\":\"json\",\"source\":\"voice\",\
        \"boot\":\"0123456789abcdef\",\"seq\":9,\"at_us\":1,\"at_unix_ms\":1}\n{\"level\":3}";
    let mut other = bus.subscribe("mic2", "follow").unwrap();
    let there = sockets(&root.0.join("mic2/subs")).remove(0);
    stranger.send_to(by_hand, &there).unwrap();
    let message = other.recv(WAIT).unwrap().expect("the message made by hand");
    assert_eq!((message.header.seq, message.gap), (9, 0));
    assert_eq!(message.json().unwrap(), json!({"level": 3}));
    assert_eq!(other.malformed(), 0);
    stranger.send_to(by_hand, &path).unwrap();
    // Rubbish alone ends a wait with nothing, in the time asked for.
    let started = Instant::now();
    assert!(subscriber.recv(BRIEF).unwrap().is_none());
    assert!(started.elapsed() < WAIT);
    assert_eq!(subscriber.malformed(), 4);
    assert_eq!(subscriber.repeated(), 0);
}

#[test]
fn directories_get_the_mode_asked_for_and_files_and_sockets_owner_and_group_only() {
    let root = Root::new();
    let bus = root.bus().directory_mode(0o750);
    let subscriber = bus.subscribe("gemini/transcript", "follow").unwrap();
    for directory in ["", "gemini", "gemini/transcript", "gemini/transcript/subs"] {
        assert_eq!(mode(&root.0.join(directory)), 0o750, "{directory:?}");
    }
    let socket = sockets(&root.0.join("gemini/transcript/subs")).remove(0);
    assert_eq!(mode(&socket), 0o660);
    let mut pose = bus.publisher("servos", Kind::Latest, "body").unwrap();
    pose.publish_json(1, &1).unwrap();
    assert_eq!(mode(&root.0.join("servos")), 0o750);
    assert_eq!(mode(&root.0.join("servos/latest")), 0o660);
    drop(subscriber);

    // Group-writable unless said otherwise, and a directory that was
    // already there keeps the mode it had.
    let other = Root::new();
    fs::create_dir(&other.0).unwrap();
    fs::set_permissions(&other.0, fs::Permissions::from_mode(0o700)).unwrap();
    let subscriber = other.bus().subscribe("servos", "follow").unwrap();
    assert_eq!(mode(&other.0), 0o700);
    assert_eq!(mode(&other.0.join("servos")), 0o770);
    assert_eq!(mode(&other.0.join("servos/subs")), 0o770);
    drop(subscriber);
}

#[test]
fn the_newest_value_reaches_a_subscriber_whose_queue_missed_it() {
    let root = Root::new();
    // The publisher looks once; the subscriber looks at `latest` whenever
    // it has nothing waiting.
    let seldom = root.bus().rescan_every(NEVER);
    let bus = root.bus().rescan_every(Duration::ZERO);
    let mut publisher = seldom
        .publisher("room/state", Kind::Latest, "eyes")
        .unwrap();
    assert_eq!(
        publisher.publish_json(1, &json!({"people": 0})).unwrap(),
        Published::default()
    );
    // Bound just after the publisher's look: the next values are sent to
    // nobody.
    let mut subscriber = bus.subscribe("room/state", "follow").unwrap();
    assert_eq!(
        publisher.publish_json(2, &json!({"people": 1})).unwrap(),
        Published::default()
    );
    let message = subscriber.recv(BRIEF).unwrap().expect("the kept value");
    assert_eq!((message.header.seq, message.gap), (2, 0));
    assert_eq!(message.json().unwrap(), json!({"people": 1}));
    // Once: it is not news a second time.
    assert!(subscriber.recv(BRIEF).unwrap().is_none());
    assert!(subscriber.recv(Duration::ZERO).unwrap().is_none());
    for people in [2, 3, 4] {
        publisher
            .publish_json(3, &json!({"people": people}))
            .unwrap();
    }
    // The values between are gone, as a latest topic's are, and counted.
    let message = subscriber.recv(BRIEF).unwrap().expect("the kept value");
    assert_eq!((message.header.seq, message.gap), (5, 2));
    assert_eq!(message.json().unwrap(), json!({"people": 4}));
    assert_eq!(subscriber.repeated(), 0);

    // The same when the queue was full: the last value has no later
    // message behind it to show that it was dropped.
    let mut known = bus.publisher("servos", Kind::Latest, "body").unwrap();
    let mut follow = bus.subscribe("servos", "follow").unwrap();
    let mut held = 0u64;
    while known.publish_json(1, &json!({"yaw": 1})).unwrap() == ONE {
        held += 1;
        assert!(held < 1_000_000, "the queue never filled");
    }
    for _ in 0..3 {
        assert_eq!(
            known.publish_json(1, &json!({"yaw": 2})).unwrap().dropped,
            1
        );
    }
    for seq in 1..=held {
        let message = follow.recv(WAIT).unwrap().expect("a held value");
        assert_eq!((message.header.seq, message.gap), (seq, 0));
    }
    let message = follow.recv(BRIEF).unwrap().expect("the newest value");
    assert_eq!((message.header.seq, message.gap), (held + 4, 3));
    assert_eq!(message.json().unwrap(), json!({"yaw": 2}));
    assert!(follow.recv(BRIEF).unwrap().is_none());
    // A value that then arrives by the queue as well is passed over.
    assert_eq!(known.publish_json(1, &json!({"yaw": 3})).unwrap(), ONE);
    assert_eq!(follow.latest().unwrap().unwrap().header.seq, held + 5);
    assert!(follow.recv(BRIEF).unwrap().is_none());
    assert_eq!(follow.repeated(), 1);
}

#[test]
fn a_subscriber_that_binds_later_is_found_at_the_publishers_next_look() {
    let root = Root::new();
    let bus = root.bus().rescan_every(Duration::from_millis(40));
    let mut publisher = bus.publisher("mic2", Kind::Event, "voice").unwrap();
    assert_eq!(publisher.publish_json(1, &1).unwrap(), Published::default());
    let mut subscriber = bus.subscribe("mic2", "follow").unwrap();
    let started = Instant::now();
    let mut seq = 1;
    let found = loop {
        seq += 1;
        if publisher.publish_json(seq, &seq).unwrap() == ONE {
            break seq;
        }
        assert!(started.elapsed() < WAIT, "never found");
        thread::sleep(Duration::from_millis(2));
    };
    // What was said before it was found is not a gap: it was not there.
    let message = subscriber
        .recv(WAIT)
        .unwrap()
        .expect("the first it was sent");
    assert_eq!((message.header.seq, message.gap), (found, 0));
    assert!(found >= 2);
}

#[test]
fn topics_are_listed_with_who_listens_and_whether_a_value_is_kept() {
    let root = Root::new();
    let bus = root.bus();
    assert!(bus.topics().unwrap().is_empty());
    let words = bus.subscribe("gemini/transcript", "memory").unwrap();
    let first = bus.subscribe("servos", "follow").unwrap();
    let second = bus.subscribe("servos", "eyes").unwrap();
    drop(UnixDatagram::bind(root.0.join("servos/subs/ghost.1.0.sock")).unwrap());
    let mut state = bus.publisher("room/state", Kind::Latest, "eyes").unwrap();
    state.publish_json(1, &1).unwrap();
    // Put there by hand: a publish to `servos` would remove the dead
    // socket that is there to be listed.
    fs::write(root.0.join("servos/latest"), b"anything").unwrap();
    // What is not a topic is not listed.
    fs::create_dir(root.0.join("Not-a-topic")).unwrap();
    fs::create_dir_all(root.0.join("empty/inside")).unwrap();
    fs::write(root.0.join("notes"), b"a file").unwrap();
    let topic = |name: &str, subscribers, stale, latest| Topic {
        name: name.into(),
        subscribers,
        stale,
        latest,
    };
    assert_eq!(
        bus.topics().unwrap(),
        [
            topic("gemini/transcript", 1, 0, false),
            topic("room/state", 0, 0, true),
            topic("servos", 2, 1, true),
        ]
    );
    drop((words, first, second));
    assert_eq!(bus.topics().unwrap()[2], topic("servos", 0, 1, true));
}

#[test]
fn a_subscriber_that_is_being_bound_is_not_taken_for_a_dead_one() {
    let root = Root::new();
    let bus = root.bus().rescan_every(Duration::ZERO);
    let done = Arc::new(AtomicBool::new(false));
    // A publisher that looks at the directory with every message and sends
    // at once: if a socket's file can be seen before the socket can be sent
    // to, this is who finds it refused and removes it.
    let publishing = thread::spawn({
        let (bus, done) = (bus.clone(), Arc::clone(&done));
        move || {
            let mut publisher = bus.publisher("mic2", Kind::Event, "voice").unwrap();
            let mut published = 0u64;
            while !done.load(Ordering::SeqCst) {
                published += 1;
                publisher.publish_json(published, &published).unwrap();
            }
            published
        }
    });
    let mut unheard = Vec::new();
    for round in 0..400 {
        let mut subscriber = bus.subscribe("mic2", "born").unwrap();
        if subscriber.recv(Duration::from_secs(2)).unwrap().is_none() {
            unheard.push(round);
        }
    }
    done.store(true, Ordering::SeqCst);
    assert!(publishing.join().unwrap() > 400);
    assert!(
        unheard.is_empty(),
        "subscribers that never heard: {unheard:?}"
    );
}

#[test]
fn a_publisher_serves_sixty_four_subscribers_and_says_once_that_there_are_more() {
    let root = Root::new();
    let bus = root.bus().rescan_every(Duration::ZERO);
    let mut subscribers: Vec<_> = (0..=lamp_bus::MOST_SUBSCRIBERS)
        .map(|_| bus.subscribe("room/state", "s").unwrap())
        .collect();
    let mut publisher = bus.publisher("room/state", Kind::Event, "eyes").unwrap();
    let all = Published {
        delivered: 64,
        dropped: 0,
        removed: 0,
    };
    let error = publisher.publish_json(1, &1).unwrap_err();
    assert!(
        error.to_string().contains("more than 64 subscribers"),
        "{error}"
    );
    // Said once, and the sixty-four were served both times.
    assert_eq!(publisher.publish_json(2, &2).unwrap(), all);
    assert_eq!(publisher.subscribers().len(), 64);
    let mut left_out = Vec::new();
    for (index, subscriber) in subscribers.iter_mut().enumerate() {
        match subscriber.recv(BRIEF).unwrap() {
            Some(message) => {
                assert_eq!(message.header.seq, 1);
                assert_eq!(subscriber.recv(WAIT).unwrap().unwrap().header.seq, 2);
            }
            None => left_out.push(index),
        }
    }
    assert_eq!(left_out.len(), 1);
    // One of the sixty-four goes: the one left out is taken on at the next
    // look, and nothing is wrong any more.
    let served = if left_out[0] == 0 { 1 } else { 0 };
    drop(subscribers.swap_remove(served));
    assert_eq!(publisher.publish_json(3, &3).unwrap(), all);
    for subscriber in &mut subscribers {
        let message = subscriber.recv(WAIT).unwrap().expect("the third");
        assert_eq!((message.header.seq, message.gap), (3, 0));
    }
    // One too many again is said again.
    subscribers.push(bus.subscribe("room/state", "s").unwrap());
    assert!(publisher.publish_json(4, &4).is_err());
    assert_eq!(publisher.publish_json(5, &5).unwrap(), all);
    assert_eq!(publisher.totals().published, 5);
}

#[test]
fn a_declaration_is_written_when_its_publisher_is_made_and_every_message_names_the_type() {
    let root = Root::new();
    let bus = root.bus();
    let topic = "body/joint_states";
    assert!(bus.declaration(topic).unwrap().is_none());
    let joints = Declaration::new("lamp_msgs/JointState", 1, Kind::Latest)
        .every(Duration::from_millis(50))
        .lasting(Duration::from_millis(5_000));
    let mut body = bus.publisher_declared(topic, &joints, "body").unwrap();
    // There before a message is: one line, whole, for the owner and the
    // group, and nothing left over from the writing of it.
    assert_eq!(body.declaration(), Some(&joints));
    assert_eq!(bus.declaration(topic).unwrap(), Some(joints.clone()));
    let file = root.0.join(topic).join("declaration");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "{\"type\":\"lamp_msgs/JointState\",\"version\":1,\"kind\":\"latest\",\
         \"period_ms\":50,\"lifespan_ms\":5000}\n"
    );
    assert_eq!(mode(&file), 0o660);
    let entry = format!(
        "body/joint_states/pubs/body.{}.{}",
        std::process::id(),
        body.boot()
    );
    assert_eq!(
        root.contents(),
        [
            "body/",
            "body/joint_states/",
            "body/joint_states/declaration",
            "body/joint_states/pubs/",
            entry.as_str()
        ]
    );
    assert!(bus.latest(topic).unwrap().is_none());

    // What it sends says what it is and how long it lasts, in the datagram
    // and in the value kept.
    let mut follow = bus
        .subscribe_typed(topic, "f", "lamp_msgs/JointState")
        .unwrap();
    assert_eq!(follow.expects(), Some("lamp_msgs/JointState"));
    assert_eq!(follow.declaration(), Some(&joints));
    assert_eq!(
        body.publish_json(1, &json!({"position": [0.1]})).unwrap(),
        ONE
    );
    let message = follow.recv(WAIT).unwrap().expect("the joints");
    assert_eq!(
        message.header.type_name.as_deref(),
        Some("lamp_msgs/JointState")
    );
    assert_eq!(message.header.expires_ms, Some(5_000));
    assert_eq!(message.header.kind, Kind::Latest);
    assert!(!message.expired);
    let kept = bus.latest(topic).unwrap().expect("the value");
    assert_eq!(kept.header, message.header);

    // The declaration is the topic's: it stays when its writer goes. The
    // next publisher of the same type and kind says the rest anew.
    drop(body);
    assert_eq!(bus.declaration(topic).unwrap(), Some(joints.clone()));
    let newer =
        Declaration::new("lamp_msgs/JointState", 2, Kind::Latest).every(Duration::from_millis(100));
    let mut body = bus.publisher_declared(topic, &newer, "body").unwrap();
    assert_eq!(bus.declaration(topic).unwrap(), Some(newer.clone()));
    assert_eq!(
        body.publish_json(2, &json!({"position": [0.2]})).unwrap(),
        ONE
    );
    let message = follow.recv(WAIT).unwrap().expect("the joints");
    assert_eq!(message.header.expires_ms, None);
    assert!(message.publisher_restarted);
    // The subscriber read it again when the new instance spoke.
    assert_eq!(follow.declaration(), Some(&newer));

    // A publisher that declares nothing publishes there as it always did,
    // and a message that names no type is taken by one that expects one.
    let mut old = bus.publisher(topic, Kind::Latest, "old_body").unwrap();
    assert!(old.declaration().is_none());
    assert_eq!(
        old.publish_json(3, &json!({"position": [0.3]})).unwrap(),
        ONE
    );
    let message = follow.recv(WAIT).unwrap().expect("the untyped joints");
    assert_eq!(message.header.type_name, None);
    assert_eq!(message.header.source, "old_body");
    assert_eq!(bus.declaration(topic).unwrap(), Some(newer));
    assert_eq!(follow.wrong_type(), 0);

    // What cannot be a declaration is refused before anything is written.
    for wrong in [
        Declaration::new("joint_state", 1, Kind::Latest),
        Declaration::new("lamp_msgs/JointState", 1, Kind::Latest).every(Duration::ZERO),
    ] {
        let error = bus.publisher_declared("x/y", &wrong, "body").err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{error}");
    }
    assert!(!root.0.join("x").exists());
    // A file there that is no declaration says nothing: it is replaced.
    fs::create_dir_all(root.0.join("x/y")).unwrap();
    fs::write(root.0.join("x/y/declaration"), b"not json").unwrap();
    assert!(bus.declaration("x/y").is_err());
    assert!(bus.subscribe_typed("x/y", "f", "lamp_msgs/Speech").is_ok());
    let speech = Declaration::new("lamp_msgs/Speech", 1, Kind::Event);
    let _voice = bus.publisher_declared("x/y", &speech, "voice").unwrap();
    assert_eq!(bus.declaration("x/y").unwrap(), Some(speech));
}

#[test]
fn two_publishers_that_disagree_about_a_topic_are_not_both_made() {
    let root = Root::new();
    let bus = root.bus();
    let topic = "light/state";
    let light = Declaration::new("lamp_msgs/Light", 1, Kind::Latest);
    let ring = bus.publisher_declared(topic, &light, "ring").unwrap();
    let before = root.contents();

    let other_type = Declaration::new("lamp_msgs/Rgb", 1, Kind::Latest);
    let error = bus
        .publisher_declared(topic, &other_type, "actions")
        .err()
        .expect("refused");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    let text = error.to_string();
    assert!(
        text.contains("topic light/state is already declared as lamp_msgs/Light (latest)")
            && text.contains("this publisher says lamp_msgs/Rgb (latest)")
            && text.contains("two programs disagree"),
        "{text}"
    );
    let other_kind = Declaration::new("lamp_msgs/Light", 1, Kind::Event);
    let error = bus
        .publisher_declared(topic, &other_kind, "actions")
        .err()
        .expect("refused");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(
        error.to_string().contains("lamp_msgs/Light (event)"),
        "{error}"
    );
    // The one refused left nothing: not its declaration, not an entry.
    assert_eq!(root.contents(), before);
    assert_eq!(bus.declaration(topic).unwrap(), Some(light.clone()));
    let sources = bus.publishers(topic).unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].name, "ring");

    // A second publisher that agrees is made, whatever its name.
    let also = bus.publisher_declared(topic, &light, "actions").unwrap();
    assert_eq!(bus.publishers(topic).unwrap().len(), 2);
    drop(also);

    // The first having gone changes nothing: the declaration is the
    // topic's, until somebody removes it, as the refusal says.
    drop(ring);
    let error = bus
        .publisher_declared(topic, &other_type, "actions")
        .err()
        .expect("refused");
    let file = root.0.join(topic).join("declaration");
    assert!(
        error
            .to_string()
            .ends_with(&format!("remove {}", file.display())),
        "{error}"
    );
    fs::remove_file(&file).unwrap();
    let actions = bus
        .publisher_declared(topic, &other_type, "actions")
        .unwrap();
    assert_eq!(bus.declaration(topic).unwrap(), Some(other_type));
    drop(actions);

    // Started at the same moment, each with a type of its own: one is made
    // and every other is told, whichever came first.
    for round in 0..20 {
        let topic = format!("race/t{round}");
        let together = Arc::new(Barrier::new(6));
        let starting: Vec<_> = (0..6)
            .map(|n| {
                let (bus, topic, together) = (bus.clone(), topic.clone(), Arc::clone(&together));
                thread::spawn(move || {
                    let mine = Declaration::new(&format!("lamp_msgs/T{n}"), 1, Kind::Event);
                    together.wait();
                    bus.publisher_declared(&topic, &mine, "p")
                        .map(|publisher| (publisher, mine))
                })
            })
            .collect();
        let outcomes: Vec<_> = starting
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        let made: Vec<_> = outcomes.iter().flatten().collect();
        assert_eq!(made.len(), 1, "round {round}");
        assert_eq!(bus.declaration(&topic).unwrap().as_ref(), Some(&made[0].1));
        for refused in outcomes.iter().filter_map(|outcome| outcome.as_ref().err()) {
            assert_eq!(refused.kind(), io::ErrorKind::InvalidData, "{refused}");
        }
        // Nothing half-written is left beside the declaration.
        let left: Vec<String> = fs::read_dir(root.0.join(&topic))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(
            left.iter()
                .all(|name| name == "declaration" || name == "pubs"),
            "{left:?}"
        );
    }
}

#[test]
fn a_subscriber_that_expects_another_type_is_refused_or_passes_its_messages_over() {
    let root = Root::new();
    let bus = root.bus();
    // A topic that is declared: the subscriber is told at once, and leaves
    // nothing behind.
    let joints = Declaration::new("lamp_msgs/JointState", 3, Kind::Latest);
    let _body = bus
        .publisher_declared("body/joint_states", &joints, "body")
        .unwrap();
    let before = root.contents();
    let error = bus
        .subscribe_typed("body/joint_states", "f", "lamp_msgs/Faces")
        .err()
        .expect("refused");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    let text = error.to_string();
    assert!(
        text.contains(
            "topic body/joint_states is declared to carry lamp_msgs/JointState (version 3)"
        ) && text.contains("this subscriber expects lamp_msgs/Faces"),
        "{text}"
    );
    assert_eq!(root.contents(), before);
    let error = bus
        .subscribe_typed("body/joint_states", "f", "faces")
        .err()
        .expect("refused");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(error.to_string().starts_with("type \"faces\""), "{error}");
    assert_eq!(root.contents(), before);
    // The type it carries, and no type at all, are taken.
    let right = bus.subscribe_typed("body/joint_states", "f", "lamp_msgs/JointState");
    assert!(right.is_ok());
    assert!(bus.subscribe("body/joint_states", "f").is_ok());

    // A topic nobody has declared: the subscriber may be there first. It
    // is not refused, and what then comes with another type in its header
    // is passed over and counted.
    let mut early = bus
        .subscribe_typed("voice/heard", "f", "lamp_msgs/Speech")
        .unwrap();
    assert!(early.declaration().is_none());
    let faces = Declaration::new("lamp_msgs/Faces", 1, Kind::Latest);
    let mut eyes = bus
        .publisher_declared("voice/heard", &faces, "eyes")
        .unwrap();
    for n in 1..=3u64 {
        assert_eq!(eyes.publish_json(n, &json!({"faces": n})).unwrap(), ONE);
    }
    assert!(early.recv(BRIEF).unwrap().is_none());
    assert_eq!(early.wrong_type(), 3);
    assert_eq!(
        (early.malformed(), early.repeated(), early.expired()),
        (0, 0, 0)
    );
    // Nor is the value those left behind handed over, by `recv` or when
    // asked for, and it is not counted a second time.
    assert!(early.latest().unwrap().is_none());
    assert!(early.recv(Duration::ZERO).unwrap().is_none());
    assert_eq!(early.wrong_type(), 3);
    assert!(bus.latest("voice/heard").unwrap().is_some());
    // A subscriber that came after the wrong value was kept is not handed
    // it either: it is counted once, as what it is.
    let mut late = bus.subscribe("voice/heard", "g").unwrap();
    assert_eq!(late.recv(BRIEF).unwrap().expect("any type").header.seq, 3);
    drop(late);

    // The topic's own publisher, once the wrong declaration is out of the
    // way: its messages are what the subscriber is given.
    drop(eyes);
    fs::remove_file(root.0.join("voice/heard/declaration")).unwrap();
    let speech = Declaration::new("lamp_msgs/Speech", 1, Kind::Latest);
    let mut voice = bus
        .publisher_declared("voice/heard", &speech, "voice")
        .unwrap();
    assert_eq!(
        voice.publish_json(1, &json!({"text": "lamp"})).unwrap(),
        ONE
    );
    let message = early.recv(WAIT).unwrap().expect("the speech");
    assert_eq!(
        message.header.type_name.as_deref(),
        Some("lamp_msgs/Speech")
    );
    assert_eq!((message.gap, message.publisher_restarted), (0, false));
    assert_eq!(early.declaration(), Some(&speech));
    let statistics = early.statistics();
    assert_eq!((statistics.received, statistics.wrong_type), (1, 3));

    // What a message that was passed over would have said of its publisher
    // is said with the next one handed over: two were missed before the
    // one of another type.
    let path = sockets(&root.0.join("voice/heard/subs")).remove(0);
    let stranger = UnixDatagram::unbound().unwrap();
    let now = unix_ms();
    let typed = |name: &str| format!(",\"type\":\"lamp_msgs/{name}\"");
    let send = |seq, extra: &str| {
        stranger
            .send_to(&by_hand("voice/heard", seq, now, extra), &path)
            .unwrap();
    };
    send(1, &typed("Speech"));
    let message = early.recv(WAIT).unwrap().expect("made by hand");
    assert_eq!((message.header.seq, message.gap), (1, 0));
    send(4, &typed("Faces"));
    send(5, &typed("Speech"));
    let message = early.recv(WAIT).unwrap().expect("the one after");
    assert_eq!((message.header.seq, message.gap), (5, 2));
    assert_eq!(early.wrong_type(), 4);
    assert_eq!(early.statistics().gaps, 2);
}

#[test]
fn a_publisher_is_listed_while_it_shows_itself_alive_and_not_after() {
    let root = Root::new();
    let lease = Duration::from_millis(300);
    let bus = root.bus().lease(lease).rescan_every(Duration::ZERO);
    let pubs = root.0.join("servos/pubs");
    let modified = |path: &Path| fs::metadata(path).unwrap().modified().unwrap();
    let files = |directory: &Path| fs::read_dir(directory).map_or(0, Iterator::count);

    // A publisher that declares nothing is not there until it speaks.
    let mut body = bus.publisher("servos", Kind::Event, "body").unwrap();
    assert!(bus.publishers("servos").unwrap().is_empty());
    let entry = pubs.join(format!("body.{}.{}", std::process::id(), body.boot()));
    let began = Instant::now();
    body.publish_json(1, &1).unwrap();
    let first = modified(&entry);
    let listed = bus.publishers("servos").unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].name, "body");
    assert_eq!(listed[0].pid, std::process::id());
    assert_eq!(listed[0].boot, body.boot());
    assert!(listed[0].age <= lease);
    assert_eq!(mode(&entry), 0o660);
    assert_eq!(fs::metadata(&entry).unwrap().len(), 0);
    assert_eq!(mode(&pubs), 0o770);
    // Publishing again at once does not touch it again: a third of the
    // lease has to pass, a second at the most.
    for n in 2..=50u64 {
        body.publish_json(n, &n).unwrap();
    }
    let again = modified(&entry);
    if began.elapsed() < lease / 3 {
        assert_eq!(again, first);
    }
    // After that the next message refreshes it.
    thread::sleep(lease / 3 + Duration::from_millis(20));
    body.publish_json(51, &51).unwrap();
    let refreshed = modified(&entry);
    assert!(refreshed > first);
    // And so does saying that it is alive, with nothing to publish.
    thread::sleep(lease / 3 + Duration::from_millis(20));
    body.alive().unwrap();
    assert!(modified(&entry) > refreshed);
    assert_eq!(body.totals().published, 51);

    // One that only ever says so is listed too, and a subscriber is given
    // the same list.
    let mut quiet = bus.publisher("servos", Kind::Event, "voice").unwrap();
    quiet.alive().unwrap();
    let follow = bus.subscribe("servos", "f").unwrap();
    let names = |sources: Vec<lamp_bus::Source>| {
        sources
            .into_iter()
            .map(|source| source.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(follow.sources().unwrap()), ["body", "voice"]);
    // One that declared its topic is there from the start.
    let state = Declaration::new("lamp_msgs/RoomState", 1, Kind::Latest);
    let declared = bus
        .publisher_declared("room/state", &state, "voice")
        .unwrap();
    assert_eq!(names(bus.publishers("room/state").unwrap()), ["voice"]);

    // Neither says anything for longer than the lease: both are taken for
    // dead, and whoever lists them removes what they left.
    thread::sleep(lease + Duration::from_millis(100));
    assert_eq!(files(&pubs), 2);
    assert!(follow.sources().unwrap().is_empty());
    assert_eq!(files(&pubs), 0);
    // It was only silent. Its next message puts it back.
    body.publish_json(52, &52).unwrap();
    assert_eq!(names(bus.publishers("servos").unwrap()), ["body"]);
    assert!(quiet.alive().is_ok());
    assert_eq!(names(bus.publishers("servos").unwrap()), ["body", "voice"]);

    // A publisher that ends takes its entry with it at once.
    drop(body);
    assert_eq!(names(bus.publishers("servos").unwrap()), ["voice"]);
    drop(quiet);
    assert_eq!(files(&pubs), 0);
    drop(declared);
    assert_eq!(files(&root.0.join("room/state/pubs")), 0);

    // Left by a process that died: listed for as long as its time is
    // within the lease, by whatever name it had, and then removed.
    let ghost = pubs.join("ghost.1.0123456789abcdef");
    fs::write(&ghost, b"").unwrap();
    let listed = bus.publishers("servos").unwrap();
    assert_eq!((listed[0].name.as_str(), listed[0].pid), ("ghost", 1));
    let set = |path: &Path, time: SystemTime| {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    };
    set(
        &ghost,
        SystemTime::now() - lease - Duration::from_millis(200),
    );
    assert!(bus.publishers("servos").unwrap().is_empty());
    assert!(!ghost.exists());
    // A time ahead of the clock by less than the lease is a clock read a
    // moment apart. One further ahead is what a clock set back leaves, and
    // would otherwise be alive for as long as the clock was wrong.
    fs::write(&ghost, b"").unwrap();
    set(&ghost, SystemTime::now() + lease / 2);
    let listed = bus.publishers("servos").unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].age, Duration::ZERO);
    set(&ghost, SystemTime::now() + Duration::from_secs(3_600));
    assert!(bus.publishers("servos").unwrap().is_empty());
    assert!(!ghost.exists());
    // What is not an entry is neither listed nor removed.
    for other in ["notes", ".body.1.0123456789abcdef.new", "body.1.xyz"] {
        fs::write(pubs.join(other), b"").unwrap();
        set(
            &pubs.join(other),
            SystemTime::now() - Duration::from_secs(60),
        );
    }
    assert!(bus.publishers("servos").unwrap().is_empty());
    assert_eq!(files(&pubs), 3);
    // A topic nobody has published has no publishers, and that is no error.
    assert!(bus.publishers("mic2").unwrap().is_empty());
    assert!(bus.publishers("../x").is_err());
}

#[test]
fn a_subscriber_is_told_once_that_the_source_has_gone_and_once_that_it_is_back() {
    let root = Root::new();
    let lease = Duration::from_millis(200);
    let bus = root.bus().lease(lease).rescan_every(Duration::ZERO);
    let began = Instant::now();
    let mut follow = bus.subscribe("room/state", "f").unwrap();
    assert!(follow.sources().unwrap().is_empty());
    // Nobody publishes it. A lease after subscribing that is said, once.
    until("the source to be missed", || follow.source_gone());
    assert!(began.elapsed() > lease, "{:?}", began.elapsed());
    assert_eq!(follow.notices(), [Notice::SourceGone]);
    assert!(follow.notices().is_empty());
    assert!(follow.source_gone());
    assert!(follow.notices().is_empty());

    // A publisher that declares its topic shows itself before it speaks:
    // the source is back, said once.
    let state = Declaration::new("lamp_msgs/RoomState", 1, Kind::Latest);
    let mut voice = bus
        .publisher_declared("room/state", &state, "voice")
        .unwrap();
    until("the source to be seen", || {
        voice.alive().unwrap();
        !follow.source_gone()
    });
    assert_eq!(follow.notices(), [Notice::SourceBack]);
    assert!(follow.notices().is_empty());
    assert_eq!(follow.sources().unwrap().len(), 1);

    // It ends without ever having spoken: nothing has come for a lease, so
    // the moment its entry is gone the source is.
    drop(voice);
    assert_eq!(follow.notices(), [Notice::SourceGone]);
    assert!(follow.notices().is_empty());

    // A publisher that declares nothing is back with its first message.
    let mut voice = bus.publisher("room/state", Kind::Latest, "voice").unwrap();
    assert!(follow.notices().is_empty());
    // Before it speaks: its last sign of life is no earlier than this.
    let spoke = Instant::now();
    assert_eq!(voice.publish_json(1, &json!("quiet")).unwrap(), ONE);
    let message = follow.recv(WAIT).unwrap().expect("the state");
    assert_eq!(message.json().unwrap(), json!("quiet"));
    assert_eq!(follow.notices(), [Notice::SourceBack]);
    assert!(!follow.source_gone());

    // It hangs: no message, and its entry is not refreshed. A lease after
    // its last sign of life it is gone, though its process is there.
    until("the source to be missed again", || follow.source_gone());
    assert!(spoke.elapsed() > lease, "{:?}", spoke.elapsed());
    assert_eq!(follow.notices(), [Notice::SourceGone]);
    assert!(follow.sources().unwrap().is_empty());
    // And back when it speaks, with nothing lost and nothing said twice.
    assert_eq!(voice.publish_json(2, &json!("busy")).unwrap(), ONE);
    let message = follow.recv(WAIT).unwrap().expect("the state");
    assert_eq!((message.header.seq, message.gap), (2, 0));
    assert_eq!(follow.notices(), [Notice::SourceBack]);
    assert!(follow.notices().is_empty());
    assert_eq!(follow.sources().unwrap().len(), 1);

    // A subscriber that never asks is told nothing and keeps receiving.
    let mut plain = bus.subscribe("room/state", "g").unwrap();
    assert_eq!(voice.publish_json(3, &json!("quiet")).unwrap().delivered, 2);
    assert_eq!(plain.recv(WAIT).unwrap().unwrap().header.seq, 3);
}

#[test]
fn messages_alone_keep_a_source_and_a_publisher_from_another_language_needs_no_entry() {
    let root = Root::new();
    let lease = Duration::from_millis(200);
    let bus = root.bus().lease(lease).rescan_every(Duration::ZERO);
    let mut follow = bus.subscribe("mic2/audio", "f").unwrap();
    let path = sockets(&root.0.join("mic2/audio/subs")).remove(0);
    let stranger = UnixDatagram::unbound().unwrap();
    // Nothing is ever put in `pubs/`. For two leases a message comes every
    // few milliseconds, and the source is there throughout.
    let started = Instant::now();
    let (mut seq, mut last, mut slowest) = (0u64, Instant::now(), Duration::ZERO);
    let mut sent = started;
    while started.elapsed() < 2 * lease {
        seq += 1;
        sent = Instant::now();
        stranger
            .send_to(&by_hand("mic2/audio", seq, unix_ms(), ""), &path)
            .unwrap();
        let message = follow.recv(WAIT).unwrap().expect("made by hand");
        assert_eq!((message.header.seq, message.gap), (seq, 0));
        let said = follow.notices();
        // If this thread was not run for half a lease, a silence that long
        // is the machine's and not the bus's, and proves nothing.
        slowest = slowest.max(last.elapsed());
        last = Instant::now();
        assert!(
            said.is_empty() || slowest > lease / 2,
            "{said:?} with messages at most {slowest:?} apart"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert!(follow.sources().unwrap().is_empty());
    assert!(!root.0.join("mic2/audio/pubs").exists());
    // It stops. With no entry to speak for it, a lease after its last
    // message it is gone.
    until("the source to be missed", || follow.source_gone());
    assert!(sent.elapsed() > lease);
    assert!(follow.notices().contains(&Notice::SourceGone));
}

#[test]
fn a_topic_that_promised_a_period_is_late_after_twice_that_and_on_time_with_the_next() {
    let root = Root::new();
    let bus = root.bus().rescan_every(Duration::ZERO);
    let period = Duration::from_millis(80);
    let topic = "body/joint_states";
    let joints = Declaration::new("lamp_msgs/JointState", 1, Kind::Event).every(period);
    // One subscriber is there before the topic is declared, one after.
    let mut early = bus.subscribe(topic, "e").unwrap();
    assert!(early.declaration().is_none());
    let mut body = bus.publisher_declared(topic, &joints, "body").unwrap();
    let mut follow = bus.subscribe(topic, "f").unwrap();
    assert_eq!(follow.declaration(), Some(&joints));
    // A topic that promises nothing is never late.
    let mut other = bus.subscribe("voice/heard", "f").unwrap();
    let mut voice = bus.publisher("voice/heard", Kind::Event, "voice").unwrap();
    assert_eq!(voice.publish_json(1, &1).unwrap(), ONE);
    assert!(other.recv(WAIT).unwrap().is_some());

    // Before it is sent: the message cannot have come earlier than this.
    let sent = Instant::now();
    assert_eq!(body.publish_json(1, &1).unwrap().delivered, 2);
    assert!(follow.recv(WAIT).unwrap().is_some());
    assert!(early.recv(WAIT).unwrap().is_some());
    // The early one learned the period when the publisher first spoke.
    assert_eq!(early.declaration(), Some(&joints));
    // Nothing more comes: late after twice the period, said once.
    until("the topic to be late", || follow.is_late());
    assert!(sent.elapsed() > 2 * period, "{:?}", sent.elapsed());
    assert_eq!(follow.notices(), [Notice::Late]);
    assert!(follow.notices().is_empty());
    assert!(follow.is_late());
    assert!(follow.notices().is_empty());
    until("the topic to be late for the early one", || early.is_late());
    assert_eq!(early.notices(), [Notice::Late]);

    // The next message ends it, said once.
    assert_eq!(body.publish_json(2, &2).unwrap().delivered, 2);
    assert!(follow.recv(WAIT).unwrap().is_some());
    assert_eq!(follow.notices(), [Notice::OnTime]);
    assert!(follow.notices().is_empty());

    // A silence nobody asked about while it lasted is said with its end:
    // the early subscriber has not looked since it was told `Late`, and the
    // other is asked nothing until the next message has come.
    assert!(early.recv(WAIT).unwrap().is_some());
    thread::sleep(2 * period + Duration::from_millis(40));
    assert_eq!(body.publish_json(3, &3).unwrap().delivered, 2);
    assert!(follow.recv(WAIT).unwrap().is_some());
    assert_eq!(follow.notices(), [Notice::Late, Notice::OnTime]);
    assert!(early.recv(WAIT).unwrap().is_some());
    assert_eq!(
        early.notices(),
        [Notice::OnTime, Notice::Late, Notice::OnTime]
    );

    // A subscriber whose declared topic never speaks at all is told so,
    // twice the period after it subscribed.
    drop(body);
    let subscribed = Instant::now();
    let mut waiting = bus.subscribe(topic, "w").unwrap();
    until("the silent topic to be late", || waiting.is_late());
    assert!(subscribed.elapsed() > 2 * period);
    assert_eq!(waiting.notices()[0], Notice::Late);

    // All this while the topic without a period was silent, and not late.
    assert!(!other.is_late());
    assert!(!other.notices().contains(&Notice::Late));
}

#[test]
fn a_subscriber_that_has_not_read_is_not_told_that_its_topic_went_quiet() {
    let root = Root::new();
    let lease = Duration::from_millis(150);
    let bus = root.bus().lease(lease).rescan_every(Duration::ZERO);
    let period = Duration::from_millis(60);
    let topic = "mic2/audio";
    // The period is declared, and the messages are made by hand: no
    // publisher has an entry to speak for the topic, so only they say that
    // it has a source.
    let audio = Declaration::new("lamp_msgs/Audio", 1, Kind::Event).every(period);
    drop(bus.publisher_declared(topic, &audio, "mic").unwrap());
    assert!(bus.publishers(topic).unwrap().is_empty());
    let mut busy = bus.subscribe(topic, "b").unwrap();
    assert_eq!(busy.declaration(), Some(&audio));
    let path = sockets(&root.0.join(topic).join("subs")).remove(0);
    let stranger = UnixDatagram::unbound().unwrap();
    // Nine messages 20 ms apart while the subscriber reads none of them:
    // longer than the lease, and than twice the period. Says how far apart
    // the two furthest apart were sent: if this thread was held up between
    // two for longer than twice the period, the topic was late, by the
    // test's own doing.
    let mut next = 1u64;
    let mut burst = |last_sent: u64| {
        let started = Instant::now();
        let mut sent = vec![last_sent];
        for _ in 0..9 {
            sent.push(unix_ms());
            stranger
                .send_to(&by_hand(topic, next, sent[sent.len() - 1], ""), &path)
                .unwrap();
            next += 1;
            thread::sleep(Duration::from_millis(20));
        }
        assert!(started.elapsed() > lease.max(2 * period));
        let widest = sent.windows(2).map(|pair| pair[1] - pair[0]).max().unwrap();
        (sent[9], widest > 2 * period.as_millis() as u64)
    };
    let read = |busy: &mut lamp_bus::Subscriber, from: u64| {
        for seq in from..from + 9 {
            let message = busy.recv(WAIT).unwrap().expect("a waiting message");
            assert_eq!((message.header.seq, message.gap), (seq, 0));
        }
    };
    // During the first burst it reads nothing, and then asks. Nothing was
    // taken in all that time and nothing is wrong with the topic: what it
    // sent is waiting.
    let (last_sent, held_up) = burst(unix_ms());
    let said = (busy.is_late(), busy.source_gone(), busy.notices());
    assert!(said == (false, false, Vec::new()) || held_up, "{said:?}");
    read(&mut busy, 1);
    let said = busy.notices();
    assert!(said.is_empty() || held_up, "{said:?}");
    // During the second it reads nothing either, and then reads without
    // having asked: the tenth message is taken a long time after the ninth
    // was. They were sent 20 ms apart, and that is what counts.
    let (_, held_up) = burst(last_sent);
    // Before the last is read: the lease runs from no earlier than this.
    let reading = Instant::now();
    read(&mut busy, 10);
    let said = busy.notices();
    assert!(said.is_empty() || held_up, "{said:?}");
    assert_eq!(busy.statistics().received, 18);
    // With everything read and nothing more coming, it is told both.
    until("the topic to be late", || busy.is_late());
    until("the source to be missed", || busy.source_gone());
    assert!(reading.elapsed() > lease);
    let said = busy.notices();
    assert!(
        said.contains(&Notice::Late) && said.contains(&Notice::SourceGone),
        "{said:?}"
    );
    // One message more, read after a silence: back, and on time again.
    stranger
        .send_to(&by_hand(topic, 19, unix_ms(), ""), &path)
        .unwrap();
    assert_eq!(busy.recv(WAIT).unwrap().expect("the next").header.seq, 19);
    let said = busy.notices();
    assert!(
        said.contains(&Notice::OnTime) && said.contains(&Notice::SourceBack),
        "{said:?}"
    );
}

#[test]
fn a_message_that_has_outlived_its_time_is_not_handed_over_and_is_counted() {
    let root = Root::new();
    let bus = root.bus().rescan_every(Duration::ZERO);
    let topic = "light/state";
    let mut follow = bus.subscribe(topic, "f").unwrap();
    let mut ring = bus.publisher(topic, Kind::Latest, "ring").unwrap();
    // Within its time it arrives as any message does.
    ring.expires_after(Some(Duration::from_secs(60)));
    assert_eq!(ring.publish_json(1, &json!({"on": true})).unwrap(), ONE);
    let message = follow.recv(WAIT).unwrap().expect("in time");
    assert_eq!(
        (message.header.expires_ms, message.expired),
        (Some(60_000), false)
    );
    assert_eq!(follow.expired(), 0);

    // One that lasts 20 ms and is read 150 ms later is not handed over,
    // from the queue or from the file, and is counted once.
    ring.expires_after(Some(Duration::from_millis(20)));
    assert_eq!(ring.publish_json(2, &json!({"on": false})).unwrap(), ONE);
    thread::sleep(Duration::from_millis(150));
    assert!(follow.recv(BRIEF).unwrap().is_none());
    assert!(follow.recv(Duration::ZERO).unwrap().is_none());
    assert_eq!(follow.expired(), 1);
    assert_eq!(follow.statistics().expired, 1);
    // Asked for, the value is given and marked: the last thing known.
    let kept = follow.latest().unwrap().expect("the old value");
    assert_eq!((kept.header.seq, kept.expired), (2, true));
    assert_eq!(kept.json().unwrap(), json!({"on": false}));
    let kept = bus.latest(topic).unwrap().expect("the old value");
    assert_eq!((kept.header.seq, kept.expired), (2, true));
    assert_eq!(follow.expired(), 1);

    // A subscriber that comes late to an expired value is not handed it
    // by `recv`, counts it once, and can still ask for it.
    let mut late = bus.subscribe(topic, "g").unwrap();
    assert!(late.recv(BRIEF).unwrap().is_none());
    assert!(late.recv(BRIEF).unwrap().is_none());
    assert_eq!(late.expired(), 1);
    assert!(late.latest().unwrap().expect("the old value").expired);
    // One that asks first is given it marked, and `recv` does not count
    // what was already taken.
    let mut asks = bus.subscribe(topic, "h").unwrap();
    assert!(asks.latest().unwrap().expect("the old value").expired);
    assert!(asks.recv(BRIEF).unwrap().is_none());
    assert_eq!(asks.expired(), 0);

    // The next value in time is handed over: the expired one was received,
    // so it is not a gap.
    ring.expires_after(Some(Duration::from_secs(60)));
    assert_eq!(
        ring.publish_json(3, &json!({"on": true}))
            .unwrap()
            .delivered,
        3
    );
    for subscriber in [&mut follow, &mut late, &mut asks] {
        let message = subscriber.recv(WAIT).unwrap().expect("in time");
        assert_eq!((message.header.seq, message.gap), (3, 0));
        assert!(!message.expired && !message.publisher_restarted);
    }
    assert!(!bus.latest(topic).unwrap().unwrap().expired);

    // By hand, so that the header says exactly when: ten seconds old with
    // a life of a hundred milliseconds, then one stamped ten seconds ahead,
    // then one with no limit that is as old as the clock.
    let path = sockets(&root.0.join(topic).join("subs")).remove(0);
    let stranger = UnixDatagram::unbound().unwrap();
    let now = unix_ms();
    let send = |seq, at, extra: &str| {
        stranger
            .send_to(&by_hand(topic, seq, at, extra), &path)
            .unwrap();
    };
    send(1, now - 10_000, ",\"expires_ms\":100");
    send(2, now + 10_000, ",\"expires_ms\":100");
    let message = follow.recv(WAIT).unwrap().expect("the one from ahead");
    assert_eq!((message.header.seq, message.gap), (2, 0));
    send(3, 1, "");
    let message = follow.recv(WAIT).unwrap().expect("the one with no limit");
    assert_eq!(message.header.at_unix_ms, 1);
    assert_eq!(follow.expired(), 2);
    // Two missed before an expired one are said with the next handed over.
    send(6, now - 10_000, ",\"expires_ms\":100");
    send(7, now, ",\"expires_ms\":60000");
    let message = follow.recv(WAIT).unwrap().expect("the one in time");
    assert_eq!((message.header.seq, message.gap), (7, 2));
    assert_eq!(follow.expired(), 3);
}

#[test]
fn a_subscriber_keeps_statistics_of_what_came_and_what_did_not() {
    let root = Root::new();
    let bus = root.bus();
    let mut follow = bus
        .subscribe_typed("servos", "f", "lamp_msgs/JointState")
        .unwrap();
    assert_eq!(follow.statistics(), Statistics::default());
    let path = sockets(&root.0.join("servos/subs")).remove(0);
    let stranger = UnixDatagram::unbound().unwrap();
    let joints = ",\"type\":\"lamp_msgs/JointState\"";
    let send = |seq, age_ms: u64, extra: &str| {
        let extra = format!("{joints}{extra}");
        stranger
            .send_to(&by_hand("servos", seq, unix_ms() - age_ms, &extra), &path)
            .unwrap();
    };
    let mut take = |seq, gap| {
        let message = follow.recv(WAIT).unwrap().expect("a message");
        assert_eq!((message.header.seq, message.gap), (seq, gap));
        follow.statistics()
    };
    // Each sent 400 ms before it is received.
    send(1, 400, "");
    let one = take(1, 0);
    assert_eq!((one.received, one.gaps), (1, 0));
    // It takes two to have a time between them.
    assert_eq!((one.mean_interval, one.largest_interval), (None, None));
    assert!(one.mean_age >= Some(Duration::from_millis(400)), "{one:?}");
    thread::sleep(Duration::from_millis(30));
    send(2, 400, "");
    take(2, 0);
    // Two never arrive.
    send(5, 400, "");
    take(5, 2);
    thread::sleep(Duration::from_millis(90));
    send(6, 400, "");
    take(6, 0);
    // One of another type, then one missed, then one that had expired.
    stranger
        .send_to(
            &by_hand("servos", 7, unix_ms(), ",\"type\":\"lamp_msgs/Faces\""),
            &path,
        )
        .unwrap();
    send(9, 10_000, ",\"expires_ms\":100");
    send(10, 400, "");
    let all = take(10, 1);
    assert_eq!(
        (all.received, all.gaps, all.expired, all.wrong_type),
        (5, 3, 1, 1)
    );
    // Four times between the five that were handed over: at least 30 ms,
    // next to nothing, at least 90 ms, next to nothing.
    let (mean, largest) = (all.mean_interval.unwrap(), all.largest_interval.unwrap());
    assert!(largest >= Duration::from_millis(90), "{all:?}");
    assert!(
        mean >= Duration::from_millis(30) && mean <= largest,
        "{all:?}"
    );
    // Every one was 400 ms old when sent, and read soon after.
    let age = all.mean_age.unwrap();
    assert!(
        age >= Duration::from_millis(400) && age < Duration::from_millis(5_400),
        "{all:?}"
    );
    assert_eq!((follow.expired(), follow.wrong_type()), (1, 1));
    assert_eq!((follow.malformed(), follow.repeated()), (0, 0));
}
