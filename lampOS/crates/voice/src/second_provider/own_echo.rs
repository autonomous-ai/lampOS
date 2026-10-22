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
//! lamp's latest answer so far. A heard word is the lamp's own only inside a
//! run of at least [`STRIP_RUN`] heard words that the answer says in the
//! same order, one after another ([`super::word_key`] matching); the
//! journal's count matches words as a set, so its K can be higher.
//!
//! - every word the lamp's own, in such runs, and at least [`HELD_LEAST`]
//!   words: held as its echo, not answered;
//! - one such run at the line's start or its end, the rest words the lamp
//!   did not say: that run is taken out of what the model is sent. Own words
//!   anywhere else leave the line as heard.
//!
//! A line that adds a word the lamp did not say is never held. A line is the
//! person's and sent as heard, never held nor cut, when it begins later than
//! the tail, has the lamp's name ([`crate::attention::called_by_name_on`],
//! or only the name called), is a question of its own
//! ([`crate::remarks::questions`] with at least one word that is not the
//! lamp's: "Did you say five o'clock?"), begins after the lamp's words when
//! they ended on a question ("Yes, start it." after "Shall I start it?"), or
//! answers an offer in the answer ([`OFFERS`]): begun after the lamp's
//! words, or over its voice with a word the lamp did not say. Only a line
//! heard over the lamp's voice that is nothing but the offer's own words is
//! still held. The journal says counts only, never words.

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

/// Words that make the lamp's answer an offer the person may take up, each
/// matched as a run of [`super::word_key`] words in the answer's last
/// sentence only (so "I can't" is not "I can", and "I can hear you. It's
/// five." is no offer): an offer is what the lamp ends on. A sign-off ("Let
/// me know if you need anything") is not one. A line answering one ("set a reminder for
/// six" after "I can set a reminder for six.") repeats the lamp's words
/// because it accepts them, so it is the person's, "?" or not. Kept short
/// and English on purpose: a missed offer only leaves the line judged as
/// before.
pub const OFFERS: &[&str] = &[
    "i can",
    "i could",
    "shall i",
    "should i",
    "want me to",
    "would you like",
    "do you want",
    "if you want",
    "if you'd like",
    "if you like",
];

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
    /// Answered as heard, with the journal's line when it would have been
    /// held or had words taken out but is the person's by a guard.
    Answered(Option<String>),
    /// Held as the lamp's own echo, with the journal's line.
    Held(String),
    /// Answered with the lamp's own words at its start or end taken out:
    /// what the model is sent, and the journal's line.
    Stripped(String, String),
}

/// The answer's words as [`super::word_key`] keys, empty ones left out.
fn keys(words: &str) -> Vec<String> {
    words
        .split_whitespace()
        .map(super::word_key)
        .filter(|key| !key.is_empty())
        .collect()
}

/// Whether the last sentence of the lamp's `answer` holds an offer of
/// [`OFFERS`].
fn offers(answer: &str) -> bool {
    let last = answer
        .split(['.', '!', '?'])
        .map(str::trim)
        .rfind(|sentence| !sentence.is_empty())
        .unwrap_or_default();
    let answer = keys(last);
    OFFERS.iter().any(|offer| {
        let offer = keys(offer);
        answer.windows(offer.len()).any(|window| window == offer)
    })
}

