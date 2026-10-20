//! Explicit Orange Pi wiring for the touch path, the product's, in a schema
//! of its own: the legacy pins reader keeps refusing unknown fields at its
//! top. This one reads a newer build's file one release forward
//! ([`crate::forward`], #1680).
use crate::{
    engine::Config,
    input::{InputError, Observation, PinReader},
    mpr121,
    pins::{Pin, PinsConfig, Pull},
    touch::{GpioSample, TouchConfig},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, Read},
    path::Path,
};

pub const HEAD_CANDIDATES: [u32; 4] = [37, 96, 97, 98];

/// A head pad's line at rest, as this lamp's factory step measured it with
/// hands away.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rest {
    High,
    Low,
}

/// How a touch shows on a head pad's line: held away from its rest while
/// the hand is there, or pulsing while it rests (a TTP223 in its pulsing
/// mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reads {
    Level,
    Pulses,
}

/// One head pad's own baseline (#711): each unit's pads rest and read
/// differently (#1764), so each lamp's factory step writes its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadRest {
    pub name: String,
    pub rest: Rest,
    pub reads: Reads,
}

/// The switch that turns the base strip (MPR121) back on: `on`. Without it
/// the strip is off: V2 doesn't use it (the project lead's decision, 19 Oct;
/// V1 had it off too). Its code and tuning are kept: a pins file's `base`
/// section and the config's `base_selected_mask` stay as written, unread.
pub const STRIP_ENV: &str = "LAMP_CONTROLS_STRIP";

/// What `lamp-controls serve` serves with: the strip set aside unless
/// `switch`, the value of [`STRIP_ENV`], is `on`, so no strip worker starts
/// and the touch state has no strip source; the head pads, the button and
/// the reset are as before. The pins, the config, and whether a configured
/// strip was set aside.
pub fn serving(
    mut wiring: TouchPinsConfig,
    mut config: Config,
    switch: Option<&str>,
) -> (TouchPinsConfig, Config, bool) {
    if switch == Some("on") {
        return (wiring, config, false);
    }
    let set_aside = wiring.base.take().is_some();
    config.base_selected_mask = None;
    (wiring, config, set_aside)
}

/// The product's pins file. A field a newer build writes is read past and
/// named once ([`crate::forward`]), except one about privacy; the `privacy`
/// section itself stays strict.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TouchPinsConfig {
    pub chip: String,
    pub buttons: Vec<Pin>,
    pub head_pads: Vec<Pin>,
    pub every_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<Pin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<mpr121::Config>,
    /// The head's timing (#711); absent values keep their defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_qualify_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_tap_min_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_gap_ms: Option<u64>,
    /// Each head pad's baseline, in `head_pads`' order, from the factory
    /// step; without it each pad reads its level by its `active_low`, as
    /// before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_rest: Option<Vec<HeadRest>>,
    /// Also give the head pads to the old engine, whose `Pet` on
    /// `controls/gesture` wakes a lamp asleep at night: for a lamp whose
    /// voice and Expression do not read `controls/touch` yet, the product's
    /// (the reviewer on #1167). Nothing else reads it: the voice only counts
    /// pets for its trace, and nothing reads `head_active`. Only beside
    /// neither `LAMP_HEAD_TOUCH=on` nor Expression's `--head-touch`, which
    /// read the head from `controls/touch`; and `--head-touch` only with the
    /// voice's sleep mute, or a hand held 3 s puts the face to sleep with
    /// the microphones open (the reviewer on #1176). Install's tests hold
    /// the product to both. With no head pads it is ignored, and the journal
    /// says so.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub legacy_head: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy: Option<crate::privacy::Pins>,
}

impl TouchPinsConfig {
    /// The pins file at `path`, the fields this build does not read named
    /// once in the journal.
    pub fn load(path: &Path) -> io::Result<Self> {
        let (wiring, unread) = Self::read(path)?;
        if let Some(line) = unread {
            eprintln!("lamp-controls: {line}");
        }
        Ok(wiring)
    }

