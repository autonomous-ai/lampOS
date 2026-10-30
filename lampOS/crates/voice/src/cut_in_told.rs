//! A spoken cut-in, told to the model (#828, the first builder at 03:15 on
//! 19 October), off unless `LAMP_CUT_IN_TOLD=on`.
//!
//! A touch or the stop button that stops the lamp tells the model that
//! what it was saying is over (`LAMP_STOP_TOLD`,
//! `crate::provider_worker::STOPPED_NOTE`). A person who cuts in by speaking
//! told it nothing. On lamp-4ace on 19 October at 02:59, "Okay, okay, that
//! is enough." was heard right, committed over the lamp's story, and the
//! answer was the story again from where it was cut: with the story cut
//! mid-sentence in its context, "Okay, okay" read as "go on".
//!
//! With the switch, the request that commits a spoken cut-in over the
//! lamp's own audible answer (`ProviderInput::Start`'s
//! `over_the_lamps_voice`, and not the first after a stop, which is told
//! that instead) is told [`NOTE`] beside what is heard, in the form of the
//! lamp's other notes. A request sends only its newest note, so it goes
//! with the others due on it, the name heard among them (`add_note`).
//! The Live route only.
//!
//! What the note asks (#3997's cut-in proposal, Friday's person's call on
//! 30 October, agreed with Sixth): an interruption is made plain, as a
//! person would make it. A line that only asks the lamp to stop or wait is
//! answered with a short "Okay, I'll stop." and nothing more; any other
//! cut-in is answered after a few words that show it was heard ("Oh, sure
//! —"), never a long apology. Asked later to go on, the lamp says in a few
//! words where it was and goes on from where it stopped, not from the
//! start.
//!
//! On lamp-06c1 on 30 October the resume came both times, but "Oh, sure —"
//! only once: the other time the model just answered. The character tells
//! it not to use "repetitive acknowledgments" (`config::CHAT_INSTRUCTION`),
//! and the acknowledgement was the note's third sentence, among other notes
//! told after it. So the note now opens with it, says that rule does not
//! hold for this reply, and gives one example, and the second route tells
//! it last among its notes, right before the person's words
//! (`second_provider::spoken`). A plain "stop" or "enough" over the second
//! route's answer never reaches the model with `LAMP_SPOKEN_STOP_ENDS`
//! (`crate::spoken_stop`); "wait", "pause" and "hold on" still do, so the
//! note keeps the stop.
//!
//! The second route keeps only the played words of a cut answer,
//! with `said_as_heard::CUT` after a piece cut while it played, when
//! `LAMP_SAID_AS_HEARD` is on; with this switch such an answer is followed
//! in the history by [`CUT_HERE`] ([`cut_here`]), so the line that asks it to go on, told no
//! cut-in note of its own, knows where it stopped. With `LAMP_SAID_AS_HEARD`
//! off the thread keeps the whole answer and nothing marks the played part;
//! the Live route's history is Gemini's own, which keeps this note.

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_CUT_IN_TOLD";
/// The provider worker's flag for it, right after `--going-on-asks`.
pub const FLAG: &str = "--cut-in-told";

/// What the model is told beside a request that cut in over its answer.
pub const NOTE: &str = "(They cut in: they spoke while you were speaking, and you stopped there. This time your first words are a short, natural sign that you heard them, such as \"Oh, sure —\", and then the answer to what they said: for this reply the rule against acknowledgments does not hold. For example, cut in with \"What's two plus two?\", say \"Oh, sure — it's four.\" If all they said is to stop or wait, such as \"stop\", \"pause\", \"wait\" or \"hold on\", with nothing else asked, say only a short \"Okay, I'll stop.\" in the language of the conversation, and nothing more. Never apologise at length. Go back to what you were saying only if they ask you to; then first say in a few words where you were, and go on from where you stopped, not from the start.)";

/// Told in the history right after an answer of the lamp's that was cut
/// while it played and is kept as heard (`LAMP_SAID_AS_HEARD`), with this
/// switch.
pub const CUT_HERE: &str = "(Your answer just above was cut off where it ends: they never heard the rest. If they ask you to go on with it, first say in a few words where you were, such as \"Where was I… the keeper had just…\", then go on from there, not from the start.)";

/// Whether the second route's worker marks cut answers in the history
/// ([`cut_here`]): set once from `--cut-in-told` as the worker starts.
static MARKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The second route's worker with `--cut-in-told` marks cut answers.
pub fn marking_cut_answers(on: bool) {
    MARKING.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// [`CUT_HERE`] after an exchange's answer that was `cut` while it played
/// and is kept as heard (`crate::thread::Recent::cut`), when the worker
/// marks cut answers; `None` otherwise.
pub fn cut_here(cut: bool) -> Option<&'static str> {
    cut_here_when(MARKING.load(std::sync::atomic::Ordering::Relaxed), cut)
}

fn cut_here_when(on: bool, cut: bool) -> Option<&'static str> {
    (on && cut).then_some(CUT_HERE)
}

/// The worker's flags for the switch, from the coordinator's environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`FLAG`] off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_only_for_on_and_its_flag_is_taken_first() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        for off in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(arguments(off).is_empty(), "{off:?}");
        }
        let flags = [FLAG.to_owned(), "x".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }

    #[test]
    fn the_note_asks_a_short_stop_a_few_words_first_and_a_clear_resume() {
        for asked in [
            "\"Okay, I'll stop.\"",
            "and nothing more",
            "\"stop\", \"pause\", \"wait\" or \"hold on\"",
            "your first words are a short, natural sign that you heard them, such as \"Oh, sure —\"",
            "the rule against acknowledgments does not hold",
            "say \"Oh, sure — it's four.\"",
            "Never apologise at length.",
            "only if they ask you to",
            "say in a few words where you were",
            "from where you stopped, not from the start",
        ] {
            assert!(NOTE.contains(asked), "{asked}");
        }
        // Told to answer now: no longer "not to answer".
        assert!(!NOTE.contains("not to answer"));
    }

    #[test]
    fn only_a_cut_answer_is_marked_and_only_with_the_switch() {
        assert_eq!(cut_here_when(true, true), Some(CUT_HERE));
        assert_eq!(cut_here_when(false, true), None);
        assert_eq!(cut_here_when(true, false), None);
        assert!(CUT_HERE.contains("where you were") && CUT_HERE.contains("not from the start"));
    }
}
