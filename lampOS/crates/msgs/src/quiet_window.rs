//! The person's quiet hours (#8866): one window for the whole lamp, set
//! from the app (`quiet.set`) and kept by Actions, its only keeper.
//!
//! Actions says it on `quiet/window` once a second, the latest value. It is
//! a setting, not a reading: a reader keeps the last one it heard when
//! Actions is down, and uses [`QuietWindow::default`] (on, 21:00 to 06:00,
//! the lamp's fixed night before #8866) only until it hears one. Skills
//! reads it: Bedtime's night ceilings hold while it is on and the local
//! time is inside it, and the evening glow and the morning hum keep their
//! hours from its start and its end, on or off.
use crate::Invalid;
use lamp_bus::{Declaration, Kind, Message};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Minutes in a local day: a start or an end is below it.
pub const MINUTES_A_DAY: u16 = 1440;

/// The window: `start` and `end` are minutes of the lamp's local day,
/// `start` included and `end` left out, and it crosses midnight when
/// `end` is below `start` (21:00 to 06:00).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietWindow {
    /// Whether the night ceilings hold in it. Off, the glow and the hum
    /// still keep their hours from `start` and `end`.
    pub on: bool,
    pub start: u16,
    pub end: u16,
}

impl Default for QuietWindow {
    /// A lamp nobody set: on, 21:00 to 06:00, as before #8866.
    fn default() -> Self {
        Self {
            on: true,
            start: 21 * 60,
            end: 6 * 60,
        }
    }
}

impl QuietWindow {
    pub const TYPE: &str = "lamp_msgs/QuietWindow";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "quiet/window";
    pub const SOURCE: &str = "actions";

    /// The newest replaces the last, said every second as `output/caps`
    /// is. A setting, so it never goes stale: a reader keeps the last it
    /// heard.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest).every(Duration::from_secs(1))
    }

    /// Each time below a day, and a start and an end that differ: a window
    /// of no time or of the whole day is never meant.
    pub fn check(&self) -> Result<(), Invalid> {
        if self.start >= MINUTES_A_DAY || self.end >= MINUTES_A_DAY {
            return Err(Invalid("a quiet time is a minute of the day".into()));
        }
        if self.start == self.end {
            return Err(Invalid(
                "quiet hours need a start and an end that differ".into(),
            ));
        }
        Ok(())
    }

    /// Whether the local minute `minute` is inside the window, on or off.
    pub fn contains(&self, minute: u16) -> bool {
        minute < MINUTES_A_DAY
            && if self.start < self.end {
                (self.start..self.end).contains(&minute)
            } else {
                minute >= self.start || minute < self.end
            }
    }

    /// The window Actions said in `message`, if it is one: from Actions,
    /// of this type, and checked. Freshness is not asked: a setting kept
    /// on the bus is still the person's after its publisher went quiet.
    pub fn from_message(message: &Message) -> Option<Self> {
        let header = &message.header;
        if header.source != Self::SOURCE || header.type_name.as_deref() != Some(Self::TYPE) {
            return None;
        }
        message
            .parse::<Self>()
            .ok()
            .filter(|window| window.check().is_ok())
    }
}

