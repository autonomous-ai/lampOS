//! The service's socket: one JSON line asked, one answered, then closed.
//!
//! - `{"op":"pose","client":"c_…","jpeg":"<base64>"}` →
//!   `{"ok":true,"pose":{"score","keypoints":[{"x","y","score"}…17]}|null,
//!   "width","height","model","ms":{"decode","npu","post","total"}}`, or
//!   `{"ok":false,"code","why","retry_ms"?}` with `code` one of `private`,
//!   `rate`, `busy`, `preparing`, `unavailable`, `bad_request`. Past
//!   [`MOST_AT_ONCE`] askers at once, the answer is `busy` before the line
//!   is read.
//! - `{"op":"status"}` → what the service has (`Service::status`).
//!
//! The socket is in the service's own runtime folder (0700, `lampos`'s), so
//! only lamp-control and the lamp's own parts reach it; an app reaches
//! `pose` only through lamp-control, whose grant check comes first.
use crate::service::{Conditions, Service};
use base64::{Engine, engine::general_purpose::STANDARD};
use lamp_msgs::{BoardHeat, HeatLevel};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// The longest line read: a JPEG of `MOST_JPEG_BYTES` in base64, and its
/// fields.
pub const MOST_LINE_BYTES: u64 = (crate::service::MOST_JPEG_BYTES as u64 * 4).div_ceil(3) + 1024;
/// How long one asker has to send its whole line, from its connection
/// (one deadline for the request, not each read: Heron's P3 on #7972), and
/// the longest a write of its answer may wait.
pub const WAIT: Duration = Duration::from_secs(5);
/// The most askers served at once; past that one is answered `busy` at
/// once and closed.
pub const MOST_AT_ONCE: usize = 8;
/// How long a `busy` answer tells the asker to wait.
pub const BUSY_RETRY: Duration = Duration::from_millis(100);
/// The most askers being told `busy` at once; past that one is closed.
const MOST_TOLD_BUSY: usize = 8;
/// The longest client name taken.
const MOST_CLIENT: usize = 64;

/// The answer to one line.
pub fn answer(
    service: &Arc<Service>,
    line: &str,
    now: Instant,
    conditions: &dyn Fn() -> Conditions,
) -> Value {
    let bad = |why: &str| json!({"ok": false, "code": "bad_request", "why": why});
    let Ok(asked) = serde_json::from_str::<Value>(line) else {
        return bad("not one JSON object");
    };
    match asked["op"].as_str() {
        Some("status") => service.status(),
        Some(crate::service::POSE) => {
            let Some(client) = asked["client"]
                .as_str()
                .filter(|client| !client.is_empty() && client.len() <= MOST_CLIENT)
            else {
                return bad("client: the asker's id");
            };
            let Some(jpeg) = asked["jpeg"]
                .as_str()
                .and_then(|text| STANDARD.decode(text).ok())
            else {
                return bad("jpeg: a JPEG in base64");
            };
            match service.pose(client, &jpeg, now, conditions) {
                Ok(answer) => answer,
                Err(refused) => refused.json(),
            }
        }
        _ => bad("op: pose or status"),
    }
}

/// The lamp's privacy and the board's heat, from the bus under `root`:
/// private when the microphone's privacy lease says so (as lamp-control's
/// watch reads `voice/privacy`), or when the bus can't be read; the heat
/// ok when none is said or it has run out (`BoardHeat`'s own rule).
pub fn conditions(root: &Path) -> Conditions {
    let Ok(bus) = lamp_bus::Bus::new(root) else {
        return Conditions {
            private: true,
            heat: HeatLevel::Ok,
        };
    };
    let private = match bus.latest(lamp_msgs::MicrophonePrivacy::TOPIC) {
        Ok(None) => false,
        Ok(Some(message)) => serde_json::from_slice::<Value>(&message.payload)
            .map_or(true, |payload| {
                payload["permitted"] == false || payload["physical_muted"] == true
            }),
        Err(_) => true,
    };
    let heat = match bus.latest(BoardHeat::TOPIC) {
        Ok(Some(message)) if !message.expired => {
            serde_json::from_slice::<BoardHeat>(&message.payload)
                .ok()
                .filter(|heat| heat.check().is_ok())
                .map_or(HeatLevel::Ok, |heat| heat.level)
        }
        _ => HeatLevel::Ok,
    };
    Conditions { private, heat }
}

