//! Each conversation turn the memory keeps, saved to the account's history
//! store too (#7771, `LAMP_HISTORY_SEND=on`, off): the product owner's
//! decision of 18 Nov that a person's conversations are kept on the server,
//! and Backend3's live store of 19 Nov, `POST /api/v1/ai/history/turns` with
//! the lamp's own key (`x-api-key`). Sent only to the lamp's own cloud API,
//! the relay host the second route's `llm_base_url` names ([`url`]); with a
//! direct provider's base nothing is sent. Off in every unit: by the first
//! builder's ruling on #7935 the save never goes on in the product before
//! the forget half (`LAMP_HISTORY_FORGET`) is on, with its row.
//!
//! **What is sent:** only an exchange the Days memory wrote (`append_linked`
//! answered with its id), as `{id, at, person, heard, said}`: the person's
//! line as heard and the lamp's reply, when it was said, and whose scope it
//! is in (a known person's memory id, or empty for the room's). No audio, no
//! picture, no kept thing, no observation, no rule. A turn the memory keeps
//! out (a keep or forget turn, a line carried or held by a face's gap, one
//! made before a forget, one whose source was forgotten) is never written
//! and so never sent: a forget could not reach a copy the lamp does not
//! know of.
//!
//! **The id** is the memory's own exchange id in its scope, `<scope>:<id>`
//! (Friday's rule on #7771, 19 Nov 14:35): `p-0123456789abcdef:42`, or
//! `everyone:42` for the room's. A forget that removes exchanges names the
//! same ids with [`turn_id`] (`LAMP_HISTORY_FORGET`, Friday's), with no
//! ledger to keep in step. The memory never gives an id twice in a scope
//! (`next_id` is never lowered), and the store treats a resend of an id as
//! a no-op, so a retry is always safe.
//!
//! **The room's lines wait for their conversation's end:** with
//! `LAMP_MEMORY_FOLLOWS_FACE` (or `LAMP_MEMORY_EARLY_WORDS`) a room line may
//! still move to the caller's own scope, with a new id, until then
//! (`adopt`). A moved line is sent under its new id and person
//! ([`Sender::moved`]); the rest go when the conversation ends
//! ([`Sender::settled`]). A held line not settled within [`HELD_FOR`] is let
//! go unsent, so no room copy can sit under an id its person's forget does
//! not reach.
//!
//! **A forget said on the lamp** ([`Sender::forgot`]) lets go of every turn
//! not yet saved, and a batch waiting to be tried again is not tried again:
//! nothing the lamp forgot reaches the server after the forget.
//!
//! **Never on the turn's path:** the memory's writer thread hands each turn
//! over under a short lock with no I/O, into a bounded queue (at most
//! [`READY_MOST`] waiting; past it the oldest is let go and counted). A
//! thread of its own sends them in order, at most [`BATCH_MOST`] a call,
//! each text cut to [`TEXT_MOST`] bytes, and tries a failed call again
//! with the same ids, up to [`TRIES`] times, waiting longer each time. Kept
//! in RAM only: what is unsent at a restart is not sent. The journal has
//! counts only, never a word, an id or a key.
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

/// `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_HISTORY_SEND";
/// The store's save call, below the lamp's cloud API base (`…/api/v1/ai`).
pub const PATH: &str = "/history/turns";
/// The room's scope in a turn's id.
pub const ROOM: &str = "everyone";
/// The store's caps (Backend3, 19 Nov): 20 turns a call, 8 KB a text.
pub const BATCH_MOST: usize = 20;
pub const TEXT_MOST: usize = 8 * 1024;
/// Turns waiting to be sent, at most; the oldest is let go past it.
pub const READY_MOST: usize = 200;
/// The room's turns held for their conversation's end, at most.
pub const HELD_MOST: usize = 64;
/// The longest a room turn is held; past it, it is let go unsent.
pub const HELD_FOR: Duration = Duration::from_secs(30 * 60);
/// Tries of one batch, the first included.
pub const TRIES: u32 = 6;
/// The longest one call takes.
pub const WITHIN: Duration = Duration::from_secs(10);
/// The longest answer read.
const ANSWER_MOST: usize = 65_536;

