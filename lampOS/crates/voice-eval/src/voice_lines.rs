//! How `score` and `cost` know a voice line: its route, its request, and
//! whether it says a request was asked, began to answer or was held back
//! (the first builder on #1712, 19 Oct 23:29: `cost` counted 34 of Live's
//! requests and 0 answered while `score` read them right). Both read the
//! journal through this one matcher, so the two can't drift.
//!
//! The lines are the voice's own (`crates/voice`):
//! - the second route's request, `lamp-voice second: request N heard W
//!   words, the last M ms after the line ended`, and its answer, `request N
//!   first answer audio M ms after the words were heard`;
//! - Live's request, any `lamp-voice provider: request N…` line (not a
//!   sound's `a sound (request N)`, a reminder's `reminder request N` or a
//!   picture's `went with request N`, which name it later in the line); its
//!   answer begun, its first sound (`request N: first sound…`, said only
//!   with `LAMP_NAME_SIGNAL`) or its answer judged played (`request N: its
//!   answer is played: …`, said whenever attention judges); and held back,
//!   `request N: its answer is held back: …`.

use std::collections::BTreeSet;

/// The voice route a line is from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Route {
    /// The second route (Deepgram and the cascade), `lamp-voice second: `.
    Second,
    /// Gemini Live, `lamp-voice provider: `.
    Live,
}

/// A request: its route, its process (`lamp-voice[PID]`, since numbers start
/// again when the service does) and its number.
pub type Request = (Route, u64, u64);

/// One of the voice's journal lines, from its route's prefix on.
#[derive(Clone, Copy, Debug)]
pub struct VoiceLine<'a> {
    pub route: Route,
    pub process: u64,
    /// The line from `lamp-voice second: ` or `lamp-voice provider: ` on.
    pub text: &'a str,
}

const SECOND: &str = "lamp-voice second: ";
const LIVE: &str = "lamp-voice provider: ";

impl<'a> VoiceLine<'a> {
    /// The voice line in a journal line, or `None` for any other.
    pub fn of(line: &'a str) -> Option<Self> {
        let process = number_after(line, "lamp-voice[").unwrap_or(0);
        let (at, route) = match (line.find(SECOND), line.find(LIVE)) {
            (Some(second), Some(live)) if live < second => (live, Route::Live),
            (Some(second), _) => (second, Route::Second),
            (None, Some(live)) => (live, Route::Live),
            (None, None) => return None,
        };
        Some(Self {
            route,
            process,
            text: &line[at..],
        })
    }

    /// The line after its route's prefix.
    fn said(&self) -> &'a str {
        let prefix = match self.route {
            Route::Second => SECOND,
            Route::Live => LIVE,
        };
        &self.text[prefix.len()..]
    }

    /// The request the line names, its first `request N`.
    pub fn request(&self) -> Option<Request> {
        number_after(self.text, "request ").map(|number| (self.route, self.process, number))
    }

    /// The request this line says was asked: the second route's words
    /// heard, or any Live line that begins with its request.
    pub fn asked(&self) -> Option<Request> {
        let asked = match self.route {
            Route::Second => self.text.contains(" words, the last "),
            Route::Live => self.said().starts_with("request "),
        };
        asked.then(|| self.request()).flatten()
    }

    /// The reason the line's answer is played for, when it says so.
    pub fn played(&self) -> Option<&'a str> {
        self.text.split("its answer is played: ").nth(1)
    }

    /// The reason the line's answer is held back for, when it says so.
    pub fn held(&self) -> Option<&'a str> {
        self.text.split("its answer is held back: ").nth(1)
    }

    /// The second route's first answer audio, in ms after the words.
    pub fn first_answer_audio(&self) -> Option<u64> {
        number_after(self.text, "first answer audio ")
    }

    /// Whether Live's answer began on this line: its first sound or its
    /// answer judged played.
    pub fn live_began(&self) -> bool {
        self.route == Route::Live
            && (self.text.contains(": first sound") || self.played().is_some())
    }
}

/// Requests asked and answered, by the lines above, for one arm.
#[derive(Debug, Default)]
pub struct Answers {
    pub asked: BTreeSet<Request>,
    /// The second route's requests with a first answer audio.
    pub second_answered: BTreeSet<Request>,
    /// Live's requests whose answer began, and those held back.
    sounded: BTreeSet<Request>,
    held: BTreeSet<Request>,
}

impl Answers {
    pub fn read(&mut self, line: &VoiceLine) {
        self.asked.extend(line.asked());
        let request = line.request();
        if line.route == Route::Second && line.first_answer_audio().is_some() {
            self.second_answered.extend(request);
        }
        if line.live_began() {
            self.sounded.extend(request);
        }
        if line.route == Route::Live && line.held().is_some() {
            self.held.extend(request);
        }
    }

    /// The requests answered: the second route's with a first answer
    /// audio, and Live's that began and were not held back.
    pub fn answered(&self) -> BTreeSet<Request> {
        let live = self.sounded.difference(&self.held).copied();
        self.second_answered.iter().copied().chain(live).collect()
    }
}

/// The number right after `before` in `text`.
fn number_after(text: &str, before: &str) -> Option<u64> {
    let rest = &text[text.find(before)? + before.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live's own lines (`crates/voice/src/provider_worker.rs`, `sight.rs`):
    /// a sound's, a reminder's and a picture's name their request later in
    /// the line and are not asked by a person.
    #[test]
    fn a_live_request_is_one_its_line_begins_with() {
        let asked = |line| VoiceLine::of(line).and_then(|line| line.asked());
        assert_eq!(
            asked(
                "Oct 19 20:31:29 lamp-4ace lamp-voice[12]: lamp-voice provider: request 5 got no answer in 4000 ms; closed as unanswered, connection kept"
            ),
            Some((Route::Live, 12, 5))
        );
        assert_eq!(
            asked(
                "Oct 19 20:31:01 lamp-4ace lamp-voice[12]: lamp-voice provider: request 3: first sound 300 ms after the model said it heard its name"
            ),
            Some((Route::Live, 12, 3))
        );
        for line in [
            "Oct 19 20:31:30 lamp-4ace lamp-voice[12]: lamp-voice provider: a sound (request 6) got no answer in 900 ms, as a sound should; closed, connection kept",
            "Oct 19 20:31:31 lamp-4ace lamp-voice[12]: lamp-voice provider: reminder request 7 got no answer in 900 ms; closed, connection kept",
            "Oct 19 20:31:32 lamp-4ace lamp-voice[12]: lamp-voice provider: sight: a picture of 41230 bytes, 120 ms old, went with request 8 (look 2)",
        ] {
            assert_eq!(asked(line), None, "{line}");
        }
    }
}
