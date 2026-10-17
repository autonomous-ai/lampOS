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
const EXCHANGES: usize = 8;
/// Whose threads are kept at once, the room's among them: past this the
/// oldest exchanges go, whoever's they were.
const PEOPLE: usize = 4;
const PERSON_BYTES: usize = 300;
const LAMP_BYTES: usize = 400;
/// What a person said counts as the question to an answer that begins within
/// this long of it.
const ASKED_WITHIN: Duration = Duration::from_secs(20);
/// An exchange older than this is not told to a new session: whoever it was
/// with has probably gone.
const REMEMBERED_FOR: Duration = Duration::from_secs(600);
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
const GOING_ON_FOR: Duration = Duration::from_secs(45);
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
    /// Handed on to the lamp's memory, or not for it to have.
    journalled: bool,
    /// The memory id of the known person it was with; `None`: the room's.
    whose: Option<String>,
    /// The note the lamp was given in writing for it, when it began the
    /// turn itself (a written request): not words anyone said.
    written: Option<String>,
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
    calls: crate::not_speech::CallsLeftOut,
    /// Who is at the lamp now, as the worker counts it: whose session it
    /// is, and whose a new exchange is when the eyes name nobody.
    whose: Option<String>,
    /// Whom the eyes name at the lamp at this moment, fresh: whose a new
    /// exchange is ([`Self::seen_now`]).
    named: Option<String>,
    /// Whom the newest session was opened for: the only exchanges it is told.
    opened_for: Option<String>,
    /// A written request not yet answered ([`Self::written`]).
    note: Option<Pending>,
    /// How new a thread file this holds: the modification time of the file
    /// last read or written, or the moment of the last forgetting, whichever
    /// is later. A file no newer than this is not read again.
    held: Option<SystemTime>,
}

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
#[derive(Debug, Clone, PartialEq)]
pub struct Recent {
    /// The note of a request the lamp was given in writing, if it was one.
    pub written: Option<String>,
    /// What the person said; empty for none.
    pub person: String,
    pub lamp: String,
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

impl Thread {
    pub const fn new() -> Self {
        Self {
            exchanges: VecDeque::new(),
            heard: String::new(),
            heard_at: None,
            heard_done: false,
            changed: false,
            unsaid: None,
            calls: crate::not_speech::CallsLeftOut::new(),
            whose: None,
            named: None,
            opened_for: None,
            note: None,
            held: None,
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
        let id = known.map(crate::remember::person_id);
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
        // Who is at the lamp and whom the session was opened for are this
        // process's own; each exchange keeps its `whose` through the file.
        let (whose, named, opened_for, note) = (
            self.whose.take(),
            self.named.take(),
            self.opened_for.take(),
            self.note.take(),
        );
        *self = Self::load(&text, now, wall);
        self.whose = whose;
        self.named = named;
        self.opened_for = opened_for;
        self.note = note;
        self.held = Some(at);
        true
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
                    && exchange.lamp.chars().any(char::is_alphanumeric)
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
        let mut thread = Self::new();
        let (Ok(kept), Ok(wall)) = (
            serde_json::from_str::<Vec<Kept>>(text),
            wall.duration_since(UNIX_EPOCH),
        ) else {
            return thread;
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
                lamp,
                // The process that wrote the file handed it on.
                journalled: true,
                whose: kept.whose,
                written: kept.written.map(|text| {
                    let mut written = String::new();
                    add(&mut written, &text, PERSON_BYTES);
                    written
                }),
            });
        }
        thread
    }

    /// A piece of what the provider heard a person say.
    pub fn person(&mut self, text: &str, finished: bool, now: Instant) {
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
        self.heard_at = Some(now);
        self.heard_done = finished;
    }

    /// The note of written request `request`: kept with the answer to that
    /// request alone, for the person it is answered for, apart from what
    /// anyone said. An answer to any later request drops it unused (the
    /// request was retired or failed), and so does a forgetting.
    pub fn written(&mut self, request: u64, text: &str) {
        let mut note = String::new();
        add(&mut note, text, PERSON_BYTES);
        self.note = Some(Pending {
            request,
            whose: self.whose.clone().or_else(|| self.opened_for.clone()),
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
        // The name of a function said where it should have been called is
        // not something the lamp said: neither that piece nor what follows
        // it in the same answer is kept, told again or taken for a
        // conversation going on.
        if self.unsaid == Some(request) {
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
            // Each person's last eight; with nobody ever known, the room's
            // eight are all there is. The person the eyes name now; nobody
            // named, the one counted; nobody known while a person's session
            // goes on: theirs.
            let whose = self
                .named
                .clone()
                .or_else(|| self.whose.clone())
                .or_else(|| self.opened_for.clone());
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
                journalled: false,
                whose,
                written,
            });
        }
        if let Some(last) = self.exchanges.back_mut() {
            add(&mut last.lamp, text, LAMP_BYTES);
            self.changed = true;
        }
    }

