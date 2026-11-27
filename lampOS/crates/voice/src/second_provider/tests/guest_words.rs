//! #8639, the memory owner's call of 27 Nov 2026 (17:42), "a guest is an
//! unknown face only", as the first builder narrowed it (21:06 on #8903,
//! option b): a line is a guest's only while an unknown face is in view
//! beside a known one, freshly matched, in the same look. An unknown face
//! alone (the owner the eyes miss), nobody in view, the owner alone or a
//! one-turn misread stays the owner's, unmarked and kept as today. A
//! guest line is marked at admission (`remember::guest::mark_line`) for the
//! room's memory, with `LAMP_MEMORY_GUEST_MARKED`.
//!
//! The private room's service records stay as they are today (call 3): the
//! pins below hold that a guest line is still the owner's for the services,
//! the read-backs and the reminders, switch on or off.
use super::*;
use crate::memory_scope::{FaceCache, Gap, ScopeLease};

/// The owner's name, as the eyes' known file names them.
const OWNER: &str = "A";

/// The private room on and `OWNER` the lamp's owner, for this thread, with
/// `LAMP_MEMORY_GUEST_MARKED` as `guest_switch` sets it; put back when
/// dropped (a red assertion included).
struct OwnersRoom;

impl OwnersRoom {
    fn on() -> Self {
        Self::with_switch(true)
    }

    fn with_switch(guest_switch: bool) -> Self {
        crate::person::set_private_room_for_test(Some(true));
        crate::known_faces::owner_for_test(Some(Some(OWNER.to_owned())));
        crate::remember::guest::set_on_for_test(Some(guest_switch));
        Self
    }
}

impl Drop for OwnersRoom {
    fn drop(&mut self) {
        crate::person::set_private_room_for_test(None);
        crate::known_faces::owner_for_test(None);
        crate::remember::guest::set_on_for_test(None);
    }
}

/// The owner's memory id.
fn owner() -> String {
    crate::remember::person_id(OWNER)
}

/// The eyes' known file naming `OWNER`, read as a fresh sighting.
fn owner_seen(what: &str) -> crate::known_faces::FaceObservation {
    let path = std::env::temp_dir().join(format!(
        "guest-words-{what}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, OWNER).unwrap();
    let seen = crate::known_faces::FaceObservation::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    seen
}

/// What the face cache's refreshes read before the line, in order: the
/// eyes' known face (none, or the owner) and the faces in view.
type Refreshes = Vec<(Option<crate::known_faces::FaceObservation>, usize)>;

/// The lease line 1 is admitted with, the face cache refreshed by
/// `refreshes` first, as the worker's refresh does. The cache holds the
/// product's no-face hold (`LAMP_FACE_NO_FACE_HOLD`, on). The engine is kept
/// with it: dropped, it revokes the lease.
fn admitted(refreshes: Refreshes) -> (Engine, ScopeLease) {
    admitted_after(None, refreshes)
}

/// [`admitted`], with an enrolled face matched `matched` before (its
/// sighting ended then) when given, before the refreshes.
fn admitted_after(matched: Option<Duration>, refreshes: Refreshes) -> (Engine, ScopeLease) {
    let cache = Arc::new(FaceCache::holding(Some(Gap::no_face())));
    if let Some(ago) = matched {
        cache.matched_ago(ago);
    }
    let (engine, _, _) = scripted(Arc::new(Script::default()));
    let runner: ScopedToolRunner =
        Arc::new(|_, _, _, _, _: ScopeLease| Box::pin(async { json!({"ok":true}) }));
    let mut engine = engine.with_memory_scope(
        lamp_turn_owner::BootId::new([86; 16]).unwrap(),
        cache.clone(),
        runner,
        None,
    );
    engine.memory_camera(true, 1);
    for (seen, in_view) in refreshes {
        assert!(cache.replace_seen(cache.begin_refresh(), seen, Some(in_view)));
    }
    engine.submit(start(1));
    let lease = engine.memory_lease().expect("line 1 is admitted");
    (engine, lease)
}

/// A guest: an unknown face in view beside the owner's, fresh, in the same
/// look (the first builder's option b, 21:06 on #8903).
fn guest() -> (Engine, ScopeLease) {
    admitted(vec![(Some(owner_seen("beside")), 2)])
}

/// The reminder's record keeper a line's `set_reminder` names.
fn reminder_setter(lease: &ScopeLease) -> Value {
    crate::tools::schedule_whose::record_caller(Some(lease), "set_reminder", &json!({}))["setter"]
        .clone()
}

/// The first builder's option b (21:06 on #8903): an unknown face in view
/// beside a known one, the owner there with them, makes the line a guest's,
/// marked for the room's memory (a write of it is marked only when it is
/// the room's: the owner's own line, seen speaking, stays theirs).
#[tokio::test]
async fn an_unknown_face_beside_a_known_one_is_a_guest_line_marked_for_the_room() {
    let _room = OwnersRoom::on();
    let (_engine, lease) = guest();
    assert!(
        crate::remember::guest::line(lease.key().request),
        "marked a guest's"
    );
}

/// An unknown face alone is never a guest's: not after an earlier match and
/// over 120 s with one unmatched face alone (the owner the eyes miss), not
/// with no face enrolled, and not after a one-turn misread. Nor is the owner
/// alone, or nobody in view. Each stays the room's, kept and unmarked.
#[tokio::test]
async fn an_unknown_face_alone_nobody_or_the_owner_alone_is_never_a_guest() {
    let _room = OwnersRoom::on();
    for (matched, refreshes, why) in [
        (
            Some(crate::memory_scope::NO_FACE_QUIET + Duration::from_secs(1)),
            vec![(None, 1)],
            "matched earlier, then over 120 s with one unmatched face alone",
        ),
        (None, vec![(None, 1), (None, 1)], "no face enrolled"),
        (None, vec![(None, 2)], "two unknown faces, none known"),
        (
            None,
            vec![(Some(owner_seen("misread")), 1), (None, 1)],
            "a one-turn misread",
        ),
        (None, vec![(None, 0)], "nobody in view"),
        (
            None,
            vec![(Some(owner_seen("alone")), 1)],
            "the owner alone",
        ),
    ] {
        let (_engine, lease) = admitted_after(matched, refreshes);
        assert!(!crate::remember::guest::line(lease.key().request), "{why}");
    }
}

/// PIN, green before and after (call 3): a guest line is still the owner's
/// for the private room's service records, read-backs and reminders, as
/// today; so is a line with nobody in view.
#[tokio::test]
async fn the_private_rooms_service_records_are_unchanged_for_a_guest() {
    for switch in [true, false] {
        let _room = OwnersRoom::with_switch(switch);
        for refreshes in [vec![(None, 1)], vec![(None, 0)]] {
            let (_engine, lease) = admitted(refreshes);
            assert_eq!(crate::services::owner_of(Some(&lease)), Some(owner()));
            assert!(crate::services::owner_hears(
                lease.key().request,
                true,
                Some(&lease)
            ));
            assert_eq!(reminder_setter(&lease), json!(owner()));
            assert_eq!(
                crate::tools::schedule_whose::caller(Some(&lease), "list_reminders"),
                json!({"everyone": true})
            );
        }
    }
}

/// With the switch off, nothing is a guest line.
#[tokio::test]
async fn with_the_switch_off_no_line_is_a_guests() {
    let _room = OwnersRoom::with_switch(false);
    let (_engine, lease) = guest();
    assert!(!crate::remember::guest::line(lease.key().request));
}
