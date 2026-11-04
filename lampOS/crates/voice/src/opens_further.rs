//! After an answer the lamp sometimes opens the talk a little further, as a
//! companion does (#5194), on the second route, off unless
//! `LAMP_OPENS_FURTHER=on`.
//!
//! The lead's internal testers, 4 November: "whatever you ask the lamp, it
//! only answers that; it never opens the conversation further." A friend
//! asked what the weather is may add "Going out later?"; asked for a
//! recipe, "Want me to set a timer for the pasta?". The lamp never did.
//!
//! Why it never did: the second route's character
//! (`deploy/character/lamp.txt`) says "Do not end replies by offering
//! more: no \"let me know if…\", no \"anything else?\", no \"I'm here if
//! you need me\". When you have said the thing, stop." and asks a question
//! back only when curious, as V1's Gemini realtime prompt forbade "extra
//! offers/follow-up questions". V1's own persona held the rule wanted
//! here (`robots/lamp/SOUL.md`, "Leave a soft door, not a questionnaire",
//! the PM on #5194): after a real exchange where a feeling sat under the
//! words, a small noticing, a quiet offer, or a thread back to what they
//! said; never interview questions; never for commands, sensing or
//! ambient turns, nor when the answer already fills its two sentences;
//! "Want me to do that?" or "Tell me more!" tacked onto a complete answer
//! is what it forbids.
//!
//! With the switch, a spoken line the gates below let through is sent with
//! [`NOTE`] beside it: that rule, which eases the character's "stop" for
//! that answer only. The character file is unchanged, and without the
//! switch nothing is told. The door is the last sentence of the same
//! reply: there is no second request, and the answer's first sentence
//! streams as before, so it adds nothing to the time to the first sound.
//! Written turns (sensing, news, reminders) are never told it.
//!
//! The gates are in code, so a test pins them ([`note`]): no note, so no
//! opening, for a command ("set a timer", "lights off", [`command`]), a
//! stop, a line in a busy room (a short factual check there included), a
//! person leaving, busy or on a call ([`closing`]), a conversation another
//! known person has spoken in, a line let out only as the conversation
//! going on, a line the model is told something else first (it cut in,
//! the lamp's name alone, a misheard line, a task waiting for a yes, a
//! stop), and right after an answer that ended with an opening of its own:
//! at most one, then it waits. A kind already used in the conversation (a
//! question back, an offer) is told as used, and after two openings there
//! is no third. Without the switch nothing is told and nothing changes.

/// Where the second route reads the switch: `on` exactly.
pub const ENVIRONMENT: &str = "LAMP_OPENS_FURTHER";

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// What the model is told beside a line the gates let through: V1's soft
/// door (`robots/lamp/SOUL.md`), in this route's words, easing the
/// character's "when you have said the thing, stop" for this answer only.
pub const NOTE: &str = "(For you to know: for this answer you may leave a soft door, not a questionnaire. If a feeling, or something of their own day or plans, sits under their words, you may end with one short line: a small noticing (\"that sounds like a lot\"), a quiet offer of something you can do yourself right now, or a gentle thread back to what they said. That is allowed here even though you usually stop when you have said the thing. Never an interview question (\"how was your day?\"), never \"want me to do that?\" or \"tell me more!\" tacked onto a complete answer, never an offer of something you cannot do, and none for a plain fact, after you did something they asked, or when your answer already fills its two sentences.)";

/// Told after [`NOTE`] when the conversation already had a question back.
pub const ASKED_ALREADY: &str =
    " (You already asked them a question back in this conversation: not another.)";

/// Told after [`NOTE`] when the conversation already had an offer.
pub const OFFERED_ALREADY: &str =
    " (You already offered them something in this conversation: not another offer.)";

/// The most openings in one conversation: after these, none.
pub const MOST: usize = 2;

/// The kind of opening an answer of the lamp's ended with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A question back ("Going out later?").
    Question,
    /// An offer ("Want me to play something calm?").
    Offer,
}

