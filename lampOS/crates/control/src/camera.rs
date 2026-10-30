//! The camera through the door: `camera.snapshot` and `camera.stream`
//! (the `camera` grant, docs/control.md, "The camera").
//!
//! The door never opens the camera. The eyes own it, and with
//! `--sight-file` they keep their newest frame in one file (a 320-across
//! JPEG, about six a second) while the camera is open, and remove it when
//! it closes (`lamp-eyes`). The door reads that file: a snapshot is the
//! newest frame if it is fresh ([`FRESH`]), and a stream sends each new one
//! as a `camera.frame` event, [`FPS_MOST`] a second at most. So the eyes and
//! the voice go on as before, and nothing here holds the device.
//!
//! Never while the lamp is private: the privacy switch ends every stream at
//! once with a `camera.ended` event and refuses a snapshot, whatever the
//! file holds. The voice stops the eyes' camera then too, so the file goes,
//! but the door doesn't wait on that.
use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use serde_json::{Value, json};

use crate::parts::Events;
use crate::protocol::{Error, ErrorCode};

/// A frame older than this is not the camera's now: the camera has closed
/// or stalled.
pub const FRESH: Duration = Duration::from_secs(2);
/// The largest frame sent; the eyes' sight frame is about 20 KiB.
pub const FRAME_MAX: u64 = 256 * 1024;
/// A stream's most frames a second, and its default. The eyes give about one
/// or two new frames a second today, so a stream sends each new one as it
/// comes, never one twice, up to the rate asked.
pub const FPS_MOST: u8 = 30;
pub const FPS_DEFAULT: u8 = 2;
/// A stream with no fresh frame this long ends, saying so.
pub const GONE_AFTER: Duration = Duration::from_secs(10);
/// `fps_now` is the rate new frames came at over this long.
pub const RATE_OVER: Duration = Duration::from_secs(2);
/// How often a stream looks for a new frame, whatever its rate: new frames
/// go as they come, not on a clock of their own.
const LOOK_EVERY: Duration = Duration::from_millis(20);

/// The source's rate, for `fps_now`: when each new frame of the eyes was
/// first seen, whichever stream saw it, and the last one's time on the file,
/// so a frame two streams see counts once, and a slow stream's skipped
/// frames still count.
#[derive(Default)]
pub struct Rate(
    std::sync::Mutex<(
        std::collections::VecDeque<std::time::Instant>,
        Option<SystemTime>,
    )>,
);

impl Rate {
    /// A frame written at `modified` seen at `at`: counted once.
    fn saw(&self, modified: SystemTime, at: std::time::Instant) {
        if let Ok(mut held) = self.0.lock() {
            let (times, last) = &mut *held;
            if *last == Some(modified) {
                return;
            }
            *last = Some(modified);
            times.push_back(at);
            while times
                .front()
                .is_some_and(|first| at.duration_since(*first) > RATE_OVER)
            {
                times.pop_front();
            }
        }
    }

    /// New frames a second over the last [`RATE_OVER`], to a tenth; none
    /// before two were seen in it.
    pub fn now(&self) -> Option<f64> {
        let held = self.0.lock().ok()?;
        let times = &held.0;
        let fresh: Vec<_> = times
            .iter()
            .filter(|at| at.elapsed() <= RATE_OVER)
            .collect();
        let (first, last) = (fresh.first()?, fresh.last()?);
        let span = last.duration_since(**first).as_secs_f64();
        (fresh.len() >= 2 && span > 0.0)
            .then(|| (((fresh.len() - 1) as f64 / span) * 10.0).round() / 10.0)
    }
}

/// Whether the lamp is private now.
pub type Private = Arc<dyn Fn() -> bool + Send + Sync>;

/// Where the fast stream's binary frames go, once the server has given it.
pub type Sink = Arc<std::sync::OnceLock<crate::parts::Binary>>;
/// The fast stream's sizes: the camera's own 25 fps MJPEG modes, as the eyes
/// serve them (`lamp_eyes::tap`). The first is the default: a quarter of
/// the bytes, for a link over a tunnel.
pub const SIZES: [&str; 2] = ["640x480", "1280x720"];
/// A camera frame larger than this is not sent.
pub const CAMERA_FRAME_MAX: usize = 512 * 1024;
/// How long the fast stream waits to connect to the eyes, and for a frame
/// before it looks again.
const FAST_WAIT: Duration = Duration::from_millis(500);

