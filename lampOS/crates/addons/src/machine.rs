//! `lamp-addons machine` (devin-0-P3, `docs/addons.md`, "The person's own
//! computer"): add-ons from an agent on the person's own computer, through
//! the lamp's own Harness daemon (openharness#1717). The lamp is itself a
//! signed-in Harness machine, so its add-on service talks to its own daemon
//! over the loopback, with the daemon's hook credential:
//!
//! - `hello` at start, which clears what was published before;
//! - `publish` of the summary topics ([`lamp_msgs::ADDON_TOPICS`]) while the
//!   person's yes holds, at most one batch a second;
//! - `proposals`, a long take of the proposals the owner's machines made,
//!   each written on `addon/proposal` stamped `own-<computer>` for the judge;
//! - `decision`, the judge's kept or dropped sent back.
//!
//! Nothing here reaches a speaker, a light or a motor: a proposal is only
//! ever put to the judge, which decides as it does for an add-on on the lamp.
use crate::stream::{Allowed, Stream};
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_msgs::{
    ADDON_HOLDS_MS, ADDON_NOTE_BYTES, ADDON_TOPICS, ADDON_WHYS, AddonDecision, AddonLight,
    AddonProposal,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// The service's switch, off unless set.
pub const ENVIRONMENT: &str = "LAMP_MACHINE_ADDONS";

/// The bus source its proposals carry, the only one besides `skills` the
/// judge takes proposals from (`crate::runtime`).
pub const SOURCE: &str = "machine_addons";

/// The stamp of the person's own computer: `own-` and 12 hex digits.
pub const OWN: &str = "own-";

/// The contract's version, which a proposal from a computer carries in
/// place of an installed package's.
pub const CONTRACT_VERSION: &str = "1.0.0";

/// Where the person's yes is kept: the machine ids of the computers they
/// said are their own. Nothing writes it yet; how the person says the yes
/// is open (`docs/addons.md`, "Open for the trusted tier").
pub const YES_FILE: &str = "/var/lib/lampOS/addons/machine-yes.json";

/// Harness's own default port (openharness `cli/src/config/env.ts`).
pub const HARNESS_PORT: u16 = 18_473;

/// The acts a computer may propose.
pub const ACTS: [&str; 2] = ["light", "say"];

/// The stream's own switch (`crate::stream`), off unless set: without it,
/// only the summaries are published, whatever the yes says.
pub const STREAM_ENVIRONMENT: &str = "LAMP_MACHINE_ADDONS_STREAM";

/// How often the bus is looked at again for topics to follow.
const LOOK_EVERY: Duration = Duration::from_secs(30);

/// The longest one take waits for the next proposal (Harness's own bound
/// is 20 s); a stop is seen within it.
const TAKE_WAIT_MS: u64 = 10_000;
/// Harness counts a device's requests against one a second.
const PUBLISH_EVERY: Duration = Duration::from_secs(1);
/// After the daemon was not reached.
const RETRY: Duration = Duration::from_secs(5);
/// How long one pass waits for a bus message.
const PASS: Duration = Duration::from_millis(100);
/// A decision the judge has not made by its hold and this is let go;
/// Harness has expired the proposal by then.
const DECISION_SLACK: Duration = Duration::from_secs(5);
/// Harness keeps at most four undecided proposals a device.
const MOST_WAITING: usize = 4;
/// A value Harness takes (`addon.publish`).
const VALUE_BYTES: usize = 4_096;
/// The yes file, read whole.
const YES_BYTES: u64 = 4_096;
/// An answer from the daemon; the largest is a take of 32 proposals.
const ANSWER_BYTES: usize = 256 * 1024;
const CONNECT: Duration = Duration::from_secs(2);

/// How it runs.
#[derive(Clone, Debug)]
pub struct Config {
    pub bus_root: Option<PathBuf>,
    pub yes_file: PathBuf,
    /// Harness's data folder, which holds its `hook-credential`.
    pub data_dir: PathBuf,
    pub port: u16,
    pub run_for: Option<Duration>,
    /// The stream to the computer (`crate::stream`), with the yes for it.
    pub stream: bool,
}

impl Config {
    /// The product's: Harness's folder as Harness names it
    /// (`ADAPTER_DATA_DIR`, else `~/.harness/cli/data`) and its port
    /// (`PORT`, else 18473).
    pub fn from_environment() -> Self {
        let data_dir = std::env::var_os("ADAPTER_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| {
                    PathBuf::from(home)
                        .join(".harness")
                        .join("cli")
                        .join("data")
                })
            })
            .unwrap_or_default();
        let port = std::env::var("PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .filter(|port| *port != 0)
            .unwrap_or(HARNESS_PORT);
        Self {
            bus_root: None,
            yes_file: PathBuf::from(YES_FILE),
            data_dir,
            port,
            run_for: None,
            stream: std::env::var(STREAM_ENVIRONMENT).as_deref() == Ok("on"),
        }
    }
}