/// How an offer's last sentence opens.
const OFFER_STARTS: &[&[&str]] = &[
    &["want", "me"],
    &["shall", "i"],
    &["should", "i"],
    &["would", "you", "like", "me"],
    &["do", "you", "want", "me"],
    &["i", "can"],
    &["i", "could"],
    &["how", "about", "i"],
    &["let", "me", "know"],
    &["i'm", "here"],
    &["i", "am", "here"],
];

/// The kind of opening the lamp's words `said` end with, by their last
/// sentence: an offer (a question or not: "I'm here if there's more."), a
/// question back ([`crate::remarks::lamp_asked`]), or none. A remark is not told apart from an answer here: the model sees
/// its own earlier lines.
pub fn kind_of(said: &str) -> Option<Kind> {
    let said = said.trim_end();
    let body = said.trim_end_matches(['?', '.', '!', ' ']);
    let last = body.rsplit(['.', '!', '?']).next().unwrap_or(body);
    let words = words(last);
    let offer = OFFER_STARTS.iter().any(|start| {
        words.len() >= start.len()
            && words
                .iter()
                .zip(start.iter())
                .all(|(word, want)| word == want)
    });
    if offer {
        Some(Kind::Offer)
    } else if crate::remarks::lamp_asked(said) {
        Some(Kind::Question)
    } else {
        None
    }
}

/// Words before a request's own words: a greeting, the lamp's name, a
/// polite opening ("Hey lamp, could you please set…").
const LEADING: &[&str] = &[
    "hey", "hi", "hello", "ok", "okay", "so", "and", "now", "um", "uh", "lamp", "lamb", "please",
    "can", "could", "would", "will", "you", "just", "go", "ahead", "then",
];

/// Words that begin a command: something the lamp does, not a talk.
const DOING: &[&str] = &[
    "set", "start", "stop", "cancel", "turn", "switch", "play", "pause", "resume", "skip", "dim",
    "brighten", "mute", "unmute", "remind", "delete", "remove", "clear", "forget", "remember",
    "note", "save", "raise", "lower", "tilt", "point", "aim", "move", "face", "wake", "shut",
    "louder", "quieter", "softer", "brighter", "dimmer", "next", "previous", "repeat", "volume",
    "lights", "light", "snooze", "dismiss", "silence",
];

/// Doing words that also begin a talk ("Remember when…", "Play is good
/// for kids", "Lights look nice tonight"), and the words after them that
/// make them one: then they are no command.
const ALSO_TALK: &[&str] = &["remember", "play", "light", "lights"];
const TALK_AFTER: &[&str] = &[
    "when", "is", "was", "are", "were", "look", "looks", "looked", "seem", "seems", "feel", "feels",
];

/// Things a command names wherever they come in a short line.
const DOING_THINGS: &[&str] = &["timer", "alarm", "reminder", "reminders", "volume"];

/// The longest line a [`DOING_THINGS`] word makes a command.
const SHORT: usize = 10;

/// Whether `heard` is a command: past a greeting, the name and a polite
/// opening ([`LEADING`]) it begins with a doing word ([`DOING`]: "Set a
/// timer…", "Lamp, lights off.", "Could you turn it down?"), or a short
/// line names a timer, alarm, reminder or the volume ("A timer for five
/// minutes."), or the light with on, off, up or down ("Light on please.").
pub fn command(heard: &str) -> bool {
    let words = words(heard);
    let at = words
        .iter()
        .position(|word| !LEADING.contains(&word.as_str()));
    if let Some(at) = at
        && DOING.contains(&words[at].as_str())
    {
        let talk = ALSO_TALK.contains(&words[at].as_str())
            && words
                .get(at + 1)
                .is_some_and(|next| TALK_AFTER.contains(&next.as_str()));
        if !talk {
            return true;
        }
    }
    let short = words.len() <= SHORT;
    let names = words
        .iter()
        .any(|word| DOING_THINGS.contains(&word.as_str()));
    let light = words
        .iter()
        .any(|word| matches!(word.as_str(), "light" | "lights"))
        && words
            .iter()
            .any(|word| matches!(word.as_str(), "on" | "off" | "up" | "down"));
    short && (names || light)
}

