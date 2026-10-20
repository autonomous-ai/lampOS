//! Who was at the lamp over the last hours, from `room/presence` (#1685,
//! "was anyone here while I was out?"): when someone was there, and when the
//! lamp could not tell. Times only, kept in memory and lost at a restart:
//! never a picture, a face or who anyone was, as `room/presence` carries
//! none (`docs/presence.md`). What showed someone is kept as one yes or no
//! a spell: whether a face did, or only a voice (`by`, #1771).
use lamp_msgs::{RoomPresence, SeenBy};
use std::collections::VecDeque;

/// A value of `room/presence` means nothing past this: a later one after a
/// longer gap means the lamp could not tell meanwhile.
const LASTS_US: u64 = RoomPresence::LASTS.as_micros() as u64;
/// The longest a skill may look back ([`crate::Skill`]'s `[history]`).
pub const KEPT_US: u64 = 24 * 3_600_000_000;
/// The most spells, and stretches unseen, kept: the oldest go first.
const MOST: usize = 64;

/// When someone was at the lamp, and when it could not tell, on the
/// engine's clock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Visits {
    /// Each spell someone was at the lamp: from, until (none while it goes
    /// on), and whether a face showed them in it, not only a voice. A value
    /// that names nothing (an older eyes') counts as a face, as before.
    spells: VecDeque<(u64, Option<u64>, bool)>,
    /// Each stretch the lamp could not tell: from, until.
    unseen: VecDeque<(u64, u64)>,
    /// The newest value: when, and whether someone was there.
    last: Option<(u64, bool)>,
    /// With `LAMP_WHO_WAS_HERE_KNOWN=on` (`crate::known`): the spells, by
    /// their start, in which the eyes knew the face at the lamp. A mark
    /// only: no name, nothing of whom.
    known: Vec<u64>,
    /// With `LAMP_WHO_WAS_HERE_NAMES=on` too (#1997): the names the eyes knew
    /// in a spell, by the spell's start, only of people who said yes to
    /// being named to others, four a spell at most, in memory only.
    names: Vec<(u64, String)>,
}

/// How a spell's person is said: as someone, someone the lamp knows, or
/// with the name of someone who said yes to being named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Telling {
    Someone,
    Known,
    Named,
}

/// The most names kept for one spell.
const MOST_NAMES: usize = 4;

impl Visits {
    /// A value of `room/presence` at `at_us`. A spell begins when someone
    /// comes and ends when the value says nobody, back at the moment the
    /// last face or voice was (`nobody_s` before). A gap past a value's
    /// three seconds is a stretch the lamp could not tell: a spell going on
    /// then ends where it was last known. `by` is what showed someone.
    pub fn seen(&mut self, someone: bool, nobody_s: u32, by: Option<SeenBy>, at_us: u64) {
        if let Some((last_us, _)) = self.last
            && at_us < last_us
        {
            return;
        }
        let mut open = matches!(self.spells.back(), Some((_, None, _)));
        if let Some((last_us, _)) = self.last
            && at_us > last_us + LASTS_US
        {
            if open && let Some((_, until, _)) = self.spells.back_mut() {
                *until = Some(last_us);
                open = false;
            }
            self.unseen.push_back((last_us, at_us));
        }
        let face = by != Some(SeenBy::Voice);
        match (open, someone) {
            (false, true) => self.spells.push_back((at_us, None, face)),
            (true, true) => {
                if let Some((_, _, seen)) = self.spells.back_mut() {
                    *seen |= face;
                }
            }
            (true, false) => {
                if let Some((from, until, _)) = self.spells.back_mut() {
                    let left = at_us.saturating_sub(u64::from(nobody_s) * 1_000_000);
                    *until = Some(left.max(*from));
                }
            }
            _ => {}
        }
        self.last = Some((at_us, someone));
        let since = at_us.saturating_sub(KEPT_US);
        while self.spells.len() > MOST
            || self
                .spells
                .front()
                .is_some_and(|(_, until, _)| until.is_some_and(|until| until < since))
        {
            self.spells.pop_front();
        }
        let oldest = self.spells.front().map_or(u64::MAX, |(from, _, _)| *from);
        self.known.retain(|from| *from >= oldest);
        self.names.retain(|(from, _)| *from >= oldest);
        while self.unseen.len() > MOST
            || self.unseen.front().is_some_and(|(_, until)| *until < since)
        {
            self.unseen.pop_front();
        }
    }