/// What it did, said when it stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub proposed: u64,
    pub refused: u64,
    pub decided: u64,
    pub published: u64,
    /// Lines made for the stream.
    pub lines: u64,
}

/// The stamp of a computer: `own-` and the first 12 hex digits of the
/// SHA-256 of its machine id, so the lamp's journal never holds the id.
pub fn stamp(machine_id: &str) -> String {
    let digest = Sha256::digest(machine_id.as_bytes());
    let hex: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("{OWN}{hex}")
}

/// The person's yes: the machine ids they said are their own computers.
/// A file missing, unreadable or not as written is no yes.
pub fn yes(path: &Path) -> BTreeSet<String> {
    let read = || -> Option<BTreeSet<String>> {
        let value = yes_file(path)?;
        let machines = value.as_object()?.get("machines")?.as_array()?;
        machines
            .iter()
            .map(|id| id.as_str().filter(|id| machine_id(id)).map(str::to_owned))
            .collect()
    };
    read().unwrap_or_default()
}

/// The person's yes for the stream: `"stream": true` beside their
/// computers, and what else it covers (`"words"`, `"busy"`). None, unless
/// all of it is as written and some computer has the yes.
pub fn yes_to_stream(path: &Path) -> Option<Allowed> {
    if yes(path).is_empty() {
        return None;
    }
    let value = yes_file(path)?;
    let flag = |name: &str| value.get(name).and_then(Value::as_bool);
    (flag("stream") == Some(true)).then(|| Allowed {
        words: flag("words") == Some(true),
        busy: flag("busy") == Some(true),
    })
}

fn yes_file(path: &Path) -> Option<Value> {
    let file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    file.take(YES_BYTES + 1).read_to_string(&mut text).ok()?;
    if text.len() as u64 > YES_BYTES {
        return None;
    }
    serde_json::from_str(&text).ok()
}

/// A machine id as Harness names one.
fn machine_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

/// A proposal taken from Harness, ready for the bus.
#[derive(Clone, Debug, PartialEq)]
pub struct Taken {
    /// Harness's id, to send the decision back.
    pub proposal_id: String,
    pub machine_id: String,
    pub proposal: AddonProposal,
}

/// Why a proposal from a computer is not put to the judge: the words sent
/// back as the decision's reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    /// Harness's id, when the proposal had one to answer.
    pub proposal_id: Option<String>,
    pub reason: &'static str,
}

