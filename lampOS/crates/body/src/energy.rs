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
    /// never past the gesture's reach nor V1's swing ([`livelier`], #8153).
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
/// quick, the step rounded up to a whole millisecond: at level 5 for all of
/// its V1 recording where it was cut from one, held to V1's own swing of it
/// (`crate::clips::V1_SWING`, the lead: level 5 is almost V1; [`sized`]);
/// at 2 to 4 the clip itself as before #8201, a joint lowered only where it
/// would move farther or faster than the level above (`under_five`). The
/// body plays it only under `LAMP_BODY_ENERGY`, so each step is stretched under the
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
        if level < 5 {
            return under_five(clip, level);
        }
        let frames = longer_frames(clip, 5);
        let gain = gain(
            &frames,
            cut_frames(clip, 5),
            clip,
            clip_size(clip.name, 5),
            true,
        );
        played(&longer(clip, 5), gain, of(5).clip_pace)
    })
}

/// A built-in clip at level 2, 3 or 4 (#8153, the lead's call, 22 Nov): as
/// before #8201, the clip itself at the level's size and pace, but no joint
/// moving farther (peak to peak) or faster (its largest step) than the level
/// above moves it over the stretch of the cut that level plays: there that
/// joint alone is lowered to it, the step kept.
fn under_five(clip: &'static Clip, level: u8) -> Clip {
    let all = 0..=clip.frames.len() - 1;
    let mut gain = gain(clip.frames, all, clip, clip_size(clip.name, level), false);
    let step_ms = step_at(clip, &made(clip.frames, gain), of(level).clip_pace);
    let above = livelier(clip, level + 1);
    let stretch = &above.frames[cut_frames(clip, level + 1)];
    for joint in 0..5 {
        let (most, fastest) = how_far_and_fast(stretch, above.step_ms, joint);
        let over = |gain: [f32; 5]| {
            let (swing, speed) = how_far_and_fast(&made(clip.frames, gain), step_ms, joint);
            (swing > most || speed > fastest).then_some((swing, speed))
        };
        let Some((swing, speed)) = over(gain) else {
            continue;
        };
        gain[joint] *= (most as f32 / swing as f32).min(fastest / speed);
        // ponytail: rounding may leave it a count over; step down until not.
        while over(gain).is_some() {
            gain[joint] *= 0.999;
        }
    }
    Clip {
        name: clip.name,
        step_ms,
        frames: made(clip.frames, gain).leak(),
    }
}

/// How far and how fast `frames`, `step_ms` apart, take `joint`: peak to
/// peak, counts, and its largest step, counts a second.
fn how_far_and_fast(frames: &[[i16; 5]], step_ms: u16, joint: usize) -> (i32, f32) {
    let at = frames.iter().map(|frame| i32::from(frame[joint]));
    let swing = at.clone().max().unwrap_or(0) - at.min().unwrap_or(0);
    let step = frames
        .windows(2)
        .map(|two| (i32::from(two[1][joint]) - i32::from(two[0][joint])).abs())
        .max()
        .unwrap_or(0);
    (swing, step as f32 * 1_000.0 / f32::from(step_ms))
}

/// The hardest the base (base yaw) of a built-in clip is asked to change
/// its speed from one step to the next at any level, counts a second a
/// second: the most level 1 asks of it ('headshake', 20 counts in two 51 ms
/// steps, 7,689.4, rounded up), which every lamp plays today. The base carries the
/// lamp's weight: on lamp-52e6 'curious' at x1.6 the size and x1.5 the
/// pace asked about 11,200 and the base rang (#8153, row-8155b).
pub const BASE_BEND: f32 = 7_690.0;

/// The longest a built-in clip plays at any level, each step stretched
/// under the servo's top speed (`Tuning::SERVO_SPEED_CAP`, the product
/// unit's `--servo-speed`), or as recorded: how long one asking for it
/// waits for its end (#8153: level 5 plays a cut clip for all of its
/// recording, longer than level 1).
pub fn longest_us(clip: &'static Clip) -> u64 {
    let ceiling = Some(f32::from(crate::body::Tuning::SERVO_SPEED_CAP));
    (1..=5)
        .map(|level| livelier(clip, level).duration_within_us(ceiling))
        .fold(clip.duration_us(), u64::max)
}

/// A built-in clip's size at `level`: the level's `clip_size`, less where
/// [`CLIP_SIZE_CAP`] holds it, or below level 5 [`UNDER_FIVE_CAP`].
pub fn clip_size(name: &str, level: u8) -> f32 {
    let under_five = UNDER_FIVE_CAP.iter().filter(|_| taken(level) < 5);
    CLIP_SIZE_CAP
        .iter()
        .chain(under_five)
        .filter(|(cap, _)| *cap == name)
        .fold(of(level).clip_size, |size, (_, cap)| size.min(*cap))
}

