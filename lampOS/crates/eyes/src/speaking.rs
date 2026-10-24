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
use lamp_msgs::{Speaking, SpeakingFrom, SpeakingState};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};

/// Where the eyes read whether to judge speaking. `on` only; with
/// `LAMP_BUS=on`, since it is published there.
pub const ENVIRONMENT: &str = "LAMP_EYES_SPEAKING";
/// With `LAMP_EYES_SPEAKING=on` too, `on` drops from a face's window the one
/// look whose eye distance is off its window's median by more than
/// [`OUTLIER_SHARE`] (a landmark misread, as one look at 14 px among 28 to
/// 33 px on lamp-52e6), instead of letting it void the window. Off: as before.
pub const OUTLIER_ENVIRONMENT: &str = "LAMP_EYES_SPEAKING_DROP_OUTLIER";
/// A look whose eye distance is this share or more off the window's median
/// is an outlier.
pub const OUTLIER_SHARE: f32 = 0.3;
/// The outliers a window may drop; one more and it is `unknown`.
pub const MOST_OUTLIERS: usize = 1;
/// How far back a face's mouth is read: five to eight looks at the eyes'
/// pace while they follow a face (250 ms, up to 350 ms on a 10 fps camera,
/// `main.rs`). With `--steady-rate` a still face is looked at 500 to 600 ms
/// apart, four looks a window, under `LEAST_LOOKS`: in practice `unknown`
/// (docs/eyes.md).
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
/// With Face Mesh V2 read on every look of the window (`--mouth-model`,
/// #720): the score is how much `mouth_open` (the inner lips' gap over the
/// face's height) changes from one look to the next, and from this it is
/// speaking. On a recorded talking face cut as the lamp sees it (#1470,
/// 4 looks a second), 95% of 2 s windows of talk scored 0.015 or more, at
/// 26 and at 51 px between the eyes alike. A still silent face on the
/// lamp's camera is not measured yet: this bar is to be checked against it.
pub const MESH_SPEAKING_AT: f32 = 0.012;
/// The marks' own jitter moves the mouth's width, look to look, this share
/// of how much it bends the eye line (squared, from the geometry of the
/// four points: (4 + 4 x 0.8^2) / 12, with the mouth 0.8 eye distances wide).
const JITTER_TO_SHAPE: f32 = 0.55;
/// A face not seen for this long is forgotten.
const FORGET_US: u64 = 3_000_000;

/// Whether the environment asks for it.
pub fn from_environment() -> Option<Mouths> {
    (std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")).then(|| Mouths {
        drop_outliers: std::env::var(OUTLIER_ENVIRONMENT).ok().as_deref() == Some("on"),
        ..Mouths::default()
    })
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
    /// When, the shape, the eye distance in pixels, the eye line's angle
    /// and the mesh's `mouth_open`, if it was read.
    looks: VecDeque<(u64, f32, f32, f32, Option<f32>)>,
    /// The eye line's squared bend, averaged over this face's looks (about
    /// the last ten): its jitter at rest, steadier than one window's.
    rest: Option<f32>,
}

/// Each face's recent mouth, by its passing id.
#[derive(Debug, Default)]
pub struct Mouths {
    faces: HashMap<u32, Mouth>,
    /// [`OUTLIER_ENVIRONMENT`].
    drop_outliers: bool,
}

const UNKNOWN: Speaking = Speaking {
    state: SpeakingState::Unknown,
    score: 0.0,
    from: None,
};

/// The branch that returned this decision. Earlier gates take precedence;
/// a metric that was not reached remains null rather than being recomputed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    InvalidEyes,
    TurnUnavailable,
    Turned,
    Small,
    /// More looks off the window's median eye distance than it may drop
    /// (`LAMP_EYES_SPEAKING_DROP_OUTLIER`).
    Outliers,
    TooFewLooks,
    Jitter,
    Mesh,
    Corners,
}

/// Numeric, ephemeral diagnostics only: no face id, marks, image or name.
#[derive(Clone, Debug, Serialize)]
pub struct MouthDiagnostic {
    pub eyes_px: Option<f32>,
    pub window_mean_eyes_px: Option<f32>,
    pub window_looks: Option<usize>,
    pub window_span_us: Option<u64>,
    pub turn_deg: Option<f32>,
    pub jitter_rad: Option<f32>,
    pub reason: Reason,
    pub mesh_current: bool,
    pub mesh_window_looks: Option<usize>,
    pub mouth_open: Option<f32>,
    /// This valid look begins a new window; invalid looks never claim a reset.
    pub window_reset: bool,
    pub speaking: Speaking,
}

