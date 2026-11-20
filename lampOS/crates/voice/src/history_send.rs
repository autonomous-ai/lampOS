//! Each conversation turn the memory keeps, saved to the account's history
//! store too (#7771, `LAMP_HISTORY_SEND=on`): the product owner's
//! decision of 18 Nov that a person's conversations are kept on the server,
//! and Backend3's live store of 19 Nov, `POST /api/v1/ai/history/turns` with
//! the lamp's own key (`x-api-key`). Sent only to the lamp's own cloud API,
//! the relay host the second route's `llm_base_url` names ([`url`]); with a
//! direct provider's base nothing is sent. The second route only: a Live
//! route turn is never saved. On in the product unit only together with
//! the forget half (`LAMP_HISTORY_FORGET`), never alone: the first
//! builder's rulings (#7935, and 17:09 on #7957).
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
//! **The id** is `lamp_msgs::history_turn_id`, `<epoch>.<scope>:<exchange
//! id>`, the one maker of it, which the memory's forget answers use too: the
//! Days store's epoch (made with the store, made anew when it is wiped whole
//! or reset), the memory scope and the exchange's id in it. A wiped or reset
//! lamp's new turns are neither taken as already saved nor refused by an old
//! tombstone. The epoch is read at start and again after every forget; when
//! it changed, the turns not yet sent wait for the next start, and the marks
//! start again under the new one. The store takes a resend of an id as a
//! no-op, so a retry is always safe.
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
//! the server keeps a tombstone). When the answer does not name its ids
//! (none comes, or a forget about a thing timed out part way), the
//! forgetting person's and the room's turns handed over so far wait for the
//! next start, when the memory no longer holds what it forgot: a batch
//! already taken off the queue to send included, checked before its every
//! try (Friday's P2 on #7957). Other people's
//! unsent turns are never let go. [`deleting`] takes the ids a forget
//! queues for deletion, by their full id.
//!
//! **Nothing lost at a restart, and no words on disk:** only ids are kept,
//! in [`MARKS_FILE`] (0600): a mark per scope, the last id the server took
//! with none before it unsent, and the last id handed over in each scope
//! (written before the post that carries it). At start the sending thread
//! asks the memory for its epoch and each scope's last id
//! (`history_issued`), sends what the memory kept after the marks up to
//! what was handed over (`exchanges_after`), and moves every mark to the
//! memory's newest id: a turn kept while no sender ran (the switch off, or
//! the Live route) is never sent, on that start or any after. The memory
//! stays the one copy of the words. An epoch unlike the marks' (a first
//! start, or a reset) starts the marks at what the memory holds now.
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
//! Nothing is sent while the privacy switch is closed
//! ([`crate::history_privacy`]): a turn said just before it closed goes once
//! it opens. While a forget is in the forget worker's hands or its delete
//! has not reached the account, no turn it could reach is sent
//! ([`crate::history_forget::holds`]); a turn a forget covered that was
//! already on the wire is deleted after its POST answers.
use lamp_msgs::{MEMORY_HISTORY_EPOCH_OP, history_turn_id, is_history_epoch};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

/// The room's memory scope; its person on the server is `""`.
pub const ROOM_SCOPE: &str = "everyone";
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
            "id": history_turn_id(store, &self.scope, self.exchange),
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

/// Whether a turn of a memory scope may be sent now ([`Sender::guarded`]).
pub type Gate = Arc<dyn Fn(&str) -> bool + Send + Sync>;
/// Turn ids posted while a forget came that covers them, to delete.
pub type Posted = Arc<dyn Fn(&[String]) + Send + Sync>;