/// A built-in clip as long as `level` plays it, before its size and pace
/// (#8153): at level 5 one cut from its recording (`crate::clips::WHOLE`)
/// is all of it (or as much of it as [`WHOLE_TRIM`] keeps), never shorter
/// than the clip itself; below 5, and any other clip, the clip itself.
pub fn longer(clip: &'static Clip, level: u8) -> Clip {
    match window(clip, level) {
        Some((whole, first, last)) => span(whole, first, last),
        None => *clip,
    }
}

/// [`longer`]'s frames, not kept.
fn longer_frames(clip: &'static Clip, level: u8) -> Vec<[i16; 5]> {
    window(clip, level).map_or_else(
        || clip.frames.to_vec(),
        |(whole, first, last)| span_frames(whole, first, last),
    )
}

/// The frames of its recording a cut clip plays at `level`, first and
/// last, or none where it plays the clip itself.
fn window(clip: &'static Clip, level: u8) -> Option<(&'static crate::clips::Whole, usize, usize)> {
    let whole = crate::clips::WHOLE
        .iter()
        .find(|whole| whole.cut.name == clip.name && whole.cut.frames == clip.frames)?;
    if taken(level) < 5 {
        return None;
    }
    let (first, last) = whole_kept(whole);
    // Never shorter than level 1: a stretch no longer than the clip keeps
    // the clip.
    (last - first >= clip.frames.len()).then_some((whole, first, last))
}

/// The frames of `clip` as `level` plays it ([`livelier`]) that are the
/// stretch V2 cut: all of them where it plays the clip itself.
pub fn cut_frames(clip: &'static Clip, level: u8) -> std::ops::RangeInclusive<usize> {
    match window(clip, level) {
        Some((whole, first, _)) => whole.from - first..=whole.to - first,
        None => 0..=clip.frames.len() - 1,
    }
}

/// The frames of `whole`'s recording level 5 plays: all of them, or as
/// many as [`WHOLE_TRIM`] keeps.
pub fn whole_kept(whole: &crate::clips::Whole) -> (usize, usize) {
    WHOLE_TRIM
        .iter()
        .find(|(name, ..)| *name == whole.cut.name)
        .map_or((0, whole.whole.frames.len() - 1), |(_, first, last, _)| {
            (*first, *last)
        })
}

/// Frames `first` to `last` of `whole`'s recording as a clip: taken from
/// the first of them, so it begins at nothing, and, ended before the
/// recording does, brought back to nothing over its last ten frames as the
/// generator ends a cut (`tools/generate_clips.py`). At the cut's frame
/// time. Its frames are kept for good.
pub fn span(whole: &crate::clips::Whole, first: usize, last: usize) -> Clip {
    Clip {
        name: whole.cut.name,
        step_ms: whole.whole.step_ms,
        frames: span_frames(whole, first, last).leak(),
    }
}

/// [`span`]'s frames, not kept.
fn span_frames(whole: &crate::clips::Whole, first: usize, last: usize) -> Vec<[i16; 5]> {
    let start = whole.whole.frames[first];
    let mut frames: Vec<[i16; 5]> = whole.whole.frames[first..=last]
        .iter()
        .map(|frame| std::array::from_fn(|joint| frame[joint] - start[joint]))
        .collect();
    let count = frames.len();
    let end = frames[count - 1];
    let tail = 10.min(count - 1);
    for step in 0..=tail {
        let share = step as f32 / tail.max(1) as f32;
        for joint in 0..5 {
            let eased = (f32::from(end[joint]) * share).round() as i16;
            frames[count - 1 - tail + step][joint] -= eased;
        }
    }
    frames
}

/// `clip` with each joint `size` times as far, held to the gesture's reach
/// (`crate::body::GESTURE_REACH`) and to V1's swing of it
/// (`crate::clips::V1_SWING`), and its frames `pace` times as quick, the
/// step rounded up to a whole millisecond. Its frames are kept for good.
/// Held so the base never changes speed harder than [`BASE_BEND`]: the
/// step is first made no quicker than that allows, never slower than the
/// clip's own; where even the clip's own pace would ask more, the base
/// keeps the clip's own size (the head and the arm still grow).
pub fn sized(clip: &Clip, size: f32, pace: f32) -> Clip {
    let gain = gain(clip.frames, 0..=clip.frames.len() - 1, clip, size, true);
    played(clip, gain, pace)
}

