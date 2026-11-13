//! Gestures: short recorded moves of the whole lamp, played on top of the
//! pose a standing lamp is in. A nod, a shake of the head, a greeting.
//!
//! A clip is frames of offsets in encoder counts for head pitch, head roll,
//! base yaw, shoulder and elbow, a fixed time apart. It begins and ends at
//! nothing, so it can be laid over whatever the lamp is doing and leaves it
//! where it was. The clips here were recorded on the old runtime
//! (`hal/recordings/`) and made smaller and slower for this body; `data.rs`
//! is generated from them. Pure data and arithmetic: when one may play, how
//! far a joint may be taken and what is watched while it plays belong to
//! the engine that asks.

#[rustfmt::skip]
mod data;
pub use data::*;

/// One recorded gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clip {
    pub name: &'static str,
    /// Time from one frame to the next.
    pub step_ms: u16,
    /// Head pitch, head roll, base yaw, shoulder, elbow: counts from the
    /// pose the lamp is in. The first and the last frame are nothing.
    pub frames: &'static [[i16; 5]],
}

impl Clip {
    pub fn duration_us(&self) -> u64 {
        (self.frames.len().saturating_sub(1) as u64) * u64::from(self.step_ms) * 1_000
    }

    /// The offsets this long after the clip began, between its frames, or
    /// none once it is over.
    pub fn at(&self, elapsed_us: u64) -> Option<[f32; 5]> {
        if elapsed_us >= self.duration_us() {
            return None;
        }
        let step_us = u64::from(self.step_ms) * 1_000;
        let frame = (elapsed_us / step_us) as usize;
        let share = (elapsed_us % step_us) as f32 / step_us as f32;
        Some(self.between(frame, share))
    }

    /// `share` of the way from frame `frame` to the next.
    fn between(&self, frame: usize, share: f32) -> [f32; 5] {
        let (from, to) = (self.frames[frame], self.frames[frame + 1]);
        std::array::from_fn(|joint| {
            f32::from(from[joint]) + (f32::from(to[joint]) - f32::from(from[joint])) * share
        })
    }

    /// The time from frame `frame` to the next, stretched where any joint
    /// would go faster than `ceiling` counts a second, so that none does;
    /// a step under it keeps its time (V1's stretch, #6893).
    fn step_within_us(&self, frame: usize, ceiling: f32) -> u64 {
        let (from, to) = (self.frames[frame], self.frames[frame + 1]);
        let farthest = (0..5)
            .map(|joint| (i32::from(to[joint]) - i32::from(from[joint])).unsigned_abs())
            .max()
            .unwrap_or(0);
        let needed = (f64::from(farthest) / f64::from(ceiling.max(1.0)) * 1e6).ceil() as u64;
        (u64::from(self.step_ms) * 1_000).max(needed)
    }

    /// [`Self::duration_us`] with each step stretched to `ceiling`
    /// (`LAMP_BODY_ENERGY`); `None` is the clip as recorded.
    pub fn duration_within_us(&self, ceiling: Option<f32>) -> u64 {
        match ceiling {
            None => self.duration_us(),
            Some(ceiling) => (0..self.frames.len().saturating_sub(1))
                .map(|frame| self.step_within_us(frame, ceiling))
                .sum(),
        }
    }

    /// [`Self::at`] with each step stretched to `ceiling`
    /// (`LAMP_BODY_ENERGY`); `None` is the clip as recorded.
    pub fn at_within(&self, elapsed_us: u64, ceiling: Option<f32>) -> Option<[f32; 5]> {
        let Some(ceiling) = ceiling else {
            return self.at(elapsed_us);
        };
        let mut left = elapsed_us;
        for frame in 0..self.frames.len().saturating_sub(1) {
            let step_us = self.step_within_us(frame, ceiling);
            if left < step_us {
                return Some(self.between(frame, left as f32 / step_us as f32));
            }
            left -= step_us;
        }
        None
    }
}

/// A glance for a call with nobody in view: the base turns a little to one
/// side and a little to the other and back, in about a second, as a person
/// called from somewhere they cannot see looks round once and lets it go.
/// Some 60 counts (five degrees) each way. Not one of [`ALL`]: the body
/// plays it for a call (`Limits::look_at_name`), nobody asks for it by name.
pub const CALLED: Clip = Clip {
    name: "called",
    step_ms: 100,
    frames: &[
        [0, 0, 0, 0, 0],
        [0, 0, 25, 0, 0],
        [0, 0, 50, 0, 0],
        [0, 0, 60, 0, 0],
        [0, 0, 50, 0, 0],
        [0, 0, 0, 0, 0],
        [0, 0, -50, 0, 0],
        [0, 0, -60, 0, 0],
        [0, 0, -50, 0, 0],
        [0, 0, -25, 0, 0],
        [0, 0, 0, 0, 0],
    ],
};

