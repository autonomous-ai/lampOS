//! Each conversation turn the memory keeps, saved to the account's history
//! store too (#7771, `LAMP_HISTORY_SEND=on`, off): the product owner's
//! decision of 18 Nov that a person's conversations are kept on the server,
//! and Backend3's live store of 19 Nov, `POST /api/v1/ai/history/turns` with
//! the lamp's own key (`x-api-key`). Sent only to the lamp's own cloud API,
//! the relay host the second route's `llm_base_url` names ([`url`]); with a
//! direct provider's base nothing is sent. The second route only: a Live
//! route turn is never saved. Off in every unit: by the first builder's
//! rulings (#7935, and 17:09 on #7957) the save goes on in the product only
//! after the forget half (`LAMP_HISTORY_FORGET`) is on, with its row.
//!
//! **What is sent:** only an exchange the Days memory wrote (`append_linked`
//! answered with its id), in a known person's scope (`p-…`) or the room's,
//! as `{id, at, person, heard, said}`: the person's line as heard and the
//! lamp's reply, when it was said, and whose it is (a known person's memory
//! id, or empty for the room's). No audio, no picture, no kept thing, no
//! observation, no rule. A turn the memory keeps out (a keep or forget turn,
//! a line carried or held by a face's gap, one made before a forget, one
//! whose source was forgotten) is never written and so never sent. A reply
//! read from a linked service (mail, calendar, a page) is saved as
//! [`crate::thread::PRIVATE_SAID`], never the read: the thread keeps only
//! that for the memory, and a turn in a request a service read answered in
//! is given that placeholder here too.
//!
//! **The id** is `lamp_msgs::history::turn_id`, `<store>:<scope>:<exchange
//! id>`: the Days store's own id (made with the store, gone at a reset), the
//! memory scope and the exchange's id in it, the same a forget names when it
//! deletes (`LAMP_HISTORY_FORGET`). A reset lamp's new turns are neither
//! taken as already saved nor refused by an old tombstone. The store takes a
//! resend of an id as a no-op, so a retry is always safe.
//!
//! **The room's lines wait for their conversation's end:** with
//! `LAMP_MEMORY_FOLLOWS_FACE` (or `LAMP_MEMORY_EARLY_WORDS`) a room line may
//! still move to the caller's own scope, with a new id, until then
//! (`adopt`). A moved line is sent under its new id and person
//! ([`Sender::moved`]); the rest go when the conversation ends
//! ([`Sender::settled`]). One held past [`HELD_FOR`] waits for the next
//! start.
//!
//! **A forget said on the lamp** ([`Sender::forgetting`]) holds every send
//! until the memory answers it ([`Sender::forget_answered`]): the ids the
//! memory removed are let go and kept as forgotten, so a save naming one is
//! dropped before any POST, the first included (the lamp's second guard;
//! the server keeps a tombstone). When the answer does not name its ids,
//! the forgetting person's and the room's unsent turns wait for the next
//! start, when the memory no longer holds what it forgot. Other people's
//! unsent turns are never let go. [`deleting`] takes the ids a forget
//! queues for deletion, by their full id.
//!
//! **Nothing lost at a restart, and no words on disk:** only a mark per
//! scope, the last id the server took with none before it unsent, is kept,
//! in [`MARKS_FILE`] (0600, ids only). At start the sending thread asks the
//! memory for its store and each scope's last id, and sends what the memory
//! kept after the marks; the memory stays the one copy of the words. A
//! store unlike the marks' (a first start, or a reset) starts the marks at
//! what the memory holds now: nothing before the switch went on is sent.
//!
//! **Never on the turn's path:** the memory's writer thread hands each turn
//! over under a short lock with no I/O, into a bounded queue (at most
//! [`READY_MOST`] waiting; past it the oldest waits for the next start). A
//! thread of its own sends them in order, at most [`BATCH_MOST`] a call,
//! each text cut to [`TEXT_MOST`] bytes, and tries a failed call again with
//! the same ids, up to [`TRIES`] times, waiting longer each time; one not
//! saved then waits for the next start too. The marks are written on that
//! thread. The journal has counts only, never a word, an id or a key.
//!
//! What was said before the privacy switch closed may still be sent while
//! it is closed: nothing new is said to the lamp then.
use lamp_msgs::history::{ROOM_SCOPE, is_store_id, turn_id};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

/// `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_HISTORY_SEND";
/// The store's save call, below the lamp's cloud API base (`…/api/v1/ai`).
pub const PATH: &str = "/history/turns";
/// The marks per scope, in the voice's state folder: ids only.
pub const MARKS_FILE: &str = "/var/lib/lampOS/voice/history-marks.json";
/// The store's caps (Backend3, 19 Nov): 20 turns a call, 8 KB a text.
pub const BATCH_MOST: usize = 20;
pub const TEXT_MOST: usize = 8 * 1024;
/// Turns waiting to be sent, at most; past it the oldest waits for the next
/// start.
pub const READY_MOST: usize = 200;
/// The room's turns held for their conversation's end, at most.
pub const HELD_MOST: usize = 64;
/// The longest a room turn is held; past it, it waits for the next start.
pub const HELD_FOR: Duration = Duration::from_secs(30 * 60);
/// Tries of one batch, the first included.
pub const TRIES: u32 = 6;
/// The longest one call takes.
pub const WITHIN: Duration = Duration::from_secs(10);
/// The longest sends wait for a forget's answer; past it, the turns waiting
/// then wait for the next start.
pub const FORGET_HOLD: Duration = Duration::from_secs(15);
/// The forgotten ids kept for the guard, newest last.
const FORGOTTEN_MOST: usize = 4_096;
/// The longest answer read.
const ANSWER_MOST: usize = 65_536;
/// The memory's answers read at start, and how long one may take.
const MEMORY_ANSWER_MOST: u64 = 131_072;
const MEMORY_WITHIN: Duration = Duration::from_secs(2);

