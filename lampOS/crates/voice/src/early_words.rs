//! What was said before a lone face was known follows that person (#2176,
//! 5a), with `LAMP_MEMORY_EARLY_WORDS=on` and the Days memory. Off unless
//! set.
//!
//! The eyes take a few seconds to know a face. Until they do, what a person
//! says is the room's (`everyone`): on lamp-4ace at 11:30 a conversation
//! began the room's and became the owner's at 11:35, and the earlier part
//! stayed the room's. With the switch, when a conversation that began the
//! room's becomes exactly one known person's, before it has ended
//! ([`crate::remember`]'s writer, 45 s idle or an end), its room exchanges
//! and the things kept in it are moved to that person's scope by the
//! memory's `adopt`, on the writer's own thread, never a turn's.
//!
//! Only when nobody else could have said them: no other face, known or
//! unknown, was seen from the conversation's first words to that moment
//! ([`Seen::alone`]). Each face is told apart by its passing track id
//! (`Face.id`, the same while that face stays in view, a new one once it
//! has gone), so one face leaving and another arriving is two faces. A look
//! with two faces or more, a second track id, another known name, a face
//! without a readable id, or any moment the eyes could not be read (the
//! camera off, no fresh look, a gap between looks over [`LOOK_GAP`]) keeps
//! everything the room's, as before. The same person who leaves and comes
//! back has a new id, so that too moves nothing. The faces
//! are read with the known file, twice a second, by the Live route's
//! person switch (`provider_worker`); a route that doesn't read them never
//! adopts anything.
use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Only the exact value `on`, with the Days memory.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_EARLY_WORDS";
static ON: OnceLock<bool> = OnceLock::new();

/// Whether the room's early words may follow a lone face once known.
pub fn on() -> bool {
    *ON.get_or_init(|| std::env::var(ENVIRONMENT).ok().as_deref() == Some("on"))
}

/// Set it once for this process; the first setting stands. For a caller
/// that runs the Days client itself.
pub fn switch(on: bool) -> bool {
    *ON.get_or_init(|| on)
}

/// Two good looks further apart than this leave a moment unseen.
pub const LOOK_GAP: Duration = Duration::from_secs(2);
/// A look older than this on the bus says nothing of now: the eyes keep a
/// look for two seconds.
const FRESH_MS: u64 = 2_000;
/// Known people, and track ids, remembered per run of looks; more is
/// "can't tell".
const NAMES: usize = 8;

/// Why the room's words stay the room's, said in the journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Two faces in one look, a second track id, or another known person.
    AnotherFace,
    /// A moment the eyes could not be read, or a face without a readable
    /// track id.
    CantTell,
}

impl Why {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AnotherFace => "another_face",
            Self::CantTell => "cant_tell",
        }
    }
}

/// What the eyes showed, one look at a time: when the current unbroken run
/// of looks began, the last two-face look, the last face without a readable
/// track id, and each known person's and each track id's last.
#[derive(Default)]
pub struct Seen {
    run_from: Option<Instant>,
    last_look: Option<Instant>,
    many_at: Option<Instant>,
    blind_at: Option<Instant>,
    names: Vec<(String, Instant)>,
    tracks: Vec<(String, Instant)>,
}

/// Note `key` seen at `now` in `list`; false when the list is full and
/// `key` is new.
fn note(list: &mut Vec<(String, Instant)>, key: String, now: Instant) -> bool {
    let full = list.len() == NAMES;
    match list.iter_mut().find(|(seen, _)| *seen == key) {
        Some((_, at)) => *at = now,
        None if full => return false,
        None => list.push((key, now)),
    }
    true
}

impl Seen {
    /// One look: `None` when the eyes could not be read, else each face's
    /// track id in it (`None` for a face whose id can't be read) and whose
    /// memory id the known file named.
    pub fn look(&mut self, look: Option<(Vec<Option<String>>, Option<String>)>, now: Instant) {
        let Some((faces, named)) = look else {
            self.run_from = None;
            self.last_look = None;
            return;
        };
        if self
            .last_look
            .is_none_or(|last| now.saturating_duration_since(last) > LOOK_GAP)
        {
            self.run_from = Some(now);
            self.names.clear();
            self.tracks.clear();
        }
        self.last_look = Some(now);
        if faces.len() > 1 {
            self.many_at = Some(now);
        }
        let mut room = true;
        for track in faces {
            match track {
                Some(track) => room &= note(&mut self.tracks, track, now),
                None => self.blind_at = Some(now),
            }
        }
        if let Some(named) = named {
            room &= note(&mut self.names, named, now);
        }
        if !room {
            self.run_from = None;
            self.last_look = None;
        }
    }

    /// Whether only `person`'s face (or none) was seen from `since` to
    /// `now`, every moment of it looked at.
    pub fn alone(&self, person: &str, since: Instant, now: Instant) -> Result<(), Why> {
        let watched = self.run_from.is_some_and(|from| from <= since)
            && self
                .last_look
                .is_some_and(|last| now.saturating_duration_since(last) <= LOOK_GAP);
        if !watched {
            return Err(Why::CantTell);
        }
        if self.many_at.is_some_and(|at| at >= since)
            || self
                .names
                .iter()
                .any(|(who, at)| who != person && *at >= since)
            || self.tracks.iter().filter(|(_, at)| *at >= since).count() > 1
        {
            return Err(Why::AnotherFace);
        }
        if self.blind_at.is_some_and(|at| at >= since) {
            return Err(Why::CantTell);
        }
        Ok(())
    }
}

/// This process's looks, fed by the route that reads the eyes.
pub static SEEN: Mutex<Seen> = Mutex::new(Seen {
    run_from: None,
    last_look: None,
    many_at: None,
    blind_at: None,
    names: Vec::new(),
    tracks: Vec::new(),
});

