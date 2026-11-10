//! The owner's voice enrolled at setup (C115, `LAMP_SAME_VOICE_ENROLS_AT_SETUP`,
//! beside `LAMP_SAME_VOICE_CONFIRMS`; `docs/second-voice.md`, "The voice
//! only confirms"). Without it the known person's own prints
//! ([`crate::same_voice::OwnerVoice`]) are RAM only, made again from their
//! first lines after every start.
//!
//! - **Only at setup, only the owner's face:** while the walk leads "who you
//!   are" ([`crate::onboarding::who_you_are_open`]), a line one face alone was
//!   seen speaking, the one face in view through it (the rule the known
//!   person's own prints are learnt by), has its print held in RAM with that
//!   face's passing id. A line no face was seen speaking, or two faces, or a
//!   face beside another, holds none. Once the step ends, only the prints of
//!   the face the walk saved as the owner (the onboarding file's owner, the
//!   eyes' known face now) are kept, at most [`same_voice::OWNER_PRINTS`]; a
//!   guest's go, and so do all when no face was saved.
//! - **Off the step's path:** each line is read in a thread of its own
//!   ([`SetupVoice::heard_at`] returns at once), and the step's end is waited
//!   for there.
//! - **Kept beside the walk's progress,** `setup-voice.json` in the
//!   onboarding folder (root's, 0700, wiped whole by the reset plan), a
//!   root-owned 0600 file written whole and read with the owner print's own
//!   checks (`crate::owner_voice::Store`). Prints only, never sound.
//! - **Given back at each voice start** ([`SetupVoice::load`]) to the
//!   known person's own prints, for the owner the onboarding file still
//!   names. A file another owner's, or none named, is removed then, and so
//!   is one found while "who you are" is open again.
//! - **Never outlives its owner's walk** (Jarvis's P2 on #5582): the file is
//!   removed when a redo of "who you are" or the whole walk is posted, when
//!   the step ends with nothing kept, and when [`SetupVoice::load`] finds it
//!   another owner's. So a redo by someone else, whose lines are never seen
//!   alone, leaves no former owner's prints they could no longer forget.
//! - **Wiped by the owner's "forget everything"** ([`forget_with_everything`]),
//!   on the same mark and rule as the services' owner print, whatever the
//!   switch says, and by the reset plan. The prints still held for the step
//!   go with it, and a line in flight then holds none, so a later settle
//!   can't write them back. A face forgotten ([`owner_face_gone`]), which
//!   drops the walk's owner mark whoever's face it was, removes them too.
//! - **Switch off:** a file an earlier on-period kept is removed at the
//!   voice's start ([`switched_off`]), where the worker runs the attention
//!   rules (`LAMP_ATTENTION`, its `if files.attention`); a start without
//!   them leaves it (Reviewer-two's P3 on #5582).
//! - **One walk, one batch:** a line whose print comes only after its walk
//!   was settled holds none, so it can't replace the walk's kept prints
//!   (Reviewer-two's P2 on #5582); "who you are" open again, and a step
//!   that ends keeping nothing, empty the prints in RAM as well as the file.
//!   Each spoken line snapshots a generation at Engine Start processing,
//!   before callbacks or speech recognition, and carries it into its reader.
//!   The reader captures the walk before checking whether setup is open and
//!   validates it before clearing or holding prints. A delayed line cannot
//!   adopt a walk that settled, was forgotten or redone meanwhile. Events
//!   still queued upstream are outside this admission boundary.
//! - **The journal holds counts only,** never a print.
use crate::owner_voice::{self, Store};
use crate::same_voice::{self, Journal, OWNER_PRINTS, OwnerVoice};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// The kept prints' file, in the onboarding file's folder.
pub const FILE: &str = "setup-voice.json";
/// The most prints held while the step is open: a guest's lines among the
/// owner's push none of theirs out.
const HELD: usize = OWNER_PRINTS * 2;
/// The longest print taken: CAM++ gives 192 values.
const MOST_VALUES: usize = 1_024;

/// What is kept: whose, and their prints, each as the voice's child gave it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Kept {
    /// The owner's memory id.
    owner: String,
    prints: Vec<Vec<f32>>,
    at_unix_ms: u64,
}

impl Kept {
    /// Only prints of one length, finite, at most [`OWNER_PRINTS`].
    fn sound(&self) -> bool {
        let size = self.prints.first().map_or(0, Vec::len);
        !self.owner.is_empty()
            && !self.prints.is_empty()
            && self.prints.len() <= OWNER_PRINTS
            && size > 0
            && size <= MOST_VALUES
            && self
                .prints
                .iter()
                .all(|print| print.len() == size && print.iter().all(|value| value.is_finite()))
    }
}

/// The waits, apart for the tests.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// How long a line's print is waited for.
    pub print_within: Duration,
    /// How long the step's end, then the owner's saved face, is waited for.
    pub settle_within: Duration,
    /// How often each is read again.
    pub every: Duration,
}

pub const TIMING: Timing = Timing {
    print_within: Duration::from_secs(5),
    settle_within: Duration::from_secs(15 * 60),
    every: Duration::from_millis(250),
};

/// What one line's thread reads, each off the step's path.
pub struct Sight {
    /// Whether the walk still leads "who you are".
    pub open: Arc<dyn Fn() -> bool + Send + Sync>,
    /// The one face seen speaking the line alone, the one face in view.
    pub seen_alone: Box<dyn FnOnce() -> Option<String> + Send>,
    /// The owner the walk saved, by memory id, and their face as the eyes
    /// know it now.
    pub owner_now: Arc<dyn Fn() -> Option<(String, String)> + Send + Sync>,
}

impl Sight {
    /// The lamp's: the onboarding file at `onboarding`, the eyes' looks
    /// between `from` and `to`, and their known file `known`.
    pub fn of(
        onboarding: PathBuf,
        eyes: Arc<crate::speaking_faces::SpeakingFaces>,
        known: PathBuf,
        from: Instant,
        to: Instant,
    ) -> Self {
        let file = onboarding.clone();
        let looks = eyes.clone();
        Self {
            open: Arc::new(move || crate::onboarding::who_you_are_open(&file)),
            seen_alone: Box::new(move || {
                let speakers = looks.speakers_with_boot(from, to);
                match speakers.as_slice() {
                    [face] if looks.one_face_in_view(face, from, to) => Some(face.clone()),
                    _ => None,
                }
            }),
            owner_now: Arc::new(move || {
                let owner =
                    crate::known_faces::memory_person(&crate::onboarding::owner_of(&onboarding)?);
                let (name, face) = eyes.known_with_boot(&known)?;
                (crate::known_faces::memory_person(&name) == owner).then_some((owner, face))
            }),
        }
    }
}

/// A line's print held while the step is open, with the face seen saying it.
struct Held {
    request: u64,
    face: String,
    print: Vec<f32>,
}

#[derive(Default)]
struct Lines {
    held: Vec<Held>,
    /// A thread waits for the step's end.
    settling: bool,
    /// Counted up by each forget ([`SetupVoice::forget`]): a line whose
    /// print was waited for across one holds none.
    forgets: u64,
    /// Counted up by each settle's end: a line of that walk whose print
    /// comes after holds none, so it can't start a settle of its own that
    /// replaces the walk's kept prints (Reviewer-two's P2 on #5582).
    settles: u64,
}

/// The enrolment, one a worker.
pub struct SetupVoice {
    /// The walk's progress file, which says whether "who you are" is open
    /// and whom it saved as the owner.
    onboarding: PathBuf,
    store: Store,
    owner: Arc<OwnerVoice>,
    journal: Journal,
    timing: Timing,
    lines: Mutex<Lines>,
    /// Changed under `lines` by settlement and forget. A line snapshots it
    /// before spawning, without making the immediate handoff take a lock.
    generation: AtomicU64,
}

/// The kept prints' store beside the onboarding file `onboarding`.
pub fn store_beside(onboarding: &Path) -> Option<Store> {
    Some(Store {
        path: onboarding.parent()?.join(FILE),
        uid: owner_voice::ROOT,
    })
}

/// [`ENROLS_AT_SETUP_ENVIRONMENT`] read by `var`: `on` exactly.
///
/// [`ENROLS_AT_SETUP_ENVIRONMENT`]: same_voice::ENROLS_AT_SETUP_ENVIRONMENT
pub fn on_from(var: impl Fn(&str) -> Option<String>) -> bool {
    var(same_voice::ENROLS_AT_SETUP_ENVIRONMENT).as_deref() == Some("on")
}

