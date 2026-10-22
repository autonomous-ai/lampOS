//! The lamp's own words heard back (#2997), off unless
//! `LAMP_SECOND_OWN_ECHO=on`. The second route only.
//!
//! On lamp-4ace on 22 October (14:40–16:45) the journal's count
//! (`of its N words, M in the lamp's answer`, [`super::whose_words`]) showed
//! 62 of 214 heard lines held some of the lamp's own just-spoken words, 8
//! were mostly its own (M ≥ N/2), and 5 of those 8 were answered: the lamp
//! answered itself, which the owner heard as being misheard.
//!
//! With the switch, a line begun while the lamp's answer could be heard
//! (`Start`'s `over_the_lamps_voice`), or within [`TAIL_US`] after the time
//! its words sent would have played to their end (the same end the rule's
//! conversation goes on from, [`super::Did::Said`]), is judged against the
//! lamp's latest answer so far with the journal's own word matching
//! ([`super::word_key`]):
//!
//! - mostly the lamp's own words (M ≥ N/2, at least [`HELD_LEAST`]): held as its echo, not answered;
//! - fewer, all of them one run at the line's start or its end, at least
//!   [`STRIP_RUN`] words: that run is taken out of what the model is sent.
//!   Own words anywhere else, or one alone, leave the line as heard.
//!
//! A person repeating the lamp is never held, nor has words taken out: a line
//! begun later than the tail, one with the lamp's name
//! ([`crate::attention::called_by_name_on`], or only the name called), a
//! question of its own ([`crate::remarks::questions`] with at least one word
//! that is not the lamp's: "Did you say five o'clock?"), or one begun after
//! the lamp's words when they ended on a question (its answer: "Yes, start
//! it." after "Shall I start it?") is the person's, sent as heard. The
//! journal says counts only, never words.

/// How long after the lamp's words would have played to their end a line
/// may begin and still be judged as its echo: 1.5 s. The end is when the
/// words sent would have played, as if each packet plays as it is sent, so
/// the speaker's own delay and the room's reverberation come after it; and a
/// person who heard the answer through needs longer than that to begin a
/// line that repeats it, which is then theirs.
pub const TAIL_US: u64 = 1_500_000;

/// The fewest of the lamp's own words, one run at a line's start or end,
/// that are taken out of a mixed line: one word alone ("so", "no", "it") is
/// as likely the person's as the lamp's.
pub const STRIP_RUN: usize = 2;

/// The fewest of the lamp's own words a line holds to be held as its echo:
/// a short line sharing two common words with the answer ("it is raining"
/// over "it is five") is as likely the person's barging in.
pub const HELD_LEAST: usize = 3;

/// The heard line, as the turn knows it.
pub(super) struct Line<'a> {
    pub request: u64,
    pub heard: &'a str,
    /// When its first packet was read from the microphone; 0 when none was.
    pub began_us: u64,
    /// Begun while the lamp's answer could be heard.
    pub over_the_lamps_voice: bool,
    /// Only the lamp's name was called (`ProviderInput::Called`).
    pub called: bool,
}

/// The lamp's latest answer so far: its request, its words sent to the
/// voice, and when they would have played to their end.
pub(super) struct Said<'a> {
    pub request: u64,
    pub words: &'a str,
    pub until_us: u64,
}

/// What becomes of a heard line.
#[derive(Debug, PartialEq)]
pub(super) enum Echo {
    /// Answered as heard, with the journal's line when it was mostly the
    /// lamp's own words but the person's by a guard.
    Answered(Option<String>),
    /// Held as the lamp's own echo, with the journal's line.
    Held(String),
    /// Answered with the lamp's own words at its start or end taken out:
    /// what the model is sent, and the journal's line.
    Stripped(String, String),
}

