//! A bounded observation of a reply that only asks the person to wait.
//!
//! This never sends input or grants speech. The coordinator decides whether
//! one continuation can still be asked on the session that said the phrase.

use lamp_gemini::{Event, Lineage, Notice};

pub(crate) const ENVIRONMENT: &str = "LAMP_HOLDING_ANSWER";
pub(crate) const FLAG: &str = "--holding-answer";
pub(crate) const NOTE: &str = "Go on: you said you would, so say it now.";
const MAX_TRANSCRIPT_BYTES: usize = 256;

pub(crate) fn on() -> bool {
    enabled(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

pub(crate) fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if enabled(value) {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

pub(crate) fn take(flags: &[String]) -> (bool, &[String]) {
    match flags.first().map(String::as_str) {
        Some(FLAG) => (true, &flags[1..]),
        _ => (false, flags),
    }
}

struct Answer {
    lineage: Lineage,
    text: String,
    qualified: bool,
}

/// One current answer, and the newest identity seen so stale starts cannot
/// resurrect a finished or failed answer. Constructed only with the option on.
#[derive(Default)]
pub(crate) struct Tracker {
    newest: Option<Lineage>,
    answer: Option<Answer>,
}

impl Tracker {
    /// Count every function call before any tool, attention, or policy veto.
    pub(crate) fn called(&mut self, lineage: Lineage) {
        if let Some(answer) = self
            .answer
            .as_mut()
            .filter(|answer| answer.lineage == lineage)
        {
            answer.qualified = false;
        }
    }

    pub(crate) fn clear(&mut self) {
        self.answer = None;
    }

    /// The caller observes each transcript once, after a kept policy notice
    /// is released, and before speech may be withheld. Only a real idle
    /// completion is evidence of the whole answer; a closed connection is not.
    pub(crate) fn observe(&mut self, notice: &Notice) -> Option<Lineage> {
        match notice {
            Notice::Event(Event::InputStarted { lineage, .. }) => {
                if self.newest.is_some_and(|newest| {
                    lineage.request.get() < newest.request.get()
                        || lineage.session.get() < newest.session.get()
                        || *lineage == newest
                }) {
                    return None;
                }
                self.newest = Some(*lineage);
                self.answer = Some(Answer {
                    lineage: *lineage,
                    text: String::with_capacity(MAX_TRANSCRIPT_BYTES),
                    qualified: true,
                });
            }
            Notice::Event(Event::OutputTranscript { lineage, text, .. }) => {
                if let Some(answer) = self
                    .answer
                    .as_mut()
                    .filter(|answer| answer.lineage == *lineage && answer.qualified)
                {
                    if text.len() > MAX_TRANSCRIPT_BYTES.saturating_sub(answer.text.len()) {
                        answer.qualified = false;
                    } else {
                        answer.text.push_str(text);
                    }
                }
            }
            Notice::Event(Event::ToolCall { lineage, .. }) => self.called(*lineage),
            Notice::Event(Event::TurnComplete {
                lineage,
                idle: true,
            }) => {
                if self
                    .answer
                    .as_ref()
                    .is_some_and(|answer| answer.lineage == *lineage)
                {
                    let answer = self.answer.take().expect("matching answer");
                    if answer.qualified && holding_only(&answer.text) {
                        return Some(*lineage);
                    }
                }
            }
            Notice::Event(Event::Interrupted { lineage } | Event::Unanswered { lineage, .. })
            | Notice::TurnLost { lineage, .. }
            | Notice::Settled { lineage, .. } => {
                if self
                    .answer
                    .as_ref()
                    .is_some_and(|answer| answer.lineage == *lineage)
                {
                    self.clear();
                }
            }
            Notice::Event(Event::InputDiscarded {
                session, through, ..
            }) => {
                if self.answer.as_ref().is_some_and(|answer| {
                    answer.lineage.session == *session
                        && answer.lineage.request.get() <= through.get()
                }) {
                    self.clear();
                }
            }
            Notice::Event(
                Event::Refused { session, .. } | Event::BarrierAssumed { session, .. },
            ) => {
                if self
                    .answer
                    .as_ref()
                    .is_some_and(|answer| answer.lineage.session == *session)
                {
                    self.clear();
                }
            }
            Notice::Unavailable { .. } => self.clear(),
            _ => {}
        }
        None
    }
}

/// Whole phrases only. Punctuation and whitespace may surround a short
/// holding phrase; extra words, an answer, or a quotation never qualify.
pub(crate) fn holding_only(text: &str) -> bool {
    let normalized = text
        .trim_matches(|c: char| c.is_whitespace() || matches!(c, '.' | '!' | '?' | '…'))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "let me think"
            | "let me think for a moment"
            | "let me think for a second"
            | "one moment"
            | "just one moment"
            | "one second"
            | "just a second"
            | "just a moment"
            | "give me a second"
            | "give me a moment"
            // A check promised with no call (#4684's line 3 on lamp-4ace on
            // 31 Oct: "Let me check that one." about an Aeron chair, then
            // nothing): the one continuation lets the search be made.
            | "let me check"
            | "let me check that"
            | "let me check that one"
            | "let me check on that"
            | "let me look that up"
            | "let me look it up"
            | "let me find out"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_gemini::{RequestId, SessionId};

    fn lineage(session: u64, request: u64) -> Lineage {
        Lineage {
            session: SessionId::new(session).unwrap(),
            request: RequestId::new(request).unwrap(),
        }
    }
    fn start(track: &mut Tracker, lineage: Lineage) {
        track.observe(&Notice::Event(Event::InputStarted {
            lineage,
            waiting_for_barrier: false,
        }));
    }
    fn text(track: &mut Tracker, lineage: Lineage, text: &str) {
        track.observe(&Notice::Event(Event::OutputTranscript {
            lineage,
            text: text.into(),
            finished: false,
        }));
    }
    fn finish(track: &mut Tracker, lineage: Lineage) -> Option<Lineage> {
        track.observe(&Notice::Event(Event::TurnComplete {
            lineage,
            idle: true,
        }))
    }

    #[test]
    fn only_the_explicit_on_value_enables_holding_answers() {
        for value in [None, Some(""), Some("off"), Some("true"), Some("ON")] {
            assert!(!enabled(value));
        }
        assert!(enabled(Some("on")));
    }

    #[test]
    fn holding_worker_arguments_are_empty_unless_on_and_preserve_the_following_flags() {
        for value in [None, Some(""), Some("off"), Some("true"), Some("ON")] {
            assert!(arguments(value).is_empty(), "{value:?}");
        }
        let mut flags = arguments(Some("on"));
        assert_eq!(flags, [FLAG]);
        flags.push("--camera".into());
        let (on, rest) = take(&flags);
        assert!(on);
        assert_eq!(rest, ["--camera"]);
        let (on, rest) = take(&flags[1..]);
        assert!(!on);
        assert_eq!(rest, ["--camera"]);
    }

    #[test]
    fn whole_holding_phrases_accept_case_spacing_and_terminal_punctuation() {
        for phrase in [
            "Let me think",
            "One moment.",
            "give me a second!",
            "  LET ME\nTHINK...  ",
            "Just a moment.",
            "Let me think for a second.",
            "Let me check that one.",
            "let me check",
            "Let me look that up…",
            "Let me find out!",
        ] {
            assert!(holding_only(phrase), "{phrase}");
        }
        for phrase in [
            "",
            "  ...",
            "Let me see",
            "  LET ME\nSEE…  ",
            "Let me see. It is yellow.",
            "One moment of silence",
            "Give me a second opinion",
            "Let me think about your answer: four.",
            "\"Let me think\"",
            "I said let me see",
            "Let me see: four",
            "one moment; the answer is four",
            "Let me think. Let me think.",
            // A check with its answer, or a look (the lamp's own
            // `LOOKING`), is no holding answer.
            "Let me check: it's $1,895.",
            "Let me check that one. It's about $1,800.",
            "Let me look.",
            "Let me check the light",
        ] {
            assert!(!holding_only(phrase), "{phrase}");
        }
    }

    #[test]
    fn chunked_phrase_finishes_once_and_only_at_idle() {
        let mut track = Tracker::default();
        let of = lineage(1, 2);
        start(&mut track, of);
        text(&mut track, of, "Let me ");
        assert!(
            track
                .observe(&Notice::Event(Event::GenerationComplete { lineage: of }))
                .is_none()
        );
        assert!(
            track
                .observe(&Notice::Event(Event::TurnComplete {
                    lineage: of,
                    idle: false
                }))
                .is_none()
        );
        text(&mut track, of, "think.");
        assert_eq!(finish(&mut track, of), Some(of));
        assert_eq!(finish(&mut track, of), None);
        start(&mut track, of);
        text(&mut track, of, "Let me think.");
        assert_eq!(finish(&mut track, of), None);
    }

    #[test]
    fn overflow_never_recovers_even_when_the_retained_prefix_is_a_phrase() {
        let mut track = Tracker::default();
        let of = lineage(1, 2);
        start(&mut track, of);
        text(&mut track, of, "Let me think");
        text(&mut track, of, &" ".repeat(MAX_TRANSCRIPT_BYTES));
        text(&mut track, of, ".");
        assert_eq!(finish(&mut track, of), None);
    }

    #[test]
    fn any_call_or_interruption_disqualifies_the_whole_answer() {
        for interrupt in [false, true] {
            let mut track = Tracker::default();
            let of = lineage(1, 2);
            start(&mut track, of);
            text(&mut track, of, "One moment.");
            if interrupt {
                track.observe(&Notice::Event(Event::Interrupted { lineage: of }));
            } else {
                track.called(of);
            }
            text(&mut track, of, "");
            assert_eq!(finish(&mut track, of), None);
        }
    }

    #[test]
    fn stale_session_request_and_start_cannot_replace_the_current_answer() {
        let mut track = Tracker::default();
        let old = lineage(1, 2);
        let of = lineage(2, 3);
        start(&mut track, old);
        text(&mut track, old, "One moment.");
        start(&mut track, of);
        start(&mut track, old);
        text(&mut track, old, "The answer is four.");
        track.called(old);
        assert_eq!(finish(&mut track, old), None);
        text(&mut track, of, "Let me think.");
        assert_eq!(finish(&mut track, of), Some(of));
    }

    #[test]
    fn a_lost_connection_or_failed_turn_never_becomes_a_holding_answer() {
        for failure in 0..3 {
            let mut track = Tracker::default();
            let of = lineage(1, 2);
            start(&mut track, of);
            text(&mut track, of, "Let me think.");
            let notice = match failure {
                0 => Notice::Unavailable {
                    error: lamp_gemini::Error::NotConnected,
                },
                1 => Notice::TurnLost {
                    lineage: of,
                    stage: lamp_gemini::Stage::Responding,
                    error: lamp_gemini::Error::NotConnected,
                },
                _ => Notice::Settled {
                    lineage: of,
                    error: lamp_gemini::Error::NotConnected,
                },
            };
            assert_eq!(track.observe(&notice), None);
            assert_eq!(finish(&mut track, of), None);
        }
    }
}
