//! The startle (#2822): one glance at a sudden sound (`ears/sudden`). Its
//! move is the startle skill's own (`skills/startle/skill.toml`,
//! `[startle]`), read once when Expression starts; Expression still plays
//! it as a reflex, within its own tick, because a startle answers within
//! 200 ms or not at all. A catalogue's own sudden clip is never played
//! ([`crate::Catalogue::from_json_naming`] drops it): there is no other
//! startle than the skill's.
use crate::{Behaviour, Error, Keyframe, Trigger};
use lamp_msgs::Voice;
use serde::Deserialize;
use std::{fs::File, io::Read, path::Path};

/// The clip's name, as the engine reports it.
pub const NAME: &str = "glance at a sound";
/// Where the product installs the skill's file (`lamp-update`'s settings).
pub const INSTALLED: &str = "/var/lib/lampOS/skills/startle/skill.toml";
/// The most a skill's file may be.
pub const MOST_FILE_BYTES: u64 = 64 * 1024;

/// The skill's `[startle]`: the clip, but for what a startle always is (a
/// `sudden` trigger, never repeated, held or carried into another state).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Startle {
    pub weight: f64,
    pub delay_us: u64,
    pub cooldown_us: u64,
    pub stroke_us: u64,
    pub quiet_only: bool,
    pub stop_when_concentrating: bool,
    pub states: Vec<Voice>,
    pub keyframes: Vec<Keyframe>,
}

impl Startle {
    /// The clip Expression plays, held to the bounds every sudden clip is.
    pub fn checked(&self) -> Result<Behaviour, Error> {
        let clip = Behaviour {
            name: NAME.into(),
            trigger: Trigger::Sudden,
            weight: self.weight,
            delay_us: self.delay_us,
            cooldown_us: self.cooldown_us,
            stroke_us: self.stroke_us,
            repeat: false,
            hold: false,
            quiet_only: self.quiet_only,
            stop_when_concentrating: self.stop_when_concentrating,
            states: self.states.clone(),
            continue_in: Vec::new(),
            keyframes: self.keyframes.clone(),
        };
        clip.check()?;
        Ok(clip)
    }
}

/// The skill file's other tables are lamp-skills'; only `[startle]` is read.
#[derive(Deserialize)]
struct SkillFile {
    startle: Option<Startle>,
}

/// The startle clip from a skill file's text.
pub fn parse(text: &str) -> Result<Behaviour, Error> {
    let file: SkillFile =
        toml::from_str(text).map_err(|error| Error(format!("startle skill: {error}")))?;
    file.startle
        .ok_or_else(|| Error("startle skill: no [startle]".into()))?
        .checked()
}

/// The startle clip from the skill file at `path`, read within a bound.
pub fn read(path: &Path) -> Result<Behaviour, Error> {
    let mut text = String::new();
    File::open(path)
        .and_then(|file| file.take(MOST_FILE_BYTES + 1).read_to_string(&mut text))
        .map_err(|error| Error(format!("{}: {error}", path.display())))?;
    if text.len() as u64 > MOST_FILE_BYTES {
        return Err(Error(format!("{}: over 64 KiB", path.display())));
    }
    parse(&text)
}

/// The glance at a remark's name, as the engine reports it (#4175).
pub const REMARK_NAME: &str = "glance at a remark";
/// A room that keeps talking about the lamp gets one glance a minute.
pub const REMARK_COOLDOWN_US: u64 = 60_000_000;
/// The glance starts within this of the remark, or not at all: a late one
/// reads as a machine catching up, not a creature that heard its name.
pub const REMARK_STROKE_US: u64 = 1_500_000;

/// The glance at a remark (#4175): the startle's own move, once, when in a
/// busy room someone said something about the lamp and the voice said
/// nothing (`voice/remarked`). Never while the lamp thinks or speaks, and
/// never shares the startle's cooldown. Only with `LAMP_REMARK_GLANCE=on`.
pub fn remark(startle: &Behaviour) -> Result<Behaviour, Error> {
    let clip = Behaviour {
        name: REMARK_NAME.into(),
        trigger: Trigger::Remarked,
        weight: startle.weight,
        delay_us: 0,
        cooldown_us: REMARK_COOLDOWN_US,
        stroke_us: REMARK_STROKE_US,
        repeat: false,
        hold: false,
        quiet_only: false,
        stop_when_concentrating: true,
        states: vec![Voice::Resting, Voice::Listening],
        continue_in: Vec::new(),
        keyframes: startle.keyframes.clone(),
    };
    clip.check()?;
    Ok(clip)
}