/// Whether the switch is on.
pub fn on() -> bool {
    on_from(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Exactly `on`.
pub fn on_from(value: Option<&str>) -> bool {
    value == Some("on")
}

/// A memory scope a forget can reach on the account: a known person's
/// `p-` id, or the room's.
fn reachable(scope: &str) -> bool {
    scope == ROOM_SCOPE || lamp_msgs::known::is_person_id(scope)
}

/// One turn as the store takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pending {
    /// The memory scope: a known person's `p-` id, or [`ROOM_SCOPE`].
    scope: String,
    exchange: u64,
    at_unix_ms: u64,
    heard: String,
    said: String,
    since: Instant,
}

impl Pending {
    fn wire(&self, store: &str) -> Value {
        json!({
            "id": turn_id(store, &self.scope, self.exchange),
            "at": utc(self.at_unix_ms),
            "person": if self.scope == ROOM_SCOPE { "" } else { self.scope.as_str() },
            "heard": self.heard,
            "said": self.said,
        })
    }
}

/// `text` cut to at most [`TEXT_MOST`] bytes, at a character's end.
pub fn capped(text: &str) -> String {
    let mut end = text.len().min(TEXT_MOST);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// Milliseconds since 1970 UTC as the store's `at`, `2026-11-18T11:40:02Z`.
pub fn utc(at_unix_ms: u64) -> String {
    let seconds = (at_unix_ms / 1_000) as i64;
    let (day, second) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, date) = crate::local_time::civil(day);
    format!(
        "{year:04}-{month:02}-{date:02}T{:02}:{:02}:{:02}Z",
        second / 3_600,
        second % 3_600 / 60,
        second % 60
    )
}

/// What the store's answer to a save means for the batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Saved (a resend of a saved id included).
    Saved,
    /// The store's switch is off (`{"enabled":false}`): no server copy, so
    /// the batch is not tried again.
    Off,
    /// Not reached, or the store failed or was busy: tried again.
    Again,
    /// Refused for the request itself, by this HTTP status: never tried
    /// again.
    Refused(u16),
}

/// The store's answer, by its HTTP status and body, in its envelope
/// (`{"status": 1, "data": ...}`) or not. A timeout, a limit or the
/// store's own failure is tried again; any other refusal is not.
pub fn answer_of(status: u16, body: &[u8]) -> Answer {
    match status {
        200..=299 => {}
        408 | 429 | 500..=599 => return Answer::Again,
        _ => return Answer::Refused(status),
    }
    let Ok(answer) = serde_json::from_slice::<Value>(body) else {
        return Answer::Again;
    };
    if answer.get("status").and_then(Value::as_i64) == Some(0) {
        return Answer::Refused(status);
    }
    let data = answer
        .get("data")
        .filter(|data| data.is_object())
        .unwrap_or(&answer);
    if data.get("enabled") == Some(&Value::Bool(false)) {
        Answer::Off
    } else if data.get("saved").is_some_and(Value::is_array) {
        Answer::Saved
    } else {
        Answer::Again
    }
}

/// How long a failed batch waits before its next try: `first`, doubled
/// each time, at most `longest`.
#[derive(Clone, Copy, Debug)]
pub struct Waits {
    pub first: Duration,
    pub longest: Duration,
}

impl Default for Waits {
    fn default() -> Self {
        Self {
            first: Duration::from_secs(2),
            longest: Duration::from_secs(60),
        }
    }
}

/// One save call: the body in, the store's answer out.
pub type Post = Box<dyn FnMut(&Value) -> Answer + Send>;
/// One line asked of the memory, and its answer if it gave one.
pub type Ask = Box<dyn FnMut(&Value) -> Option<Value> + Send>;

#[derive(Default)]
struct Queue {
    ready: VecDeque<Pending>,
    held: VecDeque<Pending>,
    /// The store's id, once the memory told it: nothing is posted before.
    store: Option<String>,
    /// Sends wait for a forget's answer until then, and whose forget it is.
    paused_until: Option<Instant>,
    paused_scope: String,
    /// Ids a forget removed, by scope and id, newest last, and each scope
    /// a forget everything emptied, through the last id offered then.
    forgotten: HashSet<(String, u64)>,
    forgotten_order: VecDeque<(String, u64)>,
    emptied: BTreeMap<String, u64>,
    /// The last id offered in each scope this run.
    offered: BTreeMap<String, u64>,
    /// Ids queued, held or being sent, by scope.
    outstanding: BTreeMap<String, BTreeSet<u64>>,
    /// The lowest id in each scope that waits for the next start.
    deferred: BTreeMap<String, u64>,
    /// The highest id the server took in each scope this run.
    acked: BTreeMap<String, u64>,
    /// The marks as last written.
    marks: BTreeMap<String, u64>,
    let_go: u64,
}

