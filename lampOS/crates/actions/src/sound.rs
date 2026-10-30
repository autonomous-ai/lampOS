//! The speaker's volume: read, change, read back.
//!
//! On lamp-4ace the speaker is an ALSA software volume with one control
//! (`Speaker` on the card `allwinnerhdmi`, a hundred points over 30 dB, so
//! 0.3 dB a point). The executive is the only writer of it.
//!
//! Louder and quieter are the same size, [`STEP`] points, about 6 dB: a
//! change a person hears clearly without it being a jump, and one "louder"
//! undoes one "quieter". Quieter stops at [`FLOOR`], the least that can
//! still be heard across a desk; silence is its own request, mute. V1 made
//! louder a tenth of the range and quieter a halving, so that one "a bit
//! quieter" took three or more "louder" to undo; that is not kept
//! (docs/actions.md). The step is reasoned, not yet heard by a person.

use crate::Outcome;
use std::{
    io,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// One louder or one quieter, in points: about 6 dB on the lamp's control.
pub const STEP: u8 = 20;
/// Where quieter stops, in points. Below it is mute's.
pub const FLOOR: u8 = 10;

/// A volume control, in points from 0 to 100.
pub trait Mixer {
    fn get(&mut self) -> io::Result<u8>;
    fn set(&mut self, level: u8) -> io::Result<()>;
}

/// Retained evidence from the sole mixer owner. A later successful read cannot
/// erase an intervening zero or loss. Sequence distinguishes actual observation
/// work from an unchanged cache; exhausted identities can never authorize audio.
#[derive(Clone, Copy)]
pub(crate) struct Observation {
    pub raw: Option<u8>,
    pub revocation: Option<u64>,
    pub sequence: Option<u64>,
}

/// The rules, over whatever control there is.
pub struct Volume<M> {
    mixer: M,
    /// The product's ceiling, never replaced by a temporary limit.
    product_ceiling: u8,
    /// A temporary raw ceiling, independent of the person's volume.
    limit: Option<u8>,
    /// Where it stood when it was muted.
    before_mute: Option<u8>,
    observed: Option<u8>,
    last_observed: Option<u8>,
    observation_revocation: Option<u64>,
    observation_sequence: Option<u64>,
    /// The raw level last read from the mixer, as it is (`heard`).
    heard_raw: Option<u8>,
}

const UNREAD: &str = "I could not read my speaker's volume.";
const UNCONFIRMED: &str = "I could not confirm my speaker's volume changed.";
const UNCHANGED: &str = "I could not change my speaker's volume.";

impl<M: Mixer> Volume<M> {
    pub fn new(mixer: M, ceiling: u8) -> Self {
        Self {
            mixer,
            product_ceiling: ceiling.clamp(1, 100),
            limit: None,
            before_mute: None,
            observed: None,
            last_observed: None,
            observation_revocation: Some(0),
            observation_sequence: Some(0),
            heard_raw: None,
        }
    }

    /// The volume as last read from the mixer, with no new read: what
    /// `speaker/volume` says after each call (`volume_out`). None before a
    /// first read.
    pub fn heard(&self) -> Option<Level> {
        self.heard_raw.map(|raw| self.level_at(raw))
    }

    pub fn observed(&self) -> Option<u8> {
        self.observed
    }

    pub(crate) fn observation(&self) -> Observation {
        Observation {
            raw: self.observed,
            revocation: self.observation_revocation,
            sequence: self.observation_sequence,
        }
    }

    fn record_observation(&mut self, raw: Option<u8>) {
        // Readback above an active accepted limit is known hardware state, but
        // is not evidence that requested audio may play. level() still reports
        // the real raw value to its caller; this cache is output permission.
        let raw = raw.filter(|raw| *raw <= 100 && (self.limit.is_none() || *raw <= self.ceiling()));
        if (raw.is_none() && self.last_observed.is_some())
            || (raw == Some(0) && self.last_observed != Some(0))
        {
            self.observation_revocation = self
                .observation_revocation
                .and_then(|revision| revision.checked_add(1));
        }
        self.observation_sequence = self
            .observation_sequence
            .and_then(|sequence| sequence.checked_add(1));
        self.last_observed = raw;
        self.observed = self
            .observation_revocation
            .and(self.observation_sequence)
            .and(raw);
    }

    fn read(&mut self) -> io::Result<u8> {
        self.observed = None;
        let result = self.mixer.get();
        if let Ok(raw) = result {
            self.heard_raw = Some(raw);
        }
        self.record_observation(result.as_ref().ok().copied());
        result
    }

    fn write_raw(&mut self, raw: u8) -> io::Result<()> {
        // Successful write-in-progress invalidation is not itself observation
        // loss. The following real read or write failure settles its evidence.
        self.observed = None;
        let result = self.mixer.set(raw);
        if result.is_err() {
            self.record_observation(None);
        }
        result
    }

    fn ceiling(&self) -> u8 {
        self.limit.map_or(self.product_ceiling, |limit| {
            limit.min(self.product_ceiling)
        })
    }

    /// Apply a raw ceiling of 1..=100, bounded by the product's ceiling.
    /// A louder mixer is lowered now; a quieter or muted one is left alone.
    /// The restriction stays active even if its hardware change fails:
    /// a later call or another `set_limit` can try the bounded write again.
    ///
    /// `None` releases the restriction without writing or restoring a level.
    /// A remembered pre-mute level is lowered with the limit too, never
    /// restored on release, so a later unmute cannot revive that old level.
    /// The result reports the actual level, not proof of durable storage;
    /// the caller owns the limit's lifetime and restart recovery.
    pub fn set_limit(&mut self, limit: Option<u8>) -> Result<Level, String> {
        if limit.is_some_and(|limit| !(1..=100).contains(&limit)) {
            return Err("volume limit must be a whole number from 1 to 100".into());
        }
        self.limit = limit;
        let ceiling = self.ceiling();
        if limit.is_some() {
            self.before_mute = self.before_mute.map(|level| level.min(ceiling));
        }
        let current = self.read().map_err(|_| UNREAD.to_owned())?;
        let now = if limit.is_some() && current > ceiling {
            self.write(ceiling)?
        } else {
            current
        };
        Ok(self.level_at(now))
    }

    /// Every write goes through the current limit. A control may round by
    /// a point, but an active limit never confirms a level above its ceiling,
    /// nor a request for silence that still reads above zero.
    fn write(&mut self, to: u8) -> Result<u8, String> {
        let ceiling = self.ceiling();
        let to = if self.limit.is_some() {
            to.min(ceiling)
        } else {
            to
        };
        self.write_raw(to).map_err(|_| UNCHANGED.to_owned())?;
        match self.read() {
            Ok(now)
                if now.abs_diff(to) <= 1
                    && (self.limit.is_none() || (now <= ceiling && (to != 0 || now == 0))) =>
            {
                Ok(now)
            }
            _ => {
                self.record_observation(None);
                Err(UNCONFIRMED.to_owned())
            }
        }
    }

    /// Set, read back, and say what is there now. A control may round: one
    /// point either way is the level asked for.
    fn change(&mut self, from: u8, to: u8, said: impl Fn(u8) -> String) -> Outcome {
        match self.write(to) {
            Ok(now) if now != from || to == from => Outcome::Done(said(now)),
            Ok(_) => {
                self.record_observation(None);
                Outcome::Failed(UNCONFIRMED.into())
            }
            Err(why) => Outcome::Failed(why),
        }
    }

    pub fn up(&mut self) -> Outcome {
        let Ok(current) = self.read() else {
            return Outcome::Failed(UNREAD.into());
        };
        let ceiling = self.ceiling();
        if current == ceiling || (current > ceiling && self.limit.is_none()) {
            return Outcome::Already(
                "My speaker is already as loud as it is allowed to go.".into(),
            );
        }
        let next = ceiling.min(current.saturating_add(STEP));
        self.before_mute = None;
        self.change(current, next, |now| {
            format!("My volume is now {now} out of {ceiling}; it was {current}.")
        })
    }

    pub fn down(&mut self) -> Outcome {
        let Ok(current) = self.read() else {
            return Outcome::Failed(UNREAD.into());
        };
        if current == 0 {
            return Outcome::Already("My speaker is muted.".into());
        }
        let ceiling = self.ceiling();
        let floor = FLOOR.min(ceiling);
        if current <= floor {
            return Outcome::Already(
                "My speaker is already as quiet as it goes without being muted.".into(),
            );
        }
        self.change(current, floor.max(current.saturating_sub(STEP)), |now| {
            format!("My volume is now {now} out of {ceiling}; it was {current}.")
        })
    }

    pub fn mute(&mut self) -> Outcome {
        let Ok(current) = self.read() else {
            return Outcome::Failed(UNREAD.into());
        };
        if current == 0 {
            return Outcome::Already("My speaker is already muted.".into());
        }
        let outcome = self.change(current, 0, |_| "My speaker is muted.".into());
        if matches!(outcome, Outcome::Done(_)) {
            self.before_mute = Some(if self.limit.is_some() {
                current.min(self.ceiling())
            } else {
                current
            });
        }
        outcome
    }

    pub fn unmute(&mut self) -> Outcome {
        let Ok(current) = self.read() else {
            return Outcome::Failed(UNREAD.into());
        };
        let ceiling = self.ceiling();
        if current > 0 {
            if self.limit.is_some() && current > ceiling {
                return self.change(current, ceiling, |now| {
                    format!("My volume is now {now} out of {ceiling}; it was {current}.")
                });
            }
            return Outcome::Already(format!(
                "My speaker is not muted; its volume is {current} out of {ceiling}."
            ));
        }
        // Where it was, or the middle when that is not known: this process
        // may have started after the mute.
        let back = self.before_mute.unwrap_or(ceiling / 2).clamp(1, ceiling);
        let outcome = self.change(0, back, |now| {
            format!("My speaker is back, at {now} out of {ceiling}.")
        });
        if matches!(outcome, Outcome::Done(_)) {
            self.before_mute = None;
        }
        outcome
    }
}

/// The volume as the Autonomous app's slider has it (V1's `volume.*`):
/// `volume` is 0 to 100 of the allowed range, `raw` the control's points and
/// `max_volume` the ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Level {
    pub volume: u8,
    pub raw: u8,
    pub max_volume: u8,
}