/// Words that close the talk wherever they come, matched as whole words
/// in order.
const CLOSING: &[&str] = &[
    "bye",
    "goodbye",
    "good night",
    "goodnight",
    "gotta go",
    "got to go",
    "have to go",
    "need to go",
    "gotta head out",
    "got to head out",
    "have to head out",
    "need to head out",
    "i'm off",
    "see you",
    "see ya",
    "talk later",
    "talk to you later",
    "later then",
    "call you back",
    "one sec",
    "one second",
    "hang on",
    "not now",
    "that's all",
    "that's it",
    "no thanks",
    "never mind",
];

/// Words that say the person is busy, going or on a call, but only of the
/// speaker themselves ([`SPEAKER`]: "I'm busy", "I'm in a meeting") or as
/// the whole short line ("Busy.", "In a meeting now."): "I've been so busy
/// at work", "a meeting tomorrow", "my friend is leaving" and "a call from
/// my boss" are feelings to open, not a close (#5235).
const BUSY: &[&str] = &[
    "busy",
    "in a meeting",
    "on a call",
    "on the phone",
    "leaving",
    "heading out",
    "about to leave",
];

/// The speaker as the subject just before a [`BUSY`] phrase.
const SPEAKER: &[&str] = &["i'm", "i am", "im"];

/// Words that may sit between [`SPEAKER`] and a [`BUSY`] phrase ("I'm
/// really busy").
const BETWEEN: &[&str] = &["really", "just", "pretty", "quite", "kinda", "a bit"];

/// Words that may follow a [`BUSY`] phrase that is the whole line ("In a
/// meeting right now.").
const AFTER: &[&str] = &["now", "right now", "sorry"];

/// The longest line that is only a thanks or an okay: a close, not a talk.
const ACKNOWLEDGES: usize = 3;

/// Words a short close is made of ("Okay, thanks.", "Cool, thank you.").
const THANKS: &[&str] = &[
    "ok", "okay", "thanks", "thank", "you", "cool", "great", "nice", "alright", "right", "got",
    "it", "sure", "lamp", "fine", "good", "perfect", "awesome", "yes", "yeah", "no",
];

/// Whether `heard` says the person is leaving, busy or on a call, or only
/// closes the talk: a [`CLOSING`] phrase anywhere ("I'm off to a meeting,
/// what's the time?"), a [`BUSY`] phrase said of the speaker ("I'm on a
/// call, what's the weather?") or as the whole line ("Busy."), or a line
/// of at most three words of thanks and okays ("Okay, thanks."), counted
/// past the greetings and the lamp's name that open it and the name that
/// ends it ([`acknowledged`]): "Hey Lamp, okay, thanks." and "Okay thank
/// you lamp" close as "Okay, thanks." does.
pub fn closing(heard: &str) -> bool {
    let words = words(heard);
    let says = |phrase: &str| (0..words.len()).any(|at| starts(&words[at..], phrase).is_some());
    let line = acknowledged(&words);
    CLOSING.iter().any(|phrase| says(phrase))
        || BUSY
            .iter()
            .any(|phrase| of_the_speaker(&words, phrase) || whole_line(line, phrase))
        || {
            !line.is_empty()
                && line.len() <= ACKNOWLEDGES
                && line.iter().all(|word| THANKS.contains(&word.as_str()))
        }
}

/// The words past `phrase` if `words` begin with it, else none.
fn starts<'w>(words: &'w [String], phrase: &str) -> Option<&'w [String]> {
    let phrase: Vec<&str> = phrase.split(' ').collect();
    (words.len() >= phrase.len()
        && words
            .iter()
            .zip(phrase.iter())
            .all(|(word, want)| word == want))
    .then(|| &words[phrase.len()..])
}

/// Whether `words` hold [`SPEAKER`], at most one [`BETWEEN`], then `phrase`.
fn of_the_speaker(words: &[String], phrase: &str) -> bool {
    (0..words.len()).any(|at| {
        SPEAKER
            .iter()
            .filter_map(|speaker| starts(&words[at..], speaker))
            .any(|rest| {
                starts(rest, phrase).is_some()
                    || BETWEEN
                        .iter()
                        .filter_map(|between| starts(rest, between))
                        .any(|rest| starts(rest, phrase).is_some())
            })
    })
}

