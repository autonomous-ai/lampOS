//! The waking face's hand-over (#1681), with a real lock: the ring is a file
//! locked with `try_lock`, as `lamp_ring::linux` locks the device, so the
//! voice's open fails exactly when the waking face still holds it.
use lamp_ring::{
    EncodedFrame, FrameSink,
    waking::{AT_ONCE_WAIT, Ask, LetGo, Lock, RELEASE_WAIT, WakingRing, let_go_at, take_at},
};
use lamp_setup::wake::{self, Ended};
use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

type Frames = Arc<Mutex<Vec<EncodedFrame>>>;

/// The ring: frames kept, the device's lock held while open.
struct Locked {
    _device: File,
    frames: Frames,
}

impl FrameSink for Locked {
    fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
        self.frames.lock().unwrap().push(frame.clone());
        Ok(())
    }
}

fn lock(device: &Path) -> io::Result<File> {
    let file = OpenOptions::new().write(true).open(device)?;
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => io::Error::from(io::ErrorKind::WouldBlock),
        std::fs::TryLockError::Error(error) => error,
    })?;
    Ok(file)
}

struct Rig {
    socket: PathBuf,
    device: PathBuf,
    frames: Frames,
    painting: thread::JoinHandle<Ended>,
    started: Instant,
}

