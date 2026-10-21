//! #1712's B kept from dropping the asker (`LAMP_MOTION_VOICE`, the first
//! builder on #1722, 21 Oct 03:27 and 03:38).
//!
//! With `LAMP_MOTION_KEEPS_ANSWER=on`, a new sound that began as the body
//! moved a joint, while an answer is awaited, is let go as the lamp's own
//! motors (`body_motion`). It was built on one case (lamp-4ace, 20 Oct
//! 04:34: a wordless line with the thinking pose's elbow), whose level was
//! never recorded. On lamp-4ace on 21 Oct its 168 let-gos all fell on
//! played lines, and at 03:11 the rest of "my name is" was let go four
//! times as the body turned to the voice. The lamp's resting sway is
//! inaudible on mic2 (-84 dBFS, the same as stillness); the big moves'
//! sound is still to be measured, with a person at the arm.
//!
//! So a sound about as loud as the asker is taken for the asker: its own
//! speech blocks, from its first read to the rule's decision, against the
//! asker's level as `asker_first` keeps it (the middle of their own speech
//! blocks, away from the lamp's echo):
//! - `record`: let go as before; its record says both levels and whether
//!   `on` would have kept it, so the bar comes from real lines;
//! - `on`: kept from the motion rule, it goes on to the rules after it, as
//!   if the body were still.
//!
//! Without the asker's level (asker-first off, or too little of the
//! request heard), or with fewer than [`crate::asker_first::ENOUGH_BLOCKS`]
//! of its own speech blocks, it is let go as before. The known cost of
//! `on`: a motor sound as loud as the asker is the asker's too, and goes on
//! to the answer wait, which takes a sound that keeps going as a line.

use crate::asker_first::{ENOUGH_BLOCKS, Levels};

/// Where the coordinator reads the switch: `record` or `on`.
pub const ENVIRONMENT: &str = "LAMP_MOTION_VOICE";

/// How far under the asker's level a sound may be and still be theirs: 6 dB,
/// asker-first's margin, until the big moves' motor sound is measured.
pub const MARGIN: u8 = crate::asker_first::MARGIN;

/// What the switch asks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Off,
    /// Judged and recorded; the sound is still let go.
    Record,
    On,
}

impl Mode {
    pub fn from_environment() -> Self {
        Self::from(std::env::var(ENVIRONMENT).ok().as_deref())
    }

    pub fn from(value: Option<&str>) -> Self {
        match value {
            Some("on") => Self::On,
            Some("record") => Self::Record,
            _ => Self::Off,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Record => "record",
            Self::On => "on",
        }
    }
}

/// What becomes of a sound begun as the body moved, while an answer is
/// awaited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Let go as the lamp's motors; `would_keep` is what `on` would do.
    LetGo { would_keep: bool },
    /// About as loud as the asker: theirs, kept from the motion rule.
    Kept,
}

/// Whether `levels` say the asker's voice: enough of its own speech blocks,
/// their upper tenth no more than [`MARGIN`] under the asker's level (the
/// middle of their speech), as asker-first judges. A voice's loud blocks
/// are its words, and a motor's whine is flat. On 6 to 10 blocks the upper
/// tenth is about the loudest, so this leans to keeping: a few loud motor
/// blocks over a quiet hum pass it (Fourth's P3 2 on #2371). `record` also
/// writes the middle, so the bar can be set on either before `on`.
pub fn is_askers(levels: Option<&Levels>) -> bool {
    levels.is_some_and(|levels| {
        levels.blocks >= ENOUGH_BLOCKS && levels.upper >= levels.asker.saturating_sub(MARGIN)
    })
}

