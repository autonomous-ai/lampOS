//! The voice's side of `lamp/did` (#7757, `LAMP_DID_VOICE=on`, off): the
//! lines the voice starts itself, with nobody speaking to it and no skill
//! having asked, each told once as its first audio is queued: news of a task
//! it tells itself, a step of the first minutes with a new person, and the
//! update line. A skill's say, a reminder and an answer are not told here (the
//! say service's own says are lamp-skills' to tell).
//!
//! Nothing here runs on the audio path: the edge hands one small [`Moment`]
//! to a thread of its own (`try_send`, room for a few, the rest let go and
//! counted), and that thread makes the did and publishes it, at most one in
//! [`EVERY`]. A reason is a fixed sentence: no task's text, no agent's answer,
//! no name (not the agent's either), nothing that was said; only that a
//! task has news, or which step of the walk, or that an update is under way.
use lamp_bus::Bus;
use lamp_msgs::{DidOutcome, DidWhat, LampDid};
use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

/// `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_DID_VOICE";
/// The name the voice tells under: the keeper's list names it.
pub const SOURCE: &str = "voice";
/// The most often a did is told: the rest are let go and counted.
pub const EVERY: Duration = Duration::from_secs(2);

/// Whether [`ENVIRONMENT`] is `on`.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// A line the voice began itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Moment {
    /// News of a task the lamp tells on its own.
    TaskNews,
    /// A line of the first minutes with a new person: `Some(n)` is the
    /// step's number in the walk's order (never a word it said), `None` a
    /// line with no step (its close, its hello).
    Onboarding(Option<u8>),
    /// The line about the software update (and its done line).
    UpdateLine,
    /// A line about the lamp's agent switch, said in a conversation
    /// (`agent_switch_told`).
    AgentSwitch,
}

impl Moment {
    /// Who decided.
    pub fn who(self) -> &'static str {
        match self {
            Moment::TaskNews => "lamp-voice:task-news",
            Moment::Onboarding(_) => "lamp-voice:onboarding",
            Moment::UpdateLine => "lamp-voice:update-line",
            Moment::AgentSwitch => "lamp-voice:agent-switch",
        }
    }

    /// Why, in fixed words: the step's number at most, nothing said.
    pub fn why(self) -> String {
        match self {
            Moment::TaskNews => "a task it handed over has news to tell".into(),
            Moment::Onboarding(Some(step)) => {
                format!("the first minutes with a new person: step {step}")
            }
            Moment::Onboarding(None) => "the first minutes with a new person".into(),
            Moment::UpdateLine => "the lamp's software is being updated".into(),
            Moment::AgentSwitch => "the lamp's agent is being switched".into(),
        }
    }
}

/// The did a line's first audio is told as, at `at_us`.
pub fn did(moment: Moment, at_us: u64) -> LampDid {
    LampDid {
        who: moment.who().to_owned(),
        what: DidWhat::Spoke,
        why: moment.why(),
        at_us,
        outcome: DidOutcome::Done,
    }
}

static LET_GO: AtomicU64 = AtomicU64::new(0);
static OUT: OnceLock<Option<mpsc::SyncSender<Moment>>> = OnceLock::new();

/// How many dids were let go: no room, or more than one in [`EVERY`].
pub fn let_go() -> u64 {
    LET_GO.load(Ordering::Relaxed)
}

/// Tell `moment`'s first audio was queued. Off: nothing at all. On: one
/// `try_send` to the thread that publishes ([`ready`] starts it with the
/// coordinator; the bus is opened there, never by the caller), never waited on.
pub fn told(moment: Moment) {
    if !on() {
        return;
    }
    if let Some(out) = sender()
        && out.try_send(moment).is_err()
    {
        LET_GO.fetch_add(1, Ordering::Relaxed);
    }
}

/// The thread's sender, the thread started on the first call.
fn sender() -> Option<&'static mpsc::SyncSender<Moment>> {
    OUT.get_or_init(|| {
        let (to_thread, from_callers) = mpsc::sync_channel::<Moment>(8);
        let spawned = std::thread::Builder::new()
            .name("did-voice".into())
            .spawn(move || publish(from_callers))
            .is_ok();
        spawned.then_some(to_thread)
    })
    .as_ref()
}

/// Start the publishing thread now, so the first [`told`] never spawns it on
/// the audio path. Off: nothing at all.
pub fn ready() {
    if on() {
        let _ = sender();
    }
}

/// The publishing thread: one did for each moment handed over, at most one
/// in [`EVERY`]. A bus that cannot be used ends it, and then nothing is told.
fn publish(moments: mpsc::Receiver<Moment>) {
    let Ok(bus) = Bus::from_environment() else {
        return;
    };
    publish_on(&bus, moments);
}

/// [`publish`] on `bus`.
fn publish_on(bus: &Bus, moments: mpsc::Receiver<Moment>) {
    let Ok(mut out) = bus.publisher_declared(LampDid::TOPIC, &LampDid::declaration(), SOURCE)
    else {
        return;
    };
    let mut last: Option<Instant> = None;
    for moment in moments {
        let now = Instant::now();
        if last.is_some_and(|last| now.duration_since(last) < EVERY) {
            LET_GO.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        last = Some(now);
        let told = did(moment, lamp_ipc::monotonic_us());
        if told.check().is_ok() {
            let _ = out.publish_json(lamp_ipc::monotonic_us(), &told);
        }
    }
}

#[cfg(test)]
mod tests;
