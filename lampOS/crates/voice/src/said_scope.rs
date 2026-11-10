//! Whether a reply the lamp says may go out as the owner's (`scope: owner` on
//! `voice/line`, #6081): only while the owner is alone in view. Every check
//! fails closed: a gap, a stale look, a second face in one look, an unknown
//! face, a room that does not read quiet (`room/state`: another voice), a
//! face away from the desk seen speaking, or camera or privacy off, and the
//! line has no scope,
//! so no computer is told it. No I/O: the state below is kept by the eyes'
//! and the provider's own loops and only read here. The camera sees its frame
//! only; someone out of frame can still hear (the grant's words say so).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

/// How long the cached known face stays proof after the provider's loop
/// noted it (it notes every 500 ms; the cache drops a face over 3 s old
/// before noting it, so the two windows add up to little more than 1 s
/// past the cache's own).
pub const KNOWN_FRESH: Duration = Duration::from_secs(1);
/// How often the owners' names are read again (a file read, off the reply path).
const OWNERS_EVERY: Duration = Duration::from_secs(5);

/// What the gate reads, all given.
pub struct Evidence<'a> {
    /// The owners' memory ids, as `person::owner_ids()` gives them.
    pub owners: &'a [String],
    /// The known face's name from the eyes' file, and how long ago it was read.
    pub known: Option<&'a str>,
    pub known_age: Duration,
    /// The most faces, every one, in any one look lately; `None` with no look.
    pub faces_in_one_look: Option<usize>,
    /// The faces of the newest look, if it is no older than [`NEWEST_FRESH`]:
    /// a look with none, or none lately (camera closed, privacy on), is no scope.
    pub newest_look_faces: Option<usize>,
    /// A face not at the desk read speaking lately.
    pub speaking_not_at_desk: bool,
    /// `room/state` reads quiet, aged (`bus_out::room_now`); anything else,
    /// or none, is no proof. It is the room's judgement of talk between
    /// people: one remark from a guest may leave it quiet.
    pub room_quiet: bool,
    /// The one-face rule's record of another voice in the conversation, said
    /// on `room/state` and aged with it. Only `Some(false)` is proof: `None`
    /// (nothing tracks it: `LAMP_PERSON_SCOPE_ONE_FACE` off, no worker) and
    /// `Some(true)` (a guest asked the lamp, a second person spoke) are none.
    pub another_voice: Option<bool>,
    /// The owner's face, the only one in view, was seen speaking since the
    /// request's input began (the eyes' `speaking`, `LAMP_EYES_SPEAKING`).
    /// A guest out of frame who asks the lamp leaves the owner's mouth
    /// still, and the voice's own other-voice bit is blind to a voice with no
    /// face on the product (`LAMP_SAME_VOICE_FOR=services`; Thirteen's P2-2).
    /// With the eyes' speaking off there is none: no scope.
    pub owner_spoke_alone: bool,
}

/// The owner alone in view, by every check.
pub fn owner_alone(e: &Evidence) -> bool {
    e.known_age <= KNOWN_FRESH
        && e.faces_in_one_look == Some(1)
        && e.newest_look_faces == Some(1)
        && e.room_quiet
        && e.another_voice == Some(false)
        && e.owner_spoke_alone
        && !e.speaking_not_at_desk
        && e.known.is_some_and(|known| {
            let known = crate::known_faces::memory_person(known);
            e.owners.contains(&known)
        })
}

/// The newest look counts for this long (the faces topic's own life).
pub const NEWEST_FRESH: Duration = Duration::from_secs(2);

struct Kept {
    at: Instant,
    person: Option<String>,
    room_quiet: bool,
    another_voice: Option<bool>,
    owners: Vec<String>,
    owners_at: Option<Instant>,
}

/// Every face (at the desk or not) of each look of the last
/// [`FACES_WITHIN`], and when a face not at the desk was last read speaking.
#[derive(Default)]
struct Looks {
    counts: VecDeque<(Instant, usize)>,
    /// The newest unexpired look's face count and when it was read; cleared
    /// when no look is there.
    newest: Option<(Instant, usize)>,
    spoke_not_at_desk: Option<Instant>,
    /// The last look with exactly one face and that face speaking.
    /// The last look with exactly one face and that face speaking, and who
    /// the eyes' known file named at that moment: a speaking look counts
    /// for the owner only if the known person then is the known person now.
    spoke_alone: Option<(Instant, Option<String>)>,
    last_look: Option<(String, u64)>,
}