/// How often a batch the gate holds is looked at again.
pub const GATE_AGAIN: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Queue {
    /// None: every turn may go (tests).
    gate: Option<Gate>,
    /// None: nothing is deleted after a post (tests).
    posted_forgotten: Option<Posted>,
    ready: VecDeque<Pending>,
    held: VecDeque<Pending>,
    /// The memory store's epoch, once the memory told it: nothing is
    /// posted before.
    store: Option<String>,
    /// A forget was answered: the epoch is read again before the next post.
    epoch_stale: bool,
    /// Sends wait for a forget's answer until then, and whose forget it is.
    paused_until: Option<Instant>,
    paused_scope: String,
    /// Ids a forget removed, by scope and id, newest last, and each scope
    /// a forget everything emptied, through the last id offered then.
    forgotten: HashSet<(String, u64)>,
    forgotten_order: VecDeque<(String, u64)>,
    emptied: BTreeMap<String, u64>,
    /// Each scope whose turns, through this id, wait for the next start: a
    /// forget answered without naming what it removed, or not answered in
    /// time. The batch already taken off the queue to send is checked
    /// against it before every try, the first included, as against
    /// [`Self::forgotten`] (Friday's P2 on #7957: such a batch was put back
    /// and posted).
    waits_through: BTreeMap<String, u64>,
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
    /// [`Self::offered`] as last written.
    offered_written: BTreeMap<String, u64>,
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

    /// Waits for the next start: a forget that named no ids covered it.
    fn waits(&self, pending: &Pending) -> bool {
        self.waits_through
            .get(&pending.scope)
            .is_some_and(|through| pending.exchange <= *through)
    }

    /// The highest id of `scope` handed over this run: offered, or queued,
    /// held or being sent (a room line moved to a person goes under its new
    /// id, which `offered` never saw).
    fn handed_through(&self, scope: &str) -> u64 {
        let offered = self.offered.get(scope).copied().unwrap_or(0);
        let outstanding = self
            .outstanding
            .get(scope)
            .and_then(|ids| ids.last().copied())
            .unwrap_or(0);
        offered.max(outstanding)
    }

    /// A forget that names no ids, of `scopes`: every turn of theirs handed
    /// over so far waits for the next start, when the memory no longer
    /// holds what it forgot. Those queued or held now, and those in a batch
    /// already taken to send ([`Self::waits`]), so none of them is posted.
    fn wait_unnamed(&mut self, scopes: &[&str]) {
        for scope in scopes {
            let through = self.handed_through(scope);
            let waits = self.waits_through.entry((*scope).to_owned()).or_insert(0);
            *waits = (*waits).max(through);
        }
        self.let_go_where(false, |pending| scopes.contains(&pending.scope.as_str()));
    }

    /// A forget everything of `scope` answered: every turn of theirs
    /// handed over so far is forgotten, and let go.
    fn empty(&mut self, scope: &str) {
        let through = self.handed_through(scope);
        let emptied = self.emptied.entry(scope.to_owned()).or_insert(0);
        *emptied = (*emptied).max(through);
        let gone = scope.to_owned();
        self.let_go_where(true, move |pending| pending.scope == gone);
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
            queue.wait_unnamed(&[&scope, ROOM_SCOPE]);
            eprintln!(
                "lamp-voice history: a forget gave no answer in time; the turns waiting go at the next start ({ENVIRONMENT})"
            );
            false
        }
        None => false,
    }
}