    /// The pins file at `path`, and the line naming the fields this build
    /// does not read, if any ([`crate::forward::read`]).
    pub fn read(path: &Path) -> io::Result<(Self, Option<String>)> {
        let mut bytes = Vec::new();
        File::open(path)?.take(16_385).read_to_end(&mut bytes)?;
        if bytes.len() > 16_384 {
            return Err(io::Error::other("touch pins file exceeds 16 KiB"));
        }
        crate::forward::read(path, &bytes)
    }

    pub fn check(&self, config: &Config) -> Result<TouchConfig, String> {
        config.check().map_err(|e| e.to_string())?;
        if self.chip != "/dev/gpiochip0" {
            return Err(
                "touch hand wiring describes Orange Pi chip 0; privacy has its own chip-1 section"
                    .into(),
            );
        }
        if let Some(privacy) = &self.privacy {
            privacy.check()?;
        }
        if !(2..=50).contains(&self.every_ms) {
            return Err("GPIO acquisition interval must be 2..50 ms".into());
        }
        if self.buttons.len() > 1 || self.buttons.iter().any(|pin| pin.line != 99) {
            return Err("only the optional primary button may be in buttons (chip 0 line 99); reset line 100 has its own field".into());
        }
        if self
            .reset
            .as_ref()
            .is_some_and(|pin| pin.line != 100 || !pin.active_low || pin.pull != Pull::Up)
        {
            return Err(
                "the optional reset is active-low chip 0 line 100, pulled up, reset only".into(),
            );
        }
        if self
            .head_pads
            .iter()
            .any(|pin| !HEAD_CANDIDATES.contains(&pin.line))
        {
            return Err(
                "head pads must be explicitly qualified per-unit candidates 37, 96, 97 or 98"
                    .into(),
            );
        }
        let mut names = BTreeSet::new();
        let mut lines = BTreeSet::new();
        for pin in self
            .buttons
            .iter()
            .chain(&self.head_pads)
            .chain(self.reset.iter())
        {
            crate::input::check_name(&pin.name).map_err(|e| e.to_string())?;
            if !names.insert(&pin.name) || !lines.insert(pin.line) {
                return Err("touch input names and GPIO lines must be unique".into());
            }
        }
        // The head pads are the pins file's: a lamp's own settings file
        // naming two and the product's pins naming four (#1798), or the
        // reverse, still start, as long as each pad both name is named
        // alike and in the same place. Controls refusing to start would
        // close the microphones on the privacy route.
        if self
            .buttons
            .iter()
            .map(|pin| &pin.name)
            .ne(config.buttons.iter())
            || self
                .head_pads
                .iter()
                .zip(&config.head_pads)
                .any(|(pin, name)| pin.name != *name)
        {
            return Err(
                "touch pins must name configured primary/head inputs in the same order".into(),
            );
        }
        if let Some(rests) = &self.head_rest {
            if rests
                .iter()
                .map(|rest| &rest.name)
                .ne(self.head_pads.iter().map(|pin| &pin.name))
            {
                return Err("head_rest must name the head pads in the same order".into());
            }
            if rests
                .iter()
                .zip(&self.head_pads)
                .any(|(rest, pin)| pin.active_low != (rest.rest == Rest::High))
            {
                return Err(
                    "a head pad resting high is active-low, and one resting low is not".into(),
                );
            }
        }
        if let Some(base) = &self.base {
            base.check()?;
        }
        let selected_mask = self
            .base
            .as_ref()
            .map(|base| base.pads.iter().fold(0_u16, |mask, &pad| mask | (1 << pad)));
        if config.base_selected_mask != selected_mask {
            return Err("touch base_selected_mask must exactly match the profile's working electrodes, or both must be absent".into());
        }
        let touch = TouchConfig {
            enabled: config.enabled,
            primary: self.buttons.first().map(|pin| pin.name.clone()),
            reset: self.reset.as_ref().map(|pin| pin.name.clone()),
            head_pads: self.head_pads.iter().map(|pin| pin.name.clone()).collect(),
            strip_pads: self.base.as_ref().map(|base| base.pads.clone()),
            head_qualify_ms: self.head_qualify_ms,
            head_tap_min_ms: self.head_tap_min_ms,
            head_gap_ms: self.head_gap_ms,
            head_pulse_pads: self
                .head_rest
                .iter()
                .flatten()
                .filter(|rest| rest.reads == Reads::Pulses)
                .map(|rest| rest.name.clone())
                .collect(),
        };
        touch.check().map_err(|e| e.to_string())?;
        Ok(touch)
    }

