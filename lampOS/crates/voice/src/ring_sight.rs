//! Where the person is, for the ring's eye ([`crate::ring_face`], task 34):
//! the eyes' newest look on the bus (`humans/faces/tracked`, `docs/eyes.md`),
//! read by the ring's own worker. Only the eye uses it, and only to say
//! which way to look; it never decides whether the ring may show anything.
//!
//! The eye is switched on in the voice's unit with `LAMP_RING_EYE=on`, which
//! the voice passes to its ring worker as `--eye`. Off unless set. With
//! `LAMP_RING_EYE=strong` it is the strong eye as well (`--eye --eye-strong`,
//! [`crate::ring_face::Face::eye_strong`]).
use lamp_bus::{Bus, Message};
use lamp_msgs::Faces;
use std::{io, path::Path};

/// Where the coordinator reads whether the ring has an eye: `on`, or
/// `strong`.
pub const ENVIRONMENT: &str = "LAMP_RING_EYE";
/// The ring worker's flag for it.
pub const FLAG: &str = "--eye";
/// The ring worker's flag for the strong eye, only after [`FLAG`].
pub const STRONG_FLAG: &str = "--eye-strong";

/// The eyes look ten times a second; so does this.
const LOOK_EVERY_US: u64 = 100_000;
/// A look older than this says nothing of where the person is now. The eyes
/// keep a look for two seconds, too long for an eye to stare at a place
/// someone has left.
const FRESH_MS: u64 = 700;

pub struct RingSight {
    bus: Bus,
    looked_at_us: Option<u64>,
    seen: Option<[f32; 2]>,
}

impl RingSight {
    /// The eyes' looks on the bus under `root`.
    pub fn on(root: &Path) -> io::Result<Self> {
        Ok(Self {
            bus: Bus::new(root)?,
            looked_at_us: None,
            seen: None,
        })
    }

    /// Call from the ring's loop with its clock: where the person is now, as
    /// [`crate::ring_face::toward`] gives it, or `None` when no fresh face
    /// is seen.
    pub fn tick(&mut self, now_us: u64) -> Option<[f32; 2]> {
        if self
            .looked_at_us
            .is_none_or(|at| now_us.saturating_sub(at) >= LOOK_EVERY_US)
        {
            self.looked_at_us = Some(now_us);
            self.seen = self
                .bus
                .latest(Faces::TOPIC)
                .ok()
                .flatten()
                .and_then(|message| nearest(&message, unix_ms()));
        }
        self.seen
    }
}

/// The ring worker's argument for the switch in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    match value {
        Some("on") => vec![FLAG.to_owned()],
        Some("strong") => vec![FLAG.to_owned(), STRONG_FLAG.to_owned()],
        _ => Vec::new(),
    }
}

/// Take a leading `--eye` off the ring worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Take a leading `--eye-strong` off the ring worker's flags, after
/// [`take`].
pub fn take_strong(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == STRONG_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The largest face in a fresh look, which is most likely the nearest
/// person, as where it is.
fn nearest(message: &Message, now_unix_ms: u64) -> Option<[f32; 2]> {
    if message.expired || now_unix_ms.saturating_sub(message.header.at_unix_ms) > FRESH_MS {
        return None;
    }
    let faces = message.parse::<Faces>().ok()?;
    faces
        .faces
        .iter()
        .filter(|face| face.check().is_ok())
        .max_by(|one, other| area(one).total_cmp(&area(other)))
        .map(|face| crate::ring_face::toward(face.xmin, face.ymin, face.xmax, face.ymax))
}

fn area(face: &lamp_msgs::Face) -> f32 {
    (face.xmax - face.xmin) * (face.ymax - face.ymin)
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::Face;
    use std::time::Duration;

    #[test]
    fn on_gives_the_eye_strong_the_strong_eye_and_anything_else_nothing() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        assert_eq!(arguments(Some("strong")), [FLAG, STRONG_FLAG]);
        for value in [None, Some(""), Some("off"), Some("ON"), Some("yes")] {
            assert!(arguments(value).is_empty(), "{value:?}");
        }
        let flags = arguments(Some("strong"));
        let (eye, rest) = take(&flags);
        let (strong, rest) = take_strong(rest);
        assert!(eye && strong && rest.is_empty());
        let flags = arguments(Some("on"));
        let (eye, rest) = take(&flags);
        assert_eq!((eye, take_strong(rest).0), (true, false));
    }

    fn face(xmin: f32, width: f32) -> Face {
        Face {
            speaking: None,
            id: format!("f{xmin}"),
            xmin,
            ymin: 0.4,
            xmax: xmin + width,
            ymax: 0.4 + width,
            confidence: 1.0,
            yaw: None,
            facing: None,
        }
    }

    #[test]
    fn the_nearest_fresh_face_is_where_the_eye_looks_and_an_old_look_is_nothing() {
        let root = std::env::temp_dir().join(format!("ring-sight-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut sight = RingSight::on(&root).unwrap();
        // No eyes: nothing seen.
        assert_eq!(sight.tick(1), None);
        let bus = Bus::new(&root).unwrap();
        let mut eyes = bus
            .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
            .unwrap();
        eyes.expires_after(Some(Duration::from_secs(2)));
        // A small face on the right of the picture and a large one on its
        // left: the large one is nearer, and it is on the lamp's left.
        eyes.publish_json(
            1,
            &Faces {
                faces: vec![face(0.8, 0.1), face(0.05, 0.3)],
            },
        )
        .unwrap();
        // Not looked at again within a tenth of a second.
        assert_eq!(sight.tick(50_000), None);
        let [right, _] = sight.tick(200_000).unwrap();
        assert!(right > 0.25, "{right}");
        // A look older than the eye trusts is nothing, although the bus still
        // keeps it.
        let message = bus.latest(Faces::TOPIC).unwrap().unwrap();
        let at = message.header.at_unix_ms;
        assert!(nearest(&message, at + FRESH_MS).is_some());
        assert!(nearest(&message, at + FRESH_MS + 1).is_none());
        // Nobody in view: nothing.
        eyes.publish_json(2, &Faces { faces: vec![] }).unwrap();
        assert_eq!(sight.tick(400_000), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_switch_is_on_only_when_set_and_its_flag_is_taken_off() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        assert!(arguments(None).is_empty() && arguments(Some("1")).is_empty());
        let flags = vec![FLAG.to_owned(), "--bus-root".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }
}