/// Whether `line` is `phrase` alone, or with one [`AFTER`] ("Busy, sorry.").
fn whole_line(line: &[String], phrase: &str) -> bool {
    starts(line, phrase).is_some_and(|rest| {
        rest.is_empty()
            || AFTER
                .iter()
                .any(|after| starts(rest, after).is_some_and(<[String]>::is_empty))
    })
}

/// `words` past the greetings and the lamp's name, or a word written for
/// it, that open them ("Hey Lamp, okay, thanks."), and the name that ends
/// them ("Thank you, lamp."), as a call reads them
/// ([`crate::attention::is_greeting`],
/// [`crate::attention::written_for_the_name`]).
fn acknowledged(words: &[String]) -> &[String] {
    let opening = |word: &String| {
        crate::attention::is_greeting(word) || crate::attention::written_for_the_name(word)
    };
    let from = words
        .iter()
        .position(|word| !opening(word))
        .unwrap_or(words.len());
    let words = &words[from..];
    let to = words
        .iter()
        .rposition(|word| !crate::attention::written_for_the_name(word))
        .map_or(0, |at| at + 1);
    &words[..to]
}

/// `heard`'s plain words, a curly apostrophe read as a straight one
/// ("I’m off").
fn words(heard: &str) -> Vec<String> {
    crate::not_speech::plain_words(&heard.replace('\u{2019}', "'"))
}

/// What the gates read of a spoken line about to be asked.
#[derive(Debug, Default)]
pub struct Moment<'a> {
    /// The line's words as heard.
    pub heard: &'a str,
    /// Judged in a busy room, by the rule or the room as it is now.
    pub busy: bool,
    /// Its words are a stop, or it is the first line after a hand's stop.
    pub stopped: bool,
    /// Let out only as the conversation going on: it may be people's talk.
    pub alone: bool,
    /// Told something else first: it cut in, the name alone, misheard, a
    /// task waiting for a yes, a search pushed away.
    pub told_else: bool,
    /// The lamp's words in the conversation so far, oldest first
    /// ([`crate::thread::Thread::conversation_so_far`]).
    pub lamp_said: &'a [String],
    /// Known people who spoke in the conversation so far.
    pub people: usize,
}

