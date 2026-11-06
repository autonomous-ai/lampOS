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
//!   ([`SetupVoice::heard`] returns at once), and the step's end is waited
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
//!   removed when "who you are" opens again (a redo's first line heard), when
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
//!   voice's start ([`switched_off`]).
//! - **The journal holds counts only,** never a print.
use crate::owner_voice::{self, Store};
use crate::same_voice::{self, Journal, OWNER_PRINTS, OwnerVoice};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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

    /// The line `request` was heard: read in a thread of its own, never on
    /// the step's path. Its print, once it comes from `print_of`, is held
    /// while the step is open and one face alone was seen saying it.
    pub fn heard(
        self: &Arc<Self>,
        request: u64,
        sight: Sight,
        print_of: crate::second_provider::PrintOf,
    ) {
        let setup = self.clone();
        let started = std::thread::Builder::new()
            .name("setup-voice".into())
            .spawn(move || setup.read_line(request, sight, print_of));
        if let Err(error) = started {
            self.say(&format!("request {request}: not read: {}", error.kind()));
        }
    }

    fn read_line(&self, request: u64, sight: Sight, print_of: crate::second_provider::PrintOf) {
        let Sight {
            open,
            seen_alone,
            owner_now,
        } = sight;
        if !open() {
            return;
        }
        // Nothing held and no settle waiting: the step is open again (a
        // redo) or for the first time, and a file kept by an earlier walk's
        // owner goes now, whoever this walk saves (Jarvis's P2 on #5582).
        let forgets = {
            let Ok(lines) = self.lines.lock() else {
                return;
            };
            if !lines.settling && lines.held.is_empty() {
                self.wipe("\"who you are\" is open again");
            }
            lines.forgets
        };
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
            self.settle(&*open, &*owner_now);
        }
    }

    /// Wait for the step's end, then for the owner's saved face, and keep
    /// the prints of that face alone; everything held goes.
    fn settle(&self, open: &dyn Fn() -> bool, owner_now: &dyn Fn() -> Option<(String, String)>) {
        let until = Instant::now() + self.timing.settle_within;
        let owner = loop {
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
        lines.settling = false;
        let held = std::mem::take(&mut lines.held);
        let Some((owner, face)) = owner else {
            self.say(&format!(
                "no owner's face saved by the step's end: {} held prints dropped",
                held.len()
            ));
            self.wipe("nothing kept by this walk");
            return;
        };
        let theirs: Vec<Held> = held
            .into_iter()
            .filter(|line| line.face == face)
            .take(OWNER_PRINTS)
            .collect();
        if theirs.is_empty() {
            self.say("no line seen said by the owner's saved face: none kept");
            self.wipe("nothing kept by this walk");
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
                self.wipe("nothing kept by this walk");
                return;
            }
        }
        for line in theirs {
            self.owner.learn(&owner, line.request, line.print);
        }
        drop(lines);
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
        lines.held.clear();
        lines.forgets = lines.forgets.wrapping_add(1);
        self.wipe("forgotten");
        self.owner.forget();
        drop(lines);
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

    /// The step's path waits for nothing: a line is handed over at once,
    /// however slow the onboarding file, the eyes and the print are.
    #[test]
    fn a_line_is_handed_over_without_waiting_for_its_print() {
        let dir = folder("latency");
        let store = store(&dir);
        let owner = Arc::new(OwnerVoice::new(CONFIRM_BAR));
        let setup = setup(&store, &owner);
        let slow = Duration::from_millis(300);
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
        let print_of: crate::second_provider::PrintOf = Arc::new(move |_| {
            std::thread::sleep(slow);
            Some(own(1))
        });
        let at = Instant::now();
        setup.heard(1, sight, print_of);
        let took = at.elapsed();
        eprintln!("setup voice: heard() returned in {took:?}");
        assert!(took < Duration::from_millis(50), "{took:?}");
        wait_held(&setup, 1);
        assert_eq!(setup.lines.lock().unwrap().held.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
