//! #8639: a visitor's words and the household's memory. A *visitor line*
//! is a line admitted with an unknown face in view, no known face seen in
//! this conversation's time (`memory_scope::RoomReason::NeverKnown`), and
//! no session person. Under `LAMP_PRIVATE_ROOM` (on in the product) every
//! line is the owner's for the services today, so a stranger alone at the
//! lamp is told the owner's service records and linked reads, and a
//! reminder they set is kept as the owner's.
//!
//! These tests hold only what every option the memory owner may rule
//! (#8639's map, section 6: A, B or C) shares: a visitor is never the
//! owner, for memory or for the services, and nothing a visitor says is
//! kept as the owner's. Whether a visitor's line is kept for the room at
//! all is the ruling's, and is not tested here.
//!
//! The pins beside them must stay green through the build: with nobody in
//! view the private room still makes the line the owner's, a known face
//! seen moments ago (a misread) still does, and the owner's known face
//! beside an unknown one keeps today's behaviour.
use super::*;
use crate::memory_scope::{FaceCache, Gap, ScopeLease};

/// The owner's name, as the eyes' known file names them.
const OWNER: &str = "A";

/// The private room on and `OWNER` the lamp's owner, for this thread, with
/// `LAMP_MEMORY_VISITOR_NOT_KEPT` as `visitor_switch` sets it; put back
/// when dropped (a red assertion included).
struct OwnersRoom;

impl OwnersRoom {
    /// With the switch on, as every test here but the off pin runs.
    fn on() -> Self {
        Self::with_switch(true)
    }

    fn with_switch(visitor_switch: bool) -> Self {
        crate::person::set_private_room_for_test(Some(true));
        crate::known_faces::owner_for_test(Some(Some(OWNER.to_owned())));
        crate::remember::visitor::set_on_for_test(Some(visitor_switch));
        Self
    }
}

impl Drop for OwnersRoom {
    fn drop(&mut self) {
        crate::person::set_private_room_for_test(None);
        crate::known_faces::owner_for_test(None);
        crate::remember::visitor::set_on_for_test(None);
    }
}

/// The owner's memory id.
fn owner() -> String {
    crate::remember::person_id(OWNER)
}