/// The worker's enrolment: with the switch on and an onboarding file, the
/// kept prints are given back to `owner` now, and lines are taken from here
/// on. Off, none, and nothing is read. Either way a forget of everything
/// finds the kept file ([`register`]).
pub fn from_environment(
    onboarding: Option<&Path>,
    owner: &Arc<OwnerVoice>,
) -> Option<Arc<SetupVoice>> {
    let store = onboarding.and_then(store_beside);
    let on = on_from(|name| std::env::var(name).ok());
    let store = store?;
    let onboarding = onboarding?;
    if !on {
        switched_off(store);
        return None;
    }
    let setup = Arc::new(SetupVoice::new(
        onboarding.to_owned(),
        store.clone(),
        owner.clone(),
        same_voice::to_stderr(),
        TIMING,
    ));
    register(store, Some(setup.clone()));
    let owner = crate::onboarding::owner_of(onboarding)
        .map(|name| crate::known_faces::memory_person(&name));
    let given = setup.load(owner.as_deref());
    eprintln!(
        "lamp-voice second: the owner's voice is enrolled at setup: the prints of their lines in \"who you are\", their face alone seen speaking, are kept beside the walk's progress; {given} given back now (LAMP_SAME_VOICE_ENROLS_AT_SETUP)"
    );
    Some(setup)
}

impl SetupVoice {
    pub fn new(
        onboarding: PathBuf,
        store: Store,
        owner: Arc<OwnerVoice>,
        journal: Journal,
        timing: Timing,
    ) -> Self {
        Self {
            onboarding,
            store,
            owner,
            journal,
            timing,
            lines: Mutex::new(Lines::default()),
            generation: AtomicU64::new(0),
        }
    }

    /// The walk's progress file.
    pub fn onboarding(&self) -> &Path {
        &self.onboarding
    }

    fn say(&self, line: &str) {
        (self.journal)(&format!("lamp-voice second: setup voice: {line}"));
    }

    /// Remove the kept file, the journal told `why` when there was one.
    fn wipe(&self, why: &str) {
        match self.store.wipe() {
            Ok(true) => self.say(&format!("{why}: the kept prints removed")),
            Ok(false) => {}
            Err(error) => self.say(&format!(
                "{why}: the kept prints could not be removed: {}",
                error.kind()
            )),
        }
    }

    /// The kept prints, for `owner` (the onboarding file's, as a memory id),
    /// given to the known person's own prints: how many. Another owner's,
    /// none named, prints the voice can't use, or any while "who you are"
    /// is open again, are removed (Jarvis's P2 on #5582).
    pub fn load(&self, owner: Option<&str>) -> usize {
        if crate::onboarding::who_you_are_open(&self.onboarding) {
            self.wipe("\"who you are\" is open again");
            return 0;
        }
        let Some(kept) = self
            .store
            .read_json_saying::<Kept>(|why| self.say(&format!("the kept file was {why}: removed")))
        else {
            return 0;
        };
        if !kept.sound() {
            self.wipe("the kept file holds no prints the voice can use");
            return 0;
        }
        if owner != Some(kept.owner.as_str()) {
            self.wipe("the kept prints are not the walk's owner's");
            return 0;
        }
        // Lines' requests count up from 1: these never meet one.
        let given = kept
            .prints
            .into_iter()
            .enumerate()
            .filter(|(at, print)| {
                self.owner
                    .learn(&kept.owner, u64::MAX - *at as u64, print.clone())
            })
            .count();
        self.say(&format!("{given} kept prints given back"));
        given
    }

    /// Read at spoken Start admission, before callbacks or STT can span a
    /// forget, settlement or redo. No enrollment lock is taken.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Direct admission for unit fixtures. Production carries its token from
    /// Engine Start rather than reacquiring it after speech recognition.
    #[cfg(test)]
    pub fn heard(
        self: &Arc<Self>,
        request: u64,
        sight: Sight,
        print_of: crate::second_provider::PrintOf,
    ) {
        self.heard_at(self.generation(), request, sight, print_of);
    }

    /// Read the admitted line in its own thread. The Start token stays with
    /// it across speech recognition and all later callbacks.
    pub(crate) fn heard_at(
        self: &Arc<Self>,
        generation: u64,
        request: u64,
        sight: Sight,
        print_of: crate::second_provider::PrintOf,
    ) {
        let setup = self.clone();
        let started = std::thread::Builder::new()
            .name("setup-voice".into())
            .spawn(move || setup.read_line(request, sight, print_of, generation));
        if let Err(error) = started {
            self.say(&format!("request {request}: not read: {}", error.kind()));
        }
    }

    fn read_line(
        &self,
        request: u64,
        sight: Sight,
        print_of: crate::second_provider::PrintOf,
        generation: u64,
    ) {
        let Sight {
            open,
            seen_alone,
            owner_now,
        } = sight;
        // Capture the walk before reading the onboarding file: that read
        // can finish after a settle or forget, and its old `open` answer
        // must not adopt the newer walk or clear its saved prints.
        let (forgets, settles) = {
            let Ok(lines) = self.lines.lock() else {
                return;
            };
            if self.generation.load(Ordering::Acquire) != generation {
                drop(lines);
                self.say(&format!(
                    "request {request}: its walk changed before the line was read: none held"
                ));
                return;
            }
            (lines.forgets, lines.settles)
        };
        if !open() {
            return;
        }
        // Nothing held and no settle waiting: the step is open again (a
        // redo) or for the first time, and a file kept by an earlier walk's
        // owner goes now, whoever this walk saves (Jarvis's P2 on #5582).
        {
            let Ok(lines) = self.lines.lock() else {
                return;
            };
            if (lines.forgets, lines.settles) != (forgets, settles) {
                drop(lines);
                self.say(&format!(
                    "request {request}: its walk changed while the step was read: none held"
                ));
                return;
            }
            if !lines.settling && lines.held.is_empty() {
                self.drop_walk("\"who you are\" is open again");
            }
        }
        let Some(face) = seen_alone() else {
            self.say(&format!(
                "request {request}: no one face alone seen saying it: no print held"
            ));
            return;
        };
        let until = Instant::now() + self.timing.print_within;
        let print = loop {
            if let Some(print) = print_of(request) {
                break print;
            }
            if Instant::now() >= until {
                self.say(&format!("request {request}: no print came: none held"));
                return;
            }
            std::thread::sleep(self.timing.every);
        };
        let settle = {
            let Ok(mut lines) = self.lines.lock() else {
                return;
            };
            if lines.forgets != forgets {
                drop(lines);
                self.say(&format!(
                    "request {request}: a forget came while its print was waited for: none held"
                ));
                return;
            }
            if lines.settles != settles {
                drop(lines);
                self.say(&format!(
                    "request {request}: its print came after its walk was settled: none held"
                ));
                return;
            }
            if lines.held.len() == HELD {
                lines.held.remove(0);
            }
            lines.held.push(Held {
                request,
                face,
                print,
            });
            !std::mem::replace(&mut lines.settling, true)
        };
        self.say(&format!(
            "request {request}: its print held until the step ends"
        ));
        if settle {
            self.settle(&*open, &*owner_now, generation);
        }
    }

    /// Wait for the step's end, then for the owner's saved face, and keep
    /// the prints of that face alone; everything held goes.
    fn settle(
        &self,
        open: &dyn Fn() -> bool,
        owner_now: &dyn Fn() -> Option<(String, String)>,
        generation: u64,
    ) {
        let until = Instant::now() + self.timing.settle_within;
        let owner = loop {
            // A forgotten walk no longer owns this waiter. A new walk may
            // already have its own settler; leave all of its state alone.
            if self.generation.load(Ordering::Acquire) != generation {
                return;
            }
            if !open()
                && let Some(owner) = owner_now()
            {
                break Some(owner);
            }
            if Instant::now() >= until {
                break None;
            }
            std::thread::sleep(self.timing.every);
        };
        // The decision, the write and the giving back are made under the
        // lines' lock, so a forget ([`Self::forget`]) is wholly before or
        // wholly after them, never undone by them (Jarvis's P3 on #5582).
        let Ok(mut lines) = self.lines.lock() else {
            return;
        };
        // Both callbacks above can finish after forget and a fresh walk.
        // Check ownership again under the lock before draining or clearing
        // anything, including the newer walk's settling flag.
        if self.generation.load(Ordering::Acquire) != generation {
            return;
        }
        lines.settling = false;
        lines.settles = lines.settles.wrapping_add(1);
        self.generation.fetch_add(1, Ordering::Release);
        let held = std::mem::take(&mut lines.held);
        let Some((owner, face)) = owner else {
            self.say(&format!(
                "no owner's face saved by the step's end: {} held prints dropped",
                held.len()
            ));
            self.drop_walk("nothing kept by this walk");
            return;
        };
        let theirs: Vec<Held> = held
            .into_iter()
            .filter(|line| line.face == face)
            .take(OWNER_PRINTS)
            .collect();
        if theirs.is_empty() {
            self.say("no line seen said by the owner's saved face: none kept");
            self.drop_walk("nothing kept by this walk");
            return;
        }
        let kept = Kept {
            owner: owner.clone(),
            prints: theirs.iter().map(|line| line.print.clone()).collect(),
            at_unix_ms: crate::app_make::now_unix_ms(),
        };
        match self.store.write_json(&kept) {
            Ok(()) => self.say(&format!("{} of the owner's prints kept", kept.prints.len())),
            Err(error) => {
                self.say(&format!("the owner's prints not kept: {}", error.kind()));
                self.drop_walk("nothing kept by this walk");
                return;
            }
        }
        for line in theirs {
            self.owner.learn(&owner, line.request, line.print);
        }
        drop(lines);
    }

