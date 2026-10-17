//! Whether a face is speaking, from the five landmarks YuNet already gives
//! (#720): the eyes, the nose tip and the two corners of the mouth. No new
//! model, no picture kept: one number a look per face.
//!
//! The mouth's shape is its width over the distance between the eyes.
//! Moving, coming closer, turning, tilting and nodding leave it as it is
//! (a nod shortens what is up and down, and both are across); a speaking
//! mouth widens and narrows as the jaw opens and the lips round. The score
//! is how much it changes from one look to the next over the last
//! [`WINDOW_US`], less what the marks' own jitter explains, read from how much
//! the eye line bends from look to look, which neither head motion nor
//! speaking does.
//!
//! The mouth's drop below the eye line is not used: with these five points
//! a nod shortens it as the jaw opening lengthens it, and the nose tip,
//! which stands out from the face, barely moves under a nod at the tilt of
//! someone looking down at a lamp (about 25 degrees). YuNet has no lip
//! points, so the opening itself is not seen. On a small face the width's
//! change is about as much as the marks jitter, so a face whose eyes are
//! less than [`LEAST_EYES_PX`] apart, turned more than [`MOST_TURN_DEG`],
//! too new, or with jittering marks is `unknown`, never `no`.
use lamp_msgs::{Speaking, SpeakingState};
use std::collections::{HashMap, VecDeque};

/// Where the eyes read whether to judge speaking. `on` only; with
/// `LAMP_BUS=on`, since it is published there.
pub const ENVIRONMENT: &str = "LAMP_EYES_SPEAKING";
/// How far back a face's mouth is read: five to eight looks at the eyes'
/// pace while they follow a face (250 ms, up to 350 ms on a 10 fps camera,
/// `main.rs`). With `--steady-rate` a still face is looked at every 500 ms,
/// four or five looks a window: mostly `unknown`, and right in under half
/// the rest (docs/eyes.md).
pub const WINDOW_US: u64 = 2_000_000;
/// The looks a window needs before it says anything.
pub const LEAST_LOOKS: usize = 5;
/// Eyes closer than this, in the detector's pixels, have mouth corners whose
/// jitter is as large as the lips' movement.
pub const LEAST_EYES_PX: f32 = 24.0;
/// A face turned further than this hides one corner behind the other.
pub const MOST_TURN_DEG: f32 = 30.0;
/// The eye line bending by more than this, in radians from one look to the
/// next (root mean square), is landmarks jittering, not a head.
pub const MOST_JITTER: f32 = 0.1;
/// The score, in eye distances a look, from which a mouth is speaking.
pub const SPEAKING_AT: f32 = 0.02;
/// The marks' own jitter moves the mouth's width, look to look, this share
/// of how much it bends the eye line (squared, from the geometry of the
/// four points: (4 + 4 x 0.8^2) / 12, with the mouth 0.8 eye distances wide).
const JITTER_TO_SHAPE: f32 = 0.55;
/// A face not seen for this long is forgotten.
const FORGET_US: u64 = 3_000_000;

