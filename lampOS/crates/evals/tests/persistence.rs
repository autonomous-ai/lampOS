//! Private synthetic host fixtures; no device, producer or runtime is enabled.
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use lamp_evals::persistence::{Completion, Exit, Outcome, Persistence, Poll, Refusal};
use lamp_evals::store::{FILE, Store};
use lamp_evals::{Build, Contract, Ingest, Observation, Origin, Recorder, SourceTime, Terminal};

const END: u64 = 500_000;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Folder(PathBuf);
impl Folder {
    fn new() -> Self {
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "lamp-evals-persistence-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
                    return Self(path);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("synthetic directory creation failed: {:?}", error.kind()),
            }
        }
        panic!("no unused fixture directory");
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn observation(turn: u64) -> Observation {
    Observation {
        epoch: 7,
        turn,
        time: SourceTime::UtcMinute((END - 1) * 60),
        build: Some(Build::parse("17001+abcdef123456").unwrap()),
        origin: Origin::Microphone,
        terminal: Terminal::Completed,
        audio_started: Some(true),
        endpoint_to_first_write_ms: Some(700),
        contract: Contract::VoiceDeliveryV1,
    }
}

fn fixture() -> Recorder {
    let mut recorder = Recorder::new(END).unwrap();
    assert_eq!(recorder.observe(observation(1)), Ingest::Accepted);
    recorder
}

fn until(owner: &mut Persistence, predicate: impl Fn(&Poll) -> bool) -> Poll {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let result = owner.poll();
        if predicate(&result) {
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "worker did not reach the test boundary"
        );
        thread::yield_now();
    }
}

#[test]
fn real_store_saves_a_private_atomic_checkpoint_and_retains_its_lock_until_exit() {
    let folder = Folder::new();
    let store = Store::open(&folder.0).unwrap();
    assert!(store.load().unwrap().is_none());
    let mut live = fixture();
    let original = live.checkpoint().unwrap();
    let mut owner = Persistence::start(store).unwrap();
    let first = owner.try_checkpoint(live.clone()).unwrap();
    assert_eq!(live.observe(observation(2)), Ingest::Accepted);
    let result = until(&mut owner, |p| p.completion.is_some());
    assert_eq!(
        result.completion,
        Some(Completion {
            ticket: first,
            outcome: Outcome::Saved
        })
    );
    assert_eq!(result.status.exit, Exit::Running);
    assert!(Store::open(&folder.0).is_err());
    assert_eq!(
        fs::metadata(folder.0.join(FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 1);

    // A saved older snapshot never mutates the live recorder or absorbs turn 2.
    owner.request_stop();
    let refused = owner.try_checkpoint(live.clone()).unwrap_err();
    assert_eq!(refused.reason, Refusal::Closed);
    assert_eq!(
        refused.recorder.checkpoint().unwrap(),
        live.checkpoint().unwrap()
    );
    let ended = until(&mut owner, |p| p.status.exit != Exit::Running);
    assert_eq!(ended.status.exit, Exit::Confirmed);
    assert_eq!(ended.status.last_confirmed, Some(first));
    let restored_store = Store::open(&folder.0).unwrap();
    let mut restored = restored_store.load().unwrap().unwrap();
    assert_eq!(restored.checkpoint().unwrap(), original);
    assert_eq!(restored.observe(observation(1)), Ingest::Duplicate);
    assert_eq!(restored.observe(observation(2)), Ingest::Accepted);
}

#[test]
fn ordered_checkpoints_round_trip_receipts_retention_and_dedupe_together() {
    let folder = Folder::new();
    let mut owner = Persistence::start(Store::open(&folder.0).unwrap()).unwrap();
    let mut live = fixture();
    let first = owner.try_checkpoint(live.clone()).unwrap();
    until(&mut owner, |p| p.completion.is_some());
    assert_eq!(live.observe(observation(1)), Ingest::Duplicate);
    assert_eq!(live.observe(observation(3)), Ingest::Accepted);
    live.advance_window(END + 1).unwrap();
    let second = owner.try_checkpoint(live.clone()).unwrap();
    assert!(second > first);
    assert_eq!(owner.status().last_confirmed, Some(first));
    owner.request_stop();
    let complete = until(&mut owner, |p| p.completion.is_some());
    assert_eq!(complete.completion.unwrap().outcome, Outcome::Saved);
    until(&mut owner, |p| p.status.exit != Exit::Running);
    let store = Store::open(&folder.0).unwrap();
    let restored = store.load().unwrap().unwrap();
    assert_eq!(restored.checkpoint().unwrap(), live.checkpoint().unwrap());
    assert_eq!(restored.snapshot().receipts.accepted, 2);
    assert_eq!(restored.snapshot().receipts.duplicates, 1);
}

fn near_capacity() -> Recorder {
    let mut recorder = Recorder::new(END).unwrap();
    for index in 0..4096 {
        let mut record = observation(index / 1024);
        record.epoch = index % 1024;
        record.build = Some(Build::parse(&format!("{}+abcdef123456", index + 1)).unwrap());
        assert_eq!(recorder.observe(record), Ingest::Accepted);
    }
    recorder
}

struct Times {
    first_us: u128,
    median_us: u128,
    p95_us: u128,
    p99_us: u128,
    max_us: u128,
}

impl std::fmt::Display for Times {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "[unit=us first={} median={} p95={} p99={} max={}]",
            self.first_us, self.median_us, self.p95_us, self.p99_us, self.max_us
        )
    }
}