/// What `mode` does with a sound begun as the body moved, heard at `levels`.
pub fn decide(mode: Mode, levels: Option<&Levels>) -> Decision {
    let askers = is_askers(levels);
    if mode == Mode::On && askers {
        Decision::Kept
    } else {
        Decision::LetGo { would_keep: askers }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels(asker: u8, upper: u8, blocks: usize) -> Levels {
        Levels {
            asker,
            heard: upper,
            upper,
            blocks,
        }
    }

    #[test]
    fn record_or_on_switches_it_on() {
        assert_eq!(Mode::from(Some("on")), Mode::On);
        assert_eq!(Mode::from(Some("record")), Mode::Record);
        for other in [None, Some(""), Some("1"), Some("ON")] {
            assert_eq!(Mode::from(other), Mode::Off, "{other:?}");
        }
    }

    /// Motor sound well under the asker is let go in every mode, and `on`
    /// would not have kept it.
    #[test]
    fn a_sound_well_under_the_asker_is_let_go() {
        let motors = levels(150, 40, 20);
        for mode in [Mode::Off, Mode::Record, Mode::On] {
            assert_eq!(
                decide(mode, Some(&motors)),
                Decision::LetGo { would_keep: false },
                "{mode:?}"
            );
        }
    }

    /// The asker going on as the body turns to them (lamp-4ace, 21 Oct
    /// 03:11): `on` keeps it, `record` lets it go and says it would have.
    /// Exactly the margin under is still theirs; one step more is not.
    #[test]
    fn the_asker_going_on_is_kept_with_on_and_recorded_with_record() {
        let asker = levels(150, 140, 10);
        assert_eq!(decide(Mode::On, Some(&asker)), Decision::Kept);
        assert_eq!(
            decide(Mode::Record, Some(&asker)),
            Decision::LetGo { would_keep: true }
        );
        assert_eq!(
            decide(Mode::Off, Some(&asker)),
            Decision::LetGo { would_keep: true }
        );
        let at_margin = levels(150, 150 - MARGIN, ENOUGH_BLOCKS);
        assert_eq!(decide(Mode::On, Some(&at_margin)), Decision::Kept);
        let past = levels(150, 150 - MARGIN - 1, ENOUGH_BLOCKS);
        assert_eq!(
            decide(Mode::On, Some(&past)),
            Decision::LetGo { would_keep: false }
        );
    }

    /// The upper tenth is what's compared, not the middle (the reviewer's
    /// P3 2 on #2371): a sound whose middle is more than the margin under
    /// the asker but whose loud blocks aren't is theirs.
    #[test]
    fn the_upper_tenth_is_compared_not_the_middle() {
        let words = Levels {
            asker: 150,
            heard: 150 - MARGIN - 30,
            upper: 150 - MARGIN / 2,
            blocks: 10,
        };
        assert_eq!(decide(Mode::On, Some(&words)), Decision::Kept);
        let quiet = Levels {
            upper: 150 - MARGIN - 1,
            ..words
        };
        assert_eq!(
            decide(Mode::On, Some(&quiet)),
            Decision::LetGo { would_keep: false }
        );
    }

    /// Without the asker's level, or with too few of its own speech blocks,
    /// it is let go as before.
    #[test]
    fn with_no_askers_level_or_too_little_heard_it_is_let_go() {
        assert_eq!(
            decide(Mode::On, None),
            Decision::LetGo { would_keep: false }
        );
        let short = levels(150, 150, ENOUGH_BLOCKS - 1);
        assert_eq!(
            decide(Mode::On, Some(&short)),
            Decision::LetGo { would_keep: false }
        );
    }

    /// The 04:34 case, the known cost (the first builder at 03:38): a
    /// wordless sound at the asker's level as a joint moves, with nothing
    /// after it. Its level can't tell it from the asker, so `on` keeps it
    /// from the motion rule; the answer wait then judges it, and lets it go
    /// only if it ends or goes quiet first.
    #[test]
    fn a_wordless_sound_at_the_askers_level_is_kept_with_on_the_known_cost() {
        let wordless = levels(150, 150, 8);
        assert_eq!(decide(Mode::On, Some(&wordless)), Decision::Kept);
        assert_eq!(
            decide(Mode::Record, Some(&wordless)),
            Decision::LetGo { would_keep: true }
        );
    }
}
