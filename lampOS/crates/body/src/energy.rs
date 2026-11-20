//! How lively the body moves, one level from 1 to 5 (#6893). Level 1 is the
//! lamp as it is today, exactly: the unit's own `--speed` and
//! `--stand-seconds`, the idle waits of `crate::idle`. Levels 2 to 5 only go
//! livelier, never under today, and only inside the caps fixed in code
//! (`Tuning::ENERGY_SPEED_CAP`, level 5's speed; `--speed` keeps
//! `Tuning::SPEED_CAP`, 300; the strain, tip and reach bounds, all untouched).
//!
//! Applied: the pose's speed and acceleration, the stand and fold pace, the
//! idle waits (`Limits::at`, `crate::idle`), the head's roll standing
//! (`Limits::at`), the listening lean (`crate::body::mood_lean`) and a spoken
//! nod's size (`crate::body`), a built-in clip's size and pace ([`livelier`],
//! #8153); under `LAMP_BODY_PRESENCE` (#7301) also the
//! size of a shift's arm sag (`idle_size`), and for a few minutes after an
//! arrival or talk the shifts' waits and size are a level or two livelier
//! than the owner's (`crate::body::LIVELY_FOR`), that switch's own, so with
//! this one off (level 1) those minutes still come.
//! How often clips start of themselves per level is [`GESTURE`]: read by the
//! voice (a small fill on an answer with no gesture of its own), Expression
//! (the pause nod's gap) and lamp-skills (a `lively` trigger's cooldown and
//! rearm, the music sway's). The body drops such a new clip while
//! lamp-control holds it (`crate::body::Asked::lively`).

use crate::clips::Clip;
use std::sync::OnceLock;

