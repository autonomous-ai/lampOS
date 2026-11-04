//! Whether Expression says the lamp is asleep for the night (#5156): its
//! `expression/state` with `asleep` true, the night's sleep by itself or by
//! the `night-sleep` skill (`docs/bedtime-morning.md`). A sleeping lamp that
//! sits up at a room sound reads as awake (CLAUDE.md rule 8), so a body that
//! knows it stays folded through a line not meant for it
//! (`body::Asked::night`).
//!
//! Read only with `LAMP_NIGHT_STAYS_FOLDED=on`, from the topic's kept value
//! at most every quarter of a second. The message is judged as the voice's
//! night face judges it (`lamp-voice`'s `ring_night::asleep_in`): only
//! Expression, enabled, saying asleep within the half second a state lasts.
//! Anything else, old, from another part or not valid, is not asleep, so an
//! Expression that stops holds nothing down: the body's latch lets go once
//! nothing has said asleep for `body::NIGHT_LAPSE`.

use lamp_bus::{Bus, Message};
use lamp_msgs::ExpressionState;
use std::time::{Duration, Instant};

/// The switch, the body's alone: off unless exactly `on`.
pub const SWITCH: &str = "LAMP_NIGHT_STAYS_FOLDED";
const LOOK_EVERY: Duration = Duration::from_millis(250);
/// How long after it was said a state still counts (Expression says its
/// state every 200 ms), shared with the voice's night face.
#[cfg(test)]
const LASTS_US: u64 = ExpressionState::LASTS_US;

/// Whether the switch's value turns it on: only `on`.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

pub fn enabled_from_environment() -> bool {
    enabled(std::env::var(SWITCH).ok().as_deref())
}

/// Whether `message` is Expression, enabled, saying at `now_us` (this
/// boot's monotonic clock) that it is asleep or falling asleep.
pub fn asleep_in(message: &Message, now_us: u64) -> bool {
    said_in(message, now_us).is_some_and(|state| state.says_asleep())
}

/// Whether `message` is Expression, enabled, saying at `now_us` that it is
/// waking: its name taken or a touch, never a line alone (#5158).
pub fn waking_in(message: &Message, now_us: u64) -> bool {
    said_in(message, now_us).is_some_and(|state| state.says_waking())
}

fn said_in(message: &Message, now_us: u64) -> Option<ExpressionState> {
    if message.expired
        || message.header.source != ExpressionState::SOURCE
        || message.header.at_us > now_us
        || now_us - message.header.at_us > ExpressionState::LASTS_US
    {
        return None;
    }
    message.parse::<ExpressionState>().ok()
}

/// The body's end of `expression/state`, for the night's sleep.
pub struct NightIn {
    bus: Bus,
    looked: Option<Instant>,
    asleep: bool,
    waking: bool,
}

impl NightIn {
    pub fn on(bus: Bus) -> Self {
        Self {
            bus,
            looked: None,
            asleep: false,
            waking: false,
        }
    }

    /// Whether Expression says now that the lamp is asleep for the night.
    /// Never waits longer than a file read.
    pub fn asleep(&mut self) -> bool {
        let now = Instant::now();
        if self.looked.is_none_or(|at| now - at >= LOOK_EVERY) {
            self.looked = Some(now);
            let now_us = lamp_ipc::monotonic_us();
            let latest = self.bus.latest(ExpressionState::TOPIC).ok().flatten();
            self.asleep = latest
                .as_ref()
                .is_some_and(|message| asleep_in(message, now_us));
            self.waking = latest
                .as_ref()
                .is_some_and(|message| waking_in(message, now_us));
        }
        self.asleep
    }

    /// Whether Expression said it was waking at the last
    /// [`NightIn::asleep`]'s read.
    pub fn waking(&self) -> bool {
        self.waking
    }

    /// The next [`NightIn::asleep`] reads the topic again, whenever it last
    /// did.
    pub fn look_now(&mut self) {
        self.looked = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_bus::Kind;
    use lamp_msgs::ClockStamp;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn state(asleep: Option<bool>, enabled: bool) -> ExpressionState {
        ExpressionState {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us: 1,
                sequence: 1,
            },
            enabled,
            turn: None,
            request: None,
            behaviours: vec!["asleep".into()],
            missing: vec![],
            warnings: vec![],
            body: false,
            light: false,
            asleep,
        }
    }