impl Queue {
    fn is_forgotten(&self, scope: &str, exchange: u64) -> bool {
        self.forgotten.contains(&(scope.to_owned(), exchange))
            || self
                .emptied
                .get(scope)
                .is_some_and(|through| exchange <= *through)
    }

    fn remember_forgotten(&mut self, scope: &str, exchange: u64) {
        let key = (scope.to_owned(), exchange);
        if self.forgotten.insert(key.clone()) {
            self.forgotten_order.push_back(key);
            if self.forgotten_order.len() > FORGOTTEN_MOST
                && let Some(old) = self.forgotten_order.pop_front()
            {
                self.forgotten.remove(&old);
            }
        }
    }

    fn take_outstanding(&mut self, scope: &str, exchange: u64) {
        if let Some(ids) = self.outstanding.get_mut(scope) {
            ids.remove(&exchange);
            if ids.is_empty() {
                self.outstanding.remove(scope);
            }
        }
    }

    /// Not sent now: it waits for the next start.
    fn defer(&mut self, pending: &Pending) {
        self.take_outstanding(&pending.scope, pending.exchange);
        let lowest = self
            .deferred
            .entry(pending.scope.clone())
            .or_insert(pending.exchange);
        *lowest = (*lowest).min(pending.exchange);
        self.let_go += 1;
    }

    /// Forgotten: never sent, and nothing waits for it.
    fn drop_forgotten(&mut self, pending: &Pending) {
        self.take_outstanding(&pending.scope, pending.exchange);
        self.let_go += 1;
    }

    /// The server took it (or answered that it never will).
    fn acked(&mut self, pending: &Pending) {
        self.take_outstanding(&pending.scope, pending.exchange);
        let high = self.acked.entry(pending.scope.clone()).or_insert(0);
        *high = (*high).max(pending.exchange);
    }

    /// Into the queue to send; past [`READY_MOST`] the oldest waits for the
    /// next start.
    fn ready_push(&mut self, pending: Pending) {
        if self.ready.len() >= READY_MOST
            && let Some(old) = self.ready.pop_front()
        {
            self.defer(&old);
        }
        self.ready.push_back(pending);
    }

    /// Room turns held past [`HELD_FOR`] wait for the next start.
    fn age_out(&mut self, now: Instant) {
        let (old, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|pending| now.saturating_duration_since(pending.since) >= HELD_FOR);
        self.held = kept.into();
        for pending in &old {
            self.defer(pending);
        }
    }

    /// The held room turns `take` picks moved to the queue to send, each
    /// as `take` changes it; the others stay held.
    fn release(&mut self, mut take: impl FnMut(&mut Pending) -> bool) {
        for mut pending in std::mem::take(&mut self.held) {
            let (scope, exchange) = (pending.scope.clone(), pending.exchange);
            if take(&mut pending) {
                if pending.scope != scope || pending.exchange != exchange {
                    self.take_outstanding(&scope, exchange);
                    self.outstanding
                        .entry(pending.scope.clone())
                        .or_default()
                        .insert(pending.exchange);
                }
                self.ready_push(pending);
            } else {
                self.held.push_back(pending);
            }
        }
    }

    /// Every unsent turn `pick` names let go: as forgotten, or to wait for
    /// the next start.
    fn let_go_where(&mut self, forgotten: bool, pick: impl Fn(&Pending) -> bool) {
        for queue in [
            std::mem::take(&mut self.ready),
            std::mem::take(&mut self.held),
        ]
        .into_iter()
        .enumerate()
        {
            let (which, pendings) = queue;
            for pending in pendings {
                if !pick(&pending) {
                    if which == 0 {
                        self.ready.push_back(pending);
                    } else {
                        self.held.push_back(pending);
                    }
                } else if forgotten {
                    self.drop_forgotten(&pending);
                } else {
                    self.defer(&pending);
                }
            }
        }
    }

    /// A scope's mark: the highest id the server took with nothing below it
    /// still unsent; never lower than the mark written.
    fn mark(&self, scope: &str) -> Option<u64> {
        let written = self.marks.get(scope).copied();
        let acked = self.acked.get(scope).copied()?;
        let low = [
            self.outstanding
                .get(scope)
                .and_then(|ids| ids.first().copied()),
            self.deferred.get(scope).copied(),
        ]
        .into_iter()
        .flatten()
        .min();
        let mark = match low {
            Some(low) => acked.min(low.saturating_sub(1)),
            None => acked,
        };
        Some(written.map_or(mark, |written| written.max(mark)))
    }
}

type Shared = (Mutex<Queue>, Condvar);

fn locked(shared: &Shared) -> MutexGuard<'_, Queue> {
    shared
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn wait_on<'a>(
    shared: &'a Shared,
    queue: MutexGuard<'a, Queue>,
    wait: Duration,
) -> MutexGuard<'a, Queue> {
    shared
        .1
        .wait_timeout(queue, wait)
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .0
}