fn place(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lamp-wake-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The waking face on a fresh ring; the first `missing` opens find no device.
fn start(name: &str, missing: usize) -> Rig {
    let dir = place(name);
    let socket = dir.join("release.sock");
    let device = dir.join("spidev");
    File::create(&device).unwrap();
    let frames: Frames = Arc::default();
    let listener = UnixListener::bind(&socket).unwrap();
    let started = Instant::now();
    let painting = thread::spawn({
        let device = device.clone();
        let frames = Arc::clone(&frames);
        let mut missing = missing;
        move || {
            wake::run(
                listener,
                move || {
                    if missing > 0 {
                        missing -= 1;
                        return Err(io::Error::from(io::ErrorKind::NotFound));
                    }
                    WakingRing::new(Locked {
                        _device: lock(&device)?,
                        frames: Arc::clone(&frames),
                    })
                },
                |_| {},
                Duration::from_millis(20),
            )
        }
    });
    Rig {
        socket,
        device,
        frames,
        painting,
        started,
    }
}

fn lit(frames: &Frames) -> bool {
    frames
        .lock()
        .unwrap()
        .iter()
        .any(|frame| *frame != EncodedFrame::black())
}

fn wait_lit(rig: &Rig) -> Duration {
    while !lit(&rig.frames) {
        assert!(rig.started.elapsed() < Duration::from_secs(3), "never lit");
        thread::sleep(Duration::from_millis(1));
    }
    rig.started.elapsed()
}

#[test]
fn it_lights_at_once_and_the_voice_finds_the_ring_free_once_answered() {
    let rig = start("fade", 0);
    let first = wait_lit(&rig);
    assert!(first < Duration::from_millis(300), "{first:?}");
    // Faded in: while it shows, the voice's lock fails, as on the lamp.
    thread::sleep(wake::FADE_IN);
    assert_eq!(
        lock(&rig.device).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    let asked = Instant::now();
    let outcome = let_go_at(&rig.socket, Ask::Fade, RELEASE_WAIT);
    assert!(matches!(outcome, LetGo::Released), "{outcome:?}");
    // The voice opens next: the lock is free the moment it is answered.
    lock(&rig.device).expect("the voice's open");
    let took = asked.elapsed();
    assert!(took < Duration::from_secs(1), "{took:?}");
    assert!(took >= wake::FADE_OUT - wake::FRAME, "it faded: {took:?}");
    assert_eq!(
        *rig.frames.lock().unwrap().last().unwrap(),
        EncodedFrame::black()
    );
    assert_eq!(rig.painting.join().unwrap(), Ended::Released);
}

#[test]
fn the_privacy_ring_has_it_within_100_ms_of_asking() {
    let rig = start("at-once", 0);
    wait_lit(&rig);
    thread::sleep(Duration::from_millis(300));
    let asked = Instant::now();
    let outcome = let_go_at(&rig.socket, Ask::AtOnce, RELEASE_WAIT);
    assert!(matches!(outcome, LetGo::Released), "{outcome:?}");
    lock(&rig.device).expect("the privacy ring's open");
    let took = asked.elapsed();
    assert!(took < Duration::from_millis(100), "{took:?}");
    assert_eq!(
        *rig.frames.lock().unwrap().last().unwrap(),
        EncodedFrame::black()
    );
    assert_eq!(rig.painting.join().unwrap(), Ended::Released);
}

#[test]
fn a_ring_that_is_not_there_yet_is_tried_again_and_shown() {
    let rig = start("late", 3);
    let first = wait_lit(&rig);
    assert!(first >= Duration::from_millis(60), "{first:?}");
    let outcome = let_go_at(&rig.socket, Ask::Fade, RELEASE_WAIT);
    assert!(matches!(outcome, LetGo::Released), "{outcome:?}");
    lock(&rig.device).expect("the voice's open");
    assert_eq!(rig.painting.join().unwrap(), Ended::Released);
}

#[test]
fn asked_before_any_ring_opened_it_answers_at_once_and_never_opens_one() {
    let rig = start("never", usize::MAX);
    thread::sleep(Duration::from_millis(100));
    let asked = Instant::now();
    let outcome = let_go_at(&rig.socket, Ask::Fade, RELEASE_WAIT);
    assert!(matches!(outcome, LetGo::Released), "{outcome:?}");
    assert!(asked.elapsed() < Duration::from_millis(100));
    lock(&rig.device).expect("the voice's open");
    assert!(rig.frames.lock().unwrap().is_empty());
    assert_eq!(rig.painting.join().unwrap(), Ended::Released);
}

#[test]
fn something_that_is_not_the_voices_ask_leaves_the_ring_lit() {
    use std::io::Write;
    let rig = start("stranger", 0);
    wait_lit(&rig);
    let mut stranger = std::os::unix::net::UnixStream::connect(&rig.socket).unwrap();
    stranger.write_all(b"hello\n").unwrap();
    drop(stranger);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(
        lock(&rig.device).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    let outcome = let_go_at(&rig.socket, Ask::AtOnce, RELEASE_WAIT);
    assert!(matches!(outcome, LetGo::Released), "{outcome:?}");
    assert_eq!(rig.painting.join().unwrap(), Ended::Released);
}

#[test]
fn a_waking_face_that_never_answers_cannot_keep_the_privacy_ring_dark() {
    // The face lights, then its next write never returns: alive, holding
    // the ring's lock, and never answering.
    let dir = place("stuck");
    let socket = dir.join("release.sock");
    let device = dir.join("spidev");
    File::create(&device).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    let frames: Frames = Arc::default();
    let (unstick, stuck) = mpsc::channel::<()>();
    let stuck = Arc::new(Mutex::new(stuck));
    struct Stuck {
        _device: File,
        frames: Frames,
        stuck: Arc<Mutex<mpsc::Receiver<()>>>,
    }
    impl FrameSink for Stuck {
        fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
            if lit(&self.frames) {
                let _ = self.stuck.lock().unwrap().recv();
            }
            self.frames.lock().unwrap().push(frame.clone());
            Ok(())
        }
    }
    let painting = thread::spawn({
        let device = device.clone();
        let frames = Arc::clone(&frames);
        move || {
            wake::run(
                listener,
                move || {
                    WakingRing::new(Stuck {
                        _device: lock(&device)?,
                        frames: Arc::clone(&frames),
                        stuck: Arc::clone(&stuck),
                    })
                },
                |_| {},
                Duration::from_millis(20),
            )
        }
    });
    let started = Instant::now();
    while !lit(&frames) {
        assert!(started.elapsed() < Duration::from_secs(3), "never lit");
        thread::sleep(Duration::from_millis(1));
    }
    thread::sleep(Duration::from_millis(100));
    assert_eq!(lock(&device).unwrap_err().kind(), io::ErrorKind::WouldBlock);

    // The privacy ring: asked at once, it is not answered, and takes the
    // device all the same.
    let asked = Instant::now();
    let mut tried = Vec::new();
    let mut privacy = take_at(&socket, |lock_it| {
        tried.push(lock_it);
        match lock_it {
            Lock::Locked => lock(&device),
            Lock::Unlocked => OpenOptions::new().write(true).open(&device),
        }
    })
    .expect("the privacy ring's open");
    io::Write::write_all(&mut privacy, b"red").unwrap();
    let took = asked.elapsed();
    assert_eq!(tried, [Lock::Locked, Lock::Unlocked]);
    assert!(
        took >= AT_ONCE_WAIT && took < AT_ONCE_WAIT + Duration::from_millis(50),
        "{took:?}"
    );
    assert_eq!(std::fs::read(&device).unwrap(), b"red");
    // Once the face's write returns, past the ask's 100 ms, only the frame
    // it was stuck in lands: it closes the ring with no black of its own,
    // so nothing more of it falls on the mute light.
    let painted = frames.lock().unwrap().len();
    drop(unstick);
    assert_eq!(painting.join().unwrap(), Ended::Released);
    let frames = frames.lock().unwrap();
    assert_eq!(frames.len(), painted + 1, "{:?}", &frames[painted..]);
    assert_ne!(frames[painted], EncodedFrame::black());
}