/// The note for `moment`, or why there is none (for the journal).
pub fn note(moment: &Moment) -> Result<String, &'static str> {
    if moment.busy {
        return Err("a busy room");
    }
    if moment.stopped {
        return Err("a stop");
    }
    if moment.alone {
        return Err("let out only as the conversation going on");
    }
    if moment.told_else {
        return Err("told something else first");
    }
    if moment.people > 1 {
        return Err("another person is in the conversation");
    }
    if command(moment.heard) {
        return Err("a command");
    }
    if closing(moment.heard) {
        return Err("leaving, busy, on a call or closing");
    }
    let kinds: Vec<Kind> = moment
        .lamp_said
        .iter()
        .filter_map(|said| kind_of(said))
        .collect();
    if moment
        .lamp_said
        .last()
        .is_some_and(|said| kind_of(said).is_some())
    {
        return Err("its last answer ended with an opening: it waits");
    }
    if kinds.len() >= MOST {
        return Err("two openings in this conversation already");
    }
    let mut note = NOTE.to_owned();
    if kinds.contains(&Kind::Question) {
        note.push_str(ASKED_ALREADY);
    }
    if kinds.contains(&Kind::Offer) {
        note.push_str(OFFERED_ALREADY);
    }
    Ok(note)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet(heard: &str) -> Moment<'_> {
        Moment {
            heard,
            people: 1,
            ..Moment::default()
        }
    }

    /// The issue's everyday questions get the note in a quiet room.
    #[test]
    fn an_everyday_question_in_a_quiet_room_may_open_further() {
        for heard in [
            "Hey lamp, what's the weather like today?",
            "What should I cook for dinner?",
            "I'm so tired today.",
            "Tell me something interesting about octopuses.",
            "How do I stay focused in the afternoon?",
            "Lamp, what time is it?",
        ] {
            assert_eq!(note(&quiet(heard)).as_deref(), Ok(NOTE), "{heard}");
        }
    }

    /// The issue's tests: a command, a stop and a busy room get no
    /// follow-up.
    #[test]
    fn a_command_a_stop_and_a_busy_room_get_no_opening() {
        for heard in [
            "Set a timer for ten minutes.",
            "Lamp, lights off.",
            "Hey lamp, could you turn the light down?",
            "Turn it up a bit.",
            "Play some jazz.",
            "Remind me to call mum at six.",
            "A timer for five minutes please.",
            "Light on.",
            "Volume down.",
            "Remember that my sister is called Mai.",
        ] {
            assert!(command(heard), "{heard}");
            assert_eq!(note(&quiet(heard)), Err("a command"), "{heard}");
        }
        let stop = Moment {
            stopped: true,
            ..quiet("Stop.")
        };
        assert_eq!(note(&stop), Err("a stop"));
        let busy = Moment {
            busy: true,
            ..quiet("What time is it?")
        };
        assert_eq!(note(&busy), Err("a busy room"));
    }

    /// Questions are no commands, even with a doing word later in them.
    #[test]
    fn a_question_is_no_command() {
        for heard in [
            "What's a good way to start the day?",
            "Can you tell me a joke?",
            "Why do cats like boxes?",
            "How long should I boil an egg?",
            "Is it going to rain tomorrow?",
        ] {
            assert!(!command(heard), "{heard}");
        }
    }

    /// Twelve's P3 on #5235: remember, play and light begin talk too.
    #[test]
    fn talk_beginning_with_a_doing_word_is_no_command() {
        for heard in [
            "Remember when we went to the beach?",
            "Play is so important for kids.",
            "Lights look nice tonight.",
        ] {
            assert!(!command(heard), "{heard}");
        }
        for heard in [
            "Remember my keys are in the drawer.",
            "Play it again.",
            "Lights off.",
        ] {
            assert!(command(heard), "{heard}");
        }
    }

    /// Leaving, busy, on a call, or only closing: no opening.
    #[test]
    fn leaving_busy_on_a_call_or_closing_get_no_opening() {
        for heard in [
            "Okay, thanks.",
            "Thank you.",
            "Bye lamp.",
            "I'm off to a meeting, what's the time?",
            "I'm on a call, what's the weather?",
            "I have to go now.",
            "I\u{2019}m off now, what's the time?",
            "I'm busy, just tell me the date.",
            "Good night.",
            "Never mind.",
        ] {
            assert!(closing(heard), "{heard}");
            assert!(note(&quiet(heard)).is_err(), "{heard}");
        }
        assert!(!closing("Thanks, and what should I read next?"));
        assert!(!command("What's going on, lamp?"));
    }

    /// Busy, a meeting, leaving or a call said of the speaker, or as the
    /// whole short line, closes; the same words in a feeling line about
    /// work, tomorrow or someone else leave the door (#5235).
    #[test]
    fn busy_or_leaving_closes_only_of_the_speaker_or_as_the_whole_line() {
        for heard in [
            "I'm busy right now.",
            "I'm in a meeting.",
            "Gotta go.",
            "Busy.",
            "In a meeting.",
            "I am really busy.",
            "I'm heading out, what's the weather?",
            "I need to head out soon.",
            "Hey lamp, on a call, sorry.",
        ] {
            assert!(closing(heard), "{heard}");
            assert_eq!(
                note(&quiet(heard)),
                Err("leaving, busy, on a call or closing"),
                "{heard}"
            );
        }
        for heard in [
            "I've been so busy at work, I'm exhausted.",
            "I have a meeting tomorrow and I'm nervous.",
            "My friend is leaving town and I'm sad.",
            "I got a call from my boss",
        ] {
            assert!(!closing(heard), "{heard}");
            assert_eq!(note(&quiet(heard)).as_deref(), Ok(NOTE), "{heard}");
        }
    }

    /// "Keep me company" is a talk, not a command (#5235).
    #[test]
    fn keep_me_company_is_no_command() {
        let heard = "Can you keep me company for a bit?";
        assert!(!command(heard));
        assert_eq!(note(&quiet(heard)).as_deref(), Ok(NOTE));
    }

    /// A greeting and the lamp's name, or a word written for it, are no
    /// words of the close: "Hey Lamp, okay, thanks." closes as "Okay,
    /// thanks." does. A thanks with a question after it still asks.
    #[test]
    fn the_name_and_a_greeting_do_not_lengthen_a_close() {
        for heard in [
            "Hey Lamp, okay, thanks.",
            "Lamp, thanks!",
            "okay thank you lamp",
            "Hey Lynn, okay, thank you.",
        ] {
            assert!(closing(heard), "{heard}");
            assert_eq!(
                note(&quiet(heard)),
                Err("leaving, busy, on a call or closing"),
                "{heard}"
            );
        }
        let asks = "Hey Lamp, thanks, what's the weather tomorrow?";
        assert!(!closing(asks));
        assert_eq!(note(&quiet(asks)).as_deref(), Ok(NOTE));
        assert!(!closing("Hey Lamp."));
    }

    /// Another person in the conversation, a line let out only as it going
    /// on, and a line told something else first: no opening.
    #[test]
    fn another_person_or_a_line_told_else_gets_no_opening() {
        let two = Moment {
            people: 2,
            ..quiet("What's the weather?")
        };
        assert_eq!(note(&two), Err("another person is in the conversation"));
        let alone = Moment {
            alone: true,
            ..quiet("What's the weather?")
        };
        assert!(note(&alone).is_err());
        let told = Moment {
            told_else: true,
            ..quiet("What's the weather?")
        };
        assert!(note(&told).is_err());
    }

    /// At most one opening, then it waits; a kind used is told as used; no
    /// third opening in a conversation.
    #[test]
    fn one_opening_then_it_waits_and_no_kind_twice() {
        let asked = vec!["It's 24 degrees and sunny. Going out later?".to_owned()];
        let after = Moment {
            lamp_said: &asked,
            ..quiet("Maybe, why?")
        };
        assert_eq!(
            note(&after),
            Err("its last answer ended with an opening: it waits")
        );
        let then = vec![
            "It's 24 degrees and sunny. Going out later?".to_owned(),
            "Then take a hat, it's bright out.".to_owned(),
        ];
        let later = Moment {
            lamp_said: &then,
            ..quiet("What should I wear?")
        };
        assert_eq!(note(&later), Ok(format!("{NOTE}{ASKED_ALREADY}")));
        let both = vec![
            "Pasta takes ten minutes. Want me to set a timer?".to_owned(),
            "Sure.".to_owned(),
            "It's sunny. Going out later?".to_owned(),
            "Nice.".to_owned(),
        ];
        let third = Moment {
            lamp_said: &both,
            ..quiet("What should I read tonight?")
        };
        assert_eq!(
            note(&third),
            Err("two openings in this conversation already")
        );
    }

    /// The note eases the character's own "stop" rule, which is why the
    /// lamp never left a door (#5194): if the character's words change,
    /// the note must be read again.
    #[test]
    fn the_note_eases_the_characters_stop_rule() {
        let character = include_str!("../../../deploy/character/lamp.txt");
        assert!(character.contains("When you have said the thing, stop."));
        assert!(NOTE.contains("even though you usually stop when you have said the thing"));
        assert!(NOTE.contains("a soft door, not a questionnaire"));
    }

    /// The kind of an answer's ending, by its last sentence.
    #[test]
    fn an_answer_ends_with_a_question_an_offer_or_neither() {
        assert_eq!(
            kind_of("It's sunny. Going out later?"),
            Some(Kind::Question)
        );
        assert_eq!(
            kind_of("Ten minutes. Want me to set a timer?"),
            Some(Kind::Offer)
        );
        assert_eq!(
            kind_of("That's lovely. Shall I play something calm?"),
            Some(Kind::Offer)
        );
        assert_eq!(
            kind_of("That sounds like a lot. I\u{2019}m here if there's more."),
            Some(Kind::Offer)
        );
        assert_eq!(kind_of("It's 3 pm."), None);
        assert_eq!(kind_of(""), None);
    }
}