/// A forget's hold, if any, ended at its bound: the turns waiting then wait
/// for the next start. True while it still holds.
fn held_by_forget(queue: &mut Queue, now: Instant) -> bool {
    match queue.paused_until {
        Some(until) if now < until => true,
        Some(_) => {
            queue.paused_until = None;
            let scope = std::mem::take(&mut queue.paused_scope);
            queue.let_go_where(false, |pending| {
                pending.scope == scope || pending.scope == ROOM_SCOPE
            });
            eprintln!(
                "lamp-voice history: a forget gave no answer in time; the turns waiting go at the next start ({ENVIRONMENT})"
            );
            false
        }
        None => false,
    }
}

/// The next turns to send, oldest first, at most [`BATCH_MOST`], and the
/// store's id; waits until there are some, the store is known and no
/// forget holds.
fn next_batch(shared: &Shared) -> (Vec<Pending>, String) {
    let mut queue = locked(shared);
    loop {
        let now = Instant::now();
        queue.age_out(now);
        let holding = held_by_forget(&mut queue, now);
        if !holding
            && !queue.ready.is_empty()
            && let Some(store) = queue.store.clone()
        {
            let take = queue.ready.len().min(BATCH_MOST);
            let batch = queue.ready.drain(..take).collect();
            return (batch, store);
        }
        let wait = match queue.paused_until {
            Some(until) => until
                .saturating_duration_since(now)
                .max(Duration::from_millis(1)),
            None => Duration::from_secs(60),
        };
        queue = wait_on(shared, queue, wait);
    }
}

/// How a batch ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ended {
    Answered(Answer),
    /// Not saved in its tries, or held past a forget's bound: waits for the
    /// next start.
    Waits,
    /// Everything in it was forgotten before it went.
    Forgotten,
}

/// One batch, tried up to [`TRIES`] times. Before every try, the first
/// included, it waits for a forget's answer and drops each turn forgotten
/// (the lamp's guard); the rest go with the same ids.
fn send_batch(
    shared: &Shared,
    post: &mut Post,
    batch: &mut Vec<Pending>,
    store: &str,
    waits: Waits,
) -> Ended {
    let mut wait = waits.first;
    for tries in 1..=TRIES {
        {
            let mut queue = locked(shared);
            loop {
                let now = Instant::now();
                match queue.paused_until {
                    Some(until) if now < until => {
                        queue = wait_on(shared, queue, until - now);
                    }
                    Some(_) => {
                        // The hold ran out: the forgetting person's and the
                        // room's wait for the next start, here as queued.
                        let scope = queue.paused_scope.clone();
                        held_by_forget(&mut queue, now);
                        let (waits, rest): (Vec<_>, Vec<_>) =
                            std::mem::take(batch).into_iter().partition(|pending| {
                                pending.scope == scope || pending.scope == ROOM_SCOPE
                            });
                        for pending in &waits {
                            queue.defer(pending);
                        }
                        *batch = rest;
                    }
                    None => break,
                }
            }
            let (forgotten, rest): (Vec<_>, Vec<_>) = std::mem::take(batch)
                .into_iter()
                .partition(|pending| queue.is_forgotten(&pending.scope, pending.exchange));
            for pending in &forgotten {
                queue.drop_forgotten(pending);
            }
            *batch = rest;
        }
        if batch.is_empty() {
            return Ended::Forgotten;
        }
        let body = json!({
            "turns": batch.iter().map(|pending| pending.wire(store)).collect::<Vec<_>>()
        });
        let answer = post(&body);
        if answer != Answer::Again {
            return Ended::Answered(answer);
        }
        if tries == TRIES {
            break;
        }
        let until = Instant::now() + wait;
        let mut queue = locked(shared);
        while let Some(left) = until
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
        {
            queue = wait_on(shared, queue, left);
        }
        wait = (wait * 2).min(waits.longest);
    }
    Ended::Waits
}

/// The marks file's contents: the store's id and each scope's mark.
fn read_marks(path: &Path) -> Option<(String, BTreeMap<String, u64>)> {
    let bytes = crate::config::read_private(path, 65_536).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let store = value.get("store")?.as_str()?.to_owned();
    let marks = value
        .get("marks")?
        .as_object()?
        .iter()
        .filter_map(|(scope, mark)| Some((scope.clone(), mark.as_u64()?)))
        .collect();
    Some((store, marks))
}

/// The marks written whole (a new file renamed into place), 0600.
fn write_marks(path: &Path, store: &str, marks: &BTreeMap<String, u64>) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let text = json!({"store": store, "marks": marks}).to_string();
    let fresh = path.with_extension("json.new");
    let _ = std::fs::remove_file(&fresh);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&fresh)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&fresh, path)
}

