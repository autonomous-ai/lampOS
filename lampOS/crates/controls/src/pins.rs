//! The lamp's real button and touch pads, read from GPIO lines
//! (`docs/controls.md`, "The board's reader").
//!
//! Which lines, and which way each is wired, is data in one file for the
//! board; nothing is guessed or probed while serving. Every configured line
//! is read together every few milliseconds and given as one complete
//! [`Observation`], stamped with the moment of the reading. A press or a
//! touch shorter than that is not seen, which the recognition's own
//! debounce would have thrown out anyway.
//!
//! [`LinePins`] is the rule and has no hardware: it reads through
//! [`Lines`]. [`GpioLines`] is the Linux character device behind it. The
//! privacy slide has a separate reader in `privacy`; it never joins a hand
//! observation or becomes a gesture.
use crate::{
    engine::Config,
    input::{InputError, Level, Observation, PinReader},
};
use lamp_msgs::{CONTROL_MAX_BUTTONS, CONTROL_MAX_HEAD_PADS, ClockStamp, ControlFault};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io};

/// How a line is held when nothing drives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pull {
    Up,
    Down,
    None,
}

/// One input: its name in the controls' configuration, its line on the
/// chip, and whether a press or a touch pulls it low.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub name: String,
    pub line: u32,
    pub active_low: bool,
    pub pull: Pull,
}

/// The board's wiring for the controls, for the legacy reader (`serve`
/// without `--touch`). Its units keep this file beside their binary, so no
/// other build's file reaches it, and it stays strict: an unknown field is
/// refused, so the touch path's file (with its `reset` and `base`) is never
/// read here as if it were this one. A pin's own fields are read as the
/// touch path reads them ([`crate::forward`], #1680).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinsConfig {
    /// The GPIO character device, for example `/dev/gpiochip0`.
    pub chip: String,
    pub buttons: Vec<Pin>,
    pub head_pads: Vec<Pin>,
    /// How often the lines are read, 2 to 50 ms.
    pub every_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy: Option<crate::privacy::Pins>,
}

impl PinsConfig {
    /// Sound in itself, and naming exactly the inputs the controls are
    /// configured for, in the same order.
    pub fn check(&self, controls: &Config) -> Result<(), String> {
        if let Some(privacy) = &self.privacy {
            privacy.check()?;
        }
        if !self.chip.starts_with("/dev/") || self.chip.len() > 64 {
            return Err("the chip is a device under /dev".into());
        }
        if !(2..=50).contains(&self.every_ms) {
            return Err("the lines are read every 2 to 50 ms".into());
        }
        if self.buttons.len() > CONTROL_MAX_BUTTONS || self.head_pads.len() > CONTROL_MAX_HEAD_PADS
        {
            return Err("more inputs than the controls take".into());
        }
        let mut lines = BTreeSet::new();
        for pin in self.buttons.iter().chain(&self.head_pads) {
            if self
                .privacy
                .as_ref()
                .is_some_and(|privacy| privacy.chip == self.chip && privacy.line == pin.line)
            {
                return Err("the privacy line cannot also be a hand input".into());
            }
            if !lines.insert(pin.line) {
                return Err(format!("line {} is named twice", pin.line));
            }
        }
        let names = |pins: &[Pin]| pins.iter().map(|pin| pin.name.clone()).collect::<Vec<_>>();
        if names(&self.buttons) != controls.buttons || names(&self.head_pads) != controls.head_pads
        {
            return Err(
                "the pins must name the controls' buttons and head pads, in the same order".into(),
            );
        }
        if lines.is_empty() && self.privacy.is_none() {
            return Err("no line is configured".into());
        }
        Ok(())
    }
}

/// The electrical level of a line: true is high. Never waits.
pub trait Lines {
    fn high(&mut self, line: u32) -> io::Result<bool>;
}

/// The reader the controls ask: every configured line, together, every
/// `every_ms`.
pub struct LinePins<L: Lines> {
    lines: L,
    config: PinsConfig,
    clock_id: String,
    session_id: String,
    sequence: u64,
    next_us: u64,
}

impl<L: Lines> LinePins<L> {
    /// `clock_id` names this start of the machine; `session_id` this start
    /// of the reader, and must differ every time.
    pub fn new(
        lines: L,
        config: PinsConfig,
        controls: &Config,
        clock_id: &str,
        session_id: String,
    ) -> Result<Self, String> {
        config.check(controls)?;
        Ok(Self {
            lines,
            config,
            clock_id: clock_id.to_owned(),
            session_id,
            sequence: 0,
            next_us: 0,
        })
    }