    fn root(name: &str) -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lamp-body-night-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// The last message `source` said on a real bus, `value` at `at_us`.
    fn said(source: &str, at_us: u64, value: &ExpressionState) -> Message {
        let root = root("said");
        let bus = Bus::new(&root).unwrap();
        let mut out = bus
            .publisher(ExpressionState::TOPIC, Kind::Latest, source)
            .unwrap();
        out.publish_json(at_us, value).unwrap();
        let message = bus.latest(ExpressionState::TOPIC).unwrap().unwrap();
        let _ = std::fs::remove_dir_all(root);
        message
    }

    #[test]
    fn only_on_turns_it_on() {
        assert!(enabled(Some("on")));
        for value in [None, Some("off"), Some("1"), Some("ON"), Some("")] {
            assert!(!enabled(value), "{value:?}");
        }
    }

    /// Only Expression, enabled, saying asleep within its half second is
    /// asleep: awake, shadow, no idle layer, another source, a state from
    /// the future or one past its life are not.
    #[test]
    fn only_a_fresh_enabled_expression_saying_asleep_is_asleep() {
        let at = 10_000_000;
        assert!(asleep_in(
            &said("expression", at, &state(Some(true), true)),
            at + LASTS_US
        ));
        for (message, now, label) in [
            (
                said("expression", at, &state(Some(false), true)),
                at,
                "awake",
            ),
            (
                said("expression", at, &state(None, true)),
                at,
                "no idle layer",
            ),
            (
                said("expression", at, &state(Some(true), false)),
                at,
                "shadow",
            ),
            (
                said("skills", at, &state(Some(true), true)),
                at,
                "another source",
            ),
            (
                said("expression", at, &state(Some(true), true)),
                at - 1,
                "future",
            ),
            (
                said("expression", at, &state(Some(true), true)),
                at + LASTS_US + 1,
                "old",
            ),
        ] {
            assert!(!asleep_in(&message, now), "{label}");
        }
    }

    /// Waking is Expression's own word for leaving sleep (a name it took, or
    /// a touch), read with the same freshness and source as asleep; a rest
    /// a line ended, or asleep itself, is not waking (#5158).
    #[test]
    fn only_a_fresh_enabled_expression_saying_waking_is_waking() {
        let at = 10_000_000;
        let with = |behaviours: &[&str], enabled: bool| ExpressionState {
            behaviours: behaviours.iter().map(|b| (*b).into()).collect(),
            ..state(Some(false), enabled)
        };
        assert!(waking_in(
            &said("expression", at, &with(&["waking"], true)),
            at + LASTS_US
        ));
        for (message, now, label) in [
            (
                said("expression", at, &state(Some(true), true)),
                at,
                "asleep",
            ),
            (said("expression", at, &with(&[], true)), at, "a rest ended"),
            (
                said("expression", at, &with(&["waking"], false)),
                at,
                "shadow",
            ),
            (
                said("skills", at, &with(&["waking"], true)),
                at,
                "another source",
            ),
            (
                said("expression", at, &with(&["waking"], true)),
                at + LASTS_US + 1,
                "old",
            ),
        ] {
            assert!(!waking_in(&message, now), "{label}");
        }
    }

    /// Read off the bus by the reader itself: nothing said is not asleep;
    /// Expression saying asleep now is; saying awake after is not.
    #[test]
    fn the_reader_follows_what_expression_says_now() {
        let root = root("reader");
        let bus = Bus::new(&root).unwrap();
        let mut night = NightIn::on(Bus::new(&root).unwrap());
        assert!(!night.asleep(), "nothing said");
        let mut out = bus
            .publisher_declared(
                ExpressionState::TOPIC,
                &ExpressionState::declaration(),
                "expression",
            )
            .unwrap();
        out.publish_json(lamp_ipc::monotonic_us(), &state(Some(true), true))
            .unwrap();
        night.look_now();
        assert!(night.asleep());
        out.publish_json(lamp_ipc::monotonic_us(), &state(Some(false), true))
            .unwrap();
        night.look_now();
        assert!(!night.asleep());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An asleep Expression stopped saying, outlived, is not asleep.
    #[test]
    fn an_old_sleep_is_not_asleep() {
        let root = root("old");
        let bus = Bus::new(&root).unwrap();
        let mut night = NightIn::on(Bus::new(&root).unwrap());
        let mut out = bus
            .publisher_declared(
                ExpressionState::TOPIC,
                &ExpressionState::declaration(),
                "expression",
            )
            .unwrap();
        out.publish_json(lamp_ipc::monotonic_us(), &state(Some(true), true))
            .unwrap();
        night.look_now();
        assert!(night.asleep());
        std::thread::sleep(Duration::from_millis(
            LASTS_US / 1_000 + lamp_bus::EXPIRY_GRACE_MS + 100,
        ));
        night.look_now();
        assert!(!night.asleep());
        let _ = std::fs::remove_dir_all(&root);
    }
}