/// Each scope's mark that rose written down, off the turn's path.
fn keep_marks(shared: &Shared, path: Option<&Path>, scopes: &BTreeSet<String>) {
    let mut queue = locked(shared);
    let Some(store) = queue.store.clone() else {
        return;
    };
    let mut rose = false;
    for scope in scopes {
        if let Some(mark) = queue.mark(scope)
            && queue.marks.get(scope) != Some(&mark)
        {
            queue.marks.insert(scope.clone(), mark);
            rose = true;
        }
    }
    let marks = queue.marks.clone();
    drop(queue);
    if rose
        && let Some(path) = path
        && write_marks(path, &store, &marks).is_err()
    {
        eprintln!("lamp-voice history: the marks could not be written ({ENVIRONMENT})");
    }
}

/// What the memory tells at start: its store's id and each scope's last id.
fn ask_store(ask: &mut Ask) -> Option<(String, BTreeMap<String, u64>)> {
    let answer = ask(&json!({"op":"store"}))?;
    let store = answer.get("store")?.as_str().filter(|s| is_store_id(s))?;
    let issued = answer
        .get("issued")?
        .as_object()?
        .iter()
        .filter(|(scope, _)| reachable(scope))
        .filter_map(|(scope, last)| Some((scope.clone(), last.as_u64()?)))
        .collect();
    Some((store.to_owned(), issued))
}

/// The start: the store's id from the memory (asked until it answers), the
/// marks read, and what the memory kept after them sent; a new store (or
/// no marks) starts the marks at what the memory holds now.
fn catch_up(shared: &Shared, post: &mut Post, ask: &mut Ask, marks: Option<&Path>, waits: Waits) {
    let (store, issued) = loop {
        if let Some(told) = ask_store(ask) {
            break told;
        }
        let queue = locked(shared);
        drop(wait_on(shared, queue, waits.first));
    };
    let kept = marks.and_then(read_marks);
    let from = match kept {
        Some((kept_store, kept)) if kept_store == store => Some(kept),
        _ => None,
    };
    {
        let mut queue = locked(shared);
        queue.store = Some(store.clone());
        queue.marks = from.clone().unwrap_or_else(|| issued.clone());
        shared.1.notify_all();
    }
    let Some(from) = from else {
        if let Some(path) = marks
            && write_marks(path, &store, &issued).is_err()
        {
            eprintln!("lamp-voice history: the marks could not be written ({ENVIRONMENT})");
        }
        return;
    };
    let mut sent = 0usize;
    for (scope, last) in &issued {
        let mut after = from.get(scope).copied().unwrap_or(0);
        while after < *last {
            let mut line = json!({"op":"exchanges_after","after":after,"limit":BATCH_MOST});
            if scope != ROOM_SCOPE {
                line["person"] = json!(scope);
            }
            let Some(page) = ask(&line).filter(|page| page["ok"] == true) else {
                eprintln!(
                    "lamp-voice history: the memory did not give what was unsent; it goes at the next start ({ENVIRONMENT})"
                );
                return;
            };
            let mut batch: Vec<Pending> = page["exchanges"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|exchange| {
                    Some(Pending {
                        scope: scope.clone(),
                        exchange: exchange["id"].as_u64().filter(|id| id <= last)?,
                        at_unix_ms: exchange["at_unix_ms"].as_u64()?,
                        heard: capped(exchange["heard"].as_str()?),
                        said: capped(exchange["said"].as_str()?),
                        since: Instant::now(),
                    })
                })
                .collect();
            let Some(highest) = batch.iter().map(|pending| pending.exchange).max() else {
                break;
            };
            {
                let mut queue = locked(shared);
                for pending in &batch {
                    queue
                        .outstanding
                        .entry(scope.clone())
                        .or_default()
                        .insert(pending.exchange);
                }
            }
            let count = batch.len();
            let ended = send_batch(shared, post, &mut batch, &store, waits);
            ended_batch(shared, &batch, ended);
            keep_marks(shared, marks, &BTreeSet::from([scope.clone()]));
            if ended == Ended::Waits {
                eprintln!(
                    "lamp-voice history: the store did not take what was unsent; it goes at the next start ({ENVIRONMENT})"
                );
                return;
            }
            sent += count;
            after = highest;
            if page["more"] != true {
                break;
            }
        }
    }
    if sent > 0 {
        eprintln!(
            "lamp-voice history: {sent} turns unsent before the start were sent ({ENVIRONMENT})"
        );
    }
}

/// A batch's turns as it ended: taken, forgotten, or waiting for the next
/// start.
fn ended_batch(shared: &Shared, batch: &[Pending], ended: Ended) {
    let mut queue = locked(shared);
    for pending in batch {
        match ended {
            Ended::Answered(_) => queue.acked(pending),
            Ended::Waits => queue.defer(pending),
            Ended::Forgotten => queue.drop_forgotten(pending),
        }
    }
}

