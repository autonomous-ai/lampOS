//! The lamp owner's enrolled voice print, for the linked services' send gate
//! only (`LAMP_SAME_VOICE_FOR=services`, `docs/same-voice.md`, "The owner's
//! voice print"). On lamp-9adf a "send" line's 1.8 s print against the
//! preparing line's 1.8 s print scored the owner's own voice 0.37-0.70
//! against [`same_voice::SAME_FROM`], 0.70, so their sends were refused. A
//! print averaged from a few of the owner's lines is steadier, and a send
//! or a read in full is held to [`same_voice::SERVICES_SAME_FROM`], 0.55.
//!
//! The first builder's terms, each held here and tested below:
//!
//! - **Only the setup owner, only after a spoken yes.** After an owner's
//!   send (sent, or refused as another voice) the model is told to ask
//!   [`ASK`] (in Vietnamese [`ASK_VI`]) once ([`ask_after_send`]); a plain
//!   yes to it, the owner's face seen saying it, starts learning from the
//!   owner's next lines ([`offer`]); nothing else does. A plain no is kept
//!   for [`NO_FOR`], with no print, and dropped once older.
//! - **On the lamp only, root's, 0600, beside the faces** ([`PATH`], in the
//!   folder of the eyes' [`FACES`]), its own file, written whole through
//!   [`Store::write`]; one that is not a root-owned 0600 regular file is
//!   never read, and is removed ([`Store::read`]).
//! - **Wiped by the reset plan** (`crates/setup/fixtures/reset-plan-product.json`),
//!   by the owner's "forget everything" ([`forget_with_everything`]), and
//!   by a sign-out, with the services' tokens ([`watch_sign_outs`]).
//! - **Never sent anywhere or backed up:** nothing names it but this module
//!   and the reset plans, and no part but the voice's own callers names this
//!   module (`nothing_but_the_reset_names_the_print`).
//! - **The journal holds counts and cosines only,** never a print.
use crate::same_voice::{self, Journal};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{self, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The eyes' known faces (`lamp-eyes --people` in the product unit): the
/// print is kept in the same folder, `/var/lib/lampos-eyes`, 0700.
pub const FACES: &str = "/var/lib/lampos-eyes/people.json";
/// The owner's voice print, its own file beside [`FACES`]. Private, as
/// [`Store::product`] is: no other module can name it.
const PATH: &str = "/var/lib/lampos-eyes/owner-voice.json";
/// Where it is written before it is put in place ([`Store::new_path`] of
/// [`PATH`]), as the reset plans name it.
#[cfg(test)]
const NEW_PATH: &str = "/var/lib/lampos-eyes/owner-voice.json.new";
/// Its owner: root, as `lamp-voice` runs.
pub const ROOT: u32 = 0;
/// Its mode.
pub const MODE: u32 = 0o600;
/// The ask, said as is.
pub const ASK: &str = "May I learn your voice, so I only send mail when it's you?";
/// The ask in Vietnamese.
pub const ASK_VI: &str = "Cho tôi học giọng của bạn nhé, để tôi chỉ gửi thư khi đúng là bạn?";
/// The words that show the lamp said the ask, in order: an ask cut before
/// them, or never said, takes no yes.
const ASKED_WORDS: [&str; 2] = ["learn your voice", "học giọng"];
/// A no is kept this long: the owner is not asked again meanwhile, and an
/// older one is dropped when read ([`State::kept`]).
pub const NO_FOR: Duration = Duration::from_secs(24 * 60 * 60);
/// Asked and not answered: not asked again in this run for this long.
pub const ASK_AGAIN_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
/// The yes comes within this after the ask.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(60);
/// The owner's lines after the yes are learnt from within this; what was
/// gathered by then and is not enough is dropped, never kept.
pub const LEARN_WITHIN: Duration = Duration::from_secs(15 * 60);
/// The least lines a print is made from: 2 (the owner, 4 Nov: most owners
/// are alone with the lamp, so the print need not be strict).
pub const LEAST_LINES: usize = 2;
/// The least speech a print is made from, all lines together: 3 s. Each
/// line's print is of its first 1.5 s of speech at most
/// (`same_voice::ASKED_AT`), 1.2 s at least (`same_voice::LEAST_SAMPLES`),
/// so it is 2 to 3 lines.
pub const LEAST_SPEECH: usize = same_voice::RATE * 3;
/// The most a kept file may be.
const MOST_BYTES: u64 = 1 << 20;
/// How often `account/state` is read for a sign-out ([`watch_sign_out`]):
/// under its 15 s lifespan, so no spell is missed.
const READ_ACCOUNT_EVERY: Duration = Duration::from_secs(5);
/// Lines whose print is still to come, waited for.
const WAITING_LINES: usize = 8;
/// The most of the lamp's words kept to find the ask in.
const SAID_MOST: usize = 2_048;

/// A plain yes to the ask, after [`plain`]: nothing more in it.
const YES: [&str; 22] = [
    "yes",
    "yeah",
    "yep",
    "yes please",
    "sure",
    "ok",
    "okay",
    "of course",
    "go ahead",
    "yes you may",
    "you may",
    "yes go ahead",
    "vâng",
    "có",
    "ừ",
    "được",
    "dạ",
    "dạ được",
    "ừ được",
    "được chứ",
    "có chứ",
    "vâng được",
];
/// A plain no.
const NO: [&str; 12] = [
    "no",
    "nope",
    "no thanks",
    "no thank you",
    "not now",
    "don't",
    "không",
    "thôi",
    "không cần",
    "không được",
    "không nhé",
    "thôi không",
];

/// The words of an answer as [`YES`] and [`NO`] hold them: lower case,
/// letters, digits and `'` only, the lamp's name taken off both ends.
fn plain(words: &str) -> String {
    let words: String = words
        .to_lowercase()
        .chars()
        .map(|letter| {
            if letter.is_alphanumeric() || letter == '\'' {
                letter
            } else {
                ' '
            }
        })
        .collect();
    let mut words: Vec<&str> = words.split_whitespace().collect();
    while words
        .first()
        .is_some_and(|word| ["lamp", "lampy"].contains(word))
    {
        words.remove(0);
    }
    while words
        .last()
        .is_some_and(|word| ["lamp", "lampy"].contains(word))
    {
        words.pop();
    }
    words.join(" ")
}

/// A plain yes, a plain no, or neither.
fn answer(words: &str) -> Option<bool> {
    let words = plain(words);
    if YES.contains(&words.as_str()) {
        Some(true)
    } else if NO.contains(&words.as_str()) {
        Some(false)
    } else {
        None
    }
}

/// What the model is told after an owner's send, the one time it asks.
pub fn ask_next() -> String {
    format!(
        "Then ask, in the conversation's language, exactly: \"{ASK}\" (in Vietnamese \"{ASK_VI}\"). \
         If they say yes, say you will learn it from the next few things they say; if no, say \
         that's fine."
    )
}

/// What is kept in [`PATH`]: the print, or a no and when.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Kept {
    /// The owner's memory id it is theirs by.
    owner: String,
    /// The print, unit length; empty for a no.
    #[serde(default)]
    print: Vec<f32>,
    #[serde(default)]
    lines: usize,
    #[serde(default)]
    speech_ms: u64,
    /// A no to the ask, with no print.
    #[serde(default)]
    declined: bool,
    at_unix_ms: u64,
}

