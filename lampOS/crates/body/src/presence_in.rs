//! Whether someone is at the lamp, for a standing lamp to stay up while
//! they are (#7301): `room/presence` (`lamp_msgs::RoomPresence`, said by
//! lamp-eyes, `docs/presence.md`), a face seen or a voice said to the lamp
//! within 20 s, or nobody for how long.
//!
//! Read only with `LAMP_BODY_PRESENCE=on`, from the topic's kept value at
//! most once a second. Someone is there while the eyes say so, and for
//! [`GRACE_S`] after: someone reading face down is not seen. A value past
//! its life (`RoomPresence::LASTS`), from the future, past its own life on
//! the bus or not valid is nobody: with the camera closed nothing is said,
//! and the lamp folds as it does today. Off: nobody, always.

use lamp_bus::{Bus, Message};
use lamp_msgs::RoomPresence;
use std::time::{Duration, Instant};

/// The switch, the body's alone: off unless exactly `on`.
pub const SWITCH: &str = "LAMP_BODY_PRESENCE";
pub const TOPIC: &str = RoomPresence::TOPIC;
const LOOK_EVERY: Duration = Duration::from_secs(1);
/// Nobody seen or heard for less than this is still someone there: a
/// person reading or writing face down for a minute or two.
/// ponytail: one number for every desk; per-person timing if a ROW shows
/// lamps folding over someone still working.
pub const GRACE_S: u32 = 120;

/// Whether the switch's value turns it on: only `on`.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

pub fn enabled_from_environment() -> bool {
    enabled(std::env::var(SWITCH).ok().as_deref())
}

/// Whether `message`, read at `now_us` (this boot's monotonic clock), says
/// someone is at the lamp or was within [`GRACE_S`].
pub fn present_in(message: &Message, now_us: u64) -> bool {
    if message.expired
        || message.header.at_us > now_us
        || now_us - message.header.at_us > RoomPresence::LASTS.as_micros() as u64
    {
        return false;
    }
    message
        .parse::<RoomPresence>()
        .ok()
        .filter(|said| said.check().is_ok())
        .is_some_and(|said| said.someone || said.nobody_s < GRACE_S)
}

/// The body's end of `room/presence`.
pub struct PresenceIn {
    bus: Bus,
    looked: Option<Instant>,
    present: bool,
}

impl PresenceIn {
    pub fn on(bus: Bus) -> Self {
        Self {
            bus,
            looked: None,
            present: false,
        }
    }

    /// Whether someone is at the lamp, as last read. Never waits longer
    /// than a file read.
    pub fn present(&mut self) -> bool {
        let now = Instant::now();
        if self.looked.is_none_or(|at| now - at >= LOOK_EVERY) {
            self.looked = Some(now);
            let now_us = lamp_ipc::monotonic_us();
            self.present = self
                .bus
                .latest(TOPIC)
                .ok()
                .flatten()
                .is_some_and(|message| present_in(&message, now_us));
        }
        self.present
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_bus::Kind;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// The last message said on a real bus, `value` at `at_us`.
    fn said(at_us: u64, value: serde_json::Value) -> Message {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lamp-body-presence-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut out = bus.publisher(TOPIC, Kind::Latest, "eyes").unwrap();
        out.publish_json(at_us, &value).unwrap();
        let message = bus.latest(TOPIC).unwrap().unwrap();
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

    /// Someone, or nobody for under two minutes, said within the value's
    /// life is someone there; nobody for longer, an old value, one from the
    /// future and one that does not read are not.
    #[test]
    fn someone_or_nobody_lately_said_freshly_is_present() {
        let at = 10_000_000;
        let lasts = RoomPresence::LASTS.as_micros() as u64;
        let someone = serde_json::json!({"someone": true, "nobody_s": 0, "by": "face"});
        assert!(present_in(&said(at, someone.clone()), at));
        assert!(present_in(&said(at, someone.clone()), at + lasts));
        let nobody = |seconds: u32| serde_json::json!({"someone": false, "nobody_s": seconds});
        assert!(present_in(&said(at, nobody(GRACE_S - 1)), at));
        for (message, now, label) in [
            (said(at, nobody(GRACE_S)), at, "nobody for two minutes"),
            (said(at, someone.clone()), at + lasts + 1, "old"),
            (said(at, someone.clone()), at - 1, "future"),
            (
                said(at, serde_json::json!({"someone": true, "nobody_s": 5})),
                at,
                "not valid",
            ),
            (
                said(at, serde_json::json!({"level": 1})),
                at,
                "not presence",
            ),
        ] {
            assert!(!present_in(&message, now), "{label}");
        }
    }
}