/// The next turns to send, oldest first, at most [`BATCH_MOST`], and the
/// epoch; waits until there are some, the epoch is known and no forget
/// holds. `None`: a forget was answered, and the epoch is read again first.
fn next_batch(shared: &Shared) -> Next {
    let mut queue = locked(shared);
    loop {
        let now = Instant::now();
        queue.age_out(now);
        let holding = held_by_forget(&mut queue, now);
        if !holding && queue.epoch_stale {
            return Next::Epoch;
        }
        if queue.store.is_some() && queue.offered != queue.offered_written {
            return Next::Write;
        }
        if !holding
            && !queue.ready.is_empty()
            && let Some(store) = queue.store.clone()
        {
            let take = queue.ready.len().min(BATCH_MOST);
            let batch = queue.ready.drain(..take).collect();
            return Next::Batch(batch, store);
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

/// What the sending thread does next.
enum Next {
    /// These turns, under this epoch.
    Batch(Vec<Pending>, String),
    /// A forget was answered: the epoch is read again first.
    Epoch,
    /// What was handed over is written down first.
    Write,
}

/// How a batch ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ended {
    Answered(Answer),
    /// Not saved in its tries, or held past a forget's bound: waits for the
    /// next start.
    Waits,
    /// Everything in it was forgotten, or waits for the next start since a
    /// forget naming no ids covered it, before it went.
    Forgotten,
    /// Put back to go first, once the epoch is read again after a forget.
    Requeued,
}

/// One batch, tried up to [`TRIES`] times. Before every try, the first
/// included, it waits for a forget's answer, drops each turn forgotten
/// (the lamp's guard) and lets each turn a forget naming no ids covered
/// wait for the next start (`waits_through`), before a batch put back
/// after a forget is queued again; the rest go with the same ids. A POST
/// already under way is not stopped: its next try is checked, and a turn
/// the store took is the server's tombstone's to refuse again.
fn send_batch(
    shared: &Shared,
    post: &mut Post,
    batch: &mut Vec<Pending>,
    store: &str,
    waits: Waits,
) -> Ended {
    let mut wait = waits.first;
    for tries in 1..=TRIES {
        // The privacy switch closed, or a forget of a scope in the batch
        // still in the forget's hands or waiting to reach the account: the
        // batch waits, the queue's lock free (the first builder's privacy
        // read on #8125, items 1 and 3).
        let gate = locked(shared).gate.clone();
        if let Some(gate) = gate {
            while batch.iter().any(|pending| !gate(&pending.scope)) {
                std::thread::sleep(GATE_AGAIN);
            }
        }
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
                        // room's wait for the next start, this batch's too
                        // (below).
                        held_by_forget(&mut queue, now);
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
            let (later, rest): (Vec<_>, Vec<_>) =
                rest.into_iter().partition(|pending| queue.waits(pending));
            for pending in &later {
                queue.defer(pending);
            }
            *batch = rest;
            if queue.epoch_stale && !batch.is_empty() {
                for pending in batch.drain(..).rev() {
                    queue.ready.push_front(pending);
                }
                return Ended::Requeued;
            }
        }
        if batch.is_empty() {
            return Ended::Forgotten;
        }
        let body = json!({
            "turns": batch.iter().map(|pending| pending.wire(store)).collect::<Vec<_>>()
        });
        let answer = post(&body);
        if matches!(answer, Answer::Saved | Answer::Again) {
            posted_while_forgotten(shared, batch, store);
        }
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

/// The Reviewer-lead's must-fix on #8091: a POST already under way when a
/// forget came that covers some of its turns (by id, a forget everything,
/// or one naming no ids) may have landed. Once it answered, those turns'
/// ids are handed to the forget's delete ([`Queue::posted_forgotten`]), so
/// a turn forgotten during its POST never stays on the account.
fn posted_while_forgotten(shared: &Shared, batch: &[Pending], store: &str) {
    let (ids, posted) = {
        let queue = locked(shared);
        let ids: Vec<String> = batch
            .iter()
            .filter(|pending| {
                queue.is_forgotten(&pending.scope, pending.exchange) || queue.waits(pending)
            })
            .map(|pending| history_turn_id(store, &pending.scope, pending.exchange))
            .collect();
        (ids, queue.posted_forgotten.clone())
    };
    if let Some(posted) = posted
        && !ids.is_empty()
    {
        eprintln!(
            "lamp-voice history: {} turns a forget covered were posted before it came; their delete is queued",
            ids.len()
        );
        posted(&ids);
    }
}

/// An id per memory scope.
type ByScope = BTreeMap<String, u64>;

/// The marks file's contents: the epoch, each scope's mark, and the last
/// id handed over in each scope.
fn read_marks(path: &Path) -> Option<(String, ByScope, ByScope)> {
    let bytes = crate::config::read_private(path, 65_536).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let store = value.get("epoch")?.as_str()?.to_owned();
    let ids = |key: &str| -> BTreeMap<String, u64> {
        value
            .get(key)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(scope, id)| Some((scope.clone(), id.as_u64()?)))
            .collect()
    };
    Some((store, ids("marks"), ids("offered")))
}

/// The marks written whole (a new file renamed into place), 0600.
fn write_marks(
    path: &Path,
    store: &str,
    marks: &BTreeMap<String, u64>,
    offered: &BTreeMap<String, u64>,
) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let text = json!({"epoch": store, "marks": marks, "offered": offered}).to_string();
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

/// Each scope's mark that rose written down, with what was handed over,
/// off the turn's path; `force`: written whatever changed.
fn keep_marks(shared: &Shared, path: Option<&Path>, scopes: &BTreeSet<String>, force: bool) {
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
    let offered = queue.offered.clone();
    let handed = queue.offered_written != offered;
    queue.offered_written = offered.clone();
    drop(queue);
    if (rose || handed || force)
        && let Some(path) = path
        && write_marks(path, &store, &marks, &offered).is_err()
    {
        eprintln!("lamp-voice history: the marks could not be written ({ENVIRONMENT})");
    }
}

/// What the memory tells at start: its epoch and each scope's last id.
fn ask_store(ask: &mut Ask) -> Option<(String, BTreeMap<String, u64>)> {
    let answer = ask(&json!({"op":"history_issued"}))?;
    let store = answer
        .get("epoch")?
        .as_str()
        .filter(|epoch| is_history_epoch(epoch))?;
    let issued = answer
        .get("issued")?
        .as_object()?
        .iter()
        .filter(|(scope, _)| reachable(scope))
        .filter_map(|(scope, last)| Some((scope.clone(), last.as_u64()?)))
        .collect();
    Some((store.to_owned(), issued))
}

/// The start: the epoch from the memory (asked until it answers), the
/// marks read, and what the memory kept after them sent; a new epoch (or
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
    let (from, carried) = match kept {
        Some((kept_store, kept, offered)) if kept_store == store => (kept, offered),
        // A first start, or a new epoch: nothing from before is sent.
        _ => (issued.clone(), BTreeMap::new()),
    };
    {
        let mut queue = locked(shared);
        queue.store = Some(store.clone());
        queue.marks = from.clone();
        for (scope, high) in &carried {
            let offered = queue.offered.entry(scope.clone()).or_insert(0);
            *offered = (*offered).max(*high);
        }
        shared.1.notify_all();
    }
    let mut sent = 0usize;
    for (scope, last) in &issued {
        let mut after = from.get(scope).copied().unwrap_or(0);
        // Only what was handed over before and never taken: a turn kept
        // while no sender ran (the switch off, the Live route) is passed.
        let high = carried.get(scope).copied().unwrap_or(0).min(*last);
        while after < high {
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
                        exchange: exchange["id"].as_u64().filter(|id| *id <= high)?,
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
            keep_marks(shared, marks, &BTreeSet::from([scope.clone()]), false);
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
        // Past what was kept with no sender: as taken, never sent.
        let mut queue = locked(shared);
        let acked = queue.acked.entry(scope.clone()).or_insert(0);
        *acked = (*acked).max(*last);
        drop(queue);
        keep_marks(shared, marks, &BTreeSet::from([scope.clone()]), false);
    }
    keep_marks(shared, marks, &BTreeSet::new(), true);
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
            Ended::Requeued => {}
        }
    }
}

