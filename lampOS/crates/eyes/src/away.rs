//! Whether anyone is at the lamp, as the eyes say it on `room/presence`
//! (#1677, `docs/presence.md`): someone while a face was seen or a voice
//! heard within [`RoomPresence::SOMEONE_WITHIN_S`], or nobody, and for how
//! long. The eyes own it because they own the larger part of the evidence,
//! the faces; the voice's lines come to them on `voice/heard`, as they do
//! for the mouth (`mesh::Heard`). Pure arithmetic on the times it is given:
//! the bus is `bus_out`'s.
//!
//! Nothing is said while the eyes are not looking (the camera closed by the
//! privacy switch, a mute or the voice gone, or stalled): nobody can be said
//! to be away then, so the last value runs out and a reader knows nothing.
//! Looks are 250 to 600 ms apart while the camera is open, so none for
//! [`BLIND_AFTER_US`] is not looking, as Expression counts the eyes quiet
//! (`crates/expression/src/alive.rs`, `EYES_QUIET_US`). When looks come back
//! after that, the count of nobody begins again from then: what happened
//! while the eyes were shut is not known.
use lamp_msgs::{RoomPresence, SeenBy};

/// No look for this long: the eyes are not looking.
pub const BLIND_AFTER_US: u64 = 5_000_000;

/// The evidence so far, and what was last said.
#[derive(Debug, Default)]
pub struct Away {
    /// Since when the eyes have been looking this time, and their last look.
    looking: Option<(u64, u64)>,
    /// When a face was last seen or a voice last heard, which, and the last
    /// face's height as a share of the picture.
    someone_at_us: Option<u64>,
    by: Option<SeenBy>,
    face_tall: Option<f32>,
    /// Whether someone was said to be there, and when it was said.
    said: Option<(bool, u64)>,
}

impl Away {
    /// A look at `now_us`, with the height of its largest face as a share
    /// of the picture if it found one.
    pub fn looked(&mut self, now_us: u64, face: Option<f32>) {
        let since = match self.looking {
            Some((since, last)) if now_us.saturating_sub(last) < BLIND_AFTER_US => since,
            _ => now_us,
        };
        self.looking = Some((since, now_us));
        if let Some(tall) = face {
            self.face_tall = Some(tall);
            self.seen(now_us, SeenBy::Face);
        }
    }

    /// A voice heard at `at_us` (when the voice heard the words, never
    /// later than now).
    pub fn heard(&mut self, at_us: u64) {
        self.seen(at_us, SeenBy::Voice);
    }

    fn seen(&mut self, at_us: u64, by: SeenBy) {
        if self.someone_at_us.is_none_or(|at| at_us >= at) {
            self.someone_at_us = Some(at_us);
            self.by = Some(by);
        }
    }

    /// Why someone is there, for the journal: a face and how tall, or a
    /// voice. Counts only, no words.
    pub fn cause(&self) -> String {
        match self.by {
            Some(SeenBy::Face) => format!(
                "face, {:.2} of the picture tall",
                self.face_tall.unwrap_or(0.0)
            ),
            Some(SeenBy::Voice) => "voice".into(),
            None => "nothing".into(),
        }
    }

    /// Whether anyone is at the lamp at `now_us`, while the eyes look.
    pub fn now(&self, now_us: u64) -> Option<RoomPresence> {
        let (since, last) = self.looking?;
        if now_us.saturating_sub(last) >= BLIND_AFTER_US {
            return None;
        }
        // Evidence from before the eyes began looking this time is not
        // counted: who came and went while they were shut is not known.
        let from = self.someone_at_us.filter(|at| *at >= since);
        let within = u64::from(RoomPresence::SOMEONE_WITHIN_S) * 1_000_000;
        if from.is_some_and(|at| now_us.saturating_sub(at) < within) {
            return Some(RoomPresence {
                someone: true,
                nobody_s: 0,
                by: self.by,
            });
        }
        let away_us = now_us.saturating_sub(from.unwrap_or(since));
        Some(RoomPresence {
            someone: false,
            nobody_s: u32::try_from(away_us / 1_000_000).unwrap_or(u32::MAX),
            by: None,
        })
    }

