//! What the eyes say on the lamp's bus (`docs/bus.md`): the faces in view,
//! each with a passing id, where it is, and whether it is turned to the
//! lamp. `humans/faces/tracked`, the type `lamp_msgs::Faces`.
//!
//! This is the sign a person gives, without meaning to, that they are
//! speaking to the lamp: they look at it. Nothing here decides anything
//! with it, and no picture, name or likeness leaves the process: a box, a
//! number for how sure the detector was, an angle and a yes or no.
//!
//! A message is good for [`LASTS`]: the eyes look a few times a second
//! while the camera is open, and a list that old says nothing about who is
//! there now. With the camera closed nothing is published, and the last
//! list is left to run out.
//!
//! With `LAMP_PRESENCE=on` as well (#1677), whether anyone is at the lamp,
//! on `room/presence` ([`crate::away`]): the faces of each look and the
//! voice's lines on `voice/heard`, said once a second while the eyes look.
use crate::{
    activity::{Judge, Look},
    away::{Away, Seen},
    faces, facing, speaking,
};
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_msgs::{
    CameraClosed, Diagnostic, Face, Faces, FamiliarLevel, HumanFamiliar, HumanSeems, Level,
    RoomActivity, RoomPresence, Speech, StoodUp, TurnVerdict, VoiceTurn,
};
use serde::Serialize;
use std::{io, time::Duration};

/// Where the eyes read whether to publish. `on` only.
pub const ENVIRONMENT: &str = "LAMP_BUS";
/// Where the eyes read whether to say if anyone is at the lamp, with the
/// bus on. `on` only.
pub const PRESENCE_ENVIRONMENT: &str = "LAMP_PRESENCE";
/// Where the eyes read which voice counts as someone (#1677's part 2):
/// `addressed`, only a turn the voice answered as said to the lamp
/// (`voice/turn`); otherwise every line heard (`voice/heard`).
pub const PRESENCE_VOICE_ENVIRONMENT: &str = "LAMP_PRESENCE_VOICE";
/// Where the eyes read whether a face far from the lamp counts (#1678):
/// `moving`, a face under `away::NEAR_TALL` only while it moves or just
/// after a near one ([`Away::far_moving`]); otherwise every face.
pub const PRESENCE_FAR_ENVIRONMENT: &str = "LAMP_PRESENCE_FAR";

/// Where the eyes read whether to say what the person at the desk is doing
/// (#1678), with the bus on: `on`, or `place` ([`activity_by`]).
pub const ACTIVITY_ENVIRONMENT: &str = "LAMP_ACTIVITY";

/// How the sitter is told from others while what they do is said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityBy {
    /// `on`: by face too, when the eyes have the recogniser (#1678's (d)).
    Face,
    /// `place` (#2834, the product's): by place only, no print ever taken,
    /// until the face path has its privacy read (eyes.md).
    Place,
}

/// What [`ACTIVITY_ENVIRONMENT`] asks for: `on` or `place` exactly, else
/// nothing is said.
pub fn activity_by(value: Option<&str>) -> Option<ActivityBy> {
    match value {
        Some("on") => Some(ActivityBy::Face),
        Some("place") => Some(ActivityBy::Place),
        _ => None,
    }
}
/// The name the eyes publish under.
pub const SOURCE: &str = "eyes";
/// How long a list of faces means anything.
pub const LASTS: Duration = Duration::from_secs(2);
/// A face is the one seen before if its box overlaps that one's by at
/// least this share of their union, and that one was seen this lately.
const SAME_OVERLAP: f32 = 0.2;
const SAME_WITHIN_US: u64 = 1_500_000;

/// Passing ids for faces, as ROS4HRI has them: the same for as long as a
/// face stays in view, nobody's once it has gone, and never a person's
/// name. A face is followed by where its box is, one look to the next.
#[derive(Debug, Default)]
pub struct Ids {
    next: u32,
    /// The id, the box (xmin, ymin, xmax, ymax) and when it was last seen.
    seen: Vec<(u32, [f32; 4], u64)>,
    /// Each face's mouth, when speaking is judged (`LAMP_EYES_SPEAKING=on`).
    pub mouths: Option<speaking::Mouths>,
    /// The last look's ids, in the detector's order.
    last: Vec<u32>,
    /// The last look's ids whose mouth read speaking.
    talking: Vec<u32>,
}

fn overlap(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let across = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let down = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let shared = across * down;
    let area = |r: &[f32; 4]| (r[2] - r[0]) * (r[3] - r[1]);
    shared / (area(a) + area(b) - shared).max(1e-6)
}

impl Ids {
    /// The ids of these boxes, in their order.
    pub fn of(&mut self, boxes: &[[f32; 4]], now_us: u64) -> Vec<u32> {
        self.seen
            .retain(|(_, _, at)| now_us.saturating_sub(*at) <= SAME_WITHIN_US);
        let mut taken = vec![false; self.seen.len()];
        let mut ids = Vec::with_capacity(boxes.len());
        for found in boxes {
            let best = self
                .seen
                .iter()
                .enumerate()
                .filter(|(index, _)| !taken[*index])
                .map(|(index, (_, was, _))| (index, overlap(found, was)))
                .filter(|(_, share)| *share >= SAME_OVERLAP)
                .max_by(|a, b| a.1.total_cmp(&b.1));
            let id = match best {
                Some((index, _)) => {
                    taken[index] = true;
                    self.seen[index].1 = *found;
                    self.seen[index].2 = now_us;
                    self.seen[index].0
                }
                None => {
                    self.next += 1;
                    self.seen.push((self.next, *found, now_us));
                    taken.push(true);
                    self.next
                }
            };
            ids.push(id);
        }
        self.last.clone_from(&ids);
        ids
    }

    /// Where the face with this passing id is in the last look, in the
    /// detector's order.
    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.last.iter().position(|seen| *seen == id)
    }
}

/// A face's box as fractions of the picture: (xmin, ymin, xmax, ymax).
/// `filled` is how much of the detector's picture the camera frame fills.
pub fn edges(face: &faces::Face, filled: (usize, usize)) -> [f32; 4] {
    let (width, height) = (filled.0.max(1) as f32, filled.1.max(1) as f32);
    [
        (face.x / width).clamp(0.0, 1.0),
        (face.y / height).clamp(0.0, 1.0),
        ((face.x + face.width) / width).clamp(0.0, 1.0),
        ((face.y + face.height) / height).clamp(0.0, 1.0),
    ]
}

/// The faces the detector found, as the message. `filled` is how much of
/// the detector's picture the camera frame fills, in its units.
///
/// A face too narrow to measure, or with an eye or the nose outside the
/// picture, has no angle and no judgement: it counts neither for nor
/// against. The angle's sign is taken from which side of the eyes the nose
/// sits, positive to the picture's right, which is the face's own left if
/// the picture is not mirrored. That has not been checked against a person.
/// `opens` is the mesh's `mouth_open` for each face found, by its index,
/// where it was read (`--mouth-model`): the speaking judge then reads it.
pub fn message(
    found: &[faces::Face],
    opens: &[Option<f32>],
    filled: (usize, usize),
    ids: &mut Ids,
    now_us: u64,
) -> Faces {
    message_observed(found, opens, filled, ids, now_us, None)
}

/// At most one numeric record, for the same largest detection the mesh uses.
#[derive(Debug, Serialize)]
pub struct MouthObservation {
    pub at_us: u64,
    pub filled: [usize; 2],
    /// Unnormalized x, y, width, height in working-detector pixels.
    pub face_box_px: [f32; 4],
    pub face_count: usize,
    #[serde(flatten)]
    pub decision: speaking::MouthDiagnostic,
}

