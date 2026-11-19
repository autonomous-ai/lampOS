//! Forget reaches the account's history (#7771): what a forget removes on
//! the lamp is deleted from the conversation history the account keeps
//! too, by the lamp's own cloud API (`POST <base>/history/turns/delete`,
//! with the lamp's key). Only with `LAMP_HISTORY_FORGET=on` (off: nothing
//! is queued or sent).
//!
//! The lamp's own forget is the only one the person waits for: "forgotten"
//! follows the memory's answer, as before. What it removed goes on a queue
//! here, persisted in the voice's state folder ([`FILE`]), and is sent in
//! the background by [`crate::second_provider::net::history`]:
//!
//! - **forget everything** of a known person (`forget_all` of a `p-` id,
//!   answered `ok`): that person's whole history, `{"all":true,
//!   "person":<their id>}`. A delete of everything never goes without a
//!   person ([`Call::All`] always names one).
//! - **any other forget** (`forget`, `forget_last`, `exchanges_forget`,
//!   `forget_said`, and the room's lines a forget everything took): the
//!   exchanges the memory says it removed (its answer's `exchanges`, by
//!   scope and id, and every page `forgot_exchanges` gives when it says
//!   `exchanges_more`), `{"ids":[…]}` as the memory tells them. Every removed id is
//!   told: a forget never deletes more of the account's history than the
//!   lamp forgot. A whole-history delete is a known person's forget
//!   everything only.
//! - **not reached:** a forget of everything of every person, or a page
//!   that could not be read, is said in the journal as not reaching the
//!   account (the voice sends neither today).
//!
//! The person key is lamp-memory's own scope id: a known person's `p-`
//! id (the eyes' person id), or `""` for the room's (`everyone`), as the
//! server's `person` is "the lamp's own person key, or empty". A saved
//! turn's id is the memory's own, `<epoch>.<scope>:<id>`
//! ([`lamp_msgs::history_turn_id`], the store's epoch changed by a wipe, so
//! an exchange id given again after one never names an older turn): the
//! memory makes it in its forget answers and this sends it as told; the
//! turn's save (#7771's other half) makes the same with the same function.
//!
//! Bounded: at most [`MOST_PENDING`] ids wait, far past anything real (an
//! id is about 30 bytes). Past it, the known person with the most waiting
//! is coalesced into a delete of their whole history, never the room's
//! (whose `""` would take every room line); the room's newest past it are
//! let go. Either is journalled as a count. Only counts are journalled,
//! never an id or a word.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// `on` and nothing else: forgets reach the account's history. Off in
/// every unit.
pub const ENVIRONMENT: &str = "LAMP_HISTORY_FORGET";
/// The queue's file, in the voice's state folder.
pub const FILE: &str = "/var/lib/lampOS/voice/history-forget.json";
/// The most ids that wait (about 600 KB); past it, see [`Pending::add_ids`].
pub const MOST_PENDING: usize = 20_000;
/// The most `forgot_exchanges` pages read after one forget.
const MOST_PAGES: usize = 64;
/// The most ids one delete names (the server's cap).
pub const MOST_IDS_A_CALL: usize = 100;
/// The room's scope in lamp-memory; its person key on the server is `""`.
const ROOM: &str = "everyone";

/// Whether [`ENVIRONMENT`] is `on`, read once.
pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on"))
}

/// The server's person key for a memory scope: a known person's `p-` id,
/// `""` for the room's; `None` for any other scope (a rules scope, a slug),
/// which no saved turn has.
pub fn person_key(scope: &str) -> Option<&str> {
    if scope == ROOM {
        Some("")
    } else if lamp_msgs::known::is_person_id(scope) {
        Some(scope)
    } else {
        None
    }
}

/// One delete call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// Everything of one person (`""` the room's): never without one.
    All { person: String },
    /// These turns, at most [`MOST_IDS_A_CALL`], of one person.
    Ids { person: String, ids: Vec<String> },
}

