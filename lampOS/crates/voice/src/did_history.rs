//! The last hour of what the lamp did, and why (`lamp/did`, #7757), kept in
//! memory by the voice so "why did you do that?" and "why did you do that
//! earlier?" can be answered by looking it up (#7751 reads it through
//! [`kept`]). At most [`MOST`] actions and [`WITHIN_US`] of them, the oldest
//! dropped first by count, then by age. Nothing is written to disk and
//! nothing survives a restart. Only with `LAMP_DID_KEEP=on` (off: nothing
//! is subscribed or kept). It holds no word a person said: a did carries a
//! plain fact as its reason ([`lamp_msgs::LampDid`]), and nothing else
//! reads this, not the cloud, telemetry, memory or any skill.
use lamp_msgs::LampDid;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_DID_KEEP";
/// The one publisher whose dids are kept.
pub const TELLER: &str = "skills";
/// The most actions kept.
pub const MOST: usize = 200;
/// How long an action is kept, in microseconds on the capture clock.
pub const WITHIN_US: u64 = 3_600_000_000;

/// Whether [`ENVIRONMENT`] is `on`.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

#[derive(Debug, Default)]
pub struct DidHistory {
    entries: VecDeque<LampDid>,
}

impl DidHistory {
    /// Keep `did`, told at `now_us`: older than an hour, or the 201st, goes.
    /// A did that fails its own check is not kept.
    pub fn keep(&mut self, did: LampDid, now_us: u64) {
        if did.check().is_err() {
            return;
        }
        self.entries.push_back(did);
        self.trim(now_us);
    }

    fn trim(&mut self, now_us: u64) {
        while self.entries.len() > MOST {
            self.entries.pop_front();
        }
        while self
            .entries
            .front()
            .is_some_and(|did| now_us.saturating_sub(did.at_us) > WITHIN_US)
        {
            self.entries.pop_front();
        }
    }

    /// The newest `n` actions still inside the hour at `now_us`, newest first.
    pub fn newest(&mut self, n: usize, now_us: u64) -> Vec<LampDid> {
        self.trim(now_us);
        self.entries.iter().rev().take(n).cloned().collect()
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
    let mut did = bus
        .subscribe_typed(LampDid::TOPIC, "voice", LampDid::TYPE)
        .ok()?;
    let history = KEPT
        .get_or_init(|| Arc::new(Mutex::new(DidHistory::default())))
        .clone();
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
                if message.header.source != TELLER {
                    continue;
                }
                let Ok(did) = message.parse::<LampDid>() else {
                    continue;
                };
                let Some(kept) = kept.upgrade() else {
                    return;
                };
                if let Ok(mut history) = kept.lock() {
                    history.keep(did, lamp_ipc::monotonic_us());
                }
            }
        })
        .ok()?;
    Some(history)
}

#[cfg(test)]
mod tests;