    /// The eyes knew the face at the lamp during the spell going on, if one
    /// is.
    pub fn known_here(&mut self) {
        if let Some((from, None, _)) = self.spells.back()
            && !self.known.contains(from)
        {
            self.known.push(*from);
        }
    }

    /// The eyes knew `name`, someone who said yes to being named, at the
    /// lamp during the spell going on, if one is.
    pub fn named(&mut self, name: &str) {
        let Some((from, None, _)) = self.spells.back() else {
            return;
        };
        let from = *from;
        let here = self.names.iter().filter(|(of, _)| *of == from);
        if here.clone().count() < MOST_NAMES && !here.clone().any(|(_, kept)| kept == name) {
            self.names.push((from, name.to_owned()));
        }
    }

    /// Only the names still in `tellable` (the eyes' word, now) are kept: a
    /// no, a forget or a reset lets the rest go.
    pub fn keep_names(&mut self, tellable: &[String]) {
        self.names.retain(|(_, name)| tellable.contains(name));
    }

    /// What the record says of the last `within_us` before `now_us`, in
    /// words for the model, with each time as `when` says it (a local time
    /// of day, or how long ago).
    pub fn told(&self, now_us: u64, within_us: u64, when: impl Fn(u64) -> String) -> String {
        self.told_as(now_us, within_us, when, Telling::Someone)
    }