/// The epoch read again after a forget (asked until the memory answers).
/// Unchanged, sends go on. Changed (a wipe whole, a reset): the turns not
/// yet sent wait for the next start, what was kept by id under the old
/// epoch goes, and the marks start again under the new one, so the next
/// start sends what the memory kept since.
fn reread_epoch(shared: &Shared, ask: &mut Ask, marks: Option<&Path>, waits: Waits) {
    let epoch = loop {
        let told = ask(&json!({"op": MEMORY_HISTORY_EPOCH_OP})).and_then(|answer| {
            answer["epoch"]
                .as_str()
                .filter(|epoch| is_history_epoch(epoch))
                .map(str::to_owned)
        });
        if let Some(epoch) = told {
            break epoch;
        }
        let queue = locked(shared);
        drop(wait_on(shared, queue, waits.first));
    };
    let mut queue = locked(shared);
    queue.epoch_stale = false;
    if queue.store.as_deref() == Some(epoch.as_str()) {
        return;
    }
    let waiting = (queue.ready.len() + queue.held.len()) as u64;
    queue.ready.clear();
    queue.held.clear();
    queue.let_go += waiting;
    queue.forgotten.clear();
    queue.forgotten_order.clear();
    queue.emptied.clear();
    queue.waits_through.clear();
    queue.offered.clear();
    queue.offered_written.clear();
    queue.outstanding.clear();
    queue.deferred.clear();
    queue.acked.clear();
    queue.marks.clear();
    queue.store = Some(epoch.clone());
    drop(queue);
    eprintln!(
        "lamp-voice history: the memory's epoch changed; {waiting} turns not sent go at the next start ({ENVIRONMENT})"
    );
    if let Some(path) = marks
        && write_marks(path, &epoch, &BTreeMap::new(), &BTreeMap::new()).is_err()
    {
        eprintln!("lamp-voice history: the marks could not be written ({ENVIRONMENT})");
    }
}

