//! The thread of a conversation, kept so that a new provider session can be
//! told how the last one went.
//!
//! A session with the provider does not last: it announces its own close
//! (on lamp-4ace anywhere from 3 to 39 minutes after it opened), and a lost
//! connection is replaced. With resumption off the new session knows nothing
//! of the old one, and a person in the middle of a conversation finds the
//! lamp has forgotten it. This keeps the last few exchanges, as the
//! provider's own transcripts gave them, in this process's memory only, and
//! [`Recalling`] adds them to the instruction of the next session it opens.
//!
//! The note says nothing of sessions, connections or memory, and does not
//! tell the lamp to keep quiet. The first one did both ("your connection was
//! just renewed ... do not mention the renewal ... if nobody speaks to you,
//! say nothing"). The lamp opened its next two answers with "Sorry about
//! that, my connection dropped!", and then answered sixteen times with a
//! spoken "...", four of them to questions put to it.
//!
//! Only exchanges the lamp took part in are kept: what a person said just
//! before an answer, and the answer. Talk around the lamp that it did not
//! answer is dropped when the next thing is heard.
//!
//! By itself this lives in the process's memory and a restart of the
//! service forgets it. Given a file ([`Thread::save`], [`Thread::load`];
//! `--thread-file` to the provider worker) the same few exchanges are also
//! kept there, so that a service that ended and was started again a few
//! seconds later carries on the conversation it was in. On a lamp that file
//! belongs in the service's own runtime directory, which is memory and not
//! a disk, and holds nothing older than the ten minutes a thread is told
//! for.
//!
//! Each exchange is also kept as whose it was: the room's, or that of the
//! person the eyes knew at the lamp ([`Thread::at_lamp`], a memory id, never
//! a name). A session is told only the exchanges of the person it was opened
//! for, and the worker opens a new session between turns when that person
//! changes (#764), so one person's conversation never reaches another's
//! session and theirs is there again when they come back. With nobody ever
//! known everything is the room's, as it always was.
use lamp_gemini::{Attempt, Connect, Dialer, ResumptionHandle, SessionConfig, SessionId};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Exchanges kept for each person (and for the room), and the most of each
/// side of one that is kept.
const EXCHANGES: usize = 12;
/// Whose threads are kept at once, the room's among them: past this the
/// oldest exchanges go, whoever's they were.
const PEOPLE: usize = 4;
const PERSON_BYTES: usize = 300;
const LAMP_BYTES: usize = 400;
/// The end of what the lamp said that is kept beside it, for whether it
/// asked ([`Thread::lamp_asked_by_its_end`]): a read-back longer than
/// [`LAMP_BYTES`] keeps its question here.
const TAIL_BYTES: usize = 160;
/// What a person said counts as the question to an answer that begins within
/// this long of it.
const ASKED_WITHIN: Duration = Duration::from_secs(20);
/// An exchange older than this is not told to a new session: whoever it was
/// with has probably gone. With `LAMP_MEMORY_REOPEN_QUIET`, the second
/// route's conversation quiet this long is over: its next turn asks the
/// memory's note again (`second_provider::worker::Sessions`).
pub(crate) const REMEMBERED_FOR: Duration = Duration::from_secs(600);
/// A conversation whose last exchange is this new is told as one that may
/// go on; an older one as one that is finished. Until 2026-10-13 every one
/// was told as "this conversation ... for you to carry on from the way
/// anyone carries on a conversation they are in". That afternoon lamp-4ace
/// had a character that answers only when called or when it is already
/// talking with someone, in an office where it was neither, and five new
/// sessions in a row, opened from one to seven minutes after its last
/// words, each answered the first or second thing it heard: colleagues'
/// talk ("por" got a sentence about the weather). Each of those answers was
/// then the newest exchange of the thread the next session was told, and
/// the last of the five answered nine times in two minutes
/// (`docs/conversation-thread.md`).
pub const GOING_ON_FOR: Duration = Duration::from_secs(45);
/// A known person's session becomes the room's, once nobody known is at the
/// lamp, only after this long with no exchange (first builder on #775:
/// lamp-4ace stands sideways and seldom sees a face, so a person talking
/// while turned away must keep their conversation).
const QUIET_BEFORE_ROOM: Duration = Duration::from_secs(120);

struct Exchange {
    request: u64,
    at: Instant,
    person: String,
    lamp: String,
    /// The end of what the lamp said, at most [`TAIL_BYTES`]. Not kept in
    /// the file: an older build reads the file with `deny_unknown_fields`,
    /// and would drop the whole thread after a rollback (#1505). A thread
    /// read back from the file takes its kept start for it, so after a
    /// restart a read-back longer than [`LAMP_BYTES`] is known to have
    /// asked only once the lamp speaks again (Fourth on #1499).
    tail: String,
    /// Handed on to the lamp's memory, or not for it to have.
    journalled: bool,
    /// Given to the conversation's running summary
    /// ([`crate::conversation_summary`]); never kept in the file.
    summarized: bool,
    /// Its line was answered in a scope held only by a face's gap or
    /// carried as a person's conversation going on
    /// ([`crate::memory_scope::ScopeSnapshot::held`]): told in the thread
    /// as ever, never given to the running summary, so none of it reaches
    /// a talk note (the reviewer-lead's P2 on #4479). Never kept in the
    /// file.
    held: bool,
    /// Cut before any of the lamp's answer played (`LAMP_SAID_AS_HEARD`): its
    /// part is [`crate::said_as_heard::CUT`], and the exchange is still told,
    /// so the person's words are (the reviewer's P2 on #2145).
    unheard: bool,
    /// The memory id of the known person it was with; `None`: the room's.
    whose: Option<String>,
    /// The note the lamp was given in writing for it, when it began the
    /// turn itself (a written request): not words anyone said.
    written: Option<String>,
}

/// A completed exchange, preserving the scope bound when its response began.
/// This is journal attribution, not request-admission identity authorization.
#[derive(Debug, PartialEq, Eq)]
pub struct FinishedExchange {
    pub request: u64,
    pub age: Duration,
    pub heard: String,
    pub said: String,
    pub whose: Option<String>,
}

pub struct Thread {
    exchanges: VecDeque<Exchange>,
    /// What has been heard since the last answer, and when.
    heard: String,
    heard_at: Option<Instant>,
    heard_done: bool,
    /// An answer has grown since the thread was last saved.
    changed: bool,
    /// The answer that opened with a function's name, if the newest did.
    unsaid: Option<u64>,
    /// The newest answer a line or a Stop cut, kept as what was heard of it
    /// (`LAMP_SAID_AS_HEARD`, [`Self::heard_only`]): no more of it is kept.
    cut: Option<u64>,
    calls: crate::not_speech::CallsLeftOut,
    /// Who is at the lamp now, as the worker counts it: whose session it
    /// is, and whose a new exchange is when the eyes name nobody.
    whose: Option<String>,
    /// Whom the eyes name at the lamp at this moment, fresh: whose a new
    /// exchange is ([`Self::seen_now`]).
    named: Option<String>,
    /// Whom the newest session was opened for: the only exchanges it is told.
    opened_for: Option<String>,
    /// With `LAMP_LIVE_SCOPED_SESSION`, each known person a spoken line was
    /// admitted for in the last two minutes, with their newest such line
    /// ([`Self::admitted`]).
    admitted: Vec<(String, Instant)>,
    /// A written request not yet answered ([`Self::written`]).
    note: Option<Pending>,
    /// How new a thread file this holds: the modification time of the file
    /// last read or written, or the moment of the last forgetting, whichever
    /// is later. A file no newer than this is not read again.
    held: Option<SystemTime>,
    /// Onboarding's sentence in hand ([`Self::count_only`]).
    count_only: Option<u64>,
    /// An add-on's quoted line, the newest ([`Self::quoted`]).
    quoted: Option<u64>,
    /// Each request's own line, as its route held it when the request was
    /// made or its first call came ([`Self::line_of`], #3113): the newest
    /// [`LINES`]. A later line never takes its place.
    lines: VecDeque<(u64, String)>,
}

/// How many requests' own lines are held ([`Thread::line_of`]).
const LINES: usize = 8;

/// One exchange as it is kept in the file: when, by the wall clock, since
/// the process that reads it back has another `Instant`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kept {
    at_unix_ms: u64,
    person: String,
    lamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    whose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    written: Option<String>,
}

/// One exchange as [`Thread::recent`] gives it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Recent {
    /// The note of a request the lamp was given in writing, if it was one.
    pub written: Option<String>,
    /// What the person said; empty for none.
    pub person: String,
    pub lamp: String,
    /// The room's, told to a person's turn as said in the room
    /// ([`Thread::recent_with_room_lines`]); never for a turn of the room's.
    pub room: bool,
}

/// A scope's exchanges due for its running summary ([`Thread::summary_due`]).
#[derive(Debug, PartialEq, Eq)]
pub struct SummaryDue {
    /// A person's memory id; `None`: the room's.
    pub whose: Option<String>,
    /// (request, what the person said, what the lamp said), oldest first.
    pub exchanges: Vec<(u64, String, String)>,
}

/// A written request's note waiting for the answer to it.
#[derive(Clone)]
struct Pending {
    request: u64,
    /// The person it is answered for.
    whose: Option<String>,
    note: String,
}

/// This process's one thread. The provider worker is one process with one
/// session supervisor.
pub static THREAD: Mutex<Thread> = Mutex::new(Thread::new());

/// [`Thread::unsay`] on this process's thread.
pub fn unsay(request: u64) {
    if let Ok(mut thread) = THREAD.lock() {
        thread.unsay(request);
    }
}

/// Onboarding's sentence (#1803) is `request`, or with `None` a new line
/// is not it: [`Thread::count_only`] on this process's thread.
pub fn count_only(request: Option<u64>) {
    if let Ok(mut thread) = THREAD.lock() {
        thread.count_only(request);
    }
}

/// Text of at most `most` bytes, with every run of whitespace or control
/// characters one space.
fn add(to: &mut String, text: &str, most: usize) {
    for c in text.chars() {
        let c = if c.is_control() || c.is_whitespace() {
            ' '
        } else {
            c
        };
        if to.len() + c.len_utf8() > most || (c == ' ' && to.ends_with(' ')) {
            continue;
        }
        to.push(c);
    }
}

/// `text` added to the end of `to`, as [`add`] does, and only the newest
/// `most` bytes of the two kept, cut at a character.
fn keep_end(to: &mut String, text: &str, most: usize) {
    add(to, text, usize::MAX);
    if to.len() > most {
        let mut from = to.len() - most;
        while !to.is_char_boundary(from) {
            from += 1;
        }
        to.drain(..from);
    }
}

impl Thread {
    pub const fn new() -> Self {
        Self {
            exchanges: VecDeque::new(),
            heard: String::new(),
            heard_at: None,
            heard_done: false,
            changed: false,
            unsaid: None,
            cut: None,
            calls: crate::not_speech::CallsLeftOut::new(),
            whose: None,
            named: None,
            opened_for: None,
            admitted: Vec::new(),
            note: None,
            held: None,
            count_only: None,
            quoted: None,
            lines: VecDeque::new(),
        }
    }

    /// Who is at the lamp now, as the worker counts it: a memory id, or
    /// `None` for nobody known. It decides whose session it is.
    pub fn at_lamp(&mut self, whose: Option<String>) {
        self.whose = whose;
    }

    /// Whom the eyes name at the lamp as the voice starts, before its first
    /// session opens, so that session is theirs. Opened for nobody, it was
    /// renewed for them the moment it was ready: the 4 `server_go_away`
    /// drops 0 s after a start on lamp-52e6 on 15 October.
    pub fn at_start(&mut self, known: Option<&str>) {
        let id = known.map(crate::known_faces::memory_person);
        self.whose.clone_from(&id);
        self.named = id;
    }

    /// Whom the eyes name at the lamp at this moment, fresh: a memory id,
    /// or `None`. A new exchange is theirs, as the memory files it; the
    /// counted person's ([`Self::at_lamp`]) only when nobody is named. So
    /// what someone says in another's place, before they count as the one
    /// at the lamp ([`crate::known_faces::SOMEONE_ELSE_AFTER`]) or whether
    /// they ever do, is theirs, and never told to the one before (the
    /// reviewer on #1100). A face kept under two names splits its thread
    /// between them, as its memory already is.
    pub fn seen_now(&mut self, named: Option<String>) {
        self.named = named;
    }

    /// Whom a session opened now would be for. Someone else known: them.
    /// Nobody known after someone was: still that person while anything
    /// was said at the lamp (an exchange) in the last two minutes, since a known person
    /// who keeps talking while turned away from the eyes is not recognised
    /// and must not lose their conversation; the room's after that.
    fn session_person(&self, now: Instant) -> Option<String> {
        // Lines admitted for someone known, lately ([`Self::admitted`]):
        // one person's, then theirs, before the count of who is at the lamp
        // catches up. Two or more known people taking turns: the room's, so
        // neither is told the other's things and no line is dropped at each
        // change of speaker (the reviewer's P2 on #1865).
        let mut lately = self
            .admitted
            .iter()
            .filter(|(_, at)| now.saturating_duration_since(*at) < QUIET_BEFORE_ROOM);
        match (lately.next(), lately.next()) {
            (Some((person, _)), None) => return Some(person.clone()),
            (Some(_), Some(_)) => return None,
            _ => {}
        }
        if self.whose.is_some() || self.opened_for.is_none() {
            return self.whose.clone();
        }
        let said = self.exchanges.back().map(|last| last.at);
        if said.is_some_and(|at| now.saturating_duration_since(at) < QUIET_BEFORE_ROOM) {
            self.opened_for.clone()
        } else {
            None
        }
    }

    /// The person at the lamp is not the one the session was opened for.
    pub fn switch_due(&self, now: Instant) -> bool {
        self.session_person(now) != self.opened_for
    }

    /// Whether to open a new session for the person at the lamp now: only
    /// between turns, with no request open and none of an answer still to
    /// be handed on. A change during a turn waits for its end.
    pub fn switch_now(&self, turn_open: bool, output_pending: bool, now: Instant) -> bool {
        self.switch_due(now) && !turn_open && !output_pending
    }

    /// A session is being opened now, for whoever is at the lamp: returns
    /// whether that is someone other than the last session's.
    pub fn opening(&mut self, now: Instant) -> bool {
        let person = self.session_person(now);
        let changed = person != self.opened_for;
        self.opened_for = person;
        // A written request waiting for its answer is answered for this
        // session's person.
        if let Some(pending) = self.note.as_mut() {
            pending.whose = self.opened_for.clone();
        }
        changed
    }

    /// A spoken line was admitted for `person`, a memory id
    /// (`LAMP_LIVE_SCOPED_SESSION`, [`crate::scoped_session`]): for the next
    /// two minutes the session is theirs, or the room's while someone else
    /// known has spoken too, and a session opened otherwise is opened again
    /// between turns. Returns whether the session now open is someone
    /// else's, so the line must not go into it.
    pub fn admitted(&mut self, person: &str, now: Instant) -> bool {
        self.admitted.retain(|(other, at)| {
            other != person && now.saturating_duration_since(*at) < QUIET_BEFORE_ROOM
        });
        self.admitted.push((person.to_owned(), now));
        crate::scoped_session::refuses(Some(person), self.opened_for.as_deref())
    }

    /// The camera was closed or changed: who a line was admitted for is
    /// no longer known.
    pub fn admitted_unknown(&mut self) {
        self.admitted.clear();
    }

    /// Whom the session being opened is for.
    pub fn opened_for(&self) -> Option<&str> {
        self.opened_for.as_deref()
    }

