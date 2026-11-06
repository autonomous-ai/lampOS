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
//! less than [`LEAST_EYES_PX`] apart, turned more than [`MOST_TURN_DEG`]
//! ([`STEADY_MOST_TURN_DEG`] for a meshed window with the switch),
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
/// With `LAMP_EYES_SPEAKING=on` too, `on` leaves out of the mesh's score every
/// look-to-look pair across which the lamp's head moved ([`Mouths::set_head_moved`])
/// or the eye distance jumped ([`MOST_EYES_STEP`]), and holds a window meshed on every look to [`LEAST_MESHED_EYES_PX`]
/// instead of [`LEAST_EYES_PX`] (#4558). Off: as before.
pub const STEADY_ENVIRONMENT: &str = "LAMP_EYES_SPEAKING_STEADY";
/// The camera's aim turning faster than this, in radians a second
/// (`full_look::aim_speeds`), is the head moving for the mesh's score. It is
/// the full look's own bar for a still head (25 counts a second, about 2
/// degrees a second, half a degree between two looks 250 ms apart): a still
/// taken under it is sharp, so the face in it is where it was.
pub const HEAD_MOVED_RADIANS_A_SECOND: f64 = crate::full_look::STILL_RADIANS_A_SECOND;
/// With [`STEADY_ENVIRONMENT`], the eye floor of a window the mesh read on
/// every look: the 24 px of [`LEAST_EYES_PX`] was set for the corners'
/// jitter, not the mesh's inner lips. To be confirmed by the 52e6 size
/// ladder on #4558.
pub const LEAST_MESHED_EYES_PX: f32 = 16.0;
/// With [`STEADY_ENVIRONMENT`], a look-to-look pair whose eye distance (in
/// the mesh's pixels) changes by more than this share of the smaller one is
/// left out of the mesh's score like a pair the head moved in: the detector's
/// box flipping between two fits on a still frame (about 8% of its width at
/// 17 px between the eyes, #4585) moves the mesh's crop and reads as lips. A
/// still large face varies about 1.2% look to look, and a face walking closer
/// less than 2%, so 5% leaves both in.
pub const MOST_EYES_STEP: f32 = 0.05;
/// With [`STEADY_ENVIRONMENT`] too, `on` skips a single still look whose eye
/// distance jumps past [`MOST_EYES_STEP`] from both its neighbours, they
/// agreeing within it, and pairs those two across it, instead of leaving
/// out both its pairs. A talking face at the desk steps its eye distance
/// past 5% on 61 to 71% of steps (lamp-52e6, 2 Nov, 17 to 28 px), mostly
/// one look at a time, so its window read `unknown` (`few_steady_pairs`,
/// 84 of 117 unread looks at about 19 px, #5522). A box that flips for one
/// look on a still face then pairs its fit with itself (#4585). Off: as before.
pub const SKIP_JUMP_ENVIRONMENT: &str = "LAMP_EYES_SPEAKING_SKIP_JUMP";
/// With [`STEADY_ENVIRONMENT`], the steady pairs a window with any pair left
/// out still needs to be judged; fewer and it is `unknown`. A talking face's
/// own landmarks step its eye distance past [`MOST_EYES_STEP`] on about 53% of
/// steps at 20 px between the eyes (median 1.0 px), a still face's on 5%, so
/// the earlier four (`LEAST_LOOKS - 1`) read most talking windows `unknown`.
/// In an offline replay of a lamp-52e6 run at about 20 px, two raised talking
/// windows read `yes` from 3 to 10 of 24 and 28 to 31 of 36 (one face), 7 to 56
/// and 39 to 66 (a mirror), and left 0 `yes` on all four still sets; one pair
/// let three single still pairs score above 0.015 (#4585).
pub const LEAST_HELD_PAIRS: usize = 2;
/// With [`STEADY_ENVIRONMENT`], the fewest pairs a window with pairs left out
/// scores `yes` over: its held pairs' squared steps are divided by at least
/// this many, so one jumpy pair of two held cannot carry the window (a lip
/// step of 0.022 scores 0.022 / sqrt(4) = 0.011, under [`MESH_SPEAKING_AT`]).
/// In an offline replay of the lamp-52e6 logs (matching the lamp's calls on
/// 133 of 133 fully logged looks), the whole window's pair count cost talking
/// windows (36 to 34 of 37, 38 to 35 of 74, 42 to 37 of 77); 4 kept the
/// held-pair counts and no still face read `yes` on any set. 3 gave the same
/// counts with a margin of only 0.0127 on that step (#4585).
pub const LEAST_HELD_PAIRS_YES: usize = 4;
/// How far back a face's mouth is read: five to eight looks at the eyes'
/// pace while they follow a face (250 ms, up to 350 ms on a 10 fps camera,
/// `main.rs`). With `--steady-rate` a still face is looked at 500 to 600 ms
/// apart, four looks a window, under `LEAST_LOOKS`: in practice `unknown`
/// (docs/eyes.md).
pub const WINDOW_US: u64 = 2_000_000;
/// The looks a window needs before it says anything.
pub const LEAST_LOOKS: usize = 5;
/// Eyes closer than this, in the detector's pixels, have mouth corners whose
/// jitter is as large as the lips' movement. A window the mesh read on every
/// look counts it in the pixels the mesh saw ([`Mouths::set_mesh_scale`]):
/// with `--full-look` the frame at its own size, four times the detector's
/// at 1280 by 960, so a face seated at the desk (eyes about 20 detector
/// pixels apart, `unknown (small)` on every look before) is read.
pub const LEAST_EYES_PX: f32 = 24.0;
/// A face turned further than this hides one corner behind the other.
pub const MOST_TURN_DEG: f32 = 30.0;
/// With [`STEADY_ENVIRONMENT`], the turn a face in a window the mesh read on
/// every look may have and still be judged, instead of [`MOST_TURN_DEG`]
/// (a corners-only window keeps that): at 30 to 40 degrees the mesh still sees both
/// mouth corners well enough. In an offline replay of a lamp-52e6 run, talking
/// looks turned 32.5 to 38.7 degrees were lost to the 30, and five still looks
/// turned 32 to 39 degrees stayed `no` or `unknown` under 40 (#4585).
pub const STEADY_MOST_TURN_DEG: f32 = 40.0;
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
/// On lamp-52e6 (#3699) with the mouth cut from the 1280 by 960 look, a
/// screen face 110 px between the eyes (in the mesh's pixels) scored at most
/// 0.0123 still (233 looks) and over 0.015 in 237 of 257 looks talking, so the
/// bar is 0.015.
pub const MESH_SPEAKING_AT: f32 = 0.015;
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
        steady: std::env::var(STEADY_ENVIRONMENT).ok().as_deref() == Some("on"),
        skip_jump: std::env::var(SKIP_JUMP_ENVIRONMENT).ok().as_deref() == Some("on"),
        ..Mouths::default()
    })
}

