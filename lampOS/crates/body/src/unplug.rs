//! Opt-in, latched unplug preparation. The body alone proves physical settling.
//! A kept request inhibits startup motion after a service restart. No request
//! clears the latch. Reboot clears the default volatile runtime bus, but a
//! persistent custom bus retains its request across reboot.
use lamp_bus::{Bus, Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub const ENVIRONMENT: &str = "LAMP_UNPLUG";
pub const REQUEST_TOPIC: &str = "body/unplug_request";
pub const STATE_TOPIC: &str = "body/unplug_state";
pub const LIFESPAN: Duration = Duration::from_millis(500);
const PERIOD: Duration = Duration::from_millis(50);
const FRESH_US: u64 = 100_000;
const SETTLE_US: u64 = 500_000;
const TIMEOUT: Duration = Duration::from_secs(30);

pub fn enabled() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub request: String,
}
impl Request {
    pub const TOPIC: &str = REQUEST_TOPIC;
    pub const TYPE: &str = "lamp_body/UnplugRequest";
    pub fn valid(&self) -> bool {
        !self.request.is_empty()
            && self.request.len() <= 128
            && self
                .request
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    }
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, 1, Kind::Latest)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Folding,
    Ready,
    Failed,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub request: String,
    pub phase: Phase,
    pub why: String,
}
impl State {
    pub const TOPIC: &str = STATE_TOPIC;
    pub const TYPE: &str = "lamp_body/UnplugState";
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, 1, Kind::Latest)
            .every(PERIOD)
            .lasting(LIFESPAN)
    }
}

fn request(bus: &Bus, source: Option<&str>) -> io::Result<Option<Request>> {
    bus.latest(REQUEST_TOPIC)?
        // A source is a shared-trust routing label, not authentication. Check
        // it before parsing so malformed foreign payloads cannot start a fold.
        .filter(|message| source.is_none_or(|source| message.header.source == source))
        .map(|message| {
            let request = message.parse::<Request>()?;
            if request.valid() {
                Ok(request)
            } else {
                Err(io::Error::other("invalid unplug request identifier"))
            }
        })
        .transpose()
}

/// Returns before any actuator access. Even an unreadable kept request vetoes
/// startup: a restart must never erase accepted intent by guessing it absent.
fn startup(bus: &Bus) -> io::Result<()> {
    if let Some(request) = request(bus, None)? {
        let mut publisher = bus.publisher_declared(STATE_TOPIC, &State::declaration(), "body")?;
        publisher.publish_json(
            lamp_ipc::monotonic_us(),
            &State {
                request: request.request,
                phase: Phase::Failed,
                why: "body restarted with a kept unplug request; startup motion inhibited".into(),
            },
        )?;
        return Err(io::Error::other(
            "kept unplug request present; startup motion inhibited",
        ));
    }
    Ok(())
}

/// Why a latched step is not folded evidence (`Glimpse::unplug_unsafe`), as
/// a failed unplug says it.
pub const UNPLUG_UNSAFE: [&str; 13] = [
    "the arm is off its fold",
    "the head or base is off its home",
    "it would tip",
    "a joint moved",
    "it is standing",
    "a gesture plays or waits",
    "it slumped",
    "a hand holds it",
    "a joint gave way",
    "it is stopping",
    "a joint was refused or left",
    "a read was bad or a goal unsent",
    "a servo reports a fault, heat or load",
];
/// The check in `UNPLUG_UNSAFE` that says nothing was read well this step: a
/// gap in the evidence, never a step judged unsafe.
pub const UNREAD: usize = 11;

#[derive(Default)]
struct Shared {
    latched: AtomicBool,
    stopped: AtomicBool,
    /// Low bit is safe; the remaining bits are the sample's monotonic time.
    evidence: AtomicU64,
    /// Never lose an unsafe sample between the slower publisher's polls.
    invalid: AtomicU64,
    /// Why the latest unsafe sample was: one more than its index in
    /// `UNPLUG_UNSAFE`.
    why: AtomicU64,
}