    /// Take the thread from `path` if the file is newer than what this
    /// holds: another voice route wrote it while this one was away. Says
    /// whether it did. A file that cannot be read changes nothing.
    // ponytail: the file's modification time, not a stamp inside it. A
    // filesystem clock a few ms coarse, or the wall clock stepped back, can
    // only miss a reload, never bring back what was forgotten; a generation
    // stamp in the file if that ever matters.
    pub fn reload(&mut self, path: &Path, now: Instant, wall: SystemTime) -> bool {
        let Some(at) = modified(path).filter(|at| self.held.is_none_or(|held| *at > held)) else {
            return false;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        // A file set aside is no thread taken: `false`, so the caller says
        // nothing more than the set-aside's own line (the reviewer on #1571).
        let (read, took) = match Self::read(&text, now, wall) {
            Ok(read) => (read, true),
            Err(why) => {
                set_aside(path, &why, text.len());
                (Self::new(), false)
            }
        };
        // Who is at the lamp and whom the session was opened for are this
        // process's own; each exchange keeps its `whose` through the file.
        let (whose, named, opened_for, admitted, note) = (
            self.whose.take(),
            self.named.take(),
            self.opened_for.take(),
            std::mem::take(&mut self.admitted),
            self.note.take(),
        );
        let (count_only, quoted) = (self.count_only, self.quoted);
        let marks: Vec<_> = self
            .exchanges
            .iter()
            .filter(|exchange| exchange.held || exchange.summarized)
            .map(|exchange| {
                (
                    exchange.whose.clone(),
                    exchange.person.clone(),
                    exchange.lamp.clone(),
                    exchange.held,
                    exchange.summarized,
                )
            })
            .collect();
        *self = read;
        // The file keeps no mark of a held or summarised exchange: the same
        // exchange read back keeps this process's.
        for exchange in &mut self.exchanges {
            if let Some((.., held, summarized)) = marks.iter().find(|(whose, person, lamp, ..)| {
                *whose == exchange.whose && *person == exchange.person && *lamp == exchange.lamp
            }) {
                exchange.held |= held;
                exchange.summarized |= summarized;
            }
        }
        self.whose = whose;
        self.named = named;
        self.opened_for = opened_for;
        self.admitted = admitted;
        self.note = note;
        // The sentence in hand stays out of a thread read meanwhile.
        self.count_only = count_only;
        self.quoted = quoted;
        self.held = Some(at);
        took
    }

    /// This process has just written the thread to `path`: what is in the
    /// file now is what it holds.
    pub fn wrote(&mut self, path: &Path) {
        self.held = self.held.max(modified(path));
    }

    /// The exchanges still worth telling, as the file's text, if anything
    /// has changed since the last time this was asked.
    pub fn save(&mut self, now: Instant, wall: SystemTime) -> Option<String> {
        if !std::mem::take(&mut self.changed) {
            return None;
        }
        let wall_ms = wall.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
        let kept: Vec<Kept> = self
            .exchanges
            .iter()
            .filter(|exchange| {
                now.saturating_duration_since(exchange.at) <= REMEMBERED_FOR
                    && (exchange.lamp.chars().any(char::is_alphanumeric) || exchange.unheard)
            })
            .map(|exchange| Kept {
                at_unix_ms: wall_ms
                    .saturating_sub(now.saturating_duration_since(exchange.at).as_millis() as u64),
                person: exchange.person.clone(),
                lamp: exchange.lamp.clone(),
                whose: exchange.whose.clone(),
                written: exchange.written.clone(),
            })
            .collect();
        serde_json::to_string(&kept).ok()
    }

    /// A thread from the file's text: what in it is from the last ten
    /// minutes, cut to the usual lengths. Text that is not such a file, or
    /// that claims to be from the future, gives an empty thread.
    pub fn load(text: &str, now: Instant, wall: SystemTime) -> Self {
        Self::read(text, now, wall).unwrap_or_else(|_| Self::new())
    }

    /// [`Self::load`], saying why when the text is not a thread file this
    /// build can read (#1505), so the file can be set aside rather than
    /// silently forgotten. A wall clock before 1970 is no fault of the file:
    /// an empty thread, read.
    pub fn read(text: &str, now: Instant, wall: SystemTime) -> Result<Self, String> {
        let mut thread = Self::new();
        // The error's kind and place, never its message, which can quote a
        // value from the file (the reviewer on #1571).
        let kept = serde_json::from_str::<Vec<Kept>>(text).map_err(|error| {
            format!(
                "{:?} at line {} column {}",
                error.classify(),
                error.line(),
                error.column()
            )
        })?;
        let Ok(wall) = wall.duration_since(UNIX_EPOCH) else {
            return Ok(thread);
        };
        let wall_ms = wall.as_millis() as u64;
        let from = kept.len().saturating_sub(EXCHANGES * PEOPLE);
        for (index, kept) in kept.into_iter().enumerate().skip(from) {
            let Some(age) = wall_ms
                .checked_sub(kept.at_unix_ms)
                .map(Duration::from_millis)
            else {
                continue;
            };
            let Some(at) = now.checked_sub(age).filter(|_| age <= REMEMBERED_FOR) else {
                continue;
            };
            // Only a memory id ([`crate::remember::person_id`]) is whose.
            if kept.whose.as_ref().is_some_and(|whose| {
                whose.len() > 32
                    || !whose
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            }) {
                continue;
            }
            let (mut person, mut lamp) = (String::new(), String::new());
            add(&mut person, &kept.person, PERSON_BYTES);
            add(&mut lamp, &kept.lamp, LAMP_BYTES);
            thread.exchanges.push_back(Exchange {
                // Never the number of a request this process will make.
                request: u64::MAX - index as u64,
                at,
                person,
                unheard: lamp.trim() == crate::said_as_heard::CUT,
                tail: lamp.clone(),
                lamp,
                // The process that wrote the file handed it on.
                journalled: true,
                summarized: false,
                held: false,
                whose: kept.whose,
                written: kept.written.map(|text| {
                    let mut written = String::new();
                    add(&mut written, &text, PERSON_BYTES);
                    written
                }),
            });
        }
        Ok(thread)
    }

    /// The line in hand is onboarding's sentence (#1803), or with `None`
    /// it is not: while it is, nothing heard is kept, and nothing of the
    /// lamp's answer to it either, so its words reach no exchange, no file,
    /// no journal and no later prompt. Set just after its Start; a Start
    /// clears it.
    pub fn count_only(&mut self, request: Option<u64>) {
        self.count_only = request;
    }

    /// Request `request` is an add-on's quoted line (#2530 part 2c):
    /// nothing the lamp says of it is kept, so no later turn is told it
    /// (the reviewer-lead on #2675). What a person says meanwhile is
    /// theirs, kept as ever, and an onboarding sentence stays marked.
    pub fn quoted(&mut self, request: u64) {
        self.quoted = Some(request);
    }

    /// A piece of what the provider heard a person say.
    pub fn person(&mut self, text: &str, finished: bool, now: Instant) {
        if self.count_only.is_some() {
            return;
        }
        // A sign-in's code or email is never kept (#1676).
        let text = &crate::sign_in::scrub(text);
        // Something new after something that was never answered: the old
        // was not said to the lamp.
        let stale = self
            .heard_at
            .is_some_and(|at| now.saturating_duration_since(at) > ASKED_WITHIN);
        if self.heard_done || stale {
            self.heard.clear();
            self.heard_done = false;
        }
        add(&mut self.heard, text, PERSON_BYTES);
        // Scrubbed joined too: a code or an account split across pieces is
        // whole only here (Jarvis's P2 on #2137).
        self.heard = crate::sign_in::scrub_piece(&self.heard);
        self.heard_at = Some(now);
        self.heard_done = finished;
    }

    /// The words heard in the line `request` answers, as far as the thread
    /// knows them: its exchange's, once the lamp has begun answering it,
    /// else what has been heard since the last answer, within the time an
    /// answer counts as asked. `None` for nothing heard (#2176, row 10).
    ///
    /// A line held for the request ([`Self::line_of`]) is its words first:
    /// the next line heard replaces what is pending, so a forget asked by
    /// one person never runs on another's next words (#3113, El's P2 on
    /// #3127).
    pub fn heard_for(&self, request: u64, now: Instant) -> Option<String> {
        let held = self
            .lines
            .iter()
            .rev()
            .find(|(held, _)| *held == request)
            .map(|(_, line)| line.trim());
        let exchange = self.exchanges.iter().rev().find(|e| e.request == request);
        let heard = match (held, exchange) {
            (Some(held), _) => held,
            (None, Some(exchange)) => exchange.person.trim(),
            (None, None)
                if self
                    .heard_at
                    .is_some_and(|at| now.saturating_duration_since(at) <= ASKED_WITHIN) =>
            {
                self.heard.trim()
            }
            (None, None) => "",
        };
        heard
            .chars()
            .any(char::is_alphanumeric)
            .then(|| heard.to_owned())
    }

    /// `request`'s own line is `words` (#3113), held for [`Self::heard_for`]
    /// whatever is heard next; the first held for a request stays.
    pub fn line_of(&mut self, request: u64, words: &str) {
        if self.lines.iter().any(|(held, _)| *held == request) {
            return;
        }
        let words = crate::sign_in::scrub(words);
        let mut line = String::new();
        add(&mut line, &words, PERSON_BYTES);
        if self.lines.len() == LINES {
            self.lines.pop_front();
        }
        self.lines.push_back((request, line));
    }

    /// [`Self::line_of`] with what the thread gives for `request` now, if it
    /// gives words: for a route whose lines come without their request (the
    /// Live route), at its first call, before anything else is heard.
    pub fn hold_line(&mut self, request: u64, now: Instant) {
        if let Some(words) = self.heard_for(request, now) {
            self.line_of(request, &words);
        }
    }

    /// The note of written request `request`: kept with the answer to that
    /// request alone, for the person it is answered for, apart from what
    /// anyone said. An answer to any later request drops it unused (the
    /// request was retired or failed), and so does a forgetting.
    pub fn written(&mut self, request: u64, text: &str) {
        let scope = self.whose.clone().or_else(|| self.opened_for.clone());
        self.written_for(request, text, scope.as_deref());
    }

    /// Keep a written note under its admitted scope, including explicit room scope.
    pub fn written_for(&mut self, request: u64, text: &str, scope: Option<&str>) {
        let text = &crate::sign_in::scrub(text);
        let mut note = String::new();
        add(&mut note, text, PERSON_BYTES);
        self.note = Some(Pending {
            request,
            whose: scope.map(str::to_owned),
            note,
        });
    }

    /// The pending note, if it is `request`'s and `whose`'s; a note of an
    /// earlier request is dropped, one of a later request is kept.
    fn note_for(&mut self, request: u64, whose: &Option<String>) -> Option<String> {
        match self.note.take() {
            Some(pending) if pending.request > request => {
                self.note = Some(pending);
                None
            }
            Some(pending) if pending.request == request && &pending.whose == whose => {
                Some(pending.note)
            }
            _ => None,
        }
    }

    /// A piece of what the lamp said in answer `request`.
    pub fn lamp(&mut self, request: u64, text: &str, now: Instant) {
        let scope = self
            .named
            .clone()
            .or_else(|| self.whose.clone())
            .or_else(|| self.opened_for.clone());
        self.lamp_for(request, text, now, scope.as_deref());
    }

    /// Attribute an answer to its admitted scope rather than a later face observation.
    pub fn lamp_for(&mut self, request: u64, text: &str, now: Instant, scope: Option<&str>) {
        let text = &crate::sign_in::scrub(text);
        // The name of a function said where it should have been called is
        // not something the lamp said: neither that piece nor what follows
        // it in the same answer is kept, told again or taken for a
        // conversation going on.
        if self.unsaid == Some(request)
            || self.count_only == Some(request)
            || self.quoted == Some(request)
        {
            return;
        }
        // An answer cut, kept as what was heard of it: what it still sends
        // was never heard.
        if self.cut == Some(request) {
            return;
        }
        let opening = self.exchanges.back().map(|last| last.request) != Some(request);
        if opening && crate::not_speech::names_a_function(text) {
            self.unsaid = Some(request);
            return;
        }
        // A whole call later in an answer is left out, in however many
        // pieces it came, the rest kept.
        if !self.calls.keeps(request, text) {
            return;
        }
        if opening {
            let asked = self
                .heard_at
                .is_some_and(|at| now.saturating_duration_since(at) <= ASKED_WITHIN);
            let person = if asked {
                std::mem::take(&mut self.heard)
            } else {
                String::new()
            };
            self.heard.clear();
            self.heard_at = None;
            self.heard_done = false;
            // Each person's last EXCHANGES; with nobody ever known, the
            // room's are all there is. The person the eyes name now; nobody
            // named, the one counted; nobody known while a person's session
            // goes on: theirs.
            let whose = scope.map(str::to_owned);
            let theirs = self.exchanges.iter().filter(|e| e.whose == whose);
            if theirs.count() == EXCHANGES {
                if let Some(oldest) = self.exchanges.iter().position(|e| e.whose == whose) {
                    self.exchanges.remove(oldest);
                }
            } else if self.exchanges.len() == EXCHANGES * PEOPLE {
                self.exchanges.pop_front();
            }
            let written = self.note_for(request, &whose);
            self.exchanges.push_back(Exchange {
                request,
                at: now,
                person,
                lamp: String::new(),
                tail: String::new(),
                journalled: false,
                summarized: false,
                held: false,
                unheard: false,
                whose,
                written,
            });
        }
        if let Some(last) = self.exchanges.back_mut() {
            add(&mut last.lamp, text, LAMP_BYTES);
            keep_end(&mut last.tail, text, TAIL_BYTES);
            // Scrubbed joined too (Jarvis's P2 on #2137).
            last.lamp = crate::sign_in::scrub_piece(&last.lamp);
            last.tail = crate::sign_in::scrub_piece(&last.tail);
            self.changed = true;
        }
    }

    /// The exchanges that are over and were not given out before, for the
    /// lamp's memory ([`crate::remember`]): every one but the newest, and
    /// the newest too when `done` is its request. Each with its request,
    /// how long ago it began, what was said, and its originally bound scope.
    /// An answer with no word in it is passed over: it is not something
    /// the lamp said.
    pub fn finished(&mut self, done: u64, now: Instant) -> Vec<FinishedExchange> {
        let newest = self.exchanges.back().map(|last| last.request);
        self.exchanges
            .iter_mut()
            .filter(|exchange| {
                !exchange.journalled
                    && (Some(exchange.request) != newest || exchange.request == done)
            })
            .filter_map(|exchange| {
                exchange.journalled = true;
                (exchange.lamp.chars().any(char::is_alphanumeric) || exchange.unheard).then(|| {
                    FinishedExchange {
                        request: exchange.request,
                        age: now.saturating_duration_since(exchange.at),
                        heard: exchange.person.trim().to_owned(),
                        said: exchange.lamp.trim().to_owned(),
                        whose: exchange.whose.clone(),
                    }
                })
            })
            .collect()
    }

    /// The lamp's words for `request` were never said: the request was
    /// given up before its answer could be played (its end was sent early,
    /// and the person went on). Its exchange is dropped, and what the person
    /// had said before it waits again, as heard, for the answer that
    /// follows, before anything heard since.
    pub fn unsay(&mut self, request: u64) {
        let Some(index) = self
            .exchanges
            .iter()
            .rposition(|exchange| exchange.request == request)
        else {
            return;
        };
        let exchange = self.exchanges.remove(index).expect("found above");
        if self.unsaid == Some(request) {
            self.unsaid = None;
        }
        let mut heard = exchange.person;
        if !self.heard.is_empty() {
            add(&mut heard, " ", PERSON_BYTES);
            add(&mut heard, &self.heard, PERSON_BYTES);
        }
        self.heard = heard;
        self.heard_at = Some(self.heard_at.map_or(exchange.at, |at| at.max(exchange.at)));
        // Whatever comes next of what they say belongs with it.
        self.heard_done = false;
        self.changed = true;
    }

    /// The lamp's words for `request` are only `said`, what was played of its
    /// answer before a line or a Stop cut it (`LAMP_SAID_AS_HEARD`,
    /// [`crate::said_as_heard`]), and nothing more of that answer is kept.
    /// With no word played, the lamp's part is [`crate::said_as_heard::CUT`]
    /// and the exchange is still told and handed on, so the person's words
    /// are never lost with it (the reviewer's P2 on #2145). Nothing changes
    /// for an answer the thread doesn't hold.
    pub fn heard_only(&mut self, request: u64, said: &str) {
        self.cut = Some(request);
        let Some(exchange) = self
            .exchanges
            .iter_mut()
            .rev()
            .find(|exchange| exchange.request == request)
        else {
            return;
        };
        exchange.unheard = !said.chars().any(char::is_alphanumeric);
        let said = if exchange.unheard {
            crate::said_as_heard::CUT
        } else {
            said
        };
        // A sign-in's code or account is never kept (#1676), here either.
        let said = &crate::sign_in::scrub_piece(said);
        exchange.lamp.clear();
        exchange.tail.clear();
        add(&mut exchange.lamp, said, LAMP_BYTES);
        keep_end(&mut exchange.tail, said, TAIL_BYTES);
        self.changed = true;
    }

    /// Drop everything: a person has had something forgotten, and what was
    /// said about it is not to be told to the next session.
    pub fn forget(&mut self) {
        crate::conversation_summary::forget_all();
        self.exchanges.clear();
        self.lines.clear();
        self.note = None;
        self.heard.clear();
        self.heard_at = None;
        self.heard_done = false;
        self.changed = true;
        // A file written before now holds what was forgotten.
        self.held = self.held.max(Some(SystemTime::now()));
    }

    /// Whether the lamp's newest line before `request` ended in a question:
    /// then a bare answer ("23") is an answer to it (#926).
    pub fn lamp_asked(&self, request: u64) -> bool {
        self.exchanges
            .iter()
            .rev()
            .filter(|exchange| exchange.request != request)
            .find(|exchange| exchange.lamp.chars().any(char::is_alphanumeric))
            .is_some_and(|exchange| exchange.lamp.trim_end().ends_with('?'))
    }

    /// [`Self::lamp_asked`], or with `by_its_end` (`LAMP_REMARKS`)
    /// [`Self::lamp_asked_by_its_end`] too. This also reaches the rule's
    /// other uses of `lamp_asked`: a short sound that answers the lamp
    /// (`SOUND_ANSWERS`) and the unclear check (#926).
    pub fn lamp_asked_with(&self, request: u64, by_its_end: bool) -> bool {
        self.lamp_asked(request) || by_its_end && self.lamp_asked_by_its_end(request)
    }

    /// [`Self::lamp_asked`] by how the lamp's newest line before `request`
    /// ended as it was said, not by what is kept of it: its last sentence
    /// ends with `?` or begins as a question does
    /// ([`crate::remarks::lamp_asked`], `LAMP_REMARKS`). A read-back longer
    /// than [`LAMP_BYTES`] lost its `?`, and Gemini's transcript of the
    /// lamp's words often has none.
    pub fn lamp_asked_by_its_end(&self, request: u64) -> bool {
        self.exchanges
            .iter()
            .rev()
            .filter(|exchange| exchange.request != request)
            .find(|exchange| exchange.lamp.chars().any(char::is_alphanumeric))
            .is_some_and(|exchange| crate::remarks::lamp_asked(&exchange.tail))
    }

    /// What the person said in the exchange the lamp's newest answer before
    /// `request` was, when that answer ended by offering to keep or change
    /// something ("Want me to keep that?", `offers`), it began within
    /// [`GOING_ON_FOR`] of `now` and it was `whose`'s, the scope the yes's
    /// `remember` writes to (`None`: the room's): the line a bare "yes" to
    /// the offer is about (#3504, N6). Never the face at the lamp: a yes
    /// from someone else must not keep another's words in their scope
    /// (Reviewer-three's P2 on #3683). `None` else, and for a note the lamp
    /// was given in writing.
    pub fn offered_line(
        &self,
        request: u64,
        whose: Option<&str>,
        now: Instant,
        offers: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let offer = self
            .exchanges
            .iter()
            .rev()
            .filter(|exchange| exchange.request != request)
            .find(|exchange| exchange.lamp.chars().any(char::is_alphanumeric))?;
        let person = offer.person.trim();
        (offer.whose.as_deref() == whose
            && offer.written.is_none()
            && now.saturating_duration_since(offer.at) <= GOING_ON_FOR
            && person.chars().any(char::is_alphanumeric)
            && crate::remarks::lamp_asked(&offer.tail)
            && offers(&offer.tail))
        .then(|| person.to_owned())
    }

    /// Whether the lamp said something to someone so lately that a
    /// conversation may still be going on.
    /// How long ago the lamp last said words in the thread's exchanges, as
    /// [`Self::going_on`] counts them; `None` when it said none: for the
    /// journal (#1712).
    pub fn said_last_ago(&self, now: Instant) -> Option<Duration> {
        self.said_last_ago_but(now, &[])
    }

    /// [`Self::said_last_ago`], the lamp's answers to `but` left out
    /// (`LAMP_GOING_ON_NOT_OWN`).
    pub fn said_last_ago_but(&self, now: Instant, but: &[u64]) -> Option<Duration> {
        self.exchanges
            .iter()
            .filter(|exchange| !but.contains(&exchange.request))
            .filter(|exchange| exchange.lamp.chars().any(char::is_alphanumeric))
            .map(|exchange| now.saturating_duration_since(exchange.at))
            .min()
    }

    /// The known person (a memory id) whose conversation is going on as
    /// `request` is heard: the newest exchange before it, one with words from
    /// the lamp or cut unheard, began within [`GOING_ON_FOR`] of `now` and is
    /// theirs (#4339). `None` for the room's, a quiet, or no exchange: an
    /// exchange of anyone else's after theirs ends it.
    pub fn going_on_with(&self, request: u64, now: Instant) -> Option<String> {
        self.exchanges
            .iter()
            .rev()
            .filter(|exchange| exchange.request != request)
            .find(|exchange| exchange.lamp.chars().any(char::is_alphanumeric) || exchange.unheard)
            .filter(|exchange| now.saturating_duration_since(exchange.at) <= GOING_ON_FOR)
            .and_then(|exchange| exchange.whose.clone())
    }

    pub fn going_on(&self, now: Instant) -> bool {
        self.exchanges.iter().any(|exchange| {
            now.saturating_duration_since(exchange.at) <= GOING_ON_FOR
                && exchange.lamp.chars().any(char::is_alphanumeric)
        })
    }

    /// What a name is kept from ([`crate::known_faces::name_heard`]): the
    /// person's words of the turn in hand, which is what is being heard or,
    /// once the lamp has begun to answer, the newest exchange; and the
    /// `exchanges` before it, oldest first, each with what the lamp said
    /// back. A note the lamp was given in writing is not anyone's words.
    pub fn names_window(&self, exchanges: usize) -> crate::known_faces::Said {
        let spoken: Vec<&Exchange> = self
            .exchanges
            .iter()
            .filter(|exchange| exchange.written.is_none())
            .collect();
        let (this, earlier) = match spoken.split_last() {
            Some((last, rest)) if self.heard.trim().is_empty() => (last.person.clone(), rest),
            _ => (self.heard.clone(), &spoken[..]),
        };
        let from = earlier.len().saturating_sub(exchanges);
        crate::known_faces::Said {
            this,
            before: earlier[from..]
                .iter()
                .map(|exchange| (exchange.person.clone(), exchange.lamp.clone()))
                .collect(),
        }
    }

    /// How many exchanges a scope keeps ([`EXCHANGES`]), so the summary's
    /// tests hold at any window.
    #[cfg(test)]
    pub(crate) const KEPT_A_SCOPE: usize = EXCHANGES;

    /// With `LAMP_CONVERSATION_SUMMARY` ([`crate::conversation_summary`]):
    /// each scope's exchanges older than its newest `keep`, with words, not
    /// yet summarised and not held ([`Self::held_for`]), once they are about to leave what a turn is told:
    /// [`EXCHANGES`] less `keep` less one of them, before the thread drops
    /// the oldest, or the oldest of them three quarters of
    /// [`REMEMBERED_FOR`] old. Each is marked as given, oldest first, as
    /// (request, what the person said, what the lamp said).
    pub fn summary_due(&mut self, keep: usize, now: Instant) -> Vec<SummaryDue> {
        // A window of `keep` or fewer would drop an exchange never given:
        // at least the oldest is given before the scope is full.
        let keep = keep.min(EXCHANGES.saturating_sub(1));
        let batch = EXCHANGES.saturating_sub(keep).saturating_sub(1).max(1);
        let ageing = REMEMBERED_FOR * 3 / 4;
        let mut scopes: Vec<Option<String>> = Vec::new();
        for exchange in &self.exchanges {
            if !scopes.contains(&exchange.whose) {
                scopes.push(exchange.whose.clone());
            }
        }
        let mut due = Vec::new();
        for whose in scopes {
            let theirs: Vec<usize> = (0..self.exchanges.len())
                .filter(|&index| self.exchanges[index].whose == whose)
                .collect();
            let older = theirs.len().saturating_sub(keep);
            let waiting: Vec<usize> = theirs[..older]
                .iter()
                .copied()
                .filter(|&index| {
                    let exchange = &self.exchanges[index];
                    !exchange.summarized
                        && (exchange.lamp.chars().any(char::is_alphanumeric)
                            || !exchange.person.trim().is_empty())
                })
                .collect();
            let old = waiting.first().is_some_and(|&index| {
                now.saturating_duration_since(self.exchanges[index].at) >= ageing
            });
            if waiting.is_empty() || (waiting.len() < batch && !old) {
                continue;
            }
            // A held exchange counts toward the batch, so the others are
            // given before they leave, but is marked and never given.
            let exchanges: Vec<_> = waiting
                .iter()
                .filter_map(|&index| {
                    let exchange = &mut self.exchanges[index];
                    exchange.summarized = true;
                    (!exchange.held).then(|| {
                        (
                            exchange.request,
                            exchange.person.trim().to_owned(),
                            exchange.lamp.trim().to_owned(),
                        )
                    })
                })
                .collect();
            if !exchanges.is_empty() {
                due.push(SummaryDue { whose, exchanges });
            }
        }
        due
    }

    /// A conversation's end (`LAMP_CONVERSATION_SUMMARY`): the exchanges of
    /// scope `whose`, made by `until`, with words, not yet summarised and
    /// not held ([`Self::held_for`]), each marked as given, oldest first; for its talk note.
    pub fn summary_rest(
        &mut self,
        whose: Option<&str>,
        until: Instant,
    ) -> Vec<(u64, String, String)> {
        self.exchanges
            .iter_mut()
            .filter(|exchange| {
                exchange.whose.as_deref() == whose
                    && exchange.at <= until
                    && !exchange.summarized
                    && !exchange.held
                    && (exchange.lamp.chars().any(char::is_alphanumeric)
                        || !exchange.person.trim().is_empty())
            })
            .map(|exchange| {
                exchange.summarized = true;
                (
                    exchange.request,
                    exchange.person.trim().to_owned(),
                    exchange.lamp.trim().to_owned(),
                )
            })
            .collect()
    }

    /// Request `request`'s line was answered in a held scope
    /// ([`crate::memory_scope::ScopeSnapshot::held`]): its exchange is
    /// never given to the running summary ([`Self::summary_due`],
    /// [`Self::summary_rest`]).
    pub fn held_for(&mut self, request: u64) {
        if let Some(exchange) = self
            .exchanges
            .iter_mut()
            .rev()
            .find(|exchange| exchange.request == request)
        {
            exchange.held = true;
        }
    }

    /// The scopes with an exchange of the last `within`.
    pub fn scopes_since(&self, now: Instant, within: Duration) -> Vec<Option<String>> {
        let mut scopes: Vec<Option<String>> = Vec::new();
        for exchange in &self.exchanges {
            if now.saturating_duration_since(exchange.at) <= within
                && !scopes.contains(&exchange.whose)
            {
                scopes.push(exchange.whose.clone());
            }
        }
        scopes
    }

    /// The person's exchanges ([`Self::opening`]) of the last `within`,
    /// at most `most`, oldest first, as (what they said, what the lamp said):
    /// for a route that gives a model the conversation as messages.
    pub fn recent(&self, now: Instant, within: Duration, most: usize) -> Vec<Recent> {
        self.recent_for(self.opened_for.as_deref(), now, within, most)
    }

    /// Read an explicit admitted scope without changing session ownership.
    /// None selects only the room, never the person currently in view.
    pub fn recent_for(
        &self,
        scope: Option<&str>,
        now: Instant,
        within: Duration,
        most: usize,
    ) -> Vec<Recent> {
        let mut recent: Vec<Recent> = self
            .exchanges
            .iter()
            .rev()
            .filter(|exchange| {
                exchange.whose.as_deref() == scope
                    && now.saturating_duration_since(exchange.at) <= within
                    && (exchange.lamp.chars().any(char::is_alphanumeric) || exchange.unheard)
            })
            .take(most)
            .map(|exchange| Recent {
                written: exchange.written.clone(),
                person: exchange.person.trim().to_owned(),
                lamp: exchange.lamp.trim().to_owned(),
                room: false,
            })
            .collect();
        recent.reverse();
        recent
    }

    /// [`Self::recent_for`] `person` (a memory id), with the room's exchanges
    /// of the same conversation among them, each marked as the room's
    /// (`LAMP_CONVERSATION_ROOM_LINES_TOLD`, #4339: on lamp-3e83 on 30 Oct one
    /// conversation went line by line to the person's thread or the room's,
    /// on whether the mouth was read speaking, so a search answered in the
    /// room's was unknown to their next turn). The same conversation: back
    /// from `now`, over their exchanges and the room's only, each within
    /// [`GOING_ON_FOR`] of the one after it; another person's exchange or a
    /// longer quiet ends it, and only the room's at or after their own first
    /// exchange in it are told. Within the same `within` and `most`, the
    /// newest of both, oldest first. One way only: a turn of the room's, or
    /// another person's, is never told theirs ([`Self::recent_for`]). A
    /// forget drops the room's with the rest ([`Self::forget`]). A room's
    /// line narrowed because another voice spoke it is told too: said
    /// aloud beside them, the accepted risk (`docs/memory.md`).
    pub fn recent_with_room_lines(
        &self,
        person: &str,
        now: Instant,
        within: Duration,
        most: usize,
    ) -> Vec<Recent> {
        let mut later = now;
        let mut start = self.exchanges.len();
        for (index, exchange) in self.exchanges.iter().enumerate().rev() {
            if exchange
                .whose
                .as_deref()
                .is_some_and(|whose| whose != person)
                || later.saturating_duration_since(exchange.at) > GOING_ON_FOR
            {
                break;
            }
            later = exchange.at;
            start = index;
        }
        let first_theirs = (start..self.exchanges.len())
            .find(|&index| self.exchanges[index].whose.as_deref() == Some(person));
        let mut recent: Vec<Recent> = self
            .exchanges
            .iter()
            .enumerate()
            .rev()
            .filter(|(index, exchange)| {
                let theirs = exchange.whose.as_deref() == Some(person);
                let rooms =
                    exchange.whose.is_none() && first_theirs.is_some_and(|first| *index >= first);
                (theirs || rooms)
                    && now.saturating_duration_since(exchange.at) <= within
                    && (exchange.lamp.chars().any(char::is_alphanumeric) || exchange.unheard)
            })
            .take(most)
            .map(|(_, exchange)| Recent {
                written: exchange.written.clone(),
                person: exchange.person.trim().to_owned(),
                lamp: exchange.lamp.trim().to_owned(),
                room: exchange.whose.is_none(),
            })
            .collect();
        recent.reverse();
        recent
    }

    /// What to tell a new session, if anything was said lately, in at most
    /// `room` bytes: the latest exchanges that fit, in the order they went,
    /// of the person the session is opened for ([`Self::opening`]) only.
    pub fn recall(&self, now: Instant, room: usize) -> Option<String> {
        self.recall_for(self.opened_for.as_deref(), now, room)
    }

    /// Read an explicit admitted scope without changing session ownership.
    /// None selects only the room, never the person currently in view.
    pub fn recall_for(&self, scope: Option<&str>, now: Instant, room: usize) -> Option<String> {
        // An answer with no word in it ("...") is the model keeping quiet
        // aloud. Told back to it, it is what it does next. One cut before any
        // of it played (`unheard`) is told, for the person's words (the
        // reviewer on #2145).
        let told = |exchange: &&Exchange| {
            exchange.whose.as_deref() == scope
                && now.saturating_duration_since(exchange.at) <= REMEMBERED_FOR
                && (exchange.lamp.chars().any(char::is_alphanumeric) || exchange.unheard)
        };
        let newest = self.exchanges.iter().rev().find(told)?;
        let note = note(now.saturating_duration_since(newest.at));
        let mut kept: Vec<String> = Vec::new();
        let mut bytes = note.len();
        for exchange in self.exchanges.iter().rev().filter(told) {
            let mut lines = String::new();
            if !exchange.person.trim().is_empty() {
                lines.push_str("Person: ");
                lines.push_str(exchange.person.trim());
                lines.push('\n');
            }
            lines.push_str("You: ");
            lines.push_str(exchange.lamp.trim());
            lines.push('\n');
            if bytes + lines.len() > room {
                break;
            }
            bytes += lines.len();
            kept.push(lines);
        }
        (!kept.is_empty()).then(|| {
            kept.reverse();
            format!("{note}{}", kept.concat())
        })
    }
}

/// What goes before the exchanges, the newest of which is this old. See the
/// top of this file for what it must not say.
fn note(age: Duration) -> String {
    if age <= GOING_ON_FOR {
        return GOING_ON.to_owned();
    }
    let minutes = (age.as_secs() + 30) / 60;
    let ago = if minutes <= 1 {
        "About a minute ago".to_owned()
    } else {
        format!("About {minutes} minutes ago")
    };
    format!(
        "\n\nEarlier\n{ago} you and a person at your desk said this to each other. That conversation is finished and they have gone back to their work. It is background only, for when someone comes back to it: never read it out and never answer any of it again.\n"
    )
}

const GOING_ON: &str = "\n\nA moment ago\nThis is what you and the person at your desk have just said to each other, for you to carry on from if they go on with it. It is background only: never read it out and never answer any of it again.\n";

/// The provider worker's flag for the file, and where the coordinator
/// reads its path. Unset: the thread is in memory only.
pub const FILE_FLAG: &str = "--thread-file";
pub const FILE_ENVIRONMENT: &str = "LAMP_THREAD_FILE";

pub(crate) fn usable(path: &Path) -> bool {
    path.is_absolute()
        && path.as_os_str().len() <= 200
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
}

/// The provider worker's arguments for the path in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    std::env::var(FILE_ENVIRONMENT)
        .ok()
        .filter(|path| usable(Path::new(path)))
        .map(|path| vec![FILE_FLAG.to_owned(), path])
        .unwrap_or_default()
}

