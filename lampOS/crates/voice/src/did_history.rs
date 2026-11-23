//! The last hour of what the lamp did, and why (`lamp/did`, #7757), kept in
//! memory by the voice so "why did you do that?" and "why did you do that
//! earlier?" can be answered by looking it up (#7751 reads it through
//! [`kept`]). At most [`MOST`] actions and [`WITHIN_US`] of them, the oldest
//! dropped first by count, then by age. Nothing is written to disk and
//! nothing survives a restart. Only with `LAMP_DID_KEEP=on` (off: nothing
//! is subscribed or kept). It holds no word a person said: a did carries a
//! plain fact as its reason ([`lamp_msgs::LampDid`]), and nothing else
//! reads this, not the cloud, telemetry, memory or any skill. The body's own
//! dids have a share of at most [`BODY_MOST`] of them.
use lamp_msgs::LampDid;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_DID_KEEP";
/// The publishers whose dids are kept, by the name they publish under, each
/// added with its own part's PR (#7757): lamp-skills, the body
/// (`LAMP_DID_BODY`), the ring (`LAMP_DID_RING`) and the voice
/// (`LAMP_DID_VOICE`). Never "any".
pub const TELLERS: [&str; 4] = ["skills", BODY, RING, VOICE];
/// The body's publishing name, whose dids have a share of their own.
const BODY: &str = "body";
/// The ring's publishing name ([`crate::did_ring::SOURCE`]).
const RING: &str = "ring";
/// The voice's publishing name ([`crate::did_voice::SOURCE`]).
const VOICE: &str = "voice";
/// The most actions kept.
pub const MOST: usize = 200;
/// The most of them the body's own may be (#7757): with `LAMP_BODY_PRESENCE`
/// on it shifts about 90 times an hour and turns to a call every 10 s, which
/// would push the skills' dids out of a shared first-in first-out. Its oldest
/// goes first; the skills' keep at least `MOST - BODY_MOST`.
pub const BODY_MOST: usize = 100;
/// The most of them the ring's own may be: it tells a few faces a day.
pub const RING_MOST: usize = 20;
/// The most of them the voice's own may be: it tells the lines it begins
/// itself (a few news, the walk's steps, one update line).
pub const VOICE_MOST: usize = 20;

/// Which part told a did: the share it is kept under.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Part {
    Skills,
    Body,
    Ring,
    Voice,
}

impl Part {
    /// The part that publishes under `source`, if it is one of the named.
    pub fn of(source: &str) -> Option<Self> {
        match source {
            "skills" => Some(Self::Skills),
            BODY => Some(Self::Body),
            RING => Some(Self::Ring),
            VOICE => Some(Self::Voice),
            _ => None,
        }
    }
}
/// How long an action is kept, in microseconds on the capture clock.
pub const WITHIN_US: u64 = 3_600_000_000;

/// Whether [`ENVIRONMENT`] is `on`.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

#[derive(Debug, Default)]
pub struct DidHistory {
    /// Oldest first, each with the part that told it.
    entries: VecDeque<(LampDid, Part)>,
}

impl DidHistory {
    /// Keep `did`, told at `now_us`: older than an hour, or the 201st, goes.
    /// A did that fails its own check is not kept.
    pub fn keep(&mut self, did: LampDid, now_us: u64) {
        self.keep_part(did, Part::Skills, now_us);
    }

    /// The same, saying which part told it: the body's share is at most
    /// [`BODY_MOST`] and the ring's [`RING_MOST`], the voice's [`VOICE_MOST`], each part's own oldest
    /// dropped first; the parts that are not skills hold at most
    /// `BODY_MOST` of the whole between them, so past [`MOST`] their oldest
    /// goes (the body's, then the ring's, then the voice's) and the skills keep at least
    /// `MOST - BODY_MOST`: no part can push another out.
    pub fn keep_part(&mut self, did: LampDid, part: Part, now_us: u64) {
        if did.check().is_err() {
            return;
        }
        self.entries.push_back((did, part));
        for (own, most) in [
            (Part::Body, BODY_MOST),
            (Part::Ring, RING_MOST),
            (Part::Voice, VOICE_MOST),
        ] {
            while self.count(own) > most {
                if let Some(oldest) = self.entries.iter().position(|(_, kept)| *kept == own) {
                    self.entries.remove(oldest);
                }
            }
        }
        self.trim(now_us);
    }