impl Mouths {
    /// This look of the face `id`: its marks in the detector's pixels, how
    /// far it is turned, in degrees, if that was measured, and how open the
    /// mesh read its mouth, if it was read ([`MESH_SPEAKING_AT`]).
    pub fn judge(
        &mut self,
        id: u32,
        marks: &[(f32, f32); 5],
        turn_deg: Option<f32>,
        open: Option<f32>,
        now_us: u64,
    ) -> Speaking {
        self.judge_observed(id, marks, turn_deg, open, now_us, None)
    }

    /// The same state transition and gates as `judge`, with an opt-in record.
    pub fn judge_with_diagnostics(
        &mut self,
        id: u32,
        marks: &[(f32, f32); 5],
        turn_deg: Option<f32>,
        open: Option<f32>,
        now_us: u64,
    ) -> (Speaking, MouthDiagnostic) {
        let mut diagnostic = MouthDiagnostic {
            eyes_px: None,
            window_mean_eyes_px: None,
            window_looks: None,
            window_span_us: None,
            turn_deg,
            jitter_rad: None,
            reason: Reason::InvalidEyes,
            mesh_current: open.is_some(),
            mesh_window_looks: None,
            mouth_open: open,
            window_reset: false,
            speaking: UNKNOWN,
        };
        let result = self.judge_observed(id, marks, turn_deg, open, now_us, Some(&mut diagnostic));
        diagnostic.speaking = result;
        (result, diagnostic)
    }

    fn judge_observed(
        &mut self,
        id: u32,
        marks: &[(f32, f32); 5],
        turn_deg: Option<f32>,
        open: Option<f32>,
        now_us: u64,
        mut diagnostic: Option<&mut MouthDiagnostic>,
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
        mouth.looks.push_back((now_us, shape, eyes, angle, open));
        while mouth
            .looks
            .front()
            .is_some_and(|first| now_us.saturating_sub(first.0) > WINDOW_US)
        {
            mouth.looks.pop_front();
        }
        // With the switch, a look whose eye distance is off the window's
        // median by OUTLIER_SHARE or more is left out; off, none is.
        let median = self.drop_outliers.then(|| {
            let mut eyes: Vec<f32> = mouth.looks.iter().map(|look| look.2).collect();
            eyes.sort_by(f32::total_cmp);
            eyes[eyes.len() / 2]
        });
        let looks: Vec<_> = mouth
            .looks
            .iter()
            .copied()
            .filter(|look| median.is_none_or(|m| (look.2 - m).abs() < OUTLIER_SHARE * m))
            .collect();
        let outliers = mouth.looks.len() - looks.len();
        let turned = turn_deg.is_none_or(|degrees| degrees > MOST_TURN_DEG);
        let mean_eyes = looks.iter().map(|look| look.2).sum::<f32>() / (looks.len().max(1) as f32);
        let small = mean_eyes < LEAST_EYES_PX;
        if let Some(detail) = diagnostic.as_deref_mut() {
            detail.eyes_px = Some(eyes);
            detail.window_mean_eyes_px = Some(mean_eyes);
            detail.window_looks = Some(mouth.looks.len());
            detail.window_span_us =
                Some(now_us.saturating_sub(mouth.looks.front().map_or(now_us, |look| look.0)));
            detail.window_reset = mouth.looks.len() == 1;
            detail.mesh_window_looks = Some(looks.iter().filter(|look| look.4.is_some()).count());
        }
        let refusal = if turned {
            Some(if turn_deg.is_none() {
                Reason::TurnUnavailable
            } else {
                Reason::Turned
            })
        } else if outliers > MOST_OUTLIERS {
            Some(Reason::Outliers)
        } else if small {
            Some(Reason::Small)
        } else if looks.len() < LEAST_LOOKS {
            Some(Reason::TooFewLooks)
        } else {
            None
        };
        if let Some(reason) = refusal {
            if let Some(detail) = diagnostic {
                detail.reason = reason;
            }
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
        if let Some(detail) = diagnostic.as_deref_mut() {
            detail.jitter_rad = Some(jitter);
        }
        if jitter > MOST_JITTER {
            if let Some(detail) = diagnostic {
                detail.reason = Reason::Jitter;
            }
            return UNKNOWN;
        }
        // The eye line's jitter at rest, fed on every judged look, so the
        // corners taking back over after the mesh have it up to date.
        let rest = mouth
            .rest
            .map_or(jitter.powi(2), |rest| rest + 0.1 * (jitter.powi(2) - rest));
        mouth.rest = Some(rest);
        // The mesh read every look of the window: its inner lips, not the
        // corners, and its own steadiness is not the eye line's.
        let opens: Option<Vec<f32>> = looks.iter().map(|look| look.4).collect();
        if let Some(opens) = opens {
            if let Some(detail) = diagnostic {
                detail.reason = Reason::Mesh;
            }
            let score = (opens
                .windows(2)
                .map(|two| (two[1] - two[0]).powi(2))
                .sum::<f32>()
                / steps)
                .sqrt();
            let state = if score >= MESH_SPEAKING_AT {
                SpeakingState::Yes
            } else {
                SpeakingState::No
            };
            return Speaking {
                state,
                score,
                from: Some(SpeakingFrom::Mesh),
            };
        }
        if let Some(detail) = diagnostic {
            detail.reason = Reason::Corners;
        }
        let change = pairs.map(|(a, b)| (b.1 - a.1).powi(2)).sum::<f32>() / steps;
        // What the jitter explains of the width's change is taken off.
        let score = (change - JITTER_TO_SHAPE * rest).max(0.0).sqrt();
        let state = if score >= SPEAKING_AT {
            SpeakingState::Yes
        } else {
            SpeakingState::No
        };
        Speaking {
            state,
            score,
            from: Some(SpeakingFrom::Corners),
        }
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
                (at, mouths.judge(1, &seen, facing::yaw_deg(&seen), None, at))
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
            let said = mouths.judge(3, &seen, None, None, look * LOOK_US);
            assert_eq!(said.state, SpeakingState::Unknown, "look {look}");
        }
    }