impl Call {
    /// The request's body.
    pub fn body(&self) -> Value {
        match self {
            Self::All { person } => serde_json::json!({"all": true, "person": person}),
            Self::Ids { ids, .. } => serde_json::json!({"ids": ids}),
        }
    }

    /// How many ids it names (an `all` none), for the journal.
    pub fn ids(&self) -> usize {
        match self {
            Self::All { .. } => 0,
            Self::Ids { ids, .. } => ids.len(),
        }
    }
}

/// What waits, as kept on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// Person keys whose whole history is to go.
    #[serde(default)]
    pub all: BTreeSet<String>,
    /// Each person key's turn ids to go.
    #[serde(default)]
    pub ids: BTreeMap<String, BTreeSet<String>>,
}

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.all.is_empty() && self.ids.is_empty()
    }

    fn count(&self) -> usize {
        self.ids.values().map(BTreeSet::len).sum()
    }

    /// Everything of `person`: their ids wait no more.
    pub fn add_all(&mut self, person: &str) {
        self.ids.remove(person);
        self.all.insert(person.to_owned());
    }

    /// `ids` of `person`, unless all of theirs already waits (it takes
    /// these too); then bounded ([`MOST_PENDING`]): the known person with
    /// the most waiting is coalesced into their whole history, never the
    /// room's `""`; with only the room's left past it, its newest are let
    /// go. Each said in the journal as a count.
    pub fn add_ids(&mut self, person: &str, ids: impl IntoIterator<Item = String>) {
        if self.all.contains(person) {
            return;
        }
        let mut ids = ids.into_iter().peekable();
        if ids.peek().is_none() {
            return;
        }
        self.ids.entry(person.to_owned()).or_default().extend(ids);
        while self.count() > MOST_PENDING {
            let most = self
                .ids
                .iter()
                .filter(|(person, _)| !person.is_empty())
                .max_by_key(|(_, ids)| ids.len())
                .map(|(person, ids)| (person.clone(), ids.len()));
            match most {
                Some((most, waiting)) => {
                    eprintln!(
                        "lamp-voice history: the forget queue is full; {waiting} turns of one person go as their whole history"
                    );
                    self.add_all(&most);
                }
                None => {
                    let over = self.count() - MOST_PENDING;
                    if let Some(room) = self.ids.get_mut("") {
                        for _ in 0..over {
                            room.pop_last();
                        }
                    }
                    eprintln!(
                        "lamp-voice history: the forget queue is full; {over} of the room's turns are not reached on the account"
                    );
                }
            }
        }
    }

    /// The next call: a person's whole history first, then up to
    /// [`MOST_IDS_A_CALL`] of one person's ids.
    pub fn next(&self) -> Option<Call> {
        if let Some(person) = self.all.first() {
            return Some(Call::All {
                person: person.clone(),
            });
        }
        let (person, ids) = self.ids.iter().find(|(_, ids)| !ids.is_empty())?;
        Some(Call::Ids {
            person: person.clone(),
            ids: ids.iter().take(MOST_IDS_A_CALL).cloned().collect(),
        })
    }

    /// `call` done: what it named waits no more.
    pub fn done(&mut self, call: &Call) {
        match call {
            Call::All { person } => {
                self.all.remove(person);
            }
            Call::Ids { person, ids } => {
                if let Some(kept) = self.ids.get_mut(person) {
                    for id in ids {
                        kept.remove(id);
                    }
                    if kept.is_empty() {
                        self.ids.remove(person);
                    }
                }
            }
        }
    }

    /// What a memory request `line` and its `answer` add (nothing unless it
    /// is one of memory's forgets, or a page of one, answered `ok`): a
    /// `forget_all` of a known person, their whole history; the exchanges
    /// the answer names, their turns. A `forget_all` of every person
    /// reaches nothing on the account, said in the journal.
    pub fn add_answered(&mut self, line: &Value, answer: &Value) {
        let op = line["op"].as_str().unwrap_or_default();
        let forget = lamp_msgs::MEMORY_FORGETS_TELLING_EXCHANGES.contains(&op);
        if answer["ok"] != true || !(forget || op == lamp_msgs::MEMORY_FORGOT_EXCHANGES_OP) {
            return;
        }
        if op == "forget_all" {
            match line.get("person") {
                Some(Value::String(person)) if lamp_msgs::known::is_person_id(person) => {
                    self.add_all(person);
                }
                // Every person's: memory tells no ids for it.
                Some(Value::Null) => eprintln!(
                    "lamp-voice history: a forget of every person's memory is not reached on the account"
                ),
                // The room's: its lines come by id below.
                _ => {}
            }
        }
        if let Some(scopes) = answer["exchanges"].as_object() {
            for (scope, ids) in scopes {
                let Some(person) = person_key(scope) else {
                    continue;
                };
                // Each the memory's own turn id, made by
                // `lamp_msgs::history_turn_id` with its store's epoch, and
                // of the scope it is told under: none else is sent.
                let ids = ids
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|turn| lamp_msgs::history_turn_scope(turn) == Some(scope.as_str()))
                    .map(str::to_owned);
                self.add_ids(person, ids);
            }
        }
    }
}