/// Take a leading `--thread-file PATH` off a worker's flags.
pub fn take_file(flags: &[String]) -> std::io::Result<(Option<std::path::PathBuf>, &[String])> {
    match flags {
        [flag, path, rest @ ..] if flag == FILE_FLAG && usable(Path::new(path)) => {
            Ok((Some(path.into()), rest))
        }
        [flag, ..] if flag == FILE_FLAG => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the thread's file needs a short absolute path",
        )),
        _ => Ok((None, flags)),
    }
}

/// Each thread file a set-aside line was said for, so a test can count the
/// lines for its own file while others run beside it.
#[cfg(test)]
static SET_ASIDE_SAID: std::sync::Mutex<Vec<std::path::PathBuf>> =
    std::sync::Mutex::new(Vec::new());

/// A thread file this build cannot read, renamed beside itself as
/// `<name>.unreadable-<unix_ms>` and said once, with why and its size,
/// never its words (#1505): set aside, not silently forgotten. In `/run`,
/// so a reboot ends it. A rename that fails is said, and changes nothing.
fn set_aside(path: &Path, why: &str, bytes: usize) {
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let mut kept = path.as_os_str().to_owned();
    kept.push(format!(".unreadable-{at}"));
    let kept = std::path::PathBuf::from(kept);
    #[cfg(test)]
    SET_ASIDE_SAID.lock().unwrap().push(path.to_path_buf());
    match std::fs::rename(path, &kept) {
        Ok(()) => eprintln!(
            "lamp-voice: the thread file {} could not be read ({why}; {bytes} bytes); set aside as {}, and the thread starts empty",
            path.display(),
            kept.display()
        ),
        Err(error) => eprintln!(
            "lamp-voice: the thread file {} could not be read ({why}; {bytes} bytes), nor set aside ({error}); the thread starts empty",
            path.display()
        ),
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|file| file.modified())
        .ok()
}