/// How long a look's count is kept.
const FACES_WITHIN: Duration = Duration::from_secs(3);
/// How long a face read speaking away from the desk blocks.
const SPOKE_NOT_AT_DESK_WITHIN: Duration = Duration::from_secs(10);
/// How often the evidence is read from the bus and the known file.
const EVERY: Duration = Duration::from_millis(250);

static KEPT: Mutex<Option<Kept>> = Mutex::new(None);
static LOOKS: Mutex<Looks> = Mutex::new(Looks {
    counts: VecDeque::new(),
    newest: None,
    spoke_not_at_desk: None,
    spoke_alone: None,
    last_look: None,
});

/// The coordinator's own reader of the evidence, once: the voice's workers
/// are other processes, so what the gate reads is taken from the bus and the
/// eyes' known file here, in the process that publishes `voice/line`. It
/// reads on a thread of its own; [`owner_alone_now`] only reads what it kept.
pub fn keep(bus: lamp_bus::Bus) {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let known = std::env::var_os(crate::known_faces::ENVIRONMENT)
            .map(PathBuf::from)
            .filter(|socket| socket.is_absolute())
            .map(|socket| crate::known_faces::known_file_of(&socket));
        keep_reading(bus, known, crate::person::owner_ids, EVERY);
    });
}

/// [`keep`] with the known file, the owners' read and the pace given.
fn keep_reading(
    bus: lamp_bus::Bus,
    known: Option<PathBuf>,
    read_owners: fn() -> Vec<String>,
    every: Duration,
) {
    std::thread::spawn(move || {
        loop {
            read_once(&bus, known.as_deref(), read_owners);
            std::thread::sleep(every);
        }
    });
}

fn read_once(
    bus: &lamp_bus::Bus,
    known: Option<&std::path::Path>,
    read_owners: fn() -> Vec<String>,
) {
    let now = Instant::now();
    // The known person now: the one a speaking look is bound to.
    let person = known
        .and_then(crate::known_faces::FaceObservation::read)
        .map(|face| face.name().to_owned());
    let faces_message = bus
        .latest(lamp_msgs::Faces::TOPIC)
        .ok()
        .flatten()
        .filter(|message| !message.expired);
    let Some(message) = faces_message else {
        LOOKS.lock().unwrap_or_else(|p| p.into_inner()).newest = None;
        return note_rest(bus, person, read_owners, now);
    };
    let look = (message.header.boot.clone(), message.header.seq);
    if let Ok(faces) = message.parse::<lamp_msgs::Faces>() {
        let mut looks = LOOKS.lock().unwrap_or_else(|p| p.into_inner());
        let every = crate::known_faces::count_at_desk(&faces, false);
        looks.newest = Some((now, every));
        if looks.last_look.as_ref() != Some(&look) {
            looks.last_look = Some(look);
            looks.counts.push_back((now, every));
            if faces.faces.len() == 1
                && faces.faces[0]
                    .speaking
                    .as_ref()
                    .is_some_and(|speaking| speaking.state == lamp_msgs::SpeakingState::Yes)
            {
                looks.spoke_alone = Some((now, person.clone()));
            }
            if crate::known_faces::speaking_not_at_desk(&faces, crate::known_faces::at_desk_only())
            {
                looks.spoke_not_at_desk = Some(now);
            }
        }
    } else {
        LOOKS.lock().unwrap_or_else(|p| p.into_inner()).newest = None;
    }
    note_rest(bus, person, read_owners, now);
}

fn note_rest(
    bus: &lamp_bus::Bus,
    person: Option<String>,
    read_owners: fn() -> Vec<String>,
    now: Instant,
) {
    let (room, another_voice) = crate::bus_out::room_and_voice_now(bus);
    let room_quiet = room == crate::room_told::Room::Quiet;
    note(person, room_quiet, another_voice, now, read_owners);
}

