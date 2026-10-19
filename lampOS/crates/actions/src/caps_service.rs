//! Optional Actions integration for durable output restrictions. The socket
//! acknowledges durable state separately from mixer readback and bus delivery.
//! A bus publication is never called a hardware or optical confirmation.

use crate::{
    caps,
    sound::{Mixer, Observation, Volume},
};
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{ClockStamp, OutputCap, OutputCaps};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    io,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const ENVIRONMENT: &str = "LAMP_OUTPUT_CAPS";
pub const EVERY_US: u64 = 1_000_000;

#[derive(Clone, Debug)]
pub struct Config {
    pub directory: PathBuf,
    pub bus_root: PathBuf,
}

impl Config {
    /// Off means no directory or bus access. Enabled use requires explicit
    /// persistent storage, separate from Actions' volatile runtime directory.
    pub fn configured(
        switch: Option<&OsStr>,
        directory: Option<&Path>,
        bus: Option<&Path>,
    ) -> io::Result<Option<Self>> {
        if switch != Some(OsStr::new("on")) {
            return Ok(None);
        }
        let directory =
            directory.ok_or_else(|| io::Error::other("LAMP_OUTPUT_CAPS=on requires --caps-dir"))?;
        let bus_root =
            bus.ok_or_else(|| io::Error::other("LAMP_OUTPUT_CAPS=on requires --bus-root"))?;
        if !safe_path(directory) || !safe_path(bus_root) {
            return Err(io::Error::other(
                "cap directories must be absolute without dot components",
            ));
        }
        Ok(Some(Self {
            directory: directory.into(),
            bus_root: bus_root.into(),
        }))
    }
}

pub fn safe_path(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
        && path
            .as_os_str()
            .as_encoded_bytes()
            .split(|byte| *byte == b'/')
            .all(|part| part != b"." && part != b"..")
}

/// One sample of the host monotonic clock and, only if known, its wall clock.
#[derive(Clone, Copy, Debug)]
pub struct Moment {
    pub at_us: u64,
    pub unix_ms: Option<u64>,
}