/// Put the thread's text in its file, whole or not at all, for its owner
/// only.
pub fn write_file(path: &Path, text: &str) -> std::io::Result<()> {
    let mut writing = path.as_os_str().to_owned();
    writing.push(".new");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&writing)?;
    file.write_all(text.as_bytes())?;
    drop(file);
    std::fs::rename(&writing, path)
}

/// `told` on the thread as it stands now, for a voice route about to tell
/// its model: anything forgotten first, so that a file written before the
/// forgetting is not taken back, then `file` if another voice route wrote
/// it since ([`Thread::reload`]). `None` if the thread cannot be had.
pub fn current<R>(file: Option<&Path>, told: impl FnOnce(&Thread) -> R) -> Option<R> {
    let mut thread = THREAD.lock().ok()?;
    crate::remember::clear_forgotten_thread(&mut thread);
    if let Some(path) = file
        && thread.reload(path, Instant::now(), SystemTime::now())
    {
        eprintln!("lamp-voice: took the thread another voice route wrote");
    }
    Some(told(&thread))
}

impl Default for Thread {
    fn default() -> Self {
        Self::new()
    }
}

/// Opens each session with the thread so far added to its instruction.
pub struct Recalling {
    config: SessionConfig,
    tools: crate::tools::Settings,
    name_signal: bool,
    /// The person at the lamp has changed once: no resumption handle is
    /// presented again, since one could carry the earlier person's
    /// conversation into the new session.
    // ponytail: off for the rest of the process after the first change,
    // even for a later reconnect of one person's session; lamps run with
    // resumption off (the relay does not forward it), and the thread
    // covers a fresh session. Per-handle ownership if that changes.
    fresh_only: bool,
    file: Option<PathBuf>,
    /// What each session is given, shared with the provider loop (#1472).
    given: Option<std::sync::Arc<crate::session_tools::Given>>,
}

/// Whether a Live connect may present its resumption handle: as the session
/// allows, and never while the memory wants a fresh context after
/// journalling stopped (`LAMP_MEMORY_JOURNAL_RESUMES`, Friday's P1 and P3 on
/// #2226): what the provider held of the earlier session, forgotten words
/// included, is then left behind.
pub(crate) fn presents_resumption(allowed: bool) -> bool {
    allowed && !crate::remember::wants_fresh_live()
}

impl Recalling {
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            tools: crate::tools::Settings::default(),
            name_signal: false,
            fresh_only: false,
            file: None,
            given: None,
        }
    }

    /// Each session's functions are kept here, and a renewal for a changed
    /// catalog opens the next one fresh ([`crate::session_tools`]).
    pub fn given(mut self, given: std::sync::Arc<crate::session_tools::Given>) -> Self {
        self.given = Some(given);
        self
    }

    /// Each session first takes the thread from this file if another voice
    /// route wrote it since ([`Thread::reload`]).
    pub fn file(mut self, file: Option<PathBuf>) -> Self {
        self.file = file;
        self
    }

    /// Each session is also asked to say when it hears the lamp's name
    /// ([`crate::attention::name_tool`]).
    pub fn name_signal(mut self, on: bool) -> Self {
        self.name_signal = on;
        self
    }

    /// Each session is also told what the model may do other than speak
    /// ([`crate::tools`]), as that stands when the session opens.
    pub fn tools(mut self, tools: crate::tools::Settings) -> Self {
        self.tools = tools;
        self
    }

    /// As [`Recalling::equipped_with`], the functions read here, for a
    /// test: a session is opened through [`Recalling::opened`], which reads
    /// them once.
    #[cfg(test)]
    fn equipped(&self, config: SessionConfig) -> SessionConfig {
        self.equipped_with(config, crate::tools::declared(&self.tools))
    }

    /// `config` with the functions `tools`, declared now. The executive's
    /// come and go with the executive. One the link would refuse is left
    /// out, the rest are kept, and the journal says which and why (#799):
    /// no setting leaves a session without functions, or says nothing.
    fn equipped_with(
        &self,
        config: SessionConfig,
        mut tools: Vec<lamp_gemini::Tool>,
    ) -> SessionConfig {
        // What this session is given, against which a changed catalog is
        // told apart, and the journal's line for it (#1472).
        if let Some(given) = &self.given {
            given.record(&tools);
        }
        eprintln!("{}", crate::session_tools::setup_line(&tools, &self.tools));
        // Without the light's, the volume's or the reminders' functions it
        // is told what it can't do right now, and with `use_skill` its
        // skills' lines (#1668), in the room kept for them.
        let config = match crate::session_tools::told_after(&tools, &self.tools) {
            Some(line) => {
                let told = format!("{}{line}", config.instruction_text());
                config.clone().instruction(&told).unwrap_or_else(|_| {
                    eprintln!(
                        "lamp-voice: the session is not told what it can't do right now, nor its skills: its instruction is full"
                    );
                    config
                })
            }
            None => config,
        };
        if self.name_signal && tools.len() < lamp_gemini::MAX_TOOLS {
            tools.push(crate::attention::name_tool());
        }
        let (kept, left_out) = crate::tools::admitted(tools);
        if !left_out.is_empty() {
            eprintln!(
                "lamp-voice: left out of the session, which the link would refuse, keeping the other {}: {}",
                kept.len(),
                left_out.join(", ")
            );
        }
        config.clone().tools(kept).unwrap_or(config)
    }

    /// The configuration for a session opened now, told the time and this
    /// thread, nothing kept after it.
    #[cfg(test)]
    fn recalled(&self, thread: &Thread, now: Instant, time: Option<&str>) -> SessionConfig {
        self.recalled_keeping(thread, now, time, 0)
    }

    /// What a session opened now is given: its functions declared once;
    /// from the thread's recall, the room the line of what it can't do
    /// right now needs ([`crate::session_tools::not_ready`]), and none when
    /// it can do it all, so a session with its functions keeps its whole
    /// recall; the recall (`recall(keep)`, none when there is no thread);
    /// and those same functions. One read decides the line, the room and
    /// what the session is sent (the reviewer and Jarvis on #1513).
    fn opened(&self, recall: impl FnOnce(usize) -> Option<SessionConfig>) -> SessionConfig {
        let tools = crate::tools::declared(&self.tools);
        let keep =
            crate::session_tools::told_after(&tools, &self.tools).map_or(0, |line| line.len());
        let config = recall(keep).unwrap_or_else(|| self.config.clone());
        self.equipped_with(config, tools)
    }

    /// As [`Recalling::recalled`], `keep` bytes left after the thread.
    fn recalled_keeping(
        &self,
        thread: &Thread,
        now: Instant,
        time: Option<&str>,
        keep: usize,
    ) -> SessionConfig {
        let written = self.config.instruction_text();
        let time = time.unwrap_or_default();
        let told = |more: &str| {
            if more.is_empty() {
                return Some(self.config.clone());
            }
            self.config
                .clone()
                .instruction(&format!("{written}{more}"))
                .ok()
        };
        // The thread takes the room the file, the time and what `keep`
        // keeps leave it; a time too long to add leaves the session as it
        // always was.
        let room =
            lamp_gemini::MAX_INSTRUCTION_BYTES.saturating_sub(written.len() + time.len() + keep);
        thread
            .recall(now, room)
            .and_then(|thread| told(&format!("{time}{thread}")))
            .or_else(|| told(time))
            .unwrap_or_else(|| self.config.clone())
    }
}

impl Recalling {
    /// Whether the session being opened may present a resumption handle:
    /// not once the person at the lamp has changed, nor for a session
    /// renewed for a changed catalog, which is opened fresh because
    /// Google's resumption with other functions is not tested (#1472). The
    /// renewal's flag is taken here, once. Nor for one renewed because it
    /// holds the typed name and a line it may not have comes (#2602,
    /// `crate::person::open_fresh_live`).
    fn presents_handle(&mut self) -> bool {
        let renewed = self.given.as_ref().is_some_and(|given| given.take_fresh());
        let for_the_name = crate::person::take_fresh_live();
        !renewed && !for_the_name && !self.fresh_only
    }
}

