//! How lively the body moves, one level from 1 to 5 (#6893). Level 1 is the
//! lamp as it is today, exactly: the unit's own `--speed` and
//! `--stand-seconds`, the idle waits of `crate::idle`. Levels 2 to 5 only go
//! livelier, never under today, and only inside the caps fixed in code
//! (`Tuning::SPEED_CAP`, the strain, tip and reach bounds, all untouched).
//!
//! Applied: the pose's speed and acceleration, the stand and fold pace, the
//! idle waits (`Limits::at`, `crate::idle`), the head's roll standing
//! (`Limits::at`), the listening lean (`crate::body::mood_lean`) and a spoken
//! nod's size (`crate::body`).
//! How often gesture clips play per level lives outside the body (the voice,
//! Expression, the music skill) and comes in its own part.

/// One level's movement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Energy {
    /// The pose's top speed, counts a second; its acceleration is six times it.
    pub speed: u16,
    /// Seconds a stand or a fold takes.
    pub stand_seconds: f32,
    /// The head's roll reach standing, counts; 0 is none (the unit's reach).
    /// Only for a body with a stand.
    pub stand_roll: u16,
    /// The shoulder and the elbow leant toward a listener, counts.
    pub listen_lean: [i16; 2],
    /// How much a spoken nod is scaled, before Expression's reach cuts it.
    pub nod_share: f32,
    /// How much each wait between shifts at rest is scaled.
    pub idle_share: f64,
}

/// Levels 1 to 5. Level 1 says the product unit's values; the body applies
/// level 1 as whatever the unit gave it.
pub const TABLE: [Energy; 5] = [
    Energy {
        speed: 300,
        stand_seconds: 6.0,
        stand_roll: 0,
        listen_lean: [0, 0],
        nod_share: 1.0,
        idle_share: 1.0,
    },
    Energy {
        speed: 425,
        stand_seconds: 5.5,
        stand_roll: 0,
        listen_lean: [0, 0],
        nod_share: 1.1,
        idle_share: 0.85,
    },
    Energy {
        speed: 550,
        stand_seconds: 5.0,
        stand_roll: 120,
        listen_lean: [-10, -15],
        nod_share: 1.2,
        idle_share: 0.7,
    },
    Energy {
        speed: 675,
        stand_seconds: 4.5,
        stand_roll: 160,
        listen_lean: [-20, -25],
        nod_share: 1.3,
        idle_share: 0.6,
    },
    Energy {
        speed: 800,
        stand_seconds: 4.0,
        stand_roll: 200,
        listen_lean: [-25, -30],
        nod_share: 1.4,
        idle_share: 0.5,
    },
];

/// A level as the body takes it: 0 or anything past 5 is level 1.
pub fn taken(said: u8) -> u8 {
    if (1..=5).contains(&said) { said } else { 1 }
}

/// The row of a level.
pub fn of(level: u8) -> &'static Energy {
    &TABLE[usize::from(taken(level)) - 1]
}

/// The pose's speed at `level` for a body given `base`: level 1 is `base`,
/// above it never slower than `base`.
pub fn speed(base: f32, level: u8) -> f32 {
    if taken(level) == 1 {
        base
    } else {
        base.max(f32::from(of(level).speed))
    }
}

/// The stand's seconds at `level` for a body given `base`: level 1 is
/// `base`, above it never slower than `base`.
pub fn stand_seconds(base: f32, level: u8) -> f32 {
    if taken(level) == 1 {
        base
    } else {
        base.min(of(level).stand_seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The value after `flag` on the product unit's command line.
    fn unit_value(flag: &str) -> f32 {
        let unit = include_str!("../../../deploy/product/lamp-body.service");
        let line = unit
            .lines()
            .find(|line| line.starts_with("ExecStart="))
            .unwrap();
        let mut words = line.split_whitespace();
        words.find(|word| *word == flag).unwrap();
        words.next().unwrap().parse().unwrap()
    }

    /// Level 1 is what the product unit runs today, and is applied as the
    /// base whatever it is.
    #[test]
    fn level_one_is_the_unit_today() {
        assert_eq!(f32::from(TABLE[0].speed), unit_value("--speed"));
        assert_eq!(TABLE[0].stand_seconds, unit_value("--stand-seconds"));
        assert_eq!(TABLE[0].idle_share, 1.0);
        for base in [50.0, 120.0, 300.0] {
            assert_eq!(speed(base, 1), base);
            assert_eq!(stand_seconds(base / 10.0, 1), base / 10.0);
        }
        for said in [0, 6, 255] {
            assert_eq!(taken(said), 1);
            assert_eq!(speed(300.0, said), 300.0);
        }
    }

    /// Each level is livelier than the one under it, never under the base.
    #[test]
    fn levels_only_go_livelier() {
        for pair in TABLE.windows(2) {
            assert!(pair[1].speed > pair[0].speed);
            assert!(pair[1].stand_seconds < pair[0].stand_seconds);
            assert!(pair[1].idle_share < pair[0].idle_share);
            assert!(pair[1].nod_share > pair[0].nod_share);
            assert!(pair[1].stand_roll >= pair[0].stand_roll);
        }
        assert_eq!(speed(300.0, 5), 800.0);
        assert_eq!(stand_seconds(6.0, 5), 4.0);
        // A unit already livelier than a level keeps its own.
        assert_eq!(speed(500.0, 2), 500.0);
        assert_eq!(stand_seconds(3.0, 5), 3.0);
    }
}
