//! The ring's side of `lamp/did` (#7757, `LAMP_DID_RING=on`, off): the four
//! faces the ring shows on its own, each told once as it begins, with what
//! the ring measured as the reason. The conversation's own faces (listening,
//! thinking, speaking) follow the person's speech and are not told, nor is a
//! light a skill or the person asked for.
//!
//! Nothing here runs on a ring's frame loop: an edge (a face's first frame
//! written) hands one small [`Face`] to a thread of its own (`try_send`, room
//! for a few, the rest let go and counted), and that thread makes the did and
//! publishes it, at most one in [`EVERY`]. A reason is a fixed sentence; the
//! privacy one says only THAT the lamp was private, never anything from
//! while it was.
use lamp_bus::Bus;
use lamp_msgs::{DidOutcome, DidWhat, LampDid};
use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

/// `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_DID_RING";
/// The name the ring tells under: the keeper's list names it.
pub const SOURCE: &str = "ring";
/// The most often a did is told: the rest are let go and counted.
pub const EVERY: Duration = Duration::from_secs(2);
/// A face seen again within this long is the same face going on, not a new
/// one begun ([`Episode`]).
pub const GAP_US: u64 = 2_000_000;

/// Whether [`ENVIRONMENT`] is `on`.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// The faces the ring shows on its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Face {
    /// While the lamp cannot hear: a hand's mute or the privacy switch closed.
    Asleep,
    /// The night's sleep: Expression asleep, the ring easing into the glow.
    NightSleep,
    /// The unplug picture, drawn once the writer accepted its frame.
    Unplug,
    /// The dim red breath for the physical mute at the switch.
    PrivacyMuted,
}

impl Face {
    /// Who decided.
    pub fn who(self) -> &'static str {
        match self {
            Face::Asleep => "lamp-ring:asleep",
            Face::NightSleep => "lamp-ring:night-sleep",
            Face::Unplug => "lamp-ring:unplug",
            Face::PrivacyMuted => "lamp-ring:privacy-muted",
        }
    }

    /// Why, in fixed words: what the ring measured, nothing heard.
    pub fn why(self) -> &'static str {
        match self {
            Face::Asleep => "the lamp cannot hear: it is muted or the privacy switch is closed",
            Face::NightSleep => "it is night and Expression is asleep",
            Face::Unplug => "the lamp was unplugged",
            Face::PrivacyMuted => "the microphone is off at the switch",
        }
    }
}

/// The did a face's beginning is told as, at `at_us`.
pub fn did(face: Face, at_us: u64) -> LampDid {
    LampDid {
        who: face.who().to_owned(),
        what: DidWhat::Lit,
        why: face.why().to_owned(),
        at_us,
        outcome: DidOutcome::Done,
    }
}

/// Whether a face drawn at `now_us` begins a new showing: it does when it was
/// not seen within [`GAP_US`]. Kept by a face that is drawn again and again.
#[derive(Debug, Default)]
pub struct Episode {
    last_us: Option<u64>,
}

impl Episode {
    /// True the first time, and again after a gap.
    pub fn seen(&mut self, now_us: u64) -> bool {
        let begins = self
            .last_us
            .is_none_or(|last| now_us.saturating_sub(last) > GAP_US);
        self.last_us = Some(now_us);
        begins
    }
}

static LET_GO: AtomicU64 = AtomicU64::new(0);
static OUT: OnceLock<Option<mpsc::SyncSender<Face>>> = OnceLock::new();

/// How many dids were let go: no room, or more than one in [`EVERY`].
pub fn let_go() -> u64 {
    LET_GO.load(Ordering::Relaxed)
}

/// Tell `face` began. Off: nothing at all. On: one `try_send` to the thread
/// that publishes (started on the first call; the bus is opened there, never
/// by the caller), never waited on.
pub fn told(face: Face) {
    if !on() {
        return;
    }
    let out = OUT.get_or_init(|| {
        let (to_thread, from_callers) = mpsc::sync_channel::<Face>(8);
        let spawned = std::thread::Builder::new()
            .name("did-ring".into())
            .spawn(move || publish(from_callers))
            .is_ok();
        spawned.then_some(to_thread)
    });
    if let Some(out) = out
        && out.try_send(face).is_err()
    {
        LET_GO.fetch_add(1, Ordering::Relaxed);
    }
}

/// The publishing thread: one did for each face handed over, at most one in
/// [`EVERY`]. A bus that cannot be used ends it, and then nothing is told.
fn publish(faces: mpsc::Receiver<Face>) {
    let Ok(bus) = Bus::from_environment() else {
        return;
    };
    publish_on(&bus, faces);
}

/// [`publish`] on `bus`.
fn publish_on(bus: &Bus, faces: mpsc::Receiver<Face>) {
    let Ok(mut out) = bus.publisher_declared(LampDid::TOPIC, &LampDid::declaration(), SOURCE)
    else {
        return;
    };
    let mut last: Option<Instant> = None;
    for face in faces {
        let now = Instant::now();
        if last.is_some_and(|last| now.duration_since(last) < EVERY) {
            LET_GO.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        last = Some(now);
        let told = did(face, lamp_ipc::monotonic_us());
        if told.check().is_ok() {
            let _ = out.publish_json(lamp_ipc::monotonic_us(), &told);
        }
    }
}

#[cfg(test)]
mod tests;