/// Judge `line` against the lamp's answer `said`.
pub(super) fn judge(line: &Line<'_>, said: Option<&Said<'_>>) -> Echo {
    let request = line.request;
    let words: Vec<&str> = line.heard.split_whitespace().collect();
    let n = words.len();
    let Some(said) = said.filter(|said| {
        said.request != 0 && said.request < request && !said.words.trim().is_empty()
    }) else {
        return Echo::Answered(None);
    };
    if n == 0 {
        return Echo::Answered(None);
    }
    let lamps: std::collections::HashSet<String> =
        said.words.split_whitespace().map(super::word_key).collect();
    let own: Vec<bool> = words
        .iter()
        .map(|word| {
            let key = super::word_key(word);
            !key.is_empty() && lamps.contains(&key)
        })
        .collect();
    let m = own.iter().filter(|own| **own).count();
    if m == 0 {
        return Echo::Answered(None);
    }
    let mostly = 2 * m >= n && m >= HELD_LEAST;
    let within = line.over_the_lamps_voice
        || line.began_us != 0
            && said.until_us != 0
            && line.began_us <= said.until_us.saturating_add(TAIL_US);
    let guard = if !within {
        Some("it began after the lamp's words and their 1.5 s tail")
    } else if line.called || crate::attention::called_by_name_on(line.heard, true) {
        Some("it has the lamp's name")
    } else if m < n && crate::remarks::questions(line.heard) {
        Some("it is a question of its own")
    } else if !line.over_the_lamps_voice && said.words.trim_end().ends_with('?') {
        Some("it began after the lamp's question")
    } else {
        None
    };
    let of = said.request;
    if let Some(why) = guard {
        return Echo::Answered(mostly.then(|| {
            format!(
                "lamp-voice second: request {request}: of its {n} words, {m} in the lamp's answer to request {of}, but {why}: answered as the person's (LAMP_SECOND_OWN_ECHO)"
            )
        }));
    }
    let when = if line.over_the_lamps_voice {
        "over the lamp's voice"
    } else {
        "within 1.5 s of the lamp's words"
    };
    if mostly {
        return Echo::Held(format!(
            "lamp-voice second: request {request}: its answer is held back: the lamp's own echo: of its {n} words, {m} in the lamp's answer to request {of}, begun {when} (LAMP_SECOND_OWN_ECHO)"
        ));
    }
    let leading = own.iter().take_while(|own| **own).count();
    let trailing = own.iter().rev().take_while(|own| **own).count();
    let (kept, at) = if leading == m && m >= STRIP_RUN {
        (&words[m..], "start")
    } else if trailing == m && m >= STRIP_RUN {
        (&words[..n - m], "end")
    } else {
        return Echo::Answered(None);
    };
    Echo::Stripped(
        kept.join(" "),
        format!(
            "lamp-voice second: request {request}: {m} of its {n} words, the lamp's own answer to request {of} at its {at}, are taken out of what the model is sent (LAMP_SECOND_OWN_ECHO)"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lamp's answer to request 3, played until 10 s.
    const ANSWER: &str = "It's five o'clock in Hanoi, and the meeting starts at six.";
    const UNTIL_US: u64 = 10_000_000;

    fn said() -> Said<'static> {
        Said {
            request: 3,
            words: ANSWER,
            until_us: UNTIL_US,
        }
    }

    fn line(heard: &str, began_us: u64, over_the_lamps_voice: bool) -> Line<'_> {
        Line {
            request: 5,
            heard,
            began_us,
            over_the_lamps_voice,
            called: false,
        }
    }

    /// The journal's count and this judgement agree on whose words are
    /// whose: the same matching.
    #[test]
    fn its_counts_are_the_journals() {
        let heard = "the meeting starts at six";
        let Echo::Held(journal) = judge(&line(heard, 9_000_000, true), Some(&said())) else {
            panic!("held")
        };
        let count = super::super::whose_words(5, heard, None, Some((3, ANSWER.to_owned())));
        assert_eq!(
            count.as_deref(),
            Some(
                "lamp-voice second: request 5: of its 5 words, 5 in the lamp's answer to request 3 so far"
            )
        );
        assert!(journal.contains("of its 5 words, 5 in the lamp's answer to request 3"));
    }

    #[test]
    fn mostly_its_own_words_during_playback_are_held_as_its_echo() {
        // 4 of 6 words the lamp's, over its voice.
        assert_eq!(
            judge(
                &line("and the meeting starts uh huh", 9_000_000, true),
                Some(&said())
            ),
            Echo::Held(
                "lamp-voice second: request 5: its answer is held back: the lamp's own echo: of its 6 words, 4 in the lamp's answer to request 3, begun over the lamp's voice (LAMP_SECOND_OWN_ECHO)".to_owned()
            )
        );
        // Half its words, begun 1 s after the lamp's would have ended.
        assert_eq!(
            judge(
                &line("starts at six okay right now", UNTIL_US + 1_000_000, false),
                Some(&said())
            ),
            Echo::Held(
                "lamp-voice second: request 5: its answer is held back: the lamp's own echo: of its 6 words, 3 in the lamp's answer to request 3, begun within 1.5 s of the lamp's words (LAMP_SECOND_OWN_ECHO)".to_owned()
            )
        );
    }

    #[test]
    fn a_short_line_with_two_of_its_words_is_answered() {
        // Two common words of the answer in three heard over its voice: as
        // likely the person barging in, so not held.
        assert_eq!(
            judge(&line("it is raining", 5_000_000, true), Some(&said())),
            Echo::Answered(None)
        );
    }

    #[test]
    fn the_persons_line_with_a_few_of_its_words_is_answered_as_heard() {
        // "the" and "at" are the lamp's too, in its middle: 2 of 9.
        assert_eq!(
            judge(
                &line(
                    "remind me about the dentist at noon tomorrow please",
                    9_000_000,
                    true
                ),
                Some(&said())
            ),
            Echo::Answered(None)
        );
    }

    #[test]
    fn a_repeat_back_after_the_tail_is_answered() {
        // Every word the lamp's, begun 2 s after its words ended.
        assert_eq!(
            judge(
                &line("five o'clock in Hanoi", UNTIL_US + 2_000_000, false),
                Some(&said())
            ),
            Echo::Answered(Some(
                "lamp-voice second: request 5: of its 4 words, 4 in the lamp's answer to request 3, but it began after the lamp's words and their 1.5 s tail: answered as the person's (LAMP_SECOND_OWN_ECHO)".to_owned()
            ))
        );
        // A question of its own repeats it back, even within the tail.
        assert_eq!(
            judge(
                &line(
                    "Did you say five o'clock in Hanoi?",
                    UNTIL_US + 500_000,
                    false
                ),
                Some(&said())
            ),
            Echo::Answered(Some(
                "lamp-voice second: request 5: of its 7 words, 4 in the lamp's answer to request 3, but it is a question of its own: answered as the person's (LAMP_SECOND_OWN_ECHO)".to_owned()
            ))
        );
        // Over its voice too, with no question mark: a question word first.
        assert!(matches!(
            judge(
                &line("is the meeting at six", 9_000_000, true),
                Some(&said())
            ),
            Echo::Answered(Some(_))
        ));
    }

    #[test]
    fn a_line_with_the_lamps_name_is_the_persons() {
        assert!(matches!(
            judge(
                &line("Lamp, the meeting starts at six", 9_000_000, true),
                Some(&said())
            ),
            Echo::Answered(Some(journal)) if journal.contains("it has the lamp's name")
        ));
        let called = Line {
            called: true,
            ..line("the meeting starts at six", 9_000_000, true)
        };
        assert!(matches!(
            judge(&called, Some(&said())),
            Echo::Answered(Some(_))
        ));
    }

    #[test]
    fn a_reply_to_the_lamps_question_is_the_persons() {
        let asked = Said {
            words: "Shall I start the timer?",
            ..said()
        };
        assert!(matches!(
            judge(&line("yes start the timer", UNTIL_US + 300_000, false), Some(&asked)),
            Echo::Answered(Some(journal)) if journal.contains("after the lamp's question")
        ));
        // Its question heard back over its own voice is still its echo.
        assert!(matches!(
            judge(&line("shall I start the timer", 9_000_000, true), Some(&asked)),
            Echo::Held(_)
        ));
    }

    #[test]
    fn its_own_words_first_or_last_are_taken_out_and_in_the_middle_kept() {
        assert_eq!(
            judge(
                &line(
                    "starts at six set an alarm for seven thirty please",
                    9_000_000,
                    true
                ),
                Some(&said())
            ),
            Echo::Stripped(
                "set an alarm for seven thirty please".to_owned(),
                "lamp-voice second: request 5: 3 of its 10 words, the lamp's own answer to request 3 at its start, are taken out of what the model is sent (LAMP_SECOND_OWN_ECHO)".to_owned()
            )
        );
        assert_eq!(
            judge(
                &line(
                    "set an alarm for seven thirty please in Hanoi",
                    UNTIL_US + 1_000_000,
                    false
                ),
                Some(&said())
            ),
            Echo::Stripped(
                "set an alarm for seven thirty please".to_owned(),
                "lamp-voice second: request 5: 2 of its 9 words, the lamp's own answer to request 3 at its end, are taken out of what the model is sent (LAMP_SECOND_OWN_ECHO)".to_owned()
            )
        );
        // In the middle: sent as heard.
        assert_eq!(
            judge(
                &line(
                    "set an alarm starts at six for seven thirty please",
                    9_000_000,
                    true
                ),
                Some(&said())
            ),
            Echo::Answered(None)
        );
        // Own words at both ends, or one alone: sent as heard.
        for heard in [
            "six set an alarm for seven thirty please Hanoi",
            "six set an alarm for seven thirty please",
        ] {
            assert_eq!(
                judge(&line(heard, 9_000_000, true), Some(&said())),
                Echo::Answered(None),
                "{heard}"
            );
        }
    }

    #[test]
    fn nothing_to_judge_against_leaves_the_line_as_heard() {
        let heard = "the meeting starts at six";
        assert_eq!(
            judge(&line(heard, 9_000_000, true), None),
            Echo::Answered(None)
        );
        for said in [
            Said {
                request: 5,
                ..said()
            },
            Said {
                request: 0,
                ..said()
            },
            Said {
                words: "  ",
                ..said()
            },
        ] {
            assert_eq!(
                judge(&line(heard, 9_000_000, true), Some(&said)),
                Echo::Answered(None)
            );
        }
        // No packet's time and not over its voice: not judged as within.
        assert!(matches!(
            judge(&line(heard, 0, false), Some(&said())),
            Echo::Answered(Some(_))
        ));
    }
}