/// Whether the environment asks for it.
pub fn from_environment() -> Option<Mouths> {
    (std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")).then(Mouths::default)
}

/// The mouth's width over the distance between the eyes, that distance in
/// pixels, and the eye line's angle. `None` if the eyes sit on one point.
pub fn shape(marks: &[(f32, f32); 5]) -> Option<(f32, f32, f32)> {
    let (eye, other, left, right) = (marks[0], marks[1], marks[3], marks[4]);
    let eyes = (other.0 - eye.0).hypot(other.1 - eye.1);
    if !eyes.is_finite() || eyes < 1e-3 {
        return None;
    }
    let width = (right.0 - left.0).hypot(right.1 - left.1) / eyes;
    Some((width, eyes, (other.1 - eye.1).atan2(other.0 - eye.0)))
}

#[derive(Debug)]
struct Mouth {
    /// When, the shape, the eye distance in pixels and the eye line's angle.
    looks: VecDeque<(u64, f32, f32, f32)>,
    /// The eye line's squared bend, averaged over this face's looks (about
    /// the last ten): its jitter at rest, steadier than one window's.
    rest: Option<f32>,
}

/// Each face's recent mouth, by its passing id.
#[derive(Debug, Default)]
pub struct Mouths {
    faces: HashMap<u32, Mouth>,
}

const UNKNOWN: Speaking = Speaking {
    state: SpeakingState::Unknown,
    score: 0.0,
};

impl Mouths {
    /// This look of the face `id`: its marks in the detector's pixels and
    /// how far it is turned, in degrees, if that was measured.
    pub fn judge(
        &mut self,
        id: u32,
        marks: &[(f32, f32); 5],
        turn_deg: Option<f32>,
        now_us: u64,
    ) -> Speaking {
        self.faces.retain(|_, mouth| {
            mouth
                .looks
                .back()
                .is_some_and(|last| now_us.saturating_sub(last.0) <= FORGET_US)
        });
        let Some((shape, eyes, angle)) = shape(marks) else {
            return UNKNOWN;
        };
        let mouth = self.faces.entry(id).or_insert(Mouth {
            looks: VecDeque::new(),
            rest: None,
        });
        mouth.looks.push_back((now_us, shape, eyes, angle));
        while mouth
            .looks
            .front()
            .is_some_and(|first| now_us.saturating_sub(first.0) > WINDOW_US)
        {
            mouth.looks.pop_front();
        }
        let looks = &mouth.looks;
        let turned = turn_deg.is_none_or(|degrees| degrees > MOST_TURN_DEG);
        let small =
            looks.iter().map(|look| look.2).sum::<f32>() / (looks.len() as f32) < LEAST_EYES_PX;
        if turned || small || looks.len() < LEAST_LOOKS {
            return UNKNOWN;
        }
        let pairs = looks.iter().zip(looks.iter().skip(1));
        let steps = (looks.len() - 1) as f32;
        // How much the eye line bends from look to look: turning, nodding
        // and coming closer leave its angle as it is, a tilting head bends
        // it only slowly, and speaking does not move the eyes, so this is
        // the marks' own jitter, in this window.
        let bends = looks
            .iter()
            .zip(looks.iter().skip(1))
            .zip(looks.iter().skip(2));
        let jitter = (bends
            .map(|((a, b), c)| (a.3 - 2.0 * b.3 + c.3).powi(2))
            .sum::<f32>()
            / (looks.len() - 2) as f32)
            .sqrt();
        if jitter > MOST_JITTER {
            return UNKNOWN;
        }
        let change = pairs.map(|(a, b)| (b.1 - a.1).powi(2)).sum::<f32>() / steps;
        // What the jitter explains of the width's change is taken off.
        let rest = mouth
            .rest
            .map_or(jitter.powi(2), |rest| rest + 0.1 * (jitter.powi(2) - rest));
        mouth.rest = Some(rest);
        let score = (change - JITTER_TO_SHAPE * rest).max(0.0).sqrt();
        let state = if score >= SPEAKING_AT {
            SpeakingState::Yes
        } else {
            SpeakingState::No
        };
        Speaking { state, score }
    }
}

#[cfg(test)]
mod tests {
    //! Synthetic faces: five points of a head in millimetres, turned,
    //! nodded, moved and scaled, seen every 250 to 350 ms (the eyes' pace while
    //! they follow a face, `main.rs`), with the marks jittering as a detector's
    //! do, and a mouth that is no pure tone.
    use super::*;
    use crate::facing;
    use std::f32::consts::TAU;

    /// Eye distance of an adult, millimetres.
    const EYES_MM: f32 = 63.0;
    const LOOK_US: u64 = 300_000;

    struct Noise(u64);
    impl Noise {
        fn uniform(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 40) as f32 + 0.5) / (1u64 << 24) as f32
        }
        fn gauss(&mut self) -> f32 {
            (-2.0 * self.uniform().ln()).sqrt() * (TAU * self.uniform()).cos()
        }
    }

    #[derive(Clone, Copy)]
    struct Scene {
        /// Eye distance in detector pixels, and its growth a second.
        eyes_px: f32,
        grow: f32,
        /// Turn and nod, amplitude in degrees and frequency.
        yaw: (f32, f32),
        pitch: (f32, f32),
        /// The head tipped down this far, degrees, as at a lamp below the eyes.
        tilt: f32,
        /// Sideways drift, pixels a second.
        drift: f32,
        /// Whether the mouth speaks: syllables at 3 to 6.5 Hz, louder and
        /// softer, the lips rounding at about 3 Hz.
        talk: bool,
        /// Marks' jitter, pixels, one standard deviation.
        jitter: f32,
        /// Time between looks, microseconds, drawn evenly from this range.
        pace: (u64, u64),
    }

    const STILL: Scene = Scene {
        eyes_px: 60.0,
        grow: 0.0,
        yaw: (0.0, 0.0),
        pitch: (0.0, 0.0),
        drift: 0.0,
        talk: false,
        jitter: 0.3,
        tilt: 0.0,
        pace: (250_000, 350_000),
    };

    fn marks(scene: &Scene, t: f32, noise: &mut Noise) -> [(f32, f32); 5] {
        let open = if scene.talk {
            let syllables = TAU * 4.7 * t + 2.0 * (TAU * 0.9 * t).sin();
            5.0 * (0.6 + 0.4 * (TAU * 1.3 * t).sin()) * (1.0 - syllables.cos())
        } else {
            0.0
        };
        let round = if scene.talk {
            2.5 * (TAU * 3.1 * t + (TAU * 0.6 * t).sin()).sin() - 0.15 * open
        } else {
            0.0
        };
        let half = EYES_MM / 2.0;
        let head = [
            [-half, 0.0, 0.0],
            [half, 0.0, 0.0],
            [0.0, 38.0, -18.0],
            [-25.0 + round, 70.0 + 0.3 * open, -2.0],
            [25.0 - round, 70.0 + 0.3 * open, -2.0],
        ];
        let yaw = (scene.yaw.0 * (TAU * scene.yaw.1 * t).sin()
            + if scene.yaw.1 == 0.0 { scene.yaw.0 } else { 0.0 })
        .to_radians();
        let pitch = (scene.tilt + scene.pitch.0 * (TAU * scene.pitch.1 * t).sin()).to_radians();
        let scale = (scene.eyes_px + scene.grow * t) / EYES_MM;
        let (cx, cy) = (160.0 + scene.drift * t, 110.0);
        head.map(|[x, y, z]| {
            let (x, z) = (
                x * yaw.cos() + z * yaw.sin(),
                -x * yaw.sin() + z * yaw.cos(),
            );
            let y = y * pitch.cos() - z * pitch.sin();
            (
                cx + scale * x + scene.jitter * noise.gauss(),
                cy + scale * y + scene.jitter * noise.gauss(),
            )
        })
    }

    /// The states judged over 12 s, after the first window.
    fn run(scene: Scene, seed: u64) -> Vec<Speaking> {
        let (mut mouths, mut noise) = (Mouths::default(), Noise(seed));
        let (least, most) = scene.pace;
        let mut times = vec![0u64];
        while let Some(&last) = times.last().filter(|&&last| last < 12_000_000) {
            times.push(last + least + ((most - least) as f32 * noise.uniform()) as u64);
        }
        times
            .into_iter()
            .map(|at| {
                let seen = marks(&scene, at as f32 / 1e6, &mut noise);
                (at, mouths.judge(1, &seen, facing::yaw_deg(&seen), at))
            })
            .filter(|(at, _)| *at >= WINDOW_US)
            .map(|(_, judged)| judged)
            .collect()
    }

    fn share(scene: Scene, state: SpeakingState) -> f32 {
        let judged: Vec<Speaking> = (1..=5).flat_map(|seed| run(scene, seed)).collect();
        judged.iter().filter(|said| said.state == state).count() as f32 / judged.len() as f32
    }

    fn mostly(scene: Scene, state: SpeakingState, what: &str) {
        let right = share(scene, state);
        assert!(
            right >= 0.9,
            "{what}: {state:?} in {:.0}% of looks",
            right * 100.0
        );
    }

    #[test]
    fn a_still_face_is_not_speaking() {
        mostly(STILL, SpeakingState::No, "still");
    }

    #[test]
    fn a_nodding_face_is_not_speaking() {
        mostly(
            Scene {
                pitch: (12.0, 0.8),
                ..STILL
            },
            SpeakingState::No,
            "nodding",
        );
    }

    /// Someone looking down at the lamp, nodding as they listen.
    #[test]
    fn a_face_nodding_at_a_tilt_is_not_speaking() {
        mostly(
            Scene {
                tilt: 20.0,
                pitch: (12.0, 0.8),
                ..STILL
            },
            SpeakingState::No,
            "nodding at a tilt",
        );
    }

    #[test]
    fn a_face_turning_from_side_to_side_is_not_speaking() {
        mostly(
            Scene {
                yaw: (20.0, 0.4),
                ..STILL
            },
            SpeakingState::No,
            "turning",
        );
    }

    #[test]
    fn a_talking_face_is_speaking() {
        mostly(
            Scene {
                talk: true,
                ..STILL
            },
            SpeakingState::Yes,
            "talking",
        );
    }

    #[test]
    fn a_face_talking_while_it_moves_is_speaking() {
        let moving = Scene {
            talk: true,
            yaw: (10.0, 0.3),
            pitch: (5.0, 0.5),
            drift: 30.0,
            ..STILL
        };
        mostly(moving, SpeakingState::Yes, "talking and moving");
    }

    /// Fails if the shape is not taken over the eye distance: a still face
    /// coming closer and drifting across changes every distance in pixels.
    #[test]
    fn a_face_coming_closer_is_not_speaking() {
        mostly(
            Scene {
                grow: 10.0,
                drift: 40.0,
                ..STILL
            },
            SpeakingState::No,
            "coming closer",
        );
    }

    #[test]
    fn a_small_turned_or_jittering_face_is_unknown() {
        mostly(
            Scene {
                eyes_px: 18.0,
                talk: true,
                ..STILL
            },
            SpeakingState::Unknown,
            "small",
        );
        mostly(
            Scene {
                yaw: (55.0, 0.0),
                talk: true,
                ..STILL
            },
            SpeakingState::Unknown,
            "turned",
        );
        mostly(
            Scene {
                jitter: 5.0,
                ..STILL
            },
            SpeakingState::Unknown,
            "jittering",
        );
    }

    /// A face too narrow to measure a turn on, or with its marks off the
    /// picture, has no turn from `bus_out`: it is `unknown`, even talking.
    #[test]
    fn a_face_whose_turn_was_not_measured_is_unknown() {
        let (mut mouths, mut noise) = (Mouths::default(), Noise(1));
        let talking = Scene {
            talk: true,
            ..STILL
        };
        for look in 0..20u64 {
            let seen = marks(&talking, look as f32 * 0.3, &mut noise);
            let said = mouths.judge(3, &seen, None, look * LOOK_US);
            assert_eq!(said.state, SpeakingState::Unknown, "look {look}");
        }
    }

    #[test]
    fn a_new_face_is_unknown_until_its_window_fills() {
        let (mut mouths, mut noise) = (Mouths::default(), Noise(1));
        for look in 0..(LEAST_LOOKS as u64 - 1) {
            let seen = marks(&STILL, look as f32 * 0.3, &mut noise);
            let said = mouths.judge(7, &seen, Some(0.0), look * LOOK_US);
            assert_eq!(said.state, SpeakingState::Unknown);
        }
    }

    /// The margins, by face size and jitter: `cargo test -p lamp-eyes
    /// speaking::tests::margins -- --nocapture` prints the share of looks
    /// judged right for a still face and a talking one.
    #[test]
    fn margins() {
        for pace in [(250_000, 350_000), (450_000, 550_000)] {
            for jitter in [0.3, 0.5, 1.0] {
                for eyes_px in [24.0, 30.0, 40.0, 60.0] {
                    let still = share(
                        Scene {
                            eyes_px,
                            jitter,
                            pace,
                            ..STILL
                        },
                        SpeakingState::No,
                    );
                    let talk = share(
                        Scene {
                            eyes_px,
                            jitter,
                            pace,
                            talk: true,
                            ..STILL
                        },
                        SpeakingState::Yes,
                    );
                    eprintln!(
                        "pace {pace:?} us, jitter {jitter} px, eyes {eyes_px} px: still no {still:.2}, talking yes {talk:.2}"
                    );
                    // What docs/eyes.md rests on: at the eyes' pace while they
                    // follow a face it holds (both at least 0.9) only at 0.3 px of
                    // jitter and 60 px between the eyes; 0.3 px at 40 px and 0.5 px
                    // at 60 px fall just short; at --steady-rate's 500 ms nowhere.
                    let holds = still >= 0.9 && talk >= 0.9;
                    let expected = pace.0 < 300_000 && (jitter, eyes_px as u32) == (0.3, 60);
                    assert_eq!(
                        holds, expected,
                        "pace {pace:?}, jitter {jitter} px, eyes {eyes_px} px"
                    );
                }
            }
        }
    }
}
