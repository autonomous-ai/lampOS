//! Opt-in, latched unplug preparation. The body alone proves physical settling.
//! A kept request inhibits startup motion after a service restart. Only a
//! matching Skills cancel ("never mind") or a reboot of the default volatile
//! runtime bus clears it; a persistent custom bus retains it across reboot.
use lamp_bus::{Bus, Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

pub const ENVIRONMENT: &str = "LAMP_UNPLUG";
pub const REQUEST_TOPIC: &str = "body/unplug_request";
pub const STATE_TOPIC: &str = "body/unplug_state";
/// Kept `{request}` from Skills: the person no longer unplugs the lamp.
pub const CANCEL_TOPIC: &str = "body/unplug_cancel";
pub const LIFESPAN: Duration = Duration::from_millis(500);
const PERIOD: Duration = Duration::from_millis(50);
const FRESH_US: u64 = 100_000;
const SETTLE_US: u64 = 500_000;
const TIMEOUT: Duration = Duration::from_secs(30);
/// After an accepted cancel the lamp stays down this long, so the spoken
/// "Okay, I'll stay." comes before any motion.
const RISE_AFTER_US: u64 = 3_000_000;
/// After an accepted cancel, a lamp not standing by then (an arm resting on
/// a book, a joint that keeps reading as moved) fails, so Skills says it
/// needs checking instead of staying down in silence (the reviewer on #2069).
const STAND_WITHIN_US: u64 = 60_000_000;

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
    pub fn cancel_declaration() -> Declaration {
        Declaration::new("lamp_body/UnplugCancel", 1, Kind::Latest)
    }
}

