//! Each client's rates (docs/control.md, "Safety, rates and quiet hours").
//!
//! Counted per client and per kind of action over sliding windows, on the
//! caller's monotonic clock: a refusal says how long until the oldest
//! counted action leaves its window. A refused action is not counted.

use std::collections::{HashMap, VecDeque};

use crate::protocol::Op;

/// A kind of action, each with its own windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    /// `joints` goals: 40 a second.
    Joints,
    /// `gesture`, `move`, `play`, `look`: one each second.
    Motion,
    /// `light`: 5 a second.
    Light,
    /// `say` and `sound`: 6 a minute and 60 an hour.
    Say,
    /// `express`: 20 a minute.
    Express,
    /// A change of the phrases a connection listens for: 6 a minute, so a
    /// program can't swap word lists to probe the room's speech
    /// (Fourteen's P2 on #3868).
    Phrases,
    /// A `skill.push`, counted once at its first part: one each 10 s and
    /// 20 an hour.
    Push,
    /// A `skill.list` or `skill.remove`, each a request to root's manage
    /// helper: 6 a minute (Reviewer-two's P2 on #3910).
    Manage,
    /// Actions a part refused: 20 a second, then the connection closes
    /// (Fourteen's P3 on #3798: a refusal is a round trip to a part, so
    /// spinning refused actions must cost something).
    Refused,
}

impl Class {
    /// The class an op is counted in, if any.
    pub fn of(op: &Op) -> Option<Self> {
        match op {
            Op::Joints { .. } => Some(Self::Joints),
            Op::Gesture { .. } | Op::Move { .. } | Op::Play { .. } | Op::Look { .. } => {
                Some(Self::Motion)
            }
            Op::Light { .. } => Some(Self::Light),
            Op::Say { .. } | Op::Sound { .. } => Some(Self::Say),
            Op::Express { .. } => Some(Self::Express),
            _ => None,
        }
    }

    /// (most, within µs) pairs, each holding.
    pub fn windows(self) -> &'static [(usize, u64)] {
        const S: u64 = 1_000_000;
        match self {
            Self::Joints => &[(40, S)],
            Self::Motion => &[(1, S)],
            Self::Light => &[(5, S)],
            Self::Say => &[(6, 60 * S), (60, 3_600 * S)],
            Self::Express => &[(20, 60 * S)],
            Self::Phrases => &[(6, 60 * S)],
            Self::Push => &[(1, 10 * S), (20, 3_600 * S)],
            Self::Manage => &[(6, 60 * S)],
            Self::Refused => &[(20, S)],
        }
    }

    /// The class's bounds, as a refusal's why says them.
    pub fn says(self) -> &'static str {
        match self {
            Self::Joints => "joints: 40 a second",
            Self::Motion => "motion: 1 each second",
            Self::Light => "light: 5 a second",
            Self::Say => "say: 6 a minute and 60 an hour",
            Self::Express => "express: 20 a minute",
            Self::Phrases => "phrases: 6 changes a minute",
            Self::Push => "skill.push: 1 each 10 s and 20 an hour",
            Self::Manage => "skill.list and skill.remove: 6 a minute",
            Self::Refused => "refused actions: 20 a second",
        }
    }

    fn longest(self) -> u64 {
        self.windows()
            .iter()
            .map(|(_, within)| *within)
            .max()
            .unwrap_or(0)
    }
}

/// The actions counted, by client and class.
#[derive(Debug, Default)]
pub struct Rates {
    seen: HashMap<(String, Class), VecDeque<u64>>,
    /// `--rates off`, a dev lamp's switch: every class lifted but
    /// [`Class::Refused`], which still closes a connection that spins.
    off: bool,
}

impl Rates {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every rate lifted but the refused actions' guard (`--rates off`).
    pub fn off() -> Self {
        Self {
            off: true,
            ..Self::default()
        }
    }

    /// Whether the rates are lifted.
    pub fn lifted(&self) -> bool {
        self.off
    }

    /// Counts `client`'s action of `class` at `now_us`, or refuses it with
    /// how many ms until it would be allowed.
    pub fn take(&mut self, client: &str, class: Class, now_us: u64) -> Result<(), u64> {
        self.check(client, class, now_us)?;
        self.count(client, class, now_us);
        Ok(())
    }

    /// Whether `client`'s action of `class` fits at `now_us`, counting
    /// nothing: the door counts it once the part takes it, so a refused
    /// action isn't counted (harness-reviewer's P2 on #3778).
    pub fn check(&mut self, client: &str, class: Class, now_us: u64) -> Result<(), u64> {
        if self.off && class != Class::Refused {
            return Ok(());
        }
        let seen = self.seen.entry((client.to_owned(), class)).or_default();
        let longest = class.longest();
        while seen.front().is_some_and(|at| at + longest <= now_us) {
            seen.pop_front();
        }
        let mut wait_us = 0;
        for (most, within) in class.windows() {
            let inside: Vec<u64> = seen
                .iter()
                .copied()
                .filter(|at| at + within > now_us)
                .collect();
            if inside.len() >= *most {
                // The action that must leave for one more to fit.
                let oldest = inside[inside.len() - most];
                wait_us = wait_us.max(oldest + within - now_us);
            }
        }
        if wait_us > 0 {
            return Err(wait_us.div_ceil(1_000));
        }
        Ok(())
    }

