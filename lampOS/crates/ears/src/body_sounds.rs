//! Body sounds of the person near the lamp (#7685): a cough, a throat cleared,
//! a sneeze, a sniff, a wheeze, told one by one on `ears/body` so a skill can
//! count them with nothing recorded. The scores come from the YAMNet the lamp
//! already runs (`classes`), and only with `LAMP_EARS_BODY_SOUNDS=on`; these
//! are a person's own sounds and stay on the lamp (docs/ears.md, "Body
//! sounds").
//!
//! A sound spans two or three overlapping patches (a ~1 s window every half
//! second), so a score over its bar is told ONCE: on the rise, then not again
//! until the score has fallen well under the bar and the class's refractory
//! has passed.
use lamp_msgs::{BodyClass, BodySound, OwnVoiceEvidence, SoundScores};

/// `LAMP_EARS_BODY_SOUNDS` is exactly `on`: anything else, or nothing, is off.
/// It acts only where the room sounds do (`LAMP_EARS_CLASSES=on`, the model
/// and the person's yes): the scores it adds are read from that classifier.
pub fn enabled(switch: Option<&str>) -> bool {
    switch == Some("on")
}

/// A class's bar and the quiet it keeps after a sound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    /// A score at or over it is a sound heard.
    pub bar: f32,
    /// No second sound of the class for this long after one, in microseconds.
    pub refractory_us: u64,
}

/// A class is re-armed once its score is under this fraction of its bar.
const REARM: f32 = 0.7;

/// The bars the lamp ships: `None` is a class not measured (#7685, its row
/// in docs/lamp-runs.md), and a class not measured is never told. A bar is
/// filled in only from the recorded sounds of a person who agreed, so each
/// published class catches at least 9 of 10 of its own sounds with under one
/// false alarm an hour; a class that cannot is left `None`.
pub const BARS: [(BodyClass, Option<Bar>); 5] = [
    (BodyClass::Cough, None),
    (BodyClass::ThroatClear, None),
    (BodyClass::Sneeze, None),
    (BodyClass::Sniff, None),
    (BodyClass::Wheeze, None),
];

fn score(scores: &SoundScores, class: BodyClass) -> f32 {
    match class {
        BodyClass::Cough => scores.cough,
        BodyClass::ThroatClear => scores.throat_clear,
        BodyClass::Sneeze => scores.sneeze,
        BodyClass::Sniff => scores.sniff,
        BodyClass::Wheeze => scores.wheeze,
    }
}

/// Turns each patch's scores into the sounds heard in it.
pub struct Detector {
    bars: [(BodyClass, Option<Bar>); 5],
    armed: [bool; 5],
    last_us: [Option<u64>; 5],
}

impl Detector {
    /// With the bars the lamp ships.
    pub fn new() -> Self {
        Self::with_bars(BARS)
    }

    pub fn with_bars(bars: [(BodyClass, Option<Bar>); 5]) -> Self {
        Self {
            bars,
            armed: [true; 5],
            last_us: [None; 5],
        }
    }

    /// The sounds heard in one patch of scores that ended at `end_us`. While
    /// the lamp speaks into the patch (`own_voice` present) nothing is told:
    /// its own voice is not its person's body, and a skill must never take
    /// the lamp's line for a cough.
    pub fn feed(
        &mut self,
        scores: &SoundScores,
        own_voice: OwnVoiceEvidence,
        end_us: u64,
    ) -> Vec<BodySound> {
        let mut heard = Vec::new();
        for (index, (class, bar)) in self.bars.iter().enumerate() {
            let Some(bar) = bar else { continue };
            let now = score(scores, *class);
            if own_voice == OwnVoiceEvidence::Present {
                // Not armed again by the lamp's own voice, and nothing told.
                continue;
            }
            if now < bar.bar * REARM {
                self.armed[index] = true;
            }
            let quiet = self.last_us[index]
                .is_none_or(|last| end_us.saturating_sub(last) >= bar.refractory_us);
            if now >= bar.bar && self.armed[index] && quiet {
                self.armed[index] = false;
                self.last_us[index] = Some(end_us);
                heard.push(BodySound {
                    class: *class,
                    score: (now * 100.0).round() / 100.0,
                    at_us: end_us,
                });
            }
        }
        heard
    }
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