    /// What to say at `now_us`, if anything: once a second while the eyes
    /// look ([`RoomPresence::EVERY`]), and at once when `someone` changes.
    pub fn say(&mut self, now_us: u64) -> Option<RoomPresence> {
        let Some(now) = self.now(now_us) else {
            self.said = None;
            return None;
        };
        let every = RoomPresence::EVERY.as_micros() as u64;
        let due = self.said.is_none_or(|(someone, at)| {
            someone != now.someone || now_us.saturating_sub(at) >= every
        });
        due.then(|| {
            self.said = Some((now.someone, now_us));
            now
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    /// Looks every 500 ms from `from` to `to` (s), with a face or not.
    fn looks(away: &mut Away, from: u64, to: u64, face: bool) {
        let mut at = from * S;
        while at < to * S {
            away.looked(at, face.then_some(0.25));
            at += S / 2;
        }
    }

    /// Someone shown by a face, or nobody for so long.
    fn present(someone: bool, nobody_s: u32) -> Option<RoomPresence> {
        Some(RoomPresence {
            someone,
            nobody_s,
            by: someone.then_some(SeenBy::Face),
        })
    }

    #[test]
    fn nothing_is_said_before_the_eyes_look_or_once_they_stop() {
        let mut away = Away::default();
        assert_eq!(away.say(0), None);
        away.heard(0);
        assert_eq!(away.now(S), None, "a voice alone, with the eyes shut");
        looks(&mut away, 10, 20, false);
        assert_eq!(away.now(20 * S), present(false, 10));
        // The last look at 19.5 s: still looking at 24.4, not at 24.5.
        assert_eq!(away.now(24 * S + 400_000), present(false, 14));
        assert_eq!(away.now(24 * S + 500_000), None);
    }

    #[test]
    fn a_face_or_a_voice_is_someone_for_twenty_seconds_then_nobody_counts_from_it() {
        let mut away = Away::default();
        looks(&mut away, 0, 30, true);
        assert_eq!(away.now(30 * S), present(true, 0));
        // The last face at 29.5 s; then empty looks.
        looks(&mut away, 30, 60, false);
        assert_eq!(away.now(49 * S), present(true, 0), "a look away");
        assert_eq!(away.now(49 * S + 500_000), present(false, 20));
        assert_eq!(away.now(59 * S + 500_000), present(false, 30));
        // A voice heard out of the camera's view is someone too.
        away.heard(60 * S);
        looks(&mut away, 60, 90, false);
        let voiced = away.now(79 * S).unwrap();
        assert_eq!((voiced.someone, voiced.by), (true, Some(SeenBy::Voice)));
        assert_eq!(away.cause(), "voice");
        assert_eq!(away.now(89 * S), present(false, 29));
        // A face after it: the face is what showed someone, and how tall.
        looks(&mut away, 90, 91, true);
        assert_eq!(away.now(91 * S), present(true, 0));
        assert_eq!(away.cause(), "face, 0.25 of the picture tall");
    }

    #[test]
    fn after_the_eyes_were_shut_nobody_counts_again_from_when_they_look_again() {
        let mut away = Away::default();
        looks(&mut away, 0, 600, false);
        assert_eq!(away.now(600 * S), present(false, 600));
        // Shut for a minute (a mute), then looking again: who came and
        // went meanwhile is not known.
        looks(&mut away, 660, 700, false);
        assert_eq!(away.now(700 * S), present(false, 40));
        // A gap under five seconds (a stall, a camera reopened) counts on.
        looks(&mut away, 704, 710, false);
        assert_eq!(away.now(710 * S), present(false, 50));
        // A face seen just before the eyes shut is not someone after.
        let mut seen = Away::default();
        looks(&mut seen, 0, 10, true);
        looks(&mut seen, 16, 20, false);
        assert_eq!(seen.now(20 * S), present(false, 4));
    }

    #[test]
    fn it_is_said_once_a_second_and_at_once_when_someone_comes_or_goes() {
        let mut away = Away::default();
        let mut said = Vec::new();
        // The loop turns every 100 ms; a look every 500 ms; a face from 30 s.
        for turn in 0..600u64 {
            let at = turn * S / 10;
            if turn % 5 == 0 {
                away.looked(at, (300..310).contains(&turn).then_some(0.25));
            }
            if let Some(now) = away.say(at) {
                now.check().unwrap();
                said.push((at, now));
            }
        }
        // Once a second for a minute, and at once where someone came and went.
        let came = said
            .iter()
            .position(|(_, now)| now.someone)
            .expect("someone came");
        assert_eq!(said[came].0, 30 * S, "said at once");
        assert_eq!(
            said[came - 1],
            (
                29 * S,
                RoomPresence {
                    someone: false,
                    nobody_s: 29,
                    by: None,
                }
            )
        );
        // Someone until 20 s after the last face (30.5 s), nobody then.
        let went = said
            .iter()
            .rposition(|(_, now)| now.someone)
            .map(|at| said[at + 1])
            .unwrap();
        assert_eq!(
            went,
            (
                S * 505 / 10,
                RoomPresence {
                    someone: false,
                    nobody_s: 20,
                    by: None,
                }
            )
        );
        assert!(said.windows(2).all(|pair| pair[1].0 - pair[0].0 <= S));
        assert_eq!(said.len(), 61, "{said:?}");
    }
}