/// `HH:MM`, 24-hour, as the app and the kept file write a quiet time.
pub fn quiet_time(minute: u16) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// The minute of the day `text` names, only when it is exactly `HH:MM`,
/// 24-hour, `00:00` to `23:59`.
pub fn quiet_minute(text: &str) -> Option<u16> {
    let bytes = text.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || ![bytes[0], bytes[1], bytes[3], bytes[4]]
            .iter()
            .all(u8::is_ascii_digit)
    {
        return None;
    }
    let hour = u16::from(bytes[0] - b'0') * 10 + u16::from(bytes[1] - b'0');
    let minute = u16::from(bytes[3] - b'0') * 10 + u16::from(bytes[4] - b'0');
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

/// The quiet window's topic, kept apart from the original catalogue.
pub fn quiet_topics() -> Vec<(&'static str, Declaration)> {
    vec![(QuietWindow::TOPIC, QuietWindow::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_old_fixed_night_and_it_is_declared() {
        let window = QuietWindow::default();
        assert_eq!(
            serde_json::to_string(&window).unwrap(),
            r#"{"on":true,"start":1260,"end":360}"#
        );
        assert!(window.check().is_ok());
        let declared = crate::declaration_of(QuietWindow::TOPIC).unwrap();
        assert_eq!(declared.type_name, QuietWindow::TYPE);
        assert_eq!(declared.kind, Kind::Latest);
        assert_eq!(declared.period_ms, Some(1_000));
        assert_eq!(declared.lifespan_ms, None);
    }

    #[test]
    fn a_window_reads_back_and_a_later_field_is_passed_over() {
        let window = QuietWindow {
            on: false,
            start: 22 * 60,
            end: 7 * 60,
        };
        let text = serde_json::to_string(&window).unwrap();
        assert_eq!(text, r#"{"on":false,"start":1320,"end":420}"#);
        assert_eq!(serde_json::from_str::<QuietWindow>(&text).unwrap(), window);
        let later = r#"{"on":false,"start":1320,"end":420,"added_later":[1]}"#;
        assert_eq!(serde_json::from_str::<QuietWindow>(later).unwrap(), window);
        for missing in [
            r#"{"start":1320,"end":420}"#,
            r#"{"on":true,"end":420}"#,
            r#"{"on":true,"start":1320}"#,
            r#"{"on":"yes","start":1320,"end":420}"#,
        ] {
            assert!(
                serde_json::from_str::<QuietWindow>(missing).is_err(),
                "{missing}"
            );
        }
    }

    #[test]
    fn each_time_is_a_minute_of_the_day_and_the_two_differ() {
        let fine = |start, end| QuietWindow {
            on: true,
            start,
            end,
        };
        assert!(fine(0, 1439).check().is_ok());
        assert!(fine(1439, 0).check().is_ok());
        assert!(fine(1440, 0).check().is_err());
        assert!(fine(0, 1440).check().is_err());
        let same = fine(600, 600).check().unwrap_err();
        assert_eq!(same.0, "quiet hours need a start and an end that differ");
    }

    #[test]
    fn inside_crosses_midnight_and_leaves_out_the_end() {
        let night = QuietWindow {
            on: true,
            start: 22 * 60,
            end: 7 * 60,
        };
        assert!(!night.contains(22 * 60 - 1));
        assert!(night.contains(22 * 60));
        assert!(night.contains(0));
        assert!(night.contains(7 * 60 - 1));
        assert!(!night.contains(7 * 60));
        assert!(!night.contains(1440));
        let day = QuietWindow {
            on: true,
            start: 13 * 60,
            end: 14 * 60,
        };
        assert!(day.contains(13 * 60) && !day.contains(14 * 60) && !day.contains(0));
    }

    #[test]
    fn a_quiet_time_is_exactly_hh_mm() {
        assert_eq!(quiet_minute("00:00"), Some(0));
        assert_eq!(quiet_minute("21:00"), Some(1260));
        assert_eq!(quiet_minute("23:59"), Some(1439));
        for refused in [
            "24:00", "7:00", "07:0", "07:60", "07-00", " 07:00", "0700", "", "ab:cd",
        ] {
            assert_eq!(quiet_minute(refused), None, "{refused:?}");
        }
        assert_eq!(quiet_time(1260), "21:00");
        assert_eq!(quiet_time(5), "00:05");
        for minute in 0..MINUTES_A_DAY {
            assert_eq!(quiet_minute(&quiet_time(minute)), Some(minute));
        }
    }

    #[test]
    fn only_actions_window_of_this_type_is_taken() {
        let root = std::env::temp_dir().join(format!("lamp-msgs-quiet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let window = QuietWindow {
            on: true,
            start: 22 * 60,
            end: 7 * 60,
        };
        let mut actions = bus
            .publisher_declared(
                QuietWindow::TOPIC,
                &QuietWindow::declaration(),
                QuietWindow::SOURCE,
            )
            .unwrap();
        actions.publish_json(1, &window).unwrap();
        let kept = bus.latest(QuietWindow::TOPIC).unwrap().unwrap();
        assert_eq!(QuietWindow::from_message(&kept), Some(window));
        let mut other = bus
            .publisher_declared(QuietWindow::TOPIC, &QuietWindow::declaration(), "cloud")
            .unwrap();
        other.publish_json(2, &window).unwrap();
        let kept = bus.latest(QuietWindow::TOPIC).unwrap().unwrap();
        assert_eq!(QuietWindow::from_message(&kept), None);
        let mut wrong = actions;
        wrong
            .publish_json(
                3,
                &QuietWindow {
                    start: 420,
                    ..window
                },
            )
            .unwrap();
        let kept = bus.latest(QuietWindow::TOPIC).unwrap().unwrap();
        assert_eq!(
            QuietWindow::from_message(&kept),
            None,
            "start and end alike"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
