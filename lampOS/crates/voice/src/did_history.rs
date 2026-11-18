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
/// added with its own part's PR (#7757): lamp-skills, and the body
/// (`LAMP_DID_BODY`). Never "any".
pub const TELLERS: [&str; 2] = ["skills", BODY];
/// The body's publishing name, whose dids have a share of their own.
const BODY: &str = "body";
/// The most actions kept.
pub const MOST: usize = 200;
/// The most of them the body's own may be (#7757): with `LAMP_BODY_PRESENCE`
/// on it shifts about 90 times an hour and turns to a call every 10 s, which
/// would push the skills' dids out of a shared first-in first-out. Its oldest
/// goes first; the skills' keep at least `MOST - BODY_MOST`.
pub const BODY_MOST: usize = 100;
/// How long an action is kept, in microseconds on the capture clock.
pub const WITHIN_US: u64 = 3_600_000_000;

/// Whether [`ENVIRONMENT`] is `on`.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

#[derive(Debug, Default)]
pub struct DidHistory {
    /// Oldest first, each with whether the body told it.
    entries: VecDeque<(LampDid, bool)>,
}

impl DidHistory {
    /// Keep `did`, told at `now_us`: older than an hour, or the 201st, goes.
    /// A did that fails its own check is not kept.
    pub fn keep(&mut self, did: LampDid, now_us: u64) {
        self.keep_from(did, false, now_us);
    }

    /// The same, saying whether the body told it: the body's share is at most
    /// [`BODY_MOST`], its own oldest dropped first, and past [`MOST`] in all
    /// the oldest of the part over its share goes (the skills' share is
    /// `MOST - BODY_MOST`), so neither can push the other out.
    pub fn keep_from(&mut self, did: LampDid, from_body: bool, now_us: u64) {
        if did.check().is_err() {
            return;
        }
        self.entries.push_back((did, from_body));
        while self.entries.iter().filter(|(_, body)| *body).count() > BODY_MOST {
            if let Some(oldest) = self.entries.iter().position(|(_, body)| *body) {
                self.entries.remove(oldest);
            }
        }
        self.trim(now_us);
    }

    fn trim(&mut self, now_us: u64) {
        while self.entries.len() > MOST {
            let others = self.entries.iter().filter(|(_, body)| !*body).count();
            // Past its share, the skills' oldest goes; else the body's.
            let drop_others = others > MOST - BODY_MOST;
            let oldest = self
                .entries
                .iter()
                .position(|(_, body)| *body != drop_others);
            match oldest {
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
                let from_body = message.header.source == BODY;
                let Ok(did) = message.parse::<LampDid>() else {
                    continue;
                };
                let Some(kept) = kept.upgrade() else {
                    return;
                };
                if let Ok(mut history) = kept.lock() {
                    history.keep_from(did, from_body, lamp_ipc::monotonic_us());
                }
            }
        })
        .ok()?;
    Some(history)
}

#[cfg(test)]
mod tests;