    /// [`Visits::told`], with `Known` each spell the eyes knew the face in
    /// said as "someone the lamp knows", never who; with `Named` too, a
    /// spell with names kept said with them.
    pub fn told_as(
        &self,
        now_us: u64,
        within_us: u64,
        when: impl Fn(u64) -> String,
        telling: Telling,
    ) -> String {
        let marked = telling != Telling::Someone;
        let Some((last_us, someone)) = self.last else {
            return "The lamp has no record of who was at it: its eyes have said nothing about \
                    the room since it started (presence is off, or they could not see). Say you \
                    can't tell."
                .into();
        };
        let since = now_us.saturating_sub(within_us);
        let seeing = now_us <= last_us + LASTS_US;
        let mut lines: Vec<(u64, String)> = Vec::new();
        for (from, until, face) in &self.spells {
            // A voice with no face: someone passing or out of view.
            let names: Vec<&str> = if telling == Telling::Named {
                self.names
                    .iter()
                    .filter(|(of, _)| of == from)
                    .map(|(_, name)| name.as_str())
                    .collect()
            } else {
                Vec::new()
            };
            let who = match names.as_slice() {
                [] if marked && self.known.contains(from) => "someone the lamp knows".to_owned(),
                [] => "someone".to_owned(),
                [one] => (*one).to_owned(),
                [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
            };
            let many = names.len() > 1;
            let (was, has_been) = match (*face, many) {
                (true, false) => ("was at the lamp", "has been at the lamp"),
                (true, true) => ("were at the lamp", "have been at the lamp"),
                (false, false) => (
                    "was heard near the lamp, not seen,",
                    "has been heard near the lamp, not seen,",
                ),
                (false, true) => (
                    "were heard near the lamp, not seen,",
                    "have been heard near the lamp, not seen,",
                ),
            };
            match until {
                Some(until) if *until >= since => lines.push((
                    *from,
                    format!(
                        "- {who} {was} for {}, {}",
                        length(until - from),
                        when(*from)
                    ),
                )),
                Some(_) => {}
                None if seeing && someone => lines.push((
                    *from,
                    format!(
                        "- {who} {has_been} for {}, since {}, and still is",
                        length(now_us.saturating_sub(*from)),
                        when(*from)
                    ),
                )),
                None => {}
            }
        }
        for (from, until) in &self.unseen {
            if *until >= since {
                lines.push((
                    *from,
                    format!(
                        "- it could not see or hear the room for {}, {}",
                        length(until - from),
                        when(*from)
                    ),
                ));
            }
        }
        if !seeing {
            lines.push((
                last_us,
                format!(
                    "- it has not been able to see the room since {}",
                    when(last_us)
                ),
            ));
        }
        lines.sort_by_key(|(from, _)| *from);
        let visits = self
            .spells
            .iter()
            .filter(|(_, until, _)| until.is_some_and(|until| until >= since))
            .count();
        let mut told = if telling == Telling::Named {
            format!(
                "The lamp's record of the room over the last {}, from its eyes and ears: times, \
                 and a name only for someone who said yes to being named to others; anyone \
                 else its eyes knew is someone the lamp knows, anyone else someone, and it keeps \
                 no picture.",
                length(within_us)
            )
        } else if marked {
            format!(
                "The lamp's record of the room over the last {}, times only, from its eyes and \
                 ears: it never knows who anyone was, only whether its eyes knew a face, someone \
                 who asked it to remember them, and keeps no picture.",
                length(within_us)
            )
        } else {
            format!(
                "The lamp's record of the room over the last {}, times only, from its eyes and \
                 ears: it never knows who anyone was, and keeps no picture.",
                length(within_us)
            )
        };
        for (_, line) in lines {
            told.push('\n');
            told.push_str(&line);
        }
        if visits == 0 {
            told.push_str("\n- nobody else was at the lamp in that time");
        }
        told
    }
}

/// A length of time as a person says it: under a minute, minutes, or hours
/// and minutes.
fn length(us: u64) -> String {
    let minutes = us / 60_000_000;
    match minutes {
        0 => "under a minute".into(),
        1..=59 => format!("{minutes} min"),
        _ if minutes.is_multiple_of(60) => format!("{} h", minutes / 60),
        _ => format!("{} h {} min", minutes / 60, minutes % 60),
    }
}

/// How long before `now_us` it was: the `when` of a record whose local time
/// is not known.
pub fn ago(now_us: u64) -> impl Fn(u64) -> String {
    move |at_us| format!("{} ago", length(now_us.saturating_sub(at_us)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;
    const M: u64 = 60 * S;

    /// The eyes' values once a second, `someone` from each spell's start
    /// until 20 s after its end, `nobody_s` counting otherwise, from `from`
    /// to `to` (seconds), but none in `shut`.
    fn said(visits: &mut Visits, spells: &[(u64, u64)], shut: &[(u64, u64)], to: u64) {
        let mut last_face = None;
        for second in 0..=to {
            if shut
                .iter()
                .any(|(from, until)| (*from..*until).contains(&second))
            {
                continue;
            }
            if spells
                .iter()
                .any(|(from, until)| (*from..*until).contains(&second))
            {
                last_face = Some(second);
            }
            let someone = last_face.is_some_and(|face| second - face < 20);
            let nobody_s = match last_face {
                _ if someone => 0,
                Some(face) => (second - face) as u32,
                None => second as u32,
            };
            visits.seen(
                someone,
                nobody_s,
                someone.then_some(SeenBy::Face),
                second * S,
            );
        }
    }

    /// Someone left at 30 s; a visitor came at 10 min for three, and the
    /// person came back at 30 min: two spells before the one going on, each
    /// ended where the last face was, not 20 s later.
    #[test]
    fn a_visit_while_someone_was_out_is_kept_as_a_spell_with_its_length() {
        let mut visits = Visits::default();
        said(
            &mut visits,
            &[(0, 30), (600, 780), (1_800, 1_900)],
            &[],
            1_900,
        );
        assert_eq!(
            visits.spells,
            [
                (0, Some(29 * S), true),
                (600 * S, Some(779 * S), true),
                (1_800 * S, None, true)
            ]
        );
        let told = visits.told(1_900 * S, 12 * 60 * M, ago(1_900 * S));
        assert!(
            told.ends_with(
                "\n- someone was at the lamp for under a minute, 31 min ago\
                 \n- someone was at the lamp for 2 min, 21 min ago\
                 \n- someone has been at the lamp for 1 min, since 1 min ago, and still is"
            ),
            "{told}"
        );
        assert!(told.starts_with("The lamp's record of the room over the last 12 h"));
        assert!(!told.contains("nobody else"));
    }

    /// Nobody but the one there now: said so. The eyes shut for ten
    /// minutes: a stretch it could not tell, and a spell going on then ends
    /// where it was last known.
    #[test]
    fn a_stretch_unseen_is_said_and_nobody_else_is_said_when_nobody_came() {
        let mut visits = Visits::default();
        said(&mut visits, &[(0, 2_000)], &[(300, 900)], 1_000);
        assert_eq!(visits.unseen, [(299 * S, 900 * S)]);
        assert_eq!(
            visits.spells,
            [(0, Some(299 * S), true), (900 * S, None, true)]
        );
        let told = visits.told(1_000 * S, 60 * M, ago(1_000 * S));
        assert!(
            told.contains("\n- it could not see or hear the room for 10 min, 11 min ago"),
            "{told}"
        );
        let mut alone = Visits::default();
        said(&mut alone, &[(0, 100)], &[], 100);
        let told = alone.told(100 * S, 60 * M, ago(100 * S));
        assert!(
            told.ends_with("and still is\n- nobody else was at the lamp in that time"),
            "{told}"
        );
    }

    /// No value ever: no record, and the model is told to say so. Values
    /// that stopped: the room not seen since then.
    #[test]
    fn with_no_value_there_is_no_record_and_with_values_stopped_it_says_since_when() {
        let none = Visits::default();
        assert!(none.told(10 * S, M, ago(10 * S)).contains("no record"));
        let mut stopped = Visits::default();
        said(&mut stopped, &[(0, 50)], &[], 60);
        let told = stopped.told(10 * M, 60 * M, ago(10 * M));
        assert!(
            told.contains("\n- it has not been able to see the room since 9 min ago"),
            "{told}"
        );
        assert!(!told.contains("still is"), "{told}");
    }

    /// Only the last day is kept, and spells past the window are not told.
    #[test]
    fn spells_older_than_the_window_are_not_told_nor_older_than_a_day_kept() {
        let mut visits = Visits::default();
        visits.seen(true, 0, None, 0);
        visits.seen(false, 20, None, 100 * S);
        let late = 25 * 3_600 * S;
        visits.seen(false, 30, None, late - 2 * S);
        visits.seen(true, 0, None, late);
        assert_eq!(visits.spells, [(late, None, true)]);
        let mut window = Visits::default();
        window.seen(true, 0, None, 0);
        window.seen(false, 20, None, 100 * S);
        window.seen(false, 0, None, 3 * 3_600 * S);
        let told = window.told(3 * 3_600 * S, 60 * M, ago(3 * 3_600 * S));
        assert!(!told.contains("someone was"), "{told}");
        assert!(told.contains("nobody else"), "{told}");
    }

    /// #1771: a spell only a voice showed is said as heard, not seen (a
    /// passer-by, or someone out of view), and one with a face in it at
    /// any moment as someone at the lamp.
    #[test]
    fn a_spell_only_a_voice_showed_is_told_as_heard_not_seen() {
        let mut visits = Visits::default();
        for second in (0..=60).step_by(2) {
            visits.seen(
                second < 40,
                if second < 40 { 0 } else { second as u32 - 20 },
                (second < 40).then_some(SeenBy::Voice),
                second * S,
            );
        }
        for second in (62..=120).step_by(2) {
            let by = if second == 80 {
                SeenBy::Face
            } else {
                SeenBy::Voice
            };
            visits.seen(true, 0, Some(by), second * S);
        }
        assert_eq!(
            visits.spells,
            [(0, Some(20 * S), false), (62 * S, None, true)]
        );
        let told = visits.told(120 * S, 60 * M, ago(120 * S));
        assert!(
            told.contains("\n- someone was heard near the lamp, not seen, for under a minute, 2 min ago")
                && told.contains("\n- someone has been at the lamp for under a minute, since under a minute ago, and still is"),
            "{told}"
        );
    }

    /// #1685's mark (`LAMP_WHO_WAS_HERE_KNOWN`, the first builder's (a)): a
    /// spell the eyes knew the face in is said as "someone the lamp knows",
    /// never who, and only when marked; without it the words are as before.
    #[test]
    fn a_spell_the_eyes_knew_is_someone_the_lamp_knows_and_never_who() {
        let mut visits = Visits::default();
        visits.known_here();
        assert!(visits.known.is_empty(), "no spell going on: nothing kept");
        visits.seen(true, 0, Some(SeenBy::Face), 10 * M);
        visits.known_here();
        visits.known_here();
        assert_eq!(visits.known, [10 * M], "once a spell");
        visits.seen(false, 20, None, 20 * M);
        visits.seen(true, 0, Some(SeenBy::Face), 30 * M);
        visits.seen(false, 20, None, 40 * M);
        let when = ago(41 * M);
        let told = visits.told_as(41 * M, 60 * M, &when, Telling::Known);
        assert!(
            told.contains("- someone the lamp knows was at the lamp for "),
            "{told}"
        );
        assert_eq!(
            told.matches("- someone was at the lamp for ").count(),
            1,
            "the second spell: {told}"
        );
        assert!(told.contains("only whether its eyes knew a face"), "{told}");
        // Unmarked (no switch, or a skill without known_faces): as before.
        let plain = visits.told_as(41 * M, 60 * M, &when, Telling::Someone);
        assert_eq!(plain, visits.told(41 * M, 60 * M, &when));
        assert!(
            !plain.contains("the lamp knows") && plain.contains("never knows who"),
            "{plain}"
        );
        // A day on, the marked spell is pruned with the spells, and its mark
        // with it (the reviewer's P3 on #1997).
        let day = KEPT_US + 41 * M;
        visits.seen(true, 0, Some(SeenBy::Face), day);
        visits.known_here();
        assert_eq!(visits.known, [day], "only the new spell's mark is kept");
    }

    /// #1997: with names on, a spell is said with the names kept for it
    /// (only of people who said yes), "were" for two; a spell the eyes knew
    /// with no name is someone the lamp knows; names no longer in the eyes'
    /// `tellable` are let go before telling.
    #[test]
    fn a_spell_is_named_only_for_who_said_yes_and_let_go_on_a_no() {
        let mut visits = Visits::default();
        visits.named("Alpha");
        assert!(visits.names.is_empty(), "no spell going on: nothing kept");
        visits.seen(true, 0, Some(SeenBy::Face), 10 * M);
        visits.known_here();
        for name in ["Alpha", "Alpha", "Beta", "Gamma", "Delta", "Epsilon"] {
            visits.named(name);
        }
        assert_eq!(visits.names.len(), 4, "at most four, each once");
        visits.seen(false, 20, None, 20 * M);
        visits.seen(true, 0, Some(SeenBy::Face), 30 * M);
        visits.known_here();
        visits.seen(false, 20, None, 40 * M);
        let when = ago(41 * M);
        visits.keep_names(&["Alpha".to_owned(), "Beta".to_owned()]);
        let told = visits.told_as(41 * M, 60 * M, &when, Telling::Named);
        assert!(
            told.contains("- Alpha and Beta were at the lamp for "),
            "{told}"
        );
        assert!(
            told.contains("- someone the lamp knows was at the lamp for "),
            "the second spell, known with no name: {told}"
        );
        assert!(
            told.contains("a name only for someone who said yes"),
            "{told}"
        );
        // Not named: the names aren't said.
        let known = visits.told_as(41 * M, 60 * M, &when, Telling::Known);
        assert!(!known.contains("Alpha"), "{known}");
        // A no: Beta's name goes.
        visits.keep_names(&["Alpha".to_owned()]);
        let after = visits.told_as(41 * M, 60 * M, &when, Telling::Named);
        assert!(after.contains("- Alpha was at the lamp for "), "{after}");
        assert!(!after.contains("Beta"), "{after}");
    }

    #[test]
    fn a_length_is_said_as_a_person_says_it() {
        assert_eq!(length(59 * S), "under a minute");
        assert_eq!(length(3 * M), "3 min");
        assert_eq!(length(120 * M), "2 h");
        assert_eq!(length(135 * M), "2 h 15 min");
    }
}
