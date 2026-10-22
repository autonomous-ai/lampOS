//! How the person talking with the lamp seems, told to the second voice.
//!
//! The eyes read the near face (`humans/seems`, #1671, `docs/eyes.md`). A
//! creature that sees someone smiling answers a touch more warmly; it never
//! says "you look happy". So a spoken request from a known person in view
//! is told, beside their words, that they seem in good spirits from their
//! face just now, with the rule to let it shape the tone only and never
//! mention it. Only after two `positive` reads in a row by Blendshape V2
//! (the product's reader), each a distinct message, fresh and sure, as
//! Mood's own run (#2716); only in a room that is not busy, and not again
//! for ten minutes once heard. Nothing of the read is kept past the note.
//!
//! `negative` is left out: brows drawn down while concentrating read as low
//! (`LAMP_SEEMS_BROW_LOW`); negative waits for the labelled session's
//! at-the-desk count (#2733).
//!
//! Off unless the service's environment says `on`; it stays off until the
//! reads' measure passes (#2733's labelled session).
use lamp_msgs::{HumanSeems, SeemsKind, SeemsReader};
use std::time::Duration;

/// Where the coordinator reads whether the face's read is told. `on` only.
pub const ENVIRONMENT: &str = "LAMP_SECOND_SEEMS";
/// The provider worker's flag for it.
pub const FLAG: &str = "--second-seems";

/// A read less sure than this breaks the run.
pub const SURE_FROM: f32 = 0.6;
/// Sure `positive` reads in a row before it is told: one is noise.
const RUN: u32 = 2;
/// And not again for this long once said.
const AGAIN_AFTER: Duration = Duration::from_secs(600);
/// The newest read, kept by the worker's poll, says nothing this long after
/// it was taken off the bus (the poll is every second; the bus's own
/// `HumanSeems::LASTS` is checked as it is taken).
const KEPT_FOR: Duration = Duration::from_secs(2);

/// What the model is told.
pub const NOTE: &str = "(For you to know, not to answer: the person you are talking with seems in good spirits from their face just now. Never mention it, their face or their mood; let it only shape your tone gently. If it does not fit what they say, ignore it.)";

/// What the provider worker knows of the near face, from `humans/seems`.
#[derive(Debug, Default)]
pub struct SeemsTold {
    /// The bus time of the last read counted, so one message counts once.
    last_said_ms: Option<u64>,
    run: u32,
    seen_at_us: Option<u64>,
    told_at_us: Option<u64>,
}

impl SeemsTold {
    /// The newest fresh read on the bus with the time it was said, or none.
    /// None, or anything but a sure `positive` by Blendshape V2 (an
    /// `unsure`, a `negative`, a FER+ read), breaks the run.
    pub fn seen(&mut self, read: Option<(HumanSeems, u64)>, now_us: u64) {
        let Some((read, said_ms)) = read else {
            *self = Self {
                told_at_us: self.told_at_us,
                ..Self::default()
            };
            return;
        };
        self.seen_at_us = Some(now_us);
        if self.last_said_ms == Some(said_ms) {
            return;
        }
        self.last_said_ms = Some(said_ms);
        // `negative` is left out (#2733): see the module's note.
        let smiling = read.kind == SeemsKind::Positive
            && read.from == SeemsReader::Blendshapes
            && read.confidence >= SURE_FROM;
        self.run = if smiling { self.run + 1 } else { 0 };
    }

    /// The note, if one is due now.
    pub fn due(&self, now_us: u64) -> Option<String> {
        let at = self.seen_at_us?;
        if now_us.saturating_sub(at) >= KEPT_FOR.as_micros() as u64 || self.run < RUN {
            return None;
        }
        if self
            .told_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < AGAIN_AFTER.as_micros() as u64)
        {
            return None;
        }
        Some(NOTE.to_owned())
    }

    /// The note was given.
    pub fn told(&mut self, now_us: u64) {
        self.told_at_us = Some(now_us);
    }
}

/// The provider worker's argument for the switch in the environment.
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

/// Take a leading `--second-seems` off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000_000;

    fn read(kind: SeemsKind, confidence: f32, said_ms: u64) -> Option<(HumanSeems, u64)> {
        Some((
            HumanSeems {
                kind,
                confidence,
                from: SeemsReader::Blendshapes,
            },
            said_ms,
        ))
    }

    #[test]
    fn two_smiles_in_a_row_are_told_once_then_not_for_ten_minutes() {
        let mut seems = SeemsTold::default();
        seems.seen(read(SeemsKind::Positive, 0.8, 1), 0);
        assert_eq!(seems.due(0), None, "one read is noise");
        // The same message polled again is not a second read.
        seems.seen(read(SeemsKind::Positive, 0.8, 1), 0);
        assert_eq!(seems.due(0), None);
        seems.seen(read(SeemsKind::Positive, 0.8, 2), 0);
        let note = seems.due(0).unwrap();
        assert!(note.contains("in good spirits") && note.contains("Never mention it"));
        assert!(note.starts_with("(For you to know, not to answer"));
        seems.told(0);
        seems.seen(read(SeemsKind::Positive, 0.9, 3), 9 * MIN);
        assert_eq!(seems.due(9 * MIN), None);
        seems.seen(read(SeemsKind::Positive, 0.9, 4), 10 * MIN);
        assert!(seems.due(10 * MIN).is_some());
        assert_eq!(
            seems.due(10 * MIN + KEPT_FOR.as_micros() as u64),
            None,
            "stale"
        );
    }

    #[test]
    fn a_low_unsure_weak_fer_plus_or_missing_read_breaks_the_run() {
        let fer_plus = Some((
            HumanSeems {
                kind: SeemsKind::Positive,
                confidence: 0.9,
                from: SeemsReader::FerPlus,
            },
            2,
        ));
        for breaker in [
            read(SeemsKind::Negative, 0.9, 2),
            read(SeemsKind::Unsure, 0.9, 2),
            read(SeemsKind::Positive, 0.59, 2),
            read(SeemsKind::Positive, f32::NAN, 2),
            fer_plus,
            None,
        ] {
            let mut seems = SeemsTold::default();
            seems.seen(read(SeemsKind::Positive, 0.9, 1), 0);
            seems.seen(breaker, 0);
            seems.seen(read(SeemsKind::Positive, 0.9, 3), 0);
            assert_eq!(seems.due(0), None, "{breaker:?}");
        }
    }

    #[test]
    fn the_switch_is_on_only_when_set_and_its_flag_is_taken_off() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        assert!(arguments(None).is_empty() && arguments(Some("yes")).is_empty());
        let flags = vec![FLAG.to_owned(), "--other".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }
}
