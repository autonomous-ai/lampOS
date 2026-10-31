//! Asking the lamp to turn off (#3697, `docs/shutdown.md`): one request
//! message, whoever asks (the primary button held, the app), which only
//! `lamp-setup watch-reset` acts on, as root, by starting `lamp-setup
//! shutdown`. No asker runs a root command itself.
use crate::{ClockStamp, Invalid};
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The switch for the ways a person asks to turn the lamp off (#3697,
/// `docs/shutdown.md`): `on` exactly; off unless set. Read by Controls (the
/// primary button's hold), Voice's ring worker (the warning while held),
/// lamp-cloud (the app's turn off) and `lamp-setup watch-reset` (which
/// starts the shutdown).
pub const SHUTDOWN_SWITCH: &str = "LAMP_SHUTDOWN";
/// The primary button released after this long asks to turn the lamp off,
/// with [`SHUTDOWN_SWITCH`] on (V1's shutdown hold, HAL-5.7).
pub const SHUTDOWN_HOLD_US: u64 = 5_000_000;
/// Released after this long, the shutdown is cancelled: nothing is asked.
pub const SHUTDOWN_HOLD_MOST_US: u64 = 15_000_000;

/// Whether [`SHUTDOWN_SWITCH`] is on in this process's environment.
pub fn shutdown_switched_on() -> bool {
    std::env::var(SHUTDOWN_SWITCH).as_deref() == Ok("on")
}

/// Whether a primary-button hold of `held_us` asks to turn the lamp off:
/// from [`SHUTDOWN_HOLD_US`] to [`SHUTDOWN_HOLD_MOST_US`], both included.
pub fn shutdown_hold(held_us: u64) -> bool {
    (SHUTDOWN_HOLD_US..=SHUTDOWN_HOLD_MOST_US).contains(&held_us)
}

/// The shutdown's topic, separate from the original seventeen.
pub fn shutdown_topics() -> Vec<(&'static str, Declaration)> {
    vec![(ShutdownRequest::TOPIC, ShutdownRequest::declaration())]
}

/// A request older than this is never acted on.
pub const SHUTDOWN_REQUEST_TTL_US: u64 = 2_000_000;

/// Who asks to turn the lamp off. Each is accepted only from its own bus
/// publisher ([`ShutdownSource::publisher`]); a name this build does not
/// know does not parse, and is never acted on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownSource {
    /// The primary button released after a hold in the shutdown window
    /// ([`shutdown_hold`]), from `lamp-controls`.
    Button,
}

impl ShutdownSource {
    /// The bus publisher a request from this source must come from.
    pub const fn publisher(self) -> &'static str {
        match self {
            Self::Button => "controls",
        }
    }
}

/// "Turn the lamp off", on [`ShutdownRequest::TOPIC`]. The stamp is the
/// asker's boot clock (`/proc/sys/kernel/random/boot_id`), as Controls'.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownRequest {
    pub stamp: ClockStamp,
    pub source: ShutdownSource,
    /// Why, for the journal: the button's hold.
    pub reason: String,
}

impl ShutdownRequest {
    pub const TYPE: &str = "lamp_msgs/ShutdownRequest";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "lamp/shutdown_request";
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
            .lasting(Duration::from_micros(SHUTDOWN_REQUEST_TTL_US))
    }
    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        if self.reason.is_empty() || self.reason.len() > 200 {
            return Err(Invalid("shutdown reason must be 1..200 bytes".into()));
        }
        Ok(())
    }
    /// Whether to act on it now: valid, from its source's own publisher
    /// (`publisher`, the bus header's), on this boot's clock and fresh.
    /// Source-agnostic: any [`ShutdownSource`] is taken the same way. The
    /// switch is the acting watcher's own to check.
    pub fn accept(&self, publisher: &str, clock_id: &str, now_us: u64) -> Result<(), &'static str> {
        self.check().map_err(|_| "invalid shutdown request")?;
        if publisher != self.source.publisher() {
            return Err("not from its source's publisher");
        }
        if self.stamp.clock_id != clock_id {
            return Err("another boot's clock");
        }
        let age = now_us
            .checked_sub(self.stamp.at_us)
            .ok_or("from the future")?;
        if age >= SHUTDOWN_REQUEST_TTL_US {
            return Err("stale");
        }
        Ok(())
    }
}