    /// The exchanges that are over and were not given out before, for the
    /// lamp's memory ([`crate::remember`]): every one but the newest, and
    /// the newest too when `done` is its request. Each with its request,
    /// how long ago it began, what the person said and what the lamp did.
    /// An answer with no word in it is passed over: it is not something
    /// the lamp said.
    pub fn finished(&mut self, done: u64, now: Instant) -> Vec<(u64, Duration, String, String)> {
        let newest = self.exchanges.back().map(|last| last.request);
        self.exchanges
            .iter_mut()
            .filter(|exchange| {
                !exchange.journalled
                    && (Some(exchange.request) != newest || exchange.request == done)
            })
            .filter_map(|exchange| {
                exchange.journalled = true;
                exchange.lamp.chars().any(char::is_alphanumeric).then(|| {
                    (
                        exchange.request,
                        now.saturating_duration_since(exchange.at),
                        exchange.person.trim().to_owned(),
                        exchange.lamp.trim().to_owned(),
                    )
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

    /// Drop everything: a person has had something forgotten, and what was
    /// said about it is not to be told to the next session.
    pub fn forget(&mut self) {
        self.exchanges.clear();
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

    /// Whether the lamp said something to someone so lately that a
    /// conversation may still be going on.
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

    /// The person's exchanges ([`Self::opening`]) of the last `within`,
    /// at most `most`, oldest first, as (what they said, what the lamp said):
    /// for a route that gives a model the conversation as messages.
    pub fn recent(&self, now: Instant, within: Duration, most: usize) -> Vec<Recent> {
        let mut recent: Vec<Recent> = self
            .exchanges
            .iter()
            .rev()
            .filter(|exchange| {
                exchange.whose == self.opened_for
                    && now.saturating_duration_since(exchange.at) <= within
                    && exchange.lamp.chars().any(char::is_alphanumeric)
            })
            .take(most)
            .map(|exchange| Recent {
                written: exchange.written.clone(),
                person: exchange.person.trim().to_owned(),
                lamp: exchange.lamp.trim().to_owned(),
            })
            .collect();
        recent.reverse();
        recent
    }

    /// What to tell a new session, if anything was said lately, in at most
    /// `room` bytes: the latest exchanges that fit, in the order they went,
    /// of the person the session is opened for ([`Self::opening`]) only.
    pub fn recall(&self, now: Instant, room: usize) -> Option<String> {
        // An answer with no word in it ("...") is the model keeping quiet
        // aloud. Told back to it, it is what it does next.
        let told = |exchange: &&Exchange| {
            exchange.whose == self.opened_for
                && now.saturating_duration_since(exchange.at) <= REMEMBERED_FOR
                && exchange.lamp.chars().any(char::is_alphanumeric)
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

fn usable(path: &Path) -> bool {
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
}

impl Recalling {
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            tools: crate::tools::Settings::default(),
            name_signal: false,
            fresh_only: false,
            file: None,
        }
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

    /// `config` with the functions declared now. The executive's come and
    /// go with the executive. One the link would refuse is left out, the
    /// rest are kept, and the journal says which and why (#799): no setting
    /// leaves a session without functions, or says nothing.
    fn equipped(&self, config: SessionConfig) -> SessionConfig {
        let mut tools = crate::tools::declared(&self.tools);
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
    /// thread.
    fn recalled(&self, thread: &Thread, now: Instant, time: Option<&str>) -> SessionConfig {
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
        // The thread takes the room the file and the time leave it; a time
        // too long to add leaves the session as it always was.
        let room = lamp_gemini::MAX_INSTRUCTION_BYTES.saturating_sub(written.len() + time.len());
        thread
            .recall(now, room)
            .and_then(|thread| told(&format!("{time}{thread}")))
            .or_else(|| told(time))
            .unwrap_or_else(|| self.config.clone())
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
        if self.fresh_only {
            resume = None;
        }
        let mut time = crate::local_time::told_here(self.tools.clock);
        // Work asked of the staff is theirs, stories included (#854); said
        // after the persona, which asks for stories to be told gladly.
        time = crate::agent_tasks::noted(time, self.tools.tasks.as_ref());
        // The legacy cached note, or an explicitly opted-in Days snapshot
        // with its provenance and a total acquisition budget of 300 ms.
        if let Some(socket) = &self.tools.memory {
            let kept = crate::remember::told_for_session(socket, session.get(), person.as_deref());
            if !kept.is_empty() {
                time = Some(time.unwrap_or_default() + &kept);
            }
        }
        let config = current(self.file.as_deref(), |thread| {
            self.recalled(thread, Instant::now(), time.as_deref())
        })
        .unwrap_or_else(|| self.config.clone());
        Dialer::new(self.equipped(config)).connect(session, resume)
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

    const ROOM: usize = 8_192;

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
        assert_eq!(finished[0].0, 2);
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
            [(
                1,
                Duration::from_secs(3),
                "How do I take my coffee?".to_owned(),
                "Black.".to_owned()
            )]
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
            .map(|(request, ..)| request)
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
        // Only the last eight exchanges, each side cut to its length.
        for request in 10..22u64 {
            thread.person(&"word ".repeat(200), true, at(700 + request));
            thread.lamp(request, &"answer ".repeat(200), at(701 + request));
        }
        let recalled = thread.recall(at(730), ROOM).unwrap();
        assert_eq!(recalled.matches("Person: ").count(), 8);
        assert_eq!(recalled.matches("You: ").count(), 8);
        assert!(recalled.len() < 8 * (PERSON_BYTES + LAMP_BYTES + 20) + 400);
        assert!(!recalled.contains('\0') && !recalled.contains("  "));
        // With less room, the latest that fit, still in order; with none, nothing.
        let whole = recalled.len();
        let fewer = thread.recall(at(730), whole - 1).unwrap();
        assert_eq!(fewer.matches("You: ").count(), 7);
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
    /// never switched, saved as before, eight exchanges.
    #[test]
    fn with_nobody_ever_known_nothing_changes() {
        let mut thread = Thread::new();
        let start = Instant::now();
        assert!(!thread.switch_due(start) && !thread.switch_now(false, false, start));
        assert!(!thread.opening(start));
        for request in 0..12u64 {
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
            for _ in 0..10 {
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