/// One proposal as Harness gives it (`addon.proposal`'s payload, with its
/// `from`), checked and made an [`AddonProposal`] numbered `number`.
/// `now_unix_ms` is the lamp's wall clock, against Harness's `expiresAt`.
pub fn convert(
    payload: &Value,
    yes: &BTreeSet<String>,
    number: u64,
    now_unix_ms: u64,
) -> Result<Taken, Refused> {
    let id = payload
        .get("proposalId")
        .and_then(Value::as_str)
        .filter(|id| uuid(id))
        .map(str::to_owned);
    let refuse = |reason| Refused {
        proposal_id: id.clone(),
        reason,
    };
    let Some(proposal_id) = id.clone() else {
        return Err(refuse("not well formed"));
    };
    let machine_id = payload
        .get("from")
        .and_then(|from| from.get("machineId"))
        .and_then(Value::as_str)
        .filter(|id| machine_id(id))
        .ok_or_else(|| refuse("no computer said it came from"))?;
    if !yes.contains(machine_id) {
        return Err(refuse("no yes for this computer"));
    }
    if payload
        .get("expiresAt")
        .and_then(Value::as_u64)
        .is_none_or(|expires| expires <= now_unix_ms)
    {
        return Err(refuse("arrived after it expired"));
    }
    let act = payload.get("act").and_then(Value::as_str).unwrap_or("");
    if !ACTS.contains(&act) {
        return Err(refuse("not_for_remote: only light and say"));
    }
    let why = payload.get("why").and_then(Value::as_str).unwrap_or("");
    if !ADDON_WHYS.contains(&why) {
        return Err(refuse("why is not one of the lamp's reasons"));
    }
    let sure = payload.get("sure").and_then(Value::as_f64).unwrap_or(0.5);
    let holds_ms = payload
        .get("holdsMs")
        .and_then(Value::as_u64)
        .and_then(|ms| u32::try_from(ms).ok())
        .filter(|ms| ADDON_HOLDS_MS.contains(ms))
        .ok_or_else(|| refuse("holds 1 s to 30 s"))?;
    let params = payload.get("params");
    let note = payload.get("note").and_then(Value::as_str);
    let (light, note) = match act {
        "light" => {
            let light = params
                .and_then(|params| serde_json::from_value::<AddonLight>(params.clone()).ok())
                .ok_or_else(|| refuse("a light names a colour and a level"))?;
            if note.is_some() {
                return Err(refuse("a light carries no note"));
            }
            (Some(light), None)
        }
        _ => {
            if params.is_some_and(|params| params.as_object().is_none_or(|p| !p.is_empty())) {
                return Err(refuse("not_for_partner: a say carries no parts"));
            }
            let note = note.ok_or_else(|| refuse("a say carries a note"))?;
            if note.len() > ADDON_NOTE_BYTES {
                return Err(refuse("a note is at most 300 bytes"));
            }
            // Harness lets a newline through; never rewritten to pass.
            if note.chars().any(char::is_control) {
                return Err(refuse("note_not_one_line"));
            }
            (None, Some(note.to_owned()))
        }
    };
    let proposal = AddonProposal {
        proposal: number,
        addon: stamp(machine_id),
        version: CONTRACT_VERSION.into(),
        act: act.into(),
        why: why.into(),
        sure: sure as f32,
        holds_ms,
        light,
        gesture: None,
        mood: None,
        note,
    };
    proposal
        .check()
        .map_err(|_| refuse("not as the lamp takes a proposal"))?;
    Ok(Taken {
        proposal_id,
        machine_id: machine_id.to_owned(),
        proposal,
    })
}

fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(at, b)| match at {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// Harness's add-on routes for a device that is this machine.
pub trait Harness: Send + Sync {
    fn hello(&self) -> io::Result<Value>;
    fn publish(&self, values: Vec<Value>) -> io::Result<Value>;
    fn decision(&self, proposal_id: &str, kept: bool, reason: &str) -> io::Result<Value>;
    fn proposals(&self, after: u64, wait_ms: u64, instance: Option<&str>) -> io::Result<Value>;
}

/// The daemon over this machine's loopback, with its hook credential.
pub struct Loopback {
    port: u16,
    credential: String,
}

impl Loopback {
    /// The credential is read as Harness reads it: a regular file of this
    /// user's, mode 0600, holding one 43-character token.
    pub fn new(data_dir: &Path, port: u16) -> io::Result<Self> {
        Ok(Self {
            port,
            credential: credential(&data_dir.join("hook-credential"))?,
        })
    }

    fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        wait: Duration,
    ) -> io::Result<Value> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let mut stream = TcpStream::connect_timeout(&address, CONNECT)?;
        stream.set_read_timeout(Some(wait + Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(CONNECT))?;
        let body = body.map(Value::to_string).unwrap_or_default();
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n",
            self.port, self.credential
        );
        if method == "POST" {
            request.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            ));
        }
        request.push_str("\r\n");
        request.push_str(&body);
        stream.write_all(request.as_bytes())?;
        let mut answer = Vec::new();
        stream
            .take(ANSWER_BYTES as u64 + 1)
            .read_to_end(&mut answer)?;
        if answer.len() > ANSWER_BYTES {
            return Err(io::Error::other("the daemon's answer is too long"));
        }
        let (status, body) = response(&answer)?;
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| io::Error::other(format!("the daemon answered {status}, not JSON")))?;
        if status != 200 {
            let code = value
                .pointer("/error/code")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(io::Error::other(format!(
                "the daemon answered {status} {code}"
            )));
        }
        Ok(value)
    }
}

impl Harness for Loopback {
    fn hello(&self) -> io::Result<Value> {
        self.call(
            "POST",
            "/api/autonomous-device/machine-addon/hello",
            Some(&json!({})),
            Duration::ZERO,
        )
    }