    /// Reuse the tested GPIO acquisition adapter, without inserting cached
    /// strip readings into its complete GPIO observations. Reset gets a
    /// separate semantic role after acquisition, never a legacy button role.
    pub fn gpio_config(&self) -> Option<(PinsConfig, Config)> {
        if self.buttons.is_empty() && self.head_pads.is_empty() && self.reset.is_none() {
            return None;
        }
        let mut buttons = self.buttons.clone();
        buttons.extend(self.reset.clone());
        let controls = Config {
            enabled: false,
            buttons: buttons.iter().map(|pin| pin.name.clone()).collect(),
            head_pads: self.head_pads.iter().map(|pin| pin.name.clone()).collect(),
            base_selected_mask: None,
        };
        Some((
            PinsConfig {
                privacy: None,
                chip: self.chip.clone(),
                buttons,
                head_pads: self.head_pads.clone(),
                every_ms: self.every_ms,
            },
            controls,
        ))
    }

    pub fn gpio_sample(&self, observation: Observation) -> Result<GpioSample, InputError> {
        observation.check()?;
        let value = |pin: &Option<&Pin>| -> Result<Option<bool>, InputError> {
            pin.map(|pin| {
                observation
                    .buttons
                    .iter()
                    .find(|level| level.name == pin.name)
                    .map(|level| level.active)
                    .ok_or_else(|| {
                        InputError::new(
                            lamp_msgs::ControlFault::Missing,
                            "configured GPIO absent from acquisition",
                        )
                    })
            })
            .transpose()
        };
        let expected = self.buttons.len() + usize::from(self.reset.is_some());
        if observation.buttons.len() != expected || observation.base_mask.is_some() {
            return Err(InputError::new(
                lamp_msgs::ControlFault::Invalid,
                "GPIO observation has unexpected inputs",
            ));
        }
        Ok(GpioSample {
            primary: value(&self.buttons.first())?,
            reset: value(&self.reset.as_ref())?,
            stamp: observation.stamp,
            session_id: observation.session_id,
            head_pads: observation.head_pads,
        })
    }

    /// A privacy-only profile uses the Controls state owner, without inventing
    /// a touch source or publishing an empty TouchState.
    pub fn privacy_only(&self, config: &Config) -> Result<bool, String> {
        if !self.buttons.is_empty()
            || !self.head_pads.is_empty()
            || self.reset.is_some()
            || self.base.is_some()
        {
            return Ok(false);
        }
        config.check().map_err(|error| error.to_string())?;
        if self.chip != "/dev/gpiochip0"
            || !(2..=50).contains(&self.every_ms)
            || !config.buttons.is_empty()
            || !config.head_pads.is_empty()
            || config.base_selected_mask.is_some()
        {
            return Err("privacy-only profile must have no configured hand inputs".into());
        }
        self.privacy
            .as_ref()
            .ok_or("privacy-only profile needs explicit privacy wiring")?
            .check()?;
        Ok(true)
    }
}

/// The item health shows for the head pads' lines (#1798).
pub const HEAD_ITEM: &str = "head pads";