impl<M: Mixer> Volume<M> {
    fn level_at(&self, raw: u8) -> Level {
        let ceiling = self.ceiling();
        let share = (u16::from(raw) * 100 + u16::from(ceiling) / 2) / u16::from(ceiling);
        Level {
            volume: share.min(100) as u8,
            raw,
            max_volume: ceiling,
        }
    }

    /// The volume now, for the app's page (#1396).
    pub fn level(&mut self) -> Result<Level, String> {
        self.read()
            .map(|raw| self.level_at(raw))
            .map_err(|_| UNREAD.to_owned())
    }

    /// Set the volume to `volume` of 100 of the allowed range, as the app's
    /// slider asks (#1396), read back: within a point of the level asked,
    /// or not done. Never past the ceiling.
    pub fn to_share(&mut self, volume: u8) -> Result<Level, String> {
        let raw = ((u16::from(volume.min(100)) * u16::from(self.ceiling()) + 50) / 100) as u8;
        let now = self.write(raw)?;
        if now > 0 {
            self.before_mute = None;
        }
        Ok(self.level_at(now))
    }
}

/// The control through `amixer`, the tool ALSA ships. Each call is one
/// short process, ended if it has not finished in [`AMIXER_WAIT`].
pub struct Amixer {
    pub card: String,
    pub control: String,
}