/// Each joint's gain for `frames` (`clip` as some level plays it) at
/// `size`: at most `size`; at most what takes the farthest of its frames
/// `cut` (the stretch V2 cut, Seventh's P2 on #8201) to the gesture's
/// reach, so the gesture is no smaller for being longer, the rest of the
/// recording held at the reach; at most what swings it, peak to peak, as
/// far as V1 swung it over all of its recording (the lead, #8153: level 5
/// is almost V1), never under 1 for that (a joint the generator's gains
/// took past V1 keeps level 1's size; only with `to_v1`, level 5's); and
/// the base 1 where its bend at the clip's own step would pass [`BASE_BEND`].
fn gain(
    frames: &[[i16; 5]],
    cut: std::ops::RangeInclusive<usize>,
    clip: &Clip,
    size: f32,
    to_v1: bool,
) -> [f32; 5] {
    let reach = crate::body::GESTURE_REACH;
    let v1 = crate::clips::V1_SWING
        .iter()
        .filter(|_| to_v1)
        .find(|(name, _)| *name == clip.name)
        .map(|(_, swing)| *swing);
    let mut gain: [f32; 5] = std::array::from_fn(|joint| {
        let most = frames[cut.clone()]
            .iter()
            .map(|frame| frame[joint].unsigned_abs())
            .max()
            .unwrap_or(0);
        let at = frames.iter().map(|frame| i32::from(frame[joint]));
        let swing = at.clone().max().unwrap_or(0) - at.min().unwrap_or(0);
        let held = match v1 {
            Some(v1) if swing > 0 => (f32::from(v1[joint]) / swing as f32).max(1.0),
            _ => size,
        };
        if most == 0 {
            1.0
        } else {
            size.min(reach[joint] / f32::from(most)).min(held)
        }
    });
    if quickest(&made(frames, gain)) > clip.step_ms && gain[2] > 1.0 {
        gain[2] = 1.0;
    }
    gain
}

/// `frames` each joint `gain` times as far, held to the gesture's reach.
fn made(frames: &[[i16; 5]], gain: [f32; 5]) -> Vec<[i16; 5]> {
    let reach = crate::body::GESTURE_REACH;
    frames
        .iter()
        .map(|frame| {
            std::array::from_fn(|joint| {
                (f32::from(frame[joint]) * gain[joint])
                    .round()
                    .clamp(-reach[joint], reach[joint]) as i16
            })
        })
        .collect()
}

/// The quickest step, ms, at which the base bends no harder than BASE_BEND.
fn quickest(frames: &[[i16; 5]]) -> u16 {
    let bend = frames
        .windows(3)
        .map(|three| {
            (i32::from(three[2][2]) - 2 * i32::from(three[1][2]) + i32::from(three[0][2]))
                .unsigned_abs()
        })
        .max()
        .unwrap_or(0);
    (1_000.0 * (bend as f32 / BASE_BEND).sqrt()).ceil() as u16
}

/// `clip` at `gain`, its frames `pace` times as quick, never so quick the
/// base bends past [`BASE_BEND`], never slower than its own. Kept for good.
fn played(clip: &Clip, gain: [f32; 5], pace: f32) -> Clip {
    let frames = made(clip.frames, gain);
    Clip {
        name: clip.name,
        step_ms: step_at(clip, &frames, pace),
        frames: frames.leak(),
    }
}

/// `clip`'s step played as `frames`, `pace` times as quick, never so quick
/// the base bends past [`BASE_BEND`], never slower than its own.
fn step_at(clip: &Clip, frames: &[[i16; 5]], pace: f32) -> u16 {
    ((f32::from(clip.step_ms) / pace).ceil() as u16)
        .max(quickest(frames))
        .min(clip.step_ms)
}

/// The built-in clips held under a level's `clip_size` (#8153), each at
/// the largest gain (to 0.01) that keeps it inside V1's anti-tip rule on
/// the deployed lamps' standing poses with half a millimetre to spare: no
/// frame past 21.5 mm off the base's axis (22 mm is refused at play time,
/// and a mood's lean then may add to the stand's), none leaning the lamp
/// back past 16 mm
/// (`crate::cog`, `every_built_in_clip_at_every_level_passes_v1s_rule`).
/// Found on the host over all 21 clips at level 5; the rest take x1.6
/// (sleepy and excited, held at 1.44 and 1.27 before V1's swing held them,
/// now pass at x1.6, #8153).
pub const CLIP_SIZE_CAP: [(&str, f32); 3] = [
    ("greeting", 1.54),
    ("surprised", 1.27),
    ("dance_groove", 1.59),
];

/// Levels 2 to 4's own caps, as they were before #8201 (the lead's call,
/// 22 Nov: levels 2 to 4 play as before): sleepy and excited held at 1.44
/// and 1.27 of the clip itself.
pub const UNDER_FIVE_CAP: [(&str, f32); 2] = [("sleepy", 1.44), ("excited", 1.27)];