fn system_moment() -> Moment {
    // Same initialized-clock sanity predicate as Skills and the ring. This
    // cannot establish NTP synchronization or the accuracy of a saved date.
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|time| u64::try_from(time.as_millis()).ok())
        .filter(|time| lamp_msgs::output_caps_clock_known(*time));
    Moment {
        at_us: lamp_ipc::monotonic_us(),
        unix_ms,
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
enum Request {
    #[serde(rename = "caps_put")]
    Put { cap: caps::Put },
    #[serde(rename = "caps_remove")]
    Remove { source: String, token: String },
    #[serde(rename = "caps_get")]
    Get {},
}

pub fn is_operation(op: &str) -> bool {
    matches!(op, "caps_put" | "caps_remove" | "caps_get")
}

pub fn unavailable() -> Value {
    json!({"ok":false,"accepted":false,"code":"unavailable","error":"output caps are not enabled"})
}

fn effective(value: caps::Effective) -> Value {
    json!({"light_channel_max":value.light_channel_max,"volume_raw_max":value.volume_raw_max})
}

fn failure(error: caps::Error) -> Value {
    let code = match &error {
        caps::Error::Invalid(_) => "bad_arguments",
        caps::Error::UnknownClock => "clock_unknown",
        caps::Error::ChangedRetry => "conflict",
        caps::Error::Stale => "stale",
        caps::Error::Full => "full",
        caps::Error::UnknownSource => "not_found",
        caps::Error::TokenMismatch => "token_mismatch",
        caps::Error::Storage(_) => "storage_uncertain",
        _ => "unavailable",
    };
    // An I/O error may contain paths. The private caller gets a stable reason;
    // neither a path nor a token is included in the server's outcome log.
    let message = if matches!(
        &error,
        caps::Error::Storage(_) | caps::Error::RecoveryRequired
    ) {
        "cap state is uncertain; reopen before retrying".into()
    } else {
        error.to_string()
    };
    json!({"ok":false,"accepted":false,"code":code,"error":message})
}

pub struct Service {
    controller: caps::Controller,
    bus: Bus,
    publisher: Publisher,
    clock_id: String,
    clock: Box<dyn Fn() -> Moment + Send>,
    looked_at: Option<u64>,
    attempted_at: Option<u64>,
    applied: Option<Option<u8>>,
    volume: Value,
    volume_confirmed: bool,
    volume_required: bool,
    published: bool,
    failed: bool,
}

impl Service {
    pub fn open(config: &Config) -> io::Result<Self> {
        // Host boot identity, never the publisher's randomly chosen session.
        let clock = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        Self::with_clock(config, clock.trim(), system_moment)
    }

    /// Fixture seam: real storage and bus, caller-supplied clocks. No fixture
    /// clock is selectable through the product command line or environment.
    pub fn with_clock(
        config: &Config,
        clock_id: &str,
        clock: impl Fn() -> Moment + Send + 'static,
    ) -> io::Result<Self> {
        let sample = clock();
        ClockStamp {
            clock_id: clock_id.into(),
            at_us: sample.at_us,
            sequence: 1,
        }
        .check()
        .map_err(io::Error::other)?;
        if sample.at_us == 0 {
            return Err(io::Error::other("cap clock must be a host monotonic time"));
        }
        let controller =
            caps::Controller::open(&config.directory, sample.unix_ms).map_err(io::Error::other)?;
        let bus = Bus::new(&config.bus_root)?;
        let publisher = bus.publisher_declared(
            OutputCaps::TOPIC,
            &OutputCaps::declaration(),
            OutputCaps::SOURCE,
        )?;
        Ok(Self {
            controller,
            bus,
            publisher,
            clock_id: clock_id.into(),
            clock: Box::new(clock),
            looked_at: None,
            attempted_at: None,
            applied: None,
            volume: json!({"required":false,"confirmed":false}),
            volume_confirmed: false,
            volume_required: false,
            published: false,
            failed: false,
        })
    }

    pub fn ready(&self) -> bool {
        !self.failed && self.published && (!self.volume_required || self.volume_confirmed)
    }

    /// Only the applicable mixer restriction gates requested audio. A light
    /// snapshot's delivery is not an additional sound permission dependency.
    pub(crate) fn volume_permitted(&self) -> bool {
        !self.volume_required || self.volume_confirmed
    }

    /// A fresh actual read within the accepted ceiling establishes compliance
    /// without another mixer call. Unknown/out-of-limit observations invalidate
    /// it and leave retry to the existing bounded cap cadence.
    pub(crate) fn observe_volume(&mut self, observation: Option<Observation>) {
        if !self.volume_required {
            return;
        }
        let confirmed = observation.is_some_and(|observation| {
            observation.revocation.is_some()
                && observation.sequence.is_some_and(|sequence| sequence > 0)
                && observation
                    .raw
                    .is_some_and(|raw| self.applied.flatten().is_some_and(|limit| raw <= limit))
        });
        self.volume_confirmed = confirmed;
        self.volume = if confirmed {
            json!({"required":true,"confirmed":true})
        } else {
            json!({"required":true,"confirmed":false,"error":"volume was not confirmed after the last observation"})
        };
    }

    /// Preserve a known fatal failure even when shutdown skips the next tick.
    /// This only reads the recorded state; it performs no disk or output work.
    pub(crate) fn check(&self) -> io::Result<()> {
        if self.failed {
            Err(io::Error::other("cap storage requires recovery"))
        } else {
            Ok(())
        }
    }

    fn apply_volume<M: Mixer>(
        &mut self,
        wanted: Option<u8>,
        sound: &mut Option<Volume<M>>,
        now: u64,
        before_restriction: &mut impl FnMut(),
    ) {
        self.volume_required = wanted.is_some();
        // With no restriction at a fresh start, there is no hardware change
        // or confirmation to obtain. Avoid polling the ordinary mixer.
        if self.applied.is_none() && wanted.is_none() {
            self.applied = Some(None);
            return;
        }
        let retry = self.volume_required
            && !self.volume_confirmed
            && self
                .attempted_at
                .is_none_or(|at| now.saturating_sub(at) >= EVERY_US);
        if self.applied == Some(wanted) && !retry {
            return;
        }
        if wanted.is_some_and(|limit| {
            !self.volume_confirmed || self.applied.flatten().is_none_or(|old| limit < old)
        }) {
            // Durable acceptance already happened. Withdraw old audible
            // evidence before a tighter or unconfirmed restriction can block
            // in mixer I/O; unrelated light updates never reach this hook.
            before_restriction();
        }
        self.applied = Some(wanted);
        let result = sound.as_mut().map(|volume| volume.set_limit(wanted));
        // Space retries after completion too: a slow failed mixer must not
        // cause back-to-back subprocess batches on the next idle pass.
        self.attempted_at = Some((self.clock)().at_us);
        match result {
            Some(Ok(level)) => {
                self.volume_confirmed = true;
                self.volume =
                    json!({"required":self.volume_required,"confirmed":true,"level":level});
            }
            Some(Err(error)) => {
                self.volume_confirmed = false;
                self.volume =
                    json!({"required":self.volume_required,"confirmed":false,"error":error});
            }
            None => {
                self.volume_confirmed = false;
                self.volume = json!({"required":self.volume_required,"confirmed":false,"error":"this lamp has no volume control"});
            }
        }
    }

    fn publish(&mut self, snapshot: &caps::Snapshot) -> io::Result<()> {
        let now = (self.clock)();
        let message = OutputCaps {
            stamp: ClockStamp {
                clock_id: self.clock_id.clone(),
                at_us: now.at_us,
                sequence: self
                    .publisher
                    .totals()
                    .published
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("cap sequence exhausted"))?,
            },
            session_id: self.publisher.boot().into(),
            revision: snapshot.revision,
            entries: snapshot
                .active
                .iter()
                .map(|cap| OutputCap {
                    source: cap.source.clone(),
                    token: cap.token.clone(),
                    issued_unix_ms: cap.issued_unix_ms,
                    valid_until_unix_ms: cap.valid_until_unix_ms,
                    light_channel_max: cap.light_channel_max,
                    volume_raw_max: cap.volume_raw_max,
                })
                .collect(),
        };
        message.check().map_err(io::Error::other)?;
        self.publisher.publish_json(now.at_us, &message)?;
        // Bus suppresses repeated errors, so Ok alone cannot prove its Latest
        // file was replaced. Check the exact kept envelope before readiness.
        let kept = self
            .bus
            .latest(OutputCaps::TOPIC)?
            .ok_or_else(|| io::Error::other("cap snapshot was not kept"))?;
        let read = OutputCaps::from_message(&kept, &self.clock_id, (self.clock)().at_us, 0)
            .map_err(io::Error::other)?;
        if read != message {
            return Err(io::Error::other(
                "another cap snapshot replaced this publisher's",
            ));
        }
        Ok(())
    }

    fn sync<M: Mixer>(
        &mut self,
        snapshot: caps::Snapshot,
        sound: &mut Option<Volume<M>>,
        before_restriction: &mut impl FnMut(),
    ) {
        self.apply_volume(
            snapshot.effective.volume_raw_max,
            sound,
            (self.clock)().at_us,
            before_restriction,
        );
        let was = self.published;
        self.published = self.publish(&snapshot).is_ok();
        if was != self.published {
            eprintln!(
                "lamp-actions: output cap snapshot {}",
                if self.published {
                    "published"
                } else {
                    "unavailable"
                }
            );
        }
    }

    /// The existing server loop calls this; disk expiry and retry work happen
    /// at most once per second, never on each 50 ms idle pass. A storage error
    /// stops service before a looser snapshot or a success can be published.
    pub fn tick<M: Mixer>(&mut self, sound: &mut Option<Volume<M>>) -> io::Result<()> {
        self.tick_guarded(sound, &mut || {})
    }

    pub(crate) fn tick_guarded<M: Mixer>(
        &mut self,
        sound: &mut Option<Volume<M>>,
        before_restriction: &mut impl FnMut(),
    ) -> io::Result<()> {
        self.check()?;
        let now = (self.clock)();
        if self.looked_at.is_some_and(|at| now.at_us < at) {
            self.failed = true;
            return Err(io::Error::other("cap monotonic clock regressed"));
        }
        if self.looked_at.is_some_and(|at| now.at_us - at < EVERY_US) {
            return Ok(());
        }
        self.looked_at = Some(now.at_us);
        let snapshot = self.controller.expire(now.unix_ms).map_err(|error| {
            self.failed = true;
            io::Error::other(error)
        })?;
        self.sync(snapshot, sound, before_restriction);
        self.looked_at = Some((self.clock)().at_us);
        Ok(())
    }

    /// Original request bytes are decoded directly: a Value round trip would
    /// hide duplicate fields. The server bounds the complete line to 8192 bytes.
    pub fn answer<M: Mixer>(&mut self, line: &[u8], sound: &mut Option<Volume<M>>) -> Value {
        self.answer_guarded(line, sound, &mut || {})
    }

    pub(crate) fn answer_guarded<M: Mixer>(
        &mut self,
        line: &[u8],
        sound: &mut Option<Volume<M>>,
        before_restriction: &mut impl FnMut(),
    ) -> Value {
        if self.failed {
            return failure(caps::Error::RecoveryRequired);
        }
        if line.len() > 8192 {
            return failure(caps::Error::Invalid("cap request is too large"));
        }
        let request: Request = match serde_json::from_slice(line) {
            Ok(request) => request,
            Err(_) => {
                return failure(caps::Error::Invalid(
                    "cap request has invalid or unknown fields",
                ));
            }
        };
        let now = (self.clock)();
        let accepted = match request {
            Request::Put { cap } => self.controller.put(cap, now.unix_ms),
            Request::Remove { source, token } => {
                self.controller.remove(&source, &token, now.unix_ms)
            }
            Request::Get {} => {
                return match self.controller.snapshot() {
                    Ok(snapshot) => {
                        json!({"ok":true,"snapshot":{"revision":snapshot.revision,"active":snapshot.active,"effective":effective(snapshot.effective)}})
                    }
                    Err(error) => failure(error),
                };
            }
        };
        match accepted {
            Ok(accepted) => {
                let snapshot = match self.controller.snapshot() {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        self.failed = true;
                        return failure(error);
                    }
                };
                self.sync(snapshot, sound, before_restriction);
                json!({"ok":true,"accepted":true,"active":accepted.active,"revision":accepted.revision,
                    "effective":effective(accepted.effective),"volume":self.volume,
                    "ring":{"published":self.published,"confirmed":false}})
            }
            Err(error) => {
                if matches!(
                    &error,
                    caps::Error::Storage(_) | caps::Error::RecoveryRequired
                ) {
                    self.failed = true;
                }
                failure(error)
            }
        }
    }

    /// After actual mixer work, a failed ordinary volume operation invalidates
    /// the last confirmation. Rejections before I/O do not call this method.
    /// Retry stays on the bounded tick cadence; its software ceiling remains.
    pub fn observe_volume_result(&mut self, name: &str, response: &Value) {
        if self.volume_required && name.starts_with("volume_") {
            if response["ok"] != true {
                self.volume_confirmed = false;
                self.volume = json!({"required":true,"confirmed":false,"error":"volume was not confirmed after the last call"});
            } else if let Some(object) = self.volume.as_object_mut() {
                // A later person choice makes a cached numeric readback stale.
                object.remove("level");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    };

    const WALL: u64 = 1_800_000_000_000;

    #[derive(Clone)]
    struct Shared {
        raw: Arc<AtomicU8>,
        reads: Arc<AtomicUsize>,
        writes: Arc<AtomicUsize>,
        fails: Arc<AtomicBool>,
    }
    impl Shared {
        fn at(raw: u8) -> Self {
            Self {
                raw: Arc::new(AtomicU8::new(raw)),
                reads: Arc::default(),
                writes: Arc::default(),
                fails: Arc::default(),
            }
        }
    }
    impl Mixer for Shared {
        fn get(&mut self) -> io::Result<u8> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.raw.load(Ordering::SeqCst))
        }
        fn set(&mut self, level: u8) -> io::Result<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            if self.fails.load(Ordering::SeqCst) {
                return Err(io::Error::other("mixer failure"));
            }
            self.raw.store(level, Ordering::SeqCst);
            Ok(())
        }
    }

    struct Bench {
        _root: tempfile::TempDir,
        config: Config,
        time: Arc<Mutex<Moment>>,
        mixer: Shared,
        sound: Option<Volume<Shared>>,
        service: Service,
    }
    impl Bench {
        fn new(restored: bool) -> Self {
            let root = tempfile::Builder::new().prefix("ac-").tempdir().unwrap();
            let config = Config {
                directory: root.path().join("caps"),
                bus_root: root.path().join("bus"),
            };
            if restored {
                let mut controller = caps::Controller::open(&config.directory, Some(WALL)).unwrap();
                controller.put(request("one", WALL), Some(WALL)).unwrap();
            }
            let time = Arc::new(Mutex::new(Moment {
                at_us: 1_000_000,
                unix_ms: Some(WALL),
            }));
            let clock = time.clone();
            let service =
                Service::with_clock(&config, "fixture-host", move || *clock.lock().unwrap())
                    .unwrap();
            let mixer = Shared::at(80);
            let sound = Some(Volume::new(mixer.clone(), 100));
            Self {
                _root: root,
                config,
                time,
                mixer,
                sound,
                service,
            }
        }
        fn advance(&self, us: u64) {
            let mut time = self.time.lock().unwrap();
            time.at_us += us;
            time.unix_ms = time.unix_ms.map(|wall| wall + us / 1000);
        }
        fn ask(&mut self, value: Value) -> Value {
            self.service
                .answer(&serde_json::to_vec(&value).unwrap(), &mut self.sound)
        }
        fn latest(&self) -> OutputCaps {
            self.service
                .bus
                .latest(OutputCaps::TOPIC)
                .unwrap()
                .unwrap()
                .parse()
                .unwrap()
        }
    }

    fn request(token: &str, issued_unix_ms: u64) -> caps::Put {
        caps::Put {
            source: "bedtime".into(),
            token: token.into(),
            issued_unix_ms,
            valid_until_unix_ms: issued_unix_ms + 60_000,
            light_channel_max: Some(12),
            volume_raw_max: Some(40),
        }
    }

    #[test]
    fn configuration_is_exactly_opt_in_and_never_touches_disabled_paths() {
        for switch in [
            None,
            Some(OsStr::new("off")),
            Some(OsStr::new("ON")),
            Some(OsStr::new("true")),
        ] {
            assert!(
                Config::configured(switch, Some(Path::new("relative")), None)
                    .unwrap()
                    .is_none()
            );
        }
        assert!(Config::configured(Some(OsStr::new("on")), None, Some(Path::new("/bus"))).is_err());
        assert!(
            Config::configured(Some(OsStr::new("on")), Some(Path::new("/caps")), None).is_err()
        );
        for dir in ["relative", "/caps/../other", "/caps/./other"] {
            assert!(
                Config::configured(
                    Some(OsStr::new("on")),
                    Some(Path::new(dir)),
                    Some(Path::new("/bus"))
                )
                .is_err()
            );
        }
        assert!(
            Config::configured(
                Some(OsStr::new("on")),
                Some(Path::new("/var/lib/lampOS/output-caps")),
                Some(Path::new("/run/lampos-bus"))
            )
            .unwrap()
            .is_some()
        );
    }

    #[test]
    fn restored_volume_is_applied_and_a_full_snapshot_is_kept_before_readiness() {
        let mut bench = Bench::new(true);
        assert!(!bench.service.ready());
        bench.service.tick(&mut bench.sound).unwrap();
        assert_eq!(bench.mixer.raw.load(Ordering::SeqCst), 40);
        assert!(bench.service.ready());
        let message = bench
            .service
            .bus
            .latest(OutputCaps::TOPIC)
            .unwrap()
            .unwrap();
        let caps = OutputCaps::from_message(&message, "fixture-host", 1_000_000, 0).unwrap();
        assert_eq!(caps.entries[0].token, "one");
        assert_eq!(caps.entries[0].volume_raw_max, Some(40));
        assert_eq!(caps.session_id, message.header.boot);
        assert_eq!(caps.stamp.sequence, message.header.seq);
        assert_eq!(message.header.expires_ms, Some(3_000));
        let got = bench.ask(json!({"op":"caps_get"}));
        assert_eq!(got["snapshot"]["active"][0]["token"], "one");
        assert!(got.get("volume").is_none());
    }

    #[test]
    fn hardware_failure_keeps_accepted_state_and_retries_only_on_the_bounded_cadence() {
        let mut bench = Bench::new(false);
        bench.service.tick(&mut bench.sound).unwrap();
        bench.mixer.fails.store(true, Ordering::SeqCst);
        let reply = bench.ask(json!({"op":"caps_put","cap":request("one", WALL)}));
        assert_eq!(reply["accepted"], true);
        assert_eq!(reply["volume"]["confirmed"], false);
        assert_eq!(reply["ring"], json!({"published":true,"confirmed":false}));
        assert!(!bench.service.ready());
        let attempts = bench.mixer.writes.load(Ordering::SeqCst);
        for _ in 0..19 {
            bench.advance(50_000);
            bench.service.tick(&mut bench.sound).unwrap();
            assert_eq!(bench.mixer.writes.load(Ordering::SeqCst), attempts);
        }
        bench.mixer.fails.store(false, Ordering::SeqCst);
        bench.advance(50_000);
        bench.service.tick(&mut bench.sound).unwrap();
        assert_eq!(bench.mixer.writes.load(Ordering::SeqCst), attempts + 1);
        assert_eq!(bench.mixer.raw.load(Ordering::SeqCst), 40);
        assert!(bench.service.ready());
        assert_eq!(
            bench.ask(json!({"op":"caps_get"}))["snapshot"]["active"][0]["token"],
            "one"
        );
    }

    #[test]
    fn heartbeat_republishes_without_polling_a_confirmed_mixer() {
        let mut bench = Bench::new(true);
        bench.service.tick(&mut bench.sound).unwrap();
        let before = bench.latest();
        let reads = bench.mixer.reads.load(Ordering::SeqCst);
        bench.advance(EVERY_US);
        bench.service.tick(&mut bench.sound).unwrap();
        let after = bench.latest();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.entries, before.entries);
        assert_eq!(after.stamp.sequence, before.stamp.sequence + 1);
        assert_eq!(bench.mixer.reads.load(Ordering::SeqCst), reads);
    }

    #[test]
    fn explicit_release_with_unknown_time_preserves_a_newer_person_choice() {
        let mut bench = Bench::new(true);
        bench.service.tick(&mut bench.sound).unwrap();
        bench.sound.as_mut().unwrap().to_share(25).unwrap();
        let writes = bench.mixer.writes.load(Ordering::SeqCst);
        bench.time.lock().unwrap().unix_ms = None;
        let reply = bench.ask(json!({"op":"caps_remove","source":"bedtime","token":"one"}));
        assert_eq!(reply["accepted"], true);
        assert_eq!(reply["active"], false);
        assert_eq!(bench.mixer.raw.load(Ordering::SeqCst), 10);
        assert_eq!(bench.mixer.writes.load(Ordering::SeqCst), writes);
        assert!(bench.latest().entries.is_empty());
        assert_eq!(
            bench.ask(json!({"op":"caps_put","cap":request("two", WALL + 1)}))["code"],
            "clock_unknown"
        );
    }

    #[test]
    fn strict_original_bytes_refuse_unknown_duplicate_and_oversized_requests() {
        let mut bench = Bench::new(false);
        for line in [
            b"{\"op\":\"caps_get\",\"extra\":true}".as_slice(),
            b"{\"op\":\"caps_get\",\"op\":\"caps_get\"}".as_slice(),
            b"{\"op\":\"caps_remove\",\"source\":\"bedtime\",\"token\":\"one\",\"extra\":true}"
                .as_slice(),
        ] {
            assert_eq!(
                bench.service.answer(line, &mut bench.sound)["code"],
                "bad_arguments"
            );
        }
        let mut put = json!({"op":"caps_put","cap":request("one", WALL)});
        put["cap"]["extra"] = true.into();
        assert_eq!(bench.ask(put)["code"], "bad_arguments");
        assert_eq!(
            bench.service.answer(&vec![b' '; 8193], &mut bench.sound)["code"],
            "bad_arguments"
        );
        assert!(
            bench.ask(json!({"op":"caps_get"}))["snapshot"]["active"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(bench.mixer.reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_suppressed_bus_error_cannot_turn_into_false_readiness() {
        let mut bench = Bench::new(true);
        bench.service.tick(&mut bench.sound).unwrap();
        let latest = bench.config.bus_root.join(OutputCaps::TOPIC).join("latest");
        std::fs::remove_file(&latest).unwrap();
        std::fs::create_dir(&latest).unwrap();
        for _ in 0..2 {
            bench.advance(EVERY_US);
            bench.service.tick(&mut bench.sound).unwrap();
            assert!(!bench.service.ready());
        }
        std::fs::remove_dir(&latest).unwrap();
        bench.advance(EVERY_US);
        bench.service.tick(&mut bench.sound).unwrap();
        assert!(bench.service.ready());
        assert_eq!(bench.latest().entries[0].volume_raw_max, Some(40));
    }

    #[test]
    fn storage_failure_never_publishes_a_release_or_claims_acceptance() {
        let mut bench = Bench::new(true);
        bench.service.tick(&mut bench.sound).unwrap();
        let before = bench.latest();
        std::fs::rename(
            &bench.config.directory,
            bench.config.directory.with_file_name("moved"),
        )
        .unwrap();
        let reply = bench.ask(json!({"op":"caps_remove","source":"bedtime","token":"one"}));
        assert_eq!(reply["accepted"], false);
        assert_eq!(reply["code"], "storage_uncertain");
        assert!(!bench.service.ready());
        assert!(bench.service.tick(&mut bench.sound).is_err());
        assert_eq!(bench.latest(), before);
        assert_eq!(bench.mixer.raw.load(Ordering::SeqCst), 40);
    }
}