    fn publish(&self, values: Vec<Value>) -> io::Result<Value> {
        self.call(
            "POST",
            "/api/autonomous-device/machine-addon/publish",
            Some(&json!({ "values": values })),
            Duration::ZERO,
        )
    }

    fn decision(&self, proposal_id: &str, kept: bool, reason: &str) -> io::Result<Value> {
        let reason: String = reason
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        let mut body = json!({
            "proposalId": proposal_id,
            "decision": if kept { "kept" } else { "dropped" },
        });
        if !reason.is_empty() {
            body["reason"] = Value::String(reason);
        }
        self.call(
            "POST",
            "/api/autonomous-device/machine-addon/decision",
            Some(&body),
            Duration::ZERO,
        )
    }

    fn proposals(&self, after: u64, wait_ms: u64, instance: Option<&str>) -> io::Result<Value> {
        let mut path = format!(
            "/api/autonomous-device/machine-addon/proposals?after={after}&waitMs={wait_ms}"
        );
        if let Some(instance) = instance.filter(|id| uuid(id)) {
            path.push_str(&format!("&instance={instance}"));
        }
        self.call("GET", &path, None, Duration::from_millis(wait_ms))
    }
}

fn credential(path: &Path) -> io::Result<String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)
        .map_err(|error| io::Error::other(format!("Harness's credential: {error}")))?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.len() > 128
        || meta.mode() & 0o777 != 0o600
        || meta.uid() != rustix::process::getuid().as_raw()
    {
        return Err(io::Error::other(
            "Harness's credential is not this user's own 0600 file",
        ));
    }
    let mut text = String::new();
    file.take(129).read_to_string(&mut text)?;
    let token = text.trim();
    if token.len() == 43
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Ok(token.to_owned())
    } else {
        Err(io::Error::other("Harness's credential is not a token"))
    }
}

