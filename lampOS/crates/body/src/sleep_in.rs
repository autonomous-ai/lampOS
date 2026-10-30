//! Whether a hand held on the head put the lamp to sleep (#1362): the
//! voice's mute with the reason `sleep` (`voice/mute`, #711), which the
//! voice says when it changes and every second while it lasts. Read only
//! with `LAMP_HEAD_TOUCH=on`, from the topic's kept value at most every
//! quarter of a second, and at once when asked ([`SleepIn::look_now`]), so
//! the fold begins within a quarter of a second of the voice saying so.
//! A value that has outlived its three seconds is not asleep: a voice that
//! stopped, or started again without head touch, holds nothing down. The
//! voice open again after a sleep (a tap on the head) is told once, so the
//! body stands at once: the lift.

use lamp_bus::Bus;
use lamp_msgs::{MuteReason, VoiceMute};
use std::time::{Duration, Instant};

/// The switch, the voice's and Expression's own (#711).
pub const SWITCH: &str = "LAMP_HEAD_TOUCH";
const LOOK_EVERY: Duration = Duration::from_millis(250);
/// The topic's one publisher: the voice's controls (`hand_controls.rs`).
const PUBLISHER: &str = "voice";

/// Whether the switch's value turns it on: only `on`.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

pub fn enabled_from_environment() -> bool {
    enabled(std::env::var(SWITCH).ok().as_deref())
}

/// What a fresh value says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Said {
    /// Muted because a hand put it to sleep.
    Asleep,
    /// Not muted.
    Open,
    /// Muted for another reason: the person's privacy.
    Other,
}

/// The body's end of `voice/mute`.
pub struct SleepIn {
    bus: Bus,
    looked: Option<Instant>,
    asleep: bool,
    woken: bool,
    private: bool,
}

impl SleepIn {
    pub fn on(bus: Bus) -> Self {
        Self {
            bus,
            looked: None,
            asleep: false,
            woken: false,
            private: false,
        }
    }

    /// Whether the voice says a hand put it to sleep. Never waits longer
    /// than a file read.
    pub fn asleep(&mut self) -> bool {
        let now = Instant::now();
        if self.looked.is_none_or(|at| now - at >= LOOK_EVERY) {
            self.looked = Some(now);
            let said = said(&self.bus);
            self.woken |= self.asleep && said == Some(Said::Open);
            self.asleep = said == Some(Said::Asleep);
            self.private = said == Some(Said::Other);
        }
        self.asleep
    }

    /// Whether the voice is muted for the person's privacy, as the last
    /// [`SleepIn::asleep`] read it: the body's lease ends on it (#3504).
    pub fn private(&self) -> bool {
        self.private
    }

    /// Whether the voice said it is open again after a sleep since this
    /// was last asked: once for each wake. Not for a sleep that only went
    /// old, nor one that became the person's privacy mute.
    pub fn woken(&mut self) -> bool {
        std::mem::take(&mut self.woken)
    }

    /// The next [`SleepIn::asleep`] reads the topic again, whenever it last
    /// did: the voice speaking to the body while it is taken to be asleep
    /// has likely woken, and its lift should not wait.
    pub fn look_now(&mut self) {
        self.looked = None;
    }
}

/// What the kept value says, while it is fresh and the voice's. Nothing
/// kept, nothing readable, an old value or one another part wrote says
/// nothing: only the voice may put the body to sleep or wake it.
fn said(bus: &Bus) -> Option<Said> {
    let mute = bus
        .latest(VoiceMute::TOPIC)
        .ok()
        .flatten()
        .filter(|message| !message.expired && message.header.source == PUBLISHER)
        .and_then(|message| message.parse::<VoiceMute>().ok())?;
    Some(match (mute.muted, mute.reason) {
        (true, Some(MuteReason::Sleep)) => Said::Asleep,
        (true, None) => Said::Other,
        (false, _) => Said::Open,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_on_turns_it_on() {
        assert!(enabled(Some("on")));
        for value in [None, Some("off"), Some("1"), Some("ON"), Some("")] {
            assert!(!enabled(value), "{value:?}");
        }
    }

    #[test]
    fn it_is_asleep_only_while_the_voice_says_a_mute_for_sleep() {
        let root = std::env::temp_dir().join(format!("lamp-body-sleep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut sleep = SleepIn::on(Bus::new(&root).unwrap());
        assert!(!sleep.asleep(), "nothing said");
        let mut out = bus
            .publisher_declared(VoiceMute::TOPIC, &VoiceMute::declaration(), "voice")
            .unwrap();
        let said = |muted, reason| VoiceMute {
            muted,
            reason,
            muted_by: None,
        };
        out.publish_json(1, &said(true, Some(MuteReason::Sleep)))
            .unwrap();
        sleep.look_now();
        assert!(sleep.asleep());
        assert!(!sleep.woken());
        // The person's privacy mute is not a sleep, nor a wake.
        assert!(!sleep.private());
        out.publish_json(2, &said(true, None)).unwrap();
        sleep.look_now();
        assert!(!sleep.asleep());
        assert!(!sleep.woken());
        assert!(sleep.private());
        out.publish_json(3, &said(true, Some(MuteReason::Sleep)))
            .unwrap();
        sleep.look_now();
        assert!(sleep.asleep());
        assert!(!sleep.private(), "a sleep is not privacy");
        // Open again after the sleep: woken, told once.
        out.publish_json(4, &said(false, None)).unwrap();
        sleep.look_now();
        assert!(!sleep.asleep());
        assert!(sleep.woken());
        assert!(!sleep.woken());
        // Still open: no second wake.
        out.publish_json(5, &said(false, None)).unwrap();
        sleep.look_now();
        assert!(!sleep.asleep());
        assert!(!sleep.woken());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Only the voice puts it to sleep: the same word from another part on
    /// the bus is not taken (the reviewer's P3 on #2445).
    #[test]
    fn a_sleep_another_part_says_is_not_taken() {
        let root =
            std::env::temp_dir().join(format!("lamp-body-sleep-other-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut sleep = SleepIn::on(Bus::new(&root).unwrap());
        let mut out = bus
            .publisher_declared(VoiceMute::TOPIC, &VoiceMute::declaration(), "skill")
            .unwrap();
        let asleep = VoiceMute {
            muted: true,
            reason: Some(MuteReason::Sleep),
            muted_by: None,
        };
        out.publish_json(1, &asleep).unwrap();
        sleep.look_now();
        assert!(!sleep.asleep());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A sleep the voice stopped saying, outlived, holds nothing down.
    #[test]
    fn an_old_sleep_is_not_asleep() {
        let root = std::env::temp_dir().join(format!("lamp-body-sleep-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut sleep = SleepIn::on(Bus::new(&root).unwrap());
        // Declared to last only 20 ms, so that it outlives it here.
        let short = VoiceMute::declaration().lasting(Duration::from_millis(20));
        let mut out = bus
            .publisher_declared(VoiceMute::TOPIC, &short, "voice")
            .unwrap();
        let asleep = VoiceMute {
            muted: true,
            reason: Some(MuteReason::Sleep),
            muted_by: None,
        };
        out.publish_json(1, &asleep).unwrap();
        std::thread::sleep(Duration::from_millis(20 + lamp_bus::EXPIRY_GRACE_MS + 100));
        sleep.look_now();
        assert!(!sleep.asleep());
        // Gone old is not woken: nothing stands it up.
        assert!(!sleep.woken());
        let _ = std::fs::remove_dir_all(&root);
    }
}
