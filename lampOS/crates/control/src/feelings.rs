//! `express`'s feelings: the `feelings` skill's own (#3691), so the lamp has
//! one set of feelings (docs/control.md, "Routing"). Each is that skill's
//! `[[do]]`: its move, compiled in from `skills/feelings/moves` (lamp-control's
//! unit reads no skills folder, Nine on #3997), and its light, one step
//! warmer and one dimmer than the person's own, for as long and with the
//! breath the skill gives it. The skill's triggers, waits, caps and quiet
//! hours are the lamp's unprompted life and not carried: a computer asking
//! is held to `express`'s own rate.

use lamp_body::moves::{self, Move};
use lamp_msgs::{Light, LightPattern, PatternKind};
use serde_json::Value;

/// One feeling as the skill shows it.
#[derive(Debug)]
pub struct Feeling {
    pub name: &'static str,
    /// The skill's move file, as it ships.
    move_file: &'static str,
    /// How long its light holds.
    pub lasting_ms: u32,
    /// Its light's breath, if it breathes.
    breathe_ms: Option<u32>,
    /// At the person's dimmest level, the warmth alone while the colour
    /// can still warm (the skill's `warm_at_dimmest`, #3691: Glad and
    /// Lively, not Low).
    warm_at_dimmest: bool,
    /// Its smallest step of [`STEPS`]: `low` goes no smaller than three
    /// quarters, so its lift back keeps the lead a head needs to come up
    /// (jarvis's P3 on #4020: 06c1's pitch lifted only once its goal led
    /// by about 47 counts).
    least_step: u8,
}

/// The skill's three, in its `[[do]]` order (`skills/feelings/skill.toml`).
pub const FEELINGS: [Feeling; 3] = [
    Feeling {
        name: "glad",
        move_file: include_str!("../../../skills/feelings/moves/glad-lift.json"),
        lasting_ms: 12_000,
        breathe_ms: Some(6_000),
        warm_at_dimmest: true,
        least_step: 1,
    },
    Feeling {
        name: "lively",
        move_file: include_str!("../../../skills/feelings/moves/perk.json"),
        lasting_ms: 8_000,
        breathe_ms: Some(4_000),
        warm_at_dimmest: true,
        least_step: 1,
    },
    Feeling {
        name: "low",
        move_file: include_str!("../../../skills/feelings/moves/low-droop.json"),
        lasting_ms: 20_000,
        breathe_ms: None,
        warm_at_dimmest: false,
        least_step: 3,
    },
];

/// The strength a move plays as written at, and above: never bigger than
/// the skill's own move.
pub const WHOLE_AT: f32 = 0.5;

/// A smaller move is a quarter, a half or three quarters of the skill's,
/// each its own name (`glad-lift-50`): the body keeps a sent move by name,
/// and a name sent again with other frames spends one of its 64 changes a
/// run (`lamp_body::moves::MOST_REPLACED`, the Reviewer-lead's P2 on
/// #4020). So the three feelings send at most 12 names, every one fixed.
pub const STEPS: u8 = 4;

/// The evening's warmth, coolest first, as `lamp_skills::glow` steps it.
const WARMTH: [&str; 2] = ["white", "warm"];

/// The feeling of this name, or why not.
pub fn named(name: &str) -> Result<&'static Feeling, String> {
    FEELINGS
        .iter()
        .find(|feeling| feeling.name == name)
        .ok_or_else(|| format!("feeling: one of {}", names()))
}

fn names() -> String {
    FEELINGS
        .iter()
        .map(|feeling| feeling.name)
        .collect::<Vec<_>>()
        .join(", ")
}

impl Feeling {
    /// The skill's move as it ships.
    pub fn written(&self) -> Move {
        serde_json::from_str(self.move_file).expect("the feelings skill's move is a move")
    }

    /// Its move at `strength` (0 to 1): as written from [`WHOLE_AT`] up;
    /// below it the nearest of the [`STEPS`] smaller ones, each count that
    /// share of the skill's and named for it; held to the body's own check
    /// (`lamp_body::moves::check`) as it will be sent. `None` under an
    /// eighth of the skill's (strength under 0.0625), which moves nothing,
    /// and never under the feeling's own least step.
    /// Only the move's own way is made smaller: a joint's counts the other
    /// way, its way back past rest, are kept as the skill wrote them.
    pub fn scaled(&self, strength: f32) -> Result<Option<Move>, String> {
        let steps = f32::from(STEPS);
        let step = ((strength / WHOLE_AT).clamp(0.0, 1.0) * steps).round() as u8;
        if step == 0 {
            return Ok(None);
        }
        let step = step.max(self.least_step);
        let mut scaled = self.written();
        if step == STEPS {
            moves::check(&scaled)?;
            return Ok(Some(scaled));
        }
        let share = f32::from(step) / steps;
        scaled.name = format!(
            "{}-{}",
            scaled.name,
            u16::from(step) * 100 / u16::from(STEPS)
        );
        // The way back past rest stays the skill's own: a weaker `low`
        // still lifts back with the lead a head needs to come up (jarvis's
        // P3 on #4020: 06c1's pitch lifted only once its goal led by 47).
        let peaks: [i16; 5] = std::array::from_fn(|joint| {
            scaled
                .frames
                .iter()
                .map(|frame| frame[joint])
                .max_by_key(|count| count.unsigned_abs())
                .unwrap_or(0)
        });
        for frame in &mut scaled.frames {
            for (joint, count) in frame.iter_mut().enumerate() {
                if count.signum() == peaks[joint].signum() {
                    *count = (f32::from(*count) * share).round() as i16;
                }
            }
        }
        moves::check(&scaled)?;
        Ok(Some(scaled))
    }

