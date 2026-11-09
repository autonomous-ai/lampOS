//! A wordless call only on a sure hearing (`LAMP_NAME_WORDLESS_BAR=on`,
//! #5385). A line that was only the lamp's name (`ProviderInput::Called`)
//! and whose words came back empty is asked as the name ("Lamp"), or it
//! would go unanswered as a sound (#1712). With the switch on, that is done
//! only when lamp-name heard the name in the line at [`BAR`] or more; under
//! it, the line is left as a sound: nothing is asked and nothing answered.
//! `name/heard` itself goes out as ever: this is the voice's reading of it.
//!
//! Why: on lamp-75c9 on 9 Nov a line the body's own move opened was heard as
//! the name at 0.941 with no words, and the lamp answered a call nobody made
//! (Eighth's report on #5385). On lamp-c0a4, 82 real calls scored 0.901 to
//! 1.000 (median 0.964); 4 had no words back, 3 of them under 0.99. Noise
//! wakes scored 0.941 to 0.978. The rule costs about 3 calls in 82, each a
//! lone "Lamp" whose words did not come back; any call with words is
//! answered as before.
//!
//! The hearings are read from `name/heard` on a subscriber of their own,
//! kept for [`KEPT_US`], and a line's best is the highest that began from
//! [`crate::attention::NAME_BEFORE_US`] before its first sound to its end.
//! A line with none read is under the bar: its call came from the name's
//! first sign alone (`name/rising`), which lamp-name never said in full.

use std::{
    collections::VecDeque,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime},
};

/// The switch, in the second worker's environment: `on` exactly.
pub const ENVIRONMENT: &str = "LAMP_NAME_WORDLESS_BAR";
/// The least a hearing scores for a line with no words to be a call.
pub const BAR: f32 = 0.99;
/// Hearings older than this are let go.
const KEPT_US: u64 = 30_000_000;
/// The most hearings kept.
const KEPT: usize = 32;
/// The bus's name for this reader.
const SUBSCRIBER: &str = "second_wordless";

/// The best score lamp-name heard the name at in a line's window (its first
/// and last sound's read times, this process's clock), if it heard it.
pub type BestHeard = Arc<dyn Fn((u64, u64)) -> Option<f32> + Send + Sync>;

/// Whether the switch is on, from its value.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether a line with no words, marked as only the name, is asked as the
/// name: lamp-name heard it in the line at [`BAR`] or more.
pub fn answered(best: Option<f32>) -> bool {
    best.is_some_and(|score| score >= BAR)
}

/// What the journal says of the best hearing.
pub fn scored(best: Option<f32>) -> String {
    best.map_or_else(
        || "lamp-name said no hearing of it".to_owned(),
        |score| format!("lamp-name heard it at {score:.3}"),
    )
}

/// The hearings read from `name/heard`, when each began by this process's
/// clock and its score.
pub struct Hearings {
    heard: lamp_bus::Subscriber,
    kept: VecDeque<(u64, f32)>,
}

impl Hearings {
    pub fn on(root: &Path) -> std::io::Result<Self> {
        let bus = lamp_bus::Bus::new(root)?;
        Ok(Self {
            heard: bus.subscribe_typed(
                lamp_msgs::NameHeard::TOPIC,
                SUBSCRIBER,
                lamp_msgs::NameHeard::TYPE,
            )?,
            kept: VecDeque::new(),
        })
    }

    /// The best hearing in `window`, reading what has come since. Never
    /// waits.
    pub fn best(&mut self, window: (u64, u64)) -> Option<f32> {
        let now_us = lamp_ipc::monotonic_us();
        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis().try_into().unwrap_or(u64::MAX));
        while let Ok(Some(message)) = self.heard.recv(Duration::ZERO) {
            if let Ok(heard) = message.parse::<lamp_msgs::NameHeard>() {
                // Its publisher's monotonic clock is its own: dated by the
                // wall clock both share.
                let ago_ms =
                    now_ms.saturating_sub(message.header.at_unix_ms) + u64::from(heard.ago_ms);
                self.keep(now_us.saturating_sub(ago_ms * 1_000), heard.score);
            }
        }
        self.kept
            .retain(|(at, _)| now_us.saturating_sub(*at) <= KEPT_US);
        best_in(&self.kept, window)
    }

    fn keep(&mut self, at_us: u64, score: f32) {
        if self.kept.len() == KEPT {
            self.kept.pop_front();
        }
        self.kept.push_back((at_us, score));
    }
}

/// The best of `kept` that began from [`crate::attention::NAME_BEFORE_US`]
/// before the window's start to its end.
fn best_in(kept: &VecDeque<(u64, f32)>, (began, ended): (u64, u64)) -> Option<f32> {
    let from = began.saturating_sub(crate::attention::NAME_BEFORE_US);
    kept.iter()
        .filter(|(at, _)| (from..=ended.max(began)).contains(at))
        .map(|(_, score)| *score)
        .reduce(f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_on_exactly_switches_it_on() {
        assert!(on(Some("on")));
        for value in [None, Some(""), Some("off"), Some("ON"), Some("0.99")] {
            assert!(!on(value), "{value:?}");
        }
    }

    /// The first builder's rule (#5385): under 0.99, or no hearing, no call.
    #[test]
    fn a_wordless_line_is_a_call_only_at_the_bar() {
        assert!(!answered(Some(0.941)));
        assert!(!answered(Some(0.989)));
        assert!(!answered(None));
        assert!(answered(Some(0.99)));
        assert!(answered(Some(1.0)));
    }

    #[test]
    fn a_lines_best_hearing_is_from_just_before_its_start_to_its_end() {
        let second = 1_000_000;
        let kept: VecDeque<(u64, f32)> = [
            (2 * second, 0.999),
            (9 * second, 0.95),
            (10 * second + 500_000, 0.94),
            (13 * second, 1.0),
        ]
        .into();
        assert_eq!(best_in(&kept, (10 * second, 12 * second)), Some(0.95));
        assert_eq!(best_in(&kept, (10 * second, 13 * second)), Some(1.0));
        assert_eq!(best_in(&kept, (20 * second, 21 * second)), None);
    }
}
