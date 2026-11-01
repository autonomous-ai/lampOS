//! A bounded observation of a reply that only asks the person to wait, or
//! ends promising a check it never called.
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

/// A holding answer: either the whole answer is a short wait phrase, or its
/// last sentence promises a check (`promises_a_check`). Either way the
/// caller has already required that nothing was called in the turn.
pub(crate) fn holding_only(text: &str) -> bool {
    let normalized = text
        .trim_matches(|c: char| c.is_whitespace() || matches!(c, '.' | '!' | '?' | '…'))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    // Whole phrases only: punctuation and whitespace may surround one;
    // extra words, an answer, a repeat or a quotation never qualify.
    let waiting = matches!(
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
    );
    waiting || promises_a_check(text)
}

/// Promise openers for an answer's last sentence (#4711). "let me see" only
/// with what it will see: bare "Let me see." is a filler or asks to be
/// shown something. "let me look" counts: the lamp's own `LOOKING` line is
/// only said in a turn that called a look, so a model's "Let me look." with
/// no call is a promise left unkept.
const PROMISES: &[&str] = &[
    "let me check",
    "let me look",
    "let me find",
    "let me search",
    "let me see how",
    "let me see what",
    "let me see if",
    "let me see whether",
    "i'll check",
    "i'll look",
    "i'll find",
    "i'll search",
    "i will check",
    "i will look",
    "i will find",
    "i will search",
];

/// The answer's last sentence promises to check, look, see or find out, and
/// says nothing after it: any earlier sentence may be anything (the lamp-4ace
/// run of 31 Oct: "…do you mean the Aeron chair? Let me check the price.").
/// A question at the end, or the answer given in the same sentence ("Let me
/// check: it's $1,895.") or after it, never qualifies.
fn promises_a_check(text: &str) -> bool {
    let text = text.trim();
    if text.ends_with('?') {
        return false;
    }
    let body = text.trim_end_matches(['.', '!', '…']);
    // A sentence ends at a terminator followed by a space, so "$1.5" or
    // "4.5 s" never splits one.
    let last = body
        .char_indices()
        .rev()
        .find(|&(at, c)| {
            matches!(c, '.' | '!' | '?' | '…')
                && body[at + c.len_utf8()..].starts_with(char::is_whitespace)
        })
        .map_or(body, |(at, c)| &body[at + c.len_utf8()..]);
    let last = last
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
        .replace('’', "'");
    !last.contains([':', ';', ',', '—', '–', '"', '“'])
        && PROMISES.iter().any(|opener| {
            last.strip_prefix(opener)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        })
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
            // A check with its answer is no holding answer. "Let me look."
            // and "Let me check the light" moved to the promises (#4711):
            // a promise left unkept with no call is what the continuation
            // is for.
            "Let me check: it's $1,895.",
            "Let me check that one. It's about $1,800.",
        ] {
            assert!(!holding_only(phrase), "{phrase}");
        }
    }

    #[test]
    fn a_last_sentence_promising_a_check_is_a_holding_answer() {
        for phrase in [
            // The first builder's lamp-4ace run, 31 Oct (#4711).
            "Let me look.",
            "Let me see how it's going.",
            "Hmm, \"Aaron Chan\" — do you mean the Aeron chair? Let me check the price.",
            // #4684's price lines, the promises (that run's first two made
            // their search, so the call keeps them out).
            "Let me check.",
            "I'll check that for you.",
            "Hmm — Herman Miller's Aeron, right? Let me look.",
            "I’ll look it up",
            "Let me check the light",
            "Let me see if it's open.",
        ] {
            assert!(holding_only(phrase), "{phrase}");
        }
        for phrase in [
            // #4684's price lines, the answers.
            "$499 on Autonomous's own page for the ErgoChair Pro.",
            "Four hundred ninety-nine dollars, straight from Autonomous's own page.",
            "Then $449 it is — …I'm not going to argue with the page.",
            "Ah, then 449 it is — the page in front of you wins. My search must've pulled an older price.",
            // A check followed by its answer.
            "Let me check. It's $1,895.",
            "Let me check — it's $1,895.",
            "Let me check... it's 499.",
            // A bare "let me see" may ask to be shown something.
            "Let me see.",
            "Let me see it.",
            "Let me see that!",
            // A question, alone or last.
            "Do you mean the Aeron chair?",
            "Let me check the price, shall I?",
            "Let me know if you need anything.",
            "\"Let me check\"",
            "Let me checkmate you.",
        ] {
            assert!(!holding_only(phrase), "{phrase}");
        }
    }

    #[test]
    fn a_promise_after_a_call_in_the_turn_is_not_holding() {
        let line = "Hmm, \"Aaron Chan\" — do you mean the Aeron chair? Let me check the price.";
        for call in [false, true] {
            let mut track = Tracker::default();
            let of = lineage(1, 2);
            start(&mut track, of);
            text(&mut track, of, line);
            if call {
                track.called(of);
            }
            assert_eq!(finish(&mut track, of), (!call).then_some(of));
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