fn times(first: Duration, repeated: &[Duration]) -> Times {
    let mut values: Vec<_> = repeated.iter().map(Duration::as_micros).collect();
    values.sort_unstable();
    let percentile = |percent: usize| values[(values.len() * percent).div_ceil(100) - 1];
    Times {
        first_us: first.as_micros(),
        median_us: percentile(50),
        p95_us: percentile(95),
        p99_us: percentile(99),
        max_us: *values.last().unwrap(),
    }
}

fn measured<T>(by: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let result = by();
    (result, start.elapsed())
}

/// Run explicitly with --ignored --nocapture. No timing assertion qualifies a
/// scheduler or flash device. Capture, offer, each poll call and direct Store
/// save are distinct boundaries. Completion wait/yield time is not poll CPU time.
#[test]
#[ignore = "explicit host timing, synthetic fixtures, no hardware guarantee"]
#[allow(clippy::print_stdout)]
fn measure_capture_offer_poll_and_full_store_save() {
    for (label, recorder) in [("small", fixture()), ("near_capacity", near_capacity())] {
        let bytes = recorder.checkpoint().unwrap().len();
        let folder = Folder::new();
        let mut owner = Persistence::start(Store::open(&folder.0).unwrap()).unwrap();
        let mut captures = Vec::new();
        let mut offers = Vec::new();
        let mut polls = Vec::new();
        let mut first_poll = None;
        let mut returned_polls = Vec::new();
        for _ in 0..32 {
            let (copy, elapsed) = measured(|| recorder.clone());
            captures.push(elapsed);
            let (ticket, elapsed) = measured(|| owner.try_checkpoint(copy).unwrap());
            offers.push(elapsed);
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let (result, elapsed) = measured(|| owner.poll());
                first_poll.get_or_insert(elapsed);
                if polls.len() < 100_000 {
                    polls.push(elapsed);
                }
                if let Some(completion) = result.completion {
                    assert_eq!(completion.ticket, ticket);
                    assert_eq!(completion.outcome, Outcome::Saved);
                    returned_polls.push(elapsed);
                    break;
                }
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
        }
        owner.request_stop();
        until(&mut owner, |p| p.status.exit != Exit::Running);
        let save_folder = Folder::new();
        let mut store = Store::open(&save_folder.0).unwrap();
        let mut saves = Vec::new();
        for _ in 0..32 {
            let (saved, elapsed) = measured(|| store.save(&recorder));
            saved.unwrap();
            saves.push(elapsed);
        }
        println!(
            "{label} checkpoint_bytes={bytes} capture={} offer={} poll={} completed_poll={} full_store_save={}",
            times(captures[0], &captures[1..]),
            times(offers[0], &offers[1..]),
            times(first_poll.unwrap(), &polls),
            times(returned_polls[0], &returned_polls[1..]),
            times(saves[0], &saves[1..])
        );
    }
}
