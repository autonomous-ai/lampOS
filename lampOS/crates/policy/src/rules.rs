//! What the people at the lamp asked it about speaking first (#813), as the
//! voice says it on `memory/rules` (`lamp_msgs::Rules`, kept until the
//! next): obeyed as a hard rule by the decisions about speaking first, the
//! hello on arrival, progress news and a skill's asks (#939), before any
//! model is asked (`lamp-policy --rules`).
//!
//! - `quiet`: asked never to speak first.
//! - `quiet_mornings`: asked not to speak first before noon, the lamp's
//!   local time. With the local time not known it holds all day: a rule
//!   kept is obeyed whenever it may hold.
//! - Per kind, `hello` for the hello and `news` for progress news (the
//!   seventh's #904, every typed rule merged, the strictest winning):
//!   `never`; `not_before_minute` and `not_from_minute`, the lamp's local
//!   minute of the day, holding all day with the local time not known;
//!   and `not_while_talking` ("on a call, or with someone"), holding while
//!   the room is not known to be quiet. Either question already waits
//!   while anyone talks to the lamp; this adds the talk that is not to it.
//!   A limit of one kind never holds the other; `quiet` and
//!   `quiet_mornings` hold both.
//! - Anything else the lamp would start itself (a skill's `unprompted` ask:
//!   a stretch break, stuffy air) has no limits of its own: it is held by
//!   `quiet`, `quiet_mornings` and the limits of both kinds. Someone who
//!   asked for no hello before nine, or no news while on a call, is not
//!   to be invited to stretch then either. Authored: a rule errs toward
//!   quiet.
//!
//! The newest `Rules` is obeyed whoever it names, the person of the last
//! session opened or rule changed, or `everyone`: who is at the lamp now is
//! not known here, and a rule errs toward quiet. Only booleans come: never
//! the words of a rule.
use lamp_msgs::{Rules, Unprompted};

/// What the lamp would say unprompted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The hello on arrival.
    Hello,
    /// News of work: progress, results.
    News,
    /// Anything else it would start itself: held by either kind's limits.
    /// Not a reminder the person set: no kept rule holds that, only someone
    /// talking (`moment::REMINDER`, the reviewer on #1003).
    Unprompted,
}

/// Noon, in minutes of the day: where a morning ends.
pub const NOON: u16 = 12 * 60;

/// The minute of the day at the lamp, `offset` minutes east of UTC.
pub fn minute_of_day(unix_ms: u64, offset: i32) -> u16 {
    let minutes = (unix_ms / 60_000) as i64 + i64::from(offset);
    minutes.rem_euclid(1_440) as u16
}

/// Whether `rules` keep the lamp from saying `kind` unprompted now:
/// `minute` is the minute of the day at the lamp, `None` if the local time
/// is not known, and `room_quiet` whether the room is known to be quiet.
pub fn keep_quiet(rules: &Rules, kind: Kind, minute: Option<u16>, room_quiet: bool) -> bool {
    let held = |limits: &Unprompted| {
        limits.never
            || limits
                .not_before_minute
                .is_some_and(|from| minute.is_none_or(|minute| minute < from))
            || limits
                .not_from_minute
                .is_some_and(|until| minute.is_none_or(|minute| minute >= until))
            || (limits.not_while_talking && !room_quiet)
    };
    rules.quiet
        || (rules.quiet_mornings && minute.is_none_or(|minute| minute < NOON))
        || match kind {
            Kind::Hello => held(&rules.hello),
            Kind::News => held(&rules.news),
            Kind::Unprompted => held(&rules.hello) || held(&rules.news),
        }
}

/// The newest rules, as the hello and progress questions hold them, and
/// the lamp's local time to read them by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    newest: Option<Rules>,
}

impl Kept {
    /// What the voice said last. Kept when the voice goes: a rule does not
    /// end with it, and without a voice the lamp says nothing anyway.
    pub fn set(&mut self, rules: Rules) {
        self.newest = Some(rules);
    }