/// One level's movement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Energy {
    /// The pose's top speed, counts a second; its acceleration is six times it.
    pub speed: u16,
    /// Seconds a stand or a fold takes.
    pub stand_seconds: f32,
    /// The head's roll reach standing, counts; 0 is none (the unit's reach).
    /// Only for a body with a stand. Listening tilts 0.85 of it to a side
    /// that alternates (`gesture::LISTENING_ROLL`), so a standing head
    /// swings 2 x 0.85 x this, peak to peak: level 3's sits midway between
    /// levels 1 and 5 as measured (#7314).
    pub stand_roll: u16,
    /// The shoulder and the elbow leant toward a listener, counts.
    pub listen_lean: [i16; 2],
    /// How much a spoken nod is scaled, before Expression's reach cuts it.
    pub nod_share: f32,
    /// How much each wait between shifts at rest is scaled.
    pub idle_share: f64,
    /// How much a shift at rest's arm offsets are scaled under
    /// `LAMP_BODY_PRESENCE` (#7301, `crate::idle::sag`); the head's never.
    pub idle_size: f32,
    /// How much a built-in clip asked of the body is scaled, joint by joint,
    /// never past the gesture's reach ([`livelier`], #8153).
    pub clip_size: f32,
    /// How much quicker a built-in clip's frames come ([`livelier`]); each
    /// step is still stretched under the servo's speed at play time.
    pub clip_pace: f32,
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
        idle_size: 1.0,
        clip_size: 1.0,
        clip_pace: 1.0,
    },
    Energy {
        speed: 425,
        stand_seconds: 5.5,
        stand_roll: 0,
        listen_lean: [0, 0],
        nod_share: 1.1,
        idle_share: 0.85,
        idle_size: 1.05,
        clip_size: 1.15,
        clip_pace: 1.125,
    },
    Energy {
        speed: 550,
        stand_seconds: 5.0,
        stand_roll: 106,
        listen_lean: [-10, -15],
        nod_share: 1.2,
        idle_share: 0.7,
        idle_size: 1.1,
        clip_size: 1.3,
        clip_pace: 1.25,
    },
    Energy {
        speed: 675,
        stand_seconds: 4.5,
        stand_roll: 153,
        listen_lean: [-20, -25],
        nod_share: 1.3,
        idle_share: 0.6,
        idle_size: 1.15,
        clip_size: 1.45,
        clip_pace: 1.375,
    },
    Energy {
        speed: 800,
        stand_seconds: 4.0,
        stand_roll: 200,
        listen_lean: [-25, -30],
        nod_share: 1.4,
        idle_share: 0.5,
        idle_size: 1.2,
        clip_size: 1.6,
        clip_pace: 1.5,
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

/// A built-in clip (`crate::clips::ALL`) as `level` plays it (#8153):
/// each joint `clip_size` times as far, held to the gesture's reach
/// (`crate::body::GESTURE_REACH`), and its frames `clip_pace` times as
/// quick, the step rounded up to a whole millisecond. The body plays it
/// only under `LAMP_BODY_ENERGY`, so each step is stretched under the
/// servo's speed (`Limits::clip_ceiling`), and checks it against V1's
/// anti-tip rule as played. Level 1, and any other clip (a move from a
/// file, a shift at rest, the call's glance), is the clip itself. Each is
/// made once and kept: at most 21 clips for each of 4 levels.
pub fn livelier(clip: &'static Clip, level: u8) -> &'static Clip {
    static MADE: [[OnceLock<Clip>; 4]; crate::clips::ALL.len()] =
        [const { [const { OnceLock::new() }; 4] }; crate::clips::ALL.len()];
    let level = taken(level);
    let built_in = crate::clips::ALL
        .iter()
        .position(|own| own.name == clip.name && own.frames == clip.frames);
    let (Some(index), 2..=5) = (built_in, level) else {
        return clip;
    };
    MADE[index][usize::from(level) - 2].get_or_init(|| {
        let row = of(level);
        let size = CLIP_SIZE_CAP
            .iter()
            .find(|(name, _)| *name == clip.name)
            .map_or(row.clip_size, |(_, cap)| row.clip_size.min(*cap));
        sized(clip, size, row.clip_pace)
    })
}

/// The hardest the base (base yaw) of a built-in clip is asked to change
/// its speed from one step to the next at any level, counts a second a
/// second: the most level 1 asks of it ('headshake', 20 counts in two 51 ms
/// steps, 7,689.4, rounded up), which every lamp plays today. The base carries the
/// lamp's weight: on lamp-52e6 'curious' at x1.6 the size and x1.5 the
/// pace asked about 11,200 and the base rang (#8153, row-8155b).
pub const BASE_BEND: f32 = 7_690.0;

/// `clip` with each joint `size` times as far, held to the gesture's reach
/// (`crate::body::GESTURE_REACH`), and its frames `pace` times as quick,
/// the step rounded up to a whole millisecond. Its frames are kept for good.
/// Held so the base never changes speed harder than [`BASE_BEND`]: the
/// step is first made no quicker than that allows, never slower than the
/// clip's own; where even the clip's own pace would ask more, the base
/// keeps the clip's own size (the head and the arm still grow).
pub fn sized(clip: &Clip, size: f32, pace: f32) -> Clip {
    let reach = crate::body::GESTURE_REACH;
    let mut gain: [f32; 5] = std::array::from_fn(|joint| {
        let most = clip
            .frames
            .iter()
            .map(|frame| frame[joint].unsigned_abs())
            .max()
            .unwrap_or(0);
        if most == 0 {
            1.0
        } else {
            size.min(reach[joint] / f32::from(most))
        }
    });
    let made = |gain: [f32; 5]| -> Vec<[i16; 5]> {
        clip.frames
            .iter()
            .map(|frame| {
                std::array::from_fn(|joint| {
                    (f32::from(frame[joint]) * gain[joint])
                        .round()
                        .clamp(-reach[joint], reach[joint]) as i16
                })
            })
            .collect()
    };
    // The quickest step, ms, at which the base bends no harder than BASE_BEND.
    let quickest = |frames: &[[i16; 5]]| -> u16 {
        let bend = frames
            .windows(3)
            .map(|three| {
                (i32::from(three[2][2]) - 2 * i32::from(three[1][2]) + i32::from(three[0][2]))
                    .unsigned_abs()
            })
            .max()
            .unwrap_or(0);
        (1_000.0 * (bend as f32 / BASE_BEND).sqrt()).ceil() as u16
    };
    let mut frames = made(gain);
    if quickest(&frames) > clip.step_ms && gain[2] > 1.0 {
        gain[2] = 1.0;
        frames = made(gain);
    }
    let step_ms = ((f32::from(clip.step_ms) / pace).ceil() as u16)
        .max(quickest(&frames))
        .min(clip.step_ms);
    Clip {
        name: clip.name,
        step_ms,
        frames: frames.leak(),
    }
}

/// The built-in clips held under a level's `clip_size` (#8153), each at
/// the largest gain (to 0.01) that keeps it inside V1's anti-tip rule on
/// the deployed lamps' standing poses with half a millimetre to spare: no
/// frame past 21.5 mm off the base's axis (22 mm is refused at play time,
/// and a mood's lean then may add to the stand's), none leaning the lamp
/// back past 16 mm
/// (`crate::cog`, `every_built_in_clip_at_level_five_passes_v1s_rule`).
/// Found on the host over all 21 clips at level 5; the rest take x1.6.
pub const CLIP_SIZE_CAP: [(&str, f32); 5] = [
    ("greeting", 1.54),
    ("surprised", 1.27),
    ("sleepy", 1.44),
    ("excited", 1.27),
    ("dance_groove", 1.59),
];

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
        fill: 0.25,
        fill_gap_s: 40,
        pause_gap_s: 3.75,
        music_cooldown_min: 20,
        music_rearm: true,
    },
    Gesture {
        fill: 0.5,
        fill_gap_s: 25,
        pause_gap_s: 3.5,
        music_cooldown_min: 15,
        music_rearm: true,
    },
    // A playlist never stays under the rearm line for 5 minutes: with it,
    // the lamp would sway once ever.
    Gesture {
        fill: 0.75,
        fill_gap_s: 15,
        pause_gap_s: 3.25,
        music_cooldown_min: 10,
        music_rearm: false,
    },
    Gesture {
        fill: 1.0,
        fill_gap_s: 8,
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

    /// Levels 1, 3 and 5 are evenly spaced in the head's standing swing as
    /// lamp-52e6 measured it (row-7153L2, 16 Nov, peak to peak of any head
    /// joint while stood: 7.7, 17.9 and 23.9 degrees with level 3 at 120
    /// counts, its roll then 2 x 0.85 x 120 = 204 counts, 17.9 degrees). The
    /// swing is linear in `stand_roll`, so level 3 is scaled to the midpoint;
    /// level 4 stays midway between 3 and 5 (each level's no less than the
    /// one under it: `levels_only_go_livelier`).
    #[test]
    fn level_three_swings_midway_between_one_and_five() {
        let (one, three_at_120, five) = (7.7_f32, 17.9_f32, 23.9_f32);
        let midway = (one + five) / 2.0;
        let roll = |level: usize| f32::from(TABLE[level - 1].stand_roll);
        let three = three_at_120 * roll(3) / 120.0;
        assert!((three - midway).abs() < 0.1, "level 3 swings {three} deg");
        assert!((2.0 * 0.85 * roll(3) * 360.0 / 4096.0 - midway).abs() < 0.1);
        assert!((roll(4) - (roll(3) + roll(5)) / 2.0).abs() <= 0.5);
        assert!(TABLE[4].stand_roll == 200 && TABLE[..2].iter().all(|row| row.stand_roll == 0));
    }

    /// Level 1 is what the product unit runs today, and is applied as the
    /// base whatever it is.
    #[test]
    fn level_one_is_the_unit_today() {
        assert_eq!(f32::from(TABLE[0].speed), unit_value("--speed"));
        assert_eq!(TABLE[0].stand_seconds, unit_value("--stand-seconds"));
        assert_eq!(TABLE[0].idle_share, 1.0);
        assert_eq!(TABLE[0].idle_size, 1.0);
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
            assert!(pair[1].idle_size > pair[0].idle_size);
            assert!(pair[1].stand_roll >= pair[0].stand_roll);
        }
        assert_eq!(speed(300.0, 5), 800.0);
        assert_eq!(stand_seconds(6.0, 5), 4.0);
        // A shift's arm sag at most a fifth larger (#7301).
        assert_eq!(TABLE[4].idle_size, 1.2);
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
        assert!(GESTURE[1..].iter().all(|row| row.fill_gap_s >= 8));
    }

    /// #8153: the fill and the music sway per level. Level 1 is today's
    /// exactly; level 5 fills every answer with no cue of its own, at least
    /// 8 s apart; 2 to 4 lie between; music sway's 6 min at 5 is kept.
    #[test]
    fn the_fill_per_level_is_every_answer_at_five_and_none_at_one() {
        assert_eq!(
            GESTURE[0],
            Gesture {
                fill: 0.0,
                fill_gap_s: 0,
                pause_gap_s: 4.0,
                music_cooldown_min: 30,
                music_rearm: true,
            }
        );
        let fill: Vec<f32> = GESTURE.iter().map(|row| row.fill).collect();
        assert_eq!(fill, [0.0, 0.25, 0.5, 0.75, 1.0]);
        let gap: Vec<u32> = GESTURE[1..].iter().map(|row| row.fill_gap_s).collect();
        assert_eq!(gap, [40, 25, 15, 8]);
        let music: Vec<u32> = GESTURE.iter().map(|row| row.music_cooldown_min).collect();
        assert_eq!(music, [30, 20, 15, 10, 6]);
    }

    /// The farthest `clip` takes each joint from the pose.
    fn farthest(clip: &Clip) -> [u16; 5] {
        std::array::from_fn(|joint| {
            clip.frames
                .iter()
                .map(|frame| frame[joint].unsigned_abs())
                .max()
                .unwrap_or(0)
        })
    }

    /// The most `clip` asks the base (base yaw) to change its speed from
    /// one step to the next, counts a second a second: its largest second
    /// difference over the step's time squared.
    fn base_bend(clip: &Clip) -> f32 {
        let step_s = f32::from(clip.step_ms) / 1_000.0;
        clip.frames
            .windows(3)
            .map(|three| {
                let bend =
                    i32::from(three[2][2]) - 2 * i32::from(three[1][2]) + i32::from(three[0][2]);
                bend.unsigned_abs() as f32 / (step_s * step_s)
            })
            .fold(0.0, f32::max)
    }

    /// #8153 follow-up, row-8155b on lamp-52e6 (out-8155b-1121-0322,
    /// L5-talk, 1795206564): 'curious' at level 5 rang the base, its
    /// position every 80 ms 91, 115, 81, 182, 168, 87, 82, 108, 107, 73,
    /// with every commanded step under the servo's 1,000 c/s. The base
    /// carries the lamp's weight; x1.6 the size at x1.5 the pace asks it to
    /// change speed 1.6 x 1.5^2 = 3.6 times as hard as level 1 does
    /// ('curious' about 11,200 c/s^2 against 3,200), and the jump in the
    /// trace sits where the clip's base speeds up most (frames 24 to 27).
    /// The bar: no level asks the base of any built-in clip to change speed
    /// harder than level 1 already asks of it somewhere (the most over
    /// `clips::ALL`, 'headshake', about 7,700 c/s^2), which every lamp
    /// plays today.
    #[test]
    fn no_level_bends_the_base_harder_than_level_one_does() {
        let bar = crate::clips::ALL
            .iter()
            .map(|clip| base_bend(clip))
            .fold(0.0, f32::max);
        assert!((7_600.0..7_800.0).contains(&bar), "{bar}");
        assert!(bar <= BASE_BEND && BASE_BEND - bar < 1.0, "{bar}");
        for level in 2..=5 {
            for clip in crate::clips::ALL {
                let bend = base_bend(livelier(clip, level));
                assert!(
                    bend <= bar + 0.5,
                    "{} at level {level}: {bend} c/s^2 against {bar}",
                    clip.name
                );
            }
        }
    }

    /// #6893 / #8153: level 5 is judged against V1 (the lead's bar), joint by
    /// joint, on the two clips the issue names. V1's peak to peak, counts,
    /// over the window V2 kept of `hal/recordings/excited.csv` (2.70 to
    /// 4.70 s) and `happy_wiggle.csv` (1.10 to 5.10 s), converted with the
    /// ranges in `hal/calibration/robots/hal_follower/hal.json` (as
    /// `tools/generate_clips.py` does; Sixth's #8153 table, 20 Nov 20:53,
    /// recomputed from the CSVs 21 Nov). The tolerance is 25% of V1's: a
    /// joint outside it is a known gap, listed with what holds it, so that
    /// closing or opening one fails here and is said.
    #[test]
    fn level_five_sizes_excited_and_happy_against_v1() {
        // (clip, joint, V1 p-p, level 5 p-p / V1 within 0.01, a known gap)
        let lines: [(&Clip, usize, u16, f32, Option<&str>); 7] = [
            (&crate::clips::EXCITED, 0, 146, 0.91, None),
            (&crate::clips::EXCITED, 3, 299, 1.21, None),
            (
                &crate::clips::EXCITED,
                4,
                502,
                0.65,
                Some("the tip cap, x1.27"),
            ),
            (&crate::clips::HAPPY, 0, 268, 0.82, None),
            (
                &crate::clips::HAPPY,
                2,
                697,
                0.34,
                Some("x1.6 of a base cut to 0.22 of V1"),
            ),
            (
                &crate::clips::HAPPY,
                3,
                178,
                1.54,
                Some("over V1: x1.6 of 0.96 of V1"),
            ),
            (&crate::clips::HAPPY, 4, 516, 0.70, Some("the reach, 360")),
        ];
        let peak_to_peak = |clip: &Clip, joint: usize| {
            let at = clip.frames.iter().map(|frame| frame[joint]);
            at.clone().max().unwrap() - at.min().unwrap()
        };
        // V1's excited has no base yaw, nor does the clip at any level.
        assert_eq!(peak_to_peak(livelier(&crate::clips::EXCITED, 5), 2), 0);
        for (clip, joint, v1, pinned, gap) in lines {
            let five = peak_to_peak(livelier(clip, 5), joint);
            let share = f32::from(five as u16) / f32::from(v1);
            assert!(
                (share - pinned).abs() <= 0.01,
                "{} joint {joint}: {five} against V1 {v1}, {share:.2}",
                clip.name
            );
            assert_eq!(
                (share - 1.0).abs() > 0.25,
                gap.is_some(),
                "{} joint {joint}: {five} against V1 {v1}, {share:.2}",
                clip.name
            );
        }
    }

    /// #8153: level 1 plays every clip as recorded, the very clip; the clip
    /// size and pace lie evenly from x1 at level 1 to x1.6 and x1.5 at 5.
    #[test]
    fn level_one_plays_every_clip_as_recorded() {
        for said in [0, 1, 6] {
            for clip in crate::clips::ALL {
                let played = livelier(clip, said);
                assert!(std::ptr::eq(played, clip), "{}", clip.name);
                assert_eq!(played.frames, clip.frames);
                assert_eq!(played.step_ms, clip.step_ms);
            }
        }
        // A clip not built in (a move from a file, the call's glance, a
        // shift at rest) is played as it is at every level.
        for level in 1..=5 {
            for clip in [&crate::clips::CALLED, &crate::clips::TILT_LEFT] {
                assert!(std::ptr::eq(livelier(clip, level), clip));
            }
        }
        let size: Vec<f32> = TABLE.iter().map(|row| row.clip_size).collect();
        let pace: Vec<f32> = TABLE.iter().map(|row| row.clip_pace).collect();
        assert_eq!(size, [1.0, 1.15, 1.3, 1.45, 1.6]);
        assert_eq!(pace, [1.0, 1.125, 1.25, 1.375, 1.5]);
    }

    /// #8153: at level 5 each built-in clip is up to 1.6 times its size
    /// (less where `CLIP_SIZE_CAP` holds it for V1's anti-tip rule), joint
    /// by joint, never past the gesture's reach (`GESTURE_REACH`), and
    /// up to 1.5 times its pace; played under the servo's 1,000 counts a
    /// second (`Limits::clip_ceiling`), no joint goes faster than that on
    /// any step. Level 3 lies between 1 and 5.
    #[test]
    fn level_five_plays_clips_bigger_and_quicker_never_past_the_reach_or_the_servo() {
        let reach = crate::body::GESTURE_REACH;
        let ceiling = Some(1_000.0);
        let (mut grown, mut full_pace) = (0, 0);
        for clip in crate::clips::ALL {
            let (one, three, five) = (farthest(clip), livelier(clip, 3), livelier(clip, 5));
            assert_eq!(five.name, clip.name);
            assert_eq!(five.frames.len(), clip.frames.len(), "{}", clip.name);
            assert_eq!(five.frames[0], [0; 5]);
            assert_eq!(five.frames[five.frames.len() - 1], [0; 5]);
            // The pace: a step 1/1.5 of its time, rounded up to a whole ms,
            // or slower where the base would bend past `BASE_BEND`, never
            // slower than the clip's own.
            let quick = (f32::from(clip.step_ms) / 1.5).ceil() as u16;
            assert!(
                quick <= five.step_ms && five.step_ms <= clip.step_ms,
                "{}",
                clip.name
            );
            assert!(three.step_ms <= clip.step_ms, "{}", clip.name);
            full_pace += usize::from(five.step_ms == quick);
            // A base held to its own size where even the clip's own pace
            // would bend it past `BASE_BEND` ('headshake', 'dance_chill').
            let base_held = farthest(five)[2] == one[2];
            for joint in 0..5 {
                let (at_three, at_five) = (farthest(three)[joint], farthest(five)[joint]);
                let size = CLIP_SIZE_CAP
                    .iter()
                    .find(|(name, _)| *name == clip.name)
                    .map_or(1.6, |(_, cap)| cap.min(1.6));
                let wanted = (f32::from(one[joint]) * size).round().min(reach[joint]);
                assert!(
                    f32::from(at_five) <= reach[joint],
                    "{} joint {joint}: {at_five}",
                    clip.name
                );
                assert!(
                    (f32::from(at_five) - wanted).abs() <= 1.0 || (joint == 2 && base_held),
                    "{} joint {joint}: {at_five} against {wanted}",
                    clip.name
                );
                assert!(
                    one[joint] <= at_three && at_three <= at_five,
                    "{} joint {joint}: {} {at_three} {at_five}",
                    clip.name,
                    one[joint]
                );
                grown += usize::from(at_five > one[joint]);
            }
            // Played at a 20 ms tick under the servo's ceiling, no joint
            // goes faster than it.
            let mut last = [0.0f32; 5];
            for elapsed in (0..five.duration_within_us(ceiling)).step_by(20_000) {
                let now = five.at_within(elapsed, ceiling).unwrap();
                for joint in 0..5 {
                    let speed = (now[joint] - last[joint]).abs() / 0.02;
                    assert!(speed <= 1_000.5, "{} joint {joint}: {speed}", clip.name);
                }
                last = now;
            }
        }
        assert!(grown >= 60, "{grown}");
        // 13 of the 21 play at the full x1.5 (#8153, the base held).
        assert!(full_pace >= 13, "{full_pace}");
        // The same clip at the same level is the same one, made once.
        assert!(std::ptr::eq(
            livelier(&crate::clips::NOD, 5),
            livelier(&crate::clips::NOD, 5)
        ));
    }
}