/// Small moves of the head alone for a lamp at rest to shift by now and
/// then (`crate::idle`): eased out over a second, held a second, eased back.
/// Inside the reach of a glance at rest (`body::REST_GLANCE_REACH`, about
/// four degrees of pitch and two of roll); the base and the arm are not
/// moved. Not in [`ALL`]: nobody asks for them by name. A lift of the head
/// is a pitch below nothing, as in `IDLE`.
pub const TILT_LEFT: Clip = Clip {
    name: "tilt_left",
    step_ms: 200,
    frames: &tilt(-1, -10),
};
pub const TILT_RIGHT: Clip = Clip {
    name: "tilt_right",
    step_ms: 200,
    frames: &tilt(1, -10),
};
pub const LOOK_UP: Clip = Clip {
    name: "look_up",
    step_ms: 200,
    frames: &tilt(0, -50),
};
/// A small lowering of the head and back, as a breath let out: a third move
/// that needs no roll, so a head that does not tilt standing has more than
/// two to shift by.
pub const DIP: Clip = Clip {
    name: "dip",
    step_ms: 200,
    frames: &tilt(0, 40),
};

/// Eased out, held and eased back: roll `side` times 25 counts, pitch `lift`.
const fn tilt(side: i16, lift: i16) -> [[i16; 5]; 17] {
    const EASE: [i16; 17] = [
        0, 8, 28, 52, 76, 92, 100, 100, 100, 100, 100, 92, 76, 52, 28, 8, 0,
    ];
    let mut frames = [[0i16; 5]; 17];
    let mut index = 0;
    while index < 17 {
        frames[index][0] = lift * EASE[index] / 100;
        frames[index][1] = side * 25 * EASE[index] / 100;
        index += 1;
    }
    frames
}

