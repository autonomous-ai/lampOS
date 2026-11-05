//! A cut-in that ended the lamp's answer with no words gets one short
//! question (the first builder on #4710, 2 Nov), on with
//! `LAMP_CUT_IN_WORDLESS_ASKS=on` (the product's, the memory owner's call
//! on 4 Nov).
//!
//! A cough, a beep or a knock over the lamp's answer can make it yield,
//! then commit as a cut-in: the answer is ended (`user_interrupted`) and
//! the line comes back with no words, so nothing is asked of the model and
//! the lamp went quiet mid-story, as if it had been told to. With the
//! switch, such a line is answered [`LINE`], a fixed line said in the
//! route's voice (in the owner's language when one is chosen), never the
//! model's, so the person who did speak can say it again.
//!
//! Only a cut-in that ended the answer: its `Start` came over the lamp's
//! audible answer (`over_the_lamps_voice`, sent only when the yield
//! committed). A yield that resumed sends no `Start`, so it stays silent; a
//! line in a busy room, or one with words, never gets it; a line that only
//! carries words cut short before it answers those instead. Asked at most
//! once in [`AGAIN_AFTER`]. The second route only.
//!
//! With `LAMP_CUT_IN_WORDLESS_RESUMES=on` too (the memory owner, 5 Nov:
//! "the once a minute is kinda strict"; off in the product since 6 Nov until
//! its busy-room path passes on a lamp, #5430), the lamp picks its answer up again,
//! as a person cut off by a cough does ([`Cuts::next`]):
//! - the first such cut in a quiet room still asks [`LINE`];
//! - the first in a busy room says [`BRIDGE`] and goes on from the sentence
//!   that was cut;
//! - another within [`AGAIN_AFTER`] of the last says [`AGAIN`], then goes on;
//! - a third within it stops there with [`PAUSE`], and any more within it
//!   leave the lamp quiet: it never loops;
//! - a line of the person's words, answered, starts the run again
//!   ([`Cuts::spoke`]), and a cut with nothing left to go on with (one of
//!   these lines cut, or the answer all played) is quiet and not counted.
//!
//! What it goes on with is the rest of the cut answer as it was sent
//! ([`crate::said_as_heard::unheard`]), from the start of the sentence that
//! was playing, the model not asked, when that answer was sent whole and
//! the cut said how much was played (`LAMP_SAID_AS_HEARD`). Otherwise the
//! model is asked to go on from the cut ([`resume_ask`]), its bridge said
//! first, so the first sound is never the model's wait.

/// The least time between two of its questions: a second knock that cuts
/// the question itself, or the next minute's, leaves the lamp quiet, never
/// asking again and again.
pub const AGAIN_AFTER: std::time::Duration = std::time::Duration::from_secs(60);
/// Where the second route's worker reads the switch.
pub const ENVIRONMENT: &str = "LAMP_CUT_IN_WORDLESS_ASKS";
/// What the lamp says when a cut-in ended its answer with no words: a
/// polite opener first, so the person hears that the lamp was cut off by a
/// sound, not that it simply stopped (the memory owner, 4 Nov: "'Did
/// someone say something?' is not enough"). "Um" is spelled as a word so
/// speech says it, and commas, never a dash, as the other rendered-ahead
/// lines have.
pub const LINE: &str = "Um, excuse me, did anyone say something?";

/// Where the second route's worker reads the resume switch.
pub const RESUMES_ENVIRONMENT: &str = "LAMP_CUT_IN_WORDLESS_RESUMES";
/// Said before the rest of an answer a first wordless cut ended in a busy
/// room: short, so the rest follows at once, as a person picks up again.
pub const BRIDGE: &str = "Anyway,";
/// Said before the rest when another wordless cut comes within
/// [`AGAIN_AFTER`] of the last (the memory owner, 5 Nov: "something like
/// 'sorry, i got interrupted again'"): light, no long sorry for the room.
pub const AGAIN: &str = "Oops, cut off again. Anyway,";
/// Said for a third wordless cut within [`AGAIN_AFTER`]: why it stops, and
/// how to have it go on (the memory owner, 5 Nov: "it must say WHY").
pub const PAUSE: &str =
    "It's a bit noisy here, so I'll pause. Just say 'Lamp' when you want me to go on.";

/// What a wordless cut that ended the answer is answered with
/// (`LAMP_CUT_IN_WORDLESS_RESUMES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// [`LINE`], in a quiet room, the first in [`AGAIN_AFTER`].
    Ask,
    /// [`BRIDGE`] and the rest, in a busy room, the first in [`AGAIN_AFTER`].
    Resume,
    /// [`AGAIN`] and the rest: the second within [`AGAIN_AFTER`] of the last.
    ResumeAgain,
    /// [`PAUSE`]: the third.
    Pause,
    /// Nothing: a fourth or more, so it never loops.
    Quiet,
}