/// Whether `body/joint_states` says the camera's aim is turning faster than
/// [`HEAD_MOVED_RADIANS_A_SECOND`]; a joint whose speed is not said is still.
pub fn head_moved(joints: &lamp_msgs::JointState) -> bool {
    crate::full_look::aim_speeds(joints)
        .iter()
        .any(|turning| turning.abs() > HEAD_MOVED_RADIANS_A_SECOND)
}

/// How long [`JointHistory`] keeps the body's joint readings: well past
/// [`crate::full_look::FRESH_MS`] on either side of a frame's capture, with
/// a frame up to a few hundred ms old at its look.
pub const JOINT_HISTORY_MS: u64 = 1_000;

/// How often the loop reads `body/joint_states` for [`JointHistory`]: half
/// the body's 80 ms period, so every reading is still caught, rather than
/// on each loop pass (about every 5 ms, 47 to 70% of a core on lamp-52e6).
pub const JOINT_READ_EVERY_US: u64 = 40_000;

/// The body's recent `body/joint_states` readings, each its bus stamp and
/// whether the head moved ([`head_moved`]), filled at most every
/// [`JOINT_READ_EVERY_US`] ([`JointHistory::read_due`]), so a look
/// judges motion at its frame's capture from the reading nearest it, not
/// only the newest one (#4558).
#[derive(Debug, Default)]
pub struct JointHistory {
    readings: VecDeque<(u64, bool)>,
    read_at_us: Option<u64>,
}

impl JointHistory {
    /// Whether the loop should read the joints now, at monotonic `now_us`:
    /// at most once every [`JOINT_READ_EVERY_US`].
    pub fn read_due(&mut self, now_us: u64) -> bool {
        if self
            .read_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < JOINT_READ_EVERY_US)
        {
            return false;
        }
        self.read_at_us = Some(now_us);
        true
    }

    /// Keeps a reading stamped `at_unix_ms` (an expired one says nothing and
    /// is not kept; the same stamp again is the same reading; one older than
    /// the newest means the clock went back, and starts again), and lets go
    /// of those more than [`JOINT_HISTORY_MS`] older than the newest.
    pub fn push(&mut self, joints: &lamp_msgs::JointState, at_unix_ms: u64, expired: bool) {
        match self.readings.back() {
            _ if expired => return,
            Some(last) if last.0 == at_unix_ms => return,
            Some(last) if last.0 > at_unix_ms => self.readings.clear(),
            _ => {}
        }
        self.readings.push_back((at_unix_ms, head_moved(joints)));
        while self
            .readings
            .front()
            .is_some_and(|first| at_unix_ms - first.0 > JOINT_HISTORY_MS)
        {
            self.readings.pop_front();
        }
    }

    /// [`head_moved`] at a frame captured at `frame_unix_ms`, from the
    /// reading nearest the capture, before or after it. None within
    /// [`crate::full_look::FRESH_MS`] of the capture says nothing of that
    /// frame: `None`, the head's motion unknown.
    pub fn head_moved_at(&self, frame_unix_ms: u64) -> Option<bool> {
        self.readings
            .iter()
            .map(|(at, moved)| (at.abs_diff(frame_unix_ms), *moved))
            .filter(|(off, _)| *off <= crate::full_look::FRESH_MS)
            .min_by_key(|(off, _)| *off)
            .map(|(_, moved)| moved)
    }
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

/// One look of a face, as [`Mouth::looks`] holds it.
type Look = (u64, f32, f32, f32, Option<f32>, f32, bool);

#[derive(Debug)]
struct Mouth {
    /// When, the shape, the eye distance in pixels, the eye line's angle,
    /// the mesh's `mouth_open`, if it was read, and the eye distance in the
    /// pixels the mesh saw (the detector's when it was not read), and
    /// whether the head moved at that look ([`Mouths::set_head_moved`]).
    looks: VecDeque<Look>,
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
    /// [`Mouths::set_mesh_scale`]; `None` is 1.
    mesh_scale: Option<f32>,
    /// [`STEADY_ENVIRONMENT`].
    steady: bool,
    /// [`SKIP_JUMP_ENVIRONMENT`], read only with `steady`.
    skip_jump: bool,
    /// [`Mouths::set_head_moved`].
    head_moved: bool,
    /// [`Mouths::set_head_moved`]: the head as read at this look's frame and
    /// that frame's age at the look, for the diagnostic; `None` until set.
    head_at_frame: Option<(HeadAtFrame, u64)>,
}

/// The head at a look's frame, as [`Mouths::set_head_moved`] was told.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadAtFrame {
    Moving,
    Still,
    /// No joint reading within [`crate::full_look::FRESH_MS`] of the capture:
    /// taken as moving.
    Unknown,
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
    /// With `LAMP_EYES_SPEAKING_STEADY`: too few steady pairs left in a
    /// window with pairs left out to judge it, or to say `yes`
    /// ([`LEAST_HELD_PAIRS`]).
    FewSteadyPairs,
    Mesh,
    Corners,
}

/// Numeric, ephemeral diagnostics only: no face id, marks, image or name.
#[derive(Clone, Debug, Serialize)]
pub struct MouthDiagnostic {
    /// This look's, in the detector's pixels.
    pub eyes_px: Option<f32>,
    /// What [`LEAST_EYES_PX`] was held to: in the mesh's pixels for a window
    /// meshed on every look, else the detector's.
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
    /// With [`STEADY_ENVIRONMENT`], how old this look's frame was at the
    /// look, and the head as read at its capture; absent without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_age_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<HeadAtFrame>,
}

impl Mouths {
    /// The frame's pixels the mesh read this look's mouths from, per
    /// detector pixel: 1 (as without it) unless the mouth is cut from the
    /// frame at its own size (`--full-look`, `mesh::crop_full`), then the
    /// frame's width over the picture's. Set once a look, before its faces
    /// are judged.
    pub fn set_mesh_scale(&mut self, scale: f32) {
        self.mesh_scale = Some(scale);
    }