/// The queue: [`Pending`] kept in `path`, read again before each change so
/// that what another process of the voice added is never lost to a stale
/// copy, and written whole (a new file renamed over it) after.
pub struct Queue {
    path: PathBuf,
    lock: Mutex<()>,
}

impl Queue {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    /// What waits now; an unreadable file is nothing (said once in the
    /// journal by [`Self::change`]).
    pub fn pending(&self) -> Pending {
        read(&self.path).unwrap_or_default()
    }

    /// `change` applied to what waits, written back when it changed.
    pub fn change(&self, change: impl FnOnce(&mut Pending)) -> io::Result<Pending> {
        let _held = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let before = match read(&self.path) {
            Ok(pending) => pending,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Pending::default(),
            Err(_) => {
                eprintln!(
                    "lamp-voice history: the forget queue could not be read; it starts again empty"
                );
                Pending::default()
            }
        };
        let mut pending = before.clone();
        change(&mut pending);
        if pending != before {
            write(&self.path, &pending)?;
        }
        Ok(pending)
    }
}

fn read(path: &Path) -> io::Result<Pending> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn write(path: &Path, pending: &Pending) -> io::Result<()> {
    if pending.is_empty() {
        return match fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("no file name"))?;
    let new = path.with_file_name(format!(".{name}.new"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&new)?;
    file.write_all(&serde_json::to_vec(pending).map_err(io::Error::other)?)?;
    file.sync_all()?;
    fs::rename(&new, path)
}

/// The voice's queue at [`FILE`], and the bell that wakes its sender.
pub fn queue() -> &'static (Queue, Arc<tokio::sync::Notify>) {
    static QUEUE: OnceLock<(Queue, Arc<tokio::sync::Notify>)> = OnceLock::new();
    QUEUE.get_or_init(|| (Queue::at(FILE), Arc::new(tokio::sync::Notify::new())))
}

/// A memory request `line` to `socket` answered `answer`: with the switch
/// on, what it removed is queued for the account's history, with every
/// page of the rest read from `socket` ([`pages`]), and the sender woken.
/// Off, nothing. On a forget's way only for a small file's write, and for
/// the pages' reads when it removed more ids than one answer names (400).
pub fn answered(socket: &Path, line: &Value, answer: &Value) {
    if !on() || answer["ok"] != true {
        return;
    }
    let (queue, bell) = queue();
    answered_into(true, queue, bell, line, answer);
    if answer["exchanges_more"] == true {
        for page in pages(|page| crate::tools::exchange_up_to(socket, page, PAGE_WITHIN, 65_536)) {
            answered_into(true, queue, bell, &page.0, &page.1);
        }
    }
}