/// The sending thread: the start's catch-up, then each batch in order; a
/// change of whether the store saves said once, and each let-go counted,
/// never a word or an id.
fn send_all(shared: &Shared, mut post: Post, mut ask: Ask, marks: Option<PathBuf>, waits: Waits) {
    catch_up(shared, &mut post, &mut ask, marks.as_deref(), waits);
    let mut saving = None;
    loop {
        let (mut batch, store) = next_batch(shared);
        let ended = send_batch(shared, &mut post, &mut batch, &store, waits);
        ended_batch(shared, &batch, ended);
        let scopes = batch.iter().map(|pending| pending.scope.clone()).collect();
        keep_marks(shared, marks.as_deref(), &scopes);
        let now_saving = match ended {
            Ended::Answered(Answer::Saved) => Some(true),
            Ended::Answered(_) | Ended::Waits => Some(false),
            Ended::Forgotten => None,
        };
        if now_saving.is_some() && saving != now_saving {
            saving = now_saving;
            eprintln!(
                "lamp-voice history: the account's history store is {} ({ENVIRONMENT})",
                match ended {
                    Ended::Answered(Answer::Saved) => "saving turns",
                    Ended::Answered(Answer::Off) => "off on the server: nothing is kept there",
                    _ => "not saving turns",
                }
            );
        }
        let why = match ended {
            Ended::Answered(Answer::Refused(status)) => Some(format!("refused, HTTP {status}")),
            Ended::Waits => Some(format!(
                "not saved in {TRIES} tries; they go at the next start"
            )),
            _ => None,
        };
        if let Some(why) = why {
            eprintln!(
                "lamp-voice history: {} turns not saved: {why} ({ENVIRONMENT})",
                batch.len()
            );
        }
    }
}

/// The queue and its sending thread.
#[derive(Clone, Default)]
pub struct Sender {
    shared: Arc<Shared>,
}

impl Sender {
    /// A queue with no thread yet: what is offered waits for [`Self::spawn`].
    pub fn new() -> Self {
        Self::default()
    }

    /// The thread that sends, with `post` for each call, `ask` for the
    /// memory and the marks at `marks` (none: kept in RAM, for tests).
    pub fn spawn(
        &self,
        post: Post,
        ask: Ask,
        marks: Option<PathBuf>,
        waits: Waits,
    ) -> std::io::Result<()> {
        let shared = Arc::clone(&self.shared);
        std::thread::Builder::new()
            .name("lamp-voice-history".into())
            .spawn(move || send_all(&shared, post, ask, marks, waits))
            .map(|_| ())
    }

    /// The memory wrote exchange `exchange` in `person`'s scope (`None`: the
    /// room's, held until its conversation ends); `private`: a linked
    /// service's read answered in its request, so the reply goes as
    /// [`crate::thread::PRIVATE_SAID`]. Never waits on the network: a short
    /// lock, no I/O. A scope no forget can reach (not a `p-` id) is never
    /// sent.
    pub fn offer(
        &self,
        person: Option<&str>,
        exchange: u64,
        at_unix_ms: u64,
        (heard, said): (&str, &str),
        private: bool,
    ) {
        let scope = person.unwrap_or(ROOM_SCOPE);
        if !reachable(scope) {
            return;
        }
        let pending = Pending {
            scope: scope.to_owned(),
            exchange,
            at_unix_ms,
            heard: capped(heard),
            said: if private {
                crate::thread::PRIVATE_SAID.to_owned()
            } else {
                capped(said)
            },
            since: Instant::now(),
        };
        let mut queue = locked(&self.shared);
        let offered = queue.offered.entry(pending.scope.clone()).or_insert(0);
        *offered = (*offered).max(exchange);
        queue
            .outstanding
            .entry(pending.scope.clone())
            .or_default()
            .insert(exchange);
        if person.is_some() {
            queue.ready_push(pending);
            self.shared.1.notify_all();
        } else {
            if queue.held.len() >= HELD_MOST
                && let Some(old) = queue.held.pop_front()
            {
                queue.defer(&old);
            }
            queue.held.push_back(pending);
        }
    }

    /// The room's exchanges `moved` (old id, new id) are `to`'s now
    /// (`adopt`): each held one is sent as theirs, under its new id.
    pub fn moved(&self, to: &str, moved: &[(u64, u64)]) {
        if !reachable(to) {
            return;
        }
        locked(&self.shared).release(|pending| {
            let Some((_, new)) = moved.iter().find(|(old, _)| *old == pending.exchange) else {
                return false;
            };
            pending.exchange = *new;
            pending.scope = to.to_owned();
            true
        });
        self.shared.1.notify_all();
    }

    /// The room's conversation of `exchanges` ended: they can no longer
    /// move, and are sent as the room's.
    pub fn settled(&self, exchanges: &[u64]) {
        locked(&self.shared).release(|pending| exchanges.contains(&pending.exchange));
        self.shared.1.notify_all();
    }

    /// A forget by `person` (`None`: the room's) is being asked of the
    /// memory: nothing is posted until [`Self::forget_answered`]; past
    /// [`FORGET_HOLD`] with no answer, their and the room's unsent turns
    /// wait for the next start.
    pub fn forgetting(&self, person: Option<&str>) {
        let mut queue = locked(&self.shared);
        queue.paused_until = Some(Instant::now() + FORGET_HOLD);
        queue.paused_scope = person.unwrap_or(ROOM_SCOPE).to_owned();
        drop(queue);
        self.shared.1.notify_all();
    }