/// Whether Skills cancelled exactly `request`. A foreign, unreadable or
/// other-request cancel is no cancel: the latch stays (fail closed).
pub fn cancelled(bus: &Bus, request: &Request) -> bool {
    bus.latest(CANCEL_TOPIC)
        .ok()
        .flatten()
        .filter(|message| message.header.source == "skills")
        .and_then(|message| message.parse::<Request>().ok())
        .is_some_and(|cancel| cancel == *request)
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

/// Whether this body's last state for `request`, however old, was Failed: a
/// failed preparation needs inspection, so its cancel never permits startup.
fn failed_last(bus: &Bus, request: &Request) -> io::Result<bool> {
    Ok(match bus.latest(STATE_TOPIC)? {
        Some(message) => {
            let state = message.parse::<State>()?;
            state.request == request.request && state.phase == Phase::Failed
        }
        None => false,
    })
}

/// The kept request still in force: one Skills has not cancelled.
pub fn in_force(bus: &Bus, source: Option<&str>) -> io::Result<Option<Request>> {
    Ok(request(bus, source)?.filter(|request| !cancelled(bus, request)))
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
    if let Some(request) = request(bus, None)?
        && (!cancelled(bus, &request) || failed_last(bus, &request)?)
    {
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
        match in_force(bus, Some("skills")) {
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

impl Shared {
    /// One worker step on the clock `now`: take a new request, judge the
    /// latest evidence, publish, and stand only when this request's
    /// preparation allows it.
    fn tick(
        &self,
        bus: &Bus,
        publisher: &mut lamp_bus::Publisher,
        preparation: &mut Option<Preparation>,
        stopping: bool,
        now: impl Fn() -> u64,
    ) {
        if preparation.is_none()
            && !stopping
            && let Some(value) = self.poll_request(bus)
        {
            *preparation = Some(Preparation::new(value, now()));
        }
        let Some(prepared) = preparation else {
            return;
        };
        let evidence = self.evidence.load(Ordering::SeqCst);
        let invalid = self.invalid.load(Ordering::SeqCst);
        let cancel = prepared.cancelled_at.is_none() && cancelled(bus, &prepared.accepted);
        // Sample time first: a concurrent motor update must not
        // look like future evidence against an earlier clock read.
        let now = now();
        let why = self.why.load(Ordering::SeqCst);
        let (state, stand) = prepared.step(now, evidence, invalid, why, stopping, cancel);
        if publisher.publish_json(now, &state).is_err() {
            // The latch remains authoritative even if publication fails.
            prepared.failed = Some("unplug state could not be published".into());
        } else if stand {
            // The stand ends only this request's preparation (the reviewer
            // on #2069, P3 3): a newer request in force moves straight to
            // its own, latched throughout, so the motor loop never sees a
            // step unlatched between them; an unreadable one keeps the latch.
            match in_force(bus, Some("skills")) {
                Ok(None) => {
                    // The motor loop opens the ordinary wake window.
                    self.latched.store(false, Ordering::SeqCst);
                    *preparation = None;
                }
                Ok(Some(next)) => *preparation = Some(Preparation::new(next, now)),
                Err(_) => {}
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
                let mut preparation: Option<Preparation> = None;
                loop {
                    let stopping = shared.stopped.load(Ordering::SeqCst);
                    shared.tick(
                        &bus,
                        &mut publisher,
                        &mut preparation,
                        stopping,
                        lamp_ipc::monotonic_us,
                    );
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

/// One accepted request, from folding to ready, failed, or cancelled.
struct Preparation {
    accepted: Request,
    began: u64,
    proof: Proof,
    failed: Option<String>,
    was_ready: bool,
    /// When a matching Skills cancel was first seen while not failed.
    cancelled_at: Option<u64>,
}
impl Preparation {
    fn new(accepted: Request, now: u64) -> Self {
        Self {
            accepted,
            began: now,
            proof: Proof::default(),
            failed: None,
            was_ready: false,
            cancelled_at: None,
        }
    }
    /// The state to publish, and whether the lamp may now stand. A cancel
    /// during Folding lets the fold finish first; a Failed preparation is
    /// never cancelled, and readiness must be fresh and safe (no hold, no
    /// contact) at the moment the latch clears. After the cancel, a hold or
    /// contact only delays the stand.
    fn step(
        &mut self,
        now: u64,
        evidence: u64,
        invalid: u64,
        why: u64,
        stopping: bool,
        cancel: bool,
    ) -> (State, bool) {
        if cancel && self.failed.is_none() {
            self.cancelled_at.get_or_insert(now);
        }
        // After an accepted cancel, an unsafe sample (a hand helping it up)
        // withdraws Ready as a gap does instead of failing (the first builder
        // on #2069, P2): the latch holds while it lasts, and the stand still
        // needs fresh, safe proof and the 3 s.
        let unsafe_seen = invalid != self.proof.invalid && self.cancelled_at.is_none();
        let ready = self.proof.observe(now, evidence, invalid);
        let elapsed = Duration::from_micros(now.saturating_sub(self.began));
        if self.failed.is_none() {
            self.failed = verdict(self.was_ready, ready, unsafe_seen, elapsed, why);
        }
        if self.failed.is_none()
            && !ready
            && self
                .cancelled_at
                .is_some_and(|at| now.saturating_sub(at) >= STAND_WITHIN_US)
        {
            self.failed = Some(
                "no fresh folded proof within 60 seconds of the cancel; unplug preparation remains latched"
                    .into(),
            );
        }
        self.was_ready |= ready;
        if stopping {
            self.failed = Some("body stopped before unplug could remain ready".into());
        }
        let phase = if self.failed.is_some() {
            Phase::Failed
        } else if ready {
            Phase::Ready
        } else {
            Phase::Folding
        };
        let stand = phase == Phase::Ready
            && self
                .cancelled_at
                .is_some_and(|at| now.saturating_sub(at) >= RISE_AFTER_US);
        let state = State {
            request: self.accepted.request.clone(),
            phase,
            why: self.failed.clone().unwrap_or_default(),
        };
        (state, stand)
    }
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
    fn publish(bus: &Bus, topic: &str, source: &str, at: u64, value: serde_json::Value) {
        bus.publisher_declared(topic, &Request::declaration(), source)
            .unwrap()
            .publish_json(at, &value)
            .unwrap();
    }

    /// Drives a preparation on safe, fresh evidence every 50 ms from `from`
    /// to `to` (in ms), with a cancel arriving at `cancel_at`. Returns the
    /// first time it may stand and the last phase.
    fn drive(
        prepared: &mut Preparation,
        (from, to): (u64, u64),
        cancel_at: Option<u64>,
        safe: impl Fn(u64) -> bool,
    ) -> (Option<u64>, Phase) {
        let mut phase = Phase::Folding;
        // As the adapter counts them: every unsafe sample.
        let mut invalid = 0;
        for ms in (from..=to).step_by(50) {
            let now = ms * 1_000;
            invalid += u64::from(!safe(ms));
            let evidence = (now << 1) | u64::from(safe(ms));
            let cancel = cancel_at.is_some_and(|at| ms >= at);
            let (state, stand) = prepared.step(now, evidence, invalid, 1, false, cancel);
            phase = state.phase;
            if stand {
                return (Some(ms), phase);
            }
        }
        (None, phase)
    }

    fn accepted() -> Preparation {
        Preparation::new(
            Request {
                request: "r1".into(),
            },
            0,
        )
    }

    #[test]
    fn a_matching_cancel_at_ready_stands_only_after_the_spoken_line() {
        let mut prepared = accepted();
        // Ready from 550 ms; the cancel comes at 2 s.
        let (stand, phase) = drive(&mut prepared, (50, 10_000), Some(2_000), |_| true);
        assert_eq!(phase, Phase::Ready);
        assert_eq!(
            stand,
            Some(5_000),
            "three seconds after the cancel, not before"
        );
    }

    #[test]
    fn a_cancel_while_folding_finishes_the_fold_first() {
        let mut prepared = accepted();
        // Still moving (unsafe) until 6 s; the cancel came at 1 s.
        let (stand, phase) = drive(&mut prepared, (50, 10_000), Some(1_000), |ms| ms >= 6_000);
        assert_eq!(phase, Phase::Ready);
        assert_eq!(
            stand,
            Some(6_500),
            "stands only once the fold is proven (500 ms settled), never mid-fold"
        );
    }

    #[test]
    fn a_failed_preparation_is_never_cancelled() {
        // Timed out before the cancel: Failed stays latched.
        let mut prepared = accepted();
        let (stand, phase) = drive(&mut prepared, (50, 40_000), Some(31_000), |_| false);
        assert_eq!((stand, phase), (None, Phase::Failed));
        // Cancelled while folding, but the fold never proved: Failed, no stand.
        let mut prepared = accepted();
        let (stand, phase) = drive(&mut prepared, (50, 40_000), Some(1_000), |_| false);
        assert_eq!((stand, phase), (None, Phase::Failed));
    }

    /// The first builder on #2069 (P2): "never mind" at Ready, then a hand
    /// on the arm to help it up. The hand withdraws Ready and delays the
    /// stand; it never fails the preparation.
    #[test]
    fn a_hand_after_the_cancel_delays_the_stand_and_never_fails_it() {
        // A hand during the 3 s, gone at 3.1 s: proven again by 3.6 s, so it
        // stands at 5 s as without it.
        let mut prepared = accepted();
        let (stand, phase) = drive(&mut prepared, (50, 10_000), Some(2_000), |ms| {
            !(3_000..3_100).contains(&ms)
        });
        assert_eq!((stand, phase), (Some(5_000), Phase::Ready));
        // A hand held past the 3 s: it stands 500 ms of fresh, safe evidence
        // after the hand lets go, never before.
        let mut prepared = accepted();
        let mut phases = Vec::new();
        let mut invalid = 0;
        let mut stood = None;
        for ms in (50..=10_000).step_by(50) {
            let now = ms * 1_000;
            let safe = !(3_000..6_000).contains(&ms);
            invalid += u64::from(!safe);
            let (state, stand) = prepared.step(
                now,
                (now << 1) | u64::from(safe),
                invalid,
                8,
                false,
                ms >= 2_000,
            );
            phases.push(state.phase);
            if stand {
                stood = Some(ms);
                break;
            }
        }
        assert_eq!(stood, Some(6_500));
        assert!(!phases.contains(&Phase::Failed), "a hand never fails it");
        // Before any cancel, the same hand still fails a Ready preparation.
        let mut prepared = accepted();
        let (stand, phase) = drive(&mut prepared, (50, 10_000), None, |ms| {
            !(3_000..3_100).contains(&ms)
        });
        assert_eq!((stand, phase), (None, Phase::Failed));
    }

    /// The reviewer on #2069 (new P3): after a cancel, something that never
    /// lets go (a joint that keeps reading as moved) fails it at 60 s.
    #[test]
    fn with_no_proof_after_the_cancel_it_fails_at_sixty_seconds() {
        let mut prepared = accepted();
        let mut invalid = 0;
        let cancel = 2_000;
        for ms in (50..=cancel + 60_000).step_by(50) {
            let now = ms * 1_000;
            let safe = ms < 3_000;
            invalid += u64::from(!safe);
            let (state, stand) = prepared.step(
                now,
                (now << 1) | u64::from(safe),
                invalid,
                8,
                false,
                ms >= cancel,
            );
            assert!(!stand);
            // Last step before the bound: 59.95 s after the cancel.
            if ms < cancel + 60_000 {
                assert_ne!(state.phase, Phase::Failed, "at {ms} ms");
            } else {
                assert_eq!(state.phase, Phase::Failed);
                assert!(state.why.contains("60 seconds"), "{}", state.why);
            }
        }
    }

    #[test]
    fn stale_body_evidence_never_stands_after_a_cancel() {
        let mut prepared = accepted();
        let mut stood = false;
        for ms in (50..=10_000).step_by(50) {
            let now = ms * 1_000;
            // Last sample stopped at 100 ms: stale from then on.
            let sampled = now.min(100_000);
            let (_, stand) = prepared.step(now, (sampled << 1) | 1, 0, 0, false, true);
            stood |= stand;
        }
        assert!(!stood);
    }

    #[test]
    fn foreign_other_request_or_malformed_cancels_are_not_cancels() {
        let root =
            std::env::temp_dir().join(format!("unplug-cancel-refused-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        let request = Request {
            request: "r1".into(),
        };
        assert!(!cancelled(&bus, &request), "no cancel at all");
        publish(
            &bus,
            CANCEL_TOPIC,
            "expression",
            1,
            serde_json::json!({"request": "r1"}),
        );
        assert!(!cancelled(&bus, &request), "foreign source");
        publish(
            &bus,
            CANCEL_TOPIC,
            "skills",
            2,
            serde_json::json!({"request": "r0"}),
        );
        assert!(
            !cancelled(&bus, &request),
            "a stale cancel of another request"
        );
        publish(
            &bus,
            CANCEL_TOPIC,
            "skills",
            3,
            serde_json::json!({"never": "mind"}),
        );
        assert!(!cancelled(&bus, &request), "malformed");
        publish(
            &bus,
            CANCEL_TOPIC,
            "skills",
            4,
            serde_json::json!({"request": "r1"}),
        );
        assert!(cancelled(&bus, &request));
        // A cancel with no request latches nothing and stands nothing.
        let shared = Shared::default();
        assert!(shared.poll_request(&bus).is_none());
        assert!(!shared.latched.load(Ordering::SeqCst));
        assert!(startup(&bus).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn restart_with_a_request_and_its_cancel_starts_normally() {
        let root =
            std::env::temp_dir().join(format!("unplug-cancel-restart-{}", std::process::id()));
        // Each case on its own bus: an inhibited start publishes Failed.
        let fresh = |case: &str| {
            let root = root.join(case);
            let _ = std::fs::remove_dir_all(&root);
            let bus = Bus::new(&root).unwrap();
            publish(
                &bus,
                REQUEST_TOPIC,
                "skills",
                1,
                serde_json::json!({"request": "r1"}),
            );
            bus
        };
        let bus = fresh("foreign");
        publish(
            &bus,
            CANCEL_TOPIC,
            "expression",
            2,
            serde_json::json!({"request": "r1"}),
        );
        assert!(
            startup(&bus).is_err(),
            "a foreign cancel never permits startup"
        );
        let bus = fresh("other");
        publish(
            &bus,
            CANCEL_TOPIC,
            "skills",
            2,
            serde_json::json!({"request": "r0"}),
        );
        assert!(
            startup(&bus).is_err(),
            "another request's cancel never does"
        );
        let state = |bus: &Bus, phase: Phase| {
            bus.publisher_declared(STATE_TOPIC, &State::declaration(), "body")
                .unwrap()
                .publish_json(
                    3,
                    &State {
                        request: "r1".into(),
                        phase,
                        why: String::new(),
                    },
                )
                .unwrap();
        };
        // A preparation that failed (say, cancelled while folding, then the
        // body stopped) needs inspection: its cancel never permits startup.
        let bus = fresh("failed");
        publish(
            &bus,
            CANCEL_TOPIC,
            "skills",
            2,
            serde_json::json!({"request": "r1"}),
        );
        state(&bus, Phase::Failed);
        assert!(startup(&bus).is_err());

        let bus = fresh("cancelled");
        publish(
            &bus,
            CANCEL_TOPIC,
            "skills",
            2,
            serde_json::json!({"request": "r1"}),
        );
        assert!(startup(&bus).is_ok());
        state(&bus, Phase::Ready);
        assert!(
            startup(&bus).is_ok(),
            "stood up after its cancel, then restarted"
        );
        let shared = Shared::default();
        assert!(
            shared.poll_request(&bus).is_none(),
            "a cancelled request never relatches"
        );
        assert!(!shared.latched.load(Ordering::SeqCst));
        // The next unplug has a fresh identifier and latches again.
        publish(
            &bus,
            REQUEST_TOPIC,
            "skills",
            5,
            serde_json::json!({"request": "r2"}),
        );
        assert_eq!(shared.poll_request(&bus).unwrap().request, "r2");
        assert!(startup(&bus).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    /// The worker's steps on a fake clock (the reviewer on #2069, P3 5):
    /// fresh, safe evidence every 50 ms, a cancel at `cancel_at` ms and a
    /// request `r2` at `next_at` ms. Returns, per step, the time and whether
    /// the motor loop would see the latch.
    fn run_worker(
        name: &str,
        cancel_at: u64,
        next_at: Option<u64>,
        until: u64,
    ) -> (Vec<(u64, bool)>, Option<String>) {
        let root = std::env::temp_dir().join(format!("unplug-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        let mut publisher = bus
            .publisher_declared(STATE_TOPIC, &State::declaration(), "body")
            .unwrap();
        let shared = Shared::default();
        let mut preparation = None;
        publish(
            &bus,
            REQUEST_TOPIC,
            "skills",
            1,
            serde_json::json!({"request": "r1"}),
        );
        let mut steps = Vec::new();
        for ms in (50..=until).step_by(50) {
            if ms == cancel_at {
                publish(
                    &bus,
                    CANCEL_TOPIC,
                    "skills",
                    2,
                    serde_json::json!({"request": "r1"}),
                );
            }
            if Some(ms) == next_at {
                publish(
                    &bus,
                    REQUEST_TOPIC,
                    "skills",
                    3,
                    serde_json::json!({"request": "r2"}),
                );
            }
            let now = ms * 1_000;
            shared.evidence.store((now << 1) | 1, Ordering::SeqCst);
            shared.tick(&bus, &mut publisher, &mut preparation, false, || now);
            steps.push((ms, shared.latched.load(Ordering::SeqCst)));
        }
        let left = preparation.map(|prepared| prepared.accepted.request);
        let _ = std::fs::remove_dir_all(root);
        (steps, left)
    }

    #[test]
    fn the_running_adapter_unlatches_on_a_matching_cancel() {
        // Ready by 600 ms; the cancel at 1 s; unlatched 3 s after it, not before.
        let (steps, left) = run_worker("cancel", 1_000, None, 6_000);
        let unlatched = steps
            .iter()
            .find(|(_, latched)| !latched)
            .map(|(ms, _)| *ms);
        assert_eq!(unlatched, Some(4_000));
        assert!(steps.iter().all(|(ms, latched)| *latched == (*ms < 4_000)));
        assert_eq!(left, None);
    }

    /// The reviewer on #2069 (P3 3): "never mind", then "actually, get ready
    /// to unplug" within the 3 s. The first request's stand drops only its
    /// own preparation: the latch never opens, so no twitch.
    #[test]
    fn a_new_request_within_the_three_seconds_keeps_the_latch() {
        let (steps, left) = run_worker("next", 1_000, Some(2_000), 6_000);
        assert!(steps.iter().all(|(_, latched)| *latched), "never unlatched");
        assert_eq!(left.as_deref(), Some("r2"));
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
