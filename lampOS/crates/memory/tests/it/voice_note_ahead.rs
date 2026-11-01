//! The second route's note read ahead of its turn (#3016), with Voice's own
//! Days client against the real store: the memory counts a follow-up's offer
//! as it gives one, and a thread offered for the last time is recorded as
//! given. So a note read as a line begins asks no follow-up; only the
//! answered turn that takes the read asks it. A thread offered on two
//! earlier days is then still the person's after a read whose line was never
//! answered, and is told, its last offer spent, by a read a turn takes.
use lamp_memory::{
    Options, Person, Server,
    days::{DATABASE, Days},
    server::Stopper,
    store::LocalTime,
    tidy::{Category, Output, Proposal},
};
use lamp_voice::remember::{DaysClient, person_id};
use rusqlite::Connection;
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread::JoinHandle,
    time::Duration,
};

const NOW: u64 = 1_791_892_800_000;
const DAY: u64 = 86_400_000;
/// What a read is given: this checks what is told and spent, not how fast,
/// and each open commits with a full fsync, which a loaded disk can take
/// past the voice's own bounds (#948).
const BUDGET: Duration = Duration::from_secs(10);
const THREAD: &str = "Tired of the synthetic commute";
static NEXT: AtomicU64 = AtomicU64::new(0);

/// A store whose person has a thread offered on two earlier days and never
/// asked about, served on its own socket on the third day.
struct Lamp {
    root: PathBuf,
    socket: PathBuf,
    stop: Stopper,
    worker: Option<JoinHandle<()>>,
    /// Held for the fixture's life, so real-store voice fixtures take turns.
    _turn: std::sync::MutexGuard<'static, ()>,
}

fn options(root: &Path) -> Options {
    Options {
        dir: root.join("store"),
        keep_days: 30,
        local: LocalTime::Offset(0),
    }
}

impl Lamp {
    fn new(person: &Person) -> Self {
        let turn = crate::voice_store_turn();
        let root = std::env::temp_dir().join(format!(
            "lamp-vahead-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let mut store = Days::open(options(&root), NOW).unwrap();
        store.maintain(NOW).unwrap();
        let said = store.append(person, NOW, THREAD, "Oh, nice", &[]).unwrap();
        let job = store
            .conversation_end(person, &format!("c{NOW}"), &[said])
            .unwrap();
        let thread = Output {
            observations: vec![Proposal {
                text: THREAD.into(),
                category: Category::Thread,
                source_ids: vec![said],
                supersedes: None,
                replaces: None,
                on: None,
            }],
        };
        store.tidy_apply(person, &job.job, &thread).unwrap();
        for day in [1, 2] {
            store.maintain(NOW + day * DAY).unwrap();
            let given = store.follow_up_until_asked(person, false, true).unwrap();
            assert_eq!(given.unwrap()["until_asked"], true, "day {day}");
        }
        drop(store);
        let now = NOW + 3 * DAY;
        let mut store = Days::open(options(&root), now).unwrap();
        let socket = root.join("m.sock");
        let server = Server::bind(&socket).unwrap();
        let stop = server.stopper();
        let worker = std::thread::spawn(move || {
            server.run(&mut store, &mut || now, &mut |_| {}).unwrap();
        });
        Self {
            root,
            socket,
            stop,
            worker: Some(worker),
            _turn: turn,
        }
    }

    /// The thread's offers so far, and whether it is recorded as given.
    fn offers(&self) -> (u64, u64) {
        let db = Connection::open(self.root.join("store").join(DATABASE)).unwrap();
        let offers = db
            .query_row(
                "SELECT coalesce(sum(offers),0) FROM follow_up_offers",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let given = db
            .query_row("SELECT count(*) FROM follow_ups", [], |r| r.get(0))
            .unwrap();
        (offers, given)
    }
}

impl Drop for Lamp {
    fn drop(&mut self) {
        self.stop.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_note_read_ahead_spends_the_last_follow_up_offer_only_when_a_turn_takes_it() {
    let id = person_id("Synthetic P");
    let lamp = Lamp::new(&Person::new(&id).unwrap());
    let client = DaysClient::new(lamp.socket.clone()).unwrap();
    client.following_up();
    assert_eq!(lamp.offers(), (2, 0), "offered twice, never asked");

    // Canceled before its context: read as a line begins, never answered.
    let read = client.read_note_within(1, Some(&id), false, BUDGET, (true, false));
    assert!(read.complete() && read.wants_follow_up());
    drop(read);
    assert_eq!(
        lamp.offers(),
        (2, 0),
        "a read never told asks no follow-up: the last offer is unspent"
    );

    // Taken by an answered turn: its follow-up read then, and told.
    let mut read = client.read_note_within(2, Some(&id), false, BUDGET, (true, false));
    read.follow_up(BUDGET);
    let note = read.adopt().expect("taken");
    assert!(note.complete && note.text.contains(THREAD), "{}", note.text);
    assert_eq!(
        lamp.offers(),
        (3, 1),
        "its last offer spent, and the thread recorded as given"
    );
}