    fn levels(lines: &mut L, pins: &[Pin]) -> io::Result<Vec<Level>> {
        pins.iter()
            .map(|pin| {
                Ok(Level {
                    name: pin.name.clone(),
                    active: lines.high(pin.line)? != pin.active_low,
                })
            })
            .collect()
    }
}

impl<L: Lines> PinReader for LinePins<L> {
    fn read(&mut self, now_us: u64) -> Result<Option<Observation>, InputError> {
        if now_us < self.next_us {
            return Ok(None);
        }
        self.next_us = now_us + self.config.every_ms * 1_000;
        let read = Self::levels(&mut self.lines, &self.config.buttons).and_then(|buttons| {
            Ok((
                buttons,
                Self::levels(&mut self.lines, &self.config.head_pads)?,
            ))
        });
        let (buttons, head_pads) = read.map_err(|error| {
            InputError::new(
                ControlFault::Missing,
                format!(
                    "the lines of {} could not be read: {error}",
                    self.config.chip
                ),
            )
        })?;
        self.sequence += 1;
        Ok(Some(Observation {
            stamp: ClockStamp {
                clock_id: self.clock_id.clone(),
                at_us: now_us,
                sequence: self.sequence,
            },
            session_id: self.session_id.clone(),
            buttons,
            head_pads,
            base_mask: None,
        }))
    }
}

/// The lines themselves, held for as long as this lives.
#[cfg(target_os = "linux")]
pub struct GpioLines {
    request: gpiocdev::Request,
}

#[cfg(target_os = "linux")]
fn bias(pull: Pull) -> gpiocdev::line::Bias {
    match pull {
        Pull::Up => gpiocdev::line::Bias::PullUp,
        Pull::Down => gpiocdev::line::Bias::PullDown,
        Pull::None => gpiocdev::line::Bias::Disabled,
    }
}

#[cfg(target_os = "linux")]
impl GpioLines {
    /// Take every configured line as an input. Fails if another program
    /// holds one of them: two readers of one line is a fault to be seen.
    pub fn open(config: &PinsConfig) -> io::Result<Self> {
        let mut builder = gpiocdev::Request::builder();
        builder
            .on_chip(&config.chip)
            .with_consumer("lampOS-controls");
        for pin in config.buttons.iter().chain(&config.head_pads) {
            builder
                .with_line(pin.line)
                .as_input()
                .with_bias(bias(pin.pull));
        }
        let request = builder.request().map_err(io::Error::other)?;
        Ok(Self { request })
    }

    /// What one line reads with a pull-down and with a pull-up, held for
    /// 10 ms each: a pad that is wired drives its line and reads the same
    /// both ways, and a bare pin follows the pull. For finding out what a
    /// board has, once, with nothing else running on these lines.
    pub fn probe(chip: &str, line: u32) -> io::Result<(bool, bool)> {
        let read = |pull: Pull| -> io::Result<bool> {
            let mut builder = gpiocdev::Request::builder();
            builder
                .on_chip(chip)
                .with_consumer("lampOS-controls-probe")
                .with_line(line)
                .as_input()
                .with_bias(bias(pull));
            let request = builder.request().map_err(io::Error::other)?;
            std::thread::sleep(std::time::Duration::from_millis(10));
            Ok(request.value(line).map_err(io::Error::other)? == gpiocdev::line::Value::Active)
        };
        Ok((read(Pull::Down)?, read(Pull::Up)?))
    }
}

