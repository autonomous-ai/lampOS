//! The timeline (#5883's second piece): each turn as V1's Flow Monitor
//! showed it, heard, understood, did, said, joined by the request number
//! `voice/heard`, `voice/turn` and `voice/said` share. What it did is the
//! light asked for (`light/command`, when it changes) and the skills'
//! stages (`skills/events`) that come inside the turn. A function's name is
//! not on the bus, so "did" is light and skills only.
//!
//! Words are kept only when the timeline is made with them, which only
//! monitor mode does: then the finished words heard and said, cut to
//! [`WORD_CHARS`], in memory only, the newest [`KEPT`] turns. Never a
//! skill's `why`. The page escapes everything it shows of it.
use crate::watch::plain;
use lamp_bus::Message;
use lamp_msgs::{Light, SkillEvent, SkillStage, Speech, TurnVerdict, VoiceTurn};
use std::collections::VecDeque;

/// The most turns kept.
pub const KEPT: usize = 30;
/// The most characters of words kept of a line heard or said.
pub const WORD_CHARS: usize = 300;
/// How long after its first step a turn takes what the lamp did, and a
/// step of the same request number: a number seen again later is another
/// turn (the voice counts again from its start).
pub const TURN_LASTS_MS: u64 = 60_000;
/// The topics only the timeline reads: not on `/bus`.
pub const TOPICS: [&str; 2] = [Light::TOPICS[0], SkillEvent::TOPIC];

/// One step of a turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// The words heard, finished; the words only with words on.
    Heard(Option<String>),
    /// The voice's verdict: answered, or held back.
    Understood(TurnVerdict),
    /// What the lamp did: `light blue at 3`, `skill wave done`.
    Did(String),
    /// The words said, finished; the words only with words on.
    Said(Option<String>),
}

/// One turn, as far as the bus has told it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub request: u64,
    /// Each step with its wall clock time, as it came.
    pub steps: Vec<(u64, Step)>,
    /// When it ended: said, or held back.
    pub ended_at: Option<u64>,
}

impl Entry {
    /// Its first step's time.
    pub fn start(&self) -> u64 {
        self.steps
            .iter()
            .map(|(at, _)| *at)
            .min()
            .unwrap_or_default()
    }

    /// When the speech ended: the words heard, finished; or, for a turn the
    /// lamp began itself, its first step.
    pub fn speech_ended(&self) -> u64 {
        self.steps
            .iter()
            .find_map(|(at, step)| matches!(step, Step::Heard(_)).then_some(*at))
            .unwrap_or_else(|| self.start())
    }

    /// Whether `at` falls inside the turn.
    fn holds(&self, at: u64) -> bool {
        let start = self.start();
        at >= start && at - start < TURN_LASTS_MS && self.ended_at.is_none_or(|ended| at <= ended)
    }
}

/// The newest turns, and the light last asked for.
#[derive(Debug, Default)]
pub struct Timeline {
    words: bool,
    /// Oldest first.
    entries: VecDeque<Entry>,
    light: Option<Light>,
}

impl Timeline {
    /// A timeline that keeps the words heard and said only when `words`.
    pub fn new(words: bool) -> Self {
        Self {
            words,
            ..Self::default()
        }
    }

    pub fn words(&self) -> bool {
        self.words
    }

    /// The turns kept, newest first.
    pub fn newest(&self) -> Vec<Entry> {
        self.entries.iter().rev().cloned().collect()
    }

