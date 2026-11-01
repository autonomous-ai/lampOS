//! Whose reminders a voice line hears and changes (Reviewer-three's
//! after-merge P2 on #4527, the first builder's (a)). Each reminder keeps
//! who set it, as a memory person id; a question about "my" reminders lists
//! the asker's own and the room's reminders. Changes by id follow the same
//! rule. One set with nobody sure (including older unowned items) belongs
//! to the room; a known person's item remains private to that person.
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
//!   none of that person's, and a held scope reads nothing of theirs.
use crate::memory_scope::ScopeLease;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

/// Tools that set a reminder or timer, whose caller names its setter.
const SETS: [&str; 3] = ["set_timer", "set_reminder", "repeat_reminder"];

/// What a reminders forget is told when the reminders could not go.
pub const REMINDERS_KEPT: &str = "The reminders they set could not be cancelled just now; tell them so, and that they can ask you to cancel them.";

/// The reader: the line's person as memory's reads take it, never a held
/// scope's, a revoked line's or the room's.
fn reader(lease: Option<&ScopeLease>) -> Option<String> {
    let scope = lease?.scope()?;
    (!scope.held()).then(|| scope.person().map(str::to_owned))?
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
    caller_among(lease, name, &crate::person::owner_ids())
}

/// [`caller`] with the owner's memory ids given.
pub(crate) fn caller_among(lease: Option<&ScopeLease>, name: &str, owners: &[String]) -> Value {
    let person = reader(lease);
    let owner = person
        .as_deref()
        .is_some_and(|person| owners.iter().any(|owner| owner == person));
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
            caller_among(Some(&ann.lease()), "list_reminders", &owners),
            json!({"asker":{"person":id("Ann"),"owner":true},"setter":null})
        );
        let bo = scope(ScopeSnapshot::of("Bo"), 2);
        assert_eq!(
            caller_among(Some(&bo.lease()), "cancel_reminder", &owners),
            json!({"asker":{"person":id("Bo"),"owner":false},"setter":null})
        );
        let nobody = json!({"asker":{"person":null,"owner":false},"setter":null});
        let room = scope(ScopeSnapshot::room(), 3);
        assert_eq!(
            caller_among(Some(&room.lease()), "list_reminders", &owners),
            nobody
        );
        let held = scope(ScopeSnapshot::held_of("Ann"), 4);
        assert_eq!(
            caller_among(Some(&held.lease()), "list_reminders", &owners),
            nobody
        );
        let one_face = scope(ScopeSnapshot::of("Ann"), 5);
        assert!(one_face.lease().take_by_one_face());
        assert_eq!(
            caller_among(Some(&one_face.lease()), "snooze_reminder", &owners),
            nobody
        );
        let revoked = scope(ScopeSnapshot::of("Ann"), 6);
        let lease = revoked.lease();
        revoked.revoke();
        assert_eq!(
            caller_among(Some(&lease), "list_reminders", &owners),
            nobody
        );
        assert_eq!(caller_among(None, "list_reminders", &owners), nobody);
    }

    /// The setter is whom a keep would be written for: the line's person,
    /// the one-face keep on a line that reads the room's, nobody on a held
    /// gap or the room's line.
    #[test]
    fn a_reminder_is_set_as_the_person_a_keep_would_be_written_for() {
        let owners: [String; 0] = [];
        let ann = scope(ScopeSnapshot::of("Ann"), 11);
        assert_eq!(
            caller_among(Some(&ann.lease()), "set_timer", &owners)["setter"],
            json!(id("Ann"))
        );
        let one_face = scope(ScopeSnapshot::of("Ann"), 12);
        assert!(one_face.lease().take_by_one_face());
        let told = caller_among(Some(&one_face.lease()), "set_reminder", &owners);
        assert_eq!(told["setter"], json!(id("Ann")), "keeps: Ann's");
        assert_eq!(told["asker"]["person"], Value::Null, "reads: the room's");
        let room = scope(ScopeSnapshot::room(), 13);
        assert_eq!(
            caller_among(Some(&room.lease()), "repeat_reminder", &owners)["setter"],
            Value::Null
        );
        let named_late = scope(ScopeSnapshot::room(), 14);
        assert!(named_late.lease().keep_for_one_face(&id("Bo")));
        assert_eq!(
            caller_among(Some(&named_late.lease()), "set_timer", &owners)["setter"],
            json!(id("Bo"))
        );
        let held = scope(ScopeSnapshot::held_of("Ann"), 15);
        assert_eq!(
            caller_among(Some(&held.lease()), "set_timer", &owners)["setter"],
            Value::Null
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
}