/// How long one page may take.
const PAGE_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// The pages of `forgot_exchanges`, each its request and answer, asked by
/// `ask` until one says no `more`, at most [`MOST_PAGES`]. A page not
/// answered ends them, said in the journal as not reached.
pub fn pages(ask: impl Fn(&Value) -> Option<Value>) -> Vec<(Value, Value)> {
    let line = serde_json::json!({"op": lamp_msgs::MEMORY_FORGOT_EXCHANGES_OP});
    let mut pages = Vec::new();
    for _ in 0..MOST_PAGES {
        match ask(&line) {
            Some(answer) if answer["ok"] == true => {
                let more = answer["more"] == true;
                pages.push((line.clone(), answer));
                if !more {
                    return pages;
                }
            }
            _ => break,
        }
    }
    eprintln!(
        "lamp-voice history: the rest of a forget's turns could not be read; they are not reached on the account"
    );
    pages
}

/// [`answered`] on a given queue, `on` or not, with no pages.
pub fn answered_into(
    on: bool,
    queue: &Queue,
    bell: &tokio::sync::Notify,
    line: &Value,
    answer: &Value,
) {
    if !on {
        return;
    }
    let mut added = Pending::default();
    added.add_answered(line, answer);
    if added.is_empty() {
        return;
    }
    match queue.change(|pending| pending.add_answered(line, answer)) {
        Ok(pending) => {
            eprintln!(
                "lamp-voice history: a forget queued for the account's history ({} whole, {} turns waiting)",
                pending.all.len(),
                pending.count()
            );
            bell.notify_one();
        }
        Err(_) => eprintln!("lamp-voice history: a forget could not be queued (the file)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MINH: &str = "p-0123456789abcdef";
    const LAN: &str = "p-fedcba9876543210";

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lamp-history-forget-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("history-forget.json")
    }

    fn bell() -> tokio::sync::Notify {
        tokio::sync::Notify::new()
    }

    /// A memory store's epoch, as its answers carry it.
    const EPOCH: &str = "0a1b2c3d";

    fn turn_id(scope: &str, id: u64) -> String {
        lamp_msgs::history_turn_id(EPOCH, scope, id)
    }

    fn turns(scope: &str, ids: impl IntoIterator<Item = u64>) -> Vec<String> {
        ids.into_iter().map(|id| turn_id(scope, id)).collect()
    }

    /// Forget everything of a known person queues their whole history,
    /// with their person key, and never a delete of everything without one.
    #[test]
    fn forget_everything_queues_the_persons_whole_history() {
        let queue = Queue::at(scratch("all"));
        answered_into(
            true,
            &queue,
            &bell(),
            &json!({"op":"forget_all","person":MINH}),
            &json!({"ok":true,"removed":4}),
        );
        let next = queue.pending().next().unwrap();
        assert_eq!(next.body(), json!({"all":true,"person":MINH}));
        // The room's lines it took go by id, under the room's empty key.
        let mut pending = Pending::default();
        pending.add_answered(
            &json!({"op":"forget_all","person":MINH}),
            &json!({"ok":true,"removed":4,"exchanges":{"everyone":turns("everyone", [3])}}),
        );
        assert!(pending.all.contains(MINH));
        assert_eq!(pending.ids[""], BTreeSet::from([turn_id("everyone", 3)]));
        // Refused, or a forget_all with no known person: nothing.
        for (line, answer) in [
            (
                json!({"op":"forget_all","person":MINH}),
                json!({"ok":false}),
            ),
            (json!({"op":"forget_all"}), json!({"ok":true,"removed":1})),
            (
                json!({"op":"forget_all","person":"r-0123456789abcdef"}),
                json!({"ok":true,"removed":1}),
            ),
            (json!({"op":"forget_all","person":null}), json!({"ok":true})),
        ] {
            let mut pending = Pending::default();
            pending.add_answered(&line, &answer);
            assert!(pending.is_empty(), "{line} {answer}");
        }
    }

    /// "Forget that" queues exactly the exchanges the memory removed, by
    /// their turn ids, in one call for that person.
    #[test]
    fn forget_that_queues_exactly_the_removed_ids() {
        let queue = Queue::at(scratch("ids"));
        answered_into(
            true,
            &queue,
            &bell(),
            &json!({"op":"forget","person":MINH,"id":9,"selective":true}),
            &json!({"ok":true,"removed":3,"exchanges":{
                MINH:turns(MINH, [4, 6]),
                "r-0123456789abcdef":turns("r-0123456789abcdef", [1]),
            }}),
        );
        let pending = queue.pending();
        assert!(pending.all.is_empty());
        assert_eq!(
            pending.next().unwrap(),
            Call::Ids {
                person: MINH.into(),
                ids: vec![turn_id(MINH, 4), turn_id(MINH, 6)],
            }
        );
        assert_eq!(
            pending.next().unwrap().body(),
            json!({"ids":["0a1b2c3d.p-0123456789abcdef:4","0a1b2c3d.p-0123456789abcdef:6"]})
        );
        // A forget that removed no exchange queues nothing.
        let empty = Queue::at(scratch("ids-none"));
        answered_into(
            true,
            &empty,
            &bell(),
            &json!({"op":"forget","person":MINH,"id":9}),
            &json!({"ok":true,"removed":1}),
        );
        assert!(empty.pending().is_empty());
    }

    /// Switched off, a forget queues nothing at all.
    #[test]
    fn switched_off_queues_nothing() {
        let path = scratch("off");
        let queue = Queue::at(&path);
        answered_into(
            false,
            &queue,
            &bell(),
            &json!({"op":"forget_all","person":MINH}),
            &json!({"ok":true,"removed":4,"exchanges":{"everyone":turns("everyone", [3])}}),
        );
        assert!(queue.pending().is_empty());
        assert!(!path.exists());
    }

    /// What waits is on disk: a new queue at the same file (the voice
    /// started again) has it, and a call done leaves no file behind.
    #[test]
    fn the_queue_survives_a_restart() {
        let path = scratch("restart");
        Queue::at(&path)
            .change(|pending| {
                pending.add_ids(MINH, [turn_id(MINH, 1)]);
                pending.add_all(LAN);
            })
            .unwrap();
        let again = Queue::at(&path);
        let pending = again.pending();
        assert!(pending.all.contains(LAN));
        assert_eq!(pending.ids[MINH].len(), 1);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        again
            .change(|pending| {
                pending.done(&Call::All { person: LAN.into() });
                pending.done(&Call::Ids {
                    person: MINH.into(),
                    ids: vec![turn_id(MINH, 1)],
                });
            })
            .unwrap();
        assert!(!path.exists());
    }

    /// A forget of 600 turns in one scope sends 600 ids and no whole
    /// history, whatever pages they came in; a known person's forget
    /// everything that took 450 room lines sends those ids and never a
    /// whole-history delete for the room (the first builder's P1 on #7935).
    #[test]
    fn a_forget_never_deletes_more_than_the_lamp_forgot() {
        let ids = |scope: &str, range: std::ops::Range<u64>| turns(scope, range);
        let mut pending = Pending::default();
        pending.add_answered(
            &json!({"op":"exchanges_forget","person":MINH}),
            &json!({"ok":true,"removed":600,"exchanges":{MINH:ids(MINH, 0..400)},"exchanges_more":true}),
        );
        let paged =
            pages(|_| Some(json!({"ok":true,"exchanges":{MINH:ids(MINH, 400..600)},"more":false})));
        assert_eq!(paged.len(), 1);
        for (line, answer) in &paged {
            pending.add_answered(line, answer);
        }
        assert!(pending.all.is_empty());
        let mut sent = 0;
        while let Some(call) = pending.next() {
            assert!(call.body().get("all").is_none(), "{}", call.body());
            sent += call.ids();
            pending.done(&call);
        }
        assert_eq!(sent, 600);
        let mut everything = Pending::default();
        everything.add_answered(
            &json!({"op":"forget_all","person":LAN}),
            &json!({"ok":true,"removed":900,"exchanges":{"everyone":ids("everyone", 0..400)},"exchanges_more":true}),
        );
        for (line, answer) in pages(|_| {
            Some(json!({"ok":true,"exchanges":{"everyone":ids("everyone", 400..450)},"more":false}))
        }) {
            everything.add_answered(&line, &answer);
        }
        assert_eq!(everything.all, BTreeSet::from([LAN.to_owned()]));
        assert_eq!(everything.ids[""].len(), 450);
        // A page not answered ends the pages.
        assert!(pages(|_| None).is_empty());
    }

    /// With 450 waiting, a second person's 51 stay 51; only past
    /// [`MOST_PENDING`] is the person with the most coalesced, never the
    /// room's; a call names at most [`MOST_IDS_A_CALL`].
    #[test]
    fn the_bound_coalesces_only_the_person_with_the_most_never_the_room() {
        let mut pending = Pending::default();
        pending.add_ids(MINH, (0..450).map(|id| turn_id(MINH, id)));
        pending.add_ids(LAN, (0..51).map(|id| turn_id(LAN, id)));
        assert!(pending.all.is_empty());
        assert_eq!(pending.ids[LAN].len(), 51);
        assert_eq!(pending.count(), 501);
        let mut full = Pending::default();
        full.add_ids(
            "",
            (0..MOST_PENDING as u64 - 10).map(|id| turn_id("everyone", id)),
        );
        full.add_ids(LAN, (0..5).map(|id| turn_id(LAN, id)));
        full.add_ids(MINH, (0..20).map(|id| turn_id(MINH, id)));
        assert_eq!(full.all, BTreeSet::from([MINH.to_owned()]));
        assert_eq!(full.ids[LAN].len(), 5);
        // Only the room's past it: its newest are let go, never its whole.
        full.add_ids("", (0..100).map(|id| turn_id("everyone", 1_000_000 + id)));
        assert!(!full.all.contains(""));
        assert!(full.count() <= MOST_PENDING);
        let mut many = Pending::default();
        many.add_ids(LAN, (0..250).map(|id| turn_id(LAN, id)));
        assert_eq!(many.next().unwrap().ids(), MOST_IDS_A_CALL);
    }

    /// Only memory's forgets (and their pages) are read: another answer's
    /// `exchanges` queues nothing; a forget of every person's memory
    /// queues nothing and never a delete without a person; the room's own
    /// forget everything goes by its ids.
    #[test]
    fn only_a_forget_answer_queues_and_never_all_without_a_person() {
        for op in ["status", "recall", "search", "kept", "adopt", "remember"] {
            let mut pending = Pending::default();
            pending.add_answered(
                &json!({"op":op,"person":MINH}),
                &json!({"ok":true,"exchanges":{MINH:turns(MINH, [1, 2])}}),
            );
            assert!(pending.is_empty(), "{op}");
        }
        let mut every = Pending::default();
        every.add_answered(
            &json!({"op":"forget_all","person":null}),
            &json!({"ok":true,"removed":9}),
        );
        assert!(every.is_empty());
        let mut room = Pending::default();
        room.add_answered(
            &json!({"op":"forget_all"}),
            &json!({"ok":true,"removed":2,"exchanges":{"everyone":turns("everyone", [1, 2])}}),
        );
        assert!(room.all.is_empty());
        assert_eq!(room.ids[""].len(), 2);
        room.add_all(MINH);
        while let Some(call) = room.next() {
            let body = call.body();
            if body.get("all").is_some() {
                assert!(body["person"].is_string(), "{body}");
            }
            room.done(&call);
        }
    }

    /// Only the memory's own turn ids, of the scope they are told under,
    /// are sent: an id without the store's epoch (an older memory's bare
    /// number or `<scope>:<id>`) or under another scope is let go, never
    /// sent as some other turn (#7771, Backend3's (a) on #7957).
    #[test]
    fn only_turn_ids_with_the_stores_epoch_are_sent() {
        let mut pending = Pending::default();
        pending.add_answered(
            &json!({"op":"forget","person":MINH,"id":9}),
            &json!({"ok":true,"removed":5,"exchanges":{MINH:[
                4,
                "p-0123456789abcdef:5",
                turn_id(LAN, 6),
                "0a1b2c3d.p-0123456789abcdef:x",
                turn_id(MINH, 7),
            ]}}),
        );
        assert_eq!(pending.ids[MINH], BTreeSet::from([turn_id(MINH, 7)]));
        assert_eq!(pending.count(), 1);
    }
}
