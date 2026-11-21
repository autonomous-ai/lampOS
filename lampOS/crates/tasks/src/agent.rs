//! One readiness for the lamp's own agent (#8138, the lead's decision 1):
//! computed here, in lamp-tasks, from what it already knows, published on
//! `tasks/agent` ([`lamp_msgs::AgentReadiness`], a kept value, replaced on
//! every change of state or why) and answered by the socket's
//! `{"op":"agent"}`. The voice refuses an ask at once from it
//! ([`refusal`], `agent_not_ready` at prepare), the app shows it.
//!
//! Its inputs ([`Facts`]), each set where lamp-tasks learns it:
//! - the lamp's default engine (the one session's look at the lamp);
//! - whether Harness is on the lamp, and the engine installed, being
//!   installed, or signed in on it (the machines' looks);
//! - OpenClaw's Gateway, probed on loopback by the one-session thread
//!   while the session is kept ([`crate::one_session::GATEWAY_LOOK_READY`],
//!   [`crate::one_session::GATEWAY_LOOK_DOWN`]);
//! - the kept session itself: opening, ready, on a task, recovering after
//!   a fault, or failed past its quick tries ([`Session`]), fed by its
//!   open, its hand-overs and turns, Harness's `watch` on it, and every
//!   fault sign.
//!
//! [`state_of`] is the whole rule, a pure function, so the state table in
//! `docs/tasks.md` is this code.
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use lamp_msgs::{AgentReadiness, Readiness};

/// The early `prepare` refusal for a task to the lamp's own agent while it
/// takes none ([`Readiness::takes_tasks`] false): its error JSON also
/// carries `"readiness": <state>`.
pub const NOT_READY: &str = "agent_not_ready";
/// The engine whose session lamp-tasks keeps and watches live (the one
/// session, [`crate::one_session::shares`]): only its readiness comes from
/// the session and the Gateway. Another default (Hermes) is read from its
/// install and sign-in alone.
pub const SESSION_ENGINE: &str = "openclaw";
/// How long the Gateway may not answer and still be read as starting
/// (`starting`/`gateway_starting`, or `recovering`/`gateway_down` after a
/// session was open); past it, `error` with `gateway_down`. A cold start
/// takes 27-50 s on lamp-c0a4 but 185-322 s on lamp-9b34 (#7174), so the
/// bound is above the slowest seen.
pub const GATEWAY_START_WITHIN: Duration = Duration::from_secs(360);
/// The why published once when lamp-tasks stops ([`Agent::stopping`]):
/// nothing reads its kept `ready` as true while it is down.
pub const TASKS_RESTARTING: &str = "tasks_restarting";

/// What the Gateway probe read last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gateway {
    /// Not probed (another engine, or no look yet).
    Unknown,
    Ready,
    /// Not answering, since this first look that found it so.
    Down(Instant),
}

/// The kept session as lamp-tasks last saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    /// None open yet, and no fault since the start.
    Closed,
    /// Being opened.
    Opening,
    /// Open and idle.
    Ready,
    /// On a turn.
    Working,
    /// A fault was seen; a fresh session is to be opened, with why.
    Recovering(&'static str),
    /// Its opens failed past the quick tries; tried again slowly, with why.
    Failed(&'static str),
}

/// The whys of a hold only the person can clear (Harness's watch reads the
/// kept session `held`: the device waiting for approval, the Gateway
/// refusing its sign-in): `error` with that why, no fresh session opened
/// for it, until the watch reads the session taking input again (#8138
/// review, C5).
pub const LASTING_HOLDS: [&str; 2] = ["held_approval", "held_sign_in"];

impl Session {
    /// Whether this is a lasting hold ([`LASTING_HOLDS`]).
    pub fn lasting(self) -> bool {
        matches!(self, Session::Failed(why) if LASTING_HOLDS.contains(&why))
    }
}

/// Whether Harness is on the lamp, as the part last read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harness {
    Unknown,
    Absent,
    Here,
}