    fn count(&self, part: Part) -> usize {
        self.entries
            .iter()
            .filter(|(_, kept)| *kept == part)
            .count()
    }

    fn trim(&mut self, now_us: u64) {
        while self.entries.len() > MOST {
            let others = self.entries.len() - self.count(Part::Skills);
            // Past their share together, the body's oldest goes, then the
            // ring's, then the voice's; else the skills'.
            let drop = if others > BODY_MOST {
                [Part::Body, Part::Ring, Part::Voice]
                    .into_iter()
                    .find(|part| self.count(*part) > 0)
                    .unwrap_or(Part::Skills)
            } else {
                Part::Skills
            };
            match self.entries.iter().position(|(_, kept)| *kept == drop) {
                Some(at) => self.entries.remove(at),
                None => self.entries.pop_front(),
            };
        }
        while self
            .entries
            .front()
            .is_some_and(|(did, _)| now_us.saturating_sub(did.at_us) > WITHIN_US)
        {
            self.entries.pop_front();
        }
    }

    /// The newest `n` actions still inside the hour at `now_us`, newest first.
    pub fn newest(&mut self, n: usize, now_us: u64) -> Vec<LampDid> {
        self.trim(now_us);
        self.entries
            .iter()
            .rev()
            .take(n)
            .map(|(did, _)| did.clone())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

static KEPT: OnceLock<Arc<Mutex<DidHistory>>> = OnceLock::new();

/// The history, if the voice keeps one (`LAMP_DID_KEEP=on` and a bus).
pub fn kept() -> Option<Arc<Mutex<DidHistory>>> {
    KEPT.get().cloned()
}

/// Start keeping: a thread of its own reads `lamp/did` and keeps what it
/// says; a turn reads [`kept`] and never the bus. The bus's publisher never
/// waits for it. A bus that cannot be read keeps nothing.
pub fn watch(bus: &lamp_bus::Bus) -> Option<Arc<Mutex<DidHistory>>> {
    let history = KEPT
        .get_or_init(|| Arc::new(Mutex::new(DidHistory::default())))
        .clone();
    watch_into(bus, history)
}

/// [`watch`], keeping into `history`.
fn watch_into(
    bus: &lamp_bus::Bus,
    history: Arc<Mutex<DidHistory>>,
) -> Option<Arc<Mutex<DidHistory>>> {
    let mut did = bus
        .subscribe_typed(LampDid::TOPIC, "voice", LampDid::TYPE)
        .ok()?;
    let kept = Arc::downgrade(&history);
    std::thread::Builder::new()
        .name("did-history".into())
        .spawn(move || {
            while let Ok(message) = did.recv(Duration::from_secs(1)) {
                let Some(message) = message else { continue };
                // A filter on who published, not a proof of who did: the
                // bus names the source its publisher declared, so this keeps
                // other parts' dids out by name (#7757). The fence in
                // lamp-msgs' stays_home test is what keeps them from being
                // told at all.
                if !TELLERS.contains(&message.header.source.as_str()) {
                    continue;
                }
                let Some(part) = Part::of(&message.header.source) else {
                    continue;
                };
                let Ok(did) = message.parse::<LampDid>() else {
                    continue;
                };
                let Some(kept) = kept.upgrade() else {
                    return;
                };
                if let Ok(mut history) = kept.lock() {
                    history.keep_part(did, part, lamp_ipc::monotonic_us());
                }
            }
        })
        .ok()?;
    Some(history)
}

#[cfg(test)]
mod tests;
