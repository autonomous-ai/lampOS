//! The body's lease (phase 0 of the lamp programming protocol, #3504,
//! agreed with Nine at 13:11 on 27 Oct): one holder, `lamp-control`, may
//! hold the body for a while, so that the lamp's own life stands back while
//! a program moves it. `lamp-control` keeps the protocol's leases, clients
//! and caps; the body knows only whether it is held, and why it stopped.
//!
//! - `{"hold":{"for_ms":N}}`, N from 1 to [`MOST_MS`]: taken, or renewed.
//! - `{"let_go":true}`: given back.
//!
//! Each is answered with one [`Said`], and a change while held is said the
//! same way, unasked:
//! - `{"lease":"held"}`;
//! - `{"lease":"suspended","why":"conversation"}`, while someone talks to
//!   the lamp, and `held` again after;
//! - `{"lease":"lost","why":"hand|slumped|private|paused|asleep|lapsed"}`: a hand on
//!   the head, the person's privacy, a pause or a sleep ends it, and so does
//!   a hold not renewed within its `for_ms`, so a holder that dies gives
//!   the lamp its own life back within 30 s;
//! - `{"lease":"free"}`, after `let_go`;
//! - `{"lease":"refused","why":...}`, for anything else.
//!
//! A hold asked while a hand, privacy, a pause or a sleep is in force is
//! answered `lost` with why, and nothing is held.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// The longest one hold lasts: the holder renews what it keeps.
pub const MOST_MS: u64 = 30_000;

/// What the lamp is doing besides, as the body sees it this step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Around {
    /// Someone is talking to the lamp: a mood other than resting.
    pub conversation: bool,
    /// A hand holds a joint (`body/hand`).
    pub hand: bool,
    /// The person's privacy mute (`voice/mute`).
    pub private: bool,
    /// Paused by the keeper (`lamp/pause`).
    pub paused: bool,
    /// A hand's sleep (`voice/mute`, #1362).
    pub asleep: bool,
    /// Let go under load or by a hand (`--overload release`, `--limp`): its
    /// torque off, the body can't be moved (the reviewer-lead on #3791).
    pub slumped: bool,
}

impl Around {
    /// What ends a lease now, if anything: the person first.
    fn ends(&self) -> Option<&'static str> {
        [
            (self.hand, "hand"),
            (self.slumped, "slumped"),
            (self.private, "private"),
            (self.paused, "paused"),
            (self.asleep, "asleep"),
        ]
        .into_iter()
        .find_map(|(now, why)| now.then_some(why))
    }
}

/// The lease's state, as said to its holder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Held,
    Suspended,
    Lost,
    Free,
    Refused,
}

/// One line to the holder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Said {
    pub lease: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<&'static str>,
}

impl Said {
    const HELD: Said = Said {
        lease: State::Held,
        why: None,
    };
    const SUSPENDED: Said = Said {
        lease: State::Suspended,
        why: Some("conversation"),
    };
    const FREE: Said = Said {
        lease: State::Free,
        why: None,
    };

    pub fn lost(why: &'static str) -> Said {
        Said {
            lease: State::Lost,
            why: Some(why),
        }
    }

    pub fn refused(why: &'static str) -> Said {
        Said {
            lease: State::Refused,
            why: Some(why),
        }
    }