impl Connect for Recalling {
    fn connect(&mut self, session: SessionId, mut resume: Option<ResumptionHandle>) -> Attempt {
        // The session is for whoever is at the lamp now, and only theirs:
        // their thread below, and their kept things.
        let person = match THREAD.lock() {
            Ok(mut thread) => {
                self.fresh_only |= thread.opening(Instant::now());
                thread.opened_for().map(str::to_owned)
            }
            Err(_) => None,
        };
        // After a stop, a reconnect is made fresh, so journalling can go on
        // (Friday's P1 on #2226): what the provider held of the earlier
        // session, forgotten words included, is left behind.
        if !presents_resumption(self.presents_handle()) {
            resume = None;
        }
        let mut time = crate::local_time::told_here(self.tools.clock, self.tools.places);
        // Work asked of the staff is theirs, stories included (#854); said
        // after the persona, which asks for stories to be told gladly.
        time = crate::agent_tasks::noted(time, self.tools.tasks.as_ref());
        // The legacy cached note, or an explicitly opted-in Days snapshot
        // with its provenance and a total acquisition budget of 300 ms.
        if let Some(socket) = &self.tools.memory {
            // Fresh when no resumption is presented: Live's server-side
            // context then holds nothing of an earlier session (#2176 row 1b).
            let kept = crate::remember::told_for_fresh_session(
                socket,
                session.get(),
                person.as_deref(),
                resume.is_none(),
            );
            if !kept.is_empty() {
                time = Some(time.unwrap_or_default() + &kept);
            }
        }
        let config = self.opened(|keep| {
            current(self.file.as_deref(), |thread| {
                self.recalled_keeping(thread, Instant::now(), time.as_deref(), keep)
            })
        });
        Dialer::new(config).connect(session, resume)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a session is sent at setup follows the switch: nothing about
    /// functions with it off, the voice's own with it on.
    #[test]
    fn a_session_is_opened_with_the_functions_the_model_may_call_now() {
        let config = || {
            SessionConfig::new(
                lamp_gemini::GOOGLE_ENDPOINT,
                lamp_gemini::Credential::api_key("test-only").unwrap(),
            )
            .unwrap()
        };
        let setup = |recalling: &Recalling| -> serde_json::Value {
            let text = lamp_gemini::wire::encode_setup(&recalling.equipped(config())).unwrap();
            serde_json::from_str(&text).unwrap()
        };
        let plain = Recalling::new(config());
        assert!(setup(&plain)["setup"].get("tools").is_none());
        let declining = Recalling::new(config()).tools(crate::tools::Settings {
            decline: true,
            exec: None,
            memory: None,
            clock: false,
            ..Default::default()
        });
        let told = setup(&declining);
        let declared = &told["setup"]["tools"][0]["functionDeclarations"];
        assert_eq!(declared.as_array().unwrap().len(), 1);
        assert_eq!(declared[0]["name"], crate::tools::NOT_FOR_ME);
        assert!(declared[0].get("parameters").is_none());
    }

    /// #1472: a session is equipped from the executive's catalog as it
    /// stands when it opens, so the one opened after the light's functions
    /// came back declares them. This was already so before #1472; the
    /// renewal (`provider_worker/tests/catalog.rs`) relies on it.
    #[test]
    fn a_session_opened_after_the_catalog_changed_declares_what_it_holds_now() {
        let folder = std::env::temp_dir().join(format!("lv-thread-cat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let catalog = |names: &[&str]| {
            let tools: Vec<_> = names
                .iter()
                .map(|name| serde_json::json!({"name": name, "description": "Do it."}))
                .collect();
            std::fs::write(
                folder.join(crate::tools::CATALOG_FILE),
                serde_json::json!({"tools": tools}).to_string(),
            )
            .unwrap();
        };
        let recalling = Recalling::new(
            SessionConfig::new(
                lamp_gemini::GOOGLE_ENDPOINT,
                lamp_gemini::Credential::api_key("test-only").unwrap(),
            )
            .unwrap(),
        )
        .tools(crate::tools::Settings {
            exec: Some(folder.clone()),
            ..Default::default()
        });
        let declared = |recalling: &Recalling| -> Vec<String> {
            let config = recalling.equipped(recalling.config.clone());
            let text = lamp_gemini::wire::encode_setup(&config).unwrap();
            let setup: serde_json::Value = serde_json::from_str(&text).unwrap();
            setup["setup"]["tools"][0]["functionDeclarations"]
                .as_array()
                .map(|all| {
                    all.iter()
                        .map(|tool| tool["name"].as_str().unwrap().to_owned())
                        .collect()
                })
                .unwrap_or_default()
        };
        catalog(&["volume_up"]);
        assert_eq!(declared(&recalling), ["volume_up"]);
        catalog(&["volume_up", "light_colour"]);
        assert_eq!(declared(&recalling), ["volume_up", "light_colour"]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// #1668: with `use_skill`, a session is given that one function for
    /// every skill, its instruction lists the skills after the rest, and
    /// the room for that list is kept from the thread when it is opened.
    #[test]
    fn with_use_skill_a_session_is_told_its_skills_after_its_instruction() {
        let folder = std::env::temp_dir().join(format!("lv-thread-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join(crate::tools::SKILLS_DIR)).unwrap();
        std::fs::write(
            folder.join(crate::tools::CATALOG_FILE),
            serde_json::json!({"tools":[
                {"name":"light_colour","description":"Colour."},
                {"name":"volume_up","description":"Louder."}]})
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            folder
                .join(crate::tools::SKILLS_DIR)
                .join(crate::tools::CATALOG_FILE),
            serde_json::json!({"tools":[{"name":"dance","description":"When asked to dance."}]})
                .to_string(),
        )
        .unwrap();
        let config = SessionConfig::new(
            lamp_gemini::GOOGLE_ENDPOINT,
            lamp_gemini::Credential::api_key("test-only").unwrap(),
        )
        .unwrap()
        .instruction("You are a lamp.")
        .unwrap();
        let settings = crate::tools::Settings {
            exec: Some(folder.clone()),
            use_skill: true,
            ..Default::default()
        };
        let section = crate::use_skill::section(&crate::tools::skill_tools(&settings));
        assert!(
            section.contains("- dance: When asked to dance."),
            "{section}"
        );
        let recalling = Recalling::new(config.clone()).tools(settings.clone());
        let equipped = recalling.equipped(config.clone());
        assert_eq!(
            equipped.instruction_text(),
            format!("You are a lamp.{section}")
        );
        let opened = recalling.opened(|keep| {
            assert_eq!(keep, section.len());
            None
        });
        assert_eq!(
            opened.instruction_text(),
            format!("You are a lamp.{section}")
        );
        // Without the switch: dance is its own function, and nothing is
        // told after the instruction.
        let off = Recalling::new(config.clone()).tools(crate::tools::Settings {
            use_skill: false,
            ..settings
        });
        assert_eq!(off.equipped(config).instruction_text(), "You are a lamp.");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The first builder's lamp run on #1495 (13:57): a session opened
    /// while the light's functions were out said it changed the light five
    /// times and made no call. One given none of them is told it can't
    /// change the light right now, after the rest of its instruction; one
    /// given them is told nothing of it, and nor is a voice without an
    /// executive.
    #[test]
    fn a_session_without_the_lights_functions_is_told_it_cannot_change_the_light() {
        let folder = std::env::temp_dir().join(format!("lv-thread-nolight-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let catalog = |names: &[&str]| {
            let tools: Vec<_> = names
                .iter()
                .map(|name| serde_json::json!({"name": name, "description": "Do it."}))
                .collect();
            std::fs::write(
                folder.join(crate::tools::CATALOG_FILE),
                serde_json::json!({"tools": tools}).to_string(),
            )
            .unwrap();
        };
        let config = SessionConfig::new(
            lamp_gemini::GOOGLE_ENDPOINT,
            lamp_gemini::Credential::api_key("test-only").unwrap(),
        )
        .unwrap()
        .instruction("You are a lamp.")
        .unwrap();
        let recalling = Recalling::new(config.clone()).tools(crate::tools::Settings {
            exec: Some(folder.clone()),
            ..Default::default()
        });
        let told = |recalling: &Recalling| {
            recalling
                .equipped(recalling.config.clone())
                .instruction_text()
                .to_owned()
        };
        catalog(&["volume_up"]);
        assert_eq!(
            told(&recalling),
            format!("You are a lamp.{}", crate::session_tools::NO_LIGHT)
        );
        assert!(told(&recalling).contains("Never say you changed it."));
        // The catalog gone (lamp-actions down): told it can't change its
        // light or its voice either (the first builder's lamp run on #1513:
        // "I've turned my voice down" with no function at all).
        std::fs::remove_file(folder.join(crate::tools::CATALOG_FILE)).unwrap();
        assert!(told(&recalling).ends_with(
            "You cannot change your light or change how loud your voice is right now: \
             they are not ready. If someone asks for any of them, say so in a few words \
             of your own, not the same way twice. Never say you did it."
        ));
        catalog(&["volume_up", "light_colour"]);
        assert_eq!(told(&recalling), "You are a lamp.");
        assert_eq!(told(&Recalling::new(config)), "You are a lamp.");
        // The thread leaves it room. With an instruction that leaves a
        // little less than the line and the thread together, the thread is
        // not recalled and the line is; without the room kept, the thread
        // would be, and the line would not fit.
        catalog(&["volume_up"]);
        let mut thread = Thread::new();
        let now = Instant::now();
        thread.person("My name is Sam, and I sit at this desk.", true, now);
        thread.lamp(3, "Nice to meet you, Sam.", now);
        let recall = thread.recall(now, usize::MAX).unwrap().len();
        let long = "x".repeat(
            lamp_gemini::MAX_INSTRUCTION_BYTES - crate::session_tools::NO_LIGHT.len() - recall + 20,
        );
        let near_full = Recalling::new(recalling.config.clone().instruction(&long).unwrap()).tools(
            crate::tools::Settings {
                exec: Some(folder.clone()),
                ..Default::default()
            },
        );
        // Through `opened`, as `connect` opens a session.
        let open = |recalling: &Recalling| {
            recalling.opened(|keep| {
                assert!(
                    keep == 0 || keep == crate::session_tools::NO_LIGHT.len(),
                    "{keep}"
                );
                Some(recalling.recalled_keeping(&thread, now, None, keep))
            })
        };
        assert_eq!(
            open(&near_full).instruction_text(),
            format!("{long}{}", crate::session_tools::NO_LIGHT)
        );
        // The catalog gone: the longer line's room is kept, and it fits too
        // (the reviewer on #1549).
        std::fs::remove_file(folder.join(crate::tools::CATALOG_FILE)).unwrap();
        let line = crate::session_tools::not_ready(&[], &near_full.tools).unwrap();
        assert!(line.len() > crate::session_tools::NO_LIGHT.len());
        let longer = "x".repeat(lamp_gemini::MAX_INSTRUCTION_BYTES - line.len() - recall + 20);
        let near_full_longer = Recalling::new(
            recalling.config.clone().instruction(&longer).unwrap(),
        )
        .tools(crate::tools::Settings {
            exec: Some(folder.clone()),
            ..Default::default()
        });
        assert_eq!(
            near_full_longer
                .opened(|keep| {
                    assert_eq!(keep, line.len());
                    Some(near_full_longer.recalled_keeping(&thread, now, None, keep))
                })
                .instruction_text(),
            format!("{longer}{line}")
        );
        // With the light, nothing is kept: its recall has all its room.
        catalog(&["volume_up", "light_colour"]);
        let whole = open(&near_full);
        assert!(whole.instruction_text().contains("My name is Sam"));
        assert!(
            !whole
                .instruction_text()
                .ends_with(crate::session_tools::NO_LIGHT)
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The reviewer on #1484: a session renewed for a changed catalog
    /// presents no handle, once; the next presents one again, unless the
    /// person at the lamp has changed, after which none ever does.
    #[test]
    fn a_session_renewed_for_its_functions_is_opened_without_a_handle_once() {
        let given = std::sync::Arc::new(crate::session_tools::Given::default());
        let mut recalling = Recalling::new(
            SessionConfig::new(
                lamp_gemini::GOOGLE_ENDPOINT,
                lamp_gemini::Credential::api_key("test-only").unwrap(),
            )
            .unwrap(),
        )
        .given(given.clone());
        assert!(recalling.presents_handle());
        given.open_fresh();
        assert!(!recalling.presents_handle());
        assert!(recalling.presents_handle());
        // Renewed because it holds the typed name (#2602): once, too.
        crate::person::open_fresh_live();
        assert!(!recalling.presents_handle());
        assert!(recalling.presents_handle());
        recalling.fresh_only = true;
        assert!(!recalling.presents_handle());
        assert!(!recalling.presents_handle());
        // Without a record, as before #1472.
        let mut plain = Recalling::new(recalling.config.clone());
        assert!(plain.presents_handle());
    }

    /// The first builder on #1588: the session renewed when the light comes
    /// back in a conversation is opened fresh, with the light's functions,
    /// not told it can't change it, and with the conversation recalled, so
    /// "and a bit warmer" right after "make your light orange" is still
    /// understood.
    #[test]
    fn a_session_renewed_for_the_light_back_recalls_the_conversation() {
        let folder = std::env::temp_dir().join(format!("lv-thread-back-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let catalog = |names: &[&str]| {
            let tools: Vec<_> = names
                .iter()
                .map(|name| serde_json::json!({"name": name, "description": "Do it."}))
                .collect();
            std::fs::write(
                folder.join(crate::tools::CATALOG_FILE),
                serde_json::json!({"tools": tools}).to_string(),
            )
            .unwrap();
        };
        let given = std::sync::Arc::new(crate::session_tools::Given::default());
        let mut recalling = Recalling::new(
            SessionConfig::new(
                lamp_gemini::GOOGLE_ENDPOINT,
                lamp_gemini::Credential::api_key("test-only").unwrap(),
            )
            .unwrap()
            .instruction("You are a lamp.")
            .unwrap(),
        )
        .tools(crate::tools::Settings {
            exec: Some(folder.clone()),
            ..Default::default()
        })
        .given(given.clone());
        let now = Instant::now();
        let open = |recalling: &Recalling, thread: &Thread| {
            recalling
                .opened(|keep| Some(recalling.recalled_keeping(thread, now, None, keep)))
                .instruction_text()
                .to_owned()
        };
        let has_light = || given.now().unwrap().contains("light_colour");
        // Opened in the gap: no light, and told so.
        catalog(&["volume_up"]);
        let mut thread = Thread::new();
        assert!(open(&recalling, &thread).ends_with(crate::session_tools::NO_LIGHT));
        assert!(!has_light());
        // Asked, and the lamp said it can't.
        thread.person("Make your light orange.", true, now);
        thread.lamp(1, "I can't change my light right now.", now);
        // The light back: the session is renewed, fresh.
        catalog(&["volume_up", "light_colour"]);
        given.open_fresh();
        assert!(!recalling.presents_handle());
        let renewed = open(&recalling, &thread);
        assert!(has_light());
        assert!(
            !renewed.contains("You cannot change your light"),
            "{renewed}"
        );
        assert!(renewed.contains("Make your light orange."), "{renewed}");
        assert!(
            renewed.contains("I can't change my light right now."),
            "{renewed}"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    const ROOM: usize = 8_192;

    /// A read-back longer than the 400 bytes kept loses its "?" in what is
    /// kept, and Gemini's transcript may drop it too: by its end the lamp
    /// still asked (#705, 13:0x: "No, not now." after "Shall I start it?"
    /// had `lamp_asked` unset, six times in a night).
    #[test]
    fn the_lamps_question_is_known_by_its_end_past_what_is_kept() {
        let now = Instant::now();
        for (asked, last) in [
            (true, "Shall I start it?"),
            (true, "Shall I start it"),
            (false, "It is done."),
        ] {
            let mut thread = Thread::new();
            thread.person("Ask Claude Code to write a long poem.", true, now);
            // In pieces, as the transcript comes: more than 400 bytes.
            for _ in 0..12 {
                thread.lamp(
                    1,
                    "I can ask Claude Code to write a long poem for you. ",
                    now,
                );
            }
            thread.lamp(1, last, now);
            assert!(
                !thread.lamp_asked(2),
                "the kept words end before the question"
            );
            assert_eq!(thread.lamp_asked_by_its_end(2), asked, "{last}");
            assert!(
                !thread.lamp_asked_with(2, false),
                "without the switch, as before"
            );
            assert_eq!(
                thread.lamp_asked_with(2, true),
                asked,
                "{last}, with the switch"
            );
            assert!(
                !thread.lamp_asked_by_its_end(1),
                "a request's own words are not before it"
            );
        }
    }

    #[test]
    fn it_keeps_what_was_asked_and_answered_and_not_the_talk_around_it() {
        let mut thread = Thread::new();
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        assert_eq!(thread.recall(at(0), ROOM), None);
        // Two people talking to each other: heard, never answered.
        thread.person("Did you send the report", false, at(1));
        thread.person(" to Maria?", true, at(2));
        thread.person("I did, but she has not replied.", true, at(5));
        assert_eq!(thread.recall(at(6), ROOM), None);
        // Then the lamp is asked something, in pieces, and answers in pieces.
        thread.person("Hey Lamp, my favourite", false, at(10));
        thread.person("  colour is\ngreen.", true, at(11));
        thread.lamp(7, "Green is a ", at(12));
        thread.lamp(7, "lovely colour!", at(13));
        // An answer with nothing heard before it: the lamp's line alone.
        thread.lamp(8, "Still here if you need me.", at(60));
        let recalled = thread.recall(at(61), ROOM).unwrap();
        assert!(recalled.ends_with(
            "Person: Hey Lamp, my favourite colour is green.\nYou: Green is a lovely colour!\nYou: Still here if you need me.\n"
        ), "{recalled}");
        assert!(!recalled.contains("Maria") && !recalled.contains("replied"));
        // Nothing in it for the lamp to talk about but the conversation,
        // whether that is going on or finished.
        assert!(recalled.starts_with("\n\nA moment ago\n"), "{recalled}");
        assert!(thread.going_on(at(61)) && !thread.going_on(at(60 + 46)));
        let later = thread.recall(at(60 + 46), ROOM).unwrap();
        assert!(
            later.starts_with("\n\nEarlier\nAbout a minute ago you and a person"),
            "{later}"
        );
        assert!(later.contains("That conversation is finished"));
        assert!(later.ends_with("You: Still here if you need me.\n"));
        assert!(
            thread
                .recall(at(60 + 400), ROOM)
                .unwrap()
                .starts_with("\n\nEarlier\nAbout 7 minutes ago you")
        );
        for told in [&recalled, &later] {
            let note = told.split("Person: ").next().unwrap().to_lowercase();
            for word in [
                "connect", "renew", "session", "memory", "forg", "lost", "sorry", "silen", "quiet",
                "nothing",
            ] {
                assert!(!note.contains(word), "{word}: {note}");
            }
            // Only a conversation that may go on is one to carry on.
            assert_eq!(note.contains("carry on"), std::ptr::eq(told, &recalled));
        }
        // Keeping quiet aloud is not an answer to carry on from.
        thread.person("Are you coming to lunch with us?", true, at(70));
        thread.lamp(9, "...", at(71));
        thread.person("Maybe.", true, at(72));
        thread.lamp(10, " \u{2026} ", at(73));
        let recalled = thread.recall(at(74), ROOM).unwrap();
        assert!(
            recalled.ends_with("You: Still here if you need me.\n"),
            "{recalled}"
        );
        assert!(!recalled.contains("lunch") && !recalled.contains("Maybe"));
    }

    /// The end sent early (`--early-end-ms`): an answer that began to
    /// arrive before the lamp was sure, for a request then given up because
    /// the person went on, was never said. The person's words wait for the
    /// answer to what they went on to say.
    #[test]
    fn words_of_a_request_given_up_before_its_answer_played_were_never_said() {
        let mut thread = Thread::new();
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        thread.person("Hey Lamp,", true, at(1));
        thread.lamp(1, "Yes? How can", at(2));
        thread.person(" what time", false, at(2));
        thread.unsay(1);
        assert_eq!(thread.recall(at(3), ROOM), None);
        assert!(!thread.going_on(at(3)));
        thread.person(" is it?", true, at(3));
        thread.lamp(2, "It's three.", at(4));
        let recalled = thread.recall(at(5), ROOM).unwrap();
        assert!(
            recalled.ends_with("Person: Hey Lamp, what time is it?\nYou: It's three.\n"),
            "{recalled}"
        );
        assert!(!recalled.contains("How can"));
        // Nothing for a request that has no words, and nothing else moves.
        thread.unsay(7);
        assert_eq!(thread.recall(at(5), ROOM).unwrap(), recalled);
        let finished = thread.finished(2, at(5));
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].request, 2);
    }

    /// `LAMP_SAID_AS_HEARD`: an answer a line cut after its first words is
    /// kept as those words, and what it still sends adds none. Its end is
    /// the heard end, so a question it never played isn't taken as asked. One
    /// cut before any word is passed over, as an answer with no word is.
    #[test]
    fn an_answer_cut_is_kept_as_what_was_heard_of_it() {
        let mut thread = Thread::new();
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let within = Duration::from_secs(300);
        thread.person("Tell me a story.", true, at(1));
        thread.lamp(1, "Once a keeper lived by the sea. ", at(2));
        thread.lamp(1, "Do you want to hear more? ", at(2));
        assert!(thread.lamp_asked(2), "as sent, it asked");
        thread.heard_only(1, "Once a keeper lived—");
        thread.lamp(1, "One winter a ship came. ", at(3));
        let recent = thread.recent(at(4), within, 6);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].person, "Tell me a story.");
        assert_eq!(recent[0].lamp, "Once a keeper lived—");
        assert!(!thread.lamp_asked(2), "its question was never played");
        // The next answer is kept whole.
        thread.person("Who was he?", true, at(5));
        thread.lamp(2, "A keeper named Tom. ", at(6));
        thread.lamp(2, "He was kind. ", at(6));
        let recent = thread.recent(at(7), within, 6);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[1].lamp, "A keeper named Tom. He was kind.");
        // Nothing played of it: the person's words are still told, the
        // lamp's part a dash (the reviewer's P2 on #2145), and handed on.
        thread.heard_only(2, "");
        let recent = thread.recent(at(7), within, 6);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[1].person, "Who was he?");
        assert_eq!(recent[1].lamp, "—");
        assert!(!thread.lamp_asked(3));
        let finished = thread.finished(2, at(7));
        assert!(
            finished
                .iter()
                .any(|exchange| exchange.request == 2 && exchange.heard == "Who was he?"),
            "{:?}",
            finished
                .iter()
                .map(|exchange| exchange.request)
                .collect::<Vec<_>>()
        );
        // Nothing for an answer the thread doesn't hold.
        thread.heard_only(9, "Anything.");
        assert_eq!(thread.recent(at(7), within, 6).len(), 2);
        // The thread file keeps it, and a voice started again reads it back
        // as cut before any word: told, and recalled into a new session (the
        // reviewer's P3 on #2145).
        let wall = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let text = thread.save(at(7), wall).expect("changed");
        let read = Thread::load(&text, at(7), wall);
        let recent = read.recent(at(7), within, 6);
        assert_eq!(recent.len(), 2, "{text}");
        assert_eq!(
            (recent[1].person.as_str(), recent[1].lamp.as_str()),
            ("Who was he?", "—")
        );
        let recalled = read.recall(at(7), ROOM).unwrap();
        assert!(recalled.contains("Person: Who was he?"), "{recalled}");
    }

    /// #1505: the thread file as a later build writes it, with a field this
    /// build doesn't know in an exchange (Tests' forward-read test, made
    /// plain by `el/thread-forward`). The same file without it is told
    /// again. With it, `Kept` stays strict, so it is set aside: the thread
    /// starts empty, `thread.json.unreadable-<ms>` beside it holds the later
    /// build's text byte for byte, and one line says so.
    #[test]
    fn a_thread_from_a_later_build_is_set_aside_and_kept() {
        let mut thread = Thread::new();
        let start = Instant::now() + Duration::from_secs(3_600);
        let wall = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        thread.person("My name is Sam.", true, start);
        thread.lamp(3, "Nice to meet you, Sam.", start + Duration::from_secs(1));
        let text = thread
            .save(
                start + Duration::from_secs(2),
                wall + Duration::from_secs(2),
            )
            .unwrap();
        let other = Instant::now() + Duration::from_secs(9_000);
        let later = wall + Duration::from_secs(10);
        // Today's own file reads back.
        assert!(
            Thread::load(&text, other, later)
                .recall(other, ROOM)
                .is_some()
        );
        let mut kept: serde_json::Value = serde_json::from_str(&text).unwrap();
        kept[0]["a_field_from_a_later_build"] = serde_json::json!(true);
        let text = serde_json::to_string(&kept).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-thread-later-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("thread.json");
        std::fs::write(&path, &text).unwrap();
        let mut reader = Thread::new();
        assert!(!reader.reload(&path, other, later), "set aside, not taken");
        assert_eq!(reader.recall(other, ROOM), None, "the thread starts empty");
        assert!(!path.exists());
        let aside: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect();
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert!(
            aside[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("thread.json.unreadable-")
        );
        assert_eq!(std::fs::read_to_string(&aside[0]).unwrap(), text);
        let said = SET_ASIDE_SAID
            .lock()
            .unwrap()
            .iter()
            .filter(|said| **said == path)
            .count();
        assert_eq!(said, 1, "one line for it");
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// The thread files kept as fixtures, one for each change of the
    /// file's shape, each written by its day's build
    /// ([`write_a_thread_fixture`]). The file outlives the voice's restart
    /// (`RuntimeDirectoryPreserve=yes`), so an update's new build reads the
    /// old build's file. Every later build reads each with every exchange
    /// in it, under whose it was, the written note kept, so an update never
    /// sets the conversation aside (the build row "Updates cover the whole
    /// lamp, V3 to V5").
    #[test]
    fn each_kept_thread_file_reads_back_as_it_was_said() {
        let kept: [(&str, &str); 1] = [(
            "2026-10-21",
            include_str!("../tests/fixtures/stores/thread-2026-10-21.json"),
        )];
        // A's memory id as the file holds it, not worked out again here.
        let a = "p-ca978112ca1bbdca";
        let said = |written: Option<&str>, person: &str, lamp: &str| Recent {
            written: written.map(str::to_owned),
            person: person.to_owned(),
            lamp: lamp.to_owned(),
            room: false,
        };
        for (day, text) in kept {
            // Read a minute after its newest exchange, within the ten
            // minutes a thread is remembered.
            let newest = serde_json::from_str::<Vec<serde_json::Value>>(text)
                .unwrap()
                .iter()
                .filter_map(|exchange| exchange["at_unix_ms"].as_u64())
                .max()
                .unwrap();
            let now = Instant::now() + Duration::from_secs(3_600);
            let wall = UNIX_EPOCH + Duration::from_millis(newest + 60_000);
            let thread = Thread::read(text, now, wall).unwrap_or_else(|why| panic!("{day}: {why}"));
            let recent = |scope| thread.recent_for(scope, now, REMEMBERED_FOR, EXCHANGES);
            assert_eq!(
                recent(None),
                [said(None, "What time is it?", "Ten past nine.")],
                "{day}"
            );
            assert_eq!(
                recent(Some(a)),
                [
                    said(None, "Remind me to water the plants.", "I will, at six."),
                    said(
                        Some("The parcel came at noon."),
                        "",
                        "Your parcel came at noon."
                    ),
                ],
                "{day}"
            );
        }
    }

    /// Writes this build's thread file to the path in
    /// `LAMP_VOICE_THREAD_FIXTURE`: the room's exchange, then a known
    /// person's (A, made up) spoken one and written one. Run by hand when the
    /// file's shape changes, keeping the ones kept.
    #[test]
    #[ignore = "writes a thread fixture: run by hand with LAMP_VOICE_THREAD_FIXTURE set"]
    fn write_a_thread_fixture() {
        let target =
            std::env::var_os("LAMP_VOICE_THREAD_FIXTURE").expect("LAMP_VOICE_THREAD_FIXTURE");
        let start = Instant::now() + Duration::from_secs(3_600);
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        // 21 Oct 2026, 02:40 UTC.
        let wall = UNIX_EPOCH + Duration::from_secs(1_792_550_400);
        let mut thread = Thread::new();
        thread.person("What time is it?", true, at(0));
        thread.lamp(1, "Ten past nine.", at(1));
        let a = crate::remember::person_id("A");
        assert_eq!(a, "p-ca978112ca1bbdca");
        thread.at_lamp(Some(a));
        thread.person("Remind me to water the plants.", true, at(10));
        thread.lamp(2, "I will, at six.", at(11));
        thread.written(3, "The parcel came at noon.");
        thread.lamp(3, "Your parcel came at noon.", at(20));
        std::fs::write(target, thread.save(at(30), wall).unwrap()).unwrap();
    }

    #[test]
    fn a_saved_thread_is_told_again_by_the_process_that_reads_it_back() {
        let mut thread = Thread::new();
        let start = Instant::now() + Duration::from_secs(3_600);
        let wall = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        assert_eq!(
            thread.save(at(0), wall),
            None,
            "nothing said, nothing to save"
        );
        thread.person("My name is Sam.", true, at(1));
        thread.lamp(3, "Nice to meet you, Sam.", at(2));
        thread.person("Are you coming to lunch?", true, at(3));
        thread.lamp(4, "...", at(4));
        thread.person("What is my name?", true, at(100));
        thread.lamp(5, "Sam!", at(101));
        let text = thread
            .save(at(102), wall + Duration::from_secs(102))
            .unwrap();
        assert_eq!(thread.save(at(103), wall), None, "unchanged since");
        assert!(!text.contains("lunch") && text.contains("Sam!"));
        // Another process, a few seconds later by the wall clock, with a
        // clock of its own that started somewhere else.
        let other = Instant::now() + Duration::from_secs(9_000);
        let later = wall + Duration::from_secs(110);
        let read = Thread::load(&text, other, later);
        let told = read.recall(other, ROOM).unwrap();
        assert!(
            told.ends_with(
                "Person: My name is Sam.\nYou: Nice to meet you, Sam.\nPerson: What is my name?\nYou: Sam!\n"
            ),
            "{told}"
        );
        // Its own request numbers never run into what it read.
        let mut read = read;
        read.person("And yours?", true, other);
        read.lamp(1, "Lamp.", other);
        assert_eq!(
            read.recall(other, ROOM).unwrap().matches("You: ").count(),
            3
        );
        // Eleven minutes on, the first exchange is not read back; an hour
        // on, nothing is. Nor from a file that says it is from the future,
        // or that is not a thread at all.
        let aged = Thread::load(&text, other, wall + Duration::from_secs(2 + 660));
        assert_eq!(
            aged.recall(other, ROOM).unwrap().matches("You: ").count(),
            1
        );
        assert_eq!(
            Thread::load(&text, other, wall + Duration::from_secs(4_000)).recall(other, ROOM),
            None
        );
        assert_eq!(Thread::load(&text, other, wall).recall(other, ROOM), None);
        for junk in ["", "{}", "[{\"at_unix_ms\":1}]", "not json"] {
            assert_eq!(Thread::load(junk, other, later).recall(other, ROOM), None);
        }
        // The file itself: whole, private, and replaced whole.
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-thread-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("thread.json");
        write_file(&path, &text).unwrap();
        write_file(&path, "[]").unwrap();
        write_file(&path, &text).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(&directory).unwrap();
        // And its flag.
        let flags: Vec<String> = [
            FILE_FLAG,
            "/run/lampos-voice/thread.json",
            "--camera-context",
        ]
        .iter()
        .map(|flag| (*flag).to_owned())
        .collect();
        let (file, rest) = take_file(&flags).unwrap();
        assert_eq!(
            file,
            Some(std::path::PathBuf::from("/run/lampos-voice/thread.json"))
        );
        assert_eq!(rest, &flags[2..]);
        assert_eq!(take_file(&flags[2..]).unwrap(), (None, &flags[2..]));
        assert!(take_file(&flags[..1]).is_err());
    }

    #[test]
    fn finished_scope_is_the_response_scope_even_when_someone_else_arrives_before_flush() {
        let start = Instant::now();
        let mut thread = Thread::new();
        thread.seen_now(Some("p-a".into()));
        thread.person("My question", true, start);
        thread.lamp(1, "Your answer", start);
        thread.seen_now(Some("p-b".into()));
        thread.at_lamp(Some("p-b".into()));
        let done = thread.finished(1, start + Duration::from_secs(1));
        assert_eq!(done[0].whose.as_deref(), Some("p-a"));
        assert_eq!(done[0].request, 1);
    }

    #[test]
    fn a_finished_batch_preserves_room_and_both_peoples_scopes() {
        let start = Instant::now();
        let mut thread = Thread::new();
        for (request, whose) in [(1, None), (2, Some("p-a")), (3, Some("p-b"))] {
            thread.seen_now(whose.map(str::to_owned));
            thread.person("Question", true, start);
            thread.lamp(request, "Answer", start);
        }
        thread.seen_now(Some("p-c".into()));
        thread.at_lamp(Some("p-c".into()));
        let done = thread.finished(3, start);
        assert_eq!(
            done.iter()
                .map(|exchange| (exchange.request, exchange.whose.as_deref()))
                .collect::<Vec<_>>(),
            [(1, None), (2, Some("p-a")), (3, Some("p-b"))]
        );
    }

    #[test]
    fn a_finished_exchange_is_given_out_once_and_forgetting_drops_them_all() {
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let mut thread = Thread::new();
        thread.person("How do I take my coffee?", true, at(0));
        thread.lamp(1, "Black.", at(1));
        // Not over yet: another request is done, and this is the newest.
        assert!(thread.finished(0, at(2)).is_empty());
        assert_eq!(
            thread.finished(1, at(4)),
            [FinishedExchange {
                request: 1,
                age: Duration::from_secs(3),
                heard: "How do I take my coffee?".to_owned(),
                said: "Black.".to_owned(),
                whose: None,
            }]
        );
        assert!(thread.finished(1, at(5)).is_empty());
        // An answer that never completed is given out when the next begins;
        // one with no word in it never is.
        thread.lamp(2, "And", at(6));
        thread.lamp(3, "...", at(7));
        thread.lamp(4, "Yes?", at(8));
        let given: Vec<u64> = thread
            .finished(9, at(9))
            .into_iter()
            .map(|exchange| exchange.request)
            .collect();
        assert_eq!(given, [2]);
        assert_eq!(thread.finished(4, at(9)).len(), 1);
        // What a restarted service reads back was handed on by the one
        // that wrote it.
        let saved = thread.save(at(9), SystemTime::now()).unwrap();
        let mut loaded = Thread::load(&saved, at(9), SystemTime::now());
        assert!(loaded.recall(at(9), 4_000).is_some());
        assert!(loaded.finished(4, at(9)).is_empty());
        thread.forget();
        assert_eq!(thread.recall(at(9), 4_000), None);
        assert!(!thread.going_on(at(9)));
        assert_eq!(thread.save(at(9), SystemTime::now()).as_deref(), Some("[]"));
    }

    #[test]
    fn a_function_named_aloud_is_not_something_the_lamp_said() {
        let start = Instant::now();
        let mut thread = Thread::new();
        thread.person("Then I will bring an umbrella.", true, start);
        thread.lamp(7, "not_for_me", start);
        thread.lamp(7, " and more of the same answer", start);
        assert!(!thread.going_on(start));
        assert_eq!(thread.recall(start, 4_000), None);
        assert!(thread.finished(7, start).is_empty());
        // The next answer is an answer.
        thread.lamp(8, "Yes?", start);
        assert!(thread.going_on(start));
        // A whole call later in an answer is left out, its words kept
        // (lamp-4ace, 2026-10-16 17:12: `not_for_me{}`).
        thread.lamp(8, " not_for_me{}", start);
        let told = thread.recall(start, 4_000).unwrap();
        assert!(
            told.contains("Yes?") && !told.contains("not_for_me"),
            "{told}"
        );
    }

    #[test]
    fn it_forgets_what_is_old_and_holds_only_so_much() {
        let mut thread = Thread::new();
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        // Something said long before an answer is not its question.
        thread.person("What time is it?", true, at(0));
        thread.lamp(1, "It is a fine day.", at(30));
        assert_eq!(
            thread.recall(at(31), ROOM).unwrap().lines().last(),
            Some("You: It is a fine day.")
        );
        assert!(!thread.recall(at(31), ROOM).unwrap().contains("What time"));
        // Ten minutes on, nothing of it is told.
        assert_eq!(thread.recall(at(30 + 601), ROOM), None);
        // Only the last EXCHANGES, each side cut to its length.
        for request in 10..26u64 {
            thread.person(&"word ".repeat(200), true, at(700 + request));
            thread.lamp(request, &"answer ".repeat(200), at(701 + request));
        }
        // Unbounded room: twelve at their longest are past ROOM.
        let recalled = thread.recall(at(730), usize::MAX).unwrap();
        assert_eq!(recalled.matches("Person: ").count(), EXCHANGES);
        assert_eq!(recalled.matches("You: ").count(), EXCHANGES);
        assert!(recalled.len() < EXCHANGES * (PERSON_BYTES + LAMP_BYTES + 20) + 400);
        assert!(!recalled.contains('\0') && !recalled.contains("  "));
        // With less room, the latest that fit, still in order; with none, nothing.
        let whole = recalled.len();
        let fewer = thread.recall(at(730), whole - 1).unwrap();
        assert_eq!(fewer.matches("You: ").count(), EXCHANGES - 1);
        assert!(recalled.ends_with(&fewer[GOING_ON.len()..]));
        assert_eq!(thread.recall(at(730), GOING_ON.len() + 10), None);
    }

    /// Second answers while Gemini is away and writes the thread; Gemini's
    /// next session is told that talk. A forgetting after Second wrote keeps
    /// the file from bringing it back.
    #[test]
    fn a_thread_another_route_wrote_while_this_one_was_away_is_told_again() {
        use lamp_gemini::{Credential, GOOGLE_ENDPOINT};
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-thread-reload-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("thread.json");
        // File timestamps can be a few ms coarser than the wall clock.
        let tick = || std::thread::sleep(Duration::from_millis(30));
        let now = Instant::now();
        let wall = SystemTime::now();
        // Gemini talks, and writes its file.
        let mut gemini = Thread::new();
        gemini.person("My name is Sam.", true, now);
        gemini.lamp(3, "Nice to meet you, Sam.", now);
        write_file(&path, &gemini.save(now, wall).unwrap()).unwrap();
        gemini.wrote(&path);
        assert!(!gemini.reload(&path, now, wall), "its own file is not news");
        // The outage: Second reads the file, talks, and writes it whole.
        tick();
        let mut second = Thread::load(&std::fs::read_to_string(&path).unwrap(), now, wall);
        second.person("Is it raining?", true, now);
        second.lamp(7, "Not right now.", now);
        write_file(&path, &second.save(now, wall).unwrap()).unwrap();
        // Gemini returns: its first session is told the outage's talk.
        let config = SessionConfig::new(GOOGLE_ENDPOINT, Credential::api_key("test-only").unwrap())
            .unwrap()
            .instruction("You are a lamp.")
            .unwrap();
        let recalling = Recalling::new(config).file(Some(path.clone()));
        assert!(gemini.reload(&path, now, wall));
        let told = recalling.recalled(&gemini, now, None);
        assert!(
            told.instruction_text().ends_with(
                "Person: My name is Sam.\nYou: Nice to meet you, Sam.\nPerson: Is it raining?\nYou: Not right now.\n"
            ),
            "{}",
            told.instruction_text()
        );
        assert!(!gemini.reload(&path, now, wall), "once");
        // Second writes again during another outage, then something is
        // forgotten before Gemini connects: the file is not taken back.
        tick();
        second.person("Remember my code is 1234.", true, now);
        second.lamp(8, "Kept.", now);
        write_file(&path, &second.save(now, wall).unwrap()).unwrap();
        tick();
        gemini.forget();
        assert!(!gemini.reload(&path, now, wall));
        assert_eq!(gemini.recall(now, ROOM), None);
        // A file written after the forgetting is.
        tick();
        let mut after = Thread::new();
        after.person("Hello again.", true, now);
        after.lamp(9, "Hello.", now);
        write_file(&path, &after.save(now, wall).unwrap()).unwrap();
        assert!(gemini.reload(&path, now, wall));
        assert!(!gemini.recall(now, ROOM).unwrap().contains("1234"));
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_new_session_is_told_the_thread_and_an_empty_one_changes_nothing() {
        use lamp_gemini::{Credential, GOOGLE_ENDPOINT};
        let config = SessionConfig::new(GOOGLE_ENDPOINT, Credential::api_key("test-only").unwrap())
            .unwrap()
            .instruction("You are a lamp.")
            .unwrap();
        let recalling = Recalling::new(config);
        let mut thread = Thread::new();
        let now = Instant::now();
        assert_eq!(
            recalling.recalled(&thread, now, None).instruction_text(),
            "You are a lamp."
        );
        // The time alone, and before the thread when there is one.
        assert_eq!(
            recalling
                .recalled(&thread, now, Some("\n\nThe time\nNoon."))
                .instruction_text(),
            "You are a lamp.\n\nThe time\nNoon."
        );
        thread.person("My name is Sam.", true, now);
        thread.lamp(3, "Nice to meet you, Sam.", now);
        let both = recalling.recalled(&thread, now, Some("\n\nThe time\nNoon."));
        assert!(
            both.instruction_text()
                .starts_with("You are a lamp.\n\nThe time\nNoon.\n\nA moment ago\n")
        );
        // A thread with no room left leaves the time; a time too long, nothing.
        let long = "x".repeat(lamp_gemini::MAX_INSTRUCTION_BYTES - 200);
        let full = Recalling::new(recalling.config.clone().instruction(&long).unwrap());
        assert_eq!(
            full.recalled(&thread, now, Some("\n\nThe time\nNoon."))
                .instruction_text(),
            format!("{long}\n\nThe time\nNoon.")
        );
        assert_eq!(
            full.recalled(&thread, now, Some(&"y".repeat(300)))
                .instruction_text(),
            long
        );
        let told = recalling.recalled(&thread, now, None);
        assert!(
            told.instruction_text()
                .starts_with("You are a lamp.\n\nA moment ago\n")
        );
        assert!(
            told.instruction_text()
                .ends_with("Person: My name is Sam.\nYou: Nice to meet you, Sam.\n")
        );
    }

    /// #764: each session is one person's. Played with the eyes' presence
    /// as the worker counts it and the instruction a new session is sent.
    #[test]
    fn a_session_is_told_only_the_thread_of_the_person_it_is_opened_for() {
        use crate::known_faces::{GONE_AFTER, Presence};
        use lamp_gemini::{Credential, GOOGLE_ENDPOINT};
        let config = SessionConfig::new(GOOGLE_ENDPOINT, Credential::api_key("test-only").unwrap())
            .unwrap()
            .instruction("You are a lamp.")
            .unwrap();
        let recalling = Recalling::new(config);
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let (a, b) = (
            crate::remember::person_id("Minh"),
            crate::remember::person_id("Sam"),
        );
        let mut presence = Presence::default();
        let mut thread = Thread::new();
        let mut seen = |thread: &mut Thread, known: Option<&str>, now: Instant| {
            let whose = presence
                .seen(known.map(str::to_owned), now)
                .map(str::to_owned);
            thread.at_lamp(whose);
            thread.seen_now(known.map(str::to_owned));
        };
        let opened = |thread: &mut Thread, now: Instant| {
            thread.opening(now);
            recalling
                .recalled(thread, now, None)
                .instruction_text()
                .to_owned()
        };
        // The room talks first, then Minh sits down.
        thread.person("Is it raining?", true, at(0));
        thread.lamp(1, "Not yet.", at(1));
        seen(&mut thread, Some(&a), at(2));
        assert!(thread.switch_now(false, false, at(2)));
        let minh = opened(&mut thread, at(2));
        assert!(!minh.contains("raining"), "only theirs: {minh}");
        thread.person("Keep my locker code, 417.", true, at(3));
        thread.lamp(2, "Kept: 417.", at(4));
        // (b) Minh looks away for a few seconds: the same session.
        seen(&mut thread, None, at(10));
        assert!(!thread.switch_due(at(10)));
        seen(&mut thread, Some(&a), at(12));
        // (a) Minh unseen 30 s and nothing said for two minutes: a new
        // session for the room, nothing of Minh's. Unseen 30 s alone is not
        // enough while the conversation is recent.
        seen(
            &mut thread,
            None,
            at(12) + GONE_AFTER - Duration::from_secs(1),
        );
        assert!(!thread.switch_due(at(41)));
        seen(&mut thread, None, at(12) + GONE_AFTER);
        assert!(!thread.switch_now(false, false, at(42)));
        assert!(!thread.switch_due(at(4 + 119)));
        assert!(thread.switch_now(false, false, at(4 + 120)));
        let room = opened(&mut thread, at(124));
        assert!(!room.contains("417"), "{room}");
        assert!(room.contains("Not yet."), "the room's own: {room}");
        assert!(!thread.switch_due(at(124)));
        // (c) Minh, then Sam in their place: once the eyes have known Sam
        // for 10 s with Minh unseen (`SOMEONE_ELSE_AFTER`), Sam's session
        // has only Sam's.
        seen(&mut thread, Some(&a), at(130));
        opened(&mut thread, at(130));
        seen(&mut thread, Some(&b), at(131));
        assert!(!thread.switch_now(false, false, at(131)));
        seen(&mut thread, Some(&b), at(141));
        // (e) Sam speaks before the switch: deferred while the turn is open
        // or its answer is still being handed on, and the exchange is Sam's.
        thread.person("What is my locker code?", true, at(142));
        assert!(
            !thread.switch_now(true, false, at(142)) && !thread.switch_now(false, true, at(142))
        );
        thread.lamp(3, "I don't remember.", at(143));
        assert!(thread.switch_now(false, false, at(143)));
        let sam = opened(&mut thread, at(144));
        assert!(!sam.contains("417") && !sam.contains("Not yet."), "{sam}");
        assert!(sam.contains("I don't remember."), "{sam}");
        // (d) Minh comes back: their thread is there again.
        seen(&mut thread, Some(&a), at(150));
        seen(&mut thread, Some(&a), at(160));
        let back = opened(&mut thread, at(160));
        assert!(back.contains("Kept: 417."), "{back}");
        assert!(!back.contains("I don't remember.") && !back.contains("Not yet."));
        // The file keeps whose each exchange is, as a memory id only.
        let text = thread.save(at(161), SystemTime::now()).unwrap();
        assert!(text.contains(&a) && text.contains(&b) && !text.contains("Minh"));
        let mut read = Thread::load(&text, at(161), SystemTime::now());
        read.at_lamp(Some(b.clone()));
        read.opening(at(161));
        let told = read.recall(at(161), ROOM).unwrap();
        assert!(told.contains("I don't remember.") && !told.contains("417"));
        let forged = text.replace(&a, "../Minh");
        let mut read = Thread::load(&forged, at(161), SystemTime::now());
        read.at_lamp(Some("../Minh".into()));
        read.opening(at(161));
        assert_eq!(read.recall(at(161), ROOM), None);
    }

    /// The reviewer on #1100: each exchange is the person's the eyes name
    /// as it is made, the counted person's only when nobody is named; the
    /// counted person decides only whose session it is. So what a newcomer
    /// says in a known person's place, B's "My bank PIN is 2468" at 13 s,
    /// is B's whether or not B comes to count, and is never told to A: when
    /// B stays, when A is read again first, and when B leaves and A comes
    /// back much later. The file keeps it so.
    #[test]
    fn what_a_newcomer_says_is_theirs_whether_or_not_they_count() {
        use crate::known_faces::Presence;
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let (a, b) = (
            crate::remember::person_id("A"),
            crate::remember::person_id("B"),
        );
        // The eyes' fresh reading at `now`, as the worker takes it.
        let seen = |presence: &mut Presence, thread: &mut Thread, known: Option<&str>, now| {
            let whose = presence
                .seen(known.map(str::to_owned), now)
                .map(str::to_owned);
            thread.at_lamp(whose);
            thread.seen_now(known.map(str::to_owned));
        };
        let recall = |thread: &mut Thread, now| {
            thread.opening(now);
            thread.recall(now, ROOM).unwrap_or_default()
        };
        // A keeps 417; B sits in A's place at 10 s and says the PIN at 13 s,
        // answered in A's session. Then `after` runs.
        let scene = |after: &dyn Fn(&mut Presence, &mut Thread)| {
            let mut presence = Presence::default();
            let mut thread = Thread::new();
            seen(&mut presence, &mut thread, Some(&a), at(0));
            thread.opening(at(0));
            thread.person("Keep my locker code, 417.", true, at(1));
            thread.lamp(1, "Kept: 417.", at(2));
            seen(&mut presence, &mut thread, Some(&b), at(10));
            seen(&mut presence, &mut thread, Some(&b), at(13));
            thread.person("My bank PIN is 2468, keep it.", true, at(13));
            thread.lamp(2, "Kept: 2468.", at(14));
            assert!(!thread.switch_due(at(14)), "still A's session");
            after(&mut presence, &mut thread);
            thread
        };
        // B stays: B counts at 20 s, and B's session has the PIN.
        let mut thread = scene(&|presence, thread| {
            seen(presence, thread, Some(&b), at(20));
            assert!(thread.switch_due(at(20)));
            let told = recall(thread, at(20));
            assert!(told.contains("2468") && !told.contains("417"), "{told}");
            seen(presence, thread, Some(&a), at(30));
            seen(presence, thread, Some(&a), at(40));
        });
        let told = recall(&mut thread, at(40));
        assert!(told.contains("417") && !told.contains("2468"), "{told}");
        // (a) A is read again first: B never counts. A's next session.
        let mut thread = scene(&|presence, thread| {
            seen(presence, thread, Some(&a), at(16));
            assert!(!thread.switch_due(at(16)));
        });
        let told = recall(&mut thread, at(90));
        assert!(told.contains("417") && !told.contains("2468"), "{told}");
        // (b) B is read only at 10 to 14 s and leaves; A is back at 200 s.
        let mut thread = scene(&|presence, thread| {
            seen(presence, thread, None, at(15));
            seen(presence, thread, None, at(60));
            seen(presence, thread, Some(&a), at(200));
        });
        let told = recall(&mut thread, at(200));
        assert!(told.contains("417") && !told.contains("2468"), "{told}");
        // The file keeps the PIN as B's: read back, A is told none of it.
        let text = thread.save(at(201), SystemTime::now()).unwrap();
        let mut read = Thread::load(&text, at(201), SystemTime::now());
        read.at_lamp(Some(a.clone()));
        read.opening(at(201));
        assert!(!read.recall(at(201), ROOM).unwrap().contains("2468"));
        read.at_lamp(Some(b.clone()));
        read.opening(at(202));
        assert!(read.recall(at(202), ROOM).unwrap().contains("2468"));
    }

    /// What a name is kept from (`LAMP_NAMES=checked`, #1057): the turn in
    /// hand is what is being heard, or, once the lamp has begun to answer
    /// it, the newest exchange; the exchanges before it come with what the
    /// lamp said back.
    #[test]
    fn a_names_window_is_the_turn_in_hand_and_the_exchanges_before_it() {
        let now = Instant::now();
        let mut thread = Thread::new();
        for (i, (said, lamp)) in [("Hello.", "Hi!"), ("My name is A.", "Shall I call you A?")]
            .iter()
            .enumerate()
        {
            thread.person(said, true, now);
            thread.lamp(i as u64 + 1, lamp, now);
        }
        thread.person("Yes.", true, now);
        let before = vec![
            ("Hello.".to_owned(), "Hi!".to_owned()),
            ("My name is A.".to_owned(), "Shall I call you A?".to_owned()),
        ];
        let window = |before: &[(String, String)]| crate::known_faces::Said {
            this: "Yes.".into(),
            before: before.to_vec(),
        };
        assert_eq!(thread.names_window(2), window(&before));
        thread.lamp(3, "Okay, A.", now);
        assert_eq!(thread.names_window(2), window(&before));
        assert_eq!(thread.names_window(1), window(&before[1..]));
    }

    /// `LAMP_LIVE_SCOPED_SESSION`: a line admitted for someone known makes
    /// the session theirs before the count catches up, refuses only another
    /// known person's session, and stops counting two minutes on or once
    /// the camera closes.
    #[test]
    fn an_admitted_person_owns_the_next_session_for_two_minutes() {
        let now = Instant::now();
        let a = crate::remember::person_id("A");
        let b = crate::remember::person_id("B");
        let mut thread = Thread::new();
        thread.at_lamp(Some(a.clone()));
        thread.opening(now);
        assert!(thread.admitted(&b, now), "B in A's session");
        assert!(thread.switch_due(now), "the count still says A");
        assert!(thread.opening(now));
        assert_eq!(thread.opened_for(), Some(b.as_str()));
        assert!(!thread.admitted(&b, now));
        assert!(
            thread.switch_due(now + QUIET_BEFORE_ROOM),
            "back to the count"
        );
        thread.admitted_unknown();
        assert!(thread.switch_due(now));
        let mut room = Thread::new();
        room.opening(now);
        assert!(!room.admitted(&a, now), "a room session is never refused");
        assert!(room.switch_due(now), "but is opened again for them");
    }

    /// The reviewer's P2 on #1865: two known people taking turns lose one
    /// line, when the second first speaks, and then talk on in a room
    /// session, told neither's things; none of their later lines is
    /// refused or opens a session. Once one has been quiet two minutes, the
    /// session is the other's again, opened between turns.
    #[test]
    fn two_known_people_taking_turns_share_a_room_session() {
        let now = Instant::now();
        let a = crate::remember::person_id("A");
        let b = crate::remember::person_id("B");
        let mut thread = Thread::new();
        thread.at_lamp(Some(a.clone()));
        thread.opening(now);
        assert!(!thread.admitted(&a, now), "A in A's session");
        assert!(!thread.switch_due(now));
        assert!(thread.admitted(&b, now), "B's first line, in A's session");
        assert!(thread.switch_due(now));
        thread.opening(now);
        assert_eq!(thread.opened_for(), None, "the room's");
        let second = Duration::from_secs(1);
        for turn in 1..=20u32 {
            let at = now + second * turn;
            let person = if turn % 2 == 0 { &a } else { &b };
            assert!(!thread.admitted(person, at), "turn {turn} refused");
            assert!(!thread.switch_due(at), "turn {turn} reopened");
        }
        let later = now + second * 20 + QUIET_BEFORE_ROOM;
        assert!(
            !thread.admitted(&a, later),
            "a room session is never refused"
        );
        assert!(thread.switch_due(later), "B quiet two minutes: A's again");
        thread.opening(later);
        assert_eq!(thread.opened_for(), Some(a.as_str()));
    }

    /// The reviewer-lead's P2 on #4479, and #4508 (carried lines may be
    /// journalled): a held or carried exchange is never given to the running
    /// summary, journalled or not, at a batch or at the conversation's end;
    /// the skip is keyed on the held scope, never on what is journalled.
    #[test]
    fn a_held_exchange_journalled_or_not_is_never_given_to_the_summary() {
        let start = Instant::now();
        let a = crate::remember::person_id("A");
        let mut thread = Thread::new();
        for request in 1..=EXCHANGES as u64 {
            thread.person(&format!("line {request}"), true, start);
            thread.lamp_for(request, "Mm.", start, Some(&a));
            if request % 3 == 0 {
                thread.held_for(request);
            }
        }
        // Journalled, as a carried line's exchange is under #4508's switch.
        assert!(!thread.finished(EXCHANGES as u64, start).is_empty());
        let given: Vec<u64> = thread
            .summary_due(4, start + Duration::from_secs(1))
            .into_iter()
            .flat_map(|due| due.exchanges.into_iter().map(|(request, ..)| request))
            .chain(
                thread
                    .summary_rest(Some(&a), start + Duration::from_secs(2))
                    .into_iter()
                    .map(|(request, ..)| request),
            )
            .collect();
        assert!(!given.is_empty());
        assert!(given.iter().all(|request| request % 3 != 0), "{given:?}");
        assert_eq!(given.len(), EXCHANGES - EXCHANGES / 3, "{given:?}");
    }

    /// #4339: A is in a conversation going on only while the newest
    /// exchange before the line in hand is A's and began within
    /// [`GOING_ON_FOR`]; someone else's in between, a quiet, or only the
    /// line's own exchange, and they are not.
    /// #4339 (`LAMP_CONVERSATION_ROOM_LINES_TOLD`): the room's exchanges
    /// told to A's turn are those of A's conversation only: from A's first
    /// exchange in it, each within the conversation's 45 s of the next, no
    /// other person's between, within the chat's window; marked as the
    /// room's. B's exchanges never, and nothing for the room's turn.
    #[test]
    fn room_lines_told_to_a_person_are_their_conversations_only() {
        let a = crate::remember::person_id("A");
        let b = crate::remember::person_id("B");
        let t0 = Instant::now();
        let at = |seconds: u64| t0 + Duration::from_secs(seconds);
        let lamps = |recent: &[Recent]| -> Vec<(String, bool)> {
            recent
                .iter()
                .map(|said| (said.lamp.clone(), said.room))
                .collect()
        };
        let mut thread = Thread::new();
        // An earlier conversation of the room's, then a quiet past 45 s.
        thread.lamp_for(1, "Old room talk.", at(0), None);
        // A's conversation: the room's before A's first is not A's.
        thread.lamp_for(2, "Room before A.", at(100), None);
        thread.lamp_for(3, "A one.", at(110), Some(&a));
        thread.lamp_for(4, "Room in A's.", at(120), None);
        thread.lamp_for(5, "A two.", at(130), Some(&a));
        let told = thread.recent_with_room_lines(&a, at(140), REMEMBERED_FOR, EXCHANGES);
        assert_eq!(
            lamps(&told),
            [
                ("A one.".to_owned(), false),
                ("Room in A's.".to_owned(), true),
                ("A two.".to_owned(), false),
            ]
        );
        // Within the most asked: the newest.
        let two = thread.recent_with_room_lines(&a, at(140), REMEMBERED_FOR, 2);
        assert_eq!(
            lamps(&two),
            [
                ("Room in A's.".to_owned(), true),
                ("A two.".to_owned(), false)
            ]
        );
        // Outside the window: none of it.
        assert!(
            thread
                .recent_with_room_lines(&a, at(140), Duration::from_secs(5), EXCHANGES)
                .is_empty()
        );
        // B's turn is told neither A's nor A's conversation's room lines.
        assert!(
            thread
                .recent_with_room_lines(&b, at(140), REMEMBERED_FOR, EXCHANGES)
                .is_empty()
        );
        // The room's turn is told the room's alone, never A's.
        assert!(
            thread
                .recent_for(None, at(140), REMEMBERED_FOR, EXCHANGES)
                .iter()
                .all(|said| !said.lamp.starts_with('A'))
        );
        // A quiet past 45 s: a new conversation, the room's line after it
        // not A's.
        thread.lamp_for(6, "Room later.", at(200), None);
        let later = thread.recent_with_room_lines(&a, at(205), REMEMBERED_FOR, EXCHANGES);
        assert_eq!(
            lamps(&later),
            [("A one.".to_owned(), false), ("A two.".to_owned(), false)],
            "another conversation's room line is not told"
        );
        // Another person's exchange between ends A's conversation.
        thread.lamp_for(7, "A three.", at(210), Some(&a));
        thread.lamp_for(8, "B one.", at(215), Some(&b));
        thread.lamp_for(9, "Room after B.", at(220), None);
        thread.lamp_for(10, "A four.", at(225), Some(&a));
        let after_b = thread.recent_with_room_lines(&a, at(230), REMEMBERED_FOR, EXCHANGES);
        assert!(
            after_b.iter().all(|said| !said.room),
            "a room line after another person's is not A's conversation's: {after_b:?}"
        );
        assert!(after_b.iter().all(|said| said.lamp != "B one."));
        // A forget drops the room's lines too.
        thread.forget();
        assert!(
            thread
                .recent_with_room_lines(&a, at(230), REMEMBERED_FOR, EXCHANGES)
                .is_empty()
        );
    }

    #[test]
    fn going_on_with_needs_their_own_newest_exchange_within_the_conversation() {
        let now = Instant::now();
        let a = crate::remember::person_id("A");
        let mut thread = Thread::new();
        assert_eq!(thread.going_on_with(2, now), None, "no exchange yet");
        thread.lamp_for(1, "Floral or woody?", now, Some(&a));
        let soon = now + Duration::from_secs(10);
        assert_eq!(thread.going_on_with(2, soon), Some(a.clone()));
        assert_eq!(
            thread.going_on_with(2, now + GOING_ON_FOR + Duration::from_secs(1)),
            None,
            "a quiet past the conversation's 45 s"
        );
        // The line's own exchange, once begun, is not the one before it.
        assert_eq!(thread.going_on_with(1, now), None);
        // The room's exchange came after A's: someone else's in between.
        thread.lamp_for(2, "Hello there.", now + Duration::from_secs(5), None);
        assert_eq!(thread.going_on_with(3, soon), None);
    }

    #[test]
    fn scoped_answer_keeps_its_admitted_person_after_the_face_changes() {
        let now = Instant::now();
        let a = crate::remember::person_id("A");
        let b = crate::remember::person_id("B");
        let mut thread = Thread::new();
        thread.at_lamp(Some(a.clone()));
        thread.opening(now);
        thread.person("A question", true, now);
        thread.at_lamp(Some(b.clone()));
        thread.lamp_for(1, "A private answer", now, Some(&a));
        assert_eq!(thread.recent_for(Some(&a), now, REMEMBERED_FOR, 6).len(), 1);
        assert!(
            thread
                .recent_for(Some(&b), now, REMEMBERED_FOR, 6)
                .is_empty()
        );
        thread.written_for(2, "room note", None);
        thread.lamp_for(2, "room answer", now, None);
        let room = thread.recent_for(None, now, REMEMBERED_FOR, 6);
        assert_eq!(room.len(), 1);
        assert_eq!(room[0].written.as_deref(), Some("room note"));
        assert!(
            thread
                .recent_for(Some(&b), now, REMEMBERED_FOR, 6)
                .is_empty()
        );
    }

    #[test]
    fn explicit_history_scope_never_uses_the_sticky_session_owner() {
        let now = Instant::now();
        let a = crate::remember::person_id("A");
        let b = crate::remember::person_id("B");
        let mut thread = Thread::new();
        thread.at_lamp(Some(a.clone()));
        thread.opening(now);
        thread.person("private marker A", true, now);
        thread.lamp(1, "private reply A", now);
        for scope in [Some(b.as_str()), None] {
            assert!(thread.recent_for(scope, now, REMEMBERED_FOR, 6).is_empty());
            assert!(thread.recall_for(scope, now, 4096).is_none());
        }
        assert!(
            thread
                .recall_for(Some(&a), now, 4096)
                .unwrap()
                .contains("private marker A")
        );
        assert_eq!(thread.opened_for(), Some(a.as_str()));
        thread.forget();
        assert!(
            thread
                .recent_for(Some(&a), now, REMEMBERED_FOR, 6)
                .is_empty()
        );
        assert!(thread.recall_for(Some(&a), now, 4096).is_none());
    }

    /// The exchanges a route gives a model as messages: the person's only,
    /// the newest few, and none once forgotten (first builder's reviewer on
    /// #778: A, forget, B, A).
    #[test]
    fn recent_exchanges_are_the_persons_own_and_none_after_a_forgetting() {
        let now = Instant::now();
        let minh = Some(crate::remember::person_id("Minh"));
        let sam = Some(crate::remember::person_id("Sam"));
        let mut thread = Thread::new();
        let open = |thread: &mut Thread, whose: &Option<String>| {
            thread.at_lamp(whose.clone());
            thread.opening(now);
        };
        open(&mut thread, &minh);
        for (i, said) in ["one", "two", "three"].iter().enumerate() {
            thread.person(said, true, now);
            thread.lamp(i as u64 + 1, &format!("heard {said}"), now);
        }
        let said: Vec<(String, String)> = thread
            .recent(now, Duration::from_secs(300), 2)
            .into_iter()
            .map(|r| (r.person, r.lamp))
            .collect();
        assert_eq!(
            said,
            [
                ("two".to_owned(), "heard two".to_owned()),
                ("three".to_owned(), "heard three".to_owned())
            ]
        );
        open(&mut thread, &sam);
        assert!(thread.recent(now, Duration::from_secs(300), 6).is_empty());
        open(&mut thread, &minh);
        assert_eq!(thread.recent(now, Duration::from_secs(300), 6).len(), 3);
        thread.forget();
        open(&mut thread, &sam);
        open(&mut thread, &minh);
        assert!(thread.recent(now, Duration::from_secs(300), 6).is_empty());
    }

    /// First builder's reviewer on #778: a written request retired before
    /// its answer leaves no note for the next request, whoever's it is; and
    /// a forgetting drops a note still waiting.
    #[test]
    fn a_written_note_is_its_own_requests_and_goes_with_a_retirement_or_a_forgetting() {
        let now = Instant::now();
        let minh = Some(crate::remember::person_id("Minh"));
        let sam = Some(crate::remember::person_id("Sam"));
        let open = |thread: &mut Thread, whose: &Option<String>| {
            thread.at_lamp(whose.clone());
            thread.opening(now);
        };
        let notes = |thread: &Thread| {
            thread
                .exchanges
                .iter()
                .filter_map(|e| e.written.clone())
                .collect::<Vec<_>>()
        };
        // Minh's written request 1 is retired before its chat; Sam's spoken
        // request 2 is answered.
        let mut thread = Thread::new();
        open(&mut thread, &minh);
        thread.written(1, "Tell them the code word is ORCHID.");
        open(&mut thread, &sam);
        thread.person("Hello.", true, now);
        thread.lamp(2, "Hi Sam.", now);
        assert!(notes(&thread).is_empty());
        // A note waiting, then a forgetting, then an answer.
        let mut thread = Thread::new();
        open(&mut thread, &minh);
        thread.written(3, "Tell them the code word is ORCHID.");
        thread.forget();
        thread.lamp(3, "Done.", now);
        assert!(notes(&thread).is_empty());
        // The note of a later request waits for its own answer.
        let mut thread = Thread::new();
        thread.written(5, "Say the time.");
        thread.lamp(4, "Earlier answer.", now);
        thread.lamp(5, "It is noon.", now);
        assert_eq!(notes(&thread), ["Say the time."]);
    }

    /// First builder's reviewer on #778: a request the lamp was given in
    /// writing stays with its answer, as a note and not as anyone's words,
    /// through the file too, so the next turn has it (ORCHID).
    #[test]
    fn a_written_request_is_kept_with_its_answer_as_a_note() {
        let now = Instant::now();
        let wall = SystemTime::now();
        let mut thread = Thread::new();
        thread.written(1, "Tell them the code word is ORCHID.");
        thread.lamp(1, "The code word is ORCHID.", now);
        let recent = thread.recent(now, Duration::from_secs(300), 6);
        assert_eq!(
            recent,
            [Recent {
                written: Some("Tell them the code word is ORCHID.".into()),
                person: String::new(),
                lamp: "The code word is ORCHID.".into(),
                room: false,
            }]
        );
        // The note is the written turn's alone: a spoken one after it has none.
        thread.person("What was it again?", true, now);
        thread.lamp(2, "ORCHID.", now);
        let read = Thread::load(&thread.save(now, wall).unwrap(), now, wall);
        let recent = read.recent(now, Duration::from_secs(300), 6);
        assert_eq!(
            recent[0].written.as_deref(),
            Some("Tell them the code word is ORCHID.")
        );
        assert_eq!(recent[1].written, None);
        assert_eq!(recent[1].person, "What was it again?");
        // A Live session's recall is as before: no note is read to it.
        assert!(!read.recall(now, ROOM).unwrap().contains("Tell them"));
    }

    /// #1505: a thread file this build cannot read (a later build's field,
    /// or no thread file at all) is set aside beside itself, renamed and
    /// kept, never read as empty and forgotten; the thread starts empty. A
    /// file it can read is left where it is.
    #[test]
    fn an_unreadable_thread_file_is_set_aside_and_the_thread_starts_empty() {
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-thread-aside-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("thread.json");
        let now = Instant::now();
        let wall = SystemTime::now();
        let wall_ms = wall.duration_since(UNIX_EPOCH).unwrap().as_millis();
        let later =
            format!(r#"[{{"at_unix_ms":{wall_ms},"person":"Hello.","lamp":"Hi.","later":true}}]"#);
        for text in [later.as_str(), "not json"] {
            std::fs::write(&path, text).unwrap();
            let mut thread = Thread::new();
            assert!(
                !thread.reload(&path, now, wall),
                "{text}: set aside, not taken"
            );
            assert_eq!(thread.recall(now, ROOM), None, "{text}");
            assert!(
                !path.exists(),
                "{text}: set aside, not left to be read again"
            );
            let kept: Vec<_> = std::fs::read_dir(&directory)
                .unwrap()
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .collect();
            assert_eq!(kept.len(), 1, "{text}: {kept:?}");
            let name = kept[0].file_name().unwrap().to_string_lossy().into_owned();
            assert!(name.starts_with("thread.json.unreadable-"), "{name}");
            assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), text);
            std::fs::remove_file(&kept[0]).unwrap();
        }
        // A file this build reads is read, and stays.
        let mut live = Thread::new();
        live.opening(now);
        live.person("Hello.", true, now);
        live.lamp(1, "Hi.", now);
        write_file(&path, &live.save(now, wall).unwrap()).unwrap();
        let mut other = Thread::new();
        assert!(other.reload(&path, now, wall));
        assert!(other.recall(now, ROOM).is_some());
        assert!(path.exists());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// First builder on #778: a person's exchange made on the second route
    /// comes back to the Live route under the same `whose`, and only to
    /// that person's session; who is at the lamp and whom the Live session
    /// was opened for are not taken from the file.
    #[test]
    fn an_exchange_on_the_second_route_comes_back_to_the_live_route_as_the_same_persons() {
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-thread-whose-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("thread.json");
        let minh = crate::remember::person_id("Minh");
        let sam = crate::remember::person_id("Sam");
        let now = Instant::now();
        let wall = SystemTime::now();
        // The Live route, with Minh at the lamp, writes its file first.
        let mut live = Thread::new();
        live.at_lamp(Some(minh.clone()));
        live.opening(now);
        live.person("Hello.", true, now);
        live.lamp(1, "Hi Minh.", now);
        write_file(&path, &live.save(now, wall).unwrap()).unwrap();
        live.wrote(&path);
        std::thread::sleep(Duration::from_millis(30));
        // The second route, its turn opened for Minh too, talks and writes.
        let mut second = Thread::load(&std::fs::read_to_string(&path).unwrap(), now, wall);
        second.at_lamp(Some(minh.clone()));
        second.opening(now);
        second.person("My locker is 417.", true, now);
        second.lamp(7, "Kept: 417.", now);
        write_file(&path, &second.save(now, wall).unwrap()).unwrap();
        // Back on the Live route: Minh's session is told it, Sam's is not.
        assert!(live.reload(&path, now, wall));
        assert_eq!(live.opened_for(), Some(minh.as_str()));
        let told = live.recall(now, ROOM).unwrap();
        assert!(
            told.contains("Kept: 417.") && told.contains("Hi Minh."),
            "{told}"
        );
        live.at_lamp(Some(sam));
        live.opening(now);
        assert!(
            live.recall(now, ROOM)
                .is_none_or(|told| !told.contains("417"))
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    /// First builder on #775: a known person who keeps talking while the
    /// eyes do not see them keeps their session; it becomes the room's only
    /// after two minutes with nothing said.
    #[test]
    fn a_known_person_talking_unseen_keeps_their_session_until_two_quiet_minutes() {
        use crate::known_faces::Presence;
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let minh = crate::remember::person_id("Minh");
        let mut presence = Presence::default();
        let mut thread = Thread::new();
        let mut seen = |thread: &mut Thread, known: Option<&str>, now: Instant| {
            thread.at_lamp(
                presence
                    .seen(known.map(str::to_owned), now)
                    .map(str::to_owned),
            );
        };
        seen(&mut thread, Some(&minh), at(0));
        thread.opening(at(0));
        // Unseen from 0 s; at 40 s the eyes have counted them gone, but
        // they talk at 20 s and 50 s.
        for second in 1..=170 {
            seen(&mut thread, None, at(second));
            if second == 20 || second == 50 {
                thread.person("And then?", true, at(second));
                thread.lamp(second, "Then the bus came.", at(second));
            }
            assert_eq!(
                thread.switch_now(false, false, at(second)),
                second >= 50 + 120,
                "{second} s"
            );
        }
        // What was said while unseen is Minh's, and so is the session.
        assert!(
            thread
                .exchanges
                .iter()
                .all(|e| e.whose.as_ref() == Some(&minh))
        );
        assert_eq!(thread.opened_for(), Some(minh.as_str()));
        assert!(thread.opening(at(170)));
        assert_eq!(thread.opened_for(), None);
        assert_eq!(thread.recall(at(170), ROOM), None);
    }

    /// With nobody ever known, a thread is what it always was: the room's,
    /// never switched, saved as before, EXCHANGES exchanges.
    #[test]
    fn with_nobody_ever_known_nothing_changes() {
        let mut thread = Thread::new();
        let start = Instant::now();
        assert!(!thread.switch_due(start) && !thread.switch_now(false, false, start));
        assert!(!thread.opening(start));
        for request in 0..(EXCHANGES as u64 + 4) {
            thread.person("hello", true, start);
            thread.lamp(request, "hi", start);
        }
        assert_eq!(thread.exchanges.len(), EXCHANGES);
        let text = thread.save(start, SystemTime::now()).unwrap();
        assert!(!text.contains("whose"), "{text}");
        assert!(!thread.opening(start + QUIET_BEFORE_ROOM));
    }

    /// Each person's thread is bounded, and so is how many are kept.
    #[test]
    fn each_persons_thread_holds_only_so_much() {
        let mut thread = Thread::new();
        let start = Instant::now();
        let mut request = 0;
        for person in 0..6 {
            let whose = format!("p-{person}");
            thread.at_lamp(Some(whose.clone()));
            for _ in 0..EXCHANGES + 2 {
                request += 1;
                thread.lamp(request, "hi", start);
            }
            let theirs = thread
                .exchanges
                .iter()
                .filter(|e| e.whose.as_deref() == Some(whose.as_str()))
                .count();
            assert_eq!(theirs, EXCHANGES);
        }
        assert_eq!(thread.exchanges.len(), EXCHANGES * PEOPLE);
    }

    /// lamp-52e6's recorded starts (#705, 17 Oct): the first session was
    /// opened before the eyes were read, so for the room, and the first
    /// reading of a known face renewed it 0 s after it was ready. Told the
    /// eyes first, it is theirs, and the same reading renews nothing.
    #[test]
    fn the_first_session_is_for_whom_the_eyes_name_at_start() {
        use crate::known_faces::Presence;
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let read = |presence: &mut Presence, thread: &mut Thread, now| {
            let whose = presence.seen(Some("Ann".into()), now);
            thread.at_lamp(whose.map(crate::remember::person_id));
        };
        let mut before = Thread::new();
        before.opening(at(0));
        read(&mut Presence::default(), &mut before, at(0));
        assert!(
            before.switch_now(false, false, at(0)),
            "the recorded renewal"
        );

        let mut thread = Thread::new();
        thread.at_start(Some("Ann"));
        thread.opening(at(0));
        let ann = crate::remember::person_id("Ann");
        assert_eq!(thread.opened_for(), Some(ann.as_str()));
        let mut presence = Presence::default();
        for second in [0, 1, 30] {
            read(&mut presence, &mut thread, at(second));
            assert!(!thread.switch_now(false, false, at(second)), "{second} s");
        }
        // The count starts from the start's reading (the reviewer on
        // #1388): a stale file or another name at the loop's first reading
        // renews nothing, where an empty count would at once.
        for first in [None, Some("Bob")] {
            let mut thread = Thread::new();
            thread.at_start(Some("Ann"));
            thread.opening(at(0));
            let mut seeded = Presence::default();
            seeded.seen(Some("Ann".into()), at(0));
            for (presence, renews) in [(&mut seeded, false), (&mut Presence::default(), true)] {
                let whose = presence.seen(first.map(str::to_owned), at(1));
                thread.at_lamp(whose.map(crate::remember::person_id));
                assert_eq!(thread.switch_now(false, false, at(1)), renews, "{first:?}");
            }
        }
        // Nobody named at start: as before, the room's.
        let mut empty = Thread::new();
        empty.at_start(None);
        assert!(!empty.opening(at(0)) && empty.opened_for().is_none());
    }
}