/// The sending thread: the start's catch-up, then each batch in order; a
/// change of whether the store saves said once, and each let-go counted,
/// never a word or an id.
fn send_all(shared: &Shared, mut post: Post, mut ask: Ask, marks: Option<PathBuf>, waits: Waits) {
    catch_up(shared, &mut post, &mut ask, marks.as_deref(), waits);
    let mut saving = None;
    loop {
        let (mut batch, store) = match next_batch(shared) {
            Next::Batch(batch, store) => (batch, store),
            Next::Epoch => {
                reread_epoch(shared, &mut ask, marks.as_deref(), waits);
                continue;
            }
            Next::Write => {
                keep_marks(shared, marks.as_deref(), &BTreeSet::new(), false);
                continue;
            }
        };
        // What it hands over is written down before it goes.
        keep_marks(shared, marks.as_deref(), &BTreeSet::new(), false);
        let ended = send_batch(shared, &mut post, &mut batch, &store, waits);
        ended_batch(shared, &batch, ended);
        let scopes = batch.iter().map(|pending| pending.scope.clone()).collect();
        keep_marks(shared, marks.as_deref(), &scopes, false);
        let now_saving = match ended {
            Ended::Answered(Answer::Saved) => Some(true),
            Ended::Answered(_) | Ended::Waits => Some(false),
            Ended::Forgotten | Ended::Requeued => None,
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

    /// Before [`Self::spawn`]: each batch waits while `gate` says no for a
    /// scope in it, and `posted` is given the ids of turns a forget covered
    /// that were posted before it came.
    pub fn guarded(&self, gate: Gate, posted: Posted) {
        let mut queue = locked(&self.shared);
        queue.gate = Some(gate);
        queue.posted_forgotten = Some(posted);
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
    /// last id handed over in it. With no ids named (a memory answer that
    /// names none, a lost answer, or `None`: what it removed is not known),
    /// the person's and the room's turns handed over so far wait for the
    /// next start, those in a batch already taken to send included
    /// ([`Queue::wait_unnamed`]). Sends go on after.
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
                                .filter_map(|id| match id {
                                    Value::Number(id) => Some((scope.clone(), id.as_u64()?)),
                                    Value::String(turn) => {
                                        let (_, scope, id) = parse_turn(turn)?;
                                        Some((scope.to_owned(), id))
                                    }
                                    _ => None,
                                })
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
            queue.empty(&scope);
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
                queue.wait_unnamed(&[&scope, ROOM_SCOPE]);
            }
            None => {
                // Theirs is emptied above (`emptied`); the room's lines a
                // forget everything took are not named.
                queue.wait_unnamed(&[ROOM_SCOPE]);
            }
        }
        queue.paused_until = None;
        queue.paused_scope.clear();
        queue.epoch_stale = true;
        self.shared.1.notify_all();
    }

    /// Ids queued for deletion by their full id (`history_turn_id`): never
    /// posted.
    pub fn deleting(&self, ids: &[String]) {
        let mut queue = locked(&self.shared);
        let mut named = Vec::new();
        for id in ids {
            let Some((store, scope, exchange)) = parse_turn(id) else {
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

    /// A memory answer the voice read (`history_forget::answered`, every
    /// forget op's way, and `history_forget::recover`'s pages): the turn
    /// ids its `exchanges` names are never posted, and each known person
    /// its `all` names (a whole history the memory kept for the account to
    /// delete: a removal in the app, `Days::forget_person`, or a forget
    /// everything) is emptied as a forget everything of theirs is, before
    /// that delete can lift the save's hold (Heron's P1 on #8125: such a
    /// person's turns waiting in the save were posted after it).
    pub fn seen(&self, answer: &Value) {
        if answer["ok"] != true {
            return;
        }
        let persons: Vec<&str> = answer["all"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|person| lamp_msgs::known::is_person_id(person))
            .collect();
        if !persons.is_empty() {
            let mut queue = locked(&self.shared);
            for person in &persons {
                queue.empty(person);
            }
            drop(queue);
            self.shared.1.notify_all();
        }
        let Some(by_scope) = answer.get("exchanges").and_then(Value::as_object) else {
            return;
        };
        let ids: Vec<String> = by_scope
            .values()
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(Value::as_str)
            .filter(|turn| parse_turn(turn).is_some())
            .map(str::to_owned)
            .collect();
        if !ids.is_empty() {
            self.deleting(&ids);
        }
    }

    /// Turns not sent so far (a full queue, a hold, a forget, a batch past
    /// its tries).
    pub fn let_go(&self) -> u64 {
        locked(&self.shared).let_go
    }
}

/// A turn id's epoch, scope and exchange id (`history_turn_id`'s form).
fn parse_turn(turn: &str) -> Option<(&str, &str, u64)> {
    let (epoch, rest) = turn.split_once('.')?;
    let (scope, id) = rest.rsplit_once(':')?;
    (is_history_epoch(epoch) && !scope.is_empty()).then_some(())?;
    Some((epoch, scope, id.parse().ok()?))
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

/// [`Sender::seen`] on the voice's sender, if started.
pub fn forget_seen(answer: &Value) {
    with_sender(|sender| sender.seen(answer));
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
    // Nothing is sent while the privacy switch is closed, nor a turn a
    // waiting forget could reach (the first builder's privacy read on
    // #8125); a turn posted as a forget covering it came is deleted after
    // (the Reviewer-lead's must-fix on #8091).
    let gate: Gate = Arc::new(|scope: &str| {
        crate::history_privacy::open() && !crate::history_forget::holds(scope)
    });
    let posted: Posted = Arc::new(crate::history_forget::delete_posted);
    start_guarded(
        value,
        url(llm_base_url),
        key,
        ask,
        Some(marks.to_owned()),
        Some((gate, posted)),
    )
}

/// [`start_from`] with the save's URL already worked out (`None`: not the
/// lamp's cloud API) and the memory's asker.
#[cfg(test)]
fn start_at(
    value: Option<&str>,
    url: Option<String>,
    key: &str,
    ask: Ask,
    marks: Option<PathBuf>,
) -> Option<Sender> {
    start_guarded(value, url, key, ask, marks, None)
}

/// [`start_at`], each batch held by `guard` ([`Sender::guarded`]) when
/// given: the product's, the privacy switch and the forget's hold.
fn start_guarded(
    value: Option<&str>,
    url: Option<String>,
    key: &str,
    ask: Ask,
    marks: Option<PathBuf>,
    guard: Option<(Gate, Posted)>,
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
    if let Some((gate, posted)) = guard {
        sender.guarded(gate, posted);
    }
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