/// Which of the `heard` words are the lamp's own: those inside a run of at
/// least [`STRIP_RUN`] heard words said one after another, in that order,
/// in `answer`. Words of the answer heard in another order are not.
fn own_in_order(heard: &[&str], answer: &[String]) -> Vec<bool> {
    let heard: Vec<String> = heard.iter().map(|word| super::word_key(word)).collect();
    let mut own = vec![false; heard.len()];
    for i in 0..heard.len() {
        let longest = (0..answer.len())
            .map(|at| {
                heard[i..]
                    .iter()
                    .zip(&answer[at..])
                    .take_while(|(word, said)| !word.is_empty() && word == said)
                    .count()
            })
            .max()
            .unwrap_or(0);
        if longest >= STRIP_RUN {
            own[i..i + longest].fill(true);
        }
    }
    own
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
    let answer = keys(said.words);
    let own = own_in_order(&words, &answer);
    let m = own.iter().filter(|own| **own).count();
    // Held: nothing the lamp did not say. Cut: one own run at an end.
    let echo = m == n && n >= HELD_LEAST;
    let leading = own.iter().take_while(|own| **own).count();
    let trailing = own.iter().rev().take_while(|own| **own).count();
    let cut = if m == 0 || m == n {
        None
    } else if leading == m {
        Some((&words[m..], "start"))
    } else if trailing == m {
        Some((&words[..n - m], "end"))
    } else {
        None
    };
    if !echo && cut.is_none() {
        return Echo::Answered(None);
    }
    let within = line.over_the_lamps_voice
        || line.began_us != 0
            && said.until_us != 0
            && line.began_us <= said.until_us.saturating_add(TAIL_US);
    let guard = if !within {
        Some("it began after the lamp's words and their 1.5 s tail")
    } else if line.called || crate::attention::called_by_name_on(line.heard, true) {
        Some("it has the lamp's name")
    } else if (m < n || !line.over_the_lamps_voice) && crate::remarks::questions(line.heard) {
        // A question that adds a word, or one begun after the lamp's voice
        // even if it only repeats it ("Did you say five o'clock?"), is the
        // person's; over its voice, a question of only its words may be
        // its own question heard back.
        Some("it is a question of its own")
    } else if !line.over_the_lamps_voice && said.words.trim_end().ends_with('?') {
        Some("it began after the lamp's question")
    } else if (!line.over_the_lamps_voice || m < n) && offers(said.words) {
        Some("it answers the lamp's offer")
    } else {
        None
    };
    let of = said.request;
    if let Some(why) = guard {
        return Echo::Answered(Some(format!(
            "lamp-voice second: request {request}: of its {n} words, {m} in the lamp's answer to request {of}, but {why}: answered as the person's (LAMP_SECOND_OWN_ECHO)"
        )));
    }
    let when = if line.over_the_lamps_voice {
        "over the lamp's voice"
    } else {
        "within 1.5 s of the lamp's words"
    };
    match cut {
        None => Echo::Held(format!(
            "lamp-voice second: request {request}: its answer is held back: the lamp's own echo: of its {n} words, {m} in the lamp's answer to request {of}, begun {when} (LAMP_SECOND_OWN_ECHO)"
        )),
        Some((kept, at)) => Echo::Stripped(
            kept.join(" "),
            format!(
                "lamp-voice second: request {request}: {m} of its {n} words, the lamp's own answer to request {of} at its {at}, are taken out of what the model is sent (LAMP_SECOND_OWN_ECHO)"
            ),
        ),
    }
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

    /// The journal's count matches words as a set; this judgement counts
    /// only runs in the answer's own order, so its count is never more than
    /// the journal's: equal for a chunk heard in order, less for the same
    /// words out of order.
    #[test]
    fn its_counts_are_at_most_the_journals() {
        let journal =
            |heard: &str| super::super::whose_words(5, heard, None, Some((3, ANSWER.to_owned())));
        let heard = "the meeting starts at six";
        let Echo::Held(held) = judge(&line(heard, 9_000_000, true), Some(&said())) else {
            panic!("held")
        };
        assert_eq!(
            journal(heard).as_deref(),
            Some(
                "lamp-voice second: request 5: of its 5 words, 5 in the lamp's answer to request 3 so far"
            )
        );
        assert!(held.contains("of its 5 words, 5 in the lamp's answer to request 3"));
        // The same words out of order: 5 by the journal's set, none in order.
        let shuffled = "six at starts meeting the";
        assert_eq!(
            journal(shuffled).as_deref(),
            Some(
                "lamp-voice second: request 5: of its 5 words, 5 in the lamp's answer to request 3 so far"
            )
        );
        assert_eq!(
            judge(&line(shuffled, 9_000_000, true), Some(&said())),
            Echo::Answered(None)
        );
    }

    #[test]
    fn a_true_echo_of_its_words_is_held() {
        // A chunk of the answer heard over its voice.
        assert_eq!(
            judge(
                &line("and the meeting starts at six", 9_000_000, true),
                Some(&said())
            ),
            Echo::Held(
                "lamp-voice second: request 5: its answer is held back: the lamp's own echo: of its 6 words, 6 in the lamp's answer to request 3, begun over the lamp's voice (LAMP_SECOND_OWN_ECHO)".to_owned()
            )
        );
        // Two runs in order, a word dropped between, begun 1 s after the
        // lamp's words would have ended.
        assert_eq!(
            judge(
                &line("in Hanoi the meeting starts", UNTIL_US + 1_000_000, false),
                Some(&said())
            ),
            Echo::Held(
                "lamp-voice second: request 5: its answer is held back: the lamp's own echo: of its 5 words, 5 in the lamp's answer to request 3, begun within 1.5 s of the lamp's words (LAMP_SECOND_OWN_ECHO)".to_owned()
            )
        );
    }

    #[test]
    fn its_words_out_of_order_are_not_its_own() {
        // Every word is in the answer, none two in its order.
        for heard in ["six at starts meeting the", "meeting six Hanoi the at"] {
            assert_eq!(
                judge(&line(heard, 9_000_000, true), Some(&said())),
                Echo::Answered(None),
                "{heard}"
            );
        }
    }

    #[test]
    fn a_line_adding_words_the_lamp_did_not_say_is_never_held() {
        // 4 of 6 words the lamp's, in order: not held, its run taken out.
        assert_eq!(
            judge(
                &line("and the meeting starts uh huh", 9_000_000, true),
                Some(&said())
            ),
            Echo::Stripped(
                "uh huh".to_owned(),
                "lamp-voice second: request 5: 4 of its 6 words, the lamp's own answer to request 3 at its start, are taken out of what the model is sent (LAMP_SECOND_OWN_ECHO)".to_owned()
            )
        );
        // A partial echo of an answer that is no offer: the new words kept.
        let set = Said {
            words: "Your reminder is set for six.",
            ..said()
        };
        assert!(matches!(
            judge(&line("reminder is set for my dentist too", UNTIL_US + 500_000, false), Some(&set)),
            Echo::Stripped(kept, _) if kept == "my dentist too"
        ));
    }

    #[test]
    fn a_line_taking_up_the_lamps_offer_is_the_persons() {
        // Every word the lamp's, within the tail: answered.
        let offer = Said {
            words: "I can set a reminder for six.",
            ..said()
        };
        assert_eq!(
            judge(
                &line("set a reminder for six", UNTIL_US + 500_000, false),
                Some(&offer)
            ),
            Echo::Answered(Some(
                "lamp-voice second: request 5: of its 5 words, 5 in the lamp's answer to request 3, but it answers the lamp's offer: answered as the person's (LAMP_SECOND_OWN_ECHO)".to_owned()
            ))
        );
        // A partial echo plus new words: sent unchanged, after its words or
        // over its voice.
        let offer = Said {
            words: "I can set a reminder.",
            ..said()
        };
        for (began_us, over) in [(UNTIL_US + 500_000, false), (9_000_000, true)] {
            assert!(matches!(
                judge(&line("set a reminder for my dentist too", began_us, over), Some(&offer)),
                Echo::Answered(Some(journal)) if journal.contains("it answers the lamp's offer")
            ));
        }
        // Nothing but the offer's own words over its voice is still its echo.
        assert!(matches!(
            judge(&line("set a reminder", 9_000_000, true), Some(&offer)),
            Echo::Held(_)
        ));
        // "I can't" is no offer.
        assert!(!offers("I can't set a reminder."));
        assert!(offers("Would you like a reminder?"));
    }

    #[test]
    fn an_offer_counts_only_in_the_answers_last_sentence() {
        // The Reviewer-lead's P2 on #3034: "I can" or a sign-off early in an
        // answer is no offer, so its echo in the tail is still held.
        let answer = Said {
            words: "I can hear you fine. The meeting starts at six today.",
            ..said()
        };
        assert!(matches!(
            judge(
                &line("the meeting starts at six today", UNTIL_US + 300_000, false),
                Some(&answer)
            ),
            Echo::Held(_)
        ));
        let signed_off = Said {
            words: "The meeting starts at six. Let me know if you need anything.",
            ..said()
        };
        assert!(matches!(
            judge(
                &line("the meeting starts at six", UNTIL_US + 300_000, false),
                Some(&signed_off)
            ),
            Echo::Held(_)
        ));
        // An offer the lamp ends on is still one.
        assert!(offers("It's five. I can set a reminder for six."));
        assert!(!offers("I can hear you. It's five."));
    }

    #[test]
    fn a_two_word_full_repeat_is_answered_as_heard_never_empty() {
        // El's P2 on #3019: all the lamp's words but under HELD_LEAST, over
        // its voice and in the tail, goes as heard, nothing taken out.
        let five = Said {
            words: "It's five o'clock.",
            ..said()
        };
        for (began_us, over) in [(9_000_000, true), (UNTIL_US + 1_000_000, false)] {
            assert_eq!(
                judge(&line("five o'clock", began_us, over), Some(&five)),
                Echo::Answered(None)
            );
        }
    }

    #[test]
    fn a_repeat_back_question_in_the_tail_is_the_persons() {
        let at = Said {
            words: "The meeting starts at six today.",
            ..said()
        };
        assert!(matches!(
            judge(&line("starts at six today?", UNTIL_US + 1_000_000, false), Some(&at)),
            Echo::Answered(Some(journal)) if journal.contains("a question of its own")
        ));
    }

    #[test]
    fn a_short_line_with_two_of_its_words_is_answered() {
        // Two common words of the answer in three heard over its voice: as
        // likely the person barging in, so not held.
        assert_eq!(
            judge(&line("it is raining", 5_000_000, true), Some(&said())),
            Echo::Answered(None)
        );
        // Only two words, both its own: too short to hold, nothing left to cut.
        assert_eq!(
            judge(&line("at six", 9_000_000, true), Some(&said())),
            Echo::Answered(None)
        );
    }

    #[test]
    fn the_persons_line_with_a_few_of_its_words_is_answered_as_heard() {
        // "the" and "at" are the lamp's too, in its middle, one alone.
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
            judge(
                &line("shall I start the timer", 9_000_000, true),
                Some(&asked)
            ),
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
            "at six set an alarm in Hanoi",
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