    /// As one line of JSON.
    pub fn line(&self) -> String {
        let mut line = serde_json::to_string(self).unwrap_or_default();
        line.push('\n');
        line
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hold {
    for_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Asked {
    hold: Option<Hold>,
    let_go: Option<bool>,
}

/// The body's one lease.
#[derive(Debug, Default)]
pub struct Lease {
    until: Option<Instant>,
    suspended: bool,
}

impl Lease {
    /// One line from the holder, `around` as it is `now`: its answer.
    pub fn ask(&mut self, line: &[u8], around: Around, now: Instant) -> Said {
        let Ok(asked) = serde_json::from_slice::<Asked>(line) else {
            return Said::refused("not a lease request");
        };
        match (asked.hold, asked.let_go) {
            (None, Some(true)) => {
                self.end();
                Said::FREE
            }
            (Some(Hold { for_ms }), None) if (1..=MOST_MS).contains(&for_ms) => {
                if let Some(why) = around.ends() {
                    self.end();
                    return Said::lost(why);
                }
                self.until = Some(now + Duration::from_millis(for_ms));
                self.suspended = around.conversation;
                if self.suspended {
                    Said::SUSPENDED
                } else {
                    Said::HELD
                }
            }
            (Some(_), None) => Said::refused("for_ms is 1 to 30000"),
            _ => Said::refused("one of hold or let_go"),
        }
    }

    /// What changed this step, if anything, to be said unasked.
    pub fn step(&mut self, around: Around, now: Instant) -> Option<Said> {
        let until = self.until?;
        if now >= until {
            self.end();
            return Some(Said::lost("lapsed"));
        }
        if let Some(why) = around.ends() {
            self.end();
            return Some(Said::lost(why));
        }
        if around.conversation != self.suspended {
            self.suspended = around.conversation;
            return Some(if self.suspended {
                Said::SUSPENDED
            } else {
                Said::HELD
            });
        }
        None
    }

    /// Whether the body is held now: the lamp's own life stands back.
    pub fn holds(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now < until) && !self.suspended
    }

    /// Whether a lease is taken, held or suspended, and not yet ended or
    /// lapsed by a step: what a run's end may still tell its holder.
    pub fn taken(&self) -> bool {
        self.until.is_some()
    }

    /// Given back, or its holder gone.
    pub fn end(&mut self) {
        self.until = None;
        self.suspended = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUIET: Around = Around {
        conversation: false,
        hand: false,
        private: false,
        paused: false,
        asleep: false,
        slumped: false,
    };

    fn said(text: &str) -> String {
        text.to_owned() + "\n"
    }

    #[test]
    fn a_hold_is_held_renewed_and_let_go() {
        let mut lease = Lease::default();
        let now = Instant::now();
        assert!(!lease.holds(now));
        let held = lease.ask(br#"{"hold":{"for_ms":5000}}"#, QUIET, now);
        assert_eq!(held.line(), said(r#"{"lease":"held"}"#));
        assert!(lease.holds(now + Duration::from_millis(4999)));
        // Renewed before it lapses, it holds on.
        let later = now + Duration::from_millis(4000);
        lease.ask(br#"{"hold":{"for_ms":5000}}"#, QUIET, later);
        assert!(lease.holds(now + Duration::from_millis(8999)));
        assert_eq!(lease.step(QUIET, now + Duration::from_millis(8999)), None);
        let free = lease.ask(br#"{"let_go":true}"#, QUIET, later);
        assert_eq!(free.line(), said(r#"{"lease":"free"}"#));
        assert!(!lease.holds(later));
        assert_eq!(lease.step(QUIET, later), None, "nothing to say once free");
    }

    #[test]
    fn a_hold_not_renewed_lapses_by_itself() {
        let mut lease = Lease::default();
        let now = Instant::now();
        lease.ask(br#"{"hold":{"for_ms":30000}}"#, QUIET, now);
        let lapsed = lease.step(QUIET, now + Duration::from_secs(30)).unwrap();
        assert_eq!(lapsed.line(), said(r#"{"lease":"lost","why":"lapsed"}"#));
        assert!(!lease.holds(now + Duration::from_secs(30)));
        assert_eq!(lease.step(QUIET, now + Duration::from_secs(31)), None);
    }

    #[test]
    fn a_conversation_suspends_it_and_its_end_gives_it_back() {
        let mut lease = Lease::default();
        let now = Instant::now();
        lease.ask(br#"{"hold":{"for_ms":10000}}"#, QUIET, now);
        let talking = Around {
            conversation: true,
            ..QUIET
        };
        let suspended = lease.step(talking, now).unwrap();
        assert_eq!(
            suspended.line(),
            said(r#"{"lease":"suspended","why":"conversation"}"#)
        );
        assert!(!lease.holds(now), "the conversation has the body");
        assert_eq!(lease.step(talking, now), None, "said once");
        assert_eq!(lease.step(QUIET, now), Some(Said::HELD));
        assert!(lease.holds(now));
        // Asked during a conversation, it is taken suspended.
        let mut lease = Lease::default();
        let asked = lease.ask(br#"{"hold":{"for_ms":10000}}"#, talking, now);
        assert_eq!(asked, Said::SUSPENDED);
        assert!(!lease.holds(now));
    }

    #[test]
    fn a_hand_privacy_a_pause_or_a_sleep_ends_it() {
        let now = Instant::now();
        for (around, why) in [
            (
                Around {
                    hand: true,
                    ..QUIET
                },
                "hand",
            ),
            (
                Around {
                    private: true,
                    ..QUIET
                },
                "private",
            ),
            (
                Around {
                    paused: true,
                    ..QUIET
                },
                "paused",
            ),
            (
                Around {
                    asleep: true,
                    ..QUIET
                },
                "asleep",
            ),
            (
                Around {
                    slumped: true,
                    ..QUIET
                },
                "slumped",
            ),
            (
                Around {
                    hand: true,
                    paused: true,
                    conversation: true,
                    ..QUIET
                },
                "hand",
            ),
        ] {
            let mut lease = Lease::default();
            lease.ask(br#"{"hold":{"for_ms":10000}}"#, QUIET, now);
            assert_eq!(lease.step(around, now), Some(Said::lost(why)), "{why}");
            assert!(!lease.holds(now));
            // It is not taken back when that ends: the holder asks again.
            assert_eq!(lease.step(QUIET, now), None);
            assert!(!lease.holds(now));
            // Asked while it lasts, it is not held.
            let mut lease = Lease::default();
            let asked = lease.ask(br#"{"hold":{"for_ms":10000}}"#, around, now);
            assert_eq!(asked, Said::lost(why), "{why}");
            assert!(!lease.holds(now));
        }
    }

    #[test]
    fn anything_else_is_refused_with_why() {
        let mut lease = Lease::default();
        let now = Instant::now();
        for (line, why) in [
            (&br#"{"hold":{"for_ms":0}}"#[..], "for_ms is 1 to 30000"),
            (br#"{"hold":{"for_ms":30001}}"#, "for_ms is 1 to 30000"),
            (
                br#"{"hold":{"for_ms":5000},"let_go":true}"#,
                "one of hold or let_go",
            ),
            (br#"{"let_go":false}"#, "one of hold or let_go"),
            (br#"{}"#, "one of hold or let_go"),
            (
                br#"{"hold":{"for_ms":5000,"priority":"person"}}"#,
                "not a lease request",
            ),
            (br#"{"take":true}"#, "not a lease request"),
            (b"hold", "not a lease request"),
        ] {
            assert_eq!(lease.ask(line, QUIET, now), Said::refused(why), "{line:?}");
            assert!(!lease.holds(now));
        }
        assert_eq!(
            Said::refused("not a lease request").line(),
            said(r#"{"lease":"refused","why":"not a lease request"}"#)
        );
    }
}