/// Whether the switch is on.
pub fn on() -> bool {
    on_from(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Exactly `on`.
pub fn on_from(value: Option<&str>) -> bool {
    value == Some("on")
}

/// A saved turn's id: the memory's exchange id in its scope, `<scope>:<id>`,
/// the scope a known person's memory id or [`ROOM`]. At most 53 characters
/// (a scope is at most 32), within the store's 64.
pub fn turn_id(person: Option<&str>, exchange: u64) -> String {
    format!("{}:{exchange}", person.unwrap_or(ROOM))
}

/// One turn as the store takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub id: String,
    pub at_unix_ms: u64,
    /// A known person's memory id, or empty for the room's.
    pub person: String,
    pub heard: String,
    pub said: String,
}

impl Turn {
    fn wire(&self) -> Value {
        json!({
            "id": self.id,
            "at": utc(self.at_unix_ms),
            "person": self.person,
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

struct Pending {
    turn: Turn,
    /// The memory's id in its scope, for a held room turn's move or end.
    exchange: u64,
    since: Instant,
}

#[derive(Default)]
struct Queue {
    ready: VecDeque<Pending>,
    held: VecDeque<Pending>,
    /// Forgets said: a batch taken before one is not tried again.
    forgets: u64,
    let_go: u64,
}

impl Queue {
    /// Into the queue to send, the oldest let go past [`READY_MOST`].
    fn ready_push(&mut self, pending: Pending) {
        if self.ready.len() >= READY_MOST {
            self.ready.pop_front();
            self.let_go += 1;
        }
        self.ready.push_back(pending);
    }

    /// Room turns held past [`HELD_FOR`] let go unsent.
    fn age_out(&mut self, now: Instant) {
        let before = self.held.len();
        self.held
            .retain(|pending| now.saturating_duration_since(pending.since) < HELD_FOR);
        self.let_go += (before - self.held.len()) as u64;
    }

    /// The held room turns `take` picks moved to the queue to send, each
    /// as `take` changes it; the others stay held.
    fn release(&mut self, mut take: impl FnMut(&mut Pending) -> bool) {
        for mut pending in std::mem::take(&mut self.held) {
            if take(&mut pending) {
                self.ready_push(pending);
            } else {
                self.held.push_back(pending);
            }
        }
    }
}

type Shared = (Mutex<Queue>, Condvar);

fn locked(shared: &Shared) -> MutexGuard<'_, Queue> {
    shared
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The next turns to send, oldest first, at most [`BATCH_MOST`], with the
/// forgets counted when they were taken; waits until there are some.
fn next_batch(shared: &Shared) -> (Vec<Pending>, u64) {
    let mut queue = locked(shared);
    loop {
        queue.age_out(Instant::now());
        if !queue.ready.is_empty() {
            let take = queue.ready.len().min(BATCH_MOST);
            let batch = queue.ready.drain(..take).collect();
            return (batch, queue.forgets);
        }
        queue = shared
            .1
            .wait_timeout(queue, Duration::from_secs(60))
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0;
    }
}

/// One batch, tried up to [`TRIES`] times with the same body: the last
/// answer and the tries made; `None` when a forget was said while it
/// waited to be tried again (it is not).
fn send_batch(
    shared: &Shared,
    post: &mut Post,
    body: &Value,
    forgets: u64,
    waits: Waits,
) -> (Option<Answer>, u32) {
    let mut wait = waits.first;
    for tries in 1..=TRIES {
        let answer = post(body);
        if answer != Answer::Again || tries == TRIES {
            return (Some(answer), tries);
        }
        let until = Instant::now() + wait;
        let mut queue = locked(shared);
        loop {
            if queue.forgets != forgets {
                return (None, tries);
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            queue = shared
                .1
                .wait_timeout(queue, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        wait = (wait * 2).min(waits.longest);
    }
    (Some(Answer::Again), TRIES)
}

/// The sending thread's loop: each batch in order, a change of whether the
/// store saves said once, and each let-go counted, never a word or an id.
fn send_all(shared: &Shared, mut post: Post, waits: Waits) {
    let mut saving = None;
    loop {
        let (batch, forgets) = next_batch(shared);
        let body = json!({
            "turns": batch.iter().map(|pending| pending.turn.wire()).collect::<Vec<_>>()
        });
        let (answer, tries) = send_batch(shared, &mut post, &body, forgets, waits);
        let saved = matches!(answer, Some(Answer::Saved | Answer::Off));
        if answer.is_some() && saving != Some(answer == Some(Answer::Saved)) {
            saving = Some(answer == Some(Answer::Saved));
            eprintln!(
                "lamp-voice history: the account's history store is {} ({ENVIRONMENT})",
                match answer {
                    Some(Answer::Saved) => "saving turns",
                    Some(Answer::Off) => "off on the server: nothing is kept there",
                    _ => "not saving turns",
                }
            );
        }
        if !saved {
            locked(shared).let_go += batch.len() as u64;
            let why = match answer {
                Some(Answer::Refused(status)) => format!("refused, HTTP {status}"),
                None => "a forget was said".to_owned(),
                _ => format!("not saved in {tries} tries"),
            };
            eprintln!(
                "lamp-voice history: {} turns let go unsent: {why}",
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

    /// The thread that sends, with `post` for each call.
    pub fn spawn(&self, post: Post, waits: Waits) -> std::io::Result<()> {
        let shared = Arc::clone(&self.shared);
        std::thread::Builder::new()
            .name("lamp-voice-history".into())
            .spawn(move || send_all(&shared, post, waits))
            .map(|_| ())
    }

    /// The memory wrote exchange `exchange` in `person`'s scope (`None`: the
    /// room's, held until its conversation ends). Never waits on the
    /// network: a short lock, no I/O.
    pub fn offer(
        &self,
        person: Option<&str>,
        exchange: u64,
        at_unix_ms: u64,
        heard: &str,
        said: &str,
    ) {
        let pending = Pending {
            turn: Turn {
                id: turn_id(person, exchange),
                at_unix_ms,
                person: person.unwrap_or_default().to_owned(),
                heard: capped(heard),
                said: capped(said),
            },
            exchange,
            since: Instant::now(),
        };
        let mut queue = locked(&self.shared);
        if person.is_some() {
            queue.ready_push(pending);
            self.shared.1.notify_all();
        } else {
            if queue.held.len() >= HELD_MOST {
                queue.held.pop_front();
                queue.let_go += 1;
            }
            queue.held.push_back(pending);
        }
    }

    /// The room's exchanges `moved` (old id, new id) are `to`'s now
    /// (`adopt`): each held one is sent as theirs, under its new id.
    pub fn moved(&self, to: &str, moved: &[(u64, u64)]) {
        locked(&self.shared).release(|pending| {
            let Some((_, new)) = moved.iter().find(|(old, _)| *old == pending.exchange) else {
                return false;
            };
            pending.exchange = *new;
            pending.turn.id = turn_id(Some(to), *new);
            pending.turn.person = to.to_owned();
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

    /// A forget was said: every turn not yet saved is let go, and a batch
    /// waiting to be tried again is not.
    pub fn forgot(&self) {
        let mut queue = locked(&self.shared);
        queue.forgets += 1;
        let dropped = queue.ready.len() + queue.held.len();
        queue.ready.clear();
        queue.held.clear();
        queue.let_go += dropped as u64;
        self.shared.1.notify_all();
    }

    /// Turns let go unsent so far (a full queue, a hold past [`HELD_FOR`], a
    /// forget, a batch past its tries or refused).
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
pub fn offered(person: Option<&str>, exchange: u64, at_unix_ms: u64, heard: &str, said: &str) {
    with_sender(|sender| sender.offer(person, exchange, at_unix_ms, heard, said));
}

/// [`Sender::moved`] on the voice's sender, if started.
pub fn moved(to: &str, pairs: &[(u64, u64)]) {
    with_sender(|sender| sender.moved(to, pairs));
}

/// [`Sender::settled`] on the voice's sender, if started.
pub fn settled(exchanges: &[u64]) {
    with_sender(|sender| sender.settled(exchanges));
}

/// [`Sender::forgot`] on the voice's sender, if started.
pub fn forgot() {
    with_sender(Sender::forgot);
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
/// its `key`): nothing unless `value` is exactly `on`; and nothing, said in
/// the journal, when the base is not the lamp's cloud API ([`url`]) or the
/// key is empty.
pub fn start_from(value: Option<&str>, llm_base_url: &str, key: &str) -> Option<Sender> {
    start_at(value, url(llm_base_url), key)
}

/// [`start_from`] with the save's URL already worked out (`None`: not the
/// lamp's cloud API).
fn start_at(value: Option<&str>, url: Option<String>, key: &str) -> Option<Sender> {
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
        return refused("the lamp's key is empty or not one line");
    }
    let Some(post) = http_post(url, key.to_owned()) else {
        return refused("no HTTPS client");
    };
    let sender = Sender::new();
    if sender.spawn(post, Waits::default()).is_err() {
        return refused("no thread to send from");
    }
    eprintln!(
        "lamp-voice history: each turn the memory keeps is sent to the account's history ({ENVIRONMENT})"
    );
    Some(sender)
}

/// [`start_from`] with this process's switch, once.
pub fn start_from_environment(llm_base_url: &str, key: &str) {
    if SENDER.get().is_some() {
        return;
    }
    if let Some(sender) = start_from(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        llm_base_url,
        key,
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