/// Where the print is kept, and whose the file must be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store {
    pub(crate) path: PathBuf,
    pub(crate) uid: u32,
}

impl Store {
    /// The lamp's: [`PATH`], root's.
    fn product() -> Self {
        Self {
            path: PathBuf::from(PATH),
            uid: ROOT,
        }
    }

    fn new_path(&self) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(".new");
        PathBuf::from(name)
    }

    /// [`Self::read_saying`], saying nothing.
    #[cfg(test)]
    fn read(&self) -> Option<Kept> {
        self.read_saying(|_| {})
    }

    /// What is kept, only from a regular file of [`MODE`] owned by
    /// [`Self::uid`]: anything else is as none. The folder is the eyes'
    /// (`lampos`), who can unlink or swap the file though not read it, so
    /// it is opened without following a link or waiting on a FIFO
    /// (`O_NOFOLLOW | O_NONBLOCK`) and checked on the open file itself
    /// (`fstat`), never by its path again. One that fails is removed, so
    /// that a copy given to `lampos` (systemd's `StateDirectory=` chowns the
    /// folder's files) is not left for them to read; `refused` is told a
    /// fixed reason, never what the file holds.
    fn read_saying(&self, refused: impl FnOnce(&'static str)) -> Option<Kept> {
        self.read_json_saying(refused)
    }

    /// [`Self::read_saying`] of any kept JSON value: the same checks, for
    /// the setup's voice prints too (`crate::setup_voice`).
    pub(crate) fn read_json_saying<T: serde::de::DeserializeOwned>(
        &self,
        refused: impl FnOnce(&'static str),
    ) -> Option<T> {
        use std::io::Read;
        let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
        let file = match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(flags.bits() as i32)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) if error.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) => {
                // A link, refused by `O_NOFOLLOW`: the link goes, not what
                // it names.
                return self.refuse("a link", refused);
            }
            Err(_) => {
                refused("not readable");
                return None;
            }
        };
        let Ok(metadata) = file.metadata() else {
            refused("not readable");
            return None;
        };
        if !metadata.file_type().is_file() {
            return self.refuse("not a regular file", refused);
        }
        if metadata.mode() & 0o7777 != MODE || metadata.uid() != self.uid {
            return self.refuse("not its owner's 0600", refused);
        }
        if metadata.len() > MOST_BYTES {
            return self.refuse("too large", refused);
        }
        let mut bytes = Vec::new();
        file.take(MOST_BYTES).read_to_end(&mut bytes).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// A file that failed [`Self::read_saying`]'s checks: removed, and
    /// `refused` told `why`.
    fn refuse<T>(&self, why: &'static str, refused: impl FnOnce(&'static str)) -> Option<T> {
        refused(why);
        let _ = std::fs::remove_file(&self.path);
        None
    }

    /// Write `kept` whole: a new file of [`MODE`], given to [`Self::uid`],
    /// put in place. Nothing is left behind when a step fails.
    fn write(&self, kept: &Kept) -> io::Result<()> {
        self.write_json(kept)
    }

    /// [`Self::write`] of any JSON value, the same way.
    pub(crate) fn write_json<T: Serialize>(&self, kept: &T) -> io::Result<()> {
        let new = self.new_path();
        let written = (|| {
            let _ = std::fs::remove_file(&new);
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(MODE)
                .open(&new)?;
            // Root's group with root; a test's own store keeps its group.
            std::os::unix::fs::fchown(&file, Some(self.uid), (self.uid == ROOT).then_some(ROOT))?;
            file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(MODE))?;
            file.write_all(&serde_json::to_vec(kept).map_err(io::Error::other)?)?;
            file.sync_all()?;
            std::fs::rename(&new, &self.path)
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&new);
        }
        written
    }

    /// Remove the print and any half-written one: whether there was one.
    pub fn wipe(&self) -> io::Result<bool> {
        let mut was = false;
        for path in [self.path.clone(), self.new_path()] {
            match std::fs::remove_file(&path) {
                Ok(()) => was = true,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(was)
    }
}

/// One of the owner's lines learnt from.
struct Line {
    request: u64,
    print: Vec<f32>,
    speech: usize,
}

/// Where the ask is.
enum Stage {
    Idle,
    /// The model was told to ask in `request`; the lamp's words in it.
    Asked {
        request: u64,
        owner: String,
        at: Instant,
        said: String,
    },
    /// A plain yes in `request`'s line, its face still to be checked.
    Yes {
        request: u64,
        owner: String,
        at: Instant,
    },
    /// Learning from the owner's lines after `from`.
    Learning {
        owner: String,
        from: u64,
        at: Instant,
        lines: Vec<Line>,
    },
}

/// The ask and the learning, one a process.
pub struct State {
    store: Store,
    journal: Journal,
    stage: Stage,
    asked_at: Option<Instant>,
    /// Lines whose print and face are still to be read.
    waiting: VecDeque<u64>,
}

impl State {
    pub fn new(store: Store, journal: Journal) -> Self {
        Self {
            store,
            journal,
            stage: Stage::Idle,
            asked_at: None,
            waiting: VecDeque::new(),
        }
    }

    fn say(&self, line: &str) {
        (self.journal)(&format!("lamp-voice: owner print: {line}"));
    }

    /// What is kept, as of `now_ms`: a file refused is journaled by its
    /// reason, and a no older than [`NO_FOR`] is dropped, so no owner id and
    /// time are kept for longer.
    fn kept(&self, now_ms: u64) -> Option<Kept> {
        let kept = self
            .store
            .read_saying(|why| self.say(&format!("the kept file was {why}: removed")))?;
        let no_for = u64::try_from(NO_FOR.as_millis()).unwrap_or(u64::MAX);
        if kept.declined && now_ms.saturating_sub(kept.at_unix_ms) >= no_for {
            match self.store.wipe() {
                Ok(_) => self.say("a no over a day old: dropped"),
                Err(error) => self.say(&format!(
                    "a no over a day old, not dropped: {}",
                    error.kind()
                )),
            }
            return None;
        }
        Some(kept)
    }

    /// The owner's print, when one is kept for `owner`.
    fn enrolled(&self, owner: &str, now_ms: u64) -> Option<Vec<f32>> {
        self.kept(now_ms)
            .filter(|kept| kept.owner == owner && !kept.declined && !kept.print.is_empty())
            .map(|kept| kept.print)
    }

    /// Whether to ask now, after `owner`'s send in `request` that their
    /// face was `seen` saying: no print, no recent no, not asked this run
    /// lately, nothing under way. Asked, it is marked so.
    fn ask(&mut self, request: u64, owner: &str, seen: bool, now: Instant, now_ms: u64) -> bool {
        if !seen
            || !matches!(self.stage, Stage::Idle)
            || self
                .asked_at
                .is_some_and(|at| now.saturating_duration_since(at) < ASK_AGAIN_AFTER)
        {
            return false;
        }
        // A no older than a day is dropped by the read, so one kept is
        // recent.
        if let Some(kept) = self.kept(now_ms).filter(|kept| kept.owner == owner)
            && (kept.declined || !kept.print.is_empty())
        {
            return false;
        }
        self.asked_at = Some(now);
        self.stage = Stage::Asked {
            request,
            owner: owner.to_owned(),
            at: now,
            said: String::new(),
        };
        self.say(&format!("request {request}: the owner is asked"));
        true
    }

    /// The lamp said `words` in `request`, whose line was `heard`: the
    /// ask's own words, or the answer to it in the first later request.
    fn lamp_answered(
        &mut self,
        request: u64,
        heard: Option<&str>,
        words: &str,
        now: Instant,
        now_ms: u64,
    ) {
        let Stage::Asked {
            request: asked,
            owner,
            at,
            said,
        } = &mut self.stage
        else {
            return;
        };
        if request == *asked {
            if said.len() + words.len() <= SAID_MOST {
                said.push_str(words);
            }
            return;
        }
        if request < *asked {
            return;
        }
        let whole = ASKED_WORDS
            .iter()
            .any(|asked| crate::services::read_back_said(asked, said));
        let (owner, at) = (std::mem::take(owner), *at);
        self.stage = Stage::Idle;
        if !whole {
            self.say("the ask was not said: nothing learnt");
            return;
        }
        if now.saturating_duration_since(at) > ANSWER_WITHIN {
            self.say("no answer soon after the ask: nothing learnt");
            return;
        }
        match heard.and_then(answer) {
            Some(true) => {
                self.say(&format!(
                    "request {request}: a yes, to be seen said by the owner"
                ));
                self.stage = Stage::Yes {
                    request,
                    owner,
                    at: now,
                };
            }
            Some(false) => {
                let kept = Kept {
                    owner,
                    print: Vec::new(),
                    lines: 0,
                    speech_ms: 0,
                    declined: true,
                    at_unix_ms: now_ms,
                };
                match self.store.write(&kept) {
                    Ok(()) => self.say("a no: not asked again for a day"),
                    Err(error) => self.say(&format!("a no, not kept: {}", error.kind())),
                }
            }
            None => self.say("the answer was no plain yes or no: nothing learnt"),
        }
    }

    /// The line `request` was decided. `print_of` gives a line's print and
    /// its speech in samples once it has come; `seen` whether the owner's
    /// face was seen saying a line, and no other.
    fn offer(
        &mut self,
        request: u64,
        now: Instant,
        now_ms: u64,
        mut print_of: impl FnMut(u64) -> Option<(Vec<f32>, usize)>,
        seen: impl Fn(u64, &str) -> bool,
    ) {
        if matches!(self.stage, Stage::Idle | Stage::Asked { .. }) {
            self.waiting.clear();
            return;
        }
        if !self.waiting.contains(&request) {
            if self.waiting.len() == WAITING_LINES {
                self.waiting.pop_front();
            }
            self.waiting.push_back(request);
        }
        if let Stage::Yes {
            request: yes,
            owner,
            at,
        } = &self.stage
            && request > *yes
        {
            let (yes, owner, at) = (*yes, owner.clone(), *at);
            if seen(yes, &owner) {
                self.say(&format!(
                    "request {yes}: the yes seen said by the owner: learning from their next lines"
                ));
                self.stage = Stage::Learning {
                    owner,
                    from: yes,
                    at,
                    lines: Vec::new(),
                };
            } else {
                self.say(&format!(
                    "request {yes}: the yes not seen said by the owner: nothing learnt"
                ));
                self.stage = Stage::Idle;
                self.waiting.clear();
                return;
            }
        }
        let Stage::Learning {
            owner,
            from,
            at,
            lines,
        } = &mut self.stage
        else {
            return;
        };
        let mut said = Vec::new();
        if now.saturating_duration_since(*at) > LEARN_WITHIN {
            said.push(format!(
                "not made: {} lines, {:.1} s in {} min; dropped",
                lines.len(),
                speech_of(lines) as f32 / same_voice::RATE as f32,
                LEARN_WITHIN.as_secs() / 60
            ));
            self.stage = Stage::Idle;
            self.waiting.clear();
            for line in said {
                self.say(&line);
            }
            return;
        }
        let from = *from;
        self.waiting.retain(|line| *line > from);
        let mut still = VecDeque::new();
        while let Some(line) = self.waiting.pop_front() {
            if lines.iter().any(|kept| kept.request == line) {
                continue;
            }
            let Some((print, speech)) = print_of(line) else {
                still.push_back(line);
                continue;
            };
            if !seen(line, owner) {
                said.push(format!(
                    "request {line}: not seen said by the owner alone: not used"
                ));
                continue;
            }
            // A line unlike the lines so far is not used: being seen saying
            // it proves the face moved, not whose voice it was, and at 2
            // lines one other voice would be half the print (Thirteen's P2
            // on #5246). The owner's own lines stay above DIFFERENT_BELOW.
            if !lines.is_empty() {
                let likeness = mean(lines).and_then(|mean| same_voice::cosine(&mean, &print));
                match likeness {
                    Some(likeness) if likeness >= same_voice::DIFFERENT_BELOW => {}
                    Some(likeness) => {
                        said.push(format!(
                            "request {line}: unlike the lines so far (cosine {likeness:.2}): not used"
                        ));
                        continue;
                    }
                    None => continue,
                }
            }
            lines.push(Line {
                request: line,
                print,
                speech,
            });
        }
        self.waiting = still;
        let speech = speech_of(lines);
        let enough = lines.len() >= LEAST_LINES && speech >= LEAST_SPEECH;
        let made = enough.then(|| mean(lines)).flatten().map(|print| Kept {
            owner: owner.clone(),
            print,
            lines: lines.len(),
            speech_ms: u64::try_from(speech * 1_000 / same_voice::RATE).unwrap_or(u64::MAX),
            declined: false,
            at_unix_ms: now_ms,
        });
        if let Some(kept) = made {
            let seconds = kept.speech_ms as f32 / 1_000.0;
            said.push(match self.store.write(&kept) {
                Ok(()) => format!("{} lines, {seconds:.1} s: kept", kept.lines),
                Err(error) => format!(
                    "{} lines, {seconds:.1} s: not kept: {}",
                    kept.lines,
                    error.kind()
                ),
            });
            self.stage = Stage::Idle;
            self.waiting.clear();
        }
        for line in said {
            self.say(&line);
        }
    }

    /// Forget all under way, as the print is wiped.
    fn forget(&mut self) {
        self.stage = Stage::Idle;
        self.waiting.clear();
    }
}

fn speech_of(lines: &[Line]) -> usize {
    lines.iter().map(|line| line.speech).sum()
}

/// The mean of the lines' prints, each first made unit length, then made
/// unit length itself; none for prints of two lengths or no length.
fn mean(lines: &[Line]) -> Option<Vec<f32>> {
    let size = lines.first()?.print.len();
    let mut sum = vec![0.0f64; size];
    for line in lines {
        if line.print.len() != size {
            return None;
        }
        let length = line
            .print
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>()
            .sqrt();
        if length == 0.0 || !length.is_finite() {
            return None;
        }
        for (into, value) in sum.iter_mut().zip(&line.print) {
            *into += f64::from(*value) / length;
        }
    }
    let length = sum.iter().map(|value| value.powi(2)).sum::<f64>().sqrt();
    if length == 0.0 || !length.is_finite() {
        return None;
    }
    Some(sum.iter().map(|value| (value / length) as f32).collect())
}

#[cfg(not(test))]
mod kept {
    use super::State;
    use std::sync::Mutex;
    static STATE: Mutex<Option<State>> = Mutex::new(None);
    pub fn state<R>(f: impl FnOnce(&mut Option<State>) -> R) -> R {
        f(&mut STATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

/// Tests: each test thread has its own.
#[cfg(test)]
mod kept {
    use super::State;
    use std::cell::RefCell;
    thread_local! {
        static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    }
    pub fn state<R>(f: impl FnOnce(&mut Option<State>) -> R) -> R {
        STATE.with(|cell| f(&mut cell.borrow_mut()))
    }
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    kept::state(|state| state.as_mut().map(f))
}

fn now_ms() -> u64 {
    crate::app_make::now_unix_ms()
}

/// The second route's prints are for the services only and the services
/// are on (`LAMP_SAME_VOICE_FOR=services`): the owner's print is asked
/// for, learnt and used from now on. Off unless called.
pub fn enable() {
    enable_with(Store::product(), same_voice::to_stderr());
}

/// With [`enable`], on the worker's bus at `bus_root`: the owner's print is
/// removed on a sign-out, in a thread of its own ([`watch_sign_out`]).
/// Without a bus, it is not, and the journal says so.
pub fn watch_sign_outs(bus_root: Option<&Path>) {
    let Some(bus) = bus_root.and_then(|root| lamp_bus::Bus::new(root).ok()) else {
        eprintln!("lamp-voice: owner print: a sign-out is not watched: no bus");
        return;
    };
    let journal = same_voice::to_stderr();
    let started = std::thread::Builder::new()
        .name("owner-print-sign-out".into())
        .spawn(move || watch_sign_out(&bus, &Store::product(), &journal));
    if let Err(error) = started {
        eprintln!(
            "lamp-voice: owner print: a sign-out is not watched: {}",
            error.kind()
        );
    }
}

/// A sign-out's spells as `account/state` says them (`lamp-cloud`, with
/// `LAMP_SERVICES=on` whatever `LAMP_SIGNED_OUT` says): one begins once.
#[derive(Default)]
struct SignOut {
    out: bool,
}

impl SignOut {
    /// Takes what was read: whether a spell begins. A state past its
    /// lifespan, or none, is signed in, as the voice reads it
    /// (`coordinator::signed_out`): a stopped `lamp-cloud` wipes nothing.
    fn began(&mut self, state: Option<lamp_msgs::AccountState>) -> bool {
        let out = state.is_some_and(|state| state.signed_out);
        let began = out && !self.out;
        self.out = out;
        began
    }
}

/// `account/state` on `bus`, when it is said and not past its lifespan.
fn account(bus: &lamp_bus::Bus) -> Option<lamp_msgs::AccountState> {
    bus.latest(lamp_msgs::AccountState::TOPIC)
        .ok()
        .flatten()
        .filter(|message| !message.expired)
        .and_then(|message| message.parse().ok())
}

/// Reads `account/state` every [`READ_ACCOUNT_EVERY`], for good: each
/// sign-out spell that begins, or is under way at start, removes the
/// owner's print ([`wipe_signed_out`]), as `lamp-cloud` wipes the
/// services' tokens for it.
pub fn watch_sign_out(bus: &lamp_bus::Bus, store: &Store, journal: &Journal) -> ! {
    let mut spell = SignOut::default();
    loop {
        if spell.began(account(bus)) {
            wipe_signed_out(store, journal);
        }
        std::thread::sleep(READ_ACCOUNT_EVERY);
    }
}

/// The lamp is signed out: the owner's print goes with the services'
/// tokens, through the same removal as "forget everything" ([`wipe`]).
fn wipe_signed_out(store: &Store, journal: &Journal) {
    let said = match wipe(store) {
        Ok(true) => "signed out: wiped with the services' tokens".to_owned(),
        Ok(false) => "signed out: none was kept".to_owned(),
        Err(error) => format!(
            "signed out: could not be wiped with the services' tokens: {}",
            error.kind()
        ),
    };
    journal(&format!("lamp-voice: owner print: {said}"));
}

/// Remove the print, and drop the ask and learning under way: whether there
/// was one.
fn wipe(store: &Store) -> io::Result<bool> {
    with(State::forget);
    store.wipe()
}

/// [`enable`] with its store and journal.
pub fn enable_with(store: Store, journal: Journal) {
    kept::state(|state| *state = Some(State::new(store, journal)));
}

/// Whether the owner's print is in use ([`enable`]).
pub fn on() -> bool {
    kept::state(|state| state.is_some())
}

/// The owner `owner`'s print, when one is kept and in use.
pub fn enrolled(owner: &str) -> Option<Vec<f32>> {
    with(|state| state.enrolled(owner, now_ms())).flatten()
}

/// The cosine of a line's `print` against `owner`'s print.
pub fn likeness(owner: &str, print: &[f32]) -> Option<f32> {
    same_voice::cosine(&enrolled(owner)?, print)
}

/// After `owner`'s send in `request`, which their face was `seen` saying:
/// what the model is told to ask, once ([`ask_next`]), or nothing.
pub fn ask_after_send(request: u64, owner: &str, seen: bool, now: Instant) -> Option<String> {
    with(|state| state.ask(request, owner, seen, now, now_ms()))
        .unwrap_or(false)
        .then(ask_next)
}

/// Whether the lamp's next words are watched for the ask
/// (`thread::Thread::lamp_for`).
pub fn waits_for_answer() -> bool {
    with(|state| matches!(state.stage, Stage::Asked { .. })).unwrap_or(false)
}

/// The lamp said `words` in `request`, whose line was `heard`.
pub fn lamp_answered(request: u64, heard: Option<&str>, words: &str, now: Instant) {
    with(|state| state.lamp_answered(request, heard, words, now, now_ms()));
}

/// The line `request` was decided on the second route: with a yes heard,
/// the owner's lines since are learnt from `voices`' prints
/// (`Heeding`'s verdict, services' prints only).
pub fn offer<P: same_voice::Printer>(request: u64, voices: &mut same_voice::Voices<P>) {
    with(|state| {
        state.offer(
            request,
            Instant::now(),
            now_ms(),
            |line| {
                let print = voices.print_of(line)?;
                Some((print, voices.usable_speech(line).unwrap_or(0)))
            },
            |line, owner| crate::services::seen_saying_owner(line, owner).0,
        );
    });
}

/// The owner's "forget everything about me", marked by the second route's
/// guard: their voice print goes too, whether or not it is in use now, and
/// the answer says so. Another person's forget, the room's, an unmarked
/// one, or one whose memory was not emptied, leaves it.
pub fn forget_with_everything(
    told: (Value, bool),
    lease: &crate::memory_scope::ScopeLease,
) -> (Value, bool) {
    if !lease.everything_marked() {
        return told;
    }
    forget_if(
        &Store::product(),
        told,
        lease.admitted_person(),
        &crate::person::owner_ids(),
    )
}

/// [`forget_with_everything`] once its mark is judged.
pub(crate) fn forget_if(
    store: &Store,
    mut told: (Value, bool),
    admitted: Option<&str>,
    owners: &[String],
) -> (Value, bool) {
    let emptied =
        told.0["forgotten"] == true || told.0["note"] == json!("I was keeping nothing of them.");
    let theirs = admitted.is_some_and(|admitted| owners.iter().any(|owner| owner == admitted));
    if !emptied || !theirs {
        return told;
    }
    match wipe(store) {
        Ok(true) => {
            eprintln!("lamp-voice: owner print: wiped with everything about them");
            told.0["voice_print"] = json!("forgotten too");
        }
        Ok(false) => {}
        Err(error) => {
            eprintln!(
                "lamp-voice: owner print: could not be wiped with everything about them: {}",
                error.kind()
            );
            told.0["everything"] = json!(false);
            told.0["voice_print"] = json!("kept: it could not be removed");
        }
    }
    told
}

/// Whether `path` is beside the faces: in [`FACES`]' folder.
pub fn beside_the_faces(path: &Path) -> bool {
    path.parent() == Path::new(FACES).parent()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lamp-ovp-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A store in `dir` owned by whoever runs the test.
    fn store(dir: &Path) -> Store {
        let probe = dir.join("probe");
        std::fs::write(&probe, b"").unwrap();
        let uid = std::fs::metadata(&probe).unwrap().uid();
        std::fs::remove_file(probe).unwrap();
        Store {
            path: dir.join("owner-voice.json"),
            uid,
        }
    }

    fn journaled(store: Store) -> (State, Arc<Mutex<Vec<String>>>) {
        let said = Arc::new(Mutex::new(Vec::new()));
        let into = said.clone();
        let journal: Journal = Arc::new(move |line: &str| into.lock().unwrap().push(line.into()));
        (State::new(store, journal), said)
    }

    /// A print near `voice`'s, a little different for each line.
    fn print(voice: usize, line: u64) -> Vec<f32> {
        (0..8)
            .map(|at| {
                let base = if at == voice { 1.0 } else { 0.05 };
                base + 0.012_345_6 * ((line as usize + at) % 3) as f32
            })
            .collect()
    }

    const OWNER: &str = "ann";
    const FULL: usize = same_voice::RATE * 3 / 2;

    /// Asked in 10, the lamp's words the ask, and `answer` heard in 11.
    fn asked_and_answered(state: &mut State, answer: &str, now: Instant) {
        assert!(state.ask(10, OWNER, true, now, 1_000));
        state.lamp_answered(10, None, "Sent. ", now, 1_000);
        state.lamp_answered(10, None, ASK, now, 1_000);
        state.lamp_answered(11, Some(answer), "Thank you.", now, 1_000);
    }

    /// Lines 12.. offered with prints of the owner's voice, each seen said
    /// by `seen`, at `now`.
    fn offer_lines(
        state: &mut State,
        lines: std::ops::Range<u64>,
        now: Instant,
        seen: &dyn Fn(u64, &str) -> bool,
    ) {
        for line in lines {
            state.offer(
                line,
                now,
                2_000,
                |request| Some((print(0, request), FULL)),
                seen,
            );
        }
    }

    #[test]
    fn the_print_is_kept_beside_the_faces_its_own_file() {
        assert!(beside_the_faces(Path::new(PATH)));
        assert!(beside_the_faces(Path::new(NEW_PATH)));
        assert_ne!(PATH, FACES);
        assert_eq!(Store::product().uid, ROOT);
        assert_eq!(Store::product().path, Path::new(PATH));
        assert_eq!(Store::product().new_path(), Path::new(NEW_PATH));
        // The eyes' own unit keeps its faces at FACES.
        let unit = include_str!("../../../deploy/product/lamp-eyes.service");
        assert!(unit.contains(&format!("--people {FACES} ")));
    }

    /// Written 0600 and given to the store's owner (root on a lamp); a file
    /// of any other mode or owner, a link or a FIFO, is never read and is
    /// removed (a link, not what it names); one that can't be given to its
    /// owner is not left behind.
    #[test]
    fn the_print_is_written_0600_to_its_owner_and_read_only_so() {
        use std::os::unix::fs::PermissionsExt;
        let dir = folder("mode");
        let store = store(&dir);
        let kept = Kept {
            owner: OWNER.into(),
            print: vec![0.6, 0.8],
            lines: 4,
            speech_ms: 6_000,
            declined: false,
            at_unix_ms: 1,
        };
        store.write(&kept).unwrap();
        let metadata = std::fs::metadata(&store.path).unwrap();
        assert_eq!(metadata.mode() & 0o7777, MODE);
        assert_eq!(metadata.uid(), store.uid);
        assert!(!store.new_path().exists());
        assert_eq!(store.read(), Some(kept.clone()));
        // Any other mode is not read, and is removed.
        std::fs::set_permissions(&store.path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(store.read(), None);
        assert!(!store.path.exists());
        // Nor anyone else's.
        store.write(&kept).unwrap();
        let elsewhere = Store {
            uid: store.uid.wrapping_add(1),
            ..store.clone()
        };
        assert_eq!(elsewhere.read(), None);
        assert!(!store.path.exists());
        // Nor a link to it: the link goes, the file it names stays.
        store.write(&kept).unwrap();
        let link = Store {
            path: dir.join("link.json"),
            ..store.clone()
        };
        std::os::unix::fs::symlink(&store.path, &link.path).unwrap();
        assert_eq!(link.read(), None);
        assert!(std::fs::symlink_metadata(&link.path).is_err());
        assert_eq!(store.read(), Some(kept.clone()));
        // Nor a FIFO, which is not waited on.
        let fifo = Store {
            path: dir.join("fifo.json"),
            ..store.clone()
        };
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo.path)
            .status()
            .unwrap();
        assert!(made.success());
        assert_eq!(fifo.read(), None);
        assert!(std::fs::symlink_metadata(&fifo.path).is_err());
        // A write that can't give it to its owner leaves nothing (unless
        // the test runs as root, who can).
        if store.uid != ROOT {
            let root = Store {
                path: dir.join("root.json"),
                uid: ROOT,
            };
            assert!(root.write(&kept).is_err());
            assert!(!root.path.exists() && !root.new_path().exists());
        }
        assert!(store.wipe().unwrap());
        assert!(!store.wipe().unwrap());
    }

    /// A file refused is journaled by a fixed reason, never what it holds.
    #[test]
    fn a_refused_file_is_journaled_by_its_reason_alone() {
        let dir = folder("refused");
        let (state, said) = journaled(store(&dir));
        std::fs::write(&state.store.path, br#"{"owner":"secret-owner-id"}"#).unwrap();
        assert!(state.kept(1_000).is_none());
        assert!(!state.store.path.exists());
        assert_eq!(
            *said.lock().unwrap(),
            ["lamp-voice: owner print: the kept file was not its owner's 0600: removed"]
        );
    }

    /// The ask is made only after a send the owner's face was seen saying,
    /// once, never with a print kept or a no within a day.
    #[test]
    fn the_owner_is_asked_once_and_only_when_seen() {
        let dir = folder("ask");
        let (mut state, _) = journaled(store(&dir));
        let now = Instant::now();
        assert!(!state.ask(10, OWNER, false, now, 1_000), "not seen");
        assert!(state.ask(10, OWNER, true, now, 1_000));
        state.stage = Stage::Idle;
        assert!(!state.ask(12, OWNER, true, now, 1_000), "asked once");
        assert!(state.ask(12, OWNER, true, now + ASK_AGAIN_AFTER, 1_000));
        assert!(ask_next().contains(ASK) && ask_next().contains(ASK_VI));
    }

    /// Only a plain yes after the ask said whole, seen said by the owner,
    /// starts learning; a no is kept a day with no print; anything else,
    /// or no ask, learns nothing.
    #[test]
    fn only_a_yes_to_the_ask_starts_learning() {
        let now = Instant::now();
        let owner_seen = |_: u64, owner: &str| owner == OWNER;
        // No ask: lines are never learnt from.
        let dir = folder("yes-none");
        let (mut state, _) = journaled(store(&dir));
        offer_lines(&mut state, 12..20, now, &owner_seen);
        assert!(state.store.read().is_none());
        // Neither yes nor no.
        for words in ["maybe later", "yes but only for mail to Bo", "what?"] {
            let (mut state, _) = journaled(store(&dir));
            asked_and_answered(&mut state, words, now);
            offer_lines(&mut state, 12..20, now, &owner_seen);
            assert!(state.store.read().is_none(), "{words}");
        }
        // The ask not said: a yes takes nothing.
        let (mut state, _) = journaled(store(&dir));
        assert!(state.ask(10, OWNER, true, now, 1_000));
        state.lamp_answered(10, None, "Sent.", now, 1_000);
        state.lamp_answered(11, Some("Yes."), "Okay.", now, 1_000);
        offer_lines(&mut state, 12..20, now, &owner_seen);
        assert!(state.store.read().is_none());
        // A no: kept with no print, and no ask for a day; then dropped.
        let (mut state, said) = journaled(store(&dir));
        asked_and_answered(&mut state, "No thanks.", now);
        let kept = state.store.read().unwrap();
        assert!(kept.declined && kept.print.is_empty());
        state.asked_at = None;
        assert!(!state.ask(30, OWNER, true, now, 1_000 + 3_600_000));
        let day = u64::try_from(NO_FOR.as_millis()).unwrap();
        assert!(state.kept(1_000 + day - 1).is_some());
        assert!(state.ask(30, OWNER, true, now, 1_000 + day));
        assert!(!state.store.path.exists(), "the old no is dropped");
        assert!(
            said.lock()
                .unwrap()
                .iter()
                .any(|line| line.ends_with("a no over a day old: dropped"))
        );
        // A yes: learnt, and in Vietnamese too.
        for words in ["Yes.", "Vâng, được."] {
            let (mut state, _) = journaled(store(&dir));
            asked_and_answered(&mut state, words, now);
            offer_lines(&mut state, 12..20, now, &owner_seen);
            let kept = state.store.read().expect(words);
            assert!(!kept.declined && kept.lines >= LEAST_LINES, "{words}");
            state.store.wipe().unwrap();
        }
    }

    /// The print is the setup owner's alone: a yes not seen said by them
    /// learns nothing, and only lines seen said by them are learnt from.
    #[test]
    fn only_the_owners_seen_lines_make_the_print() {
        let now = Instant::now();
        let dir = folder("owner");
        // The yes said by another face: nothing.
        let (mut state, _) = journaled(store(&dir));
        asked_and_answered(&mut state, "Yes.", now);
        offer_lines(&mut state, 12..20, now, &|line, _| line != 11);
        assert!(state.store.read().is_none());
        // Every other line another person's: those are never used.
        let (mut state, said) = journaled(store(&dir));
        asked_and_answered(&mut state, "Yes.", now);
        let theirs =
            |line: u64, owner: &str| owner == OWNER && (line == 11 || line.is_multiple_of(2));
        offer_lines(&mut state, 12..19, now, &theirs);
        let kept = state.store.read().unwrap();
        assert_eq!(kept.owner, OWNER);
        assert_eq!(kept.lines, 2, "12 and 14 alone");
        assert!(
            said.lock()
                .unwrap()
                .iter()
                .any(|line| line.contains("request 13: not seen said by the owner alone"))
        );
        // Only the owner's print is theirs, and it is not dropped with age.
        assert!(state.enrolled(OWNER, u64::MAX).is_some());
        assert!(state.enrolled("bo", 2_000).is_none());
    }

    /// From 2 lines and 3 s of speech: two whole lines of 1.5 s each make
    /// a print.
    #[test]
    fn two_lines_and_three_seconds_make_a_print() {
        assert_eq!((LEAST_LINES, LEAST_SPEECH), (2, same_voice::RATE * 3));
        let now = Instant::now();
        let dir = folder("two");
        let (mut state, said) = journaled(store(&dir));
        asked_and_answered(&mut state, "Yes.", now);
        offer_lines(&mut state, 12..13, now, &|_, _| true);
        assert!(state.store.read().is_none(), "one line is not enough");
        offer_lines(&mut state, 13..14, now, &|_, _| true);
        let kept = state.store.read().unwrap();
        assert_eq!((kept.lines, kept.speech_ms), (2, 3_000));
        assert!(
            said.lock()
                .unwrap()
                .iter()
                .any(|line| line.ends_with("2 lines, 3.0 s: kept"))
        );
    }

    /// Under 3 s of speech is not enough, however many lines; a line
    /// unlike the others is left out and the next line waited for; too long
    /// after the yes, what was gathered is dropped.
    #[test]
    fn a_print_needs_three_seconds_and_an_unlike_line_is_left_out() {
        let now = Instant::now();
        let dir = folder("enough");
        let owner_seen = |_: u64, _: &str| true;
        let (mut state, said) = journaled(store(&dir));
        asked_and_answered(&mut state, "Yes.", now);
        // Two short lines, 1.2 s each: 2.4 s is not enough.
        let short = same_voice::RATE * 6 / 5;
        for line in 12..14 {
            state.offer(line, now, 2_000, |r| Some((print(0, r), short)), owner_seen);
        }
        assert!(state.store.read().is_none());
        // Another voice is left out.
        state.offer(14, now, 2_000, |r| Some((print(5, r), FULL)), owner_seen);
        assert!(state.store.read().is_none());
        state.offer(15, now, 2_000, |r| Some((print(0, r), FULL)), owner_seen);
        let kept = state.store.read().unwrap();
        assert_eq!((kept.lines, kept.speech_ms), (3, 3_900));
        let said = said.lock().unwrap();
        assert!(said.iter().any(|line| line.contains("request 14: unlike")));
        assert!(
            said.iter()
                .any(|line| line.ends_with("3 lines, 3.9 s: kept"))
        );
        // Too slow: dropped, never kept.
        state.store.wipe().unwrap();
        let (mut state, _) = journaled(store(&dir));
        asked_and_answered(&mut state, "Yes.", now);
        offer_lines(&mut state, 12..13, now, &owner_seen);
        offer_lines(&mut state, 13..20, now + LEARN_WITHIN * 2, &owner_seen);
        assert!(state.store.read().is_none());
    }

    /// A print still to come is waited for, and used once it comes.
    #[test]
    fn a_line_whose_print_comes_later_is_learnt_from_later() {
        let now = Instant::now();
        let dir = folder("later");
        let (mut state, _) = journaled(store(&dir));
        asked_and_answered(&mut state, "Yes.", now);
        let owner_seen = |_: u64, _: &str| true;
        state.offer(12, now, 2_000, |_| None, owner_seen);
        offer_lines(&mut state, 13..16, now, &owner_seen);
        assert_eq!(state.store.read().map(|kept| kept.lines), Some(2));
    }

    /// The journal holds counts and cosines only: no print's value.
    #[test]
    fn the_journal_holds_no_print() {
        let now = Instant::now();
        let dir = folder("journal");
        let (mut state, said) = journaled(store(&dir));
        asked_and_answered(&mut state, "Yes.", now);
        offer_lines(&mut state, 12..20, now, &|_, _| true);
        let kept = state.store.read().unwrap();
        let said = said.lock().unwrap();
        assert!(said.iter().any(|line| line.contains("lines,")));
        for line in said.iter() {
            assert!(line.starts_with("lamp-voice: owner print: "), "{line}");
            assert!(!line.contains('['), "{line}");
            for value in kept.print.iter().chain(&print(0, 12)) {
                assert!(!line.contains(&format!("{value:.4}")), "{line}");
            }
            // At most the two decimals of a cosine or seconds.
            assert!(
                !line
                    .split(|letter: char| !letter.is_ascii_digit() && letter != '.')
                    .any(|number| number
                        .split_once('.')
                        .is_some_and(|(_, decimals)| decimals.len() > 2)),
                "{line}"
            );
        }
    }

    /// The owner's "forget everything" wipes the print; another person's,
    /// or one that emptied nothing, leaves it.
    #[test]
    fn forget_everything_wipes_the_owners_print() {
        let dir = folder("forget");
        let store = store(&dir);
        let kept = Kept {
            owner: OWNER.into(),
            print: vec![1.0],
            lines: 4,
            speech_ms: 6_000,
            declined: false,
            at_unix_ms: 1,
        };
        store.write(&kept).unwrap();
        let owners = [OWNER.to_owned()];
        let emptied = (json!({"forgotten": true}), true);
        let told = forget_if(&store, emptied.clone(), Some("bo"), &owners);
        assert_eq!(told, emptied);
        let told = forget_if(
            &store,
            (json!({"forgotten": false}), true),
            Some(OWNER),
            &owners,
        );
        assert!(told.0.get("voice_print").is_none());
        assert!(store.path.exists());
        let told = forget_if(&store, emptied, Some(OWNER), &owners);
        assert_eq!(told.0["voice_print"], "forgotten too");
        assert!(!store.path.exists() && !store.new_path().exists());
    }

    /// A sign-out spell removes the print and its half-written copy once,
    /// learning under way dropped, and says so; one with none kept is fine;
    /// a state past its lifespan, none, or signed in removes nothing.
    #[test]
    fn a_sign_out_wipes_the_owners_print() {
        use lamp_msgs::AccountState;
        let dir = folder("sign-out");
        let store = store(&dir);
        let kept = Kept {
            owner: OWNER.into(),
            print: vec![1.0],
            lines: 4,
            speech_ms: 6_000,
            declined: false,
            at_unix_ms: 1,
        };
        let said = Arc::new(Mutex::new(Vec::<String>::new()));
        let into = said.clone();
        let journal: Journal = Arc::new(move |line: &str| into.lock().unwrap().push(line.into()));
        // Learning under way in this thread's state.
        enable_with(store.clone(), journal.clone());
        with(|state| {
            state.stage = Stage::Learning {
                owner: OWNER.into(),
                from: 1,
                at: Instant::now(),
                lines: Vec::new(),
            }
        });
        // The state as `lamp-cloud` says it, on a bus of the test's own.
        let bus = lamp_bus::Bus::new(dir.join("bus")).unwrap();
        let mut spell = SignOut::default();
        assert!(!spell.began(account(&bus)), "none said: signed in");
        let mut publisher = bus
            .publisher_declared(AccountState::TOPIC, &AccountState::declaration(), "cloud")
            .unwrap();
        publisher
            .publish_json(lamp_ipc::monotonic_us(), &AccountState::default())
            .unwrap();
        assert!(!spell.began(account(&bus)), "signed in");
        store.write(&kept).unwrap();
        std::fs::write(store.new_path(), b"half").unwrap();
        let out = AccountState {
            signed_out: true,
            for_s: 0,
        };
        publisher
            .publish_json(lamp_ipc::monotonic_us(), &out)
            .unwrap();
        assert!(spell.began(account(&bus)));
        wipe_signed_out(&store, &journal);
        assert!(!store.path.exists() && !store.new_path().exists());
        assert!(with(|state| matches!(state.stage, Stage::Idle)).unwrap());
        // The same spell read again begins nothing; with none kept, a new
        // spell is fine.
        assert!(!spell.began(Some(out)));
        assert!(!spell.began(None));
        assert!(spell.began(Some(out)));
        wipe_signed_out(&store, &journal);
        assert_eq!(
            *said.lock().unwrap(),
            [
                "lamp-voice: owner print: signed out: wiped with the services' tokens",
                "lamp-voice: owner print: signed out: none was kept",
            ]
        );
        kept::state(|state| *state = None);
    }

    /// The product's reset plan wipes the print and its half-written copy.
    #[test]
    fn the_reset_plan_wipes_the_print() {
        for plan in [
            include_str!("../../setup/fixtures/reset-plan-product.json"),
            include_str!("../../setup/fixtures/reset-plan-example.json"),
        ] {
            let plan: Value = serde_json::from_str(plan).unwrap();
            let wipe = plan["wipe"].as_array().unwrap();
            for path in [PATH, NEW_PATH] {
                assert!(wipe.iter().any(|wiped| wiped == path), "{path}");
            }
        }
    }

    /// Never sent anywhere or backed up: no file of the product's, and no
    /// source of any part, names the print or the faces' folder whole, but
    /// this module and the reset plans. So no list a part sends, uploads,
    /// shares, keeps in the update's records or backs up can hold it: the
    /// update's records keep only the files a build writes, the Store's
    /// share only a made skill's folder, the reports typed fields, and the
    /// eyes' folder is no one's whole. And no file but these names this
    /// module at all, so no new part can reach the store through it
    /// unseen: [`PATH`], [`NEW_PATH`] and [`Store::product`] are private.
    #[test]
    fn nothing_but_the_reset_names_the_print() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let allowed = [
            "crates/voice/src/owner_voice.rs",
            "crates/setup/fixtures/reset-plan-product.json",
            "crates/setup/fixtures/reset-plan-example.json",
            "crates/setup/tests/it/reset.rs",
        ];
        let whole = [
            "\"/var/lib/lampos-eyes\"",
            "\"/var/lib/lampos-eyes/\"",
            "/var/lib/lampos-eyes/*",
            "/var/lib/lampos-eyes ",
        ];
        // Today's: the module, its declaration, its callers in the voice,
        // the synthetic enrollment Engine fixture and the reset test's comment.
        let callers = [
            "crates/setup/tests/it/reset.rs",
            "crates/voice/src/lib.rs",
            "crates/voice/src/owner_voice.rs",
            "crates/voice/src/second_provider/tests/voice_confirms.rs",
            "crates/voice/src/second_provider/worker.rs",
            "crates/voice/src/services.rs",
            "crates/voice/src/setup_voice.rs",
            "crates/voice/src/thread.rs",
            "crates/voice/src/tools.rs",
        ];
        let mut naming = Vec::new();
        let mut stack = vec![root.join("crates"), root.join("deploy")];
        let mut read = 0;
        while let Some(at) = stack.pop() {
            for entry in std::fs::read_dir(&at).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy();
                if path.is_dir() {
                    if name != "target" && !name.starts_with('.') {
                        stack.push(path);
                    }
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                read += 1;
                let relative = path.strip_prefix(&root).unwrap().to_string_lossy();
                if text.contains("owner_voice") {
                    naming.push(relative.to_string());
                }
                if allowed.iter().any(|allowed| relative == *allowed) {
                    continue;
                }
                assert!(!text.contains("owner-voice"), "{relative} names the print");
                for whole in whole {
                    assert!(!text.contains(whole), "{relative} names {whole}");
                }
            }
        }
        assert!(read > 100, "{read}");
        naming.sort();
        assert_eq!(naming, callers, "the files that name `owner_voice`");
    }
}