fn message_observed(
    found: &[faces::Face],
    opens: &[Option<f32>],
    filled: (usize, usize),
    ids: &mut Ids,
    now_us: u64,
    mut diagnostic: Option<&mut Option<MouthObservation>>,
) -> Faces {
    let largest = diagnostic.as_ref().and_then(|_| {
        found
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.height.total_cmp(&b.1.height))
            .map(|(index, _)| index)
    });
    let (width, height) = (filled.0.max(1) as f32, filled.1.max(1) as f32);
    let boxes: Vec<[f32; 4]> = found.iter().map(|face| edges(face, filled)).collect();
    let numbers = ids.of(&boxes, now_us);
    ids.talking.clear();
    let faces = found
        .iter()
        .zip(&boxes)
        .zip(numbers)
        .enumerate()
        // A box with no size is not a face in the picture.
        .filter(|(_, ((_, edges), _))| edges[0] < edges[2] && edges[1] < edges[3])
        .map(|(index, ((face, edges), id))| {
            let width_share = face.width / width;
            let from_middle = ((face.x + face.width / 2.0) / width * 2.0 - 1.0).abs();
            let measured = width_share >= facing::LEAST_WIDTH
                && facing::marks_seen(&face.marks, width, height);
            let turn = measured.then(|| facing::yaw_deg(&face.marks)).flatten();
            let to_the_right = face.marks[2].0 >= (face.marks[0].0 + face.marks[1].0) / 2.0;
            let speaking = ids.mouths.as_mut().map(|mouths| {
                let open = opens.get(index).copied().flatten();
                if largest == Some(index)
                    && let Some(output) = diagnostic.as_deref_mut()
                {
                    let (result, decision) =
                        mouths.judge_with_diagnostics(id, &face.marks, turn, open, now_us);
                    *output = Some(MouthObservation {
                        at_us: now_us,
                        filled: [filled.0, filled.1],
                        face_box_px: [face.x, face.y, face.width, face.height],
                        face_count: found.len(),
                        decision,
                    });
                    result
                } else {
                    mouths.judge(id, &face.marks, turn, open, now_us)
                }
            });
            if speaking.is_some_and(|speaking| speaking.state == lamp_msgs::SpeakingState::Yes) {
                ids.talking.push(id);
            }
            Face {
                speaking,
                id: format!("f{id}"),
                xmin: edges[0],
                ymin: edges[1],
                xmax: edges[2],
                ymax: edges[3],
                confidence: face.score.clamp(0.0, 1.0),
                yaw: turn
                    .map(|degrees| degrees.to_radians() * if to_the_right { 1.0 } else { -1.0 }),
                facing: turn.map(|degrees| facing::facing(Some(degrees), width_share, from_middle)),
            }
        })
        .collect();
    Faces { faces }
}

/// How the camera is, as far as the lamp's health is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Camera {
    /// Closed because seeing is not permitted now. That is the eyes
    /// working as they should: with the privacy switch closed, or nobody
    /// to look at, they do not look.
    Closed,
    Open,
    /// Permitted, and it does not open.
    NotOpening,
    /// It was open and stopped giving pictures.
    Failed,
}

/// The eyes' own report (`diagnostics`): well unless the camera cannot be
/// used when it may be, or people's records were set aside because this
/// build could not read them (`set_aside`, #1505), so that a person whose
/// face the lamp no longer knows is told why. A count, never a name. The
/// camera's warning comes first when there are both.
/// `resets` counts the camera's USB device reset since it last opened (#2126).
pub fn health(camera: Camera, set_aside: usize, resets: u32) -> Diagnostic {
    let (mut level, message, state) = match camera {
        Camera::Closed => (Level::Ok, "", "closed"),
        Camera::Open => (Level::Ok, "", "open"),
        Camera::NotOpening => (Level::Warn, "the camera does not open", "not opening"),
        Camera::Failed => (Level::Warn, "the camera stopped giving pictures", "failed"),
    };
    let mut message = message.to_owned();
    if resets > 0 && camera == Camera::NotOpening {
        let times = if resets == 1 {
            "once".to_owned()
        } else {
            format!("{resets} times")
        };
        message = format!("{message}; its USB device was reset {times}");
    }
    if set_aside > 0 {
        if message.is_empty() {
            message = format!("the eyes set aside {set_aside} people's records they couldn't read");
        }
        level = Level::Warn;
    }
    Diagnostic::new(level, SOURCE, &message)
        .with("camera", state)
        .with("set_aside", set_aside)
        .with("camera_resets", resets)
}

pub struct BusOut {
    faces: Publisher,
    health: Publisher,
    /// `eyes/camera_closed`: the receipt for a close the voice asked about
    /// (#1802). Kept alive on every tick, open camera or not, so the voice
    /// can see that these eyes give receipts before it asks for one.
    closed: Publisher,
    ids: Ids,
    alive_at_us: u64,
    reported_at_us: Option<u64>,
    mouth_diagnostics: bool,
    /// With `LAMP_PRESENCE=on`: whether anyone is at the lamp.
    presence: Option<Presence>,
    /// With `lamp-eyes --visits on`: how familiar the remembered person at
    /// the lamp is (#1670).
    familiar: Option<Publisher>,
    /// With `lamp-eyes --seems`: how the person at the lamp seems from
    /// their face (#1671).
    seems: Option<Publisher>,
    /// With `LAMP_ACTIVITY=on` or `place` (the product's): what the person
    /// at the desk is doing.
    activity: Option<ActivityOut>,
}

/// `room/activity` and `room/stood_up`, and what they are judged from.
struct ActivityOut {
    out: Publisher,
    stood: Publisher,
    judge: Judge,
    by: ActivityBy,
}

/// `room/presence`, what it is judged from, and the voice's lines.
struct Presence {
    out: Publisher,
    /// The voice's lines on `voice/heard`, or with `addressed` its answered
    /// turns on `voice/turn`.
    heard: Subscriber,
    addressed: bool,
    away: Away,
    /// Whether the last value said someone, for the journal's line.
    said_someone: bool,
}