#[cfg(target_os = "linux")]
impl Lines for GpioLines {
    fn high(&mut self, line: u32) -> io::Result<bool> {
        // No active-low flag is asked of the kernel: active is electrical high.
        Ok(self.request.value(line).map_err(io::Error::other)? == gpiocdev::line::Value::Active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Scripted {
        high: BTreeMap<u32, bool>,
        fail: bool,
        reads: u32,
    }
    impl Lines for &mut Scripted {
        fn high(&mut self, line: u32) -> io::Result<bool> {
            self.reads += 1;
            if self.fail {
                return Err(io::Error::other("the chip has gone"));
            }
            Ok(self.high[&line])
        }
    }

    fn lamp() -> (PinsConfig, Config) {
        let pin = |name: &str, line| Pin {
            name: name.into(),
            line,
            active_low: true,
            pull: Pull::Up,
        };
        (
            PinsConfig {
                privacy: None,
                chip: "/dev/gpiochip0".into(),
                buttons: vec![pin("primary", 99)],
                head_pads: vec![pin("head-1", 37), pin("head-2", 96)],
                every_ms: 5,
            },
            Config {
                enabled: false,
                buttons: vec!["primary".into()],
                head_pads: vec!["head-1".into(), "head-2".into()],
                base_selected_mask: None,
            },
        )
    }

    /// Every line together, each as pressed or not by the way it is
    /// wired, every 5 ms and not between, numbered one after another.
    #[test]
    fn the_lines_are_read_together_at_their_pace_and_given_as_pressed_or_not() {
        let (pins, controls) = lamp();
        let mut scripted = Scripted {
            high: [(99, true), (37, true), (96, true)].into(),
            ..Scripted::default()
        };
        let mut reader =
            LinePins::new(&mut scripted, pins, &controls, "boot", "pins-1".into()).unwrap();
        let first = reader.read(1_000_000).unwrap().unwrap();
        first.check().unwrap();
        assert_eq!((first.stamp.at_us, first.stamp.sequence), (1_000_000, 1));
        assert_eq!(first.session_id, "pins-1");
        assert!(
            first
                .buttons
                .iter()
                .chain(&first.head_pads)
                .all(|level| !level.active)
        );
        assert_eq!(first.base_mask, None);
        // Not yet 5 ms on: nothing new, and the lines are not read.
        assert_eq!(reader.read(1_004_999).unwrap(), None);
        // The button is pressed (its line goes low) and one pad touched.
        reader.lines.high.insert(99, false);
        reader.lines.high.insert(96, false);
        let second = reader.read(1_005_000).unwrap().unwrap();
        assert_eq!(second.stamp.sequence, 2);
        let pressed: Vec<(&str, bool)> = second
            .buttons
            .iter()
            .chain(&second.head_pads)
            .map(|level| (level.name.as_str(), level.active))
            .collect();
        assert_eq!(
            pressed,
            [("primary", true), ("head-1", false), ("head-2", true)]
        );
        assert_eq!(reader.lines.reads, 6, "three lines, twice");
    }

    /// A chip that cannot be read is said as missing input, never as
    /// released, and the next good reading goes on from the next number.
    #[test]
    fn a_line_that_cannot_be_read_is_missing_input_and_not_a_release() {
        let (pins, controls) = lamp();
        let mut scripted = Scripted {
            high: [(99, false), (37, true), (96, true)].into(),
            ..Scripted::default()
        };
        let mut reader =
            LinePins::new(&mut scripted, pins, &controls, "boot", "pins-1".into()).unwrap();
        assert!(reader.read(10_000).unwrap().unwrap().buttons[0].active);
        reader.lines.fail = true;
        let error = reader.read(20_000).unwrap_err();
        assert_eq!(error.fault, ControlFault::Missing);
        reader.lines.fail = false;
        let after = reader.read(30_000).unwrap().unwrap();
        assert_eq!(after.stamp.sequence, 2);
        assert!(after.buttons[0].active);
    }

    #[test]
    fn the_wiring_must_name_the_controls_inputs_and_no_line_twice() {
        let (pins, controls) = lamp();
        assert_eq!(pins.check(&controls), Ok(()));
        let mut twice = pins.clone();
        twice.head_pads[1].line = 99;
        assert!(twice.check(&controls).is_err());
        let mut other = pins.clone();
        other.head_pads[0].name = "head-9".into();
        assert!(other.check(&controls).is_err());
        let mut swapped = pins.clone();
        swapped.head_pads.swap(0, 1);
        assert!(swapped.check(&controls).is_err());
        for every_ms in [0, 1, 51] {
            let mut paced = pins.clone();
            paced.every_ms = every_ms;
            assert!(paced.check(&controls).is_err(), "{every_ms}");
        }
        let mut elsewhere = pins.clone();
        elsewhere.chip = "gpiochip0".into();
        assert!(elsewhere.check(&controls).is_err());
        // A file with a field nobody knows is refused, not half read.
        assert!(
            serde_json::from_str::<PinsConfig>(
                r#"{"chip":"/dev/gpiochip0","buttons":[],"head_pads":[],"every_ms":5,"privacy":9}"#
            )
            .is_err()
        );
        let read: PinsConfig = serde_json::from_str(
            r#"{"chip":"/dev/gpiochip0","buttons":[{"name":"primary","line":99,"active_low":true,"pull":"up"}],"head_pads":[],"every_ms":5}"#,
        )
        .unwrap();
        assert_eq!(read.buttons[0].pull, Pull::Up);
    }
}