    /// Whether the lamp's head was moving at this look ([`head_moved_at`]).
    /// Set once a look, before its faces are judged; read only with
    /// [`STEADY_ENVIRONMENT`]. Unknown (`None`) is taken as moving, so that
    /// look's pairs are left out: a stalled joint stream or a frame older
    /// than its reading may hide a blurred look.
    /// `frame_age_ms` is how old the frame was at the look, for the
    /// diagnostic only.
    pub fn set_head_moved(&mut self, moved: Option<bool>, frame_age_ms: u64) {
        self.head_moved = moved.unwrap_or(true);
        let head = match moved {
            Some(true) => HeadAtFrame::Moving,
            Some(false) => HeadAtFrame::Still,
            None => HeadAtFrame::Unknown,
        };
        self.head_at_frame = Some((head, frame_age_ms));
    }

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
            frame_age_ms: self.head_at_frame.map(|(_, age)| age),
            head: self.head_at_frame.map(|(head, _)| head),
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
        let mesh_eyes = open.map_or(eyes, |_| eyes * self.mesh_scale.unwrap_or(1.0));
        mouth
            .looks
            .push_back((now_us, shape, eyes, angle, open, mesh_eyes, self.head_moved));
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
        // In window order, a dropped look left as a gap: movement is only
        // measured between looks that were next to each other, never joined
        // across a dropped one as if 500 ms were one 250 ms step.
        let kept: Vec<_> = mouth
            .looks
            .iter()
            .map(|&look| {
                median
                    .is_none_or(|m| (look.2 - m).abs() < OUTLIER_SHARE * m)
                    .then_some(look)
            })
            .collect();
        let looks: Vec<_> = kept.iter().flatten().copied().collect();
        let outliers = mouth.looks.len() - looks.len();
        // A window the mesh read on every look is judged by the mesh, so
        // its eyes are counted in the pixels the mesh saw; any other, by the
        // corners, in the detector's.
        let all_meshed = looks.iter().all(|look| look.4.is_some());
        // The mesh sees both mouth corners further round than the corners
        // themselves do, so only a meshed window takes the wider turn.
        let most_turn = if self.steady && all_meshed {
            STEADY_MOST_TURN_DEG
        } else {
            MOST_TURN_DEG
        };
        let turned = turn_deg.is_none_or(|degrees| degrees > most_turn);
        let mean_eyes = looks
            .iter()
            .map(|look| if all_meshed { look.5 } else { look.2 })
            .sum::<f32>()
            / (looks.len().max(1) as f32);
        let least_eyes = if self.steady && all_meshed {
            LEAST_MESHED_EYES_PX
        } else {
            LEAST_EYES_PX
        };
        let small = mean_eyes < least_eyes;
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
        } else if small {
            Some(Reason::Small)
        } else if outliers > MOST_OUTLIERS {
            Some(Reason::Outliers)
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
        let pairs: Vec<_> = kept
            .windows(2)
            .filter_map(|two| Some((two[0]?, two[1]?)))
            .collect();
        let steps = pairs.len().max(1) as f32;
        // How much the eye line bends from look to look: turning, nodding
        // and coming closer leave its angle as it is, a tilting head bends
        // it only slowly, and speaking does not move the eyes, so this is
        // the marks' own jitter, in this window.
        let bends: Vec<_> = kept
            .windows(3)
            .filter_map(|three| Some((three[0]?, three[1]?, three[2]?)))
            .collect();
        let jitter = (bends
            .iter()
            .map(|(a, b, c)| (a.3 - 2.0 * b.3 + c.3).powi(2))
            .sum::<f32>()
            / bends.len().max(1) as f32)
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
        if all_meshed {
            if let Some(detail) = diagnostic.as_deref_mut() {
                detail.reason = Reason::Mesh;
            }
            // With the switch, a pair across which the head moved, or the
            // eye distance jumped by more than MOST_EYES_STEP, is left out:
            // the head's own motion, or the box flipping on a still frame,
            // moves the crop and reads as lips moving (#4558, #4585). The pairs kept are still, so they score the
            // mouth as well as a still window does; only too few of them
            // leave nothing to judge (#4585: counting such a pair as a gap
            // read a talking face `unknown` for 2 s after every head move).
            // With SKIP_JUMP too, a still look whose eye distance jumps
            // away from both its neighbours, they agreeing with each other,
            // is skipped and its neighbours are paired across it: a talking
            // face's own landmark jitter at the desk then costs one pair,
            // not two (#5522), and a box flipping for one look on a still
            // face pairs one fit with itself (#4585).
            let jumped = |a: &Look, b: &Look| (b.5 - a.5).abs() > MOST_EYES_STEP * a.5.min(b.5);
            let mut held = Vec::new();
            let mut at = 0;
            while at + 1 < kept.len() {
                let next = match kept.get(at..at + 3) {
                    Some([Some(a), Some(b), Some(c)])
                        if self.steady
                            && self.skip_jump
                            && !b.6
                            && !jumped(a, c)
                            && jumped(a, b)
                            && jumped(b, c) =>
                    {
                        at + 2
                    }
                    _ => at + 1,
                };
                if let (Some(a), Some(b)) = (kept[at], kept[next])
                    && !(self.steady && (a.6 || b.6 || jumped(&a, &b)))
                {
                    held.push((a, b));
                }
                at = next;
            }
            // Only a window with pairs left out is held to that count, so a
            // still one, and every window with the switch off, is judged as
            // before.
            let dropped = held.len() < pairs.len();
            if dropped && held.len() < LEAST_HELD_PAIRS {
                if let Some(detail) = diagnostic {
                    detail.reason = Reason::FewSteadyPairs;
                }
                return UNKNOWN;
            }
            let squares = held
                .iter()
                .filter_map(|(a, b)| Some((b.4? - a.4?).powi(2)))
                .sum::<f32>();
            let score = (squares / held.len().max(1) as f32).sqrt();
            // With pairs left out, `yes` is scored over at least
            // LEAST_HELD_PAIRS_YES pairs: one jumpy pair of two held would
            // otherwise carry half the window (0.022 scores 0.0156 over two,
            // 0.011 over four, #4585). `no` stands on the held pairs; a
            // window between the two is `unknown`.
            let whole = (squares / held.len().max(LEAST_HELD_PAIRS_YES) as f32).sqrt();
            let state =
                if !dropped && score >= MESH_SPEAKING_AT || dropped && whole >= MESH_SPEAKING_AT {
                    SpeakingState::Yes
                } else if score < MESH_SPEAKING_AT {
                    SpeakingState::No
                } else {
                    if let Some(detail) = diagnostic {
                        detail.reason = Reason::FewSteadyPairs;
                    }
                    return UNKNOWN;
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
        let change = pairs.iter().map(|(a, b)| (b.1 - a.1).powi(2)).sum::<f32>() / steps;
        // What the jitter explains of the width's change is taken off.
        let score = (change - JITTER_TO_SHAPE * rest).max(0.0).sqrt();
        // A window the mesh read on some looks but not all is the mesh
        // missing a look, not a face it cannot read: the corners may say
        // `no` there but never `yes` (#3699: on lamp-52e6 a still face's
        // corners scored 0.08 across such a gap).
        let gapped = looks.iter().any(|look| look.4.is_some());
        let state = if score >= SPEAKING_AT && !gapped {
            SpeakingState::Yes
        } else if score >= SPEAKING_AT {
            return UNKNOWN;
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
        // One look in the window without the mesh: the corners judge, but
        // a window the mesh read on its other looks never says yes by them
        // (#3699): the talking corners give unknown, not yes.
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
        assert_eq!(last.state, SpeakingState::Unknown, "{last:?}");
        // Without the mesh on any look, the same talking corners say yes.
        let mut corners = Mouths::default();
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
            last = Some(corners.judge(4, &seen, Some(0.0), None, look * LOOK_US));
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

    /// A face seated at the desk in a 1280 by 960 frame (#3126's lamp-52e6):
    /// its box 0.22 of the height, 53 of the picture's 240 rows, its eyes
    /// about 21 picture pixels apart. Meshed from the picture it is `small`
    /// on every look; meshed from the frame at its own size (four of its
    /// pixels a picture pixel) it is read, and a talking mouth is `yes`. A
    /// window the mesh missed a look of stays on the detector's pixels.
    #[test]
    fn a_seated_face_meshed_from_the_full_frame_passes_the_eye_distance_gate() {
        let tall = 0.22 * 240.0;
        let eyes = 0.4 * tall;
        assert!(
            eyes < LEAST_EYES_PX && eyes * 4.0 >= LEAST_EYES_PX,
            "{eyes}"
        );
        let run = |scale: Option<f32>, missed: Option<u64>| {
            let mut judge = Mouths::default();
            if let Some(scale) = scale {
                judge.set_mesh_scale(scale);
            }
            let mut last = None;
            for look in 0..6u64 {
                let open = (Some(look) != missed).then_some(0.04 * (look % 2) as f32);
                last = Some(judge.judge_with_diagnostics(
                    1,
                    &marks(eyes, 0.0),
                    Some(0.0),
                    open,
                    look * 250_000,
                ));
            }
            last.unwrap()
        };
        for scale in [None, Some(1.0)] {
            let (said, detail) = run(scale, None);
            assert_eq!((said, detail.reason), (UNKNOWN, Reason::Small));
        }
        let (said, detail) = run(Some(1_280.0 / 320.0), None);
        assert_eq!(detail.reason, Reason::Mesh);
        assert_eq!(said.state, SpeakingState::Yes);
        assert_eq!(detail.eyes_px, Some(eyes));
        assert!((detail.window_mean_eyes_px.unwrap() - 4.0 * eyes).abs() < 1e-3);
        let (said, detail) = run(Some(4.0), Some(3));
        assert_eq!((said, detail.reason), (UNKNOWN, Reason::Small));
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

#[cfg(test)]
mod third_review_outlier {
    use super::*;

    fn marks(eyes: f32, angle: f32, width: f32) -> [(f32, f32); 5] {
        [
            (0.0, 0.0),
            (eyes * angle.cos(), eyes * angle.sin()),
            (eyes / 2.0, 10.0),
            (0.0, 20.0),
            (eyes * width, 20.0),
        ]
    }

    fn run(
        drop_outliers: bool,
        looks: impl IntoIterator<Item = (f32, f32, f32, Option<f32>)>,
    ) -> Vec<(Speaking, MouthDiagnostic)> {
        let mut mouths = Mouths {
            drop_outliers,
            ..Mouths::default()
        };
        looks
            .into_iter()
            .enumerate()
            .map(|(i, (eyes, angle, width, mesh))| {
                mouths.judge_with_diagnostics(
                    1,
                    &marks(eyes, angle, width),
                    Some(0.0),
                    mesh,
                    i as u64 * 250_000,
                )
            })
            .collect()
    }

    #[test]
    fn corners_and_high_outliers_retain_talking_and_still_answers() {
        for mesh in [false, true] {
            for talking in [false, true] {
                for bad_eyes in [14.0, 50.0] {
                    let input: Vec<_> = (0..9)
                        .map(|i| {
                            let (eyes, angle) = if i == 4 { (bad_eyes, 0.3) } else { (30.0, 0.0) };
                            let change = if talking && i % 2 == 0 { 0.04 } else { 0.0 };
                            (eyes, angle, 0.8 + change, mesh.then_some(0.02 + change))
                        })
                        .collect();
                    let on = run(true, input.clone());
                    let off = run(false, input);
                    assert_eq!(off.last().unwrap().1.reason, Reason::Jitter);
                    assert_eq!(
                        on.last().unwrap().0.state,
                        if talking {
                            SpeakingState::Yes
                        } else {
                            SpeakingState::No
                        }
                    );
                    assert_eq!(
                        on.last().unwrap().1.reason,
                        if mesh { Reason::Mesh } else { Reason::Corners }
                    );
                }
            }
        }
    }

    #[test]
    fn distance_alone_and_one_missing_mesh_do_not_prove_unknown() {
        for missing_mesh in [false, true] {
            let input = (0..9).map(|i| {
                let eyes = if i == 4 { 14.0 } else { 30.0 };
                let open = if i % 2 == 0 { 0.04 } else { 0.08 };
                (eyes, 0.0, 0.8, (!(missing_mesh && i == 4)).then_some(open))
            });
            let answers = run(false, input);
            let (answer, diagnostic) = answers.last().unwrap();
            assert_eq!(diagnostic.jitter_rad, Some(0.0));
            assert_eq!(
                answer.state,
                if missing_mesh {
                    SpeakingState::No
                } else {
                    SpeakingState::Yes
                }
            );
            assert_eq!(
                diagnostic.reason,
                if missing_mesh {
                    Reason::Corners
                } else {
                    Reason::Mesh
                }
            );
        }
    }

    #[test]
    fn an_angular_outlier_at_constant_distance_is_still_refused() {
        let input = (0..9).map(|i| {
            (
                30.0,
                if i == 4 { 0.3 } else { 0.0 },
                0.8,
                Some(if i % 2 == 0 { 0.04 } else { 0.08 }),
            )
        });
        let answers = run(true, input);
        assert_eq!(answers.last().unwrap().0.state, SpeakingState::Unknown);
        assert_eq!(answers.last().unwrap().1.reason, Reason::Jitter);
    }

    #[test]
    fn two_outliers_report_outliers_and_real_scale_change_never_speaks() {
        let input = (0..9).map(|i| {
            (
                if i == 3 || i == 6 { 14.0 } else { 30.0 },
                0.0,
                0.8,
                Some(0.002),
            )
        });
        let answers = run(true, input);
        let (answer, diagnostic) = answers.last().unwrap();
        assert_eq!(answer.state, SpeakingState::Unknown);
        assert_eq!(diagnostic.reason, Reason::Outliers);
        assert_eq!(diagnostic.window_looks, Some(9));
        assert_eq!(diagnostic.mesh_window_looks, Some(7));
        assert_eq!(
            serde_json::to_value(diagnostic).unwrap()["reason"],
            "outliers"
        );
        for mesh in [false, true] {
            let input = (0..24).map(|i| {
                (
                    if i < 9 { 30.0 } else { 45.0 },
                    0.0,
                    0.8,
                    mesh.then_some(0.002),
                )
            });
            let answers = run(true, input);
            assert!(
                answers
                    .iter()
                    .all(|(answer, _)| answer.state != SpeakingState::Yes)
            );
            assert_eq!(answers[8].0.state, SpeakingState::No);
            assert_eq!(answers[10].1.reason, Reason::Outliers);
            assert_eq!(answers.last().unwrap().0.state, SpeakingState::No);
        }
    }

    #[test]
    fn a_far_face_with_two_outliers_reports_small_first() {
        let input = (0..9).map(|i| {
            (
                if i == 3 || i == 6 { 10.0 } else { 20.0 },
                0.0,
                0.8,
                Some(0.002),
            )
        });
        let answers = run(true, input);
        let (answer, diagnostic) = answers.last().unwrap();
        assert_eq!(answer.state, SpeakingState::Unknown);
        assert_eq!(diagnostic.reason, Reason::Small);
    }

    #[test]
    fn dropping_a_look_does_not_accelerate_a_subthreshold_mesh_change() {
        // The mesh itself remains regular: each 250 ms change is 0.0101,
        // below MESH_SPEAKING_AT. Only one detector eye-distance is high.
        let input: Vec<_> = (0..9)
            .map(|i| {
                (
                    if i == 4 { 42.0 } else { 30.0 },
                    0.0,
                    0.8,
                    Some(0.02 + i as f32 * 0.0101),
                )
            })
            .collect();
        let off = run(false, input.clone());
        let on = run(true, input);
        let off = off.last().unwrap();
        let on = on.last().unwrap();
        eprintln!("third_review mesh: off={:?} on={:?}", off.0, on.0);
        assert_eq!(off.0.state, SpeakingState::No);
        assert_eq!(
            on.0.state,
            SpeakingState::No,
            "dropping a middle sample must not treat a 500 ms change as one 250 ms step: {:?}",
            on.0
        );
    }

    #[test]
    fn dropping_a_look_does_not_accelerate_a_subthreshold_corner_change() {
        // The normalized width changes by 0.018 every 250 ms; the same
        // temporal-gap issue applies without mesh or angular jitter.
        let input: Vec<_> = (0..9)
            .map(|i| {
                (
                    if i == 4 { 42.0 } else { 30.0 },
                    0.0,
                    0.65 + i as f32 * 0.018,
                    None,
                )
            })
            .collect();
        let off = run(false, input.clone());
        let on = run(true, input);
        let off = off.last().unwrap();
        let on = on.last().unwrap();
        eprintln!("third_review corners: off={:?} on={:?}", off.0, on.0);
        assert_eq!(off.0.state, SpeakingState::No);
        assert_eq!(
            on.0.state,
            SpeakingState::No,
            "dropping a middle sample must not treat a 500 ms change as one 250 ms step: {:?}",
            on.0
        );
    }
}

#[cfg(test)]
mod steady_tests {
    //! `LAMP_EYES_SPEAKING_STEADY` (#4558): a still face whose mouth reads
    //! open only while the lamp's head moves, and faces 18 px between the eyes.
    use super::*;

    fn marks(eyes: f32) -> [(f32, f32); 5] {
        [
            (0.0, 0.0),
            (eyes, 0.0),
            (eyes / 2.0, 10.0),
            (0.0, 20.0),
            (eyes, 20.0),
        ]
    }

    /// The last verdict over ten looks 250 ms apart, the head moving at
    /// `moving`, the mouth read open (0.06 from 0.002) at `open`, meshed on
    /// every look unless `corners`.
    fn last(steady: bool, eyes: f32, moving: &[u64], open: &[u64], corners: bool) -> Speaking {
        let mut mouths = Mouths {
            steady,
            ..Mouths::default()
        };
        let mut said = UNKNOWN;
        for look in 0..10 {
            mouths.set_head_moved(Some(moving.contains(&look)), 0);
            let gap = (!corners).then_some(if open.contains(&look) { 0.06 } else { 0.002 });
            said = mouths.judge(1, &marks(eyes), Some(0.0), gap, look * 250_000);
        }
        said
    }

    #[test]
    fn a_mouth_that_jumps_only_while_the_head_moves_is_not_speaking_with_the_switch() {
        let at = [3, 7];
        assert_eq!(last(false, 30.0, &at, &at, false).state, SpeakingState::Yes);
        assert_eq!(last(true, 30.0, &at, &at, false).state, SpeakingState::No);
    }

    #[test]
    fn the_same_jumps_on_a_still_head_are_speaking_either_way() {
        for steady in [false, true] {
            assert_eq!(
                last(steady, 30.0, &[], &[3, 7], false).state,
                SpeakingState::Yes
            );
        }
    }

    #[test]
    fn a_meshed_face_18_px_apart_is_judged_only_with_the_switch() {
        assert_eq!(
            last(false, 18.0, &[], &[3, 7], false).state,
            SpeakingState::Unknown
        );
        assert_eq!(
            last(true, 18.0, &[], &[3, 7], false).state,
            SpeakingState::Yes
        );
        assert_eq!(last(true, 18.0, &[], &[], false).state, SpeakingState::No);
        assert_eq!(
            last(true, 15.0, &[], &[3, 7], false).state,
            SpeakingState::Unknown
        );
    }

    #[test]
    fn a_corners_only_face_18_px_apart_stays_unknown() {
        for steady in [false, true] {
            assert_eq!(
                last(steady, 18.0, &[], &[], true).state,
                SpeakingState::Unknown
            );
        }
    }

    #[test]
    fn a_talking_window_with_one_moving_look_says_yes() {
        // Talking on every look, the head moving once: the pairs left are
        // still and score as speech (lamp-52e6 read such windows `unknown`
        // for 2 s after every head move, #4585).
        let every: Vec<u64> = (0..10).filter(|look| look % 2 == 1).collect();
        for steady in [false, true] {
            assert_eq!(
                last(steady, 30.0, &[6], &every, false).state,
                SpeakingState::Yes
            );
        }
    }

    #[test]
    fn a_window_the_head_moved_through_is_unknown() {
        // Every pair but one touches a move: too few still pairs to judge.
        let every: Vec<u64> = (0..10).filter(|look| look % 2 == 1).collect();
        assert_eq!(
            last(true, 30.0, &[1, 3, 6, 8], &every, false).state,
            SpeakingState::Unknown
        );
    }

    /// Every verdict over ten meshed looks 250 ms apart, the head still,
    /// with each look's eye distance and `mouth_open`.
    fn each(steady: bool, eyes: &[f32], open: &[f32]) -> Vec<SpeakingState> {
        let mut mouths = Mouths {
            steady,
            ..Mouths::default()
        };
        (0..eyes.len())
            .map(|look| {
                mouths
                    .judge(
                        1,
                        &marks(eyes[look]),
                        Some(0.0),
                        Some(open[look]),
                        look as u64 * 250_000,
                    )
                    .state
            })
            .collect()
    }

    /// A still screen face about 17 px between the eyes whose detector box
    /// flips between two fits (#4585), the mouth crop's `mouth_open` jumping
    /// with it.
    fn flipping() -> (Vec<f32>, Vec<f32>) {
        let eyes = vec![18.2, 16.8, 18.2, 18.2, 16.8, 18.2, 16.8, 16.8, 18.2, 16.8];
        let open = eyes
            .iter()
            .enumerate()
            .map(|(look, &e)| {
                0.040 + if e > 17.5 { 0.020 } else { 0.0 } + 0.0005 * ((look * 7 % 3) as f32 - 1.0)
            })
            .collect();
        (eyes, open)
    }

    #[test]
    fn a_still_face_whose_box_flips_is_not_speaking() {
        let (eyes, open) = flipping();
        let said = each(true, &eyes, &open);
        // Its two steady pairs, once the window holds them, read `no`.
        assert!(
            said[4..7]
                .iter()
                .all(|state| *state == SpeakingState::Unknown),
            "{said:?}"
        );
        assert!(
            said[7..].iter().all(|state| *state == SpeakingState::No),
            "{said:?}"
        );
    }

    #[test]
    fn a_still_face_whose_box_flips_stays_unknown_without_the_switch() {
        let (eyes, open) = flipping();
        let said = each(false, &eyes, &open);
        assert!(
            said.iter().all(|state| *state == SpeakingState::Unknown),
            "{said:?}"
        );
    }

    #[test]
    fn a_talking_face_with_steady_eyes_is_speaking() {
        let eyes: Vec<f32> = (0..10)
            .map(|look| 18.0 * if look % 2 == 0 { 1.01 } else { 0.99 })
            .collect();
        let open: Vec<f32> = (0..10)
            .map(|look| if look % 2 == 0 { 0.002 } else { 0.06 })
            .collect();
        let said = each(true, &eyes, &open);
        assert!(
            said[4..].iter().all(|state| *state == SpeakingState::Yes),
            "{said:?}"
        );
    }

    /// A talking face about 20 px between the eyes whose own landmarks move
    /// its eye distance past MOST_EYES_STEP on most steps (as on lamp-52e6),
    /// the mouth open on every odd look.
    fn jittering(eyes: &[f32]) -> Vec<SpeakingState> {
        let open: Vec<f32> = (0..eyes.len())
            .map(|look| if look % 2 == 0 { 0.002 } else { 0.06 })
            .collect();
        each(true, eyes, &open)
    }

    #[test]
    fn a_talking_face_whose_eyes_jitter_past_the_step_is_speaking() {
        // Windows 0..8 and 1..9 keep two and three steady pairs.
        let said = jittering(&[20.0, 21.5, 20.0, 20.0, 21.5, 20.0, 20.0, 21.5, 20.0, 20.0]);
        assert!(
            said[8..].iter().all(|state| *state == SpeakingState::Yes),
            "{said:?}"
        );
    }

    #[test]
    fn a_window_with_one_steady_pair_left_is_unknown() {
        let said = jittering(&[20.0, 21.5, 20.0, 21.5, 20.0, 20.0, 21.5, 20.0, 21.5, 20.0]);
        assert_eq!(said[9], SpeakingState::Unknown, "{said:?}");
    }

    /// Every verdict over meshed looks of one face, the head still and the
    /// face turned 7 degrees, with `LAMP_EYES_SPEAKING_SKIP_JUMP` as
    /// `skip_jump`: (ms since the first look, eye distance, `mouth_open`).
    fn timed(skip_jump: bool, looks: &[(u64, f32, f32)]) -> Vec<SpeakingState> {
        let mut mouths = Mouths {
            steady: true,
            skip_jump,
            ..Mouths::default()
        };
        looks
            .iter()
            .map(|&(ms, eyes, open)| {
                mouths.set_head_moved(Some(false), 0);
                mouths
                    .judge(1, &marks(eyes), Some(7.0), Some(open), ms * 1000)
                    .state
            })
            .collect()
    }

    /// A window of lamp-52e6's `--mouth-diagnostics` (row turntalk, 2 Nov
    /// 23:2x, #4558): a talking face at the desk, about 19 to 21 px between
    /// the eyes. 20.02 and 19.49 px each jump past 5% from neighbours that
    /// agree, so both their pairs were left out and the window read
    /// `unknown` (`few_steady_pairs`).
    const DESK_TALK: [(u64, f32, f32); 8] = [
        (0, 19.02, 0.0328),
        (264, 20.02, 0.0785),
        (517, 18.47, 0.1131),
        (760, 20.31, 0.1073),
        (1024, 20.95, 0.0904),
        (1277, 20.71, 0.1076),
        (1612, 19.49, 0.0017),
        (1862, 20.75, 0.0844),
    ];

    #[test]
    fn a_talking_face_at_the_desk_whose_eyes_jump_one_look_at_a_time_is_speaking_with_skip_jump() {
        assert_eq!(timed(true, &DESK_TALK)[7], SpeakingState::Yes);
        // Without it, as on the lamp: unread.
        assert_eq!(timed(false, &DESK_TALK)[7], SpeakingState::Unknown);
    }

    #[test]
    fn a_still_face_whose_box_flips_for_one_look_is_never_speaking_with_skip_jump() {
        // The smaller still face of #4585 (lamp-52e6, 2 Nov 09:15, row both):
        // its eye distance from its box's width at the looks around the three
        // it read `yes` without the step check, the mouth crop's `mouth_open`
        // 0.020 higher on the wider fit.
        let looks: Vec<(u64, f32, f32)> = [
            (0, 17.37),
            (243, 17.39),
            (675, 17.41),
            (950, 17.32),
            (1227, 18.45),
            (1502, 18.69),
            (1777, 17.29),
            (2053, 17.44),
            (2329, 18.91),
            (2760, 17.41),
            (3033, 18.84),
            (3308, 18.70),
            (3584, 18.71),
        ]
        .iter()
        .enumerate()
        .map(|(look, &(ms, eyes))| {
            let wide = if eyes > 18.0 { 0.020 } else { 0.0 };
            (
                ms,
                eyes,
                0.040 + wide + 0.0005 * ((look * 7 % 3) as f32 - 1.0),
            )
        })
        .collect();
        for skip_jump in [false, true] {
            let said = timed(skip_jump, &looks);
            assert!(!said.contains(&SpeakingState::Yes), "{skip_jump}: {said:?}");
        }
        // The flipping box of #4585, ten looks 250 ms apart.
        let (eyes, open) = flipping();
        let looks: Vec<_> = (0..eyes.len())
            .map(|look| (look as u64 * 250, eyes[look], open[look]))
            .collect();
        let said = timed(true, &looks);
        assert!(!said.contains(&SpeakingState::Yes), "{said:?}");
    }

    #[test]
    fn a_still_face_at_the_desk_is_never_speaking_with_skip_jump() {
        // DESK_TALK's own eye distances, the mouth shut (the mesh's own
        // jitter, about 0.002), over two windows.
        let looks: Vec<_> = DESK_TALK
            .iter()
            .chain(DESK_TALK.iter())
            .enumerate()
            .map(|(look, &(_, eyes, _))| {
                let open = 0.030 + 0.002 * ((look * 5 % 3) as f32 - 1.0);
                (look as u64 * 250, eyes, open)
            })
            .collect();
        let said = timed(true, &looks);
        assert!(!said.contains(&SpeakingState::Yes), "{said:?}");
    }

    /// Every verdict over ten meshed looks 250 ms apart of a face 30 px
    /// between the eyes turned `degrees`, the mouth's `mouth_open` per look.
    fn turned(steady: bool, degrees: f32, open: impl Fn(usize) -> f32) -> Vec<SpeakingState> {
        let mut mouths = Mouths {
            steady,
            ..Mouths::default()
        };
        (0..10)
            .map(|look| {
                mouths
                    .judge(
                        1,
                        &marks(30.0),
                        Some(degrees),
                        Some(open(look)),
                        look as u64 * 250_000,
                    )
                    .state
            })
            .collect()
    }

    #[test]
    fn a_talking_face_turned_35_degrees_is_speaking_only_with_the_switch() {
        let talking = |look: usize| if look.is_multiple_of(2) { 0.002 } else { 0.06 };
        let on = turned(true, 35.0, talking);
        assert!(
            on[4..].iter().all(|state| *state == SpeakingState::Yes),
            "{on:?}"
        );
        let off = turned(false, 35.0, talking);
        assert!(
            off.iter().all(|state| *state == SpeakingState::Unknown),
            "{off:?}"
        );
        assert!(
            turned(true, 41.0, talking)
                .iter()
                .all(|state| *state == SpeakingState::Unknown)
        );
    }

    #[test]
    fn a_still_face_turned_35_degrees_is_never_speaking() {
        let still = |look: usize| 0.002 + 0.0005 * ((look * 7 % 3) as f32 - 1.0);
        let on = turned(true, 35.0, still);
        assert!(
            on.iter().all(|state| *state != SpeakingState::Yes),
            "{on:?}"
        );
        assert!(on[4..].contains(&SpeakingState::No), "{on:?}");
    }

    #[test]
    fn a_corners_only_face_turned_35_degrees_stays_unknown_with_the_switch() {
        let mut mouths = Mouths {
            steady: true,
            ..Mouths::default()
        };
        let said: Vec<_> = (0..10)
            .map(|look| {
                mouths
                    .judge(1, &marks(30.0), Some(35.0), None, look * 250_000)
                    .state
            })
            .collect();
        assert!(
            said.iter().all(|state| *state == SpeakingState::Unknown),
            "{said:?}"
        );
    }

    #[test]
    fn a_gapped_window_never_says_yes() {
        let every: Vec<u64> = (0..10).filter(|look| look % 2 == 1).collect();
        // The head moving through the whole window leaves nothing to judge.
        let all: Vec<u64> = (0..10).collect();
        assert_eq!(
            last(true, 30.0, &all, &every, false).state,
            SpeakingState::Unknown
        );
    }

    /// Every verdict over meshed looks 250 ms apart with the switch on, each
    /// look's eye distance and `mouth_open`, the head moving at `moving`.
    fn moving_each(eyes: &[f32], moving: &[usize], open: &[f32]) -> Vec<SpeakingState> {
        let mut mouths = Mouths {
            steady: true,
            ..Mouths::default()
        };
        (0..eyes.len())
            .map(|look| {
                mouths.set_head_moved(Some(moving.contains(&look)), 0);
                mouths
                    .judge(
                        1,
                        &marks(eyes[look]),
                        Some(0.0),
                        Some(open[look]),
                        look as u64 * 250_000,
                    )
                    .state
            })
            .collect()
    }

    /// One lip step of 0.022 on the last look, every earlier look 0.002.
    fn one_step() -> Vec<f32> {
        (0..10)
            .map(|look| if look == 9 { 0.024 } else { 0.002 })
            .collect()
    }

    #[test]
    fn two_held_pairs_with_one_lip_step_in_a_moved_window_are_not_yes() {
        // The review's case: the head moves at looks 1 to 6, so of the
        // window's eight pairs only (7,8) and (8,9) are held, one stepping
        // 0.022: sqrt(0.022^2 / 2) = 0.0156 over two pairs, 0.0078 over eight.
        let said = moving_each(&[30.0; 10], &[1, 2, 3, 4, 5, 6], &one_step());
        assert_ne!(said[9], SpeakingState::Yes, "{said:?}");
    }

    #[test]
    fn two_held_pairs_with_one_lip_step_after_eye_steps_are_not_yes() {
        // The same two held pairs, the others left out by the eye step alone,
        // the head still.
        let eyes = [20.0, 21.5, 20.0, 21.5, 20.0, 21.5, 20.0, 21.5, 21.5, 21.5];
        let said = moving_each(&eyes, &[], &one_step());
        assert_ne!(said[9], SpeakingState::Yes, "{said:?}");
    }

    #[test]
    fn three_held_pairs_of_talking_in_a_moved_window_are_yes() {
        // The head moves at looks 1 to 5, leaving (6,7), (7,8) and (8,9),
        // each a lip step of 0.02: sqrt(3 * 0.02^2 / 4) = 0.0173. Over the
        // window's eight pairs it would be 0.0122, `unknown`.
        let open: Vec<f32> = (0..10)
            .map(|look| if look % 2 == 0 { 0.002 } else { 0.022 })
            .collect();
        let said = moving_each(&[30.0; 10], &[1, 2, 3, 4, 5], &open);
        assert_eq!(said[9], SpeakingState::Yes, "{said:?}");
    }

    #[test]
    fn a_still_face_while_the_head_moves_is_never_speaking() {
        // The mouth reads open only on moving looks, which come in bursts;
        // the still looks between jitter by 0.0005.
        let moving = [2, 3, 7, 8, 9, 13, 16, 17, 18, 19];
        let open: Vec<f32> = (0..24)
            .map(|look| {
                if moving.contains(&look) {
                    0.06
                } else {
                    0.002 + 0.0005 * ((look * 7 % 3) as f32 - 1.0)
                }
            })
            .collect();
        let said = moving_each(&[30.0; 24], &moving, &open);
        assert!(!said.contains(&SpeakingState::Yes), "{said:?}");
        assert!(said.contains(&SpeakingState::No), "{said:?}");
    }

    fn turning(yaw: f64) -> lamp_msgs::JointState {
        lamp_msgs::JointState {
            name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                .map(str::to_owned)
                .to_vec(),
            position: vec![0.0; 5],
            velocity: vec![yaw, 0.0, 0.0, 0.0, 0.0],
            effort: vec![],
            commanded: vec![],
        }
    }

    #[test]
    fn the_head_moves_only_when_the_aim_turns_past_the_still_bar() {
        let bar = HEAD_MOVED_RADIANS_A_SECOND;
        assert!(head_moved(&turning(bar * 1.5)));
        assert!(head_moved(&turning(-bar * 1.5)));
        assert!(!head_moved(&turning(bar * 0.5)));
        // A reading without speeds says nothing of turning.
        let mut unsaid = turning(bar * 2.0);
        unsaid.velocity.clear();
        assert!(!head_moved(&unsaid));
    }

    /// A history holding these readings: each its offset from the capture
    /// in ms, and whether the head was turning.
    fn history(capture: u64, readings: &[(i64, bool)]) -> JointHistory {
        let mut history = JointHistory::default();
        for (off, moving) in readings {
            let speed = if *moving {
                HEAD_MOVED_RADIANS_A_SECOND * 2.0
            } else {
                0.0
            };
            history.push(&turning(speed), capture.saturating_add_signed(*off), false);
        }
        history
    }

    #[test]
    fn a_reading_missing_expired_or_not_fresh_at_the_capture_is_unknown() {
        let fresh = crate::full_look::FRESH_MS as i64;
        let frame = 10_000;
        assert_eq!(JointHistory::default().head_moved_at(frame), None);
        let mut expired = JointHistory::default();
        expired.push(&turning(HEAD_MOVED_RADIANS_A_SECOND * 2.0), frame, true);
        assert_eq!(expired.head_moved_at(frame), None);
        // Older than the capture, or newer (judged after a slow mesh).
        assert_eq!(
            history(frame, &[(-fresh - 1, false)]).head_moved_at(frame),
            None
        );
        assert_eq!(
            history(frame, &[(fresh + 1, false)]).head_moved_at(frame),
            None
        );
        assert_eq!(
            history(frame, &[(-fresh, false)]).head_moved_at(frame),
            Some(false)
        );
        assert_eq!(
            history(frame, &[(fresh, true)]).head_moved_at(frame),
            Some(true)
        );
    }

    #[test]
    fn a_still_reading_after_the_capture_counts_though_the_newest_is_stale() {
        // Joints every 80 ms, but the loop kept only what it read between
        // slow looks: one 200 ms before the capture, then 100 and 180 ms
        // after it, a frame 200 ms old at its look (lamp-52e6, #4558). The
        // newest is past FRESH_MS; the one 100 ms after the capture is not.
        let frame = 50_000;
        let joints = history(frame, &[(-200, false), (100, false), (180, false)]);
        assert_eq!(joints.head_moved_at(frame), Some(false));
    }

    #[test]
    fn the_reading_nearest_the_capture_says_whether_the_head_moved() {
        let frame = 50_000;
        let joints = history(frame, &[(-120, false), (30, true), (150, false)]);
        assert_eq!(joints.head_moved_at(frame), Some(true));
        let joints = history(frame, &[(-120, true), (-20, false), (60, true)]);
        assert_eq!(joints.head_moved_at(frame), Some(false));
    }

    #[test]
    fn no_reading_near_the_capture_either_side_is_unknown_and_its_pairs_drop() {
        let frame = 50_000;
        let joints = history(frame, &[(-200, false), (170, false), (250, false)]);
        let moved = joints.head_moved_at(frame);
        assert_eq!(moved, None);
        // Unknown is taken as moving: a stale still never counts.
        let mut mouths = Mouths {
            steady: true,
            ..Mouths::default()
        };
        mouths.set_head_moved(moved, 200);
        assert!(mouths.head_moved);
    }

    #[test]
    fn the_joints_are_read_at_most_every_40_ms_not_each_pass() {
        // 1000 loop passes in one second, one every millisecond.
        let mut joints = JointHistory::default();
        let reads = (0..1_000u64)
            .filter(|pass| joints.read_due(7_000_000 + pass * 1_000))
            .count();
        assert!(reads <= 25, "{reads} reads in a second");
        // Still often enough to catch each 80 ms reading.
        assert!(reads >= 20, "{reads} reads in a second");
    }

    #[test]
    fn the_joint_history_keeps_about_a_second() {
        let mut joints = JointHistory::default();
        let still = turning(0.0);
        for at in (0..100_000).step_by(80) {
            joints.push(&still, at, false);
            // The same reading read again on the next pass is not kept twice.
            joints.push(&still, at, false);
        }
        assert!(
            joints.readings.len() as u64 <= JOINT_HISTORY_MS / 80 + 1,
            "{}",
            joints.readings.len()
        );
        assert_eq!(joints.head_moved_at(99_920 - JOINT_HISTORY_MS - 200), None);
        assert_eq!(joints.head_moved_at(99_920), Some(false));
        // A clock gone back starts again rather than refusing every reading.
        joints.push(&still, 5_000, false);
        assert_eq!(joints.readings.len(), 1);
        assert_eq!(joints.head_moved_at(5_000), Some(false));
    }

    #[test]
    fn the_mouth_diagnostic_says_the_frames_age_and_the_head_at_it() {
        let line = |head: Option<Option<bool>>| {
            let mut mouths = Mouths {
                steady: true,
                ..Mouths::default()
            };
            if let Some(moved) = head {
                mouths.set_head_moved(moved, 230);
            }
            let (_, diagnostic) =
                mouths.judge_with_diagnostics(1, &marks(30.0), Some(0.0), Some(0.01), 0);
            serde_json::to_string(&diagnostic).unwrap()
        };
        for (moved, word) in [
            (Some(true), "moving"),
            (Some(false), "still"),
            (None, "unknown"),
        ] {
            let said = line(Some(moved));
            assert!(said.contains("\"frame_age_ms\":230"), "{said}");
            assert!(said.contains(&format!("\"head\":\"{word}\"")), "{said}");
        }
        // Never told (the switch off): the line is as it was.
        let said = line(None);
        assert!(
            !said.contains("frame_age_ms") && !said.contains("\"head\""),
            "{said}"
        );
    }

    #[test]
    fn a_look_whose_head_motion_is_unknown_has_its_pairs_left_out() {
        // The mouth reads open only at looks with no fresh reading: left out
        // as moving looks are, so `no`; read as still they would be `yes`.
        let at = [3, 7];
        let said = |unknown_moves: bool| {
            let mut mouths = Mouths {
                steady: true,
                ..Mouths::default()
            };
            let mut said = UNKNOWN;
            for look in 0..10 {
                let blind = at.contains(&look);
                mouths.set_head_moved(
                    if blind && unknown_moves {
                        None
                    } else {
                        Some(false)
                    },
                    0,
                );
                let gap = if blind { 0.06 } else { 0.002 };
                said = mouths.judge(1, &marks(30.0), Some(0.0), Some(gap), look * 250_000);
            }
            said.state
        };
        assert_eq!(said(true), SpeakingState::No);
        assert_eq!(said(false), SpeakingState::Yes);
    }

    #[test]
    fn too_few_steady_pairs_name_their_own_reason() {
        let mut mouths = Mouths {
            steady: true,
            ..Mouths::default()
        };
        let mut reason = Reason::Mesh;
        for look in 0..10 {
            mouths.set_head_moved(Some((1..=7).contains(&look)), 0);
            let (_, detail) = mouths.judge_with_diagnostics(
                1,
                &marks(30.0),
                Some(0.0),
                Some(0.002),
                look * 250_000,
            );
            reason = detail.reason;
        }
        assert_eq!(reason, Reason::FewSteadyPairs);
    }
}