    #[test]
    fn a_new_face_is_unknown_until_its_window_fills() {
        let (mut mouths, mut noise) = (Mouths::default(), Noise(1));
        for look in 0..(LEAST_LOOKS as u64 - 1) {
            let seen = marks(&STILL, look as f32 * 0.3, &mut noise);
            let said = mouths.judge(7, &seen, Some(0.0), None, look * LOOK_US);
            assert_eq!(said.state, SpeakingState::Unknown);
        }
    }

    /// With the mesh read on every look (#720): a mouth whose inner-lip gap
    /// opens and shuts is speaking, one that holds still (the mesh's own
    /// jitter, a few thousandths) is not, whatever the corners do; and a
    /// window with one look the mesh missed is judged by the corners.
    #[test]
    fn with_the_mesh_the_inner_lips_decide() {
        let (mut talking, mut still, mut noise) = (Mouths::default(), Mouths::default(), Noise(5));
        for look in 0..20u64 {
            // The corners of a talking face, for both: only the mesh differs.
            let seen = marks(
                &Scene {
                    talk: true,
                    ..STILL
                },
                look as f32 * 0.3,
                &mut noise,
            );
            let gap = 0.04 + 0.03 * (look as f32 * 1.7).sin();
            let held = 0.01 + 0.003 * (noise.uniform() - 0.5);
            let said = talking.judge(1, &seen, Some(0.0), Some(gap), look * LOOK_US);
            let quiet = still.judge(2, &seen, Some(0.0), Some(held), look * LOOK_US);
            if look + 1 >= LEAST_LOOKS as u64 {
                assert_eq!(
                    said.state,
                    SpeakingState::Yes,
                    "talking, look {look}: {said:?}"
                );
                assert_eq!(
                    quiet.state,
                    SpeakingState::No,
                    "still, look {look}: {quiet:?}"
                );
            }
        }
        // One look in the window without the mesh: the corners judge. The
        // mesh would hold this mouth still (0.01 each look) and say no; the
        // talking corners say yes.
        let mut mixed = Mouths::default();
        let mut last = None;
        for look in 0..8u64 {
            let seen = marks(
                &Scene {
                    talk: true,
                    ..STILL
                },
                look as f32 * 0.3,
                &mut noise,
            );
            let open = (look != 5).then_some(0.01);
            last = Some(mixed.judge(3, &seen, Some(0.0), open, look * LOOK_US));
        }
        let last = last.unwrap();
        assert_eq!(last.state, SpeakingState::Yes, "{last:?}");
        assert_eq!(last.from, Some(SpeakingFrom::Corners));
    }