    /// The walk's prints gone, kept file and the known person's own prints
    /// in RAM alike, at one boundary: "who you are" open again, or a step
    /// that ends keeping nothing (Reviewer-two's P3 on #5582). Called with
    /// the lines' lock held, so the lock order stays lines, then
    /// [`OwnerVoice`].
    fn drop_walk(&self, why: &str) {
        self.wipe(why);
        self.owner.forget();
    }

    /// Everything of the step's: the prints held and any line in flight
    /// (it then holds none), the kept file, and the known person's own
    /// prints in RAM. Under the lines' lock, so a settle can't write any of
    /// it back (Jarvis's P3 on #5582).
    fn forget(&self) {
        let mut lines = self
            .lines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.forget_locked(&mut lines);
    }

    /// Memory's validated app/voice forget, independent of a tool receipt.
    /// Pending setup lines have no final owner yet, so every person's forget
    /// invalidates the whole pending batch. An unrelated valid kept owner is
    /// preserved. Lock ordering stays lines, then the shared confirmation cache.
    pub(crate) fn memory_forgot(&self, person: Option<&str>) {
        if person.is_some_and(|person| !lamp_msgs::known::is_person_id(person)) {
            return;
        }
        let mut lines = self
            .lines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        lines.held.clear();
        lines.settling = false;
        lines.forgets = lines.forgets.wrapping_add(1);
        self.generation.fetch_add(1, Ordering::Release);
        match person {
            Some(person) => self.owner.forget_person(person),
            None => self.owner.forget(),
        }
        let preserved = person.is_some_and(|person| {
            self.store
                .read_json_saying::<Kept>(|why| {
                    self.say(&format!(
                        "memory forget: kept prints could not be used ({why}); cleanup attempted"
                    ));
                })
                .is_some_and(|kept| {
                    kept.sound()
                        && lamp_msgs::known::is_person_id(&kept.owner)
                        && kept.owner != person
                })
        });
        if !preserved {
            self.wipe("memory forgotten");
        }
    }

    fn forget_locked(&self, lines: &mut Lines) {
        lines.held.clear();
        // The old waiter is fenced by generation. Let a fresh walk start
        // its own waiter without depending on the old callbacks returning.
        lines.settling = false;
        lines.forgets = lines.forgets.wrapping_add(1);
        self.generation.fetch_add(1, Ordering::Release);
        self.wipe("forgotten");
        self.owner.forget();
    }
}