impl BusOut {
    /// The publisher, if the environment says `on` and the bus can be used.
    /// A bus that cannot is said once and the eyes go on without. With
    /// `familiar` (`--visits on`), how familiar the remembered person at the
    /// lamp is is said too (#1670), and with `seems` (`--seems`), how the
    /// person at the lamp seems from their face (#1671).
    pub fn from_environment(familiar: bool, seems: bool) -> Option<Self> {
        if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
            return None;
        }
        let (bus, mut out) = match Bus::from_environment().and_then(|bus| {
            let out = Self::on(&bus)?;
            Ok((bus, out))
        }) {
            Ok(both) => both,
            Err(error) => {
                eprintln!("lamp-eyes: nothing is published on the bus: {error}");
                return None;
            }
        };
        // Without it, or if it can't be said, the faces are published as
        // before.
        if std::env::var(PRESENCE_ENVIRONMENT).ok().as_deref() == Some("on")
            && let Err(error) = out.say_presence(&bus)
        {
            eprintln!("lamp-eyes: whether anyone is at the lamp is not said: {error}");
        }
        if familiar && let Err(error) = out.say_familiar(&bus) {
            eprintln!("lamp-eyes: how familiar the person at the lamp is is not said: {error}");
        }
        if let Some(by) = activity_by(std::env::var(ACTIVITY_ENVIRONMENT).ok().as_deref())
            && let Err(error) = out.say_activity(&bus, by)
        {
            eprintln!("lamp-eyes: what the person at the desk is doing is not said: {error}");
        }
        if seems && let Err(error) = out.say_seems(&bus) {
            eprintln!("lamp-eyes: how the person at the lamp seems is not said: {error}");
        }
        // What this process says, so a trial can tell a build or a switch
        // that didn't take from a behaviour that didn't come (#1722, 20 Oct
        // 01:11: a lamp-eyes without activity said nothing of it).
        eprintln!("lamp-eyes: {}", out.said());
        Some(out)
    }

    /// What is said on the bus, in words for the journal: the topics, and
    /// the switched ones that aren't said with the switch that would.
    pub fn said(&self) -> String {
        let mut topics = vec![Faces::TOPIC, Diagnostic::TOPIC];
        let mut off = Vec::new();
        match self.presence {
            Some(_) => topics.push(RoomPresence::TOPIC),
            None => off.push(format!("{} ({PRESENCE_ENVIRONMENT})", RoomPresence::TOPIC)),
        }
        if self.familiar.is_some() {
            topics.push(HumanFamiliar::TOPIC);
        }
        match self.activity {
            Some(_) => topics.extend([RoomActivity::TOPIC, StoodUp::TOPIC]),
            None => off.push(format!("{} ({ACTIVITY_ENVIRONMENT})", RoomActivity::TOPIC)),
        }
        if self.seems.is_some() {
            topics.push(HumanSeems::TOPIC);
        }
        let mut line = format!("said on the bus: {}", topics.join(", "));
        if !off.is_empty() {
            line.push_str(&format!("; not said: {}", off.join(", ")));
        }
        line
    }

    pub fn on(bus: &Bus) -> io::Result<Self> {
        let mut faces = bus.publisher_declared(Faces::TOPIC, &Faces::declaration(), SOURCE)?;
        faces.expires_after(Some(LASTS));
        let health =
            bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), SOURCE)?;
        let closed =
            bus.publisher_declared(CameraClosed::TOPIC, &CameraClosed::declaration(), SOURCE)?;
        Ok(Self {
            faces,
            health,
            closed,
            ids: Ids {
                mouths: speaking::from_environment(),
                ..Ids::default()
            },
            alive_at_us: 0,
            reported_at_us: None,
            mouth_diagnostics: false,
            presence: None,
            familiar: None,
            activity: None,
            seems: None,
        })
    }

    /// Also say what the person at the desk is doing, on `room/activity`,
    /// and when they stand up and leave, on `room/stood_up` (#1678).
    pub fn say_activity(&mut self, bus: &Bus, by: ActivityBy) -> io::Result<()> {
        let mut out =
            bus.publisher_declared(RoomActivity::TOPIC, &RoomActivity::declaration(), SOURCE)?;
        out.expires_after(Some(RoomActivity::LASTS));
        let stood = bus.publisher_declared(StoodUp::TOPIC, &StoodUp::declaration(), SOURCE)?;
        self.activity = Some(ActivityOut {
            out,
            stood,
            judge: Judge::default(),
            by,
        });
        Ok(())
    }

    /// With activity on, tell the sitter from others by face: the eyes'
    /// recogniser gives the prints the judge asks for (#1678's (d)). Not
    /// with `place`, which never takes a print.
    pub fn activity_by_face(&mut self) {
        if let Some(activity) = self.activity.as_mut()
            && activity.by == ActivityBy::Face
        {
            activity.judge = Judge::with_prints();
        }
    }

    /// The camera closed (not permitted, failed, or reopening): the
    /// sitter's prints go with it.
    pub fn activity_forget_prints(&mut self) {
        if let Some(activity) = self.activity.as_mut() {
            activity.judge.forget_prints();
        }
    }

    /// A yawn of the largest face (`yawn`, #3709): one more in the sitting's
    /// count on `room/activity`. Without activity, nothing.
    pub fn activity_yawned(&mut self, now_us: u64) {
        if let Some(activity) = self.activity.as_mut() {
            activity.judge.yawned(now_us);
        }
    }

    /// Whether the face-expressions yes is kept (`Judge::yawns_allowed`):
    /// without it the sitting's yawns are forgotten.
    pub fn activity_yawns_allowed(&mut self, yes: bool) {
        if let Some(activity) = self.activity.as_mut() {
            activity.judge.yawns_allowed(yes);
        }
    }

    /// Which face of the last look the judge wants printed, if any.
    pub fn activity_wants_print(&mut self, now_us: u64) -> Option<usize> {
        self.activity.as_mut()?.judge.wants_print(now_us)
    }

    /// The print asked for, or none if it could not be taken: in memory
    /// only, and the journal says what it decided, a number only.
    pub fn activity_printed(&mut self, now_us: u64, print: Option<&[f32]>) {
        if let Some(activity) = self.activity.as_mut()
            && let Some(line) = activity.judge.printed(now_us, print)
        {
            eprintln!("lamp-eyes: {line}");
        }
    }

    /// Also say whether anyone is at the lamp, on `room/presence`, from the
    /// faces of each look and the voice's lines on `voice/heard` (#1677), or
    /// with `LAMP_PRESENCE_VOICE=addressed` only its answered turns.
    pub fn say_presence(&mut self, bus: &Bus) -> io::Result<()> {
        let addressed =
            std::env::var(PRESENCE_VOICE_ENVIRONMENT).ok().as_deref() == Some("addressed");
        self.say_presence_by(bus, addressed)?;
        if std::env::var(PRESENCE_FAR_ENVIRONMENT).ok().as_deref() == Some("moving") {
            self.judge_far_faces();
        }
        Ok(())
    }

    /// With presence said, a face far from the lamp counts only while it
    /// moves or just after a near one (`LAMP_PRESENCE_FAR=moving`, #1678).
    pub fn judge_far_faces(&mut self) {
        if let Some(presence) = self.presence.as_mut() {
            presence.away = Away::far_moving();
        }
    }

    /// As [`BusOut::say_presence`], with which voice counts given.
    pub fn say_presence_by(&mut self, bus: &Bus, addressed: bool) -> io::Result<()> {
        let mut out =
            bus.publisher_declared(RoomPresence::TOPIC, &RoomPresence::declaration(), SOURCE)?;
        out.expires_after(Some(RoomPresence::LASTS));
        let heard = if addressed {
            bus.subscribe_typed(VoiceTurn::TOPIC, "eyes_presence", VoiceTurn::TYPE)?
        } else {
            bus.subscribe_typed(Speech::TOPICS[0], "eyes_presence", Speech::TYPE)?
        };
        self.presence = Some(Presence {
            out,
            heard,
            addressed,
            away: Away::default(),
            said_someone: false,
        });
        Ok(())
    }

    /// Also say how familiar the remembered person at the lamp is, on
    /// `humans/familiar` (#1670).
    pub fn say_familiar(&mut self, bus: &Bus) -> io::Result<()> {
        let mut out =
            bus.publisher_declared(HumanFamiliar::TOPIC, &HumanFamiliar::declaration(), SOURCE)?;
        out.expires_after(Some(HumanFamiliar::LASTS));
        self.familiar = Some(out);
        Ok(())
    }

    /// At each recognition: how familiar the remembered person at the lamp
    /// is, or `None` for nobody near, someone not remembered or not
    /// recognised. Nothing without [`Self::say_familiar`].
    pub fn familiar(&mut self, level: Option<FamiliarLevel>, now_us: u64) {
        if let Some(out) = self.familiar.as_mut() {
            let _ = out.publish_json(now_us, &HumanFamiliar { level });
        }
    }

    /// Also say how the person at the lamp seems from their face, on
    /// `humans/seems` (#1671).
    pub fn say_seems(&mut self, bus: &Bus) -> io::Result<()> {
        let mut out =
            bus.publisher_declared(HumanSeems::TOPIC, &HumanSeems::declaration(), SOURCE)?;
        out.expires_after(Some(HumanSeems::LASTS));
        self.seems = Some(out);
        Ok(())
    }

    /// At each read of the near face: how it seems, as a kind and a
    /// confidence only. Nothing without [`Self::say_seems`].
    pub fn seems(&mut self, read: HumanSeems, now_us: u64) {
        if let Some(out) = self.seems.as_mut() {
            let _ = out.publish_json(now_us, &read);
        }
    }

    /// Diagnostics observe an already enabled judge; they never enable inference
    /// or speaking publication. The existing nonblocking log relay owns output.
    pub fn enable_mouth_diagnostics(&mut self) -> Result<(), &'static str> {
        if self.ids.mouths.is_none() {
            return Err("--mouth-diagnostics needs LAMP_EYES_SPEAKING=on");
        }
        self.mouth_diagnostics = true;
        Ok(())
    }

    /// The frame's pixels this look's mouths were meshed from, per
    /// detector pixel (`speaking::Mouths::set_mesh_scale`).
    pub fn set_mesh_scale(&mut self, scale: f32) {
        if let Some(mouths) = self.ids.mouths.as_mut() {
            mouths.set_mesh_scale(scale);
        }
    }

    /// Whether the lamp's head was moving at this look
    /// (`speaking::Mouths::set_head_moved`).
    pub fn set_head_moved(&mut self, moved: bool) {
        if let Some(mouths) = self.ids.mouths.as_mut() {
            mouths.set_head_moved(moved);
        }
    }

    /// One look: what the detector found in it.
    pub fn saw(
        &mut self,
        found: &[faces::Face],
        opens: &[Option<f32>],
        filled: (usize, usize),
        now_us: u64,
    ) {
        let mut diagnostic = None;
        let seen = message_observed(
            found,
            opens,
            filled,
            &mut self.ids,
            now_us,
            self.mouth_diagnostics.then_some(&mut diagnostic),
        );
        self.alive_at_us = now_us;
        let _ = self.faces.publish_json(now_us, &seen);
        if let Some(presence) = self.presence.as_mut() {
            // The faces as published, clamped to the picture, so the
            // journal's height matches `humans/faces/tracked`.
            let faces: Vec<Seen> = seen
                .faces
                .iter()
                .map(|face| Seen {
                    id: face.id.clone(),
                    across: (face.xmin + face.xmax) / 2.0,
                    down: (face.ymin + face.ymax) / 2.0,
                    tall: face.ymax - face.ymin,
                    wide: filled.0 as f32 / filled.1.max(1) as f32,
                })
                .collect();
            presence.away.looked_at(now_us, &faces);
        }
        if let Some(activity) = self.activity.as_mut() {
            activity.judge.looked(now_us, &Look::all(found, filled));
        }
        if let Some(diagnostic) = diagnostic
            && let Ok(line) = serde_json::to_string(&diagnostic)
        {
            eprintln!("lamp-eyes: mouth diagnostic {line}");
        }
    }

    /// The passing id the last look gave the face at `index` of what the
    /// detector found, only while speaking is judged: the eyes' `known-face`
    /// file names it beside the known person's name, so the voice can tell
    /// whether the face seen speaking is theirs.
    pub fn speaking_id(&self, index: usize) -> Option<String> {
        self.ids.mouths.as_ref()?;
        self.ids.last.get(index).map(|id| format!("f{id}"))
    }

    /// The faces publisher's boot: a passing id means a face only under it
    /// (`boot/f3`, as the voice keeps it).
    pub fn faces_boot(&self) -> &str {
        self.faces.boot()
    }

    /// The passing ids that read speaking in the last look (none unless
    /// speaking is judged), and the detector's index of a passing id in it:
    /// for turning to the one the lamp answers (`crate::answered`).
    pub fn talking(&self) -> &[u32] {
        &self.ids.talking
    }

    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.ids.index_of(id)
    }

    /// Call from the loop whether or not the camera is open: with it closed
    /// nothing is published, and this is what says the eyes are still there.
    /// With `LAMP_PRESENCE=on`, whether anyone is at the lamp is said from
    /// here, once a second while the eyes look.
    pub fn tick(&mut self, now_us: u64) {
        if now_us.saturating_sub(self.alive_at_us) >= 1_000_000 {
            self.alive_at_us = now_us;
            let _ = self.faces.alive();
        }
        // Every tick, not only when no look was published for a second: the
        // looks keep the faces alive but never this, so with the camera open
        // its entry outlived the lease and the voice saw no receipt-giving
        // eyes (lamp-52e6, 21 October). Costs nothing until it is due.
        let _ = self.closed.alive();
        if let Some(presence) = self.presence.as_mut() {
            // When the voice heard the words, by the host's monotonic clock,
            // never later than now: lines that waited are as old as they are.
            // With `addressed`, only a turn answered as said to the lamp, at
            // its verdict: chatter it held back is nobody.
            while let Ok(Some(line)) = presence.heard.recv(Duration::ZERO) {
                let counts = !presence.addressed
                    || line
                        .parse::<VoiceTurn>()
                        .is_ok_and(|turn| turn.verdict == TurnVerdict::Answered);
                if counts {
                    presence.away.heard(line.header.at_us.min(now_us));
                }
            }
            let before = presence.said_someone;
            if let Some(now) = presence.away.say(now_us) {
                if now.someone && !before {
                    eprintln!("lamp-eyes: someone at the lamp ({})", presence.away.cause());
                }
                presence.said_someone = now.someone;
                let _ = presence.out.publish_json(now_us, &now);
            }
        }
        if let Some(activity) = self.activity.as_mut() {
            if let Some(now) = activity.judge.say(now_us) {
                let _ = activity.out.publish_json(now_us, &now);
            }
            if let Some(up) = activity.judge.stood_up(now_us) {
                let _ = activity.stood.publish_json(now_us, &up);
            }
            // Numbers only, so a scripted sitting can set the lines (#1678).
            if let Some(line) = activity.judge.journal(now_us) {
                eprintln!("lamp-eyes: {line}");
            }
            if let Some(line) = activity.judge.lost_journal() {
                eprintln!("lamp-eyes: {line}");
            }
        }
    }

    /// Say that the camera is shut for the close the voice asked about.
    pub fn camera_closed(&mut self, receipt: CameraClosed, now_us: u64) {
        let _ = self.closed.publish_json(now_us, &receipt);
    }

    /// Say how the eyes are. Call from the loop on every turn, whatever
    /// the camera is doing: it is said a little more often than once a
    /// second, as the topic promises, and no more.
    pub fn report(&mut self, camera: Camera, set_aside: usize, resets: u32, now_us: u64) {
        if self
            .reported_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < 950_000)
        {
            return;
        }
        self.reported_at_us = Some(now_us);
        let _ = self
            .health
            .publish_json(now_us, &health(camera, set_aside, resets));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::SeemsKind;

    /// A face of this box in a 640 by 480 picture, its nose `nose` eye
    /// distances to the right of the point between its eyes.
    fn face(x: f32, y: f32, width: f32, height: f32, nose: f32) -> faces::Face {
        let (left, right) = (x + width * 0.3, x + width * 0.7);
        let eyes_y = y + height * 0.4;
        let middle = (left + right) / 2.0;
        faces::Face {
            score: 0.93,
            x,
            y,
            width,
            height,
            marks: [
                (left, eyes_y),
                (right, eyes_y),
                (middle + nose * (right - left), eyes_y + height * 0.2),
                (left, y + height * 0.8),
                (right, y + height * 0.8),
            ],
        }
    }
    const PICTURE: (usize, usize) = (640, 480);

    #[test]
    fn a_face_becomes_a_box_in_fractions_with_an_angle_and_whether_it_faces_the_lamp() {
        let mut ids = Ids::default();
        let seen = message(
            &[
                face(240.0, 120.0, 160.0, 200.0, 0.0),
                face(40.0, 100.0, 120.0, 150.0, 0.95),
            ],
            &[],
            PICTURE,
            &mut ids,
            0,
        );
        seen.check().unwrap();
        let [straight, at_the_edge] = &seen.faces[..] else {
            panic!("two faces")
        };
        assert_eq!(straight.id, "f1");
        assert!((straight.xmin - 0.375).abs() < 1e-4 && (straight.xmax - 0.625).abs() < 1e-4);
        assert!((straight.ymin - 0.25).abs() < 1e-4 && (straight.ymax - 0.6667).abs() < 1e-3);
        assert!((straight.confidence - 0.93).abs() < 1e-6);
        assert!(straight.yaw.unwrap().abs() < 0.02);
        assert_eq!(straight.facing, Some(true));
        // Nose nearly at one eye: turned well away, to the picture's right.
        // At the edge of the picture the lens shows that much turn on a
        // face looking straight at the lamp, and it still counts as facing.
        assert_eq!(at_the_edge.id, "f2");
        assert!(at_the_edge.yaw.unwrap() > 1.0, "{:?}", at_the_edge.yaw);
        assert_eq!(at_the_edge.facing, Some(true));
        // The same turn in the middle of the picture is a face turned away,
        // and the other way round it has the other sign.
        let turned = |nose| {
            message(
                &[face(260.0, 100.0, 120.0, 150.0, nose)],
                &[],
                PICTURE,
                &mut Ids::default(),
                0,
            )
        };
        assert_eq!(turned(0.95).faces[0].facing, Some(false));
        assert!(turned(0.95).faces[0].yaw.unwrap() > 1.0);
        assert!(turned(-0.95).faces[0].yaw.unwrap() < -1.0);
        assert_eq!(turned(-0.95).faces[0].facing, Some(false));
    }

    #[test]
    fn a_face_too_small_or_cut_off_has_no_angle_and_no_judgement() {
        let mut ids = Ids::default();
        // 40 of 640 across is under the least width that can be measured.
        let small = message(
            &[face(300.0, 200.0, 40.0, 50.0, 0.0)],
            &[],
            PICTURE,
            &mut ids,
            0,
        );
        assert_eq!((small.faces[0].yaw, small.faces[0].facing), (None, None));
        // An eye outside the picture was guessed, not seen.
        let cut = message(
            &[face(-60.0, 100.0, 160.0, 200.0, 0.0)],
            &[],
            PICTURE,
            &mut ids,
            0,
        );
        assert_eq!((cut.faces[0].yaw, cut.faces[0].facing), (None, None));
        assert_eq!(cut.faces[0].xmin, 0.0);
        cut.check().unwrap();
        // A box wholly outside has no size and is left out; none is a list of none.
        let outside = message(
            &[face(700.0, 100.0, 100.0, 100.0, 0.0)],
            &[],
            PICTURE,
            &mut ids,
            0,
        );
        assert!(outside.faces.is_empty());
        assert!(message(&[], &[], PICTURE, &mut ids, 0).faces.is_empty());
    }

    #[test]
    fn a_face_keeps_its_id_while_it_stays_and_a_new_one_gets_a_new_id() {
        let mut ids = Ids::default();
        let at = |x: f32| [x, 0.2, x + 0.2, 0.6];
        assert_eq!(ids.of(&[at(0.1)], 0), [1]);
        // It moved a little; another arrived.
        assert_eq!(ids.of(&[at(0.6), at(0.13)], 300_000), [2, 1]);
        // One look without the first, then it is back where it was.
        assert_eq!(ids.of(&[at(0.6)], 600_000), [2]);
        assert_eq!(ids.of(&[at(0.6), at(0.15)], 900_000), [2, 1]);
        // Gone for longer than a moment: whoever is there now is new, and
        // an id is never given again.
        assert_eq!(ids.of(&[at(0.15)], 3_000_000), [3]);
        // Two boxes on one old face: only one of them is that face.
        assert_eq!(ids.of(&[at(0.15), at(0.16)], 3_100_000), [3, 4]);
    }

    #[test]
    fn each_look_is_published_and_lasts_two_seconds() {
        let root = std::path::PathBuf::from(format!("/tmp/leb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        out.saw(&[face(240.0, 120.0, 160.0, 200.0, 0.0)], &[], PICTURE, 5);
        out.tick(6);
        let latest = bus.latest(Faces::TOPIC).unwrap().unwrap();
        let seen: Faces = latest.parse().unwrap();
        assert_eq!(seen.faces.len(), 1);
        assert_eq!(seen.faces[0].facing, Some(true));
        assert_eq!(latest.header.expires_ms, Some(2_000));
        // With the bus gone nothing fails.
        std::fs::remove_dir_all(&root).unwrap();
        out.saw(&[], &[], PICTURE, 7);
        out.tick(2_000_000);
        out.report(Camera::Open, 0, 0, 2_000_000);
    }

    #[test]
    fn the_receipt_publisher_stays_alive_while_looks_keep_coming() {
        let root = std::path::PathBuf::from(format!("/tmp/lec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // A short lease, so it is shown every 100 ms and outlived in 300.
        let bus = Bus::new(&root)
            .unwrap()
            .lease(std::time::Duration::from_millis(300));
        let mut out = BusOut::on(&bus).unwrap();
        // The camera open: a look four times a second, each followed by a tick.
        for look in 0..8u64 {
            let at = 1_000_000 + look * 250_000;
            out.saw(&[face(240.0, 120.0, 160.0, 200.0, 0.0)], &[], PICTURE, at);
            out.tick(at + 1);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let sources = bus.publishers(CameraClosed::TOPIC).unwrap();
        assert!(sources.iter().any(|source| source.name == SOURCE));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_eyes_are_well_with_the_camera_closed_or_open_and_warn_when_it_cannot_be_used() {
        for (camera, level, message, state) in [
            (Camera::Closed, Level::Ok, "", "closed"),
            (Camera::Open, Level::Ok, "", "open"),
            (
                Camera::NotOpening,
                Level::Warn,
                "the camera does not open",
                "not opening",
            ),
            (
                Camera::Failed,
                Level::Warn,
                "the camera stopped giving pictures",
                "failed",
            ),
        ] {
            let report = health(camera, 0, 0);
            assert_eq!(
                (report.level, report.name.as_str(), report.message.as_str()),
                (level, "eyes", message)
            );
            assert_eq!(report.values.len(), 3);
            assert_eq!(
                (
                    report.values[0].key.as_str(),
                    report.values[0].value.as_str()
                ),
                ("camera", state)
            );
        }
    }

    /// #1505: while people's records are set aside, the eyes say so at
    /// `Warn`, with the count and never a name; the camera's own warning
    /// comes first.
    #[test]
    fn set_aside_records_make_the_eyes_warn_with_a_count() {
        let report = health(Camera::Open, 2, 0);
        assert_eq!(report.level, Level::Warn);
        assert_eq!(
            report.message,
            "the eyes set aside 2 people's records they couldn't read"
        );
        assert!(
            report
                .values
                .iter()
                .any(|value| value.key == "set_aside" && value.value == "2"),
            "{:?}",
            report.values
        );
        let both = health(Camera::Failed, 1, 0);
        assert_eq!(both.level, Level::Warn);
        assert_eq!(both.message, "the camera stopped giving pictures");
        assert_eq!(health(Camera::Open, 0, 0).level, Level::Ok);
        // A stuck camera's resets are counted and said (#2126).
        assert_eq!(
            health(Camera::NotOpening, 0, 1).message,
            "the camera does not open; its USB device was reset once"
        );
        let stuck = health(Camera::NotOpening, 0, 2);
        assert_eq!(stuck.level, Level::Warn);
        assert_eq!(
            stuck.message,
            "the camera does not open; its USB device was reset 2 times"
        );
        assert!(
            stuck
                .values
                .iter()
                .any(|value| value.key == "camera_resets" && value.value == "2")
        );
    }

    #[test]
    fn how_the_eyes_are_is_said_about_once_a_second_whatever_the_camera_does() {
        let root = std::path::PathBuf::from(format!("/tmp/leh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut hears = bus
            .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        // Three seconds of the loop, a turn every 5 ms: closed, then the
        // camera does not open.
        for turn in 0..600u64 {
            let camera = if turn < 300 {
                Camera::Closed
            } else {
                Camera::NotOpening
            };
            out.report(camera, 0, 0, turn * 5_000);
        }
        let mut said = Vec::new();
        while let Some(message) = hears.recv(std::time::Duration::from_millis(50)).unwrap() {
            said.push(message.parse::<Diagnostic>().unwrap());
        }
        assert_eq!(said.len(), 4, "{said:?}");
        assert_eq!(said[0].level, Level::Ok);
        assert_eq!(said[3].message, "the camera does not open");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// #1677: with presence on, each look and the voice's lines make
    /// `room/presence`, said once a second, good for three, and nothing once
    /// the looks stop.
    #[test]
    fn whether_anyone_is_at_the_lamp_is_said_from_the_looks_and_the_voices_lines() {
        let root = std::path::PathBuf::from(format!("/tmp/lep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut hears = bus
            .subscribe_typed(RoomPresence::TOPIC, "test", RoomPresence::TYPE)
            .unwrap();
        let mut voice = bus
            .publisher_declared(Speech::TOPICS[0], &Speech::declaration(), "voice")
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        let mut said = Vec::new();
        let mut take = |hears: &mut lamp_bus::Subscriber| {
            while let Some(message) = hears.recv(Duration::ZERO).unwrap() {
                assert_eq!(message.header.expires_ms, Some(3_000));
                said.push(message.parse::<RoomPresence>().unwrap());
            }
        };
        // Without the switch, nothing is said.
        out.saw(&[], &[], PICTURE, 1_000_000);
        out.tick(1_000_000);
        take(&mut hears);
        out.say_presence(&bus).unwrap();
        // An empty room for 25 s, a look every 500 ms.
        let s = 1_000_000u64;
        // Read after each turn: a subscriber keeps only so many of a kept
        // topic's messages.
        for look in 0..50u64 {
            let at = 2 * s + look * s / 2;
            out.saw(&[], &[], PICTURE, at);
            out.tick(at);
            take(&mut hears);
        }
        // A voice heard at 26.5 s (the bus stamps the words when heard).
        voice
            .publish_json(
                26 * s + s / 2,
                &Speech {
                    text: "where are my keys".into(),
                    finished: true,
                    locale: None,
                    request: None,
                },
            )
            .unwrap();
        out.saw(&[], &[], PICTURE, 27 * s);
        out.tick(27 * s);
        take(&mut hears);
        // A face at 28 s.
        let seen = [face(240.0, 120.0, 160.0, 200.0, 0.0)];
        out.saw(&seen, &[], PICTURE, 28 * s);
        out.tick(28 * s);
        take(&mut hears);
        // The looks stop: after 5 s nothing more is said.
        for turn in 0..20u64 {
            out.tick(28 * s + turn * s / 2);
            take(&mut hears);
        }
        assert_eq!(
            said.first(),
            Some(&RoomPresence {
                someone: false,
                nobody_s: 0,
                by: None,
            })
        );
        let before_voice = said
            .iter()
            .position(|now| now.someone)
            .expect("the voice is someone");
        assert_eq!(
            said[before_voice - 1],
            RoomPresence {
                someone: false,
                nobody_s: 24,
                by: None,
            }
        );
        assert_eq!(before_voice, 25, "once a second, then at once: {said:?}");
        // What showed someone: the voice first, then the face at 28 s.
        assert_eq!(said[before_voice].by, Some(lamp_msgs::SeenBy::Voice));
        assert_eq!(said.last().unwrap().by, Some(lamp_msgs::SeenBy::Face));
        // Someone from the voice on, for as long as the eyes look.
        assert!(
            said[before_voice..].iter().all(|now| now.someone),
            "{said:?}"
        );
        assert_eq!(said.len(), before_voice + 6, "{said:?}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// #1677's part 2: with `LAMP_PRESENCE_VOICE=addressed`, a voice is
    /// someone only on a turn answered as said to the lamp; a turn held back
    /// and a line merely heard are nobody.
    #[test]
    fn with_addressed_only_an_answered_turn_is_someone() {
        let root = std::path::PathBuf::from(format!("/tmp/lpa-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut hears = bus
            .subscribe_typed(RoomPresence::TOPIC, "test", RoomPresence::TYPE)
            .unwrap();
        let mut turns = bus
            .publisher_declared(VoiceTurn::TOPIC, &VoiceTurn::declaration(), "voice")
            .unwrap();
        let mut heard = bus
            .publisher_declared(Speech::TOPICS[0], &Speech::declaration(), "voice")
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        out.say_presence_by(&bus, true).unwrap();
        let s = 1_000_000u64;
        let mut last = None;
        let mut look = |out: &mut BusOut, at: u64, last: &mut Option<RoomPresence>| {
            out.saw(&[], &[], PICTURE, at);
            out.tick(at);
            while let Some(message) = hears.recv(Duration::ZERO).unwrap() {
                *last = Some(message.parse::<RoomPresence>().unwrap());
            }
        };
        look(&mut out, s, &mut last);
        // Chatter: a line heard, and a turn held back as not said to it.
        let line = Speech {
            text: "did you see the match".into(),
            finished: true,
            locale: None,
            request: None,
        };
        heard.publish_json(2 * s, &line).unwrap();
        let held = VoiceTurn {
            request: 7,
            verdict: TurnVerdict::HeldBack,
        };
        turns.publish_json(3 * s, &held).unwrap();
        look(&mut out, 4 * s, &mut last);
        assert_eq!(last.map(|now| now.someone), Some(false), "{last:?}");
        // Said to the lamp: someone, by voice.
        let answered = VoiceTurn {
            request: 8,
            verdict: TurnVerdict::Answered,
        };
        turns.publish_json(5 * s, &answered).unwrap();
        look(&mut out, 6 * s, &mut last);
        assert_eq!(
            last,
            Some(RoomPresence {
                someone: true,
                nobody_s: 0,
                by: Some(lamp_msgs::SeenBy::Voice),
            })
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// #1678: with far faces judged, a small steady face (0.06 of the
    /// picture tall, its box moving a pixel or two) is nobody; a near one is
    /// someone.
    #[test]
    fn with_far_faces_judged_a_small_steady_face_is_nobody() {
        let root = std::path::PathBuf::from(format!("/tmp/lpf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut hears = bus
            .subscribe_typed(RoomPresence::TOPIC, "test", RoomPresence::TYPE)
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        out.say_presence_by(&bus, false).unwrap();
        out.judge_far_faces();
        let s = 1_000_000u64;
        let mut last = None;
        for look in 0..60u64 {
            let at = s + look * s / 2;
            // 29 of 480 tall, at the picture's right, a pixel back and forth.
            let x = 540.0 + (look % 2) as f32;
            out.saw(&[face(x, 200.0, 24.0, 29.0, 0.0)], &[], PICTURE, at);
            out.tick(at);
            while let Some(message) = hears.recv(Duration::ZERO).unwrap() {
                last = Some(message.parse::<RoomPresence>().unwrap());
            }
        }
        assert_eq!(last.map(|now| now.someone), Some(false), "{last:?}");
        // A face 96 of 480 tall is near: someone at once.
        out.saw(&[face(300.0, 200.0, 80.0, 96.0, 0.0)], &[], PICTURE, 32 * s);
        out.tick(32 * s);
        while let Some(message) = hears.recv(Duration::ZERO).unwrap() {
            last = Some(message.parse::<RoomPresence>().unwrap());
        }
        assert_eq!(last.map(|now| now.someone), Some(true), "{last:?}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The journal's height is the published face's: a face cut by the
    /// picture's edge counts only what is in the picture, and a box with no
    /// size once clamped counts for nothing.
    #[test]
    fn the_journals_height_is_the_published_faces() {
        let root = std::path::PathBuf::from(format!("/tmp/lej-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        out.say_presence(&bus).unwrap();
        // 200 of 480 tall, from 400: 80 of it in the picture.
        out.saw(
            &[face(240.0, 400.0, 160.0, 200.0, 0.0)],
            &[],
            PICTURE,
            1_000_000,
        );
        let away = &out.presence.as_ref().unwrap().away;
        assert_eq!(away.cause(), "face, 0.17 of the picture tall");
        // Wholly below the picture, beside a face 96 tall: only the second counts.
        out.saw(
            &[
                face(240.0, 500.0, 300.0, 400.0, 0.0),
                face(40.0, 100.0, 80.0, 96.0, 0.0),
            ],
            &[],
            PICTURE,
            2_000_000,
        );
        let away = &out.presence.as_ref().unwrap().away;
        assert_eq!(away.cause(), "face, 0.20 of the picture tall");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// #1678: with activity on, each look's largest face makes
    /// `room/activity`, at the desk while the baseline comes in, said once a
    /// second and good for three; someone rising out of the chair and gone
    /// makes one `room/stood_up`.
    #[test]
    fn what_the_person_at_the_desk_does_is_said_from_the_looks() {
        let root = std::path::PathBuf::from(format!("/tmp/lea-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut hears = bus
            .subscribe_typed(RoomActivity::TOPIC, "test", RoomActivity::TYPE)
            .unwrap();
        let mut stood = bus
            .subscribe_typed(StoodUp::TOPIC, "test", StoodUp::TYPE)
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        out.say_activity(&bus, ActivityBy::Face).unwrap();
        let s = 1_000_000u64;
        let mut said = Vec::new();
        // Seated 30 s, a face 120 of 480 tall, a look every 500 ms.
        for look in 0..60u64 {
            let at = look * s / 2;
            out.saw(&[face(260.0, 180.0, 96.0, 120.0, 0.0)], &[], PICTURE, at);
            out.tick(at);
            while let Some(message) = hears.recv(Duration::ZERO).unwrap() {
                assert_eq!(message.header.expires_ms, Some(3_000));
                said.push(message.parse::<RoomActivity>().unwrap());
            }
        }
        assert!(said.len() >= 29, "{said:?}");
        assert!(
            said.iter()
                .all(|now| now.state == lamp_msgs::Activity::AtDesk && !now.baseline)
        );
        assert_eq!(said.last().unwrap().sitting_s, 29);
        // Rising out of the chair over a second, then gone.
        for (k, top) in [180.0, 140.0, 100.0, 60.0].into_iter().enumerate() {
            let at = 30 * s + k as u64 * s / 4;
            let tall = 120.0 * (1.0 - 0.06 * k as f32);
            out.saw(&[face(260.0, top, tall * 0.8, tall, 0.0)], &[], PICTURE, at);
            out.tick(at);
        }
        for look in 0..8u64 {
            let at = 31 * s + look * s / 2;
            out.saw(&[], &[], PICTURE, at);
            out.tick(at);
        }
        let up = stood.recv(Duration::ZERO).unwrap().expect("stood up");
        assert_eq!(up.parse::<StoodUp>().unwrap().sitting_s, 30);
        assert_eq!(
            up.header.expires_ms,
            Some(5_000),
            "a departure's short life"
        );
        assert!(stood.recv(Duration::ZERO).unwrap().is_none(), "once");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// `LAMP_ACTIVITY=place` (#2834, the product's): the person at the desk
    /// is said as with `on`, but followed by place only, so the judge never
    /// asks for a face print even when the eyes have the recogniser; `on`
    /// asks for the sitter's within the sitting's first looks.
    #[test]
    fn with_place_the_sitter_is_never_printed() {
        assert_eq!(activity_by(Some("on")), Some(ActivityBy::Face));
        assert_eq!(activity_by(Some("place")), Some(ActivityBy::Place));
        for off in [None, Some(""), Some("off"), Some("ON"), Some("yes")] {
            assert_eq!(activity_by(off), None, "{off:?}");
        }
        let root = std::path::PathBuf::from(format!("/tmp/lepl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let asked = |by: ActivityBy| {
            let mut out = BusOut::on(&bus).unwrap();
            out.say_activity(&bus, by).unwrap();
            // As main.rs does when the eyes have the recogniser.
            out.activity_by_face();
            let s = 1_000_000u64;
            let mut asked = 0;
            for look in 0..120u64 {
                let at = look * s / 2;
                out.saw(&[face(260.0, 180.0, 96.0, 120.0, 0.0)], &[], PICTURE, at);
                out.tick(at);
                if out.activity_wants_print(at).is_some() {
                    asked += 1;
                    out.activity_printed(at, Some(&[0.5; 128]));
                }
            }
            asked
        };
        assert!(
            asked(ActivityBy::Face) > 0,
            "on asks for the sitter's print"
        );
        assert_eq!(asked(ActivityBy::Place), 0, "place never does");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// #1722 (20 Oct 01:11): the start line says what is said, and which
    /// switched topics aren't with the switch that would say them.
    #[test]
    fn the_start_line_says_which_topics_are_said() {
        let root = std::path::PathBuf::from(format!("/tmp/lebl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        assert_eq!(
            out.said(),
            "said on the bus: humans/faces/tracked, diagnostics; not said: room/presence \
             (LAMP_PRESENCE), room/activity (LAMP_ACTIVITY)"
        );
        out.say_activity(&bus, ActivityBy::Face).unwrap();
        assert_eq!(
            out.said(),
            "said on the bus: humans/faces/tracked, diagnostics, room/activity, \
             room/stood_up; not said: room/presence (LAMP_PRESENCE)"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn diagnostics_preserve_bus_bytes_and_exclude_identity_and_media() {
        let mut off = Ids {
            mouths: Some(speaking::Mouths::default()),
            ..Ids::default()
        };
        let mut on = Ids {
            mouths: Some(speaking::Mouths::default()),
            ..Ids::default()
        };
        for look in 0..32u64 {
            let looks = [
                face(240.0, 120.0, 160.0, 200.0, 0.0),
                face(40.0, 100.0, 120.0, 150.0, 0.0),
            ];
            let opens = [Some(if look % 3 == 0 { 0.04 } else { 0.0 }), None];
            let expected = message(&looks, &opens, PICTURE, &mut off, look * 250_000);
            let mut detail = None;
            let actual = message_observed(
                &looks,
                &opens,
                PICTURE,
                &mut on,
                look * 250_000,
                Some(&mut detail),
            );
            assert_eq!(
                serde_json::to_vec(&expected).unwrap(),
                serde_json::to_vec(&actual).unwrap()
            );
            let detail = detail.unwrap();
            assert_eq!(detail.face_box_px, [240.0, 120.0, 160.0, 200.0]);
            assert_eq!(detail.face_count, 2);
            let record = serde_json::to_value(&detail).unwrap();
            let mut keys = record
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>();
            keys.sort_unstable();
            let mut allowed = [
                "at_us",
                "filled",
                "face_box_px",
                "face_count",
                "eyes_px",
                "window_mean_eyes_px",
                "window_looks",
                "window_span_us",
                "turn_deg",
                "jitter_rad",
                "reason",
                "mesh_current",
                "mesh_window_looks",
                "mouth_open",
                "window_reset",
                "speaking",
            ];
            allowed.sort_unstable();
            assert_eq!(keys, allowed);
            let speech = record["speaking"].as_object().unwrap();
            assert!(
                speech
                    .keys()
                    .all(|key| ["state", "score", "from"].contains(&key.as_str()))
            );
            assert!(serde_json::to_vec(&detail).unwrap().len() < 2_048);
        }
        let mut detail = None;
        message_observed(&[], &[], PICTURE, &mut on, 9_000_000, Some(&mut detail));
        assert!(detail.is_none(), "no face is not a judgement");
        message_observed(
            &[face(240.0, 120.0, 160.0, 200.0, 0.0)],
            &[],
            PICTURE,
            &mut Ids::default(),
            0,
            Some(&mut detail),
        );
        assert!(detail.is_none(), "diagnostics never enable the judge");
    }

    #[test]
    fn diagnostic_logging_is_off_by_default_and_cannot_enable_speaking() {
        let root = std::path::PathBuf::from(format!("/tmp/led-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = Bus::new(&root).unwrap();
        let mut output = BusOut::on(&bus).unwrap();
        assert!(!output.mouth_diagnostics);
        output.ids.mouths = None;
        assert!(output.enable_mouth_diagnostics().is_err());
        assert!(!output.mouth_diagnostics);
        output.ids.mouths = Some(speaking::Mouths::default());
        assert!(output.enable_mouth_diagnostics().is_ok());
        assert!(output.mouth_diagnostics);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Explicit host CPU measurement, not a timing gate or a device claim.
    #[test]
    #[ignore = "host diagnostic cost report; run explicitly with --nocapture"]
    fn mouth_diagnostic_cost_report() {
        use std::{hint::black_box, time::Instant};
        let looks = [face(240.0, 120.0, 160.0, 200.0, 0.0)];
        let n = 10_000u64;
        for enabled in [false, true] {
            let mut ids = Ids {
                mouths: Some(speaking::Mouths::default()),
                ..Ids::default()
            };
            let began = Instant::now();
            let mut bytes = 0usize;
            for look in 0..n {
                let mut detail = None;
                let seen = message_observed(
                    &looks,
                    &[Some(if look % 3 == 0 { 0.04 } else { 0.0 })],
                    PICTURE,
                    &mut ids,
                    look * 250_000,
                    enabled.then_some(&mut detail),
                );
                bytes += black_box(serde_json::to_vec(&seen).unwrap()).len();
                if let Some(detail) = detail {
                    bytes += black_box(serde_json::to_vec(&detail).unwrap()).len();
                }
            }
            eprintln!(
                "mouth diagnostic cost enabled={enabled} looks={n} mean_us={:.3} formatted_bytes={bytes}; host only, no inference or journal writes",
                began.elapsed().as_secs_f64() * 1e6 / n as f64
            );
        }
    }

    #[test]
    fn speaking_is_said_only_when_asked_for() {
        let looks = [
            face(240.0, 120.0, 160.0, 200.0, 0.0),
            face(40.0, 100.0, 120.0, 150.0, 0.0),
        ];
        let mut off = Ids::default();
        let text = serde_json::to_string(&message(&looks, &[], PICTURE, &mut off, 0)).unwrap();
        assert!(!text.contains("speaking"), "{text}");
        // Nobody reads speaking with it off, so nobody is turned to (#1810).
        assert!(off.talking.is_empty());
        // A passing id is found where the detector put its face (#2270).
        let first = off.last[1];
        let moved = [looks[1], looks[0]];
        message(&moved, &[], PICTURE, &mut off, 100_000);
        assert_eq!(off.index_of(first), Some(0));
        assert_eq!(off.index_of(first + 100), None);
        let mut on = Ids {
            mouths: Some(speaking::Mouths::default()),
            ..Ids::default()
        };
        for look in 0..2u64 {
            let seen = message(&looks, &[], PICTURE, &mut on, look * 150_000);
            assert_eq!(seen.faces.len(), 2);
            for tracked in &seen.faces {
                // New faces: too few looks to tell yet.
                let said = tracked.speaking.expect("judged");
                assert_eq!(said.state, lamp_msgs::SpeakingState::Unknown);
            }
            let text = serde_json::to_string(&seen).unwrap();
            assert_eq!(text.matches("\"speaking\":").count(), 2, "{text}");
            // `unknown` is not speaking.
            assert!(on.talking.is_empty());
        }
    }

    /// #1670: with familiarity on, each recognition says the level of the
    /// remembered person at the lamp, or none, from the eyes, good for five
    /// seconds; without it, nothing is said. No name is in it.
    #[test]
    fn how_familiar_the_person_at_the_lamp_is_is_said_only_when_switched_on() {
        let root = std::path::PathBuf::from(format!("/tmp/lef-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut hears = bus
            .subscribe_typed(HumanFamiliar::TOPIC, "test", HumanFamiliar::TYPE)
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        out.familiar(Some(FamiliarLevel::Close), 1_000_000);
        assert!(hears.recv(Duration::ZERO).unwrap().is_none());
        out.say_familiar(&bus).unwrap();
        out.familiar(Some(FamiliarLevel::Regular), 2_000_000);
        out.familiar(None, 4_000_000);
        let mut said = Vec::new();
        while let Some(message) = hears.recv(Duration::ZERO).unwrap() {
            assert_eq!(message.header.source, "eyes");
            assert_eq!(message.header.expires_ms, Some(5_000));
            let text = String::from_utf8_lossy(&message.payload).into_owned();
            assert!(!text.contains("name"), "{text}");
            said.push(message.parse::<HumanFamiliar>().unwrap().level);
        }
        assert_eq!(said, [Some(FamiliarLevel::Regular), None]);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// #1671: with the face read on, each read says a kind and a
    /// confidence, from the eyes, good for five seconds; without it,
    /// nothing is said.
    #[test]
    fn how_the_person_at_the_lamp_seems_is_said_only_when_switched_on() {
        let root = std::path::PathBuf::from(format!("/tmp/les-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut hears = bus
            .subscribe_typed(HumanSeems::TOPIC, "test", HumanSeems::TYPE)
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        let read = |kind, confidence| HumanSeems {
            kind,
            confidence,
            from: lamp_msgs::SeemsReader::FerPlus,
        };
        out.seems(read(SeemsKind::Positive, 0.9), 1_000_000);
        assert!(hears.recv(Duration::ZERO).unwrap().is_none());
        out.say_seems(&bus).unwrap();
        out.seems(read(SeemsKind::Negative, 0.7), 2_000_000);
        out.seems(read(SeemsKind::Unsure, 0.3), 4_000_000);
        let mut said = Vec::new();
        while let Some(message) = hears.recv(Duration::ZERO).unwrap() {
            assert_eq!(message.header.source, "eyes");
            assert_eq!(message.header.expires_ms, Some(5_000));
            let text = String::from_utf8_lossy(&message.payload).into_owned();
            // A kind, a confidence and which reader said it: nothing else.
            assert_eq!(text.matches(':').count(), 3, "{text}");
            said.push(message.parse::<HumanSeems>().unwrap());
        }
        assert_eq!(
            said,
            [read(SeemsKind::Negative, 0.7), read(SeemsKind::Unsure, 0.3)]
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