impl Shared {
    /// Live requests belong to Skills. Foreign labels are ignored; a broken
    /// message whose source cannot be read retains the fail-closed inhibit.
    fn poll_request(&self, bus: &Bus) -> Option<Request> {
        match request(bus, Some("skills")) {
            Ok(Some(value)) => {
                self.latched.store(true, Ordering::SeqCst);
                Some(value)
            }
            Ok(None) => None,
            Err(_) => {
                self.latched.store(true, Ordering::SeqCst);
                None
            }
        }
    }
}

/// The motor thread only stores atomics. All bus I/O stays on a bounded worker.
pub struct Adapter {
    shared: Arc<Shared>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Adapter {
    pub fn from_environment() -> io::Result<Option<Self>> {
        Self::configured(Bus::from_environment()?, enabled())
    }
    fn configured(bus: Bus, enabled: bool) -> io::Result<Option<Self>> {
        // The switch controls new live requests, not recovery from an existing
        // latch. Check before actuator access even after the switch was removed.
        startup(&bus)?;
        if !enabled {
            return Ok(None);
        }
        let mut publisher = bus.publisher_declared(STATE_TOPIC, &State::declaration(), "body")?;
        let shared = Arc::new(Shared::default());
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("body-unplug".into())
            .spawn(move || {
                let shared = worker_shared;
                let mut accepted: Option<Request> = None;
                let mut began = Instant::now();
                let mut proof = Proof::default();
                let mut failed: Option<String> = None;
                let mut was_ready = false;
                loop {
                    let stopping = shared.stopped.load(Ordering::SeqCst);
                    if accepted.is_none()
                        && !stopping
                        && let Some(value) = shared.poll_request(&bus)
                    {
                        accepted = Some(value);
                        began = Instant::now();
                    }
                    if let Some(accepted) = &accepted {
                        let evidence = shared.evidence.load(Ordering::SeqCst);
                        let invalid = shared.invalid.load(Ordering::SeqCst);
                        // Sample time first: a concurrent motor update must not
                        // look like future evidence against an earlier clock read.
                        let now = lamp_ipc::monotonic_us();
                        let unsafe_seen = invalid != proof.invalid;
                        let ready = proof.observe(now, evidence, invalid);
                        failed = failed.or_else(|| {
                            verdict(
                                was_ready,
                                ready,
                                unsafe_seen,
                                began.elapsed(),
                                shared.why.load(Ordering::SeqCst),
                            )
                        });
                        was_ready |= ready;
                        if stopping {
                            failed = Some("body stopped before unplug could remain ready".into());
                        }
                        let state = State {
                            request: accepted.request.clone(),
                            phase: if failed.is_some() {
                                Phase::Failed
                            } else if ready {
                                Phase::Ready
                            } else {
                                Phase::Folding
                            },
                            why: failed.clone().unwrap_or_default(),
                        };
                        if publisher.publish_json(now, &state).is_err() {
                            // The latch remains authoritative even if publication fails.
                            failed = Some("unplug state could not be published".into());
                        }
                    }
                    if stopping {
                        break;
                    }
                    thread::sleep(PERIOD);
                }
            })?;
        Ok(Some(Self {
            shared,
            worker: Some(worker),
        }))
    }
    pub fn latched(&self) -> bool {
        self.shared.latched.load(Ordering::SeqCst)
    }
    /// One step's evidence: folded and safe or not, and why not when the
    /// step was judged (an index into `UNPLUG_UNSAFE`).
    pub fn sample(&self, safe: bool, why: Option<u8>) {
        if !safe {
            self.shared
                .why
                .store(why.map_or(0, |why| u64::from(why) + 1), Ordering::SeqCst);
            self.shared.invalid.fetch_add(1, Ordering::SeqCst);
        }
        self.shared.evidence.store(
            (lamp_ipc::monotonic_us() << 1) | u64::from(safe),
            Ordering::SeqCst,
        );
    }
    pub fn stop(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
    }
}
impl Drop for Adapter {
    fn drop(&mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Why a preparation fails now, if it does. Only a sample that saw it
/// unsafe loses readiness for good. No fresh sample (a read missed or bad,
/// the motor thread late while the voice speaks) withdraws Ready until it
/// is proven again; it moved nothing. The 30 s limit is on reaching Ready
/// the first time, never on proving it again after a gap.
fn verdict(
    was_ready: bool,
    ready: bool,
    unsafe_seen: bool,
    elapsed: Duration,
    why: u64,
) -> Option<String> {
    if was_ready && !ready && unsafe_seen {
        let because = (why as usize)
            .checked_sub(1)
            .and_then(|at| UNPLUG_UNSAFE.get(at))
            .unwrap_or(&"an unsafe sample");
        return Some(format!(
            "physical readiness was lost ({because}); unplug preparation remains latched"
        ));
    }
    (!was_ready && !ready && elapsed >= TIMEOUT)
        .then(|| "physical folded position was not verified within 30 seconds".into())
}

#[derive(Default)]
struct Proof {
    since: Option<u64>,
    last: u64,
    invalid: u64,
}
impl Proof {
    fn observe(&mut self, now: u64, evidence: u64, invalid: u64) -> bool {
        let sampled = evidence >> 1;
        let safe = evidence & 1 == 1;
        let fresh = sampled <= now && now - sampled <= FRESH_US;
        if !safe
            || !fresh
            || invalid != self.invalid
            || sampled.saturating_sub(self.last) > FRESH_US
        {
            self.since = None;
        }
        self.invalid = invalid;
        self.last = sampled;
        if !safe || !fresh {
            return false;
        }
        let since = *self.since.get_or_insert(sampled);
        sampled.saturating_sub(since) >= SETTLE_US
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_requires_half_a_second_of_fresh_safe_evidence() {
        let mut proof = Proof::default();
        for tick in 1..=10 {
            let now = tick * 50_000;
            assert!(!proof.observe(now, (now << 1) | 1, 0));
        }
        assert!(proof.observe(550_000, (550_000 << 1) | 1, 0));
        assert!(
            !proof.observe(700_001, (550_000 << 1) | 1, 0),
            "old stationary samples do not prove readiness"
        );
    }
    #[test]
    fn an_invalid_read_between_polls_resets_proof() {
        let mut proof = Proof::default();
        for tick in 1..=12 {
            let now = tick * 50_000;
            proof.observe(now, (now << 1) | 1, 0);
        }
        assert!(!proof.observe(650_000, (650_000 << 1) | 1, 1));
        assert!(!proof.observe(700_000, 700_000 << 1, 1));
    }
    #[test]
    fn pause_or_a_commanded_fold_supplies_no_physical_proof() {
        assert!(!Proof::default().observe(1_000_000, 0, 0));
        assert!(!Proof::default().observe(10, (20 << 1) | 1, 0));
    }
    #[test]
    fn live_foreign_source_cannot_start_a_fold_even_with_a_malformed_payload() {
        let root = std::env::temp_dir().join(format!("unplug-source-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        let shared = Shared::default();
        let mut foreign = bus
            .publisher_declared(REQUEST_TOPIC, &Request::declaration(), "expression")
            .unwrap();
        foreign
            .publish_json(
                1,
                &Request {
                    request: "foreign-1".into(),
                },
            )
            .unwrap();
        assert!(shared.poll_request(&bus).is_none());
        assert!(!shared.latched.load(Ordering::SeqCst));
        assert!(bus.latest(STATE_TOPIC).unwrap().is_none());
        assert!(
            startup(&bus).is_err(),
            "even a foreign kept request vetoes restart"
        );
        foreign
            .publish_json(2, &serde_json::json!({"not_a_request": true}))
            .unwrap();
        assert!(shared.poll_request(&bus).is_none());
        assert!(
            !shared.latched.load(Ordering::SeqCst),
            "foreign payload errors must not become live inhibits"
        );
        assert!(
            startup(&bus).is_err(),
            "malformed foreign payload still vetoes restart"
        );
        let mut skills = bus
            .publisher_declared(REQUEST_TOPIC, &Request::declaration(), "skills")
            .unwrap();
        skills
            .publish_json(
                3,
                &Request {
                    request: "skills-1".into(),
                },
            )
            .unwrap();
        assert_eq!(shared.poll_request(&bus).unwrap().request, "skills-1");
        assert!(shared.latched.load(Ordering::SeqCst));
        foreign
            .publish_json(
                4,
                &Request {
                    request: "foreign-2".into(),
                },
            )
            .unwrap();
        assert!(shared.poll_request(&bus).is_none());
        assert!(
            shared.latched.load(Ordering::SeqCst),
            "a foreign request never clears an accepted latch"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// lamp-52e6 on 2026-10-20 (#2069 try 1): Ready, then a second voice
    /// turn, and "physical readiness was lost" for good. Samples that stop
    /// for a while (a read missed, the motor thread late) withdraw Ready
    /// and it comes back; only a sample that saw it unsafe fails, and says
    /// why.
    #[test]
    fn a_gap_in_samples_withdraws_ready_and_only_an_unsafe_one_fails() {
        let root = std::env::temp_dir().join(format!("unplug-gap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        let adapter = Adapter::configured(bus.clone(), true).unwrap().unwrap();
        bus.publisher_declared(REQUEST_TOPIC, &Request::declaration(), "skills")
            .unwrap()
            .publish_json(
                1,
                &Request {
                    request: "gap-1".into(),
                },
            )
            .unwrap();
        let phase = || {
            bus.latest(STATE_TOPIC)
                .unwrap()
                .map(|message| message.parse::<State>().unwrap())
                .map(|state| (state.phase, state.why))
        };
        let samples = |for_ms: u64| {
            for _ in 0..for_ms / 20 {
                adapter.sample(true, None);
                thread::sleep(Duration::from_millis(20));
            }
        };
        samples(900);
        assert_eq!(phase().unwrap().0, Phase::Ready);
        thread::sleep(Duration::from_millis(300));
        assert_eq!(phase().unwrap().0, Phase::Folding, "a gap is not Ready");
        samples(900);
        assert_eq!(phase().unwrap().0, Phase::Ready, "Ready again");
        adapter.sample(false, Some(3));
        thread::sleep(Duration::from_millis(150));
        let (phase, why) = phase().unwrap();
        assert_eq!(phase, Phase::Failed);
        assert!(why.contains("a joint moved"), "{why}");
        drop(adapter);
        let _ = std::fs::remove_dir_all(root);
    }

    /// The reviewer on #1699 (09:38): Ready at 12 s, a gap at 35 s. Proving
    /// it again is not reaching it late: never "not verified within 30
    /// seconds".
    #[test]
    fn a_gap_after_the_time_limit_is_not_a_timeout_once_ready() {
        let s = Duration::from_secs;
        assert_eq!(verdict(false, true, false, s(12), 0), None);
        assert_eq!(verdict(true, false, false, s(35), 0), None, "a gap");
        assert_eq!(verdict(true, true, false, s(36), 0), None, "Ready again");
        assert!(
            verdict(false, false, false, s(30), 0)
                .is_some_and(|why| why.contains("not verified within 30 seconds"))
        );
        assert!(
            verdict(true, false, true, s(40), 4).is_some_and(|why| why.contains("a joint moved"))
        );
    }

    #[test]
    fn malformed_skills_request_keeps_live_fail_closed_behavior() {
        let root =
            std::env::temp_dir().join(format!("unplug-malformed-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        let shared = Shared::default();
        let mut skills = bus
            .publisher_declared(REQUEST_TOPIC, &Request::declaration(), "skills")
            .unwrap();
        skills
            .publish_json(
                1,
                &Request {
                    request: String::new(),
                },
            )
            .unwrap();
        assert!(shared.poll_request(&bus).is_none());
        assert!(shared.latched.load(Ordering::SeqCst));
        assert!(startup(&bus).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn disabling_live_unplug_never_bypasses_a_kept_startup_latch() {
        let root = std::env::temp_dir().join(format!("unplug-off-restart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        assert!(Adapter::configured(bus.clone(), false).unwrap().is_none());
        assert!(
            !root.exists(),
            "off with no request creates no publisher or worker state"
        );
        for source in ["skills", "expression"] {
            let mut publisher = bus
                .publisher_declared(REQUEST_TOPIC, &Request::declaration(), source)
                .unwrap();
            publisher
                .publish_json(
                    1,
                    &Request {
                        request: "latched-before-switch-off".into(),
                    },
                )
                .unwrap();
            assert!(
                Adapter::configured(bus.clone(), false).is_err(),
                "{source}: disabling cannot permit startup"
            );
            let state = bus
                .latest(STATE_TOPIC)
                .unwrap()
                .unwrap()
                .parse::<State>()
                .unwrap();
            assert_eq!(state.phase, Phase::Failed);
            assert_eq!(state.request, "latched-before-switch-off");
            publisher
                .publish_json(
                    2,
                    &Request {
                        request: String::new(),
                    },
                )
                .unwrap();
            assert!(
                Adapter::configured(bus.clone(), false).is_err(),
                "malformed identifier remains inhibited when off"
            );
        }
        std::fs::write(
            root.join(REQUEST_TOPIC).join("latest"),
            b"malformed kept state",
        )
        .unwrap();
        assert!(
            Adapter::configured(bus.clone(), false).is_err(),
            "unreadable kept bytes remain inhibited when off"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn disabling_live_unplug_does_not_treat_a_filesystem_error_as_an_absent_latch() {
        let root =
            std::env::temp_dir().join(format!("unplug-off-filesystem-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // A real ENOTDIR while opening the kept request, not a malformed
        // payload and not a permission-mode test that passes differently as root.
        let obstruction = root.join("body");
        std::fs::write(&obstruction, b"not a topic directory").unwrap();
        let bus = Bus::new(&root).unwrap();
        let error = match Adapter::configured(bus, false) {
            Err(error) => error,
            Ok(_) => panic!("an unreadable kept path must inhibit startup even when off"),
        };
        assert_eq!(error.kind(), io::ErrorKind::NotADirectory);
        assert_eq!(
            std::fs::read(&obstruction).unwrap(),
            b"not a topic directory"
        );
        assert_eq!(
            std::fs::read_dir(&root).unwrap().count(),
            1,
            "no publisher state was made"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn kept_request_vetoes_restart_without_hardware() {
        let root = std::env::temp_dir().join(format!("unplug-restart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        assert!(startup(&bus).is_ok());
        let mut publisher = bus
            .publisher_declared(REQUEST_TOPIC, &Request::declaration(), "test")
            .unwrap();
        publisher
            .publish_json(
                1,
                &Request {
                    request: "request-1".into(),
                },
            )
            .unwrap();
        assert!(startup(&bus).is_err());
        let state = bus
            .latest(STATE_TOPIC)
            .unwrap()
            .unwrap()
            .parse::<State>()
            .unwrap();
        assert_eq!(state.phase, Phase::Failed);
        assert_eq!(state.request, "request-1");
        publisher
            .publish_json(
                2,
                &Request {
                    request: String::new(),
                },
            )
            .unwrap();
        assert!(
            startup(&bus).is_err(),
            "invalid identifier cannot permit startup"
        );
        std::fs::write(
            root.join(REQUEST_TOPIC).join("latest"),
            b"malformed kept state",
        )
        .unwrap();
        assert!(
            startup(&bus).is_err(),
            "malformed kept bytes cannot permit startup"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