const AMIXER_WAIT: Duration = Duration::from_millis(800);

impl Amixer {
    fn run(&self, arguments: &[&str]) -> io::Result<String> {
        let mut child = Command::new("amixer")
            .args(["-c", &self.card])
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                let output = child.wait_with_output()?;
                if !status.success() {
                    return Err(io::Error::other("amixer refused"));
                }
                return String::from_utf8(output.stdout)
                    .map_err(|_| io::Error::other("amixer said something unreadable"));
            }
            if started.elapsed() >= AMIXER_WAIT {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "amixer did not finish",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// The first `[NN%]` in what `amixer` printed.
pub fn percent(printed: &str) -> Option<u8> {
    printed.split('[').skip(1).find_map(|rest| {
        let (number, _) = rest.split_once("%]")?;
        number.parse::<u8>().ok().filter(|level| *level <= 100)
    })
}

impl Mixer for Amixer {
    fn get(&mut self) -> io::Result<u8> {
        percent(&self.run(&["sget", &self.control])?)
            .ok_or_else(|| io::Error::other("amixer printed no level"))
    }
    fn set(&mut self, level: u8) -> io::Result<()> {
        self.run(&["-q", "sset", &self.control, &format!("{}%", level.min(100))])
            .map(drop)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A control that holds what it is given, or misbehaves as told.
    #[derive(Default)]
    pub(crate) struct Fake {
        pub level: u8,
        pub unreadable: bool,
        pub unwritable: bool,
        /// Takes a write and keeps its old level.
        pub stuck: bool,
        pub writes: Vec<u8>,
    }
    impl Mixer for Fake {
        fn get(&mut self) -> io::Result<u8> {
            if self.unreadable {
                return Err(io::Error::other("unreadable"));
            }
            Ok(self.level)
        }
        fn set(&mut self, level: u8) -> io::Result<()> {
            if self.unwritable {
                return Err(io::Error::other("unwritable"));
            }
            self.writes.push(level);
            if !self.stuck {
                self.level = level;
            }
            Ok(())
        }
    }
    fn at(level: u8, ceiling: u8) -> Volume<Fake> {
        Volume::new(
            Fake {
                level,
                ..Fake::default()
            },
            ceiling,
        )
    }
    fn said(outcome: Outcome) -> String {
        match outcome {
            Outcome::Done(said) | Outcome::Already(said) => said,
            Outcome::Failed(said) => panic!("expected it done, was told: {said}"),
        }
    }
    /// Said, and nothing changed: an `Already`, never answered short (#1520).
    fn already(outcome: Outcome) -> String {
        match outcome {
            Outcome::Already(said) => said,
            other => panic!("expected it already so, was told: {other:?}"),
        }
    }

    /// #1396: the app's slider reads and sets the volume as a share of the
    /// allowed range, never past the ceiling, and a set is read back.
    #[test]
    fn the_apps_slider_is_a_share_of_the_ceiling_read_back() {
        let mut volume = at(60, 80);
        assert_eq!(
            volume.level(),
            Ok(Level {
                volume: 75,
                raw: 60,
                max_volume: 80
            })
        );
        assert_eq!(
            volume.to_share(50),
            Ok(Level {
                volume: 50,
                raw: 40,
                max_volume: 80
            })
        );
        assert_eq!(volume.to_share(100).unwrap().raw, 80);
        assert_eq!(
            volume.to_share(0).unwrap(),
            Level {
                volume: 0,
                raw: 0,
                max_volume: 80
            }
        );
        let mut stuck = Volume::new(
            Fake {
                level: 30,
                stuck: true,
                ..Fake::default()
            },
            100,
        );
        assert_eq!(stuck.to_share(90), Err(UNCONFIRMED.to_owned()));
        let mut unreadable = Volume::new(
            Fake {
                unreadable: true,
                ..Fake::default()
            },
            100,
        );
        assert_eq!(unreadable.level(), Err(UNREAD.to_owned()));
    }

    #[test]
    fn a_limit_lowers_the_mixer_and_keeps_the_product_ceiling() {
        let mut volume = at(70, 80);
        assert_eq!(
            volume.set_limit(Some(40)),
            Ok(Level {
                volume: 100,
                raw: 40,
                max_volume: 40,
            })
        );
        assert_eq!(volume.mixer.writes, [40]);
        // Reapplying it, or relaxing it, never raises the speaker.
        assert_eq!(volume.set_limit(Some(40)).unwrap().raw, 40);
        assert_eq!(
            volume.set_limit(Some(90)),
            Ok(Level {
                volume: 50,
                raw: 40,
                max_volume: 80,
            })
        );
        assert_eq!(volume.set_limit(None).unwrap().max_volume, 80);
        assert_eq!(volume.mixer.writes, [40]);
        assert_eq!(
            said(volume.up()),
            "My volume is now 60 out of 80; it was 40."
        );
    }

    #[test]
    fn entering_or_releasing_a_limit_never_raises_or_unmutes_the_speaker() {
        for current in [0, 1, 10, 30, 40] {
            let mut volume = at(current, 80);
            assert_eq!(volume.set_limit(Some(40)).unwrap().raw, current);
            assert_eq!(volume.set_limit(None).unwrap().raw, current);
            assert!(volume.mixer.writes.is_empty(), "was at {current}");
        }
    }

    #[test]
    fn invalid_limits_leave_the_existing_restriction_and_hardware_alone() {
        let mut volume = at(70, 80);
        volume.set_limit(Some(40)).unwrap();
        volume.mixer.unreadable = true;
        for limit in [0, 101, 255] {
            assert_eq!(
                volume.set_limit(Some(limit)),
                Err("volume limit must be a whole number from 1 to 100".into())
            );
        }
        volume.mixer.unreadable = false;
        assert_eq!(volume.level().unwrap().max_volume, 40);
        assert_eq!(volume.mixer.writes, [40]);
    }

    #[test]
    fn releasing_a_limit_does_not_undo_a_persons_quieter_or_muted_choice() {
        let mut volume = at(80, 100);
        volume.set_limit(Some(40)).unwrap();
        said(volume.down());
        assert_eq!(volume.set_limit(None).unwrap().raw, 20);
        assert_eq!(volume.mixer.writes, [40, 20]);
        // The app's newer choice belongs to the person too.
        volume.set_limit(Some(40)).unwrap();
        assert_eq!(volume.to_share(25).unwrap().raw, 10);
        assert_eq!(volume.set_limit(None).unwrap().raw, 10);
        assert_eq!(volume.mixer.writes, [40, 20, 10]);
        volume.set_limit(Some(40)).unwrap();
        said(volume.mute());
        assert_eq!(volume.set_limit(None).unwrap().raw, 0);
        assert_eq!(volume.mixer.writes, [40, 20, 10, 0]);
        assert_eq!(
            said(volume.unmute()),
            "My speaker is back, at 10 out of 100."
        );
    }

    #[test]
    fn releasing_a_limit_cannot_revive_a_louder_pre_mute_level() {
        let mut volume = at(70, 100);
        said(volume.mute());
        assert_eq!(volume.set_limit(Some(40)).unwrap().raw, 0);
        assert_eq!(volume.set_limit(Some(60)).unwrap().raw, 0);
        assert_eq!(volume.set_limit(None).unwrap().raw, 0);
        assert_eq!(volume.mixer.writes, [0]);
        assert_eq!(
            said(volume.unmute()),
            "My speaker is back, at 40 out of 100."
        );

        // A mixer that has risen outside the owner must not be remembered
        // above the active limit when the person mutes it again.
        volume.set_limit(Some(30)).unwrap();
        volume.mixer.level = 90;
        said(volume.mute());
        volume.set_limit(None).unwrap();
        assert_eq!(
            said(volume.unmute()),
            "My speaker is back, at 30 out of 100."
        );
    }

    #[test]
    fn every_person_write_obeys_the_limit_when_the_mixer_is_above_it_again() {
        for action in ["up", "down", "unmute", "mute", "app"] {
            let mut volume = at(80, 100);
            volume.set_limit(Some(40)).unwrap();
            volume.mixer.writes.clear();
            volume.mixer.level = 95;
            let expected = if action == "mute" { 0 } else { 40 };
            match action {
                "up" => assert!(matches!(volume.up(), Outcome::Done(_))),
                "down" => assert!(matches!(volume.down(), Outcome::Done(_))),
                "unmute" => assert!(matches!(volume.unmute(), Outcome::Done(_))),
                "mute" => assert!(matches!(volume.mute(), Outcome::Done(_))),
                _ => assert_eq!(volume.to_share(100).unwrap().raw, expected),
            }
            assert_eq!(volume.mixer.level, expected, "{action}");
            assert_eq!(volume.mixer.writes, [expected], "{action}");
        }
    }

    #[test]
    fn the_app_and_explicit_unmute_use_the_effective_range_even_below_the_floor() {
        let mut volume = at(70, 80);
        volume.set_limit(Some(40)).unwrap();
        assert_eq!(
            volume.to_share(50).unwrap(),
            Level {
                volume: 50,
                raw: 20,
                max_volume: 40,
            }
        );
        said(volume.mute());
        assert_eq!(
            said(volume.unmute()),
            "My speaker is back, at 20 out of 40."
        );
        volume.set_limit(Some(3)).unwrap();
        assert_eq!(
            already(volume.down()),
            "My speaker is already as quiet as it goes without being muted."
        );
        assert_eq!(volume.to_share(0).unwrap().raw, 0);
        assert_eq!(said(volume.unmute()), "My speaker is back, at 1 out of 3.");
        assert_eq!(volume.to_share(100).unwrap().raw, 3);
    }

    #[test]
    fn a_failed_limit_still_restricts_later_calls_and_can_be_retried() {
        for failure in ["read", "write", "stuck"] {
            let mut volume = at(80, 100);
            volume.mixer.unreadable = failure == "read";
            volume.mixer.unwritable = failure == "write";
            volume.mixer.stuck = failure == "stuck";
            let expected = match failure {
                "read" => UNREAD,
                "write" => UNCHANGED,
                _ => UNCONFIRMED,
            };
            assert_eq!(
                volume.set_limit(Some(40)),
                Err(expected.into()),
                "{failure}"
            );
            volume.mixer.unreadable = false;
            volume.mixer.unwritable = false;
            volume.mixer.stuck = false;
            assert_eq!(volume.level().unwrap().max_volume, 40, "{failure}");
            assert_eq!(volume.mixer.level, 80, "{failure}");
            assert!(matches!(volume.up(), Outcome::Done(_)), "{failure}");
            assert_eq!(volume.mixer.level, 40, "{failure}");
            volume.mixer.level = 80;
            assert_eq!(volume.set_limit(Some(40)).unwrap().raw, 40);
        }
    }

    #[test]
    fn a_limit_whose_readback_fails_is_kept_without_claiming_it_was_applied() {
        struct ReadbackFailure {
            level: u8,
            written: bool,
        }
        impl Mixer for ReadbackFailure {
            fn get(&mut self) -> io::Result<u8> {
                if self.written {
                    Err(io::Error::other("readback failed"))
                } else {
                    Ok(self.level)
                }
            }
            fn set(&mut self, level: u8) -> io::Result<()> {
                self.level = level;
                self.written = true;
                Ok(())
            }
        }
        let mut volume = Volume::new(
            ReadbackFailure {
                level: 80,
                written: false,
            },
            100,
        );
        assert_eq!(volume.set_limit(Some(40)), Err(UNCONFIRMED.into()));
        volume.mixer.written = false;
        assert_eq!(volume.level().unwrap().raw, 40);
        assert_eq!(volume.level().unwrap().max_volume, 40);
        assert_eq!(volume.to_share(100), Err(UNCONFIRMED.into()));
        assert_eq!(volume.mixer.level, 40);
    }

    #[test]
    fn a_control_rounding_above_the_limit_or_above_silence_is_not_confirmed() {
        struct RoundingUp(u8);
        impl Mixer for RoundingUp {
            fn get(&mut self) -> io::Result<u8> {
                Ok(self.0)
            }
            fn set(&mut self, level: u8) -> io::Result<()> {
                self.0 = level + 1;
                Ok(())
            }
        }
        let mut volume = Volume::new(RoundingUp(80), 100);
        assert_eq!(volume.set_limit(Some(40)), Err(UNCONFIRMED.into()));
        assert!(matches!(volume.up(), Outcome::Failed(_)));
        assert!(matches!(volume.unmute(), Outcome::Failed(_)));
        volume.mixer.0 = 80;
        assert!(matches!(volume.down(), Outcome::Failed(_)));
        assert_eq!(volume.to_share(100), Err(UNCONFIRMED.into()));
        assert_eq!(volume.level().unwrap().max_volume, 40);
        assert!(matches!(volume.mute(), Outcome::Failed(_)));
        assert_eq!(volume.to_share(0), Err(UNCONFIRMED.into()));
    }

    #[test]
    fn louder_is_one_step_and_stops_at_the_ceiling() {
        let mut volume = at(50, 100);
        assert_eq!(
            said(volume.up()),
            "My volume is now 70 out of 100; it was 50."
        );
        let mut volume = at(75, 80);
        assert_eq!(
            said(volume.up()),
            "My volume is now 80 out of 80; it was 75."
        );
        // At the ceiling nothing is written.
        assert_eq!(
            already(volume.up()),
            "My speaker is already as loud as it is allowed to go."
        );
        assert_eq!(volume.mixer.writes, [80]);
    }

    /// One "quieter" is undone by one "louder", which V1's halving was not.
    #[test]
    fn one_louder_undoes_one_quieter() {
        let mut volume = at(75, 100);
        volume.down();
        assert_eq!(volume.mixer.level, 55);
        volume.up();
        assert_eq!(volume.mixer.level, 75);
    }

    #[test]
    fn quieter_is_one_step_and_stops_where_it_can_still_be_heard() {
        let mut volume = at(60, 100);
        for (now, was) in [(40, 60), (20, 40), (10, 20)] {
            assert_eq!(
                said(volume.down()),
                format!("My volume is now {now} out of 100; it was {was}.")
            );
        }
        assert_eq!(
            already(volume.down()),
            "My speaker is already as quiet as it goes without being muted."
        );
        assert_eq!(volume.mixer.writes, [40, 20, 10]);
        // Muted is said as muted, and nothing is written.
        let mut volume = at(0, 100);
        assert_eq!(said(volume.down()), "My speaker is muted.");
        assert!(volume.mixer.writes.is_empty());
    }

    #[test]
    fn mute_remembers_where_the_volume_was_and_unmute_puts_it_back() {
        let mut volume = at(70, 100);
        assert_eq!(said(volume.mute()), "My speaker is muted.");
        assert_eq!(volume.mixer.level, 0);
        assert_eq!(already(volume.mute()), "My speaker is already muted.");
        // Quieter while muted changes nothing either (the reviewer on #1520).
        assert_eq!(already(volume.down()), "My speaker is muted.");
        assert_eq!(volume.mixer.level, 0);
        assert_eq!(
            said(volume.unmute()),
            "My speaker is back, at 70 out of 100."
        );
        assert_eq!(
            already(volume.unmute()),
            "My speaker is not muted; its volume is 70 out of 100."
        );
        // A process that started after the mute has nothing remembered: the
        // middle of the allowed range.
        let mut volume = at(0, 80);
        assert_eq!(
            said(volume.unmute()),
            "My speaker is back, at 40 out of 80."
        );
    }

    /// What goes back is what was established, never a success phrase for a
    /// failure: an unreadable control, an unwritable one, and one that takes
    /// the write and does not move.
    #[test]
    fn a_failure_is_said_as_a_failure() {
        let failed = |outcome: Outcome| match outcome {
            Outcome::Failed(said) => said,
            Outcome::Done(said) | Outcome::Already(said) => {
                panic!("expected a failure, was told: {said}")
            }
        };
        let mut unreadable = at(50, 100);
        unreadable.mixer.unreadable = true;
        for outcome in [
            unreadable.up(),
            unreadable.down(),
            unreadable.mute(),
            unreadable.unmute(),
        ] {
            assert_eq!(failed(outcome), UNREAD);
        }
        assert!(unreadable.mixer.writes.is_empty());
        let mut unwritable = at(50, 100);
        unwritable.mixer.unwritable = true;
        assert_eq!(
            failed(unwritable.up()),
            "I could not change my speaker's volume."
        );
        let mut stuck = at(50, 100);
        stuck.mixer.stuck = true;
        assert_eq!(failed(stuck.up()), UNCONFIRMED);
        assert_eq!(failed(stuck.down()), UNCONFIRMED);
        assert_eq!(failed(stuck.mute()), UNCONFIRMED);
        // Nothing is remembered of a mute that did not happen.
        assert_eq!(stuck.before_mute, None);
    }

    #[test]
    fn a_control_that_rounds_by_a_point_is_taken_as_set_and_the_level_read_is_the_one_said() {
        struct Rounding(u8);
        impl Mixer for Rounding {
            fn get(&mut self) -> io::Result<u8> {
                Ok(self.0)
            }
            fn set(&mut self, level: u8) -> io::Result<()> {
                self.0 = level.saturating_sub(1);
                Ok(())
            }
        }
        let mut volume = Volume::new(Rounding(50), 100);
        assert_eq!(
            said(volume.up()),
            "My volume is now 69 out of 100; it was 50."
        );
    }

    #[test]
    fn observation_loss_survives_recovery_without_counting_successful_writes() {
        let mut volume = at(50, 100);
        volume.level().unwrap();
        volume.mixer.unreadable = true;
        assert!(volume.level().is_err());
        let lost = volume.observation();
        assert_eq!((lost.raw, lost.revocation), (None, Some(1)));
        volume.mixer.unreadable = false;
        said(volume.up());
        said(volume.down());
        let recovered = volume.observation();
        assert_eq!((recovered.raw, recovered.revocation), (Some(50), Some(1)));
        assert_eq!(volume.mixer.writes, [70, 50]);

        volume.mixer.unwritable = true;
        assert!(matches!(volume.up(), Outcome::Failed(_)));
        assert_eq!(volume.observation().revocation, Some(2));
        volume.mixer.unwritable = false;
        said(volume.up());
        assert_eq!(volume.observation().raw, Some(70));
        assert_eq!(volume.observation().revocation, Some(2));
    }

    #[test]
    fn an_exhausted_observation_identity_cannot_recover_into_audible_evidence() {
        for sequence in [false, true] {
            let mut volume = at(50, 100);
            volume.level().unwrap();
            if sequence {
                volume.observation_sequence = Some(u64::MAX);
            } else {
                volume.observation_revocation = Some(u64::MAX);
            }
            volume.mixer.level = 0;
            said(volume.up());
            assert_eq!(volume.mixer.level, STEP);
            assert_eq!(volume.observed(), None);
            let identity = |volume: &Volume<Fake>| {
                if sequence {
                    volume.observation().sequence
                } else {
                    volume.observation().revocation
                }
            };
            assert_eq!(identity(&volume), None);
            said(volume.up());
            assert_eq!(volume.observed(), None);
            assert_eq!(identity(&volume), None);
        }
    }

    #[test]
    fn the_level_is_read_from_what_amixer_prints() {
        let printed = "Simple mixer control 'Speaker',0\n  Capabilities: volume\n  Playback channels: Front Left - Front Right\n  Limits: 0 - 100\n  Front Left: 62 [62%]\n  Front Right: 62 [62%]\n";
        assert_eq!(percent(printed), Some(62));
        assert_eq!(percent("  Mono: Playback 0 [0%] [-30.00dB] [on]"), Some(0));
        assert_eq!(percent("  Mono: Playback 255 [100%] [0.00dB]"), Some(100));
        assert_eq!(percent("no level here [on]"), None);
        assert_eq!(percent("[300%]"), None);
    }
}
