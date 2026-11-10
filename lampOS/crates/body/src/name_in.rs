//! The lamp's name, heard (`name/heard`, `docs/name.md`), for a body that
//! turns to a call (`body::Limits::look_at_name`). Looked at once a step and
//! never waited for: the body's loop reads and writes the servos every
//! 20 ms and a hearing is a few bytes, a few times a minute at most.
//!
//! With `LAMP_NAME_WORDLESS_BAR=on` (#5385), only a hearing at [`WORDLESS_BAR`]
//! or more is turned to: the body cannot know whether words follow it (the
//! voice knows them about 1.7 s later), and a hearing under the bar with no
//! words is no call. A real call under the bar still has the voice's own
//! listening, which turns the lamp up from the moment the person's speech
//! began; only the glance for a call with nobody in view, and the 2 s of
//! listening a call adds when the voice is at rest, are left out.

use lamp_bus::{Bus, Subscriber};
use lamp_msgs::NameHeard;
use std::{io, time::Duration};

/// A hearing whose name began longer ago than this is not a call to turn
/// to now: the listener says one a tenth of a second or more after the
/// name began, so anything later is a body that was busy or a bus behind.
const FRESH_MS: u32 = 1_500;

/// The switch, in the body's environment: `on` exactly. The voice's second
/// worker reads the same one (`lamp_voice::second_provider::wordless_call`).
pub const WORDLESS_ENVIRONMENT: &str = "LAMP_NAME_WORDLESS_BAR";
/// The least a hearing scores to be turned to with the switch on.
pub const WORDLESS_BAR: f32 = 0.99;

/// The bar a hearing is turned to at, from the switch's value: none (any
/// hearing) unless it is `on` exactly.
pub fn bar_from(value: Option<&str>) -> Option<f32> {
    (value == Some("on")).then_some(WORDLESS_BAR)
}

/// The body's end of `name/heard`.
pub struct NameIn {
    heard: Subscriber,
    /// The least score turned to (`LAMP_NAME_WORDLESS_BAR`); none: any.
    bar: Option<f32>,
}

impl NameIn {
    /// `bar`: the least score turned to, from [`bar_from`]; none, any
    /// fresh hearing, as before.
    pub fn on(bus: &Bus, bar: Option<f32>) -> io::Result<Self> {
        Ok(Self {
            bar,
            heard: bus.subscribe_typed(
                NameHeard::TOPIC,
                crate::bus_out::SOURCE,
                NameHeard::TYPE,
            )?,
        })
    }

    /// Whether a fresh hearing came since the last look. Never waits.
    pub fn called(&mut self) -> bool {
        let mut called = false;
        for _ in 0..8 {
            let Ok(Some(message)) = self.heard.recv(Duration::ZERO) else {
                break;
            };
            called |= message
                .parse::<NameHeard>()
                .is_ok_and(|heard| taken(&heard, self.bar));
        }
        called
    }
}

fn fresh(heard: &NameHeard) -> bool {
    heard.check().is_ok() && heard.ago_ms <= FRESH_MS
}

/// A fresh hearing at `bar` or more, or at any score with none.
fn taken(heard: &NameHeard, bar: Option<f32>) -> bool {
    fresh(heard) && bar.is_none_or(|bar| heard.score >= bar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_sound_and_recent_hearing_is_a_call() {
        let heard = |score, ago_ms| NameHeard { score, ago_ms };
        assert!(fresh(&heard(0.8, 300)));
        assert!(fresh(&heard(0.8, FRESH_MS)));
        assert!(!fresh(&heard(0.8, FRESH_MS + 1)));
        assert!(!fresh(&heard(1.5, 300)));
    }

    #[test]
    fn only_on_exactly_sets_the_wordless_bar() {
        assert_eq!(bar_from(Some("on")), Some(0.99));
        for value in [None, Some(""), Some("off"), Some("ON"), Some("0.99")] {
            assert_eq!(bar_from(value), None, "{value:?}");
        }
    }

    /// #5385: with the switch on, a hearing under 0.99 (lamp-75c9's 0.941
    /// in its own move's sound) is not turned to; at 0.99 or more it is.
    /// Off, any fresh hearing is, as before.
    #[test]
    fn with_the_wordless_bar_only_a_sure_hearing_is_a_call() {
        let heard = |score, ago_ms| NameHeard { score, ago_ms };
        let on = bar_from(Some("on"));
        assert!(!taken(&heard(0.941, 75), on));
        assert!(!taken(&heard(0.989, 75), on));
        assert!(taken(&heard(0.99, 75), on));
        assert!(taken(&heard(1.0, 75), on));
        assert!(!taken(&heard(1.0, FRESH_MS + 1), on));
        for score in [0.8, 0.941, 0.989, 0.99, 1.0] {
            assert_eq!(taken(&heard(score, 75), None), fresh(&heard(score, 75)));
            assert!(taken(&heard(score, 75), None), "{score}");
        }
    }
}