/// The eyes' known file naming `OWNER`, read as a fresh sighting.
fn owner_seen(what: &str) -> crate::known_faces::FaceObservation {
    let path = std::env::temp_dir().join(format!(
        "visitor-words-{what}-{}-{:?}",
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
/// `refreshes` first, as the worker's refresh does
/// (`FaceCache::replace_marked` with the eyes' faces in view). The cache
/// holds the product's no-face hold (`LAMP_FACE_NO_FACE_HOLD`, on). The engine is kept with it: dropped, it revokes the lease.
fn admitted(refreshes: Refreshes) -> (Engine, ScopeLease) {
    let cache = Arc::new(FaceCache::holding(Some(Gap::no_face())));
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

/// A visitor: one unknown face in view, no known face ever seen.
fn visitor() -> (Engine, ScopeLease) {
    admitted(vec![(None, 1)])
}

/// The reminder's record keeper a line's `set_reminder` names
/// (`schedule_whose::record_caller`, `LAMP_SERVICE_RECORDS_REMINDERS`).
fn reminder_setter(lease: &ScopeLease) -> Value {
    crate::tools::schedule_whose::record_caller(Some(lease), "set_reminder", &json!({}))["setter"]
        .clone()
}

/// A visitor asking is told nothing of the owner's:
/// not their memory (the line is the room's, never theirs), and not their
/// service records, linked reads or private read-backs (`services::owner_of`
/// is the one gate for `records_noting`, `linked_noting`,
/// `service_records::source::run` and `service_read_ids`; `owner_hears`
/// for a made skill's read-back). services.rs's `owner_of` (the private
/// room's early return) and `owner_hears` (its own) make it the owner's
/// today.
#[tokio::test]
async fn a_visitor_is_told_nothing_of_the_owners_memory_or_service_records() {
    let _room = OwnersRoom::on();
    let (_engine, lease) = visitor();
    let scope = lease.scope().expect("a current lease");
    assert_eq!(
        scope.person(),
        None,
        "memory: the room's, never the owner's"
    );
    assert_eq!(
        lease.reached_by_one_face(crate::remember::RECALL),
        None,
        "a recall reaches nobody's memory by one face"
    );
    assert_eq!(
        crate::services::owner_of(Some(&lease)),
        None,
        "services: a visitor is never the owner (records, linked reads)"
    );
    assert!(
        !crate::services::owner_hears(lease.key().request, true, Some(&lease)),
        "a private read-back is never said whole to a visitor"
    );
}

/// Nothing a visitor says is kept as the owner's: a
/// keep runs in no person's scope, and a reminder they set is not kept as
/// the owner's record (`schedule_whose::private_room_record_caller` falls
/// back to the owner today).
#[tokio::test]
async fn a_visitors_line_is_never_kept_as_the_owners() {
    let _room = OwnersRoom::on();
    let (_engine, lease) = visitor();
    let scope = lease.scope().expect("a current lease");
    assert_eq!(scope.person(), None, "a keep is never in the owner's scope");
    assert_eq!(
        lease.reached_by_one_face(crate::remember::REMEMBER),
        None,
        "a keep reaches nobody by one face"
    );
    assert_eq!(lease.kept_by_one_face(), None);
    assert_ne!(
        reminder_setter(&lease),
        json!(owner()),
        "a visitor's reminder is not kept as the owner's record"
    );
}

/// PIN, green now and after. Nobody in view, no known face ever: the
/// private room makes the line the owner's for the services, as today
/// (their off-camera "what's on my calendar?").
#[tokio::test]
async fn with_nobody_in_view_the_private_room_line_is_still_the_owners() {
    let _room = OwnersRoom::on();
    let (_engine, lease) = admitted(vec![(None, 0)]);
    assert_eq!(lease.scope().unwrap().person(), None, "memory: the room's");
    assert_eq!(crate::services::owner_of(Some(&lease)), Some(owner()));
    assert!(crate::services::owner_hears(
        lease.key().request,
        true,
        Some(&lease)
    ));
    assert_eq!(reminder_setter(&lease), json!(owner()));
}

/// PIN, green now and after. The owner's face seen moments ago, now an
/// unknown face in view (a misread: `RoomReason::NoKnown`, within
/// `NO_FACE_QUIET`): not a visitor, so the line stays the owner's (the
/// owner's 23 Nov rule: a one-turn misread keeps the user).
#[tokio::test]
async fn a_known_face_seen_moments_ago_is_a_misread_and_stays_the_owners() {
    let _room = OwnersRoom::on();
    let (_engine, lease) = admitted(vec![(Some(owner_seen("misread")), 1), (None, 1)]);
    assert_eq!(lease.scope().unwrap().person(), None, "memory: the room's");
    assert_eq!(crate::services::owner_of(Some(&lease)), Some(owner()));
    assert!(crate::services::owner_hears(
        lease.key().request,
        true,
        Some(&lease)
    ));
    assert_eq!(reminder_setter(&lease), json!(owner()));
}

/// PIN, green now and after. The owner's known face beside an unknown one:
/// the line is admitted in the owner's scope and is the owner's for the
/// services, as today (`LAMP_PERSON_SCOPE_SEEN` may still narrow it at the
/// line's end; that is unchanged).
#[tokio::test]
async fn the_owners_known_face_beside_an_unknown_one_keeps_todays_behaviour() {
    let _room = OwnersRoom::on();
    let (_engine, lease) = admitted(vec![(Some(owner_seen("beside")), 2)]);
    assert_eq!(lease.scope().unwrap().person(), Some(owner().as_str()));
    assert_eq!(crate::services::owner_of(Some(&lease)), Some(owner()));
    assert!(crate::services::owner_hears(
        lease.key().request,
        true,
        Some(&lease)
    ));
    assert_eq!(reminder_setter(&lease), json!(owner()));
}

/// A visitor at the lamp: the private room's other checks yield too. Their
/// reminders list asks as nobody's (`schedule_whose::caller`, not
/// everyone's), the records are never told in full, no yes of theirs is
/// the owner's by the private room, and the app's lines and tools are not
/// theirs (`remember::visitor::private_room`).
#[tokio::test]
async fn a_visitor_gets_no_reminders_records_or_app_lines_of_the_owners() {
    let _room = OwnersRoom::on();
    let (_engine, lease) = visitor();
    let request = lease.key().request;
    assert!(crate::remember::visitor::of(Some(&lease)));
    assert!(!crate::remember::visitor::private_room(Some(&lease)));
    assert!(!crate::remember::visitor::private_room_for_request(request));
    assert_ne!(
        crate::tools::schedule_whose::caller(Some(&lease), "list_reminders"),
        json!({"everyone": true}),
        "a visitor never lists everyone's reminders"
    );
    assert!(!crate::services::records_in_full(request, &owner()));
    assert!(!crate::services::owners_yes_of(request, true, None));
    // The newest line is the visitor's: a run's write is not read back.
    assert!(!crate::services::owner_sees_alone_now());
    assert!(!crate::run_write_yes::read_back_allowed_now());
}

/// PIN: the switch off, a visitor's line is as stock: the private room
/// makes it the owner's for the services and the reminders.
#[tokio::test]
async fn with_the_switch_off_a_visitors_line_is_as_stock() {
    let _room = OwnersRoom::with_switch(false);
    let (_engine, lease) = visitor();
    assert_eq!(lease.scope().unwrap().person(), None, "memory: the room's");
    assert!(!crate::remember::visitor::of(Some(&lease)));
    assert!(crate::remember::visitor::private_room(Some(&lease)));
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
    assert!(crate::services::records_in_full(
        lease.key().request,
        &owner()
    ));
}

/// PIN: nobody in view and the misread are never visitor lines, so the
/// other checks keep the private room too.
#[tokio::test]
async fn nobody_in_view_and_a_misread_are_never_visitor_lines() {
    let _room = OwnersRoom::on();
    for refreshes in [
        vec![(None, 0)],
        vec![(Some(owner_seen("never-visitor")), 1), (None, 1)],
    ] {
        let (_engine, lease) = admitted(refreshes);
        assert!(!crate::remember::visitor::of(Some(&lease)));
        assert!(crate::remember::visitor::private_room_for_request(
            lease.key().request
        ));
        assert_eq!(
            crate::tools::schedule_whose::caller(Some(&lease), "list_reminders"),
            json!({"everyone": true})
        );
    }
}