/// The cut clips level 5 does not play for all of their recording (#8153),
/// each as long as V1's anti-tip rule lets it be at its level-5 size on the
/// deployed lamps' standing poses, with the same half millimetre to spare
/// as [`CLIP_SIZE_CAP`] (no frame past 21.5 mm off the base's axis, none
/// leaning the lamp back past 16 mm): its recording's first and last frame
/// kept, and the bound that held it. Found on the host
/// (`cog::level_five_keeps_as_much_of_each_recording_as_v1s_rule_allows`,
/// which cuts the crossing frames off from all of it and finds exactly
/// these). The rest play all of it.
pub const WHOLE_TRIM: [(&str, usize, usize, &str); 2] = [
    ("caring", 0, 165, "the tip rule, 21.5 mm off the axis"),
    ("scanning", 28, 359, "the tip rule, 21.5 mm off the axis"),
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
    /// one step to the next, counts a second a second, played under
    /// `ceiling` (`Limits::clip_ceiling`; `None` is as recorded): each
    /// step's speed is its counts over its own time, which the ceiling may
    /// have stretched (`Clip::step_within_us`), and the change between two
    /// steps is over the mean of their times. With even steps this is the
    /// second difference over the step's time squared.
    fn base_bend(clip: &Clip, ceiling: Option<f32>) -> f32 {
        let step_s = |frame: usize| match ceiling {
            None => f32::from(clip.step_ms) / 1_000.0,
            Some(ceiling) => clip.step_within_us(frame, ceiling) as f32 / 1e6,
        };
        let speed = |frame: usize| {
            f32::from(clip.frames[frame + 1][2] - clip.frames[frame][2]) / step_s(frame)
        };
        (1..clip.frames.len().saturating_sub(1))
            .map(|frame| {
                (speed(frame) - speed(frame - 1)).abs()
                    / ((step_s(frame - 1) + step_s(frame)) / 2.0)
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
            .map(|clip| base_bend(clip, None))
            .fold(0.0, f32::max);
        assert!((7_600.0..7_800.0).contains(&bar), "{bar}");
        assert!(bar <= BASE_BEND && BASE_BEND - bar < 1.0, "{bar}");
        for level in 2..=5 {
            for clip in crate::clips::ALL {
                // Played as it is played: each step stretched under the
                // servo's 1,000, which makes some steps uneven (#8183).
                let bend = base_bend(livelier(clip, level), Some(1_000.0));
                assert!(
                    bend <= bar + 0.5,
                    "{} at level {level}: {bend} c/s^2 against {bar}",
                    clip.name
                );
            }
        }
    }

    /// #6893 / #8153: level 5 is judged against V1 (the lead's bar: level 5
    /// is almost V1, never past it), joint by joint, on the two clips the
    /// issue names. V1's peak to peak, counts, over all of
    /// `hal/recordings/excited.csv` and `happy_wiggle.csv`, the stretch
    /// level 5 now plays (`clips::V1_SWING`, checked against the files by
    /// `level_five_swings_no_joint_past_v1`). No joint is past V1 but
    /// where level 1's own gains already are; a joint under 0.75 of V1 is
    /// a known gap, listed with what holds it, so that closing or opening
    /// one fails here and is said.
    #[test]
    fn level_five_sizes_excited_and_happy_against_v1() {
        // (clip, joint, V1 p-p, level 5 p-p / V1 within 0.01, what holds it)
        let lines: [(&Clip, usize, u16, f32, Option<&str>); 7] = [
            (
                &crate::clips::EXCITED,
                0,
                154,
                1.34,
                Some("over V1: level 1's own gain, 1.35 of V1, not grown"),
            ),
            (&crate::clips::EXCITED, 3, 337, 1.0, None),
            (&crate::clips::EXCITED, 4, 676, 0.67, Some("the reach, 360")),
            (&crate::clips::HAPPY, 0, 314, 1.0, None),
            (
                &crate::clips::HAPPY,
                2,
                697,
                0.35,
                Some("x1.6 of a base cut to 0.22 of V1"),
            ),
            (&crate::clips::HAPPY, 3, 228, 1.0, None),
            (&crate::clips::HAPPY, 4, 627, 0.78, None),
        ];
        let peak_to_peak = |clip: &Clip, joint: usize| {
            let at = clip.frames.iter().map(|frame| frame[joint]);
            at.clone().max().unwrap() - at.min().unwrap()
        };
        // V1's excited has no base yaw, nor does the clip at any level.
        assert_eq!(peak_to_peak(livelier(&crate::clips::EXCITED, 5), 2), 0);
        for (clip, joint, v1, pinned, held) in lines {
            assert_eq!(
                crate::clips::V1_SWING
                    .iter()
                    .find(|(name, _)| *name == clip.name)
                    .unwrap()
                    .1[joint],
                v1
            );
            let five = peak_to_peak(livelier(clip, 5), joint);
            let share = f32::from(five as u16) / f32::from(v1);
            assert!(
                (share - pinned).abs() <= 0.01,
                "{} joint {joint}: {five} against V1 {v1}, {share:.2}",
                clip.name
            );
            assert_eq!(
                !(0.75..=1.01).contains(&share),
                held.is_some(),
                "{} joint {joint}: {five} against V1 {v1}, {share:.2}",
                clip.name
            );
        }
    }

    /// How far `clip` swings `joint`, peak to peak.
    fn spread(clip: &Clip, joint: usize) -> i32 {
        let at = clip.frames.iter().map(|frame| i32::from(frame[joint]));
        at.clone().max().unwrap() - at.min().unwrap()
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
    /// by joint, never past V1's own swing of the joint (the lead, 21 Nov:
    /// where bigger would pass V1, V1 wins, `level_five_swings_no_joint_past_v1`)
    /// nor the gesture's reach (`GESTURE_REACH`), and
    /// up to 1.5 times its pace; played under the servo's 1,000 counts a
    /// second (`Limits::clip_ceiling`), no joint goes faster than that on
    /// any step. Level 3 lies between 1 and 5.
    #[test]
    fn level_five_plays_clips_bigger_and_quicker_never_past_the_reach_or_the_servo() {
        let reach = crate::body::GESTURE_REACH;
        let ceiling = Some(1_000.0);
        let (mut grown, mut full_pace, mut slowed) = (0, 0, vec![]);
        for clip in crate::clips::ALL {
            // Sized against the clip as long as level 5 plays it (#8153,
            // `longer`): the cut clip itself for one not cut.
            let long = longer(clip, 5);
            let (one, three, five) = (farthest(&long), livelier(clip, 3), livelier(clip, 5));
            assert_eq!(five.name, clip.name);
            assert_eq!(five.frames.len(), long.frames.len(), "{}", clip.name);
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
            if five.step_ms != quick {
                slowed.push(clip.name);
            }
            // A base held to its own size where even the clip's own pace
            // would bend it past `BASE_BEND` ('headshake', 'dance_chill').
            let base_held = farthest(five)[2] == one[2];
            let v1 = crate::clips::V1_SWING
                .iter()
                .find(|(name, _)| *name == clip.name)
                .unwrap()
                .1;
            for joint in 0..5 {
                let (at_three, at_five) = (farthest(three)[joint], farthest(five)[joint]);
                let size = CLIP_SIZE_CAP
                    .iter()
                    .find(|(name, _)| *name == clip.name)
                    .map_or(1.6, |(_, cap)| cap.min(1.6));
                assert!(
                    f32::from(at_five) <= reach[joint],
                    "{} joint {joint}: {at_five}",
                    clip.name
                );
                // Its swing over all it plays is the size's, or less where
                // a bound holds it, none loosened (the lead, 21 Nov): the
                // reach from the cut's farthest frame as played (the rest of the
                // recording held at it), V1's own swing of the joint (never
                // under level 1's), the base kept at its own size.
                let own = spread(&long, joint) as f32;
                let most = long.frames[cut_frames(clip, 5)]
                    .iter()
                    .map(|frame| frame[joint].unsigned_abs())
                    .max()
                    .unwrap();
                let gain = size
                    .min(reach[joint] / f32::from(most.max(1)))
                    .min((f32::from(v1[joint]) / own.max(1.0)).max(1.0));
                let (swing, wanted) = (spread(five, joint) as f32, own * gain);
                let clamped = at_five as f32 == reach[joint];
                assert!(
                    swing <= wanted + 1.0
                        && (swing >= wanted - 1.0 || clamped || (joint == 2 && base_held)),
                    "{} joint {joint}: {swing} against {wanted}",
                    clip.name
                );
                // Each level plays its own stretch of the recording, taken
                // from its own first frame, so its farthest from the pose is
                // not comparable with level 1's (happy's yaw 160 at 3 and
                // 144 at 5; caring's elbow 333 at 1 and 307 at 5, taken
                // from the recording's first frame): its swing, peak to
                // peak, is never under level 1's.
                let level_one = farthest(clip)[joint];
                assert!(
                    spread(clip, joint) <= spread(five, joint) && (at_three > 0 || level_one == 0),
                    "{} joint {joint}: {} against {}",
                    clip.name,
                    spread(clip, joint),
                    spread(five, joint)
                );
                grown += usize::from(at_five > level_one);
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
        // 15 of the 21 play at the full x1.5 (#8153, the base held).
        assert!(full_pace >= 15, "{full_pace}");
        // The other 6 at a slower step, the base held under `BASE_BEND`
        // (docs/body.md says the same list).
        assert_eq!(
            slowed,
            [
                "headshake",
                "acknowledge",
                "curious",
                "dance_chill",
                "dance_hype",
                "dance_waltz"
            ]
        );
        // The same clip at the same level is the same one, made once.
        assert!(std::ptr::eq(
            livelier(&crate::clips::NOD, 5),
            livelier(&crate::clips::NOD, 5)
        ));
    }

    /// `hal/recordings/NAME.csv` of each built-in clip (#8153), as the
    /// generator names them (`tools/generate_clips.py`).
    const RECORDINGS: [(&str, &str); 21] = [
        ("nod", include_str!("../../../../hal/recordings/nod.csv")),
        (
            "headshake",
            include_str!("../../../../hal/recordings/headshake.csv"),
        ),
        (
            "greeting",
            include_str!("../../../../hal/recordings/greeting.csv"),
        ),
        (
            "laugh",
            include_str!("../../../../hal/recordings/laugh.csv"),
        ),
        ("shy", include_str!("../../../../hal/recordings/shy.csv")),
        (
            "stretching",
            include_str!("../../../../hal/recordings/stretching.csv"),
        ),
        ("idle", include_str!("../../../../hal/recordings/idle.csv")),
        (
            "acknowledge",
            include_str!("../../../../hal/recordings/acknowledge.csv"),
        ),
        (
            "curious",
            include_str!("../../../../hal/recordings/curious.csv"),
        ),
        (
            "happy",
            include_str!("../../../../hal/recordings/happy_wiggle.csv"),
        ),
        ("caring", include_str!("../../../../hal/recordings/sad.csv")),
        (
            "surprised",
            include_str!("../../../../hal/recordings/shock.csv"),
        ),
        (
            "confused",
            include_str!("../../../../hal/recordings/confused.csv"),
        ),
        (
            "sleepy",
            include_str!("../../../../hal/recordings/sleepy.csv"),
        ),
        (
            "excited",
            include_str!("../../../../hal/recordings/excited.csv"),
        ),
        (
            "scanning",
            include_str!("../../../../hal/recordings/scanning.csv"),
        ),
        (
            "dance_chill",
            include_str!("../../../../hal/recordings/music_chill.csv"),
        ),
        (
            "dance_groove",
            include_str!("../../../../hal/recordings/music_groove.csv"),
        ),
        (
            "dance_hype",
            include_str!("../../../../hal/recordings/music_hype.csv"),
        ),
        (
            "dance_waltz",
            include_str!("../../../../hal/recordings/music_waltz.csv"),
        ),
        (
            "dance_jazz",
            include_str!("../../../../hal/recordings/music_jazz.csv"),
        ),
    ];

    /// #8153 (the lead, 20 Nov): level 5 plays each built-in clip for all
    /// of its V1 recording, not only the stretch V2 cut (2 to 9 s): its
    /// frames are the recording's rows, every one, unless `WHOLE_TRIM` names
    /// the bound that trims it, and then exactly the rows it keeps. Levels
    /// 1 to 4 play the clip itself, as long (the lead's call, 22 Nov:
    /// levels 2 to 4 as before #8201; `level_one_plays_every_clip_as_recorded`).
    /// Its time is the rows at the clip's frame time over the level's 1.5
    /// pace, each step stretched under the servo's 1,000 counts a second.
    /// Prints a line a clip (`cargo test -- --nocapture`).
    #[test]
    fn level_five_plays_each_clip_for_all_of_its_v1_recording() {
        let ceiling = Some(1_000.0);
        let mut whole = 0;
        for clip in crate::clips::ALL {
            let (_, csv) = RECORDINGS
                .iter()
                .find(|(name, _)| *name == clip.name)
                .unwrap();
            let rows: Vec<&str> = csv.lines().skip(1).filter(|row| !row.is_empty()).collect();
            let seconds = |row: &str| row.split(',').next().unwrap().parse::<f64>().unwrap();
            let v1_s = seconds(rows[rows.len() - 1]) - seconds(rows[0]);
            let trim = WHOLE_TRIM.iter().find(|(name, ..)| *name == clip.name);
            // Trimmed to no longer than the clip itself, the clip is kept.
            let kept = trim.map_or(rows.len(), |(_, first, last, _)| {
                (last - first + 1).max(clip.frames.len())
            });
            let five = livelier(clip, 5);
            assert_eq!(
                five.frames.len(),
                kept,
                "{}: {} rows",
                clip.name,
                rows.len()
            );
            if let Some((_, first, last, why)) = trim {
                assert!(*first > 0 || *last < rows.len() - 1, "{}", clip.name);
                assert!(!why.is_empty(), "{}", clip.name);
            }
            // Each level longer than the one under it, for a clip cut, or
            // the clip itself where that would be shorter (excited below 5).
            let lengths: Vec<usize> = (1..=5)
                .map(|level| livelier(clip, level).frames.len())
                .collect();
            if kept == clip.frames.len() {
                assert!(
                    lengths.iter().all(|length| *length == kept),
                    "{} {lengths:?}",
                    clip.name
                );
            } else if crate::clips::WHOLE
                .iter()
                .any(|whole| whole.cut.name == clip.name)
            {
                whole += 1;
                assert_eq!(lengths[0], clip.frames.len(), "{}", clip.name);
                assert!(
                    lengths
                        .windows(2)
                        .all(|pair| pair[0] < pair[1] || pair[1] == lengths[0]),
                    "{} {lengths:?}",
                    clip.name
                );
            } else {
                unreachable!("{}: {kept} frames of {} rows", clip.name, rows.len());
            }
            eprintln!(
                "length {:<13} level 1 {:5.2} s, V1 {v1_s:5.2} s ({} rows), level 5 {:5.2} s ({kept} rows){}",
                clip.name,
                clip.duration_us() as f64 / 1e6,
                rows.len(),
                five.duration_within_us(ceiling) as f64 / 1e6,
                trim.map_or(String::new(), |(_, first, last, why)| format!(
                    ", rows {first} to {last}: {why}"
                )),
            );
        }
        assert_eq!(whole, crate::clips::WHOLE.len());
    }

    /// V1's swing of each joint, peak to peak, counts, over all of `csv`
    /// (a `hal/recordings` file): the generator's units, lerobot's -100 to
    /// 100 over each joint's range in counts (`tools/generate_clips.py`'s
    /// RANGE, from `hal/calibration/robots/hal_follower/hal.json`), in the
    /// clip's joint order. Read here from the files, not from the
    /// generated `clips::V1_SWING`, so that table is checked too.
    fn v1_swing_of(csv: &str) -> [i32; 5] {
        let ranges = [
            ("wrist_pitch", 1464.0, 3071.0),
            ("wrist_roll", 621.0, 3254.0),
            ("base_yaw", 847.0, 3275.0),
            ("base_pitch", 0.0, 4095.0),
            ("elbow_pitch", 0.0, 4095.0),
        ];
        let mut lines = csv.lines();
        let header: Vec<&str> = lines.next().unwrap().split(',').collect();
        let rows: Vec<Vec<f64>> = lines
            .filter(|line| !line.is_empty())
            .map(|line| line.split(',').map(|v| v.parse().unwrap()).collect())
            .collect();
        std::array::from_fn(|joint| {
            let (name, lo, hi) = ranges[joint];
            let column = header
                .iter()
                .position(|title| *title == format!("{name}.pos"))
                .unwrap();
            let counts = rows
                .iter()
                .map(|row| (row[column].clamp(-100.0, 100.0) + 100.0) / 200.0 * (hi - lo) + lo);
            let (most, least) = counts.fold((f64::MIN, f64::MAX), |(most, least), at| {
                (most.max(at), least.min(at))
            });
            (most - least).round() as i32
        })
    }

    /// The lead's rule (#8153, via V2, 21 Nov): level 5 is almost V1, and no
    /// safety bound is loosened for it. At level 5 no joint of any built-in
    /// clip swings, peak to peak over all it plays, past V1's swing of that
    /// joint over all of its recording (`v1_swing_of`), a count of rounding;
    /// except a joint the generator's own gains already took past V1, which
    /// level 5 grows no further than level 1's gains play it over the same
    /// stretch (`longer`, unscaled), so it is never smaller than level 1.
    /// Prints each joint the cap holds (`cargo test -- --nocapture`).
    #[test]
    fn level_five_swings_no_joint_past_v1() {
        for clip in crate::clips::ALL {
            let (_, csv) = RECORDINGS
                .iter()
                .find(|(name, _)| *name == clip.name)
                .unwrap();
            let v1 = v1_swing_of(csv);
            let (long, five) = (longer(clip, 5), livelier(clip, 5));
            for (joint, &v1_joint) in v1.iter().enumerate() {
                let (own, at_five) = (spread(&long, joint), spread(five, joint));
                let bar = v1_joint.max(own);
                assert!(
                    at_five <= bar + 1,
                    "{} joint {joint}: {at_five} at level 5 against V1 {}, level 1's gains {own}",
                    clip.name,
                    v1_joint
                );
                let wanted = (own as f32 * clip_size(clip.name, 5)).round() as i32;
                if wanted > bar + 1 && own > 0 {
                    eprintln!(
                        "v1 cap {:<13} joint {joint}: V1 {} level 5 {at_five} (x{} would be {wanted})",
                        clip.name,
                        v1_joint,
                        clip_size(clip.name, 5)
                    );
                }
            }
        }
    }

    /// Seventh's HOLD P2 on #8201: a longer clip is not a smaller one.
    /// Over the stretch V2 cut, each joint swings, peak to peak, no less at
    /// a level than the level under it would swing that same stretch (a
    /// count of rounding), for every built-in clip and level 2 to 5:
    /// surprised's elbow and excited's shoulder and elbow sized from all of
    /// their recording swung less at 5 than at 4. The level under's swing
    /// is its gain (its swing over its own stretch's, as `longer` gives it)
    /// on this level's stretch: the same frames but for excited, whose
    /// clip below level 5 is its bounce played twice, eased at each end,
    /// and at 5 that bounce once in its recording, a little smaller.
    #[test]
    fn the_swing_over_the_cut_never_drops_below_the_level_under_it() {
        let swing = |played: &Clip, frames: std::ops::RangeInclusive<usize>, joint: usize| {
            let at = played.frames[frames]
                .iter()
                .map(|frame| i32::from(frame[joint]));
            at.clone().max().unwrap() - at.min().unwrap()
        };
        for clip in crate::clips::ALL {
            for level in 2..=5 {
                let (here, there) = (cut_frames(clip, level), cut_frames(clip, level - 1));
                let (now, under) = (livelier(clip, level), livelier(clip, level - 1));
                let (raw_here, raw_there) = (longer(clip, level), longer(clip, level - 1));
                for joint in 0..5 {
                    let now = swing(now, here.clone(), joint);
                    let under = swing(under, there.clone(), joint);
                    let (raw_here, raw_there) = (
                        swing(&raw_here, here.clone(), joint),
                        swing(&raw_there, there.clone(), joint),
                    );
                    let wanted = under as f32 * raw_here as f32 / raw_there.max(1) as f32;
                    assert!(
                        now as f32 + 1.0 >= wanted.round(),
                        "{} joint {joint}: {now} at level {level}, {under} at {} ({wanted:.0} on this stretch)",
                        clip.name,
                        level - 1
                    );
                }
            }
        }
    }

    /// `livelier` for levels 2 to 4 before #8201 (94672e3b7b's, the body's
    /// `sized` and that day's `CLIP_SIZE_CAP`, kept verbatim here as the
    /// reference): the clip itself, each joint the level's size, held to
    /// the gesture's reach and the base at its own size where its bend at
    /// the clip's own step would pass [`BASE_BEND`], its frames the level's
    /// pace quicker.
    fn before_8201(clip: &Clip, level: u8) -> Clip {
        const CAP: [(&str, f32); 5] = [
            ("greeting", 1.54),
            ("surprised", 1.27),
            ("sleepy", 1.44),
            ("excited", 1.27),
            ("dance_groove", 1.59),
        ];
        let row = of(level);
        let size = CAP
            .iter()
            .find(|(name, _)| *name == clip.name)
            .map_or(row.clip_size, |(_, cap)| row.clip_size.min(*cap));
        let pace = row.clip_pace;
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

    /// How far and how fast `frames`, `step_ms` apart, take `joint`: peak
    /// to peak, counts, and its largest step, counts a second.
    fn how_far_and_fast(frames: &[[i16; 5]], step_ms: u16, joint: usize) -> (i32, f32) {
        let at = frames.iter().map(|frame| i32::from(frame[joint]));
        let swing = at.clone().max().unwrap_or(0) - at.min().unwrap_or(0);
        let step = frames
            .windows(2)
            .map(|two| (i32::from(two[1][joint]) - i32::from(two[0][joint])).abs())
            .max()
            .unwrap_or(0);
        (swing, step as f32 * 1_000.0 / f32::from(step_ms))
    }

    /// The lead's call on #8153 (V2, 22 Nov 16:3x, the first builder's (b)):
    /// levels 2 to 4 stay as they were before #8201, except where one would
    /// move a joint more than the level above it, and there it is capped at
    /// that level. So at 2 to 4 every built-in clip has the frames and the
    /// step [`before_8201`] gives it, but for a joint that swings farther
    /// (peak to peak) or steps faster than the level above does over the
    /// stretch that level plays of the cut (`cut_frames`), which is lowered
    /// to no more than it; and no level 1 to 4 moves any joint more than
    /// the level above. Prints each joint the cap holds
    /// (`cargo test -- --nocapture`).
    #[test]
    fn levels_two_to_four_play_as_before_8201_never_more_than_the_level_above() {
        let mut held = Vec::new();
        for clip in crate::clips::ALL {
            for level in 2..=4u8 {
                let (old, now) = (before_8201(clip, level), livelier(clip, level));
                let above = livelier(clip, level + 1);
                let stretch = &above.frames[cut_frames(clip, level + 1)];
                assert_eq!(
                    (now.step_ms, now.frames.len()),
                    (old.step_ms, old.frames.len()),
                    "{} at level {level}",
                    clip.name
                );
                for joint in 0..5 {
                    let column = |clip: &Clip| -> Vec<i16> {
                        clip.frames.iter().map(|frame| frame[joint]).collect()
                    };
                    let (swing, speed) = how_far_and_fast(old.frames, old.step_ms, joint);
                    let (top_swing, top_speed) = how_far_and_fast(stretch, above.step_ms, joint);
                    if swing <= top_swing && speed <= top_speed {
                        assert_eq!(
                            column(now),
                            column(&old),
                            "{} at level {level}, joint {joint}",
                            clip.name
                        );
                        continue;
                    }
                    let (now_swing, now_speed) = how_far_and_fast(now.frames, now.step_ms, joint);
                    eprintln!(
                        "cap {:<13} level {level} joint {joint}: before #8201 {swing} counts, {speed:.0}/s; level {} {top_swing}, {top_speed:.0}/s; now {now_swing}, {now_speed:.0}/s",
                        clip.name,
                        level + 1
                    );
                    assert!(
                        now_swing <= top_swing.min(swing) && now_speed <= top_speed.min(speed),
                        "{} at level {level}, joint {joint}",
                        clip.name
                    );
                    held.push(format!("{} {level} {joint}", clip.name));
                }
            }
        }
        // Where the cap holds: clip, level, joint (pinned, so a change is said).
        let pinned: [&str; 0] = [];
        assert_eq!(held, pinned);
        for clip in crate::clips::ALL {
            for level in 1..=4u8 {
                let (here, above) = (livelier(clip, level), livelier(clip, level + 1));
                let stretch = &above.frames[cut_frames(clip, level + 1)];
                for joint in 0..5 {
                    let (swing, speed) = how_far_and_fast(here.frames, here.step_ms, joint);
                    let (top_swing, top_speed) = how_far_and_fast(stretch, above.step_ms, joint);
                    assert!(
                        swing <= top_swing && speed <= top_speed,
                        "{} joint {joint}: level {level} {swing}, {speed:.0}/s; level {} {top_swing}, {top_speed:.0}/s",
                        clip.name,
                        level + 1
                    );
                }
            }
        }
    }
}