    /// The margins, by face size and jitter: `cargo test -p lamp-eyes
    /// speaking::tests::margins -- --nocapture` prints the share of looks
    /// judged right for a still face and a talking one.
    #[test]
    fn margins() {
        for pace in [(250_000, 350_000), (500_000, 600_000)] {
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
                    // at 60 px fall just short; at --steady-rate's 500 to 600 ms
                    // it is never wrong, only `unknown`.
                    if pace.0 >= 500_000 {
                        for talk in [false, true] {
                            let scene = Scene {
                                eyes_px,
                                jitter,
                                pace,
                                talk,
                                ..STILL
                            };
                            assert_eq!(share(scene, SpeakingState::Unknown), 1.0);
                        }
                    }
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

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    fn marks(eyes: f32, angle: f32) -> [(f32, f32); 5] {
        let (sin, cos) = angle.sin_cos();
        [
            (0.0, 0.0),
            (eyes * cos, eyes * sin),
            (eyes / 2.0, 10.0),
            (0.0, 20.0),
            (eyes, 20.0),
        ]
    }

    #[test]
    fn diagnostic_names_the_branch_that_actually_returned_the_verdict() {
        for (turn, eyes, expected) in [
            (None, 30.0, Reason::TurnUnavailable),
            (Some(31.0), 30.0, Reason::Turned),
            (Some(0.0), 23.75, Reason::Small),
            (Some(0.0), 30.0, Reason::TooFewLooks),
        ] {
            let (said, detail) =
                Mouths::default().judge_with_diagnostics(1, &marks(eyes, 0.0), turn, Some(0.0), 0);
            assert_eq!(said, UNKNOWN);
            assert_eq!(detail.reason, expected);
            assert_eq!(detail.eyes_px, Some(eyes));
            assert_eq!(detail.jitter_rad, None, "later gates are not evaluated");
            assert!(detail.window_reset);
        }
        for (mesh, changing, expected, state) in [
            (true, false, Reason::Mesh, SpeakingState::No),
            (true, true, Reason::Mesh, SpeakingState::Yes),
            (false, false, Reason::Corners, SpeakingState::No),
        ] {
            let mut judge = Mouths::default();
            for look in 0..5 {
                let open = mesh.then_some(if changing && look % 2 == 1 { 0.04 } else { 0.0 });
                let (said, detail) = judge.judge_with_diagnostics(
                    1,
                    &marks(30.0, 0.0),
                    Some(0.0),
                    open,
                    look * 250_000,
                );
                if look == 4 {
                    assert_eq!(detail.reason, expected);
                    assert_eq!(said.state, state);
                    assert_eq!(detail.speaking, said);
                    assert_eq!(detail.jitter_rad, Some(0.0));
                    assert_eq!(detail.window_span_us, Some(1_000_000));
                }
            }
        }
        let mut judge = Mouths::default();
        for look in 0..5 {
            let (_, detail) = judge.judge_with_diagnostics(
                1,
                &marks(30.0, if look % 2 == 0 { 0.2 } else { -0.2 }),
                Some(0.0),
                Some(0.0),
                look * 250_000,
            );
            if look == 4 {
                assert_eq!(detail.reason, Reason::Jitter);
                assert!(detail.jitter_rad.unwrap() > MOST_JITTER);
            }
        }
    }

    #[test]
    fn window_mean_and_mesh_coverage_are_not_current_sample_or_rounded_values() {
        let mut judge = Mouths::default();
        for look in 0..4 {
            judge.judge(1, &marks(23.0, 0.0), Some(0.0), Some(0.0), look * 250_000);
        }
        let (said, detail) =
            judge.judge_with_diagnostics(1, &marks(28.0, 0.0), Some(0.0), None, 1_000_000);
        assert_eq!(detail.window_mean_eyes_px, Some(24.0));
        assert_eq!(detail.eyes_px, Some(28.0));
        assert_eq!(detail.window_looks, Some(5));
        assert_eq!(detail.mesh_window_looks, Some(4));
        assert!(!detail.mesh_current);
        assert_eq!(detail.reason, Reason::Corners);
        assert_eq!(said.from, Some(SpeakingFrom::Corners));
    }

    #[test]
    fn invalid_eyes_do_not_report_empty_history_and_reset_means_new_valid_window() {
        let mut judge = Mouths::default();
        for look in 0..5 {
            judge.judge(1, &marks(30.0, 0.0), Some(0.0), Some(0.0), look * 250_000);
        }
        let (_, invalid) =
            judge.judge_with_diagnostics(1, &[(0.0, 0.0); 5], Some(0.0), Some(0.0), 1_250_000);
        assert_eq!(invalid.reason, Reason::InvalidEyes);
        assert_eq!(invalid.window_looks, None);
        assert_eq!(invalid.window_span_us, None);
        assert_eq!(invalid.mesh_window_looks, None);
        assert!(!invalid.window_reset);
        let (_, next) =
            judge.judge_with_diagnostics(1, &marks(30.0, 0.0), Some(0.0), Some(0.0), 1_500_000);
        assert_eq!(
            next.window_looks,
            Some(6),
            "invalid look left history untouched"
        );
        assert!(!next.window_reset);
        let (_, gap) =
            judge.judge_with_diagnostics(1, &marks(30.0, 0.0), Some(0.0), Some(0.0), 3_750_000);
        assert_eq!(gap.window_looks, Some(1));
        assert!(
            gap.window_reset,
            "expired window, even before the face is forgotten"
        );
        let (_, other) =
            judge.judge_with_diagnostics(2, &marks(30.0, 0.0), Some(0.0), Some(0.0), 4_000_000);
        assert!(
            other.window_reset,
            "a new passing track begins its own window"
        );
    }
}

#[cfg(test)]
mod outlier_tests {
    //! One misread look (eyes 14 px among 30 px looks, its eye line bent) in
    //! a speaking face's window, as on lamp-52e6 (#3296).
    use super::*;

    fn marks(eyes: f32, angle: f32) -> [(f32, f32); 5] {
        let (sin, cos) = angle.sin_cos();
        [
            (0.0, 0.0),
            (eyes * cos, eyes * sin),
            (eyes / 2.0, 10.0),
            (0.0, 20.0),
            (eyes, 20.0),
        ]
    }

    /// The last verdict over `looks` looks 250 ms apart, the misread ones at
    /// `bad`; the mesh's gap opens and shuts when `talking`.
    fn last(drop_outliers: bool, talking: bool, looks: u64, bad: &[u64]) -> Speaking {
        let mut mouths = Mouths {
            drop_outliers,
            ..Mouths::default()
        };
        let mut said = UNKNOWN;
        for look in 0..looks {
            let seen = if bad.contains(&look) {
                marks(14.0, 0.3)
            } else {
                marks(30.0 + (look % 3) as f32, 0.0)
            };
            let open = if talking {
                if look % 2 == 0 { 0.04 } else { 0.08 }
            } else {
                0.002
            };
            said = mouths.judge(1, &seen, Some(0.0), Some(open), look * 250_000);
        }
        said
    }

    #[test]
    fn one_outlier_is_dropped_only_with_the_switch() {
        assert_eq!(last(true, true, 9, &[6]).state, SpeakingState::Yes);
        assert_eq!(last(false, true, 9, &[6]).state, SpeakingState::Unknown);
    }

    #[test]
    fn two_outliers_or_too_few_good_looks_stay_unknown() {
        assert_eq!(last(true, true, 9, &[3, 6]).state, SpeakingState::Unknown);
        assert_eq!(last(true, true, 5, &[3]).state, SpeakingState::Unknown);
    }

    #[test]
    fn a_still_face_with_one_outlier_is_not_speaking() {
        assert_eq!(last(true, false, 9, &[6]).state, SpeakingState::No);
    }

    #[test]
    fn without_an_outlier_the_switch_changes_nothing() {
        for talking in [false, true] {
            for looks in 1..12 {
                assert_eq!(
                    last(true, talking, looks, &[]),
                    last(false, talking, looks, &[])
                );
            }
        }
    }
}