    /// Its light against the person's own as the ring says it shows it
    /// (`get`'s `light`, from `light/state`), as `lamp_skills::glow::relative`
    /// makes the skill's: one step warmer and at least one level down, never
    /// up; breathing from the person's level down to that and back, or
    /// steady there; at level 1, Glad and Lively warm a white light alone,
    /// steady (#3691). Or why none: the light off, not known, or at its
    /// dimmest with no warmth to add.
    pub fn light(&self, now: &Value) -> Result<(Light, Option<LightPattern>), &'static str> {
        let (Some(on), Some(colour), Some(level)) = (
            now["on"].as_bool(),
            now["colour"].as_str(),
            now["level"]
                .as_u64()
                .and_then(|level| u8::try_from(level).ok()),
        ) else {
            return Err("the light shown is not known");
        };
        if !on {
            return Err("the light is off");
        }
        let warmer = match WARMTH.iter().position(|shade| *shade == colour) {
            Some(at) => WARMTH[(at + 1).min(WARMTH.len() - 1)],
            None => colour,
        };
        let low = level.saturating_sub(1).max(1);
        if low == level {
            if self.warm_at_dimmest && warmer != colour {
                return Ok((
                    Light {
                        on: true,
                        colour: warmer.to_owned(),
                        level,
                    },
                    None,
                ));
            }
            return Err("the person's light is at its dimmest");
        }
        let colour = warmer;
        let (level, pattern) = match self.breathe_ms {
            Some(period_ms) => (
                level,
                Some(LightPattern {
                    kind: PatternKind::Breathe,
                    period_ms,
                    low,
                }),
            ),
            None => (low, None),
        };
        Ok((
            Light {
                on: true,
                colour: colour.to_owned(),
                level,
            },
            pattern,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    fn skill() -> &'static Path {
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../skills/feelings"
        ))
    }

    /// Nine on #3997: a move edited in the skill can't drift from the
    /// door's copy.
    #[test]
    fn each_compiled_move_is_the_skills_file_and_passes_the_bodys_check() {
        for (feeling, file) in
            FEELINGS
                .iter()
                .zip(["glad-lift.json", "perk.json", "low-droop.json"])
        {
            let on_disk = moves::read(&skill().join("moves").join(file)).unwrap();
            assert_eq!(feeling.written(), on_disk, "{}", feeling.name);
            moves::check(&on_disk).unwrap();
        }
    }

    #[derive(serde::Deserialize)]
    struct SkillFile {
        #[serde(rename = "do")]
        does: Vec<Do>,
    }

    #[derive(serde::Deserialize)]
    struct Do {
        on: String,
        body: toml::Value,
        light: toml::Value,
    }

    fn ms(text: &str) -> u32 {
        text.strip_suffix('s').unwrap().parse::<u32>().unwrap() * 1_000
    }

    /// The table is the skill's `[[do]]`, feeling by feeling: its move, its
    /// light's `for`, its breath, one step warmer and one dimmer, and
    /// whether it warms alone at the dimmest level.
    #[test]
    fn the_table_is_the_skills_do() {
        let text = std::fs::read_to_string(skill().join("skill.toml")).unwrap();
        let file: SkillFile = toml::from_str(&text).unwrap();
        assert_eq!(file.does.len(), FEELINGS.len());
        for (feeling, does) in FEELINGS.iter().zip(&file.does) {
            assert_eq!(feeling.name, does.on);
            assert_eq!(
                Some(feeling.written().name.as_str()),
                does.body["move"].as_str()
            );
            assert_eq!(
                does.light["relative"].as_table().map(|relative| (
                    relative["warmer"].as_integer(),
                    relative["dimmer"].as_integer(),
                    relative
                        .get("warm_at_dimmest")
                        .and_then(toml::Value::as_bool)
                        .unwrap_or(false)
                )),
                Some((Some(1), Some(1), feeling.warm_at_dimmest)),
                "{}",
                feeling.name
            );
            assert_eq!(
                feeling.lasting_ms,
                ms(does.light["for"].as_str().unwrap()),
                "{}",
                feeling.name
            );
            let breath = does
                .light
                .get("pattern")
                .map(|pattern| ms(pattern["period"].as_str().unwrap()));
            assert_eq!(feeling.breathe_ms, breath, "{}", feeling.name);
        }
    }

    /// The light is what lamp-skills makes of the skill's own, for every
    /// light a person may have on (`lamp_skills::glow::relative`).
    #[test]
    fn the_light_is_the_skills_relative_light() {
        for feeling in &FEELINGS {
            let call = lamp_skills::Call::Relative {
                warmer: 1,
                dimmer: 1,
                lasting: None,
                breathe_ms: feeling.breathe_ms,
                warm_at_dimmest: feeling.warm_at_dimmest,
            };
            for colour in Light::COLOURS {
                for level in 1..=Light::BRIGHTEST {
                    for on in [true, false] {
                        let now = Light {
                            on,
                            colour: colour.to_owned(),
                            level,
                        };
                        let theirs = lamp_skills::glow::relative(Some(&now), &call);
                        let ours = feeling.light(&json!(now));
                        match (theirs, ours) {
                            (
                                Ok(lamp_skills::Call::Light {
                                    colour,
                                    level,
                                    pattern,
                                    ..
                                }),
                                Ok((light, ours)),
                            ) => {
                                assert_eq!((colour, level), (light.colour, Some(light.level)));
                                assert_eq!(pattern, ours);
                            }
                            (Err(_), Err(_)) => {}
                            (theirs, ours) => {
                                panic!("{} at {now:?}: {theirs:?} and {ours:?}", feeling.name)
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(
            FEELINGS[0].light(&Value::Null).unwrap_err(),
            "the light shown is not known"
        );
    }

    /// jarvis's P3 on #4020: a droop that lifts back past rest keeps that
    /// lift whole when the droop is smaller.
    #[test]
    fn a_weaker_move_keeps_its_way_back_past_rest_whole() {
        let droop = Feeling {
            name: "low",
            move_file: r#"{"name":"low-droop","step_ms":300,"frames":[[0,0,0,0,0],[20,0,0,0,0],[40,0,0,0,0],[40,0,0,0,0],[20,0,0,0,0],[-25,0,0,0,0],[0,0,0,0,0]]}"#,
            lasting_ms: 20_000,
            breathe_ms: None,
            warm_at_dimmest: false,
            least_step: 1,
        };
        let weak = droop.scaled(0.125).unwrap().unwrap();
        let pitch: Vec<i16> = weak.frames.iter().map(|frame| frame[0]).collect();
        assert_eq!(pitch, [0, 5, 10, 10, 5, -25, 0]);
    }

    #[test]
    fn a_weaker_feeling_is_a_smaller_move_never_a_bigger_one() {
        for feeling in &FEELINGS {
            let written = feeling.written();
            for strength in [0.5, 0.75, 1.0] {
                assert_eq!(feeling.scaled(strength).unwrap(), Some(written.clone()));
            }
            // How far it goes its own way: the counts with the sign of its
            // largest, so a lift back past rest, kept whole, isn't counted
            // (jarvis's P2 on #4020, with #3994's `low-droop`).
            let way = written
                .frames
                .iter()
                .flatten()
                .copied()
                .max_by_key(|count| count.unsigned_abs())
                .unwrap()
                .signum();
            let peak = |frames: &[[i16; 5]]| {
                frames
                    .iter()
                    .flatten()
                    .filter(|count| count.signum() == way)
                    .map(|count| count.unsigned_abs())
                    .max()
                    .unwrap()
            };
            let mut last = (STEPS, peak(&written.frames));
            for (strength, step) in [(0.4, 3), (0.25, 2), (0.1, 1)] {
                let step = u8::max(step, feeling.least_step);
                let scaled = feeling.scaled(strength).unwrap().unwrap();
                if step == STEPS {
                    assert_eq!(scaled, written);
                    continue;
                }
                assert_eq!(
                    scaled.name,
                    format!("{}-{}", written.name, u16::from(step) * 25)
                );
                let now = peak(&scaled.frames);
                if step < last.0 {
                    assert!(now < last.1, "{} at {strength}: {now}", feeling.name);
                } else {
                    assert_eq!(now, last.1, "{} at {strength}", feeling.name);
                }
                assert!(now > 0);
                last = (step, now);
            }
            // Each step is one fixed move: strengths that round alike send
            // the same name and frames, so the body never replaces it.
            assert_eq!(feeling.scaled(0.26).unwrap(), feeling.scaled(0.24).unwrap());
            for strength in [0.0, 0.06] {
                assert_eq!(feeling.scaled(strength).unwrap(), None, "{strength}");
            }
        }
        // `low` goes no smaller than three quarters (jarvis's P3 on #4020).
        let low = named("low").unwrap();
        assert_eq!(low.scaled(0.1).unwrap().unwrap().name, "low-droop-75");
        assert_eq!(named("glad").unwrap().name, "glad");
        assert_eq!(
            named("happy").unwrap_err(),
            "feeling: one of glad, lively, low"
        );
    }
}