    /// Take what `message` says of a turn, if anything.
    pub fn take(&mut self, message: &Message) {
        let at = message.header.at_unix_ms;
        let topic = message.header.topic.as_str();
        if Speech::TOPICS.contains(&topic) {
            let Ok(speech) = message.parse::<Speech>() else {
                return;
            };
            let (true, Some(request)) = (speech.finished, speech.request) else {
                return;
            };
            let words = self.words.then(|| cut(&speech.text));
            let step = if topic == Speech::TOPICS[0] {
                Step::Heard(words)
            } else {
                Step::Said(words)
            };
            self.add(request, at, step);
        } else if topic == VoiceTurn::TOPIC {
            if let Ok(turn) = message.parse::<VoiceTurn>() {
                self.add(turn.request, at, Step::Understood(turn.verdict));
            }
        } else if topic == Light::TOPICS[0] {
            // Asked for again each second: only a change is a thing done.
            let Ok(light) = message.parse::<Light>() else {
                return;
            };
            if self.light.as_ref() == Some(&light) {
                return;
            }
            let did = if light.on {
                format!("light {} at {}", plain(&light.colour), light.level)
            } else {
                "light off".to_owned()
            };
            self.light = Some(light);
            self.did(at, did);
        } else if topic == SkillEvent::TOPIC
            && let Ok(event) = message.parse::<SkillEvent>()
        {
            let stage = match (event.stage, event.allowed) {
                (SkillStage::Triggered, _) => "triggered",
                (SkillStage::Asked, _) => "asked",
                (SkillStage::Answered, Some(false)) => "not allowed",
                (SkillStage::Answered, _) => "allowed",
                (SkillStage::Done, _) => "done",
            };
            self.did(at, format!("skill {} {stage}", plain(&event.skill)));
        }
    }

    /// A step of `request` at `at`: of its turn, or of a new one.
    pub fn add(&mut self, request: u64, at: u64, step: Step) {
        let ends = matches!(
            step,
            Step::Said(_) | Step::Understood(TurnVerdict::HeldBack)
        );
        let found =
            self.entries.iter_mut().rev().find(|entry| {
                entry.request == request && at.abs_diff(entry.start()) < TURN_LASTS_MS
            });
        let entry = match found {
            Some(entry) => entry,
            None => {
                self.entries.push_back(Entry {
                    request,
                    steps: Vec::new(),
                    ended_at: None,
                });
                while self.entries.len() > KEPT {
                    self.entries.pop_front();
                }
                self.entries.back_mut().expect("just pushed")
            }
        };
        entry.steps.push((at, step));
        if ends {
            entry.ended_at = Some(entry.ended_at.map_or(at, |ended| ended.max(at)));
        }
    }

    /// What the lamp did at `at`, in the newest turn that holds it.
    pub fn did(&mut self, at: u64, what: String) {
        if let Some(entry) = self.entries.iter_mut().rev().find(|entry| entry.holds(at)) {
            entry.steps.push((at, Step::Did(what)));
        }
    }
}

/// `text` cut to [`WORD_CHARS`], with a mark when it was.
fn cut(text: &str) -> String {
    let mut chars = text.chars();
    let mut cut: String = chars.by_ref().take(WORD_CHARS).collect();
    if chars.next().is_some() {
        cut.push('…');
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_thirty_turns_are_kept() {
        let mut timeline = Timeline::new(false);
        for request in 1..=40 {
            timeline.add(request, request * 10, Step::Heard(None));
        }
        let newest = timeline.newest();
        assert_eq!(newest.len(), KEPT);
        assert_eq!(newest[0].request, 40);
        assert_eq!(newest[KEPT - 1].request, 11);
    }

    #[test]
    fn what_the_lamp_did_goes_to_the_turn_that_holds_it() {
        let mut timeline = Timeline::new(false);
        timeline.did(500, "light red at 2".into());
        timeline.add(4, 1_000, Step::Heard(None));
        timeline.did(1_200, "light blue at 3".into());
        timeline.add(4, 1_500, Step::Said(None));
        timeline.did(2_000, "light off".into());
        let newest = timeline.newest();
        assert_eq!(
            newest[0].steps,
            vec![
                (1_000, Step::Heard(None)),
                (1_200, Step::Did("light blue at 3".into())),
                (1_500, Step::Said(None)),
            ]
        );
        // A turn held back ends there too.
        timeline.add(5, 3_000, Step::Understood(TurnVerdict::HeldBack));
        timeline.did(3_100, "skill wave done".into());
        assert_eq!(timeline.newest()[0].steps.len(), 1);
    }

    #[test]
    fn a_number_seen_again_much_later_is_another_turn() {
        let mut timeline = Timeline::new(false);
        timeline.add(1, 1_000, Step::Heard(None));
        timeline.add(1, 1_000 + TURN_LASTS_MS, Step::Heard(None));
        assert_eq!(timeline.newest().len(), 2);
    }

    #[test]
    fn words_are_cut() {
        let long = "a".repeat(WORD_CHARS + 5);
        let cut = cut(&long);
        assert_eq!(cut.chars().count(), WORD_CHARS + 1);
        assert!(cut.ends_with('…'));
    }
}
