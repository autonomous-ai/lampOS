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
//! How often clips start of themselves per level is [`GESTURE`]: read by the
//! voice (a small fill on an answer with no gesture of its own), Expression
//! (the pause nod's gap) and lamp-skills (a `lively` trigger's cooldown and
//! rearm, the music sway's). The body drops such a new clip while
//! lamp-control holds it (`crate::body::Asked::lively`).

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

/// One level's rate of clips started of themselves (#6893).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gesture {
    /// The share of answers with no gesture of their own (`cue_for` gives
    /// none) that get a small fill: a nod, an acknowledge or a curious.
    pub fill: f32,
    /// The least seconds between two fills.
    pub fill_gap_s: u32,
    /// The least seconds between two of Expression's pause nods.
    pub pause_gap_s: f32,
    /// A `lively` skill trigger's cooldown, minutes (music sway's).
    pub music_cooldown_min: u32,
    /// Whether a `lively` trigger keeps its `rearm`.
    pub music_rearm: bool,
}

/// Levels 1 to 5. Level 1 is today: no fill, the catalogue's 4 s pause gap,
/// music sway's own 30 minutes and its rearm, applied as the files say.
pub const GESTURE: [Gesture; 5] = [
    Gesture {
        fill: 0.0,
        fill_gap_s: 0,
        pause_gap_s: 4.0,
        music_cooldown_min: 30,
        music_rearm: true,
    },
    Gesture {
        fill: 0.10,
        fill_gap_s: 60,
        pause_gap_s: 3.75,
        music_cooldown_min: 20,
        music_rearm: true,
    },
    Gesture {
        fill: 0.20,
        fill_gap_s: 45,
        pause_gap_s: 3.5,
        music_cooldown_min: 15,
        music_rearm: true,
    },
    // A playlist never stays under the rearm line for 5 minutes: with it,
    // the lamp would sway once ever.
    Gesture {
        fill: 0.30,
        fill_gap_s: 30,
        pause_gap_s: 3.25,
        music_cooldown_min: 10,
        music_rearm: false,
    },
    Gesture {
        fill: 0.40,
        fill_gap_s: 20,
        pause_gap_s: 3.0,
        music_cooldown_min: 6,
        music_rearm: false,
    },
];

/// The rate row of a level.
pub fn gesture(level: u8) -> &'static Gesture {
    &GESTURE[usize::from(taken(level)) - 1]
}

/// The pause nod's gap at `level` for a catalogue's `cooldown_us`: level 1
/// is the catalogue's, above it never longer than it.
pub fn pause_cooldown_us(cooldown_us: u64, level: u8) -> u64 {
    if taken(level) == 1 {
        cooldown_us
    } else {
        cooldown_us.min((f64::from(gesture(level).pause_gap_s) * 1e6) as u64)
    }
}

/// A `lively` trigger's cooldown at `level` for its file's `cooldown_us`:
/// level 1 is the file's, above it never longer than it.
pub fn lively_cooldown_us(cooldown_us: u64, level: u8) -> u64 {
    if taken(level) == 1 {
        cooldown_us
    } else {
        cooldown_us.min(u64::from(gesture(level).music_cooldown_min) * 60_000_000)
    }
}

/// Whether a `lively` trigger keeps the `rearm` its file gives at `level`.
pub fn lively_rearm(rearm: bool, level: u8) -> bool {
    rearm && gesture(level).music_rearm
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

    /// Level 1 starts nothing new and keeps every file's own numbers; the
    /// pause gap never goes under 3 s, nor a level's rate under the one below.
    #[test]
    fn level_one_starts_nothing_new_and_five_is_the_most() {
        assert_eq!(gesture(1).fill, 0.0);
        for said in [0, 1, 6] {
            assert_eq!(pause_cooldown_us(4_000_000, said), 4_000_000);
            assert_eq!(lively_cooldown_us(1_800_000_000, said), 1_800_000_000);
            assert!(lively_rearm(true, said));
            assert!(!lively_rearm(false, said));
        }
        assert_eq!(pause_cooldown_us(4_000_000, 5), 3_000_000);
        assert_eq!(lively_cooldown_us(1_800_000_000, 5), 360_000_000);
        assert!(!lively_rearm(true, 5));
        // A file already quicker keeps its own.
        assert_eq!(pause_cooldown_us(2_000_000, 5), 2_000_000);
        assert_eq!(lively_cooldown_us(60_000_000, 5), 60_000_000);
        for pair in GESTURE.windows(2) {
            assert!(pair[1].fill > pair[0].fill);
            assert!(pair[1].pause_gap_s < pair[0].pause_gap_s);
            assert!(pair[1].music_cooldown_min < pair[0].music_cooldown_min);
        }
        assert!(GESTURE.iter().all(|row| row.pause_gap_s >= 3.0));
        assert!(GESTURE[1..].iter().all(|row| row.fill_gap_s >= 20));
    }
}