    /// Whether a rule keeps the lamp from saying `kind` unprompted at
    /// `unix_ms`, its local time `utc_offset_minutes` east of UTC if known,
    /// with the room known to be quiet or not.
    pub fn hold(
        &self,
        kind: Kind,
        unix_ms: u64,
        utc_offset_minutes: Option<i32>,
        room_quiet: bool,
    ) -> bool {
        self.newest.as_ref().is_some_and(|rules| {
            keep_quiet(
                rules,
                kind,
                utc_offset_minutes.map(|offset| minute_of_day(unix_ms, offset)),
                room_quiet,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(quiet: bool, quiet_mornings: bool) -> Rules {
        Rules {
            person: "p-be9688998f085098".into(),
            quiet,
            quiet_mornings,
            rules: 1,
            hello: Unprompted::default(),
            news: Unprompted::default(),
            keep_light_on: false,
        }
    }

    /// 2026-10-16 01:59 UTC: 08:59 in Hanoi, seven hours east.
    const HANOI_0859: u64 = 1_792_115_940_000;
    const HANOI: i32 = 7 * 60;

    #[test]
    fn quiet_holds_always_and_quiet_mornings_until_noon_or_always_if_the_time_is_not_known() {
        assert_eq!(minute_of_day(HANOI_0859, HANOI), 8 * 60 + 59);
        assert_eq!(minute_of_day(HANOI_0859, 0), 60 + 59);
        // Across midnight backwards: 01:59 UTC is 20:59 the day before, five
        // hours west.
        assert_eq!(minute_of_day(HANOI_0859, -5 * 60), 20 * 60 + 59);
        for kind in [Kind::Hello, Kind::News, Kind::Unprompted] {
            assert!(keep_quiet(&rules(true, false), kind, Some(15 * 60), true));
            assert!(keep_quiet(&rules(false, true), kind, Some(NOON - 1), true));
            assert!(!keep_quiet(&rules(false, true), kind, Some(NOON), true));
            assert!(keep_quiet(&rules(false, true), kind, None, true));
            assert!(!keep_quiet(&rules(false, false), kind, None, false));
        }

        let hold =
            |kept: &Kept, at: u64, offset: Option<i32>| kept.hold(Kind::Hello, at, offset, true);
        let mut kept = Kept::default();
        assert!(!hold(&kept, HANOI_0859, Some(HANOI)));
        kept.set(rules(false, true));
        assert!(hold(&kept, HANOI_0859, Some(HANOI)));
        // Three hours and a minute later it is noon in Hanoi.
        assert!(!hold(&kept, HANOI_0859 + 181 * 60_000, Some(HANOI)));
        assert!(hold(&kept, HANOI_0859 + 181 * 60_000, None));
        // The newest is obeyed: a rule forgotten is a rule no more.
        kept.set(rules(false, false));
        assert!(!hold(&kept, HANOI_0859, Some(HANOI)));
    }

    /// The seventh's typed limits (#904), each for its own kind only:
    /// never; not before and not from a minute of the day, all day with the
    /// time not known; not while talking, unless the room is known quiet.
    #[test]
    fn a_limit_of_one_kind_holds_that_kind_only_by_the_hour_and_the_room() {
        let limited = |limits: Unprompted| Rules {
            hello: limits,
            ..rules(false, false)
        };
        let never = limited(Unprompted {
            never: true,
            ..Unprompted::default()
        });
        assert!(keep_quiet(&never, Kind::Hello, Some(15 * 60), true));
        assert!(!keep_quiet(&never, Kind::News, Some(15 * 60), true));
        // Not before 09:00 and not from 22:00.
        let hours = limited(Unprompted {
            not_before_minute: Some(9 * 60),
            not_from_minute: Some(22 * 60),
            ..Unprompted::default()
        });
        for (minute, held) in [
            (8 * 60 + 59, true),
            (9 * 60, false),
            (21 * 60 + 59, false),
            (22 * 60, true),
            (0, true),
        ] {
            assert_eq!(
                keep_quiet(&hours, Kind::Hello, Some(minute), true),
                held,
                "{minute}"
            );
            assert!(!keep_quiet(&hours, Kind::News, Some(minute), true));
        }
        assert!(keep_quiet(&hours, Kind::Hello, None, true));
        // Not while talking: a busy room, or one not known, holds.
        let talking = Rules {
            news: Unprompted {
                not_while_talking: true,
                ..Unprompted::default()
            },
            ..rules(false, false)
        };
        assert!(!keep_quiet(&talking, Kind::News, Some(15 * 60), true));
        assert!(keep_quiet(&talking, Kind::News, Some(15 * 60), false));
        assert!(!keep_quiet(&talking, Kind::Hello, Some(15 * 60), false));
    }

    /// Anything else the lamp would start (#939) is held by either kind's
    /// limits, and by neither when there are none.
    #[test]
    fn anything_else_unprompted_is_held_by_the_limits_of_either_kind() {
        let at_three = Some(15 * 60);
        assert!(!keep_quiet(
            &rules(false, false),
            Kind::Unprompted,
            at_three,
            false
        ));
        let no_hello = Rules {
            hello: Unprompted {
                never: true,
                ..Unprompted::default()
            },
            ..rules(false, false)
        };
        assert!(keep_quiet(&no_hello, Kind::Unprompted, at_three, true));
        let no_news_before_nine = Rules {
            news: Unprompted {
                not_before_minute: Some(9 * 60),
                ..Unprompted::default()
            },
            ..rules(false, false)
        };
        assert!(keep_quiet(
            &no_news_before_nine,
            Kind::Unprompted,
            Some(8 * 60 + 59),
            true
        ));
        assert!(!keep_quiet(
            &no_news_before_nine,
            Kind::Unprompted,
            Some(9 * 60),
            true
        ));
        assert!(keep_quiet(
            &no_news_before_nine,
            Kind::Unprompted,
            None,
            true
        ));
        let not_on_a_call = Rules {
            news: Unprompted {
                not_while_talking: true,
                ..Unprompted::default()
            },
            ..rules(false, false)
        };
        assert!(keep_quiet(
            &not_on_a_call,
            Kind::Unprompted,
            at_three,
            false
        ));
        assert!(!keep_quiet(
            &not_on_a_call,
            Kind::Unprompted,
            at_three,
            true
        ));
    }
}