/// Everything [`state_of`] reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    /// Tracked at all: the one session on (`LAMP_TASKS_ONE_SESSION`).
    pub on: bool,
    /// The lamp's default engine, by its bus name; `None`: none set.
    pub engine: Option<String>,
    pub harness: Harness,
    /// The engine known installed on the lamp; `None`: not known.
    pub installed: Option<bool>,
    /// lamp-update installing it now (Harness reads the session
    /// `installing`).
    pub installing: bool,
    /// The engine known signed in on the lamp; `None`: not known. Harness's
    /// own sign-out is not this: a signed-out lamp runs its agent (#6407).
    pub signed_in: Option<bool>,
    pub gateway: Gateway,
    pub session: Session,
    /// A session was open at least once since the start: a Gateway down
    /// after it is a fault (`recovering`), before it its start.
    pub opened_once: bool,
    /// Harness takes no reply here (`reply_off`, an older Harness): every
    /// task goes the per-task way, with no kept session to wait for, so
    /// the agent is `ready` past its install and sign-in.
    pub per_task: bool,
}

impl Default for Facts {
    fn default() -> Self {
        Facts {
            on: false,
            engine: None,
            harness: Harness::Unknown,
            installed: None,
            installing: false,
            signed_in: None,
            gateway: Gateway::Unknown,
            session: Session::Closed,
            opened_once: false,
            per_task: false,
        }
    }
}

/// The readiness of `facts` at `now`, with its why: the first rule that
/// holds, in this order.
pub fn state_of(facts: &Facts, now: Instant) -> (Readiness, Option<&'static str>) {
    let Some(engine) = facts.engine.as_deref().filter(|_| facts.on) else {
        return (Readiness::Off, None);
    };
    if facts.installing {
        return (Readiness::Installing, Some("installing"));
    }
    if facts.harness == Harness::Absent {
        return (Readiness::NotInstalled, Some("harness_missing"));
    }
    if facts.installed == Some(false) {
        return (Readiness::NotInstalled, Some("engine_missing"));
    }
    if facts.signed_in == Some(false) {
        return (Readiness::Error, Some("signed_out"));
    }
    if engine != SESSION_ENGINE || facts.per_task {
        // No session of lamp-tasks' own to read: its install and sign-in
        // above are all that is known.
        return (Readiness::Ready, None);
    }
    if let Gateway::Down(since) = facts.gateway {
        return if now.saturating_duration_since(since) >= GATEWAY_START_WITHIN {
            (Readiness::Error, Some("gateway_down"))
        } else if facts.opened_once {
            (Readiness::Recovering, Some("gateway_down"))
        } else {
            (Readiness::Starting, Some("gateway_starting"))
        };
    }
    match facts.session {
        Session::Failed(why) => (Readiness::Error, Some(why)),
        Session::Recovering(why) => (Readiness::Recovering, Some(why)),
        Session::Closed if facts.harness == Harness::Unknown => {
            (Readiness::Starting, Some("harness_starting"))
        }
        Session::Closed | Session::Opening => (Readiness::Starting, Some("session_starting")),
        Session::Ready => (Readiness::Ready, None),
        Session::Working => (Readiness::Working, None),
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |since| {
            since.as_millis().clamp(1, u128::from(u64::MAX)) as u64
        })
}

struct Kept {
    facts: Facts,
    state: Readiness,
    why: Option<&'static str>,
    since_ms: u64,
    /// Counted at each change of state or why.
    changes: u64,
}

/// The lamp's own agent's readiness, kept and recomputed on every change
/// of its [`Facts`].
pub struct Agent {
    kept: Mutex<Kept>,
}

impl Default for Agent {
    fn default() -> Self {
        Agent {
            kept: Mutex::new(Kept {
                facts: Facts::default(),
                state: Readiness::Off,
                why: None,
                since_ms: unix_ms(),
                changes: 0,
            }),
        }
    }
}

