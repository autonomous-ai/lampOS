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
use crate::{away::Away, faces, facing, speaking};
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_msgs::{Diagnostic, Face, Faces, Level, RoomPresence, Speech};
use std::{io, time::Duration};

/// Where the eyes read whether to publish. `on` only.
pub const ENVIRONMENT: &str = "LAMP_BUS";
/// Where the eyes read whether to say if anyone is at the lamp, with the
/// bus on. `on` only.
pub const PRESENCE_ENVIRONMENT: &str = "LAMP_PRESENCE";
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
        ids
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
    let (width, height) = (filled.0.max(1) as f32, filled.1.max(1) as f32);
    let boxes: Vec<[f32; 4]> = found.iter().map(|face| edges(face, filled)).collect();
    let numbers = ids.of(&boxes, now_us);
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
                mouths.judge(id, &face.marks, turn, open, now_us)
            });
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
pub fn health(camera: Camera, set_aside: usize) -> Diagnostic {
    let (mut level, message, state) = match camera {
        Camera::Closed => (Level::Ok, "", "closed"),
        Camera::Open => (Level::Ok, "", "open"),
        Camera::NotOpening => (Level::Warn, "the camera does not open", "not opening"),
        Camera::Failed => (Level::Warn, "the camera stopped giving pictures", "failed"),
    };
    let mut message = message.to_owned();
    if set_aside > 0 {
        if message.is_empty() {
            message = format!("the eyes set aside {set_aside} people's records they couldn't read");
        }
        level = Level::Warn;
    }
    Diagnostic::new(level, SOURCE, &message)
        .with("camera", state)
        .with("set_aside", set_aside)
}

pub struct BusOut {
    faces: Publisher,
    health: Publisher,
    ids: Ids,
    alive_at_us: u64,
    reported_at_us: Option<u64>,
    /// With `LAMP_PRESENCE=on`: whether anyone is at the lamp.
    presence: Option<Presence>,
}

/// `room/presence`, what it is judged from, and the voice's lines.
struct Presence {
    out: Publisher,
    heard: Subscriber,
    away: Away,
    /// Whether the last value said someone, for the journal's line.
    said_someone: bool,
}

impl BusOut {
    /// The publisher, if the environment says `on` and the bus can be used.
    /// A bus that cannot is said once and the eyes go on without.
    pub fn from_environment() -> Option<Self> {
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
        Some(out)
    }

    pub fn on(bus: &Bus) -> io::Result<Self> {
        let mut faces = bus.publisher_declared(Faces::TOPIC, &Faces::declaration(), SOURCE)?;
        faces.expires_after(Some(LASTS));
        let health =
            bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), SOURCE)?;
        Ok(Self {
            faces,
            health,
            ids: Ids {
                mouths: speaking::from_environment(),
                ..Ids::default()
            },
            alive_at_us: 0,
            reported_at_us: None,
            presence: None,
        })
    }

    /// Also say whether anyone is at the lamp, on `room/presence`, from the
    /// faces of each look and the voice's lines on `voice/heard` (#1677).
    pub fn say_presence(&mut self, bus: &Bus) -> io::Result<()> {
        let mut out =
            bus.publisher_declared(RoomPresence::TOPIC, &RoomPresence::declaration(), SOURCE)?;
        out.expires_after(Some(RoomPresence::LASTS));
        let heard = bus.subscribe_typed(Speech::TOPICS[0], "eyes_presence", Speech::TYPE)?;
        self.presence = Some(Presence {
            out,
            heard,
            away: Away::default(),
            said_someone: false,
        });
        Ok(())
    }

    /// One look: what the detector found in it.
    pub fn saw(
        &mut self,
        found: &[faces::Face],
        opens: &[Option<f32>],
        filled: (usize, usize),
        now_us: u64,
    ) {
        let seen = message(found, opens, filled, &mut self.ids, now_us);
        self.alive_at_us = now_us;
        let _ = self.faces.publish_json(now_us, &seen);
        if let Some(presence) = self.presence.as_mut() {
            // The tallest face as published, clamped to the picture, so the
            // journal's height matches `humans/faces/tracked`.
            let tall = seen
                .faces
                .iter()
                .map(|face| face.ymax - face.ymin)
                .reduce(f32::max);
            presence.away.looked(now_us, tall);
        }
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
        if let Some(presence) = self.presence.as_mut() {
            // When the voice heard the words, by the host's monotonic clock,
            // never later than now: lines that waited are as old as they are.
            while let Ok(Some(line)) = presence.heard.recv(Duration::ZERO) {
                presence.away.heard(line.header.at_us.min(now_us));
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
    }

    /// Say how the eyes are. Call from the loop on every turn, whatever
    /// the camera is doing: it is said a little more often than once a
    /// second, as the topic promises, and no more.
    pub fn report(&mut self, camera: Camera, set_aside: usize, now_us: u64) {
        if self
            .reported_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < 950_000)
        {
            return;
        }
        self.reported_at_us = Some(now_us);
        let _ = self.health.publish_json(now_us, &health(camera, set_aside));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        out.report(Camera::Open, 0, 2_000_000);
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
            let report = health(camera, 0);
            assert_eq!(
                (report.level, report.name.as_str(), report.message.as_str()),
                (level, "eyes", message)
            );
            assert_eq!(report.values.len(), 2);
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
        let report = health(Camera::Open, 2);
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
        let both = health(Camera::Failed, 1);
        assert_eq!(both.level, Level::Warn);
        assert_eq!(both.message, "the camera stopped giving pictures");
        assert_eq!(health(Camera::Open, 0).level, Level::Ok);
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
            out.report(camera, 0, turn * 5_000);
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

    /// The journal's height is the published face's: a face cut by the
    /// picture's edge counts only what is in the picture, and a box with no
    /// size once clamped counts for nothing.
    #[test]
    fn the_journals_height_is_the_published_faces() {
        let root = std::path::PathBuf::from(format!("/tmp/leh-{}", std::process::id()));
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

    #[test]
    fn speaking_is_said_only_when_asked_for() {
        let looks = [
            face(240.0, 120.0, 160.0, 200.0, 0.0),
            face(40.0, 100.0, 120.0, 150.0, 0.0),
        ];
        let mut off = Ids::default();
        let text = serde_json::to_string(&message(&looks, &[], PICTURE, &mut off, 0)).unwrap();
        assert!(!text.contains("speaking"), "{text}");
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
        }
    }
}