    /// The memory's `answer` to a forget by `person` (`None`: the room's),
    /// `everything` for a forget everything: the ids it names removed are
    /// let go and kept as forgotten (`exchanges`, by scope; or `removed`'s
    /// one exchange); a forget everything's scope is forgotten through the
    /// last id offered in it. With no ids named, the person's and the
    /// room's unsent turns wait for the next start. Sends go on after.
    pub fn forget_answered(&self, person: Option<&str>, everything: bool, answer: Option<&Value>) {
        let scope = person.unwrap_or(ROOM_SCOPE).to_owned();
        let mut queue = locked(&self.shared);
        let ok = answer.is_some_and(|answer| answer["ok"] == true);
        let named: Option<Vec<(String, u64)>> = answer.and_then(|answer| {
            if let Some(by_scope) = answer.get("exchanges").and_then(Value::as_object) {
                return Some(
                    by_scope
                        .iter()
                        .flat_map(|(scope, ids)| {
                            ids.as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_u64)
                                .map(|id| (scope.clone(), id))
                                .collect::<Vec<_>>()
                        })
                        .collect(),
                );
            }
            let removed = answer.get("removed")?;
            (removed.get("kind").and_then(Value::as_str) == Some("exchange"))
                .then(|| removed.get("id").and_then(Value::as_u64))
                .flatten()
                .map(|id| vec![(scope.clone(), id)])
        });
        if ok && everything {
            let through = queue.offered.get(&scope).copied().unwrap_or(0);
            let emptied = queue.emptied.entry(scope.clone()).or_insert(0);
            *emptied = (*emptied).max(through);
            let gone = scope.clone();
            queue.let_go_where(true, move |pending| pending.scope == gone);
        }
        match named.filter(|_| ok) {
            Some(named) => {
                for (scope, id) in &named {
                    queue.remember_forgotten(scope, *id);
                }
                queue.let_go_where(true, |pending| {
                    named
                        .iter()
                        .any(|(scope, id)| *scope == pending.scope && *id == pending.exchange)
                });
            }
            None if !(ok && everything) => {
                // What went is not named: the person's and the room's wait
                // for the next start, when the memory no longer has it.
                queue.let_go_where(false, |pending| {
                    pending.scope == scope || pending.scope == ROOM_SCOPE
                });
            }
            None => {
                queue.let_go_where(false, |pending| pending.scope == ROOM_SCOPE);
            }
        }
        queue.paused_until = None;
        queue.paused_scope.clear();
        self.shared.1.notify_all();
    }

    /// Ids queued for deletion by their full id (`turn_id`): never posted.
    pub fn deleting(&self, ids: &[String]) {
        let mut queue = locked(&self.shared);
        let mut named = Vec::new();
        for id in ids {
            let mut parts = id.splitn(3, ':');
            let (Some(store), Some(scope), Some(exchange)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let Ok(exchange) = exchange.parse::<u64>() else {
                continue;
            };
            if queue.store.as_deref().is_some_and(|ours| ours != store) {
                continue;
            }
            queue.remember_forgotten(scope, exchange);
            named.push((scope.to_owned(), exchange));
        }
        queue.let_go_where(true, |pending| {
            named
                .iter()
                .any(|(scope, id)| *scope == pending.scope && *id == pending.exchange)
        });
        self.shared.1.notify_all();
    }

    /// Turns not sent so far (a full queue, a hold, a forget, a batch past
    /// its tries).
    pub fn let_go(&self) -> u64 {
        locked(&self.shared).let_go
    }
}

/// The voice's one sender, once started.
static SENDER: OnceLock<Sender> = OnceLock::new();

#[cfg(test)]
thread_local! {
    /// A test's own sender for what runs on its thread (the memory's
    /// writer called directly), in place of the process's.
    static TEST_SENDER: std::cell::RefCell<Option<Sender>> = const { std::cell::RefCell::new(None) };
}

/// Tests: the sender a fake memory looks at when it answers a forget, and
/// whether it was holding its sends then.
#[cfg(test)]
pub(crate) static FORGET_PROBE: Mutex<Option<Sender>> = Mutex::new(None);
#[cfg(test)]
pub(crate) static HELD_AT_FORGET: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Tests: a fake memory answering a forget notes whether sends are held.
#[cfg(test)]
pub(crate) fn probe_forget() {
    let probe = FORGET_PROBE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(sender) = probe {
        let holding = locked(&sender.shared).paused_until.is_some();
        HELD_AT_FORGET.store(holding, std::sync::atomic::Ordering::SeqCst);
    }
}

fn with_sender(act: impl FnOnce(&Sender)) {
    #[cfg(test)]
    {
        let test = TEST_SENDER.with(|sender| sender.borrow().clone());
        if let Some(sender) = test {
            act(&sender);
            return;
        }
    }
    if let Some(sender) = SENDER.get() {
        act(sender);
    }
}

/// [`Sender::offer`] on the voice's sender, if started.
pub fn offered(
    person: Option<&str>,
    exchange: u64,
    at_unix_ms: u64,
    words: (&str, &str),
    private: bool,
) {
    with_sender(|sender| sender.offer(person, exchange, at_unix_ms, words, private));
}

/// [`Sender::moved`] on the voice's sender, if started.
pub fn moved(to: &str, pairs: &[(u64, u64)]) {
    with_sender(|sender| sender.moved(to, pairs));
}

