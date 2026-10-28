//! A set-up lamp whose saved network is gone opens setup's own hotspot after
//! 90 s, so a new router never needs a reset (#3020, `docs/setup.md`, "When
//! the saved network is gone"). Off unless `LAMP_WIFI_RECOVERY=on`.
//!
//! The policy here is pure: [`Recovery::step`] takes what was seen and the
//! time, and says what to do. V1 did the same in the parent repository's
//! `system/network/recovery.go` (90 s lost, a retry every 2 min with no
//! phone on the hotspot, 45 s to join), and `crates/surround`'s `Recovery`
//! has its shape. Stricter than V1: a lamp only slow to get its address
//! (joined to a known network's radio) never opens the hotspot, nor while
//! setup or a Wi-Fi change runs. Its own network still in reach but not
//! joining (a changed password) is gone: that is a case it is for.

/// How long the lamp must be off every known network, and not joining
/// one, before the hotspot opens: V1's `wifiLossGrace`.
pub const LOSS_GRACE_MS: u64 = 90_000;
/// How often, with the hotspot open and no phone on it, the saved network
/// is tried again: V1's.
pub const RETRY_EVERY_MS: u64 = 120_000;
/// How long past its due time a phone on the hotspot may hold a retry
/// (El's P2 on #3024): a phone that remembers the open hotspot from setup
/// may join it by itself while the router reboots, and would otherwise
/// keep the lamp off its network all night. A setup run holds it always.
pub const PHONE_HOLDS_MS: u64 = 600_000;
/// How long a try of the saved network has to join: V1's.
pub const JOIN_WAIT_MS: u64 = 45_000;
/// Kept while `lamp-setup recover` runs with its switch on (its unit's
/// runtime folder, gone when it ends): lamp-cloud reads it for
/// `system.info`'s `recovery` (#3020), so the app names the hotspot only
/// to a lamp that will open one.
pub const ON_FILE: &str = "/run/lampos-setup-recover/on";
/// What setup's status says while recovering (`runtime_phase`).
pub const RECOVERING: &str = "recovering";
/// The switch, exactly `on`.
pub const SWITCH: &str = "LAMP_WIFI_RECOVERY";
/// What the lamp says as the hotspot first opens for a loss (#3743, agreed
/// with Sixth on #3020): what happened and what to do, with no control named
/// and nothing erased. With no network the voice's model can't be reached,
/// so it is only ever [`RECORDING_OPENED`], recorded at build time in the
/// lamp's own voice; this is its transcript. Under 10 s, so any loader
/// takes the clip.
pub const SAY_OPENED: &str =
    "I can't reach my Wi-Fi, so I've opened setup. Set my Wi-Fi again in the app.";
/// [`SAY_OPENED`]'s recording, by the name the say service takes, the
/// failure notice `wifi_recovering` (`docs/failure-notices.md`).
pub const RECORDING_OPENED: &str = "wifi_recovering";

/// Whether the switch's value turns recovery on.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// One look at the lamp's Wi-Fi.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// A station address on the Wi-Fi: joined to a network the saved file
    /// names (the saved one, or another known one wpa_supplicant took).
    pub online: bool,
    /// Joined to a known network's radio (`wpa_state=COMPLETED`), its
    /// address not yet there: only slow. A wrong key never gets here.
    pub associated: bool,
    /// Setup itself, or a Wi-Fi change from the app, is running.
    pub busy: bool,
    /// Phones on the hotspot; None when they can't be counted, which counts
    /// as some.
    pub phones: Option<u16>,
}

/// What to do now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Open setup's hotspot and page (`device-ap-mode`), saying why.
    OpenHotspot,
    /// Close it and try the saved network (`device-sta-mode`).
    Rejoin,
}

/// Where the lamp is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Online,
    /// The saved network gone since.
    Lost {
        since_ms: u64,
    },
    /// The hotspot open; the saved network tried again from then.
    Hotspot {
        retry_ms: u64,
    },
    /// Trying the saved network until then.
    Rejoining {
        until_ms: u64,
    },
}

/// The policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recovery {
    state: State,
    /// [`SAY_OPENED`] was said for this loss: kept until the lamp is online
    /// again, so neither a reopening after a failed retry nor one after a
    /// retry that went back to `Lost` (only slow, or busy) says it again
    /// (Second's P3 on #4157).
    told: bool,
}

impl Default for Recovery {
    fn default() -> Self {
        Self {
            state: State::Online,
            told: false,
        }
    }
}

impl Recovery {
    pub fn state(&self) -> State {
        self.state
    }

    /// Whether the lamp's status should say `recovering`: the hotspot open,
    /// or a try of the saved network from it.
    pub fn recovering(&self) -> bool {
        matches!(self.state, State::Hotspot { .. } | State::Rejoining { .. })
    }

    /// Whether `act`, the step's own, is the hotspot's first opening for
    /// this loss: [`SAY_OPENED`] is said then, once a loss, until the lamp
    /// is online again.
    pub fn says_opened(&mut self, act: Option<Act>) -> bool {
        let first = act == Some(Act::OpenHotspot) && !self.told;
        self.told |= first;
        first
    }

    /// What `seen` at `now_ms` asks for.
    pub fn step(&mut self, now_ms: u64, seen: Seen) -> Option<Act> {
        let (state, act) = match self.state {
            _ if seen.online => (State::Online, None),
            State::Online => (State::Lost { since_ms: now_ms }, None),
            // Slow, or someone else at the radio: the 90 s start again, so
            // only a whole 90 s off every known network opens it.
            State::Lost { .. } if seen.associated || seen.busy => {
                (State::Lost { since_ms: now_ms }, None)
            }
            State::Lost { since_ms } if now_ms.saturating_sub(since_ms) >= LOSS_GRACE_MS => (
                State::Hotspot {
                    retry_ms: now_ms.saturating_add(RETRY_EVERY_MS),
                },
                Some(Act::OpenHotspot),
            ),
            State::Lost { since_ms } => (State::Lost { since_ms }, None),
            // Every 2 min, never during a setup run, and from under a
            // phone only once it has held the retry 10 min. Not sooner
            // when the saved network shows in a scan: with a changed
            // password it always does, and the hotspot would never stay up.
            State::Hotspot { retry_ms }
                if !seen.busy
                    && now_ms >= retry_ms
                    && (seen.phones == Some(0)
                        || now_ms.saturating_sub(retry_ms) >= PHONE_HOLDS_MS) =>
            {
                (
                    State::Rejoining {
                        until_ms: now_ms.saturating_add(JOIN_WAIT_MS),
                    },
                    Some(Act::Rejoin),
                )
            }
            State::Hotspot { retry_ms } => (State::Hotspot { retry_ms }, None),
            // Joined to the radio and waiting for an address, or someone
            // else at the radio (a Wi-Fi change): never cut back to the
            // hotspot, as it is never opened from Lost for those. It is
            // lost afresh, so once that clears the whole 90 s run again
            // before the hotspot (reviewer-two on #3024).
            State::Rejoining { .. } if seen.associated || seen.busy => {
                (State::Lost { since_ms: now_ms }, None)
            }
            State::Rejoining { until_ms } if now_ms >= until_ms => (
                State::Hotspot {
                    retry_ms: now_ms.saturating_add(RETRY_EVERY_MS),
                },
                Some(Act::OpenHotspot),
            ),
            State::Rejoining { until_ms } => (State::Rejoining { until_ms }, None),
        };
        if state == State::Online {
            self.told = false;
        }
        self.state = state;
        act
    }
}