/// An HTTP/1.1 answer's status and body, chunked or not.
fn response(bytes: &[u8]) -> io::Result<(u16, Vec<u8>)> {
    let bad = || io::Error::other("the daemon's answer is not HTTP");
    let split = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(bad)?;
    let head = std::str::from_utf8(&bytes[..split]).map_err(|_| bad())?;
    let body = &bytes[split + 4..];
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(bad)?;
    let chunked = lines.any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    if !chunked {
        return Ok((status, body.to_vec()));
    }
    let mut out = Vec::new();
    let mut rest = body;
    loop {
        let end = rest.windows(2).position(|w| w == b"\r\n").ok_or_else(bad)?;
        let size = std::str::from_utf8(&rest[..end])
            .ok()
            .and_then(|size| usize::from_str_radix(size.split(';').next()?.trim(), 16).ok())
            .ok_or_else(bad)?;
        rest = &rest[end + 2..];
        if size == 0 {
            return Ok((status, out));
        }
        if rest.len() < size + 2 {
            return Err(bad());
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

/// What the take thread tells the service.
enum Took {
    Proposal(Value),
    /// The store restarted: hello and publish everything again.
    Reset,
}

/// Takes proposals until `stop`, skipping those already in the inbox when it
/// starts: a proposal made before this service ran is not this service's.
fn take(harness: Arc<dyn Harness>, stop: Arc<AtomicBool>, out: mpsc::Sender<Took>) {
    let mut cursor: Option<u64> = None;
    let mut instance: Option<String> = None;
    let mut said_down = false;
    while !stop.load(Ordering::Relaxed) {
        let wait = if cursor.is_some() { TAKE_WAIT_MS } else { 0 };
        let answer = match harness.proposals(cursor.unwrap_or(0), wait, instance.as_deref()) {
            Ok(answer) => answer,
            Err(error) => {
                if !said_down {
                    eprintln!("lamp-addons machine: no proposals taken: {error}");
                    said_down = true;
                }
                sleep_unless(&stop, RETRY);
                continue;
            }
        };
        said_down = false;
        let next = answer.get("next").and_then(Value::as_u64).unwrap_or(0);
        let reset = answer.get("reset").and_then(Value::as_bool) == Some(true);
        instance = answer
            .get("instance")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let first = cursor.is_none();
        cursor = Some(next);
        if reset && !first && out.send(Took::Reset).is_err() {
            return;
        }
        if first && !reset {
            continue;
        }
        let proposals = answer
            .get("proposals")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for entry in proposals {
            if let Some(proposal) = entry.get("proposal")
                && out.send(Took::Proposal(proposal.clone())).is_err()
            {
                return;
            }
        }
    }
}

fn sleep_unless(stop: &AtomicBool, wait: Duration) {
    let until = Instant::now() + wait;
    while !stop.load(Ordering::Relaxed) && Instant::now() < until {
        std::thread::sleep(PASS);
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// What Harness holds of the lamp: in step, or to be cleared and sent again.
struct Published {
    /// The newest summary of each topic: its value and when the lamp made it.
    latest: BTreeMap<String, (Value, u64)>,
    /// Topics changed since the last batch Harness took.
    dirty: BTreeSet<String>,
    /// A hello is owed: at start, after a reset, and when the yes was
    /// withdrawn, until Harness answers one.
    hello_owed: bool,
    last_batch: Option<Instant>,
    last_try: Option<Instant>,
}

impl Published {
    fn summary(&mut self, topic: &str, value: Value, at_unix_ms: u64) {
        let Some(object) = value.as_object() else {
            return;
        };
        if Value::Object(object.clone()).to_string().len() > VALUE_BYTES {
            eprintln!("lamp-addons machine: {topic} withheld: too_big");
            return;
        }
        self.latest.insert(topic.to_owned(), (value, at_unix_ms));
        self.dirty.insert(topic.to_owned());
    }

    /// Harness brought in step with the yes, at most one request a second.
    /// The stream's slots, when it is on, go in the same batch.
    fn sync(
        &mut self,
        harness: &dyn Harness,
        yes: bool,
        stream: Option<&mut Stream>,
        report: &mut Report,
    ) {
        let now = Instant::now();
        if self.last_try.is_some_and(|at| now < at + PUBLISH_EVERY) {
            return;
        }
        if self.hello_owed {
            self.last_try = Some(now);
            match harness.hello() {
                Ok(_) => {
                    self.hello_owed = false;
                    self.dirty = self.latest.keys().cloned().collect();
                    eprintln!("lamp-addons machine: hello: what was published is cleared");
                }
                Err(error) => {
                    eprintln!("lamp-addons machine: hello not answered: {error}");
                    self.last_try = Some(now + RETRY);
                }
            }
            return;
        }
        if !yes || self.last_batch.is_some_and(|at| now < at + PUBLISH_EVERY) {
            return;
        }
        let packed = stream.as_ref().and_then(|stream| stream.pack());
        if self.dirty.is_empty() && packed.is_none() {
            return;
        }
        let at = unix_ms();
        // At most five summaries and eight slots: within Harness's 16 a batch.
        let values: Vec<Value> =
            self.dirty
                .iter()
                .filter_map(|topic| {
                    let (value, at) = self.latest.get(topic)?;
                    Some(json!({ "topic": topic, "value": value, "at": at }))
                })
                .chain(packed.iter().flat_map(|packed| {
                    packed.values.iter().map(
                        move |(topic, value)| json!({ "topic": topic, "value": value, "at": at }),
                    )
                }))
                .take(16)
                .collect();
        self.last_try = Some(now);
        match harness.publish(values) {
            Ok(_) => {
                report.published += 1;
                self.last_batch = Some(now);
                self.dirty.clear();
                if let (Some(stream), Some(packed)) = (stream, &packed) {
                    stream.sent(packed);
                }
            }
            Err(error) => {
                eprintln!("lamp-addons machine: summaries not published: {error}");
                self.last_try = Some(now + RETRY);
            }
        }
    }
}

/// A proposal put to the judge, waiting for its decision.
struct Waiting {
    proposal_id: String,
    until: Instant,
}

pub fn serve(
    config: Config,
    stop: Arc<AtomicBool>,
    harness: Arc<dyn Harness>,
) -> io::Result<Report> {
    let bus = match &config.bus_root {
        Some(root) => Bus::new(root)?,
        None => Bus::from_environment()?,
    };
    let mut summaries: Vec<(String, Subscriber)> = ADDON_TOPICS
        .iter()
        // The computer keeps its own clock; nothing on the lamp's bus says it.
        .filter(|topic| **topic != "clock/local")
        .map(|topic| Ok((topic.to_string(), bus.subscribe(topic, "machine")?)))
        .collect::<io::Result<_>>()?;
    let mut decisions = bus.subscribe(AddonDecision::TOPIC, "machine")?;
    let mut proposals: Publisher =
        bus.publisher_declared(AddonProposal::TOPIC, &AddonProposal::declaration(), SOURCE)?;
    let mut published = Published {
        latest: BTreeMap::new(),
        dirty: BTreeSet::new(),
        hello_owed: true,
        last_batch: None,
        last_try: None,
    };
    for (topic, _) in &summaries {
        if let Ok(Some(message)) = bus.latest(topic)
            && let Ok(value) = message.json()
        {
            published.summary(topic, value, message.header.at_unix_ms);
        }
    }
    let (sender, took) = mpsc::channel();
    let taker = {
        let (harness, stop) = (harness.clone(), stop.clone());
        std::thread::Builder::new()
            .name("machine-addons-take".into())
            .spawn(move || take(harness, stop, sender))?
    };
    let mut waiting: HashMap<u64, Waiting> = HashMap::new();
    let mut report = Report::default();
    let mut had_yes = false;
    // The stream: the bus's topics followed, and what the yes allowed last.
    let mut stream = Stream::default();
    let mut followed: BTreeMap<String, Subscriber> = BTreeMap::new();
    let mut streaming: Option<Allowed> = None;
    let mut looked: Option<Instant> = None;
    eprintln!("lamp-addons machine: taking proposals from the person's own computers");
    let began = Instant::now();
    while !stop.load(Ordering::Relaxed)
        && config
            .run_for
            .is_none_or(|run_for| began.elapsed() < run_for)
    {
        let yes = yes(&config.yes_file);
        if had_yes && yes.is_empty() {
            // Withdrawn: at once on the lamp, and a hello clears Harness.
            eprintln!("lamp-addons machine: the yes is withdrawn; clearing what was published");
            published.hello_owed = true;
            published.last_try = None;
        }
        had_yes = !yes.is_empty();
        let allowed = if config.stream {
            yes_to_stream(&config.yes_file)
        } else {
            None
        };
        if allowed != streaming {
            if streaming.is_some() {
                // Narrowed or withdrawn: what the computer holds goes too.
                eprintln!(
                    "lamp-addons machine: the stream's yes changed; clearing what was published"
                );
                published.hello_owed = true;
                published.last_try = None;
                stream = Stream::default();
            }
            followed.retain(|topic, _| {
                allowed.is_some_and(|allowed| crate::stream::followed(topic, allowed))
            });
            streaming = allowed;
            looked = None;
            if let Some(allowed) = allowed {
                eprintln!(
                    "lamp-addons machine: streaming the bus to the computer (words {}, busy {})",
                    if allowed.words { "on" } else { "off" },
                    if allowed.busy { "on" } else { "off" }
                );
            }
        }
        if let Some(allowed) = streaming
            && looked.is_none_or(|at| at.elapsed() >= LOOK_EVERY)
        {
            looked = Some(Instant::now());
            for topic in bus.topics()? {
                if crate::stream::followed(&topic.name, allowed)
                    && !followed.contains_key(&topic.name)
                {
                    match bus.subscribe(&topic.name, "ms") {
                        Ok(subscriber) => {
                            followed.insert(topic.name, subscriber);
                        }
                        Err(error) => {
                            eprintln!("lamp-addons machine: {} not followed: {error}", topic.name)
                        }
                    }
                }
            }
        }
        for (topic, subscriber) in &mut followed {
            while let Some(message) = subscriber.recv(Duration::ZERO)? {
                if let Ok(value) = message.json()
                    && stream.take(topic, value, message.header.at_unix_ms)
                {
                    report.lines += 1;
                }
            }
        }
        for (topic, subscriber) in &mut summaries {
            while let Some(message) = subscriber.recv(Duration::ZERO)? {
                if let Ok(value) = message.json() {
                    published.summary(topic, value, message.header.at_unix_ms);
                }
            }
        }
        published.sync(
            harness.as_ref(),
            had_yes,
            streaming.is_some().then_some(&mut stream),
            &mut report,
        );
        while let Ok(news) = took.try_recv() {
            match news {
                Took::Reset => {
                    eprintln!("lamp-addons machine: Harness restarted; publishing again");
                    published.hello_owed = true;
                    published.last_try = None;
                }
                Took::Proposal(payload) => {
                    let number = lamp_ipc::monotonic_us().max(1);
                    let taken = if waiting.len() >= MOST_WAITING {
                        Err(Refused {
                            proposal_id: payload
                                .get("proposalId")
                                .and_then(Value::as_str)
                                .filter(|id| uuid(id))
                                .map(str::to_owned),
                            reason: "busy: four are waiting for the judge",
                        })
                    } else {
                        convert(&payload, &yes, number, unix_ms())
                    };
                    match taken {
                        Ok(taken) => {
                            let at = lamp_ipc::monotonic_us();
                            match proposals.publish_json(at, &taken.proposal) {
                                Ok(_) => {
                                    report.proposed += 1;
                                    eprintln!(
                                        "lamp-addons machine: {} {} #{} put to the judge",
                                        taken.proposal.addon, taken.proposal.act, number
                                    );
                                    waiting.insert(
                                        number,
                                        Waiting {
                                            proposal_id: taken.proposal_id,
                                            until: Instant::now()
                                                + Duration::from_millis(
                                                    taken.proposal.holds_ms.into(),
                                                )
                                                + DECISION_SLACK,
                                        },
                                    );
                                }
                                Err(error) => {
                                    eprintln!(
                                        "lamp-addons machine: #{number} not written: {error}"
                                    );
                                    answer(
                                        harness.as_ref(),
                                        &taken.proposal_id,
                                        false,
                                        "the lamp could not take it",
                                    );
                                    report.refused += 1;
                                }
                            }
                        }
                        Err(refused) => {
                            report.refused += 1;
                            eprintln!(
                                "lamp-addons machine: refused a proposal: {}",
                                refused.reason
                            );
                            if let Some(id) = refused.proposal_id {
                                answer(harness.as_ref(), &id, false, refused.reason);
                            }
                        }
                    }
                }
            }
        }
        while let Some(message) = decisions.recv(PASS)? {
            if message.header.source != "addons" {
                continue;
            }
            let Ok(decision) = message.parse::<AddonDecision>() else {
                continue;
            };
            let Some(pending) = waiting.remove(&decision.proposal) else {
                continue;
            };
            report.decided += 1;
            answer(
                harness.as_ref(),
                &pending.proposal_id,
                decision.choice == "kept",
                &decision.why,
            );
        }
        let now = Instant::now();
        waiting.retain(|number, pending| {
            let keep = now < pending.until;
            if !keep {
                eprintln!("lamp-addons machine: #{number} not decided in time; Harness expires it");
            }
            keep
        });
    }
    stop.store(true, Ordering::Relaxed);
    drop(took);
    let _ = taker.join();
    Ok(report)
}

/// A decision sent back: journaled when it can't be, never sent again as a
/// new proposal.
fn answer(harness: &dyn Harness, proposal_id: &str, kept: bool, reason: &str) {
    if let Err(error) = harness.decision(proposal_id, kept, reason) {
        eprintln!("lamp-addons machine: the decision on {proposal_id} not sent: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "8c7d3f0e-1b2a-4c5d-9e8f-0a1b2c3d4e5f";
    const NOW: u64 = 1_761_030_000_000;

    fn yes() -> BTreeSet<String> {
        BTreeSet::from(["mac-of-the-owner".to_owned()])
    }

    fn glow() -> Value {
        json!({"proposalId": ID, "addon": "evening-glow", "act": "light", "why": "evening",
            "sure": 0.6, "holdsMs": 30000, "params": {"colour": "warm", "level": 2},
            "expiresAt": NOW + 30_000, "from": {"machineId": "mac-of-the-owner", "label": "Mac"}})
    }

    fn refused(payload: Value) -> &'static str {
        convert(&payload, &yes(), 7, NOW).unwrap_err().reason
    }

    #[test]
    fn a_computer_is_stamped_by_its_id_never_its_words() {
        let taken = convert(&glow(), &yes(), 7, NOW).unwrap();
        assert_eq!(taken.proposal_id, ID);
        assert_eq!(taken.proposal.addon, stamp("mac-of-the-owner"));
        assert_eq!(taken.proposal.addon.len(), 16);
        assert!(taken.proposal.addon.starts_with("own-"));
        assert_eq!(taken.proposal.version, CONTRACT_VERSION);
        assert_eq!(taken.proposal.proposal, 7);
        assert_eq!(taken.proposal.light.unwrap().colour, "warm");
        assert_ne!(stamp("mac-of-the-owner"), stamp("another-mac"));
    }

    #[test]
    fn only_a_computer_with_the_yes_is_put_to_the_judge() {
        let mut other = glow();
        other["from"]["machineId"] = json!("another-mac");
        assert_eq!(refused(other), "no yes for this computer");
        assert_eq!(
            convert(&glow(), &BTreeSet::new(), 7, NOW)
                .unwrap_err()
                .reason,
            "no yes for this computer"
        );
        let mut nobody = glow();
        nobody.as_object_mut().unwrap().remove("from");
        assert_eq!(refused(nobody), "no computer said it came from");
    }

    #[test]
    fn a_refusal_names_the_proposal_to_answer() {
        let mut late = glow();
        late["expiresAt"] = json!(NOW);
        let refusal = convert(&late, &yes(), 7, NOW).unwrap_err();
        assert_eq!(refusal.proposal_id.as_deref(), Some(ID));
        assert_eq!(refusal.reason, "arrived after it expired");
        let mut nameless = glow();
        nameless["proposalId"] = json!("not-a-uuid");
        assert_eq!(
            convert(&nameless, &yes(), 7, NOW).unwrap_err().proposal_id,
            None
        );
    }

    #[test]
    fn only_light_and_say_with_the_lamps_reasons() {
        let mut moving = glow();
        moving["act"] = json!("move");
        assert_eq!(refused(moving), "not_for_remote: only light and say");
        let mut free = glow();
        free["why"] = json!("messages");
        assert_eq!(refused(free), "why is not one of the lamp's reasons");
        let mut long = glow();
        long["holdsMs"] = json!(30_001);
        assert_eq!(refused(long), "holds 1 s to 30 s");
        let mut blinding = glow();
        blinding["params"] = json!({"colour": "warm", "level": 9});
        assert_eq!(refused(blinding), "not as the lamp takes a proposal");
    }

    #[test]
    fn a_say_carries_one_line_of_at_most_300_bytes_never_rewritten() {
        let mut say = glow();
        say["act"] = json!("say");
        say.as_object_mut().unwrap().remove("params");
        say["note"] = json!("Three new posts in the team channel.");
        let taken = convert(&say, &yes(), 7, NOW).unwrap();
        assert_eq!(
            taken.proposal.note.as_deref(),
            Some("Three new posts in the team channel.")
        );
        let mut lines = say.clone();
        lines["note"] = json!("one\ntwo");
        assert_eq!(refused(lines), "note_not_one_line");
        let mut long = say.clone();
        long["note"] = json!("a".repeat(301));
        assert_eq!(refused(long), "a note is at most 300 bytes");
        let mut parts = say.clone();
        parts["params"] = json!({"part": {"id": "a", "n": 1, "of": 2}});
        assert_eq!(refused(parts), "not_for_partner: a say carries no parts");
        let mut silent = say;
        silent.as_object_mut().unwrap().remove("note");
        assert_eq!(refused(silent), "a say carries a note");
    }

    #[test]
    fn the_yes_is_read_whole_or_not_at_all() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("yes.json");
        assert!(super::yes(&file).is_empty());
        std::fs::write(&file, r#"{"machines":["mac-of-the-owner"]}"#).unwrap();
        assert_eq!(super::yes(&file), yes());
        std::fs::write(&file, r#"{"machines":["mac-of-the-owner", "bad id"]}"#).unwrap();
        assert!(super::yes(&file).is_empty());
        std::fs::write(&file, "not json").unwrap();
        assert!(super::yes(&file).is_empty());
        std::fs::write(
            &file,
            format!(r#"{{"machines":["{}"]}}"#, "a".repeat(5_000)),
        )
        .unwrap();
        assert!(super::yes(&file).is_empty());
    }

    #[test]
    fn an_answer_is_read_plain_or_chunked() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(response(plain).unwrap(), (200, b"{}".to_vec()));
        let chunked = b"HTTP/1.1 409 Conflict\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"a\r\n4\r\n\":1}\r\n0\r\n\r\n";
        assert_eq!(response(chunked).unwrap(), (409, br#"{"a":1}"#.to_vec()));
        assert!(response(b"garbage").is_err());
        assert!(
            response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nff\r\nshort").is_err()
        );
    }

    #[test]
    fn the_credential_is_this_users_own_0600_token() {
        use std::os::unix::fs::PermissionsExt;
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("hook-credential");
        assert!(credential(&file).is_err());
        std::fs::write(&file, format!("{}\n", "A".repeat(43))).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(credential(&file).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(credential(&file).unwrap(), "A".repeat(43));
        std::fs::write(&file, "short\n").unwrap();
        assert!(credential(&file).is_err());
        let link = folder.path().join("link");
        std::fs::write(&file, format!("{}\n", "A".repeat(43))).unwrap();
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(credential(&link).is_err());
    }
}