/// [`Sender::settled`] on the voice's sender, if started.
pub fn settled(exchanges: &[u64]) {
    with_sender(|sender| sender.settled(exchanges));
}

/// [`Sender::forgetting`] on the voice's sender, if started.
pub fn forgetting(person: Option<&str>) {
    with_sender(|sender| sender.forgetting(person));
}

/// [`Sender::forget_answered`] on the voice's sender, if started.
pub fn forget_answered(person: Option<&str>, everything: bool, answer: Option<&Value>) {
    with_sender(|sender| sender.forget_answered(person, everything, answer));
}

/// [`Sender::deleting`] on the voice's sender, if started: for the forget
/// half's queue (`LAMP_HISTORY_FORGET`).
pub fn deleting(ids: &[String]) {
    with_sender(|sender| sender.deleting(ids));
}

/// Where saves go, from the second route's `llm_base_url`: the lamp's own
/// cloud API (`https://campaign-api.autonomous.ai/api/v1/ai`, with or
/// without its `/v1`), the relay every turn already goes through and the
/// host a forget's deletes go to (`LAMP_HISTORY_FORGET`). Any other base
/// (a direct provider, plain HTTP, a test address) is `None`: nothing is
/// sent (the first builder's P2 on #7935).
pub fn url(llm_base_url: &str) -> Option<String> {
    let base = llm_base_url.trim().trim_end_matches('/');
    let host = base.strip_prefix("https://")?.split('/').next()?;
    if host != crate::second_config::COMPANY_RELAY_HOST {
        return None;
    }
    let base = base.strip_suffix("/v1").unwrap_or(base);
    Some(format!("{base}{PATH}"))
}

/// Starts the voice's sender with the switch `value`, the second route's
/// `llm_base_url` and its key (the lamp's own, the file lamp-cloud loads as
/// its `key`), the memory's socket and the marks file: nothing unless
/// `value` is exactly `on`; and nothing, said in the journal, when the base
/// is not the lamp's cloud API ([`url`]) or the key is empty. A key that
/// comes later (a sign-in after the worker started) starts it at the
/// worker's next start.
pub fn start_from(
    value: Option<&str>,
    llm_base_url: &str,
    key: &str,
    memory: &Path,
    marks: &Path,
) -> Option<Sender> {
    let socket = memory.to_owned();
    let ask: Ask = Box::new(move |line: &Value| {
        crate::tools::exchange_up_to(&socket, line, MEMORY_WITHIN, MEMORY_ANSWER_MOST)
    });
    start_at(value, url(llm_base_url), key, ask, Some(marks.to_owned()))
}

/// [`start_from`] with the save's URL already worked out (`None`: not the
/// lamp's cloud API) and the memory's asker.
fn start_at(
    value: Option<&str>,
    url: Option<String>,
    key: &str,
    ask: Ask,
    marks: Option<PathBuf>,
) -> Option<Sender> {
    if !on_from(value) {
        return None;
    }
    let refused = |why: &str| {
        eprintln!("lamp-voice history: no turn is sent to the account: {why} ({ENVIRONMENT})");
        None
    };
    let Some(url) = url else {
        return refused("the route is not the lamp's own cloud API");
    };
    let key = key.trim();
    if key.is_empty() || key.chars().any(char::is_control) {
        return refused(
            "the lamp's key is empty or not one line; a sign-in starts it at the voice's next start",
        );
    }
    let Some(post) = http_post(url, key.to_owned()) else {
        return refused("no HTTPS client");
    };
    let sender = Sender::new();
    if sender.spawn(post, ask, marks, Waits::default()).is_err() {
        return refused("no thread to send from");
    }
    eprintln!(
        "lamp-voice history: each turn the memory keeps is sent to the account's history ({ENVIRONMENT})"
    );
    Some(sender)
}

/// [`start_from`] with this process's switch and [`MARKS_FILE`], once.
pub fn start_from_environment(llm_base_url: &str, key: &str, memory: &Path) {
    if SENDER.get().is_some() {
        return;
    }
    if let Some(sender) = start_from(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        llm_base_url,
        key,
        memory,
        Path::new(MARKS_FILE),
    ) {
        let _ = SENDER.set(sender);
    }
}

/// The save call over HTTPS (plain only in this crate's tests), on a
/// runtime of its own on the sending thread, each call within [`WITHIN`].
fn http_post(url: String, key: String) -> Option<Post> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let client = reqwest::Client::builder()
        .tls_backend_preconfigured(crate::second_provider::net::tls().ok()?)
        .https_only(!cfg!(test))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(WITHIN)
        .build()
        .ok()?;
    Some(Box::new(move |body: &Value| {
        runtime.block_on(async {
            let sent = client
                .post(&url)
                .header("x-api-key", &key)
                .header("content-type", "application/json")
                .body(body.to_string())
                .send()
                .await;
            let Ok(response) = sent else {
                return Answer::Again;
            };
            let status = response.status().as_u16();
            match response.bytes().await {
                Ok(bytes) if bytes.len() <= ANSWER_MOST => answer_of(status, &bytes),
                _ => Answer::Again,
            }
        })
    }))
}

#[cfg(test)]
#[path = "history_send/tests.rs"]
mod tests;