/// With the switch, one look into [`SEEN`]: the camera allowed, the eyes'
/// newest faces on the bus (none when it has no bus), and the memory id the
/// known file names. Never waits on anything but the bus's file.
pub fn look(camera: bool, bus: Option<&lamp_bus::Bus>, named: Option<String>, now: Instant) {
    if !on() {
        return;
    }
    let faces = bus
        .filter(|_| camera)
        .and_then(|bus| faces_now(bus, unix_ms()));
    if let Ok(mut seen) = SEEN.lock() {
        seen.look(faces.map(|faces| (faces, named)), now);
    }
}

/// Each face's track id in the eyes' newest fresh look, `None` without one;
/// a face that fails its check or has an empty id has no readable id.
fn faces_now(bus: &lamp_bus::Bus, now_unix_ms: u64) -> Option<Vec<Option<String>>> {
    let message = bus.latest(lamp_msgs::Faces::TOPIC).ok()??;
    if message.expired || now_unix_ms.saturating_sub(message.header.at_unix_ms) > FRESH_MS {
        return None;
    }
    let faces = message.parse::<lamp_msgs::Faces>().ok()?;
    Some(
        faces
            .faces
            .into_iter()
            .map(|face| (face.check().is_ok() && !face.id.trim().is_empty()).then_some(face.id))
            .collect(),
    )
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "p-a";

    /// A look at so many ms, unread (`None`) or its faces' track ids
    /// (space-separated, `?` for one without a readable id, empty for no
    /// face) and whose name.
    type At<'a> = (u64, Option<(&'a str, Option<&'a str>)>);

    fn watched(looks: &[At<'_>]) -> (Seen, Instant) {
        let start = Instant::now();
        let mut seen = Seen::default();
        for (ms, look) in looks {
            seen.look(
                look.map(|(ids, who)| {
                    let ids = ids
                        .split_whitespace()
                        .map(|id| (id != "?").then(|| id.to_owned()))
                        .collect();
                    (ids, who.map(str::to_owned))
                }),
                start + Duration::from_millis(*ms),
            );
        }
        (seen, start)
    }

    #[test]
    fn one_face_then_known_is_alone_and_anything_else_is_not() {
        let at = |start: Instant, ms| start + Duration::from_millis(ms);
        // An unknown face, then the same one known: theirs.
        let (seen, start) = watched(&[
            (0, Some(("a", None))),
            (500, Some(("a", None))),
            (1000, Some(("a", Some(A)))),
        ]);
        assert_eq!(seen.alone(A, at(start, 0), at(start, 1200)), Ok(()));
        // Nobody in view for a moment (under the gap), then the same track:
        // still looked at, still theirs.
        let (seen, start) = watched(&[
            (0, Some(("a", None))),
            (500, Some(("", None))),
            (1000, Some(("a", Some(A)))),
        ]);
        assert_eq!(seen.alone(A, at(start, 0), at(start, 1000)), Ok(()));
        // One face leaves and another arrives, known: two faces, the first
        // one's words are not the second's (reviewer-lead, P2).
        let (seen, start) = watched(&[
            (0, Some(("a", None))),
            (500, Some(("", None))),
            (1000, Some(("b", Some(A)))),
        ]);
        assert_eq!(
            seen.alone(A, at(start, 0), at(start, 1000)),
            Err(Why::AnotherFace)
        );
        // ... but a track gone before the conversation began doesn't count.
        assert_eq!(seen.alone(A, at(start, 600), at(start, 1000)), Ok(()));
        // Two faces in one look.
        let (seen, start) = watched(&[
            (0, Some(("a", None))),
            (500, Some(("a b", None))),
            (1000, Some(("a", Some(A)))),
        ]);
        assert_eq!(
            seen.alone(A, at(start, 0), at(start, 1000)),
            Err(Why::AnotherFace)
        );
        // ... but before the conversation began, it doesn't count.
        assert_eq!(seen.alone(A, at(start, 600), at(start, 1000)), Ok(()));
        // Another known person.
        let (seen, start) = watched(&[(0, Some(("a", Some("p-b")))), (500, Some(("a", Some(A))))]);
        assert_eq!(
            seen.alone(A, at(start, 0), at(start, 500)),
            Err(Why::AnotherFace)
        );
        // The eyes not read for a moment, a gap, looks begun late, a face
        // without a readable id, or none lately: can't tell.
        for looks in [
            vec![
                (0, Some(("a", None))),
                (500, None),
                (1000, Some(("a", Some(A)))),
            ],
            vec![(0, Some(("a", None))), (2600, Some(("a", Some(A))))],
            vec![(300, Some(("a", None))), (800, Some(("a", Some(A))))],
            vec![
                (0, Some(("?", None))),
                (500, Some(("a", None))),
                (1000, Some(("a", Some(A)))),
            ],
        ] {
            let (seen, start) = watched(&looks);
            assert_eq!(
                seen.alone(A, at(start, 0), at(start, 1000)),
                Err(Why::CantTell),
                "{looks:?}"
            );
        }
        let (seen, start) = watched(&[(0, Some(("a", Some(A))))]);
        assert_eq!(
            seen.alone(A, at(start, 0), at(start, 2500)),
            Err(Why::CantTell)
        );
        // More track ids in one run than are remembered: can't tell.
        let many: Vec<String> = (0..=NAMES).map(|n| format!("t{n}")).collect();
        let looks: Vec<At<'_>> = many
            .iter()
            .enumerate()
            .map(|(n, id)| (n as u64 * 100, Some((id.as_str(), None))))
            .collect();
        let (seen, start) = watched(&looks);
        assert_eq!(
            seen.alone(A, at(start, 0), at(start, 900)),
            Err(Why::CantTell)
        );
    }
}