/// Keeps what was read. The owners are read here, before the lock, so a
/// reply piece asking [`owner_alone_now`] never waits on the person file
/// (Thirteen's P2 on #6101).
fn note(
    person: Option<String>,
    room_quiet: bool,
    another_voice: Option<bool>,
    now: Instant,
    read_owners: impl FnOnce() -> Vec<String>,
) {
    let due = KEPT
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .is_none_or(|old| {
            old.owners_at
                .is_none_or(|at| now.saturating_duration_since(at) >= OWNERS_EVERY)
        });
    let read = due.then(read_owners);
    let mut kept = KEPT.lock().unwrap_or_else(|p| p.into_inner());
    let (owners, owners_at) = match (read, kept.take()) {
        (Some(owners), _) => (owners, Some(now)),
        (None, Some(old)) => (old.owners, old.owners_at),
        (None, None) => (Vec::new(), None),
    };
    *kept = Some(Kept {
        at: now,
        person,
        room_quiet,
        another_voice,
        owners,
        owners_at,
    });
}

/// How long before a request's input began a look still counts as the
/// owner speaking it (the voice notes the input a little after it starts).
const SPOKE_BEFORE_INPUT: Duration = Duration::from_secs(1);

/// [`owner_alone`] for now and for the request whose input began at
/// `input_at`, from what was kept. No I/O.
pub fn owner_alone_now(input_at: Option<Instant>) -> bool {
    let now = Instant::now();
    let (most, newest, spoke, spoke_alone) = {
        let mut looks = LOOKS.lock().unwrap_or_else(|p| p.into_inner());
        while looks
            .counts
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > FACES_WITHIN)
        {
            looks.counts.pop_front();
        }
        (
            looks.counts.iter().map(|(_, faces)| *faces).max(),
            looks
                .newest
                .filter(|(at, _)| now.saturating_duration_since(*at) <= NEWEST_FRESH)
                .map(|(_, faces)| faces),
            looks
                .spoke_not_at_desk
                .is_some_and(|at| now.saturating_duration_since(at) <= SPOKE_NOT_AT_DESK_WITHIN),
            looks.spoke_alone.clone(),
        )
    };
    let kept = KEPT.lock().unwrap_or_else(|p| p.into_inner());
    let Some(kept) = kept.as_ref() else {
        return false;
    };
    owner_alone(&Evidence {
        owners: &kept.owners,
        known: kept.person.as_deref(),
        known_age: now.saturating_duration_since(kept.at),
        faces_in_one_look: most,
        newest_look_faces: newest,
        speaking_not_at_desk: spoke,
        room_quiet: kept.room_quiet,
        another_voice: kept.another_voice,
        // Seen speaking since the input began, by the person the eyes name
        // now (never one face's speech taken for another's).
        owner_spoke_alone: input_at.is_some_and(|began| {
            spoke_alone.as_ref().is_some_and(|(at, who)| {
                *at + SPOKE_BEFORE_INPUT >= began
                    && who.is_some()
                    && who.as_deref() == kept.person.as_deref()
            })
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "Example Name";

    /// The owners as `person::owner_ids()` gives them: memory ids, not names.
    fn owner_ids() -> Vec<String> {
        vec![crate::known_faces::memory_person(OWNER)]
    }

    /// The two tests that use the statics run one at a time, from empty.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn reset() {
        *KEPT.lock().unwrap_or_else(|p| p.into_inner()) = None;
        let mut looks = LOOKS.lock().unwrap_or_else(|p| p.into_inner());
        looks.counts.clear();
        looks.newest = None;
        looks.spoke_not_at_desk = None;
        looks.spoke_alone = None;
        looks.last_look = None;
    }

    fn alone(owners: &[String]) -> Evidence<'_> {
        Evidence {
            owners,
            known: Some(OWNER),
            known_age: Duration::from_secs(1),
            faces_in_one_look: Some(1),
            newest_look_faces: Some(1),
            speaking_not_at_desk: false,
            room_quiet: true,
            another_voice: Some(false),
            owner_spoke_alone: true,
        }
    }

    /// A slow owners read (the person file) never holds a reply piece's ask.
    #[test]
    fn a_slow_owners_read_never_holds_the_reply_pieces_ask() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        let reading = std::thread::spawn(|| {
            note(
                Some(OWNER.to_owned()),
                true,
                Some(false),
                Instant::now(),
                || {
                    std::thread::sleep(Duration::from_millis(400));
                    owner_ids()
                },
            );
        });
        std::thread::sleep(Duration::from_millis(50));
        let asked = Instant::now();
        let _ = owner_alone_now(None);
        assert!(
            asked.elapsed() < Duration::from_millis(200),
            "the ask waited {:?} on the owners read",
            asked.elapsed()
        );
        reading.join().unwrap();
    }

    /// A face whose mouth moves as a speaking one does.
    fn face(id: &str) -> lamp_msgs::Face {
        serde_json::from_value(serde_json::json!({
            "id": id, "xmin": 0.1, "ymin": 0.1, "xmax": 0.3, "ymax": 0.4, "confidence": 0.9,
            "speaking": {"state": "yes", "score": 0.1}
        }))
        .unwrap()
    }

    /// A face whose mouth is still.
    fn still_face(id: &str) -> lamp_msgs::Face {
        serde_json::from_value(serde_json::json!({
            "id": id, "xmin": 0.1, "ymin": 0.1, "xmax": 0.3, "ymax": 0.4, "confidence": 0.9,
            "speaking": {"state": "no", "score": 0.0}
        }))
        .unwrap()
    }

    /// The whole path in this process: the faces and the room on a bus and
    /// the eyes' known file in, the gate's answer out (the workers are other
    /// processes, so nothing of it may come from them; Reviewer-lead's P2).
    /// The owners are given as `person::owner_ids()` gives them, memory ids
    /// (Thirteen's P2: a name there hid that the owner never matched).
    #[test]
    fn the_gate_reads_its_evidence_from_the_bus_and_the_known_file() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        let began = Instant::now();
        let root = std::path::PathBuf::from(format!("/tmp/lvb-said-scope-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut faces = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "test",
            )
            .unwrap();
        let mut room = bus
            .publisher_declared(
                lamp_msgs::RoomState::TOPIC,
                &lamp_msgs::RoomState::declaration(),
                "test",
            )
            .unwrap();
        let known = root.join("known");
        std::fs::write(&known, format!("{OWNER}\n")).unwrap();
        let quiet = lamp_msgs::RoomState {
            state: lamp_msgs::Room::Quiet,
            another_voice: Some(false),
        };
        room.publish_json(1, &quiet).unwrap();
        faces
            .publish_json(
                2,
                &lamp_msgs::Faces {
                    faces: vec![face("f1")],
                },
            )
            .unwrap();
        read_once(&bus, Some(&known), owner_ids);
        assert!(
            owner_alone_now(Some(began)),
            "the owner alone, a quiet room"
        );
        // A look with no face (camera closed, privacy on, the owner gone)
        // takes it at once, though the known file is still there.
        faces
            .publish_json(3, &lamp_msgs::Faces { faces: vec![] })
            .unwrap();
        read_once(&bus, Some(&known), owner_ids);
        assert!(!owner_alone_now(Some(began)), "the newest look has nobody");
        // One face again, then a second face in the next look.
        faces
            .publish_json(
                4,
                &lamp_msgs::Faces {
                    faces: vec![face("f1")],
                },
            )
            .unwrap();
        faces
            .publish_json(
                5,
                &lamp_msgs::Faces {
                    faces: vec![face("f1"), face("f2")],
                },
            )
            .unwrap();
        read_once(&bus, Some(&known), owner_ids);
        assert!(!owner_alone_now(Some(began)), "two faces in one look");
        let _ = std::fs::remove_dir_all(root);
    }

    /// A room last said quiet long ago is no quiet room: a stopped worker
    /// leaves its last `room/state` kept (Reviewer-lead's P2 on #6101).
    #[test]
    fn an_old_quiet_room_state_is_no_scope() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        let began = Instant::now();
        let root = std::path::PathBuf::from(format!("/tmp/lvb-said-room-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut faces = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "test",
            )
            .unwrap();
        let mut room = bus
            .publisher_declared(
                lamp_msgs::RoomState::TOPIC,
                &lamp_msgs::RoomState::declaration(),
                "test",
            )
            .unwrap();
        let known = root.join("known");
        std::fs::write(&known, format!("{OWNER}\n")).unwrap();
        room.publish_json(
            1,
            &lamp_msgs::RoomState {
                state: lamp_msgs::Room::Quiet,
                another_voice: Some(false),
            },
        )
        .unwrap();
        faces
            .publish_json(
                2,
                &lamp_msgs::Faces {
                    faces: vec![face("f1")],
                },
            )
            .unwrap();
        read_once(&bus, Some(&known), owner_ids);
        assert!(owner_alone_now(Some(began)), "a fresh quiet room");
        // The room is not said again (its publisher stopped); the faces and
        // the known file go on.
        std::thread::sleep(Duration::from_millis(3_300));
        std::fs::write(&known, format!("{OWNER}\n")).unwrap();
        faces
            .publish_json(
                3,
                &lamp_msgs::Faces {
                    faces: vec![face("f1")],
                },
            )
            .unwrap();
        read_once(&bus, Some(&known), owner_ids);
        assert!(
            !owner_alone_now(Some(began)),
            "a quiet room last said over 3 s ago"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// The tracker's word rides `room/state` into the coordinator: a guest
    /// who asked the lamp directly (`another_voice: true`) and a lamp that
    /// tracks nothing (the word absent) both give no scope, while the owner
    /// alone with a tracked `false` does.
    #[test]
    fn the_trackers_word_on_another_voice_decides_on_the_bus() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        let began = Instant::now();
        let root = std::path::PathBuf::from(format!("/tmp/lvb-said-voice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut faces = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "test",
            )
            .unwrap();
        let mut room = bus
            .publisher_declared(
                lamp_msgs::RoomState::TOPIC,
                &lamp_msgs::RoomState::declaration(),
                "test",
            )
            .unwrap();
        let known = root.join("known");
        std::fs::write(&known, format!("{OWNER}\n")).unwrap();
        let mut said = |at: u64, another_voice: Option<bool>| {
            room.publish_json(
                at,
                &lamp_msgs::RoomState {
                    state: lamp_msgs::Room::Quiet,
                    another_voice,
                },
            )
            .unwrap();
            faces
                .publish_json(
                    at,
                    &lamp_msgs::Faces {
                        faces: vec![face("f1")],
                    },
                )
                .unwrap();
            read_once(&bus, Some(&known), owner_ids);
            owner_alone_now(Some(began))
        };
        assert!(said(1, Some(false)), "tracked, no other voice");
        assert!(!said(2, Some(true)), "a guest asked the lamp directly");
        assert!(!said(3, None), "nothing tracks it: the rule is off");
        assert!(said(4, Some(false)), "and back");
        let _ = std::fs::remove_dir_all(root);
    }

    /// The product's voice cannot hear another voice with no face
    /// (`LAMP_SAME_VOICE_FOR=services`), so the scope also needs the owner's
    /// own face seen speaking since the request's input began: a guest out of
    /// frame leaves the owner's mouth still (Thirteen's P2-2 on 2839a288e0).
    #[test]
    fn a_guest_out_of_frame_while_the_owner_sits_still_is_no_scope() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        let root = std::path::PathBuf::from(format!("/tmp/lvb-said-still-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut faces = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "test",
            )
            .unwrap();
        let mut room = bus
            .publisher_declared(
                lamp_msgs::RoomState::TOPIC,
                &lamp_msgs::RoomState::declaration(),
                "test",
            )
            .unwrap();
        let known = root.join("known");
        std::fs::write(&known, format!("{OWNER}\n")).unwrap();
        room.publish_json(
            1,
            &lamp_msgs::RoomState {
                state: lamp_msgs::Room::Quiet,
                another_voice: Some(false),
            },
        )
        .unwrap();
        // The owner sat still while the request's input went on.
        let began = Instant::now();
        faces
            .publish_json(
                2,
                &lamp_msgs::Faces {
                    faces: vec![still_face("f1")],
                },
            )
            .unwrap();
        read_once(&bus, Some(&known), owner_ids);
        assert!(
            !owner_alone_now(Some(began)),
            "the owner alone, quiet, the other-voice word false, but nobody they speak"
        );
        // The owner speaks next: this request is theirs.
        faces
            .publish_json(
                3,
                &lamp_msgs::Faces {
                    faces: vec![face("f1")],
                },
            )
            .unwrap();
        read_once(&bus, Some(&known), owner_ids);
        assert!(owner_alone_now(Some(began)), "the owner spoke it");
        // A later request whose input began over a second after that look
        // is not theirs.
        assert!(
            !owner_alone_now(Some(Instant::now() + Duration::from_secs(2))),
            "spoken only before this input began"
        );
        assert!(!owner_alone_now(None), "no input noted: no scope");
        let _ = std::fs::remove_dir_all(root);
    }

    /// A speaking look counts for the person the eyes named at that moment:
    /// one made while nobody known was named, or while another person was,
    /// is no speech of the owner's now.
    #[test]
    fn a_speaking_look_is_bound_to_the_known_person_it_was_made_under() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        let root = std::path::PathBuf::from(format!("/tmp/lvb-said-bound-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut faces = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "test",
            )
            .unwrap();
        let mut room = bus
            .publisher_declared(
                lamp_msgs::RoomState::TOPIC,
                &lamp_msgs::RoomState::declaration(),
                "test",
            )
            .unwrap();
        let known = root.join("known");
        let began = Instant::now();
        let mut look = |at: u64, who: Option<&str>, speaking: lamp_msgs::Face| {
            room.publish_json(
                at,
                &lamp_msgs::RoomState {
                    state: lamp_msgs::Room::Quiet,
                    another_voice: Some(false),
                },
            )
            .unwrap();
            match who {
                Some(who) => std::fs::write(&known, format!("{who}\n")).unwrap(),
                None => {
                    let _ = std::fs::remove_file(&known);
                }
            }
            faces
                .publish_json(
                    at,
                    &lamp_msgs::Faces {
                        faces: vec![speaking],
                    },
                )
                .unwrap();
            read_once(&bus, Some(&known), owner_ids);
            owner_alone_now(Some(began))
        };
        // Speaking while nobody known is named: the speech is no one's we
        // know; and the owner named only after it, silent, is not it either.
        assert!(!look(1, None, face("f1")));
        assert!(
            !look(2, Some(OWNER), still_face("f1")),
            "spoke unnamed, named only still"
        );
        // Speaking under the owner's name: theirs.
        assert!(look(3, Some(OWNER), face("f1")));
        // The owner named while another person spoke under their own name
        // is not the owner's speech: bound to who was named then.
        reset();
        assert!(!look(4, Some("Someone Else"), face("f1")));
        assert!(!look(5, Some(OWNER), still_face("f1")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_owner_alone_in_view_is_the_owners_scope() {
        let owners = owner_ids();
        assert!(owner_alone(&alone(&owners)));
    }

    #[test]
    fn a_second_face_in_one_look_is_no_scope() {
        let owners = owner_ids();
        let mut e = alone(&owners);
        e.faces_in_one_look = Some(2);
        assert!(!owner_alone(&e));
        // No look at all (camera off, eyes down): none either.
        e.faces_in_one_look = None;
        assert!(!owner_alone(&e));
    }

    #[test]
    fn the_newest_look_decides_at_once_a_closed_camera_or_privacy_has_none() {
        let owners = owner_ids();
        let mut e = alone(&owners);
        e.newest_look_faces = Some(0);
        assert!(!owner_alone(&e), "the newest look has nobody");
        e.newest_look_faces = None;
        assert!(!owner_alone(&e), "no newest look in its 2 s");
    }

    #[test]
    fn a_room_not_quiet_or_a_face_speaking_away_from_the_desk_is_no_scope() {
        let owners = owner_ids();
        let mut e = alone(&owners);
        e.room_quiet = false;
        assert!(!owner_alone(&e));
        e.room_quiet = true;
        e.speaking_not_at_desk = true;
        assert!(!owner_alone(&e));
    }

    /// A guest out of frame asking the lamp directly leaves the room quiet
    /// and the owner the one face in view: only the tracker's word on
    /// another voice stops that reply, and nothing tracking it proves nothing.
    #[test]
    fn a_guest_asking_the_lamp_directly_is_no_scope() {
        let owners = owner_ids();
        let mut e = alone(&owners);
        e.another_voice = Some(true);
        assert!(!owner_alone(&e), "another voice recorded");
        e.another_voice = None;
        assert!(!owner_alone(&e), "nothing tracks it: the rule is off");
        // The other-voice word is blind to a voice with no face: the
        // owner's face must have been seen speaking this request.
        let mut e = alone(&owners);
        e.owner_spoke_alone = false;
        assert!(!owner_alone(&e), "the owner's mouth was still");
    }

    #[test]
    fn a_stale_face_an_unknown_face_or_a_guest_is_no_scope() {
        let owners = owner_ids();
        let mut e = alone(&owners);
        e.known_age = Duration::from_secs(2);
        assert!(!owner_alone(&e));
        let mut e = alone(&owners);
        e.known = None;
        assert!(!owner_alone(&e));
        let mut e = alone(&owners);
        e.known = Some("Someone Else");
        assert!(!owner_alone(&e));
        // No owners at all: nobody matches.
        let none: [String; 0] = [];
        assert!(!owner_alone(&alone(&none)));
    }
}