impl Agent {
    /// Its facts changed by `change`, read at `now`: the readiness made
    /// again, journaled once when its state or why moved.
    pub fn update_at(&self, now: Instant, change: impl FnOnce(&mut Facts)) {
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        change(&mut kept.facts);
        if matches!(kept.facts.session, Session::Ready | Session::Working) {
            kept.facts.opened_once = true;
        }
        let (state, why) = state_of(&kept.facts, now);
        if (state, why) != (kept.state, kept.why) {
            eprintln!(
                "tasks: agent: {} -> {}{}",
                name(kept.state),
                name(state),
                why.map_or_else(String::new, |why| format!(" ({why})"))
            );
            if state != kept.state {
                kept.since_ms = unix_ms();
            }
            kept.state = state;
            kept.why = why;
            kept.changes = kept.changes.wrapping_add(1);
        }
    }

    /// [`Agent::update_at`] now.
    pub fn update(&self, change: impl FnOnce(&mut Facts)) {
        self.update_at(Instant::now(), change);
    }

    /// The kept session's state now.
    pub fn session(&self, session: Session) {
        self.update(|facts| facts.session = session);
    }

    /// The kept session moved from `from` to `to`, only when it is in one
    /// of `from` now: a turn's end never undoes a fault seen meanwhile.
    pub fn session_from(&self, from: &[Session], to: Session) {
        self.update(|facts| {
            if from.contains(&facts.session) {
                facts.session = to;
            }
        });
    }

    /// The facts as they are now.
    pub fn facts(&self) -> Facts {
        self.kept
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .facts
            .clone()
    }

    /// The state and why now (time passing moves a Gateway down from
    /// starting to `error`: read again at each call).
    pub fn state(&self) -> (Readiness, Option<&'static str>) {
        self.update(|_| {});
        let kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        (kept.state, kept.why)
    }

    /// The value for the bus and the socket, read now.
    pub fn readiness(&self) -> AgentReadiness {
        self.update(|_| {});
        let kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        AgentReadiness {
            engine: if kept.state == Readiness::Off {
                String::new()
            } else {
                kept.facts.engine.clone().unwrap_or_default()
            },
            state: kept.state,
            why: kept.why.map(str::to_owned),
            since_unix_ms: kept.since_ms,
            read_unix_ms: unix_ms(),
        }
    }

    /// The value, when it changed since `seen` (the changes counted when it
    /// was last taken), `seen` moved on.
    pub fn changed(&self, seen: &mut Option<u64>) -> Option<AgentReadiness> {
        let value = self.readiness();
        let changes = self.kept.lock().unwrap_or_else(|e| e.into_inner()).changes;
        (*seen != Some(changes)).then(|| {
            *seen = Some(changes);
            value
        })
    }

    /// The value published once as lamp-tasks stops: `starting` with
    /// [`TASKS_RESTARTING`], so the voice and the app never trust a kept
    /// `ready` while it is down. `None` while nothing is tracked.
    pub fn stopping(&self) -> Option<AgentReadiness> {
        let kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        let engine = kept.facts.engine.clone().filter(|_| kept.facts.on)?;
        Some(AgentReadiness {
            engine,
            state: Readiness::Starting,
            why: Some(TASKS_RESTARTING.to_owned()),
            since_unix_ms: unix_ms(),
            read_unix_ms: unix_ms(),
        })
    }

    /// Whether a task for the lamp's own agent on `engine` is refused at
    /// prepare now ([`NOT_READY`]), with the state: only while tracked, for
    /// the lamp's default engine itself, in any state but `ready` and
    /// `working`. `off` never refuses (nothing is tracked then: the switch
    /// off keeps today's hand-overs).
    pub fn refusal(&self, engine: &str) -> Option<Readiness> {
        let (state, _) = self.state();
        let facts = self.facts();
        (facts.on
            && state != Readiness::Off
            && !state.takes_tasks()
            && facts.engine.as_deref() == Some(engine))
        .then_some(state)
    }
}

/// The lamp's one [`Agent`], shared by the one session, the owner loop and
/// the socket.
pub fn lamp() -> Arc<Agent> {
    static LAMP: OnceLock<Arc<Agent>> = OnceLock::new();
    LAMP.get_or_init(Arc::default).clone()
}

