//! Whose reminders a voice line hears and changes (Reviewer-three's
//! after-merge P2 on #4527, the first builder's (a)). Each reminder keeps
//! who set it, as a memory person id; a question about "my" reminders lists
//! only the asker's own, and `cancel_reminder` / `snooze_reminder` by id act
//! only on the asker's own and the room's. One set with nobody sure (set
//! before this, or by an unknown face or the room's line) is the room's,
//! heard and changed on every line (the first builder on #4710, 3 Nov 10:09).
//! No switch: a person's reminders narrow to their own line, and the ones
//! with no setter (including any stored before setters existed) widen to
//! every line.
//!
//! Who that is follows memory's scope ([`crate::memory_scope`]):
//! - the **setter** is the person a keep in the line would be written for at
//!   that moment: the line's own person, or, for a line read as the room's,
//!   the known face the eyes named by its end ([`ScopeLease::keeper_within`])
//!   or the one-face keep ([`ScopeLease::kept_by_one_face`], #4584). A
//!   line held only by a face's gap or carried as a conversation keeps
//!   nothing at once, so it sets as nobody's;
//! - the **reader** is the line's person as memory's reads take it
//!   ([`ScopeLease::scope`]): a one-face line reads the room's, so it hears
//!   none of that person's, and a held scope reads nothing of theirs;
//! - with `LAMP_ONE_FACE_REMINDERS` ([`ONE_FACE_ENVIRONMENT`], lamp-29e7 on
//!   3 Nov: the person's face alone in 17 of 17 looks, read speaking in 0,
//!   and "What reminders do I have?" left out their own), a one-face line's
//!   `list_reminders`, `cancel_reminder` and `snooze_reminder` are asked as
//!   the person its keep goes to ([`ScopeLease::kept_by_one_face`]), so it
//!   hears and changes their own reminders and the room's, never another
//!   person's. Only those three: memory's reads stay the room's. Known risk,
//!   accepted by the memory owner on 4 Nov: a voice not seen saying it, with
//!   the person's face alone in view, hears (and may cancel or snooze) the
//!   person's reminders.
use crate::memory_scope::ScopeLease;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

/// Tools that set a reminder or timer, whose caller names its setter.
const SETS: [&str; 3] = ["set_timer", "set_reminder", "repeat_reminder"];

/// The reminder calls that read who set each item (the Schedule's
/// `Asker::may_see`).
const READS: [&str; 3] = ["list_reminders", "cancel_reminder", "snooze_reminder"];

/// `on` exactly; off unless set. The second route's worker reads it.
pub const ONE_FACE_ENVIRONMENT: &str = "LAMP_ONE_FACE_REMINDERS";