impl Answer {
    /// The fixed line said first, and whether the rest follows it.
    pub fn line(self) -> Option<(&'static str, bool)> {
        match self {
            Self::Ask => Some((LINE, false)),
            Self::Resume => Some((BRIDGE, true)),
            Self::ResumeAgain => Some((AGAIN, true)),
            Self::Pause => Some((PAUSE, false)),
            Self::Quiet => None,
        }
    }
}

/// The wordless cuts that ended the lamp's answer, in a run: each within
/// [`AGAIN_AFTER`] of the one before it.
#[derive(Debug, Default)]
pub struct Cuts {
    last: Option<std::time::Instant>,
    run: u32,
}

impl Cuts {
    /// Another wordless cut at `now`, in a busy room or not: what it gets.
    pub fn next(&mut self, now: std::time::Instant, busy: bool) -> Answer {
        let within = self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < AGAIN_AFTER);
        self.run = if within {
            self.run.saturating_add(1)
        } else {
            1
        };
        self.last = Some(now);
        match self.run {
            1 if busy => Answer::Resume,
            1 => Answer::Ask,
            2 => Answer::ResumeAgain,
            3 => Answer::Pause,
            _ => Answer::Quiet,
        }
    }

    /// The person spoke, with words, and was answered: the run starts again,
    /// so a cut after "Lamp, go on" that followed [`PAUSE`] picks the answer
    /// up, never left quiet (the Reviewer-lead's P2 on #5430).
    pub fn spoke(&mut self) {
        *self = Self::default();
    }
}

/// The note the model is asked with when the rest of the cut answer is not
/// kept (sent before it was written whole, or not told how much was
/// played): `bridge`, already said, then on from the cut.
pub fn resume_ask(bridge: &str) -> String {
    format!(
        "(No one spoke: a sound cut off your answer just above, and you have already said \"{bridge}\" to pick it up again. Go on from where it was cut, not from the start, in the same words and tone: no apology, no greeting, no \"where was I\", and do not say \"{bridge}\" again.)"
    )
}

/// Whether the switch is on in this process's environment: `on` only.
pub fn on() -> bool {
    on_for(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether the resume switch is on in this process's environment: `on` only.
pub fn resumes() -> bool {
    on_for(std::env::var(RESUMES_ENVIRONMENT).ok().as_deref())
}

fn on_for(value: Option<&str>) -> bool {
    value == Some("on")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_only_for_on() {
        assert!(on_for(Some("on")));
        for off in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(!on_for(off), "{off:?}");
        }
    }

    #[test]
    fn the_line_is_short_and_asks() {
        assert!(LINE.starts_with("Um, excuse me,"));
        assert!(LINE.ends_with('?'));
        assert!(!LINE.contains(['—', '–']));
        assert!(LINE.split_whitespace().count() <= 7);
    }

    /// The memory owner, 5 Nov: busy first resumes, quiet first asks, the
    /// second within the minute says it was cut again, the third pauses,
    /// then quiet; a minute's gap starts afresh.
    #[test]
    fn each_cut_in_a_run_gets_its_answer() {
        let start = std::time::Instant::now();
        let at = |seconds| start + std::time::Duration::from_secs(seconds);
        let mut cuts = Cuts::default();
        assert_eq!(cuts.next(at(0), false), Answer::Ask);
        assert_eq!(cuts.next(at(30), false), Answer::ResumeAgain);
        assert_eq!(
            cuts.next(at(80), true),
            Answer::Pause,
            "within 60 s of the last"
        );
        assert_eq!(cuts.next(at(90), false), Answer::Quiet);
        assert_eq!(cuts.next(at(100), true), Answer::Quiet);
        assert_eq!(
            cuts.next(at(160), true),
            Answer::Resume,
            "a minute after the last"
        );
        assert_eq!(cuts.next(at(220), false), Answer::Ask);
        // The person spoke after the pause: the run starts again.
        cuts.spoke();
        assert_eq!(cuts.next(at(230), true), Answer::Resume);
        let mut busy = Cuts::default();
        assert_eq!(busy.next(at(0), true), Answer::Resume);
        assert_eq!(busy.next(at(59), true), Answer::ResumeAgain);
    }

    #[test]
    fn the_resume_lines_are_short_and_light() {
        for line in [BRIDGE, AGAIN, PAUSE] {
            assert!(!line.contains(['—', '–']), "{line}");
        }
        assert!(BRIDGE.split_whitespace().count() <= 2);
        assert!(AGAIN.split_whitespace().count() <= 5);
        assert!(!AGAIN.to_lowercase().contains("sorry"));
        assert!(PAUSE.contains("noisy") && PAUSE.contains("'Lamp'"));
        assert_eq!(Answer::Resume.line(), Some((BRIDGE, true)));
        assert_eq!(Answer::ResumeAgain.line(), Some((AGAIN, true)));
        assert_eq!(Answer::Pause.line(), Some((PAUSE, false)));
        assert_eq!(Answer::Ask.line(), Some((LINE, false)));
        assert_eq!(Answer::Quiet.line(), None);
        let ask = resume_ask(BRIDGE);
        assert!(ask.contains("not from the start") && ask.contains("no apology"));
        assert!(ask.contains("\"Anyway,\""));
    }
}