/// A size asked for, if it's one of [`SIZES`], as width and height.
pub fn size_of(size: &str) -> Option<(u32, u32)> {
    if !SIZES.contains(&size) {
        return None;
    }
    let (width, height) = size.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

/// One fast frame as it goes on the WebSocket, binary: the header's length
/// (4 bytes, big-endian), the header (JSON: `ev` `camera.frame`, `bytes`,
/// `width`, `height`, `fps_now`), then the JPEG.
pub fn binary_frame(header: &Value, jpeg: &[u8]) -> Vec<u8> {
    let header = header.to_string().into_bytes();
    let length = u32::try_from(header.len())
        .unwrap_or(u32::MAX)
        .to_be_bytes();
    [&length[..], &header, jpeg].concat()
}

/// Asks the ring for its mark (`light/mark`, `lamp_msgs::RingMark`) for a
/// client: `blink` once for a snapshot, else held [`MARK_HOLDS`] ahead.
pub type Mark = Arc<dyn Fn(&str, bool) + Send + Sync>;
/// A stream's mark is asked again this often...
pub const MARK_EVERY: Duration = Duration::from_secs(1);
/// ...to hold this far ahead, so one lost renewal leaves no gap.
pub const MARK_HOLDS: Duration = Duration::from_secs(2);

/// The mark asked for on the bus, `lamp_msgs::RingMark` on `light/mark`, on
/// the boot's monotonic clock `clock_id`, with `publish` sending it.
pub fn mark_on(
    clock_id: String,
    publish: impl Fn(&lamp_msgs::RingMark) + Send + Sync + 'static,
) -> Mark {
    Arc::new(move |client: &str, blink: bool| {
        let now_us = lamp_ipc::monotonic_us();
        publish(&lamp_msgs::RingMark {
            claim: lamp_msgs::Claim {
                source: format!("control:{client}"),
                priority: lamp_msgs::Priority::Skill,
                clock_id: clock_id.clone(),
                until_us: now_us + u64::try_from(MARK_HOLDS.as_micros()).unwrap_or(u64::MAX),
            },
            kind: lamp_msgs::MarkKind::Camera,
            blink,
        });
    })
}

/// The newest frame: its JPEG and how old it is.
#[derive(Debug)]
pub struct Frame {
    pub jpeg: Vec<u8>,
    pub age: Duration,
    pub modified: SystemTime,
}

impl Frame {
    /// As a reply or an event carries it.
    pub fn json(&self) -> Value {
        json!({
            "jpeg": base64::engine::general_purpose::STANDARD.encode(&self.jpeg),
            "bytes": self.jpeg.len(),
            "age_ms": u64::try_from(self.age.as_millis()).unwrap_or(u64::MAX),
        })
    }
}

/// The newest frame in `path` if it is fresh at `now`, or why not.
pub fn newest(path: &Path, now: SystemTime) -> Result<Frame, String> {
    let meta = std::fs::metadata(path).map_err(|_| "the camera is closed".to_owned())?;
    if meta.len() > FRAME_MAX {
        return Err(format!("the frame is over {FRAME_MAX} bytes"));
    }
    let modified = meta
        .modified()
        .map_err(|_| "the frame has no time".to_owned())?;
    let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
    if age > FRESH {
        return Err("the camera is closed".to_owned());
    }
    let jpeg = std::fs::read(path).map_err(|_| "the camera is closed".to_owned())?;
    // A frame caught mid-write by the eyes is not whole.
    if !(jpeg.starts_with(&[0xff, 0xd8]) && jpeg.ends_with(&[0xff, 0xd9])) {
        return Err("the frame was being written; ask again".to_owned());
    }
    Ok(Frame {
        jpeg,
        age,
        modified,
    })
}

/// The camera as the door reads it: the eyes' sight file, whether the lamp
/// is private, and each client's stream.
pub struct Camera {
    sight: PathBuf,
    private: Private,
    streams: HashMap<String, Arc<AtomicBool>>,
    /// The ring's mark; none, no camera at all (it is never seen unmarked).
    mark: Option<Mark>,
    rate: Arc<Rate>,
    /// The eyes' frames socket and where its frames go: the fast stream.
    fast: Option<(PathBuf, Sink)>,
}

impl Camera {
    pub fn new(sight: PathBuf, private: Private) -> Self {
        Self {
            sight,
            private,
            streams: HashMap::new(),
            mark: None,
            rate: Arc::default(),
            fast: None,
        }
    }

    /// The fast stream: every frame the camera makes, from the eyes' frames
    /// socket, as binary frames through `sink`.
    pub fn fast(mut self, socket: PathBuf, sink: Sink) -> Self {
        self.fast = Some((socket, sink));
        self
    }

    /// The ring's mark while it is seen through.
    pub fn marking(mut self, mark: Mark) -> Self {
        self.mark = Some(mark);
        self
    }

    fn mark(&self) -> Result<&Mark, Error> {
        self.mark.as_ref().ok_or_else(|| {
            Error::new(
                ErrorCode::Refused,
                "the camera is not shown on the ring here, so it is not given",
            )
        })
    }

    /// `camera.snapshot`: the ring blinks its mark as it is taken.
    pub fn snapshot(&self, client: &str) -> Result<Value, Error> {
        if (self.private)() {
            return Err(Error::new(ErrorCode::Private, "the lamp is private"));
        }
        let mark = self.mark()?;
        let frame = newest(&self.sight, SystemTime::now())
            .map_err(|why| Error::new(ErrorCode::Refused, why))?;
        mark(client, true);
        Ok(frame.json())
    }

    /// `camera.stream`, on or off. On again while one runs takes the new
    /// rate.
    pub fn stream(
        &mut self,
        client: &str,
        on: bool,
        fps: Option<u8>,
        size: Option<&str>,
        events: Events,
    ) -> Result<Value, Error> {
        self.end(client);
        if !on {
            return Ok(json!({"stream": "off"}));
        }
        if (self.private)() {
            return Err(Error::new(ErrorCode::Private, "the lamp is private"));
        }
        let mark = self.mark()?.clone();
        let fps = fps.unwrap_or(FPS_DEFAULT).clamp(1, FPS_MOST);
        let running = Arc::new(AtomicBool::new(true));
        self.streams.insert(client.to_owned(), running.clone());
        let (sight, private, client) =
            (self.sight.clone(), self.private.clone(), client.to_owned());
        let rate = self.rate.clone();
        // The fast stream when the eyes serve their frames: every frame the
        // camera makes, at the size asked. Else the eyes' newest frame.
        let asked = size.unwrap_or(SIZES[0]);
        let fast = self.fast.as_ref().and_then(|(socket, sink)| {
            let sink = sink.get()?.clone();
            let size = size_of(asked)?;
            let mut reader = UnixStream::connect(socket).ok()?;
            reader.set_write_timeout(Some(FAST_WAIT)).ok()?;
            reader
                .write_all(format!("{}x{}\n", size.0, size.1).as_bytes())
                .ok()?;
            Some((reader, sink, size))
        });
        let fast_size = fast.as_ref().map(|(_, _, _)| asked.to_owned());
        std::thread::spawn(move || {
            let marked = |at: &mut Option<std::time::Instant>| {
                if at.is_none_or(|at| at.elapsed() >= MARK_EVERY) {
                    mark(&client, false);
                    *at = Some(std::time::Instant::now());
                }
            };
            match fast {
                Some((reader, sink, size)) => {
                    let send = |frame: &[u8]| sink(&client, frame);
                    run_fast(
                        reader,
                        size,
                        fps,
                        &running,
                        &private,
                        &marked,
                        &rate,
                        &send,
                        &|event| events(&client, event),
                    );
                }
                None => run(&sight, fps, &running, &private, &marked, &rate, &|event| {
                    events(&client, event)
                }),
            }
        });
        Ok(match fast_size {
            Some(size) => json!({
                "stream": "on", "fps": fps, "fps_now": self.rate.now(), "fast": true, "size": size,
            }),
            None => json!({"stream": "on", "fps": fps, "fps_now": self.rate.now()}),
        })
    }

    /// Ends `client`'s stream, if any, without an event: its own `off`,
    /// `stop` or going.
    pub fn end(&mut self, client: &str) {
        if let Some(running) = self.streams.remove(client) {
            running.store(false, Ordering::SeqCst);
        }
    }
}

/// One stream, on its own thread, until `running` is cleared, the lamp is
/// private or the camera has been gone [`GONE_AFTER`].
fn run(
    sight: &Path,
    fps: u8,
    running: &AtomicBool,
    private: &Private,
    marked: &dyn Fn(&mut Option<std::time::Instant>),
    rate: &Rate,
    tell: &dyn Fn(Value),
) {
    // At most `fps` a second: a new frame sooner than this after the last
    // one sent waits for the next look.
    let least_gap = Duration::from_millis(1_000 / u64::from(fps));
    let mut sent_at: Option<std::time::Instant> = None;
    let mut last: Option<SystemTime> = None;
    let mut fresh_at = std::time::Instant::now();
    let mut marked_at = None;
    while running.load(Ordering::SeqCst) {
        if private() {
            running.store(false, Ordering::SeqCst);
            tell(json!({"ev": "camera.ended", "why": "the lamp is private"}));
            return;
        }
        match newest(sight, SystemTime::now()) {
            Ok(frame) => {
                let now = std::time::Instant::now();
                // Every new frame of the eyes counts toward their rate,
                // sent here or not.
                rate.saw(frame.modified, now);
                // The mark is on the ring before the picture leaves.
                marked(&mut marked_at);
                fresh_at = now;
                if last != Some(frame.modified)
                    && sent_at.is_none_or(|at| at.elapsed() >= least_gap)
                {
                    last = Some(frame.modified);
                    sent_at = Some(now);
                    let mut event = frame.json();
                    event["ev"] = json!("camera.frame");
                    event["fps_now"] = json!(rate.now());
                    tell(event);
                }
            }
            Err(_) if fresh_at.elapsed() >= GONE_AFTER => {
                running.store(false, Ordering::SeqCst);
                tell(json!({"ev": "camera.ended", "why": "the camera is closed"}));
                return;
            }
            Err(_) => {}
        }
        std::thread::sleep(LOOK_EVERY);
    }
}

/// One fast stream, on its own thread: each frame the eyes pass on, at most
/// `fps` a second, as a binary frame through `send`, the mark asked before
/// each. Ends when `running` is cleared, the lamp turns private, the eyes
/// let it go (the camera closed), or no frame comes for [`GONE_AFTER`].
#[allow(clippy::too_many_arguments)]
fn run_fast(
    mut reader: UnixStream,
    (width, height): (u32, u32),
    fps: u8,
    running: &AtomicBool,
    private: &Private,
    marked: &dyn Fn(&mut Option<std::time::Instant>),
    rate: &Rate,
    send: &dyn Fn(&[u8]),
    tell: &dyn Fn(Value),
) {
    let _ = reader.set_read_timeout(Some(FAST_WAIT));
    let least_gap = Duration::from_millis(1_000 / u64::from(fps));
    let (mut sent_at, mut marked_at) = (None::<std::time::Instant>, None);
    let mut fresh_at = std::time::Instant::now();
    let ended = |why: &str| {
        running.store(false, Ordering::SeqCst);
        tell(json!({"ev": "camera.ended", "why": why}));
    };
    while running.load(Ordering::SeqCst) {
        if private() {
            return ended("the lamp is private");
        }
        let mut length = [0u8; 4];
        match reader.read_exact(&mut length) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if fresh_at.elapsed() >= GONE_AFTER {
                    return ended("the camera is closed");
                }
                continue;
            }
            Err(_) => return ended("the camera is closed"),
        }
        let length = u32::from_be_bytes(length) as usize;
        let mut jpeg = vec![0u8; length];
        if reader.read_exact(&mut jpeg).is_err() {
            return ended("the camera is closed");
        }
        let now = std::time::Instant::now();
        fresh_at = now;
        rate.saw(SystemTime::now(), now);
        if length > CAMERA_FRAME_MAX || sent_at.is_some_and(|at| at.elapsed() < least_gap) {
            continue;
        }
        // The mark is on the ring before the picture leaves.
        marked(&mut marked_at);
        sent_at = Some(now);
        let header = json!({
            "ev": "camera.frame", "bytes": length, "width": width, "height": height,
            "fps_now": rate.now(),
        });
        send(&binary_frame(&header, &jpeg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const JPEG: &[u8] = &[0xff, 0xd8, 1, 2, 3, 0xff, 0xd9];

    fn dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lamp-control-camera-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_snapshot_is_the_newest_whole_fresh_frame() {
        let sight = dir().join("sight.jpg");
        let camera = Camera::new(sight.clone(), Arc::new(|| false)).marking(Arc::new(|_, _| {}));
        assert_eq!(
            camera.snapshot("c_1").unwrap_err().why,
            "the camera is closed"
        );
        std::fs::write(&sight, JPEG).unwrap();
        let reply = camera.snapshot("c_1").unwrap();
        assert_eq!(reply["bytes"], 7);
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(reply["jpeg"].as_str().unwrap())
                .unwrap(),
            JPEG
        );
        // Caught mid-write.
        std::fs::write(&sight, &JPEG[..4]).unwrap();
        assert!(
            camera
                .snapshot("c_1")
                .unwrap_err()
                .why
                .contains("ask again")
        );
        // Old: the camera has closed.
        let later = SystemTime::now() + FRESH + Duration::from_secs(1);
        std::fs::write(&sight, JPEG).unwrap();
        assert_eq!(newest(&sight, later).unwrap_err(), "the camera is closed");
    }

    #[test]
    fn the_ring_marks_every_picture_and_with_no_mark_there_is_no_camera() {
        let sight = dir().join("sight.jpg");
        std::fs::write(&sight, JPEG).unwrap();
        let asked: Arc<Mutex<Vec<(String, bool)>>> = Arc::default();
        let into = asked.clone();
        let mut camera = Camera::new(sight.clone(), Arc::new(|| false)).marking(Arc::new(
            move |client: &str, blink| into.lock().unwrap().push((client.to_owned(), blink)),
        ));
        camera.snapshot("c_1").unwrap();
        assert_eq!(*asked.lock().unwrap(), [("c_1".to_owned(), true)]);
        let events: Events = Arc::new(|_, _| {});
        camera.stream("c_2", true, Some(5), None, events).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            asked.lock().unwrap().contains(&("c_2".to_owned(), false)),
            "held while it streams"
        );
        camera.end("c_2");
        // A camera nobody can see on the ring is no camera.
        let unmarked = Camera::new(sight, Arc::new(|| false));
        assert!(
            unmarked
                .snapshot("c_1")
                .unwrap_err()
                .why
                .contains("not shown on the ring")
        );
    }

    #[test]
    fn fps_now_is_the_eyes_rate_whoever_streams_and_however_slowly() {
        let sight = dir().join("sight.jpg");
        std::fs::write(&sight, JPEG).unwrap();
        let mut camera =
            Camera::new(sight.clone(), Arc::new(|| false)).marking(Arc::new(|_, _| {}));
        let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
        let into = seen.clone();
        let events: Events = Arc::new(move |_, event| into.lock().unwrap().push(event));
        // Before two frames were seen, none.
        assert_eq!(
            camera
                .stream("c_1", true, Some(30), None, events.clone())
                .unwrap()["fps_now"],
            Value::Null
        );
        // A second computer, and a slow one: neither doubles nor halves it.
        camera
            .stream("c_2", true, Some(1), None, events.clone())
            .unwrap();
        // The eyes write a new frame every 100 ms for 1.5 s: 10 a second.
        for _ in 0..15 {
            std::thread::sleep(Duration::from_millis(100));
            let next = sight.with_extension("new");
            std::fs::write(&next, JPEG).unwrap();
            std::fs::rename(&next, &sight).unwrap();
        }
        let fps = camera.rate.now().expect("measured");
        assert!((8.0..=12.0).contains(&fps), "{fps}");
        // The first frame each sent said none yet: null, never 0.
        let seen = seen.lock().unwrap();
        assert!(seen.iter().any(|event| event["fps_now"].is_null()));
        assert!(seen.iter().all(|event| event["fps_now"].is_null()
            || event["fps_now"].as_f64().is_some_and(|fps| fps > 0.0)));
        camera.end("c_1");
        camera.end("c_2");
    }

    #[test]
    fn the_fast_stream_sends_each_eyes_frame_as_a_binary_frame_at_the_size_asked() {
        let folder = dir();
        let (sight, socket) = (folder.join("sight.jpg"), folder.join("frames.sock"));
        std::fs::write(&sight, JPEG).unwrap();
        // The eyes' side: says nothing back, writes 10 frames 20 ms apart.
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let eyes = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = [0u8; 8];
            stream.read_exact(&mut asked).unwrap();
            for _ in 0..10 {
                stream
                    .write_all(&(JPEG.len() as u32).to_be_bytes())
                    .unwrap();
                stream.write_all(JPEG).unwrap();
                std::thread::sleep(Duration::from_millis(20));
            }
            String::from_utf8(asked.to_vec()).unwrap()
        });
        let got: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
        let into = got.clone();
        let sink: Sink = Arc::default();
        let _ = sink.set(Arc::new(move |client: &str, frame: &[u8]| {
            assert_eq!(client, "c_1");
            into.lock().unwrap().push(frame.to_vec());
        }));
        let mut camera = Camera::new(sight, Arc::new(|| false))
            .marking(Arc::new(|_, _| {}))
            .fast(socket, sink);
        let events: Events = Arc::new(|_, _| {});
        let answer = camera.stream("c_1", true, Some(30), None, events).unwrap();
        assert_eq!(
            (answer["fast"].as_bool(), answer["size"].as_str()),
            (Some(true), Some("640x480"))
        );
        assert_eq!(eyes.join().unwrap(), "640x480\n");
        std::thread::sleep(Duration::from_millis(200));
        camera.end("c_1");
        let got = got.lock().unwrap();
        assert!(got.len() >= 5, "{}", got.len());
        // Each: the header's length, the header, the JPEG.
        let frame = &got[0];
        let length = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
        let header: Value = serde_json::from_slice(&frame[4..4 + length]).unwrap();
        assert_eq!(
            (header["ev"].as_str(), header["width"].as_u64()),
            (Some("camera.frame"), Some(640))
        );
        assert_eq!(&frame[4 + length..], JPEG);
    }

    #[test]
    fn nothing_is_seen_while_private() {
        let sight = dir().join("sight.jpg");
        std::fs::write(&sight, JPEG).unwrap();
        let mut camera = Camera::new(sight, Arc::new(|| true)).marking(Arc::new(|_, _| {}));
        assert_eq!(camera.snapshot("c_1").unwrap_err().code, ErrorCode::Private);
        let events: Events = Arc::new(|_, _| {});
        assert_eq!(
            camera
                .stream("c_1", true, None, None, events)
                .unwrap_err()
                .code,
            ErrorCode::Private
        );
    }

    #[test]
    fn a_stream_sends_each_new_frame_once_and_ends_when_private() {
        let sight = dir().join("sight.jpg");
        std::fs::write(&sight, JPEG).unwrap();
        let private = Arc::new(AtomicBool::new(false));
        let watching = private.clone();
        let mut camera = Camera::new(
            sight.clone(),
            Arc::new(move || watching.load(Ordering::SeqCst)),
        )
        .marking(Arc::new(|_, _| {}));
        let seen: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let into = seen.clone();
        let events: Events = Arc::new(move |client, event| {
            into.lock().unwrap().push((client.to_owned(), event));
        });
        assert_eq!(
            camera.stream("c_1", true, Some(5), None, events).unwrap(),
            json!({"stream": "on", "fps": 5, "fps_now": null})
        );
        std::thread::sleep(Duration::from_millis(500));
        let frames = |seen: &Mutex<Vec<(String, Value)>>| {
            seen.lock()
                .unwrap()
                .iter()
                .filter(|(_, event)| event["ev"] == "camera.frame")
                .count()
        };
        // The same file, unchanged: one frame, not five.
        assert_eq!(frames(&seen), 1);
        private.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(500));
        let seen = seen.lock().unwrap();
        let last = &seen.last().unwrap().1;
        assert_eq!(last["ev"], "camera.ended");
        assert_eq!(last["why"], "the lamp is private");
        assert!(seen.iter().all(|(client, _)| client == "c_1"));
    }

    #[test]
    fn off_and_stop_end_a_stream_with_no_more_frames() {
        let sight = dir().join("sight.jpg");
        std::fs::write(&sight, JPEG).unwrap();
        let mut camera =
            Camera::new(sight.clone(), Arc::new(|| false)).marking(Arc::new(|_, _| {}));
        let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
        let into = seen.clone();
        let events: Events = Arc::new(move |_, event| into.lock().unwrap().push(event));
        camera
            .stream("c_1", true, Some(5), None, events.clone())
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            camera.stream("c_1", false, None, None, events).unwrap(),
            json!({"stream": "off"})
        );
        let before = seen.lock().unwrap().len();
        // A new frame after off is never sent.
        std::thread::sleep(Duration::from_millis(50));
        std::fs::write(&sight, JPEG).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(seen.lock().unwrap().len(), before);
    }
}
