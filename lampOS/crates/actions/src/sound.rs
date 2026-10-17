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

/// The rules, over whatever control there is.
pub struct Volume<M> {
    mixer: M,
    /// The loudest the speaker is allowed, in points.
    ceiling: u8,
    /// Where it stood when it was muted.
    before_mute: Option<u8>,
}

const UNREAD: &str = "I could not read my speaker's volume.";
const UNCONFIRMED: &str = "I could not confirm my speaker's volume changed.";

impl<M: Mixer> Volume<M> {
    pub fn new(mixer: M, ceiling: u8) -> Self {
        Self {
            mixer,
            ceiling: ceiling.clamp(1, 100),
            before_mute: None,
        }
    }

    /// Set, read back, and say what is there now. A control may round: one
    /// point either way is the level asked for.
    fn change(&mut self, from: u8, to: u8, said: impl Fn(u8) -> String) -> Outcome {
        if self.mixer.set(to).is_err() {
            return Outcome::Failed("I could not change my speaker's volume.".into());
        }
        match self.mixer.get() {
            Ok(now) if now.abs_diff(to) <= 1 && (now != from || to == from) => {
                Outcome::Done(said(now))
            }
            _ => Outcome::Failed(UNCONFIRMED.into()),
        }
    }

    pub fn up(&mut self) -> Outcome {
        let Ok(current) = self.mixer.get() else {
            return Outcome::Failed(UNREAD.into());
        };
        let ceiling = self.ceiling;
        if current >= ceiling {
            return Outcome::Done("My speaker is already as loud as it is allowed to go.".into());
        }
        let next = ceiling.min(current.saturating_add(STEP));
        self.before_mute = None;
        self.change(current, next, |now| {
            format!("My volume is now {now} out of {ceiling}; it was {current}.")
        })
    }

    pub fn down(&mut self) -> Outcome {
        let Ok(current) = self.mixer.get() else {
            return Outcome::Failed(UNREAD.into());
        };
        if current == 0 {
            return Outcome::Done("My speaker is muted.".into());
        }
        let floor = FLOOR.min(self.ceiling);
        if current <= floor {
            return Outcome::Done(
                "My speaker is already as quiet as it goes without being muted.".into(),
            );
        }
        let ceiling = self.ceiling;
        self.change(current, floor.max(current.saturating_sub(STEP)), |now| {
            format!("My volume is now {now} out of {ceiling}; it was {current}.")
        })
    }

    pub fn mute(&mut self) -> Outcome {
        let Ok(current) = self.mixer.get() else {
            return Outcome::Failed(UNREAD.into());
        };
        if current == 0 {
            return Outcome::Done("My speaker is already muted.".into());
        }
        let outcome = self.change(current, 0, |_| "My speaker is muted.".into());
        if matches!(outcome, Outcome::Done(_)) {
            self.before_mute = Some(current);
        }
        outcome
    }

    pub fn unmute(&mut self) -> Outcome {
        let Ok(current) = self.mixer.get() else {
            return Outcome::Failed(UNREAD.into());
        };
        let ceiling = self.ceiling;
        if current > 0 {
            return Outcome::Done(format!(
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
        let share = (u16::from(raw) * 100 + u16::from(self.ceiling) / 2) / u16::from(self.ceiling);
        Level {
            volume: share.min(100) as u8,
            raw,
            max_volume: self.ceiling,
        }
    }

    /// The volume now, for the app's page (#1396).
    pub fn level(&mut self) -> Result<Level, String> {
        self.mixer
            .get()
            .map(|raw| self.level_at(raw))
            .map_err(|_| UNREAD.to_owned())
    }

    /// Set the volume to `volume` of 100 of the allowed range, as the app's
    /// slider asks (#1396), read back: within a point of the level asked,
    /// or not done. Never past the ceiling.
    pub fn to_share(&mut self, volume: u8) -> Result<Level, String> {
        let raw = ((u16::from(volume.min(100)) * u16::from(self.ceiling) + 50) / 100) as u8;
        if self.mixer.set(raw).is_err() {
            return Err("I could not change my speaker's volume.".to_owned());
        }
        match self.mixer.get() {
            Ok(now) if now.abs_diff(raw) <= 1 => {
                if now > 0 {
                    self.before_mute = None;
                }
                Ok(self.level_at(now))
            }
            _ => Err(UNCONFIRMED.to_owned()),
        }
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
            Outcome::Done(said) => said,
            Outcome::Failed(said) => panic!("expected it done, was told: {said}"),
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
            said(volume.up()),
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
            said(volume.down()),
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
        assert_eq!(said(volume.mute()), "My speaker is already muted.");
        assert_eq!(
            said(volume.unmute()),
            "My speaker is back, at 70 out of 100."
        );
        assert_eq!(
            said(volume.unmute()),
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
            Outcome::Done(said) => panic!("expected a failure, was told: {said}"),
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
    fn the_level_is_read_from_what_amixer_prints() {
        let printed = "Simple mixer control 'Speaker',0\n  Capabilities: volume\n  Playback channels: Front Left - Front Right\n  Limits: 0 - 100\n  Front Left: 62 [62%]\n  Front Right: 62 [62%]\n";
        assert_eq!(percent(printed), Some(62));
        assert_eq!(percent("  Mono: Playback 0 [0%] [-30.00dB] [on]"), Some(0));
        assert_eq!(percent("  Mono: Playback 255 [100%] [0.00dB]"), Some(100));
        assert_eq!(percent("no level here [on]"), None);
        assert_eq!(percent("[300%]"), None);
    }
}