/// What one probe at Controls' start found on each head pad's line
/// (#1798): a pad drives its line, so it reads the same with a pull-down and
/// a pull-up; a line with nothing on it follows the pull
/// ([`crate::pins::GpioLines::probe`], as `discover-head` and `probe` read
/// them). Pins naming lines with no pad, as lamp-52e6's 37 and 96 were,
/// read no touch and said nothing; this says so.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeadLines {
    /// The lines a pad drives, in the pins' order.
    pub wired: Vec<u32>,
    /// The lines that follow the pull: nothing on them.
    pub bare: Vec<u32>,
    /// The lines that couldn't be probed (held by another program).
    pub unprobed: Vec<u32>,
}

impl HeadLines {
    /// Each pad as `probe` reads its line: high with a pull-down, high with
    /// a pull-up ([`crate::pins::GpioLines::probe_ending`], ending on the
    /// pad's own pull).
    pub fn probed(pads: &[Pin], mut probe: impl FnMut(&Pin) -> io::Result<(bool, bool)>) -> Self {
        let mut lines = Self::default();
        for pad in pads {
            match probe(pad) {
                Ok((down, up)) if down == up => lines.wired.push(pad.line),
                Ok(_) => lines.bare.push(pad.line),
                Err(_) => lines.unprobed.push(pad.line),
            }
        }
        lines
    }

    /// The journal's line at start, and health's message while no pad is
    /// left out.
    pub fn said(&self) -> String {
        let mut parts = Vec::new();
        if self.wired.is_empty() {
            if !self.bare.is_empty() {
                parts.push(format!(
                    "nothing wired on {}: the head reads no touch",
                    named(&self.bare)
                ));
            }
        } else {
            parts.push(format!("wired on {}", named(&self.wired)));
            if !self.bare.is_empty() {
                parts.push(format!("nothing on {}", list(&self.bare, ", ")));
            }
        }
        if !self.unprobed.is_empty() {
            parts.push(format!("{} not probed", named(&self.unprobed)));
        }
        parts.join("; ")
    }

    /// What health says ([`HEAD_ITEM`]): well while a pad drives one of
    /// the lines; a warning when none does, or none could be told, or a pad
    /// has read touched since the head was first read and is left out
    /// (`left_out`, its lines; [`crate::touch::TouchEngine::head_left_out`]).
    pub fn health(&self, left_out: &[u32]) -> lamp_msgs::Diagnostic {
        let mut message = self.said();
        if !left_out.is_empty() {
            // Since the head's lines were first read: Controls' start, or the
            // reader's return after a fault.
            message = format!(
                "{message}; {} touched since the head was first read: left out until let go",
                named(left_out)
            );
        }
        let level = if self.wired.is_empty() || !left_out.is_empty() {
            lamp_msgs::Level::Warn
        } else {
            lamp_msgs::Level::Ok
        };
        lamp_msgs::Diagnostic::new(level, HEAD_ITEM, &message)
            .with("wired", list(&self.wired, ","))
            .with("bare", list(&self.bare, ","))
            .with("unprobed", list(&self.unprobed, ","))
            .with("left_out", list(left_out, ","))
    }
}

/// `lines`, joined by `between`.
fn list(lines: &[u32], between: &str) -> String {
    lines
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(between)
}

/// "line 97" or "lines 97, 98".
fn named(lines: &[u32]) -> String {
    let word = if lines.len() == 1 { "line" } else { "lines" };
    format!("{word} {}", list(lines, ", "))
}

/// No configured GPIO is absence, not a failed optional strip or a made-up
/// released input. The touch state contains only explicitly configured sources.
pub struct NoGpio;
impl PinReader for NoGpio {
    fn read(&mut self, _: u64) -> Result<Option<Observation>, InputError> {
        Ok(None)
    }
}

/// An optional GPIO acquisition that could not start. Report its fault once;
/// later polls cannot turn it into a fresh release or stop the strip owner.
pub struct FailedGpio(Option<InputError>);
impl FailedGpio {
    pub fn new(message: impl Into<String>) -> Self {
        Self(Some(InputError::new(
            lamp_msgs::ControlFault::Missing,
            message,
        )))
    }
}
impl PinReader for FailedGpio {
    fn read(&mut self, _: u64) -> Result<Option<Observation>, InputError> {
        match self.0.take() {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }
}
