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
/// A [`RECORDING_OPENED`] not played (refused: the lamp speaking, a higher
/// say, the voice not up yet; or no say service) is asked again this long
/// after, the wait doubling each time (#5550): at 5 s, 15 s and 35 s.
pub const SAY_AGAIN_FIRST_MS: u64 = 5_000;
/// Never past this from the first ask: its ask's `expires_in_ms`.
pub const SAY_AGAIN_WITHIN_MS: u64 = 60_000;

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
    /// [`SAY_OPENED`] not played and asked again: the first ask, the wait,
    /// and when it is due (`u64::MAX` once asked).
    again: Option<Again>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Again {
    first_ms: u64,
    wait_ms: u64,
    due_ms: u64,
}

impl Default for Recovery {
    fn default() -> Self {
        Self {
            state: State::Online,
            told: false,
            again: None,
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
        if first {
            self.again = None;
        }
        first
    }

    /// [`SAY_OPENED`], asked at `now_ms`, was not played: asked again after
    /// [`SAY_AGAIN_FIRST_MS`], then twice the wait each time, while that is
    /// within [`SAY_AGAIN_WITHIN_MS`] of the first ask.
    pub fn not_played(&mut self, now_ms: u64) {
        let (first_ms, wait_ms) = match self.again {
            Some(again) => (again.first_ms, again.wait_ms.saturating_mul(2)),
            None => (now_ms, SAY_AGAIN_FIRST_MS),
        };
        let due_ms = now_ms.saturating_add(wait_ms);
        self.again = (due_ms.saturating_sub(first_ms) <= SAY_AGAIN_WITHIN_MS).then_some(Again {
            first_ms,
            wait_ms,
            due_ms,
        });
    }

    /// Whether [`SAY_OPENED`] is to be asked again now: due, within its
    /// minute, and the hotspot open (not during a try of the saved network,
    /// when it is closed, nor once the lamp is online).
    pub fn says_again(&mut self, now_ms: u64) -> bool {
        let Some(again) = self.again.as_mut() else {
            return false;
        };
        let due = now_ms >= again.due_ms
            && now_ms.saturating_sub(again.first_ms) <= SAY_AGAIN_WITHIN_MS
            && matches!(self.state, State::Hotspot { .. });
        if due {
            again.due_ms = u64::MAX;
        }
        due
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
            self.again = None;
        }
        self.state = state;
        act
    }
}

/// How long the boot repair waits for the Wi-Fi's interface to appear: the
/// unit starts after `network-pre.target`, which a driver's late firmware
/// load may beat, and `device-sta-mode` stops at its first `iw` without it.
pub const INTERFACE_WAIT_MS: u64 = 30_000;

/// Whether the image's `device-ap-mode` left the radio a hotspot (#4863),
/// read from what it changes that outlives a boot: `wpa_supplicant@wlan0`
/// masked (its unit a link to `/dev/null`) or not enabled (no link in
/// `multi-user.target.wants`, where `device-sta-mode`'s `systemctl enable`
/// puts it), or `denyinterfaces wlan0` in dhcpcd's file. Any one leaves the
/// lamp with no client network at boot, and only `device-sta-mode` undoes
/// them; not enabled also covers one cut between its `unmask` and its
/// `enable` (Fifth's P3 on #4931). `root` is `/` on a lamp. Read from the
/// lamp's own state, not a note of ours: it holds whoever left the hotspot
/// (a build before this one too, as lamp-0c89 was left), and this unit
/// can't write `/var/lib` (`ProtectSystem=strict`). The image's scripts name
/// `wlan0` only, so this does too.
pub fn left_as_hotspot(root: &std::path::Path) -> bool {
    let units = root.join("etc/systemd/system");
    let masked = std::fs::read_link(units.join("wpa_supplicant@wlan0.service"))
        .is_ok_and(|target| target == std::path::Path::new("/dev/null"));
    let enabled = std::fs::symlink_metadata(
        units.join("multi-user.target.wants/wpa_supplicant@wlan0.service"),
    )
    .is_ok();
    let denied = std::fs::read_to_string(root.join("etc/dhcpcd.conf"))
        .is_ok_and(|conf| conf.lines().any(|line| line == "denyinterfaces wlan0"));
    masked || !enabled || denied
}

/// `device-sta-mode`, then a sync whatever it ended with: the root is
/// mounted `commit=600`, and its lasting steps (dhcpcd's file, the units
/// enabled, disabled and unmasked) come before its first restart, which
/// systemd refuses during a shutdown.
pub fn back_to_station(host: &mut impl crate::wifi::Host) -> std::io::Result<()> {
    let result = host.station();
    rustix::fs::sync();
    result
}

/// As `recover` closes its hotspot on a stop (#4863): the radio goes back to
/// the station, unless the marker is gone, a reset, where setup's own
/// hotspot takes over. `None`: nothing was done.
pub fn closed(
    host: &mut impl crate::wifi::Host,
    marker: &std::path::Path,
) -> Option<std::io::Result<()>> {
    marker.exists().then(|| back_to_station(host))
}

/// How many times the boot repair runs `device-sta-mode` before it gives
/// up until the next boot, and the wait between (Fifth's P2 on #4931): the
/// script disables the hotspot before its first `iw`, which has no
/// fallback, so a radio not ready yet leaves the lamp neither a hotspot nor
/// a station. Worst case 30 s for the interface, 3 x 45 s
/// ([`crate::wifi::RADIO_TIMEOUT`]) and 2 x 20 s: under 4 min, before the
/// watch begins; the unit is `Type=simple`, so no start timeout applies.
pub const STATION_TRIES: u32 = 3;
pub const STATION_RETRY_MS: u64 = 20_000;

/// At `recover`'s start, whatever its switch says (#4863): a set-up lamp
/// left a hotspot ([`left_as_hotspot`]) by a stop that never closed it, a
/// power cut above all, goes back to the station. Never before the marker
/// (a first setup's hotspot is meant), nor while setup runs or a Wi-Fi
/// change's `.lamp-previous` is there (theirs is the radio). `None`: nothing
/// was done. A failed return still left a hotspot is tried again
/// [`STATION_TRIES`] times in all, [`STATION_RETRY_MS`] apart (the host's
/// wait, so tests don't sleep); after the last, the next boot tries again.
pub fn at_start(
    host: &mut impl crate::wifi::Host,
    root: &std::path::Path,
    marker: &std::path::Path,
    previous: &std::path::Path,
    setup_active: impl FnOnce() -> bool,
) -> Option<std::io::Result<()>> {
    if !marker.exists() || !left_as_hotspot(root) || previous.exists() || setup_active() {
        return None;
    }
    let interface = root.join("sys/class/net/wlan0");
    let since = host.now_ms();
    while !interface.exists() && host.now_ms().saturating_sub(since) < INTERFACE_WAIT_MS {
        host.sleep(std::time::Duration::from_secs(1));
    }
    let mut tries = 1;
    loop {
        let result = back_to_station(host);
        if result.is_ok() || tries >= STATION_TRIES || !left_as_hotspot(root) {
            return Some(result);
        }
        tries += 1;
        host.sleep(std::time::Duration::from_millis(STATION_RETRY_MS));
    }
}