#[cfg(not(test))]
mod registered {
    use super::Registered;
    use std::sync::Mutex;
    static AT: Mutex<Option<Registered>> = Mutex::new(None);
    pub fn with<R>(f: impl FnOnce(&mut Option<Registered>) -> R) -> R {
        f(&mut AT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

/// Tests: each test thread has its own.
#[cfg(test)]
mod registered {
    use super::Registered;
    use std::cell::RefCell;
    thread_local! {
        static AT: RefCell<Option<Registered>> = const { RefCell::new(None) };
    }
    pub fn with<R>(f: impl FnOnce(&mut Option<Registered>) -> R) -> R {
        AT.with(|cell| f(&mut cell.borrow_mut()))
    }
}

/// Where the kept prints are, and with the switch on the enrolment that
/// holds and feeds them, for a forget of everything.
type Registered = (Store, Option<Arc<SetupVoice>>);

/// The worker's kept file and, with the switch on, its enrolment.
pub fn register(store: Store, setup: Option<Arc<SetupVoice>>) {
    registered::with(|at| *at = Some((store, setup)));
}

/// The switch off at the voice's start: a file an earlier on-period kept
/// is removed (Jarvis's P3 on #5582), and a forget of everything still
/// knows where it was.
pub fn switched_off(store: Store) {
    match store.wipe() {
        Ok(true) => eprintln!(
            "lamp-voice second: setup voice: switched off: the prints an earlier start kept removed"
        ),
        Ok(false) => {}
        Err(error) => eprintln!(
            "lamp-voice second: setup voice: switched off: the prints an earlier start kept could not be removed: {}",
            error.kind()
        ),
    }
    register(store, None);
}

/// Post an accepted owner-step or whole-walk redo and fence the old batch
/// at the same boundary. Posting already performs file I/O on the tool path;
/// holding lines across it keeps old settlers from publishing between the
/// post and invalidation. Heard still only spawns its reader without I/O.
/// A failed post leaves the current enrolment untouched.
pub(crate) fn post_redo(
    onboarding: &Path,
    post: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<()> {
    let setup = registered::with(|at| at.as_ref().and_then(|(_, setup)| setup.clone()))
        .filter(|setup| setup.onboarding == onboarding);
    let Some(setup) = setup else {
        return post();
    };
    let mut lines = setup
        .lines
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    post()?;
    setup.forget_locked(&mut lines);
    Ok(())
}

/// A face forgotten while the walk at `onboarding` keeps an owner
/// (`forget_my_face`, a forget of everything that took the face, or the
/// eyes no longer knowing the owner): the walk drops its owner mark,
/// whoever's face it was, so the prints kept for that owner go now, with
/// those held and those in RAM (Jarvis's P3 on #5582). Before, they went
/// unused and stayed until the next start.
pub fn owner_face_gone(onboarding: &Path) {
    let setup = registered::with(|at| at.as_ref().and_then(|(_, setup)| setup.clone()))
        .filter(|setup| setup.onboarding == onboarding);
    if let Some(setup) = setup {
        setup.forget();
        return;
    }
    if let Some(store) = store_beside(onboarding)
        && let Err(error) = store.wipe()
    {
        eprintln!(
            "lamp-voice second: setup voice: a face forgotten: the kept prints could not be removed: {}",
            error.kind()
        );
    }
}

/// The owner's "forget everything about me", marked by the second route's
/// guard: the prints kept at setup go too, on the services' owner print's
/// rule (`owner_voice::forget_if`), and the known person's own prints in
/// RAM with them.
pub fn forget_with_everything(
    told: (Value, bool),
    lease: &crate::memory_scope::ScopeLease,
) -> (Value, bool) {
    if !lease.everything_marked() {
        return told;
    }
    forget_if(told, lease.admitted_person(), &crate::person::owner_ids())
}

/// [`forget_with_everything`] once its mark is judged.
fn forget_if(told: (Value, bool), admitted: Option<&str>, owners: &[String]) -> (Value, bool) {
    let Some((store, setup)) = registered::with(|at| at.clone()) else {
        return told;
    };
    let before = told.0.clone();
    let told = owner_voice::forget_if(&store, told, admitted, owners);
    let emptied = told.0["forgotten"] == true || told.0["note"] == "I was keeping nothing of them.";
    let theirs = admitted.is_some_and(|admitted| owners.iter().any(|owner| owner == admitted));
    if emptied && theirs {
        if let Some(setup) = setup {
            setup.forget();
        }
        if told.0 != before {
            eprintln!("lamp-voice second: setup voice: the owner's kept prints forgotten too");
        }
    }
    told
}

#[cfg(test)]
mod tests {
    /// The current-thread worker must remain schedulable while setup cleanup
    /// waits for the enrollment lock. The thread's fallback bounds failures.
    #[tokio::test(flavor = "current_thread")]
    async fn app_forget_setup_batch_wait_does_not_block_the_worker_reactor() {
        use std::future::Future;
        use std::sync::{atomic::AtomicBool, mpsc};
        let dir = folder("app-forget-reactor");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        let person = lamp_msgs::known::person_id("Synthetic reactor owner");
        for request in 1..=3 {
            assert!(owner.learn(&person, request, own(request)));
        }
        let bus_dir = crate::process::SessionDirectory::create().unwrap();
        let bus = lamp_bus::Bus::new(&bus_dir.path).unwrap();
        let forgot = crate::bus_out::ForgotIn::on(&bus).unwrap();
        let mut memory = bus
            .publisher_declared(
                lamp_msgs::MemoryForgot::TOPIC,
                &lamp_msgs::MemoryForgot::declaration(),
                "memory",
            )
            .unwrap();
        memory
            .publish_json(
                1,
                &lamp_msgs::MemoryForgot {
                    person: Some(person.clone()),
                },
            )
            .unwrap();
        // A later event in this SAME batch must retire the pending write
        // before the first person's setup cleanup waits on storage's lock.
        memory
            .publish_json(
                2,
                &lamp_msgs::MemoryForgot {
                    person: Some("everyone".into()),
                },
            )
            .unwrap();
        crate::services::pending_for_test(Some(crate::services::Pending::new(
            "synthetic",
            1,
            Instant::now(),
            "Synthetic draft",
            &person,
        )));
        let (locked_tx, locked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let fallback = Arc::new(AtomicBool::new(false));
        let holder_setup = setup.clone();
        let holder_fallback = fallback.clone();
        let holder = std::thread::spawn(move || {
            let _lines = holder_setup.lines.lock().unwrap();
            locked_tx.send(()).unwrap();
            if release_rx.recv_timeout(Duration::from_secs(2)).is_err() {
                holder_fallback.store(true, Ordering::SeqCst);
            }
        });
        locked_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (polled_tx, polled_rx) = tokio::sync::oneshot::channel();
        let task_setup = setup.clone();
        let mut task = tokio::spawn(async move {
            let mut future = Box::pin(crate::second_provider::worker::setup_forgot_batch_for_test(
                forgot, task_setup,
            ));
            let mut polled = Some(polled_tx);
            futures_util::future::poll_fn(|cx| {
                // The batch has no timer before cleanup. Its first poll reaches
                // the actual dispatch; an inline callback waits for fallback.
                let state = future.as_mut().poll(cx);
                if let Some(polled) = polled.take() {
                    let _ = polled.send(());
                }
                state
            })
            .await
        });
        let polled = tokio::time::timeout(Duration::from_secs(5), polled_rx).await;
        let retired_before_storage = !crate::services::pending_for_test_is_some();
        let heartbeat = tokio::spawn(async { true });
        let heartbeat_ran = heartbeat.await.unwrap();
        let released_before_fallback = !fallback.load(Ordering::SeqCst);
        let _ = release_tx.send(());
        holder.join().unwrap();
        // Completion is the exact production batch returning its subscriber
        // after await, independent of the confirmation cache representation.
        let complete = tokio::time::timeout(Duration::from_secs(5), &mut task).await;
        if complete.is_err() {
            task.abort();
            let _ = task.await;
        }
        let remaining = owner.known(&person);
        drop(setup);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(polled.is_ok_and(|value| value.is_ok()));
        assert!(
            heartbeat_ran && released_before_fallback,
            "the reactor must release cleanup before the holder's 2s failure fallback"
        );
        assert!(
            matches!(complete, Ok(Ok(true))),
            "the serialized blocking callback completed"
        );
        assert!(
            retired_before_storage,
            "all current-batch pending writes retire before setup storage is released"
        );
        assert_eq!(remaining, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn engine_forget_batch_preserves_consumer_matrix() {
        let _one = crate::provider_worker::tests::one_thread();
        use crate::second_provider::{Engine, Listener, Services, Told, VoiceConfirms, net};
        use futures_util::{
            future::BoxFuture,
            stream::{self, BoxStream},
        };
        struct Silent;
        impl Services for Silent {
            fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
                Box::pin(std::future::pending())
            }
            fn chat(
                &self,
                _: Vec<net::Message>,
            ) -> BoxStream<'static, net::Result<net::ChatEvent>> {
                Box::pin(stream::pending())
            }
            fn speak(&self, _: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
                Box::pin(stream::pending())
            }
        }
        for summaries in [false, true] {
            for enrolled in [false, true] {
                let dir = folder(&format!("forget-matrix-{summaries}-{enrolled}"));
                let store = store(&dir);
                let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
                let setup = enrolled.then(|| setup(&store, &owner));
                let person = lamp_msgs::known::person_id("Synthetic matrix owner");
                for request in 1..=3 {
                    assert!(owner.learn(&person, request, own(request)));
                }
                let engine = Engine::new(
                    Arc::new(Silent),
                    Told::default,
                    Arc::new(|_, _, _, _| Box::pin(async { serde_json::Value::Null })),
                )
                .confirming_by_voice(VoiceConfirms {
                    owner: owner.clone(),
                    setup: setup.clone(),
                    print_of: Arc::new(|_| None),
                });
                let bus_dir = crate::process::SessionDirectory::create().unwrap();
                let bus = lamp_bus::Bus::new(&bus_dir.path).unwrap();
                let forgot = crate::bus_out::ForgotIn::on(&bus).unwrap();
                let mut memory = bus
                    .publisher_declared(
                        lamp_msgs::MemoryForgot::TOPIC,
                        &lamp_msgs::MemoryForgot::declaration(),
                        "memory",
                    )
                    .unwrap();
                crate::services::pending_for_test(Some(crate::services::Pending::new(
                    "synthetic",
                    1,
                    Instant::now(),
                    "Synthetic draft",
                    &person,
                )));
                memory
                    .publish_json(
                        1,
                        &lamp_msgs::MemoryForgot {
                            person: Some(person.clone()),
                        },
                    )
                    .unwrap();
                assert!(
                    crate::second_provider::worker::engine_forgot_batch_for_test(
                        forgot, &engine, summaries
                    )
                    .await
                );
                assert!(!crate::services::pending_for_test_is_some());
                assert_eq!(
                    owner.known(&person),
                    if enrolled { 0 } else { 3 },
                    "setup-only scope does not add confirmation cleanup"
                );
                drop(engine);
                drop(setup);
                std::fs::remove_dir_all(dir).unwrap();
            }
        }
    }

    /// Composition regression: the app-forget callback clears the shared
    /// Engine cache while an old setup settler is paused after reading its owner.
    #[test]
    fn an_app_forget_fences_an_old_setup_settler() {
        use crate::second_provider::{Engine, Listener, Services, Told, VoiceConfirms, net};
        use futures_util::{
            future::BoxFuture,
            stream::{self, BoxStream},
        };
        use std::sync::mpsc;
        struct Silent;
        impl Services for Silent {
            fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
                Box::pin(std::future::pending())
            }
            fn chat(
                &self,
                _: Vec<net::Message>,
            ) -> BoxStream<'static, net::Result<net::ChatEvent>> {
                Box::pin(stream::pending())
            }
            fn speak(&self, _: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
                Box::pin(stream::pending())
            }
        }
        let _one = crate::provider_worker::tests::one_thread();
        let dir = folder("app-forget-composition-proof");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        let engine = Engine::new(
            Arc::new(Silent),
            Told::default,
            Arc::new(|_, _, _, _| Box::pin(async { serde_json::Value::Null })),
        )
        .confirming_by_voice(VoiceConfirms {
            owner: owner.clone(),
            setup: Some(setup.clone()),
            print_of: prints(own),
        });
        let person = lamp_msgs::known::person_id("Synthetic setup owner");
        let bus_dir = crate::process::SessionDirectory::create().unwrap();
        let bus = lamp_bus::Bus::new(&bus_dir.path).unwrap();
        let mut forgot = crate::bus_out::ForgotIn::on(&bus).unwrap();
        let mut memory = bus
            .publisher_declared(
                lamp_msgs::MemoryForgot::TOPIC,
                &lamp_msgs::MemoryForgot::declaration(),
                "memory",
            )
            .unwrap();
        let old = Walk::new();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let mut sight = old.sight(Some(FACE));
        let saved_person = person.clone();
        sight.owner_now = Arc::new(move || {
            let result = Some((saved_person.clone(), FACE.to_owned()));
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            result
        });
        let reader = setup.clone();
        let old_setup_generation = setup.generation();
        let old_thread = std::thread::spawn(move || {
            reader.read_line(1, sight, prints(own), old_setup_generation)
        });
        wait_held(&setup, 1);
        for request in 2..=3 {
            setup.heard(request, old.sight(Some(FACE)), prints(own));
        }
        wait_held(&setup, 3);
        assert_eq!(setup.lines.lock().unwrap().held.len(), 3);
        old.saved(FACE);
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        // Teach the Engine cache only after the initial open-step cleanup.
        for request in 101..=103 {
            assert!(owner.learn(&person, request, own(request)));
        }
        assert!(matches!(
            owner.judge(&person, &own(1)),
            Confirmed::Theirs(_)
        ));
        assert!(
            !store.path.exists(),
            "settler is still held before persistence"
        );
        memory
            .publish_json(
                1,
                &lamp_msgs::MemoryForgot {
                    person: Some(person.clone()),
                },
            )
            .unwrap();
        crate::second_provider::worker::forget_received(
            &mut forgot,
            1_000_000,
            crate::second_provider::worker::setup_forget_of(&engine).as_deref(),
            false,
        );
        let immediate = (
            owner.known(&person),
            owner.judge(&person, &own(1)),
            store.path.exists(),
        );
        release_tx.send(()).unwrap();
        old_thread.join().unwrap();
        let after = (
            owner.known(&person),
            owner.judge(&person, &own(1)),
            store.path.exists(),
        );
        let persisted = store
            .read_json_saying::<Kept>(|_| {})
            .map(|kept| kept.prints.len());
        // Fresh setup work is accepted, while old admission generations stay stale.
        setup.forget();
        setup.read_line(
            99,
            Walk::new().sight(Some(FACE)),
            prints(own),
            old_setup_generation,
        );
        assert!(setup.lines.lock().unwrap().held.is_empty());
        let fresh = Walk::new();
        for request in 11..=13 {
            setup.heard(request, fresh.sight(Some(FACE)), prints(own));
        }
        wait_held(&setup, 3);
        *fresh.owner.lock().unwrap() = Some((person.clone(), FACE.to_owned()));
        fresh.open.store(false, Ordering::SeqCst);
        wait_settled(&setup);
        let fresh_count = owner.known(&person);
        let fresh_judge = owner.judge(&person, &own(1));
        let fresh_disk = store
            .read_json_saying::<Kept>(|_| {})
            .map(|kept| kept.prints.len());
        setup.forget();
        drop(engine);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            immediate,
            (0, Confirmed::Unknown, false),
            "actual callback boundary"
        );
        assert_eq!(fresh_count, 3);
        assert!(matches!(fresh_judge, Confirmed::Theirs(_)));
        assert_eq!(fresh_disk, Some(3));
        assert_eq!(
            (after.0, persisted),
            (0, None),
            "old setup publication after app forget: {after:?}"
        );
        assert_eq!(after.1, Confirmed::Unknown);
        assert!(!after.2);
    }

    #[test]
    fn memory_forget_preserves_another_kept_owner_but_fences_pending_setup() {
        let dir = folder("memory-forget-scopes");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        let a = lamp_msgs::known::person_id("Synthetic A");
        let b = lamp_msgs::known::person_id("Synthetic B");
        let kept = Kept {
            owner: a.clone(),
            prints: vec![own(1), own(2), own(3)],
            at_unix_ms: 1,
        };
        store.write_json(&kept).unwrap();
        for request in 1..=3 {
            assert!(owner.learn(&a, request, own(request)));
        }
        let generation = setup.generation();
        for person in ["everyone", "r-0123456789abcdef", "malformed"] {
            setup.memory_forgot(Some(person));
        }
        assert_eq!(setup.generation(), generation);
        setup.memory_forgot(Some(&b));
        assert_ne!(setup.generation(), generation);
        assert_eq!(store.read_json_saying::<Kept>(|_| {}).unwrap(), kept);
        assert_eq!(owner.known(&a), 3);
        assert!(matches!(owner.judge(&a, &own(1)), Confirmed::Theirs(_)));
        setup.read_line(10, Walk::new().sight(Some(FACE)), prints(own), generation);
        assert!(setup.lines.lock().unwrap().held.is_empty());
        setup.memory_forgot(None);
        assert!(!store.path.exists());
        assert_eq!(owner.known(&a), 0);
        store.write_json(&kept).unwrap();
        owner.learn(&a, 11, own(1));
        setup.memory_forgot(Some(&a));
        assert!(!store.path.exists());
        assert_eq!(owner.known(&a), 0);
        // A malformed store has no trustworthy unrelated owner to preserve.
        store.write_json(&json!({"owner":"unknown"})).unwrap();
        setup.memory_forgot(Some(&b));
        assert!(!store.path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    use super::*;
    use crate::same_voice::{CONFIRM_BAR, Confirmed};
    use serde_json::json;
    use std::os::unix::fs::MetadataExt;

    const OWNER: &str = "person_walter";
    const FACE: &str = "b1/f3";
    const GUEST: &str = "b1/f7";

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lamp-c115-{}-{name}", std::process::id()));
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
            path: dir.join(FILE),
            uid,
        }
    }

    const FAST: Timing = Timing {
        print_within: Duration::from_millis(400),
        settle_within: Duration::from_secs(2),
        every: Duration::from_millis(5),
    };

    fn quiet() -> Journal {
        Arc::new(|_: &str| {})
    }

    fn setup(store: &Store, owner: &Arc<OwnerVoice>) -> Arc<SetupVoice> {
        Arc::new(SetupVoice::new(
            store.path.clone(),
            store.clone(),
            owner.clone(),
            quiet(),
            FAST,
        ))
    }

    /// The owner's voice, a little different each line.
    fn own(line: u64) -> Vec<f32> {
        vec![1.0, 0.05 * (line % 3) as f32, 0.0]
    }

    /// The walk: open until `closed` is set, its owner then `owner`.
    struct Walk {
        open: Arc<std::sync::atomic::AtomicBool>,
        owner: Arc<Mutex<Option<(String, String)>>>,
    }

    impl Walk {
        fn new() -> Self {
            Self {
                open: Arc::new(std::sync::atomic::AtomicBool::new(true)),
                owner: Arc::new(Mutex::new(None)),
            }
        }
        fn sight(&self, seen: Option<&str>) -> Sight {
            let (open, owner) = (self.open.clone(), self.owner.clone());
            let seen = seen.map(str::to_owned);
            Sight {
                open: Arc::new(move || open.load(std::sync::atomic::Ordering::SeqCst)),
                seen_alone: Box::new(move || seen),
                owner_now: Arc::new(move || owner.lock().unwrap().clone()),
            }
        }
        /// The step ends with `face` saved as the owner's.
        fn saved(&self, face: &str) {
            *self.owner.lock().unwrap() = Some((OWNER.to_owned(), face.to_owned()));
            self.open.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn prints(
        print: impl Fn(u64) -> Vec<f32> + Send + Sync + 'static,
    ) -> crate::second_provider::PrintOf {
        Arc::new(move |request| Some(print(request)))
    }

    /// Until `file` is there, or 5 s.
    fn wait_for(file: &Path) {
        let until = Instant::now() + Duration::from_secs(5);
        while !file.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Until `held` prints are held, or 5 s.
    fn wait_held(setup: &SetupVoice, held: usize) {
        let until = Instant::now() + Duration::from_secs(5);
        while setup.lines.lock().unwrap().held.len() < held && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_stale_settler_cannot_drain_or_clear_a_fresh_walk_after_forget() {
        stale_settler_after_redo(None);
    }

    #[test]
    fn a_stale_settler_cannot_drain_a_fresh_walk_after_ordinary_redo() {
        stale_settler_after_redo(Some("who_you_are"));
        stale_settler_after_redo(Some(crate::onboarding::REDO_ALL));
    }

    fn stale_settler_after_redo(redo: Option<&str>) {
        use std::sync::mpsc;
        let dir = folder(&format!("stale-settler-{}", redo.unwrap_or("forget")));
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        let old = Walk::new();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let mut sight = old.sight(Some(FACE));
        sight.owner_now = Arc::new(move || {
            // Observe the old owner, then model descheduling before the
            // settler can take the lines lock and publish that decision.
            let result = Some((OWNER.to_owned(), FACE.to_owned()));
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            result
        });
        let reader = setup.clone();
        let generation = setup.generation.load(Ordering::Acquire);
        let old_thread = std::thread::spawn(move || {
            reader.read_line(1, sight, prints(own), generation);
        });
        wait_held(&setup, 1);
        old.saved(FACE);
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        if let Some(step) = redo {
            register(store.clone(), Some(setup.clone()));
            assert_eq!(redo_tool(&setup.onboarding, step).0["ok"], true);
        } else {
            setup.forget();
        }

        // A new walk must obtain a settler while the old callback is still
        // blocked, and that old callback must not consume its three prints.
        let fresh = Walk::new();
        for request in 2..=4 {
            setup.heard(request, fresh.sight(Some(FACE)), prints(own));
        }
        wait_held(&setup, 3);
        release_tx.send(()).unwrap();
        old_thread.join().unwrap();
        {
            let lines = setup.lines.lock().unwrap();
            assert_eq!(lines.held.len(), 3);
            assert!(lines.settling);
        }
        assert!(!store.path.exists());
        assert_eq!(owner.known(OWNER), 0);

        // Control: the fresh walk's own waiter still completes normally,
        // with its new owner, without needing another heard line.
        let new_owner = "p-fresh-owner";
        *fresh.owner.lock().unwrap() = Some((new_owner.to_owned(), FACE.to_owned()));
        fresh.open.store(false, Ordering::SeqCst);
        wait_settled(&setup);
        let kept: Kept = store.read_json_saying(|_| {}).unwrap();
        assert_eq!(kept.owner, new_owner);
        assert_eq!(kept.prints.len(), 3);
        assert_eq!(owner.known(new_owner), 3);
        assert_eq!(owner.known(OWNER), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn redo_tool(file: &Path, step: &str) -> (Value, bool) {
        crate::onboarding::redo_for_setup_test(file, step)
    }

    #[test]
    fn redo_only_invalidates_enrolment_when_the_owner_walk_is_posted() {
        let dir = folder("redo-post-boundary");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        register(store.clone(), Some(setup.clone()));
        for step in ["who_you_are", crate::onboarding::REDO_ALL] {
            store
                .write_json(&Kept {
                    owner: OWNER.into(),
                    prints: vec![own(1), own(2), own(3)],
                    at_unix_ms: 1,
                })
                .unwrap();
            assert_eq!(setup.load(Some(OWNER)), 3);
            {
                let mut lines = setup.lines.lock().unwrap();
                lines.held.push(Held {
                    request: 7,
                    face: FACE.into(),
                    print: own(7),
                });
                lines.settling = true;
            }
            let generation = setup.generation.load(Ordering::Acquire);
            // Other steps and refusals must not reset a known voice.
            assert_eq!(redo_tool(&setup.onboarding, "privacy").0["ok"], Value::Null);
            assert_eq!(
                redo_tool(&setup.onboarding, "not-a-step").0["ok"],
                Value::Null
            );
            assert_eq!(redo_tool(&setup.onboarding, "where_you_sit").0["ok"], true);
            let mailbox = crate::onboarding::mailbox_of(&setup.onboarding);
            std::fs::write(&mailbox, vec![b' '; 16 * 1024]).unwrap();
            assert!(!redo_tool(&setup.onboarding, step).1);
            assert_eq!(setup.generation.load(Ordering::Acquire), generation);
            assert!(store.path.exists());
            assert_eq!(owner.known(OWNER), 3);
            {
                let lines = setup.lines.lock().unwrap();
                assert_eq!(lines.held.len(), 1);
                assert!(lines.settling);
            }
            std::fs::remove_file(mailbox).unwrap();
            assert_eq!(redo_tool(&setup.onboarding, step).0["ok"], true);
            assert_eq!(setup.generation.load(Ordering::Acquire), generation + 1);
            assert!(!store.path.exists());
            assert_eq!(owner.known(OWNER), 0);
            let lines = setup.lines.lock().unwrap();
            assert!(lines.held.is_empty());
            assert!(!lines.settling);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// C115: the owner's prints taken in "who you are" are kept, and after a
    /// restart (a new worker's own prints, empty) given back, so the owner's
    /// first line no face was seen speaking is judged theirs.
    #[test]
    fn a_print_taken_at_setup_is_kept_and_the_first_line_after_a_restart_is_known() {
        let dir = folder("kept");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        let walk = Walk::new();
        for request in 1..=3 {
            setup.heard(request, walk.sight(Some(FACE)), prints(own));
        }
        wait_held(&setup, 3);
        walk.saved(FACE);
        wait_for(&store.path);
        // Written whole, 0600, prints only.
        std::thread::sleep(Duration::from_millis(50));
        let meta = std::fs::metadata(&store.path).unwrap();
        assert_eq!(meta.mode() & 0o7777, owner_voice::MODE);
        let kept: Value = serde_json::from_slice(&std::fs::read(&store.path).unwrap()).unwrap();
        assert_eq!(kept["owner"], OWNER);
        assert_eq!(kept["prints"].as_array().unwrap().len(), 3);
        let mut keys: Vec<_> = kept.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["at_unix_ms", "owner", "prints"]);
        // The voice starts again: nothing in RAM until the kept prints come back.
        let restarted = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        assert_eq!(restarted.judge(OWNER, &own(9)), Confirmed::Unknown);
        let again = SetupVoice::new(
            store.path.clone(),
            store.clone(),
            restarted.clone(),
            quiet(),
            FAST,
        );
        assert_eq!(again.load(Some(OWNER)), 3);
        assert!(matches!(
            restarted.judge(OWNER, &own(9)),
            Confirmed::Theirs(_)
        ));
        // Another owner named by the walk: not used.
        let other = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let theirs = SetupVoice::new(
            store.path.clone(),
            store.clone(),
            other.clone(),
            quiet(),
            FAST,
        );
        assert_eq!(theirs.load(Some("person_someone")), 0);
        assert_eq!(theirs.load(None), 0);
        assert_eq!(other.known(OWNER), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A guest's face, or a line no face alone was seen saying, at setup
    /// takes no print: only the saved owner's face is kept.
    #[test]
    fn a_guest_or_an_unseen_speaker_at_setup_takes_no_print() {
        let dir = folder("guest");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        let walk = Walk::new();
        // Unseen: nothing held at all.
        setup.heard(1, walk.sight(None), prints(own));
        // The guest's face alone.
        setup.heard(2, walk.sight(Some(GUEST)), prints(own));
        wait_held(&setup, 1);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(setup.lines.lock().unwrap().held.len(), 1);
        walk.saved(FACE);
        let until = Instant::now() + Duration::from_secs(5);
        while setup.lines.lock().unwrap().settling && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!store.path.exists());
        assert_eq!(owner.known(OWNER), 0);
        // A step that ends with no face saved keeps nothing either.
        let walk = Walk::new();
        setup.heard(3, walk.sight(Some(FACE)), prints(own));
        wait_held(&setup, 1);
        walk.open.store(false, std::sync::atomic::Ordering::SeqCst);
        // No owner ever: the settle runs out (FAST) and drops it.
        let until = Instant::now() + Duration::from_secs(8);
        while setup.lines.lock().unwrap().settling && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(setup.lines.lock().unwrap().held.is_empty());
        assert!(!store.path.exists());
        // A line heard once the step is over is not read at all.
        setup.heard(4, walk.sight(Some(FACE)), prints(own));
        std::thread::sleep(Duration::from_millis(50));
        assert!(setup.lines.lock().unwrap().held.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The owner's "forget everything" removes the kept prints and those in
    /// RAM; another person's, or one that emptied nothing, leaves them.
    #[test]
    fn forget_everything_deletes_the_kept_prints() {
        let dir = folder("forget");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        store
            .write_json(&Kept {
                owner: OWNER.into(),
                prints: vec![own(1), own(2), own(3)],
                at_unix_ms: 1,
            })
            .unwrap();
        let setup = setup(&store, &owner);
        assert_eq!(setup.load(Some(OWNER)), 3);
        register(store.clone(), Some(setup.clone()));
        let owners = [OWNER.to_owned()];
        let emptied = (json!({"forgotten": true}), true);
        // Someone else's forget: kept.
        let told = forget_if(emptied.clone(), Some("person_guest"), &owners);
        assert_eq!(told.0, emptied.0);
        assert!(store.path.exists());
        // Nothing emptied: kept.
        let told = forget_if((json!({"forgotten": false}), true), Some(OWNER), &owners);
        assert_eq!(told.0["voice_print"], Value::Null);
        assert!(store.path.exists());
        // The owner's: gone, file and RAM.
        let told = forget_if(emptied, Some(OWNER), &owners);
        assert_eq!(told.0["voice_print"], "forgotten too");
        assert!(!store.path.exists());
        assert_eq!(owner.known(OWNER), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Off (unset or anything but `on`): no enrolment is made, so nothing is
    /// held, kept or given back, and the known person's own prints start
    /// empty, as before.
    #[test]
    fn switch_off_nothing_is_taken_or_given_back() {
        assert!(!on_from(|_| None));
        assert!(!on_from(|_| Some("yes".into())));
        assert!(on_from(|name| {
            (name == same_voice::ENROLS_AT_SETUP_ENVIRONMENT).then(|| "on".into())
        }));
        let dir = folder("off");
        let onboarding = dir.join("progress.json");
        let store = store_beside(&onboarding).unwrap();
        assert_eq!(store.path, dir.join(FILE));
        // A file an earlier on-period kept is removed at start (Jarvis's
        // P3 on #5582).
        std::fs::write(&store.path, b"{}").unwrap();
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        // `from_environment` with the switch unset in the test's environment.
        assert!(std::env::var(same_voice::ENROLS_AT_SETUP_ENVIRONMENT).is_err());
        assert!(from_environment(Some(&onboarding), &owner).is_none());
        assert!(from_environment(None, &owner).is_none());
        assert_eq!(owner.known(OWNER), 0);
        assert!(!store.path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Until no settle waits, or 8 s.
    fn wait_settled(setup: &SetupVoice) {
        let until = Instant::now() + Duration::from_secs(8);
        while setup.lines.lock().unwrap().settling && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A kept file of the owner's three prints at `store`.
    fn kept_three(store: &Store) {
        store
            .write_json(&Kept {
                owner: OWNER.into(),
                prints: vec![own(1), own(2), own(3)],
                at_unix_ms: 1,
            })
            .unwrap();
    }

    /// Jarvis's P2 on #5582: A sets up and their prints are kept; B redoes
    /// the walk and is never seen speaking alone, so nothing of B's is
    /// held. A's file goes all the same, as "who you are" opens again, so
    /// no former owner's prints stay that A's forget could no longer reach.
    /// A step that ends keeping nothing removes it too, and so does a start
    /// that finds another owner's, or the step open again.
    #[test]
    fn a_redo_by_someone_not_seen_alone_leaves_no_former_owners_prints() {
        let dir = folder("redo");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        // A's walk.
        let walk = Walk::new();
        for request in 1..=3 {
            setup.heard(request, walk.sight(Some(FACE)), prints(own));
        }
        wait_held(&setup, 3);
        walk.saved(FACE);
        wait_for(&store.path);
        wait_settled(&setup);
        assert!(store.path.exists());
        // B's redo: open again, B's line seen by no face alone.
        let redo = Walk::new();
        setup.heard(4, redo.sight(None), prints(own));
        let until = Instant::now() + Duration::from_secs(5);
        while store.path.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!store.path.exists(), "A's prints outlived the redo");
        assert!(setup.lines.lock().unwrap().held.is_empty());
        // A step that ends with only a guest's lines keeps nothing, and a
        // file there by then goes.
        let walk = Walk::new();
        setup.heard(5, walk.sight(Some(GUEST)), prints(own));
        wait_held(&setup, 1);
        kept_three(&store);
        walk.saved(FACE);
        wait_settled(&setup);
        assert!(!store.path.exists());
        // One that ends with no face saved, the same.
        let walk = Walk::new();
        setup.heard(6, walk.sight(Some(FACE)), prints(own));
        wait_held(&setup, 1);
        kept_three(&store);
        walk.open.store(false, std::sync::atomic::Ordering::SeqCst);
        wait_settled(&setup);
        assert!(!store.path.exists());
        // A start that finds another owner's, or none named: removed.
        kept_three(&store);
        let fresh = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        assert_eq!(self::setup(&store, &fresh).load(Some("person_b")), 0);
        assert!(!store.path.exists());
        kept_three(&store);
        assert_eq!(self::setup(&store, &fresh).load(None), 0);
        assert!(!store.path.exists());
        // A start while the walk is open again: removed, even the owner's.
        let progress = dir.join("progress.json");
        std::fs::write(&progress, br#"{"steps":{}}"#).unwrap();
        kept_three(&store);
        let open = SetupVoice::new(progress, store.clone(), fresh.clone(), quiet(), FAST);
        assert_eq!(open.load(Some(OWNER)), 0);
        assert!(!store.path.exists());
        assert_eq!(fresh.known(OWNER), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Jarvis's P3 on #5582: the owner's "forget everything" while the step
    /// is open takes the prints held for it and any line's in flight, so
    /// the settle at the step's end writes nothing back.
    #[test]
    fn a_forget_before_the_settle_is_not_undone_by_it() {
        let dir = folder("race");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        register(store.clone(), Some(setup.clone()));
        let walk = Walk::new();
        for request in 1..=2 {
            setup.heard(request, walk.sight(Some(FACE)), prints(own));
        }
        wait_held(&setup, 2);
        // A line whose print is still coming when the forget is made.
        let slow: crate::second_provider::PrintOf = Arc::new(|request| {
            std::thread::sleep(Duration::from_millis(150));
            Some(own(request))
        });
        setup.heard(3, walk.sight(Some(FACE)), slow);
        std::thread::sleep(Duration::from_millis(30));
        let owners = [OWNER.to_owned()];
        forget_if((json!({"forgotten": true}), true), Some(OWNER), &owners);
        assert!(setup.lines.lock().unwrap().held.is_empty());
        std::thread::sleep(Duration::from_millis(250));
        assert!(
            setup.lines.lock().unwrap().held.is_empty(),
            "the line in flight was held after the forget"
        );
        walk.saved(FACE);
        wait_settled(&setup);
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !store.path.exists(),
            "the settle wrote the forgotten prints"
        );
        assert_eq!(owner.known(OWNER), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Jarvis's P3 on #5582: a face forgotten (the walk's owner mark goes,
    /// whoever's face) removes the kept prints and those held, and with no
    /// enrolment registered the file beside the walk's progress.
    #[test]
    fn a_face_forgotten_removes_the_kept_prints() {
        let dir = folder("face");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let progress = dir.join("progress.json");
        let setup = Arc::new(SetupVoice::new(
            progress.clone(),
            store.clone(),
            owner.clone(),
            quiet(),
            FAST,
        ));
        kept_three(&store);
        assert_eq!(setup.load(Some(OWNER)), 3);
        register(store.clone(), Some(setup.clone()));
        owner_face_gone(&progress);
        assert!(!store.path.exists());
        assert_eq!(owner.known(OWNER), 0);
        // Switch off: none registered, the file beside the walk's progress.
        register(store.clone(), None);
        kept_three(&store);
        owner_face_gone(&progress);
        assert!(!store.path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A journal kept for the test to read.
    fn noted() -> (Journal, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let into = lines.clone();
        let journal: Journal =
            Arc::new(move |line: &str| into.lock().unwrap().push(line.to_owned()));
        (journal, lines)
    }

    /// How many prints the kept file at `store` holds.
    fn kept_prints(store: &Store) -> usize {
        let kept: Value = serde_json::from_slice(&std::fs::read(&store.path).unwrap()).unwrap();
        kept["prints"].as_array().unwrap().len()
    }

    /// Reviewer-two's P2 on #5582: three of the owner's prints are held, a
    /// fourth line's print is still coming (held at a barrier) when the
    /// step ends and the walk is settled with the three. Once that print
    /// comes it holds none: it never starts a settle of its own that would
    /// replace the three with itself alone, and a fresh start still knows
    /// the owner's voice.
    #[test]
    fn a_print_that_comes_after_its_walk_settled_does_not_replace_the_kept_ones() {
        let dir = folder("late");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let (journal, noted) = noted();
        let setup = Arc::new(SetupVoice::new(
            store.path.clone(),
            store.clone(),
            owner.clone(),
            journal,
            FAST,
        ));
        let walk = Walk::new();
        for request in 1..=3 {
            setup.heard(request, walk.sight(Some(FACE)), prints(own));
        }
        wait_held(&setup, 3);
        let (entered, at_barrier) = std::sync::mpsc::channel::<()>();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let (entered, released) = (Mutex::new(entered), Mutex::new(released));
        let late: crate::second_provider::PrintOf = Arc::new(move |request| {
            let _ = entered.lock().unwrap().send(());
            let _ = released.lock().unwrap().recv();
            Some(own(request))
        });
        setup.heard(4, walk.sight(Some(FACE)), late);
        at_barrier
            .recv_timeout(Duration::from_secs(5))
            .expect("the fourth line's print was never asked for");
        walk.saved(FACE);
        wait_for(&store.path);
        wait_settled(&setup);
        assert_eq!(kept_prints(&store), 3);
        // The fourth print comes now.
        release.send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !noted
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains("request 4:"))
            && Instant::now() < until
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        wait_settled(&setup);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            kept_prints(&store),
            3,
            "a late print replaced the kept ones"
        );
        let restarted = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        assert_eq!(self::setup(&store, &restarted).load(Some(OWNER)), 3);
        assert!(matches!(
            restarted.judge(OWNER, &own(9)),
            Confirmed::Theirs(_)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Reviewer-two's P3 on #5582: "who you are" open again takes the old
    /// walk's prints from RAM at the same time as from disk, and so does a
    /// step that ends keeping nothing.
    #[test]
    fn a_redo_or_a_walk_keeping_nothing_clears_the_old_prints_in_ram() {
        let dir = folder("ram");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        kept_three(&store);
        assert_eq!(setup.load(Some(OWNER)), 3);
        // A redo, its line seen by no face alone, read where it is heard.
        let redo = Walk::new();
        setup.read_line(
            1,
            redo.sight(None),
            prints(own),
            setup.generation.load(Ordering::Acquire),
        );
        assert!(!store.path.exists());
        assert_eq!(
            owner.known(OWNER),
            0,
            "the old walk's prints still usable in RAM"
        );
        // A step that ends with only a guest's lines held.
        let walk = Walk::new();
        setup.heard(2, walk.sight(Some(GUEST)), prints(own));
        wait_held(&setup, 1);
        for at in 0..3 {
            owner.learn(OWNER, 100 + at, own(at));
        }
        walk.saved(FACE);
        wait_settled(&setup);
        assert_eq!(owner.known(OWNER), 0);
        // One that ends with no face saved.
        let walk = Walk::new();
        setup.heard(3, walk.sight(Some(FACE)), prints(own));
        wait_held(&setup, 1);
        for at in 0..3 {
            owner.learn(OWNER, 200 + at, own(at));
        }
        walk.open.store(false, std::sync::atomic::Ordering::SeqCst);
        wait_settled(&setup);
        assert_eq!(owner.known(OWNER), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The step's path waits for nothing: a line is handed over at once,
    /// however slow the onboarding file, the eyes and the print are.
    #[test]
    fn a_line_is_handed_over_without_waiting_for_its_print() {
        let dir = folder("latency");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        let slow = Duration::from_millis(300);
        // A slow redo post holds the enrolment mutex, but never the heard
        // handoff. Refuse the synthetic post so these readers remain current.
        let (entered, posting) = std::sync::mpsc::channel();
        let blocked = setup.clone();
        let registered_store = store.clone();
        let redo = std::thread::spawn(move || {
            register(registered_store, Some(blocked.clone()));
            post_redo(&blocked.onboarding, || {
                entered.send(()).unwrap();
                std::thread::sleep(slow);
                Err(std::io::Error::other("synthetic slow post refused"))
            })
            .unwrap_err();
        });
        posting.recv_timeout(Duration::from_secs(5)).unwrap();
        for request in 1..=3 {
            let sight = Sight {
                open: Arc::new(move || {
                    std::thread::sleep(slow);
                    true
                }),
                seen_alone: Box::new(move || {
                    std::thread::sleep(slow);
                    Some(FACE.to_owned())
                }),
                owner_now: Arc::new(|| None),
            };
            let print_of: crate::second_provider::PrintOf = Arc::new(move |request| {
                std::thread::sleep(slow);
                Some(own(request))
            });
            let at = Instant::now();
            let generation = setup.generation();
            setup.heard_at(generation, request, sight, print_of);
            let took = at.elapsed();
            eprintln!("setup voice: generation + heard_at({request}) returned in {took:?}");
            assert!(took < Duration::from_millis(50), "{took:?}");
        }
        redo.join().unwrap();
        wait_held(&setup, 3);
        assert_eq!(setup.lines.lock().unwrap().held.len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A stale result of the first onboarding read must not take the next
    /// walk's epoch and replace the batch that settled while it was read.
    #[test]
    fn a_late_open_observation_cannot_replace_a_settled_walk() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = folder("early-open-settle");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        let walk = Walk::new();
        for request in 1..=3 {
            setup.heard(request, walk.sight(Some(FACE)), prints(own));
        }
        wait_held(&setup, 3);
        assert_eq!(setup.lines.lock().unwrap().held.len(), 3);
        let (entered, at_barrier) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let released = Mutex::new(released);
        let mut sight = walk.sight(Some(FACE));
        let current = sight.open.clone();
        let first = AtomicBool::new(true);
        sight.open = Arc::new(move || {
            let observed = current();
            if first.swap(false, Ordering::SeqCst) {
                assert!(observed);
                entered.send(()).unwrap();
                released
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            observed
        });
        let reader = {
            let generation = setup.generation.load(Ordering::Acquire);
            let setup = setup.clone();
            std::thread::spawn(move || setup.read_line(4, sight, prints(own), generation))
        };
        at_barrier.recv_timeout(Duration::from_secs(5)).unwrap();
        walk.saved(FACE);
        wait_settled(&setup);
        assert_eq!(kept_prints(&store), 3);
        release.send(()).unwrap();
        reader.join().unwrap();
        let saved = kept_prints(&store);
        let restarted = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let loaded = self::setup(&store, &restarted).load(Some(OWNER));
        let judged = restarted.judge(OWNER, &own(9));
        eprintln!(
            "settled=3 after_late_open={saved} restart_loaded={loaded} restart_judgment={judged:?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(
            saved, 3,
            "a pre-settlement open observation captured the post-settlement epoch"
        );
        assert_eq!(loaded, 3);
        assert!(matches!(judged, Confirmed::Theirs(_)));
    }

    /// A forget during the first onboarding read must fence that line too,
    /// before its stale open observation can begin a new batch.
    #[test]
    fn a_late_open_observation_cannot_restore_forgotten_prints() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = folder("early-open-forget");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        kept_three(&store);
        assert_eq!(setup.load(Some(OWNER)), 3);
        let walk = Walk::new();
        let (entered, at_barrier) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let released = Mutex::new(released);
        let mut sight = walk.sight(Some(FACE));
        let current = sight.open.clone();
        let first = AtomicBool::new(true);
        sight.open = Arc::new(move || {
            let observed = current();
            if first.swap(false, Ordering::SeqCst) {
                assert!(observed);
                entered.send(()).unwrap();
                released
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            observed
        });
        let reader = {
            let generation = setup.generation.load(Ordering::Acquire);
            let setup = setup.clone();
            std::thread::spawn(move || setup.read_line(4, sight, prints(own), generation))
        };
        at_barrier.recv_timeout(Duration::from_secs(5)).unwrap();
        setup.forget();
        assert!(!store.path.exists());
        assert_eq!(owner.known(OWNER), 0);
        // Even an owner returned by a later sight cannot authorize this
        // pre-forget line to restore its print.
        walk.saved(FACE);
        release.send(()).unwrap();
        reader.join().unwrap();
        assert!(!store.path.exists());
        assert_eq!(owner.known(OWNER), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A line queued before settlement must not join a quick redo, even
    /// when its background reader starts only after the new step opens.
    #[test]
    fn a_queued_line_cannot_join_a_redo_after_its_walk_settled() {
        let dir = folder("queued-redo");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        let walk = Walk::new();
        for request in 1..=3 {
            setup.heard(request, walk.sight(Some(FACE)), prints(own));
        }
        wait_held(&setup, 3);
        assert_eq!(setup.lines.lock().unwrap().held.len(), 3);
        // These are the values heard() hands to its spawned closure. Hold
        // that closure's execution until the old walk settles and reopens.
        let generation = setup.generation.load(Ordering::Acquire);
        let sight = walk.sight(Some(FACE));
        walk.saved(FACE);
        wait_settled(&setup);
        assert_eq!(kept_prints(&store), 3);
        walk.open.store(true, Ordering::SeqCst);
        setup.read_line(4, sight, prints(own), generation);
        assert_eq!(kept_prints(&store), 3);
        assert_eq!(owner.known(OWNER), 3);
        assert!(setup.lines.lock().unwrap().held.is_empty());
        // A newly admitted, unseen redo line still clears the former walk.
        setup.read_line(
            5,
            walk.sight(None),
            prints(own),
            setup.generation.load(Ordering::Acquire),
        );
        assert!(!store.path.exists());
        assert_eq!(owner.known(OWNER), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