/// A pose-using app installed under `apps`: the model is fetched now, not
/// at the app's first ask. A lamp with no such app fetches nothing. Whether
/// one is installed.
pub fn apps_tick(service: &Arc<Service>, apps: &Path, now: Instant) -> bool {
    let uses = crate::apps::any_uses(apps, crate::service::POSE);
    if uses {
        let _ = service.prepare(now);
    }
    uses
}

/// Serves `listener` until the process ends: each asker on a thread of its
/// own, at most [`MOST_AT_ONCE`]; past that each is answered `busy`.
pub fn serve(
    listener: UnixListener,
    service: Arc<Service>,
    conditions: Arc<dyn Fn() -> Conditions + Send + Sync>,
) {
    serve_within(listener, service, conditions, WAIT);
}

/// [`serve`], with `wait` for [`WAIT`].
pub fn serve_within(
    listener: UnixListener,
    service: Arc<Service>,
    conditions: Arc<dyn Fn() -> Conditions + Send + Sync>,
    wait: Duration,
) {
    let at_once = Arc::new(AtomicUsize::new(0));
    let told_busy = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if at_once.fetch_add(1, Ordering::SeqCst) >= MOST_AT_ONCE {
            at_once.fetch_sub(1, Ordering::SeqCst);
            if told_busy.fetch_add(1, Ordering::SeqCst) >= MOST_TOLD_BUSY {
                told_busy.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let told_busy = told_busy.clone();
            std::thread::spawn(move || {
                busy(stream, wait);
                told_busy.fetch_sub(1, Ordering::SeqCst);
            });
            continue;
        }
        let (service, conditions, at_once) = (service.clone(), conditions.clone(), at_once.clone());
        std::thread::spawn(move || {
            one(stream, &service, conditions.as_ref(), wait);
            at_once.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// The answer past [`MOST_AT_ONCE`].
pub fn busy_answer() -> Value {
    json!({
        "ok": false,
        "code": "busy",
        "why": "the NPU service is answering as many asks as it takes",
        "retry_ms": BUSY_RETRY.as_millis() as u64,
    })
}

/// Tells an asker past the limit `busy` before its line is read, then
/// reads and drops the line, so its own write is never cut short and it
/// reads the answer (Heron's P3 on #7972: it was closed unanswered).
fn busy(stream: UnixStream, wait: Duration) {
    let _ = stream.set_write_timeout(Some(wait));
    let mut writer = &stream;
    if writeln!(writer, "{}", busy_answer()).is_err() {
        return;
    }
    let mut line = String::new();
    let _ = BufReader::new(Within::new(&stream, wait).take(MOST_LINE_BYTES)).read_line(&mut line);
}

/// A stream read until one deadline, however its bytes come.
struct Within<'a> {
    stream: &'a UnixStream,
    until: Instant,
}

impl<'a> Within<'a> {
    fn new(stream: &'a UnixStream, wait: Duration) -> Self {
        Self {
            stream,
            until: Instant::now() + wait,
        }
    }
}

impl Read for Within<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        let mut stream = self.stream;
        stream.read(buf)
    }
}

fn one(
    stream: UnixStream,
    service: &Arc<Service>,
    conditions: &dyn Fn() -> Conditions,
    wait: Duration,
) {
    let _ = stream.set_write_timeout(Some(wait));
    let mut line = String::new();
    if BufReader::new(Within::new(&stream, wait).take(MOST_LINE_BYTES))
        .read_line(&mut line)
        .is_err()
        || !line.ends_with('\n')
    {
        return;
    }
    let reply = answer(service, line.trim_end(), Instant::now(), conditions);
    let mut writer = &stream;
    let _ = writeln!(writer, "{reply}");
}
