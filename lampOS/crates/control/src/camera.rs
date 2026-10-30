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

/// When the last new frames came, for `fps_now`: shared by every stream.
#[derive(Default)]
pub struct Rate(std::sync::Mutex<std::collections::VecDeque<std::time::Instant>>);

impl Rate {
    fn came(&self, at: std::time::Instant) {
        if let Ok(mut times) = self.0.lock() {
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
        let times = self.0.lock().ok()?;
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
}

impl Camera {
    pub fn new(sight: PathBuf, private: Private) -> Self {
        Self {
            sight,
            private,
            streams: HashMap::new(),
            mark: None,
            rate: Arc::default(),
        }
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
        std::thread::spawn(move || {
            let marked = |at: &mut Option<std::time::Instant>| {
                if at.is_none_or(|at| at.elapsed() >= MARK_EVERY) {
                    mark(&client, false);
                    *at = Some(std::time::Instant::now());
                }
            };
            run(&sight, fps, &running, &private, &marked, &rate, &|event| {
                events(&client, event)
            });
        });
        Ok(json!({"stream": "on", "fps": fps, "fps_now": self.rate.now()}))
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
            Ok(frame)
                if last != Some(frame.modified)
                    && sent_at.is_none_or(|at| at.elapsed() >= least_gap) =>
            {
                // The mark is on the ring before the picture leaves.
                marked(&mut marked_at);
                last = Some(frame.modified);
                let now = std::time::Instant::now();
                fresh_at = now;
                sent_at = Some(now);
                rate.came(now);
                let mut event = frame.json();
                event["ev"] = json!("camera.frame");
                event["fps_now"] = json!(rate.now().unwrap_or(0.0));
                tell(event);
            }
            Ok(_) => {
                marked(&mut marked_at);
                fresh_at = std::time::Instant::now();
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
        camera.stream("c_2", true, Some(5), events).unwrap();
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
    fn nothing_is_seen_while_private() {
        let sight = dir().join("sight.jpg");
        std::fs::write(&sight, JPEG).unwrap();
        let mut camera = Camera::new(sight, Arc::new(|| true)).marking(Arc::new(|_, _| {}));
        assert_eq!(camera.snapshot("c_1").unwrap_err().code, ErrorCode::Private);
        let events: Events = Arc::new(|_, _| {});
        assert_eq!(
            camera.stream("c_1", true, None, events).unwrap_err().code,
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
            camera.stream("c_1", true, Some(5), events).unwrap(),
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
        camera.stream("c_1", true, Some(5), events.clone()).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            camera.stream("c_1", false, None, events).unwrap(),
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