/// The clip of this name.
pub fn named(name: &str) -> Option<&'static Clip> {
    ALL.into_iter().find(|clip| clip.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fastest any clip asks each joint to go, counts a second, and the
    /// farthest it takes each from the pose: what the generator promises.
    const FASTEST: [f32; 5] = [900.0, 1_300.0, 1_000.0, 800.0, 800.0];
    const FARTHEST: [i16; 5] = [200, 200, 150, 350, 350];

    #[test]
    fn every_clip_begins_and_ends_at_nothing_and_stays_inside_its_speed_and_size() {
        assert!(ALL.len() >= 4);
        for clip in ALL {
            assert_eq!(named(clip.name), Some(clip));
            assert!(
                clip.frames.len() >= 2 && clip.step_ms >= 40,
                "{}",
                clip.name
            );
            assert_eq!(clip.frames[0], [0; 5], "{}", clip.name);
            assert_eq!(clip.frames[clip.frames.len() - 1], [0; 5], "{}", clip.name);
            assert!(
                (1_500_000..=10_000_000).contains(&clip.duration_us()),
                "{}",
                clip.name
            );
            let step = f32::from(clip.step_ms) / 1_000.0;
            for pair in clip.frames.windows(2) {
                for joint in 0..5 {
                    // A count of rounding either way at each end.
                    let speed =
                        (f32::from(pair[1][joint]) - f32::from(pair[0][joint])).abs() / step;
                    assert!(
                        speed <= FASTEST[joint] + 2.0 / step,
                        "{} joint {joint}: {speed}",
                        clip.name
                    );
                    assert!(
                        pair[1][joint].abs() <= FARTHEST[joint],
                        "{} joint {joint}",
                        clip.name
                    );
                }
            }
        }
        assert_eq!(named("dance"), None);
    }

    #[test]
    fn the_small_moves_at_rest_move_the_head_alone_inside_a_glance_at_rest() {
        let reach = crate::body::REST_GLANCE_REACH;
        for clip in [&TILT_LEFT, &TILT_RIGHT, &LOOK_UP, &DIP] {
            assert_eq!(named(clip.name), None);
            assert_eq!(clip.duration_us(), 3_200_000, "{}", clip.name);
            assert_eq!(clip.frames[0], [0; 5]);
            assert_eq!(clip.frames[clip.frames.len() - 1], [0; 5]);
            for frame in clip.frames {
                assert!(f32::from(frame[0].abs()) <= reach[0], "{}", clip.name);
                assert!(f32::from(frame[1].abs()) <= reach[1], "{}", clip.name);
                assert_eq!(frame[2..], [0; 3], "{}", clip.name);
            }
        }
        assert_eq!(TILT_LEFT.frames[8], [-10, -25, 0, 0, 0]);
        assert_eq!(TILT_RIGHT.frames[8], [-10, 25, 0, 0, 0]);
        assert_eq!(LOOK_UP.frames[8], [-50, 0, 0, 0, 0]);
        assert_eq!(DIP.frames[8], [40, 0, 0, 0, 0]);
    }

    #[test]
    fn a_clip_is_read_between_its_frames_and_is_over_at_its_end() {
        const TWO: Clip = Clip {
            name: "two",
            step_ms: 100,
            frames: &[[0, 0, 0, 0, 0], [10, -20, 0, 40, 0], [0, 0, 0, 0, 0]],
        };
        assert_eq!(TWO.duration_us(), 200_000);
        assert_eq!(TWO.at(0), Some([0.0; 5]));
        assert_eq!(TWO.at(50_000), Some([5.0, -10.0, 0.0, 20.0, 0.0]));
        assert_eq!(TWO.at(100_000), Some([10.0, -20.0, 0.0, 40.0, 0.0]));
        assert_eq!(TWO.at(150_000), Some([5.0, -10.0, 0.0, 20.0, 0.0]));
        assert_eq!(TWO.at(200_000), None);
        assert_eq!(TWO.at(u64::MAX), None);
    }

    /// `LAMP_BODY_ENERGY` (#6893): a step of head roll at `FASTEST`'s
    /// 1,300 counts a second is stretched to the servo's 1,000; a step under
    /// it keeps its time; no ceiling is the clip as recorded.
    #[test]
    fn a_step_over_the_servos_speed_is_stretched_to_it_and_one_under_it_is_not() {
        const ROLL: Clip = Clip {
            name: "roll",
            step_ms: 100,
            frames: &[[0, 0, 0, 0, 0], [0, 130, 0, 0, 0], [0, 130, 50, 0, 0]],
        };
        let ceiling = Some(1_000.0);
        assert_eq!(ROLL.duration_within_us(None), ROLL.duration_us());
        assert_eq!(ROLL.duration_within_us(ceiling), 130_000 + 100_000);
        for elapsed in (0..=300_000).step_by(5_000) {
            assert_eq!(ROLL.at_within(elapsed, None), ROLL.at(elapsed));
        }
        assert_eq!(
            ROLL.at_within(65_000, ceiling),
            Some([0.0, 65.0, 0.0, 0.0, 0.0])
        );
        assert_eq!(
            ROLL.at_within(130_000, ceiling),
            Some([0.0, 130.0, 0.0, 0.0, 0.0])
        );
        assert_eq!(
            ROLL.at_within(180_000, ceiling),
            Some([0.0, 130.0, 25.0, 0.0, 0.0])
        );
        assert_eq!(ROLL.at_within(230_000, ceiling), None);
        // Played at a 20 ms tick, no joint goes faster than the ceiling, and
        // every built-in clip comes out at least as long, ending at nothing.
        for clip in ALL.into_iter().chain([&ROLL]) {
            let mut last = [0.0f32; 5];
            for elapsed in (0..clip.duration_within_us(ceiling)).step_by(20_000) {
                let now = clip.at_within(elapsed, ceiling).unwrap();
                for joint in 0..5 {
                    let speed = (now[joint] - last[joint]).abs() / 0.02;
                    assert!(speed <= 1_000.5, "{} joint {joint}: {speed}", clip.name);
                }
                last = now;
            }
            assert!(clip.duration_within_us(ceiling) >= clip.duration_us());
        }
    }

    /// docs/control.md's gestures are the body's clips but `idle`, the
    /// lamp's own life, and `scanning`, a sweep rule 8 keeps for a person's
    /// ask: a clip added here is either named there or left out on purpose.
    #[test]
    fn the_control_schemas_gestures_are_the_clips_but_idle_and_scanning() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../docs/control.schema.json")).unwrap();
        let named: Vec<&str> = schema["$defs"]["gesture"]["examples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect();
        let offered: Vec<&str> = ALL
            .iter()
            .map(|clip| clip.name)
            .filter(|name| !["idle", "scanning"].contains(name))
            .collect();
        assert_eq!(named, offered);
    }
}