/// Whether [`ONE_FACE_ENVIRONMENT`] is on.
pub(crate) fn one_face_on() -> bool {
    std::env::var(ONE_FACE_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// What a reminders forget is told when the reminders could not go.
pub const REMINDERS_KEPT: &str = "The reminders they set could not be cancelled just now; tell them so, and that they can ask you to cancel them.";

/// The reader: the line's person as memory's reads take it, never a held
/// scope's, a revoked line's or the room's.
fn reader(lease: Option<&ScopeLease>) -> Option<String> {
    let scope = lease?.scope()?;
    (!scope.held()).then(|| scope.person().map(str::to_owned))?
}

/// The reader of a reminder call `name` on a one-face line
/// ([`ONE_FACE_ENVIRONMENT`]): the person its keep goes to, for [`READS`]
/// only, while the lease is current, not held and not narrowed since
/// ([`ScopeLease::narrow_to_room`] ends the keep). `None` for every other
/// line: two faces, an unknown face or another voice never take the keep,
/// nor a carried line unless it is read as its person's
/// (`LAMP_CONVERSATION_CARRIED_MEMORY`, `ScopeLease::read_carried`).
fn one_face_reader(lease: Option<&ScopeLease>, name: &str) -> Option<String> {
    let lease = lease.filter(|_| READS.contains(&name))?;
    lease.scope().filter(|scope| !scope.held())?;
    lease.kept_by_one_face()
}

/// The setter: whose scope a keep in this line would be written to now.
fn setter(lease: Option<&ScopeLease>) -> Option<String> {
    let lease = lease?;
    let scope = lease.scope()?;
    if scope.held() {
        return None;
    }
    if let Some(person) = scope.person() {
        return Some(person.to_owned());
    }
    lease
        .keeper_within(crate::memory_scope::KEEPER_WAIT)
        .or_else(|| lease.kept_by_one_face())
}

/// The caller the executive is told beside `name`'s call, for this line:
/// `{"asker":{"person","owner"},"setter"}` (`lamp_actions::schedule::Caller`).
pub fn caller(lease: Option<&ScopeLease>, name: &str) -> Value {
    // The private room (`person::private_room`): this line sees every item,
    // as the app's calls do, and what it sets is everyone's.
    if crate::person::private_room() {
        return json!({"everyone":true});
    }
    caller_among(lease, name, &crate::person::owner_ids(), one_face_on())
}

/// [`caller`] with the owner's memory ids and [`ONE_FACE_ENVIRONMENT`]
/// given. A one-face reader is never the owner's line: on an older
/// Schedule that would list every item, so it lists only their own and the
/// room's, or none.
pub(crate) fn caller_among(
    lease: Option<&ScopeLease>,
    name: &str,
    owners: &[String],
    one_face: bool,
) -> Value {
    let person = reader(lease);
    let owner = person
        .as_deref()
        .is_some_and(|person| owners.iter().any(|owner| owner == person));
    let person = person.or_else(|| one_face.then(|| one_face_reader(lease, name)).flatten());
    let setter = SETS.contains(&name).then(|| setter(lease)).flatten();
    json!({"asker":{"person":person,"owner":owner},"setter":setter})
}

/// After a person's "forget everything about me" emptied their memory, the
/// reminders they set go too, through the executive's own op
/// (`schedule_forget`), never a model's call. Only the line's own person
/// (as memory's forget took it); the room's, a held or a revoked line, or a
/// forget that emptied nothing, leaves them and `told` as they were.
/// `told` says `reminders: {"cancelled": n}` when some went, and
/// [`REMINDERS_KEPT`] when they could not.
pub fn forget_reminders(
    told: (Value, bool),
    directory: &Path,
    lease: &ScopeLease,
) -> (Value, bool) {
    forget_reminders_within(told, directory, lease, super::ASK_DEADLINE)
}

fn forget_reminders_within(
    mut told: (Value, bool),
    directory: &Path,
    lease: &ScopeLease,
    deadline: Duration,
) -> (Value, bool) {
    let emptied =
        told.0["forgotten"] == true || told.0["note"] == json!("I was keeping nothing of them.");
    let Some(person) = reader(Some(lease)).filter(|_| emptied) else {
        return told;
    };
    let line = json!({"op":"schedule_forget","person":person});
    let answer = super::exchange(&directory.join(super::CALL_SOCKET), &line, deadline);
    match answer
        .as_ref()
        .filter(|answer| answer["ok"] == true)
        .and_then(|answer| answer["result"]["forgotten"].as_u64())
    {
        Some(0) => {}
        Some(cancelled) => {
            eprintln!(
                "lamp-voice: a person's forget of everything cancelled the {cancelled} reminders they set"
            );
            told.0["reminders"] = json!({"cancelled": cancelled});
        }
        None => {
            eprintln!(
                "lamp-voice: a person's forget of everything could not cancel the reminders they set"
            );
            told.0["reminders"] = json!(REMINDERS_KEPT);
        }
    }
    told
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
        path::PathBuf,
    };

    fn scope(snapshot: ScopeSnapshot, request: u64) -> RequestScope {
        RequestScope::new(
            RequestKey {
                worker: lamp_turn_owner::BootId::new([7; 16]).unwrap(),
                session: 1,
                request,
            },
            snapshot,
        )
    }
    fn id(name: &str) -> String {
        crate::remember::person_id(name)
    }

    /// Two people each asking "my reminders" are each their own reader; the
    /// owner's line is the owner's; an unknown face, the room, a held gap,
    /// a one-face line and a revoked one read none of anyone's.
    #[test]
    fn each_line_is_its_own_reader_and_only_the_owners_is_the_owner() {
        let owners = [id("Ann")];
        let ann = scope(ScopeSnapshot::of("Ann"), 1);
        assert_eq!(
            caller_among(Some(&ann.lease()), "list_reminders", &owners, false),
            json!({"asker":{"person":id("Ann"),"owner":true},"setter":null})
        );
        let bo = scope(ScopeSnapshot::of("Bo"), 2);
        assert_eq!(
            caller_among(Some(&bo.lease()), "cancel_reminder", &owners, false),
            json!({"asker":{"person":id("Bo"),"owner":false},"setter":null})
        );
        let nobody = json!({"asker":{"person":null,"owner":false},"setter":null});
        let room = scope(ScopeSnapshot::room(), 3);
        assert_eq!(
            caller_among(Some(&room.lease()), "list_reminders", &owners, false),
            nobody
        );
        let held = scope(ScopeSnapshot::held_of("Ann"), 4);
        assert_eq!(
            caller_among(Some(&held.lease()), "list_reminders", &owners, false),
            nobody
        );
        let one_face = scope(ScopeSnapshot::of("Ann"), 5);
        assert!(one_face.lease().take_by_one_face());
        assert_eq!(
            caller_among(Some(&one_face.lease()), "snooze_reminder", &owners, false),
            nobody
        );
        let revoked = scope(ScopeSnapshot::of("Ann"), 6);
        let lease = revoked.lease();
        revoked.revoke();
        assert_eq!(
            caller_among(Some(&lease), "list_reminders", &owners, false),
            nobody
        );
        assert_eq!(caller_among(None, "list_reminders", &owners, false), nobody);
    }

    /// The setter is whom a keep would be written for: the line's person,
    /// the one-face keep on a line that reads the room's, nobody on a held
    /// gap or the room's line.
    #[test]
    fn a_reminder_is_set_as_the_person_a_keep_would_be_written_for() {
        let owners: [String; 0] = [];
        let ann = scope(ScopeSnapshot::of("Ann"), 11);
        assert_eq!(
            caller_among(Some(&ann.lease()), "set_timer", &owners, false)["setter"],
            json!(id("Ann"))
        );
        let one_face = scope(ScopeSnapshot::of("Ann"), 12);
        assert!(one_face.lease().take_by_one_face());
        let told = caller_among(Some(&one_face.lease()), "set_reminder", &owners, false);
        assert_eq!(told["setter"], json!(id("Ann")), "keeps: Ann's");
        assert_eq!(told["asker"]["person"], Value::Null, "reads: the room's");
        let room = scope(ScopeSnapshot::room(), 13);
        assert_eq!(
            caller_among(Some(&room.lease()), "repeat_reminder", &owners, false)["setter"],
            Value::Null
        );
        let named_late = scope(ScopeSnapshot::room(), 14);
        assert!(named_late.lease().keep_for_one_face(&id("Bo")));
        assert_eq!(
            caller_among(Some(&named_late.lease()), "set_timer", &owners, false)["setter"],
            json!(id("Bo"))
        );
        let held = scope(ScopeSnapshot::held_of("Ann"), 15);
        assert_eq!(
            caller_among(Some(&held.lease()), "set_timer", &owners, false)["setter"],
            Value::Null
        );
    }

    /// Whose reminders the line's `asker` lets it hear, as the Schedule
    /// reads it: one set by `set_by` (a memory id), or the room's (`None`).
    fn may_see(told: &Value, set_by: Option<&str>) -> bool {
        serde_json::from_value::<lamp_schedule::protocol::Asker>(told["asker"].clone())
            .unwrap()
            .may_see(set_by)
    }

    /// lamp-29e7 (3 Nov): Ann's face the one face in view, never read
    /// speaking, no other voice. With `LAMP_ONE_FACE_REMINDERS`, "What
    /// reminders do I have?" hears Ann's own and the room's, never Bo's,
    /// and so do the cancel and snooze lookups; a set call, the owner flag
    /// and every line not taken by one face are as without it.
    #[test]
    fn a_one_face_lines_reminder_reads_are_its_persons_own_and_the_rooms() {
        let owners = [id("Ann")];
        let one_face = scope(ScopeSnapshot::of("Ann"), 31);
        assert!(one_face.lease().take_by_one_face());
        for name in READS {
            let told = caller_among(Some(&one_face.lease()), name, &owners, true);
            assert_eq!(
                told,
                json!({"asker":{"person":id("Ann"),"owner":false},"setter":null}),
                "{name}"
            );
            assert!(may_see(&told, Some(&id("Ann"))), "{name}: Ann's own");
            assert!(may_see(&told, None), "{name}: the room's");
            assert!(!may_see(&told, Some(&id("Bo"))), "{name}: never Bo's");
        }
        // The switch off: the room's only, as before.
        let off = caller_among(Some(&one_face.lease()), "list_reminders", &owners, false);
        assert_eq!(off["asker"]["person"], Value::Null, "off");
        assert!(!may_see(&off, Some(&id("Ann"))), "off: Ann's hidden");
        assert!(may_see(&off, None), "off: the room's");
        // A set call's asker is unchanged; its setter is the keep's.
        let set = caller_among(Some(&one_face.lease()), "set_reminder", &owners, true);
        assert_eq!(set["asker"]["person"], Value::Null);
        assert_eq!(set["setter"], json!(id("Ann")));
        // Named late by the eyes (the same one-face decision): Bo's own.
        let named_late = scope(ScopeSnapshot::room(), 32);
        assert!(named_late.lease().keep_for_one_face(&id("Bo")));
        let told = caller_among(Some(&named_late.lease()), "list_reminders", &owners, true);
        assert_eq!(told["asker"]["person"], json!(id("Bo")));
        // Narrowed after (a face seen speaking it that is not Ann's): the
        // room's again.
        let narrowed = scope(ScopeSnapshot::of("Ann"), 33);
        assert!(narrowed.lease().take_by_one_face());
        narrowed.lease().narrow_to_room();
        let told = caller_among(Some(&narrowed.lease()), "list_reminders", &owners, true);
        assert_eq!(told["asker"]["person"], Value::Null, "narrowed");
        // No one-face keep taken (two faces, an unknown face, another
        // voice): the room's line, which still hears the room's reminders.
        let room = scope(ScopeSnapshot::room(), 34);
        let told = caller_among(Some(&room.lease()), "list_reminders", &owners, true);
        assert_eq!(told["asker"]["person"], Value::Null, "the room's");
        assert!(may_see(&told, None), "the room's reminders still show");
        assert!(!may_see(&told, Some(&id("Ann"))));
        // A held scope or a revoked line: nobody's.
        let held = scope(ScopeSnapshot::held_of("Ann"), 35);
        let told = caller_among(Some(&held.lease()), "list_reminders", &owners, true);
        assert_eq!(told["asker"]["person"], Value::Null, "held");
        let revoked = scope(ScopeSnapshot::of("Ann"), 36);
        let lease = revoked.lease();
        assert!(lease.take_by_one_face());
        revoked.revoke();
        let told = caller_among(Some(&lease), "list_reminders", &owners, true);
        assert_eq!(told["asker"]["person"], Value::Null, "revoked");
        // A line seen as Ann's own reads as before, the owner's.
        let ann = scope(ScopeSnapshot::of("Ann"), 37);
        assert_eq!(
            caller_among(Some(&ann.lease()), "list_reminders", &owners, true),
            json!({"asker":{"person":id("Ann"),"owner":true},"setter":null})
        );
    }

    fn executive(name: &str) -> (PathBuf, std::thread::JoinHandle<Value>) {
        let dir = std::env::temp_dir().join(format!("lsw-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join(super::super::CALL_SOCKET)).unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            stream
                .write_all(b"{\"ok\":true,\"result\":{\"forgotten\":2}}\n")
                .unwrap();
            serde_json::from_str(&line).unwrap()
        });
        (dir, worker)
    }

    /// "Forget everything about me": once their memory is emptied, the
    /// reminders the person set go too, by the executive's own op; the
    /// room's forget, or one that emptied nothing, asks nothing.
    #[test]
    fn a_persons_emptied_forget_cancels_their_reminders_and_says_so() {
        let (dir, worker) = executive("forget");
        let ann = scope(ScopeSnapshot::of("Ann"), 21);
        let told = forget_reminders_within(
            (json!({"forgotten": true}), true),
            &dir,
            &ann.lease(),
            Duration::from_secs(5),
        );
        assert_eq!(told.0["reminders"], json!({"cancelled": 2}));
        assert_eq!(
            worker.join().unwrap(),
            json!({"op":"schedule_forget","person":id("Ann")})
        );
        let _ = std::fs::remove_dir_all(&dir);
        let nowhere = Path::new("/nonexistent-lsw");
        let room = scope(ScopeSnapshot::room(), 22);
        let left = forget_reminders_within(
            (json!({"forgotten": true}), true),
            nowhere,
            &room.lease(),
            Duration::from_millis(50),
        );
        assert_eq!(
            left.0,
            json!({"forgotten": true}),
            "the room's: nothing asked"
        );
        let kept = forget_reminders_within(
            (json!({"forgotten": false}), true),
            nowhere,
            &ann.lease(),
            Duration::from_millis(50),
        );
        assert_eq!(kept.0, json!({"forgotten": false}), "nothing emptied");
        let failed = forget_reminders_within(
            (json!({"forgotten": true}), true),
            nowhere,
            &ann.lease(),
            Duration::from_millis(50),
        );
        assert_eq!(failed.0["reminders"], json!(REMINDERS_KEPT));
    }

    /// The private room: the line sees every reminder, whoever set it.
    #[test]
    fn the_private_room_asks_for_everyone() {
        crate::person::set_private_room_for_test(Some(true));
        assert_eq!(caller(None, "list_reminders"), json!({"everyone": true}));
        crate::person::set_private_room_for_test(None);
    }
}