    /// Counts `client`'s action of `class` at `now_us`.
    pub fn count(&mut self, client: &str, class: Class, now_us: u64) {
        self.seen
            .entry((client.to_owned(), class))
            .or_default()
            .push_back(now_us);
    }

    /// Forgets a client's counts (revoked or unpaired).
    pub fn forget(&mut self, client: &str) {
        self.seen.retain(|(of, _), _| of != client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    #[test]
    fn each_class_holds_its_windows_and_says_when_to_try_again() {
        let mut rates = Rates::new();
        // Motion: one each second (the person's 09:43, was one each 2 s).
        assert_eq!(rates.take("c_1", Class::Motion, 0), Ok(()));
        assert_eq!(rates.take("c_1", Class::Motion, S / 2), Err(500));
        assert_eq!(rates.take("c_1", Class::Motion, S), Ok(()));
        // Another client counts on its own.
        assert_eq!(rates.take("c_2", Class::Motion, S), Ok(()));
        // Joints: 40 a second (was 20).
        for n in 0..40 {
            assert_eq!(rates.take("c_1", Class::Joints, n * 10_000), Ok(()), "{n}");
        }
        assert_eq!(rates.take("c_1", Class::Joints, 500_000), Err(500));
        assert_eq!(rates.take("c_1", Class::Joints, S), Ok(()));
        // Express: 20 a minute (was 1).
        for n in 0..20 {
            assert_eq!(rates.take("c_1", Class::Express, n * S), Ok(()), "{n}");
        }
        assert_eq!(rates.take("c_1", Class::Express, 20 * S), Err(40_000));
        assert_eq!(rates.take("c_1", Class::Express, 60 * S), Ok(()));
    }

    /// `--rates off`: every class lifted, at any pace, but the refused
    /// actions' guard, which still bites at 20 a second.
    #[test]
    fn rates_off_lifts_every_class_but_the_refused_guard() {
        let mut rates = Rates::off();
        assert!(rates.lifted() && !Rates::new().lifted());
        for class in [
            Class::Joints,
            Class::Motion,
            Class::Light,
            Class::Say,
            Class::Express,
            Class::Phrases,
            Class::Push,
            Class::Manage,
        ] {
            for n in 0..1_000 {
                assert_eq!(rates.take("c_1", class, n), Ok(()), "{class:?} {n}");
            }
        }
        for n in 0..20 {
            assert_eq!(rates.take("c_1", Class::Refused, n), Ok(()), "{n}");
        }
        assert!(rates.take("c_1", Class::Refused, 20).is_err());
    }

    #[test]
    fn say_holds_six_a_minute_and_sixty_an_hour() {
        let mut rates = Rates::new();
        let mut now = 0;
        let mut said = 0;
        // Asked every 5 s for an hour: 6 in each minute, 60 in the hour.
        while now < 3_600 * S {
            if rates.take("c_1", Class::Say, now).is_ok() {
                said += 1;
            }
            now += 5 * S;
        }
        assert_eq!(said, 60);
        let mut rates = Rates::new();
        for n in 0..6 {
            assert_eq!(rates.take("c_1", Class::Say, n * S), Ok(()));
        }
        assert_eq!(rates.take("c_1", Class::Say, 6 * S), Err(54_000));
        rates.forget("c_1");
        assert_eq!(rates.take("c_1", Class::Say, 6 * S), Ok(()));
    }

    #[test]
    fn ops_fall_in_their_classes() {
        let op = |frame: &str| crate::protocol::parse(frame).unwrap().op;
        assert_eq!(
            Class::of(&op(r#"{"id":1,"op":"gesture","name":"nod"}"#)),
            Some(Class::Motion)
        );
        assert_eq!(
            Class::of(&op(r#"{"id":1,"op":"say","text":"Hi."}"#)),
            Some(Class::Say)
        );
        assert_eq!(
            Class::of(&op(r#"{"id":1,"op":"sound","name":"chime"}"#)),
            Some(Class::Say)
        );
        assert_eq!(
            Class::of(&op(r#"{"id":1,"op":"express","feeling":"glad"}"#)),
            Some(Class::Express)
        );
        assert_eq!(Class::of(&op(r#"{"id":1,"op":"stop"}"#)), None);
        assert_eq!(
            Class::of(&op(r#"{"id":1,"op":"get","state":["lamp"]}"#)),
            None
        );
    }
}