/// A state's name as the bus says it.
pub fn name(state: Readiness) -> &'static str {
    match state {
        Readiness::Off => "off",
        Readiness::NotInstalled => "not_installed",
        Readiness::Installing => "installing",
        Readiness::Starting => "starting",
        Readiness::Ready => "ready",
        Readiness::Working => "working",
        Readiness::Recovering => "recovering",
        Readiness::Error => "error",
    }
}

/// The message of a [`NOT_READY`] refusal in `state`: one fixed line a
/// state, so the socket's error JSON can say the state back
/// ([`state_of_message`]) with no field added to every error.
pub fn message(state: Readiness) -> &'static str {
    match state {
        Readiness::Installing => "The lamp's agent is being installed; nothing was handed over.",
        Readiness::NotInstalled => "The lamp's agent is not installed; nothing was handed over.",
        Readiness::Starting => "The lamp's agent is still starting; nothing was handed over.",
        Readiness::Recovering => {
            "The lamp's agent is recovering from a problem; nothing was handed over."
        }
        Readiness::Error => "The lamp's agent has a problem; nothing was handed over.",
        Readiness::Off | Readiness::Ready | Readiness::Working => {
            "The lamp's agent can't take work now; nothing was handed over."
        }
    }
}

/// The state a [`NOT_READY`] refusal's message says ([`message`]).
pub fn state_of_message(text: &str) -> Option<Readiness> {
    [
        Readiness::Installing,
        Readiness::NotInstalled,
        Readiness::Starting,
        Readiness::Recovering,
        Readiness::Error,
    ]
    .into_iter()
    .find(|state| message(*state) == text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn openclaw() -> Facts {
        Facts {
            on: true,
            engine: Some("openclaw".into()),
            harness: Harness::Here,
            ..Facts::default()
        }
    }

    #[test]
    fn the_state_table_reads_each_input_in_order() {
        let now = Instant::now();
        let mut facts = openclaw();
        assert_eq!(
            state_of(&facts, now),
            (Readiness::Starting, Some("session_starting"))
        );
        facts.session = Session::Ready;
        assert_eq!(state_of(&facts, now), (Readiness::Ready, None));
        facts.session = Session::Working;
        assert_eq!(state_of(&facts, now), (Readiness::Working, None));
        facts.session = Session::Recovering("stuck");
        assert_eq!(
            state_of(&facts, now),
            (Readiness::Recovering, Some("stuck"))
        );
        facts.session = Session::Failed("open_failed");
        assert_eq!(
            state_of(&facts, now),
            (Readiness::Error, Some("open_failed"))
        );
        // The Gateway before the session.
        facts.session = Session::Ready;
        facts.gateway = Gateway::Down(now);
        assert_eq!(
            state_of(&facts, now),
            (Readiness::Starting, Some("gateway_starting"))
        );
        facts.opened_once = true;
        assert_eq!(
            state_of(&facts, now),
            (Readiness::Recovering, Some("gateway_down"))
        );
        assert_eq!(
            state_of(&facts, now + GATEWAY_START_WITHIN),
            (Readiness::Error, Some("gateway_down"))
        );
        // Sign-in, install, Harness and the switch before the Gateway.
        facts.signed_in = Some(false);
        assert_eq!(
            state_of(&facts, now),
            (Readiness::Error, Some("signed_out"))
        );
        facts.installed = Some(false);
        assert_eq!(
            state_of(&facts, now),
            (Readiness::NotInstalled, Some("engine_missing"))
        );
        facts.harness = Harness::Absent;
        assert_eq!(
            state_of(&facts, now),
            (Readiness::NotInstalled, Some("harness_missing"))
        );
        facts.installing = true;
        assert_eq!(
            state_of(&facts, now),
            (Readiness::Installing, Some("installing"))
        );
        facts.on = false;
        assert_eq!(state_of(&facts, now), (Readiness::Off, None));
        // Hermes: no session of lamp-tasks' own, ready once installed.
        let hermes = Facts {
            engine: Some("hermes".into()),
            ..openclaw()
        };
        assert_eq!(state_of(&hermes, now), (Readiness::Ready, None));
    }

    /// #8138 review (1): Harness taking no reply here (`reply_off`) sends
    /// every task the per-task way: `ready`, never refused waiting for a
    /// kept session that is never opened.
    #[test]
    fn a_lamp_with_no_kept_session_reads_ready_and_lets_tasks_through() {
        let agent = Agent::default();
        agent.update(|facts| *facts = openclaw());
        assert_eq!(agent.refusal("openclaw"), Some(Readiness::Starting));
        agent.update(|facts| facts.per_task = true);
        assert_eq!(agent.state(), (Readiness::Ready, None));
        assert_eq!(agent.refusal("openclaw"), None);
        // Its install and sign-in still count.
        agent.update(|facts| facts.signed_in = Some(false));
        assert_eq!(agent.refusal("openclaw"), Some(Readiness::Error));
    }

    /// #8138 review (6): a Gateway cold start of 185-322 s (lamp-9b34,
    /// #7174) reads `starting`, never `error`, until 360 s.
    #[test]
    fn a_slow_gateway_cold_start_is_starting_until_360_s() {
        let now = Instant::now();
        let facts = Facts {
            gateway: Gateway::Down(now),
            ..openclaw()
        };
        for secs in [60, 185, 322, 359] {
            assert_eq!(
                state_of(&facts, now + Duration::from_secs(secs)),
                (Readiness::Starting, Some("gateway_starting")),
                "{secs} s"
            );
        }
        assert_eq!(
            state_of(&facts, now + Duration::from_secs(360)),
            (Readiness::Error, Some("gateway_down"))
        );
    }

    /// #8138 review (7): lamp-tasks stopping publishes `starting` with
    /// `tasks_restarting`, a value the bus's check takes; nothing while
    /// untracked.
    #[test]
    fn stopping_reads_starting_tasks_restarting_and_passes_the_check() {
        let agent = Agent::default();
        assert!(agent.stopping().is_none());
        agent.update(|facts| *facts = openclaw());
        agent.session(Session::Ready);
        let stopping = agent.stopping().unwrap();
        assert_eq!(
            (stopping.state, stopping.why.as_deref()),
            (Readiness::Starting, Some(TASKS_RESTARTING))
        );
        assert_eq!(stopping.engine, "openclaw");
        assert!(stopping.check().is_ok(), "{:?}", stopping.check());
    }

    #[test]
    fn each_change_is_counted_once_with_its_since() {
        let agent = Agent::default();
        let mut seen = None;
        assert_eq!(agent.changed(&mut seen).unwrap().state, Readiness::Off);
        assert!(agent.changed(&mut seen).is_none());
        agent.update(|facts| *facts = openclaw());
        let starting = agent.changed(&mut seen).unwrap();
        assert_eq!(starting.state, Readiness::Starting);
        assert!(starting.check().is_ok());
        agent.session(Session::Ready);
        let ready = agent.changed(&mut seen).unwrap();
        assert_eq!(
            (ready.state, ready.why.as_deref()),
            (Readiness::Ready, None)
        );
        assert!(ready.check().is_ok());
        // A turn's end after a fault leaves the fault.
        agent.session(Session::Recovering("engine_not_ready"));
        agent.session_from(&[Session::Working], Session::Ready);
        assert_eq!(
            agent.state(),
            (Readiness::Recovering, Some("engine_not_ready"))
        );
    }

    #[test]
    fn only_the_tracked_default_engine_not_taking_tasks_is_refused() {
        let agent = Agent::default();
        // Off: nothing tracked, nothing refused.
        assert_eq!(agent.refusal("openclaw"), None);
        agent.update(|facts| *facts = openclaw());
        assert_eq!(agent.refusal("openclaw"), Some(Readiness::Starting));
        assert_eq!(agent.refusal("claude"), None);
        agent.session(Session::Working);
        assert_eq!(agent.refusal("openclaw"), None);
        agent.session(Session::Recovering("stuck"));
        assert_eq!(agent.refusal("openclaw"), Some(Readiness::Recovering));
        for state in [
            Readiness::Installing,
            Readiness::NotInstalled,
            Readiness::Starting,
            Readiness::Recovering,
            Readiness::Error,
        ] {
            assert_eq!(state_of_message(message(state)), Some(state));
        }
    }
}
