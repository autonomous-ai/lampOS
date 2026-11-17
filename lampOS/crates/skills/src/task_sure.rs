//! A skill's `do.task` made sure of (`LAMP_SKILL_TASK_SURE`, `on` only,
//! off unless set, on in the product's lamp-skills): two ways its hand-over to
//! lamp-tasks ([`crate::task::hand`]) ended with nothing said.
//!
//! - **A run that failed is told** ([`failed_line`]): lamp-tasks' `ok:false`
//!   answer, or none, is said to the owner as one short fixed line, behind
//!   a told answer's gate (`crate::service_read::tell`, so quiet hours and
//!   the rest hold it), never the task's words nor lamp-tasks' own. A key
//!   not added (`missing_secret`) says what to do in the app; any other
//!   refusal before its agent began says it couldn't run, and a run that
//!   began and failed, or whose answer never came, that it didn't finish.
//!   Before, only the journal had it, even after the voice said "<skill>
//!   is running now" (`lamp_voice::made_run`).
//! - **A run refused `busy` waits its turn** ([`Line`], [`hand_in_turn`]):
//!   lamp-tasks runs one skill's task at a time and refuses another as
//!   `busy` (`lamp_tasks::skill_task::BUSY`), and the run was taken back off
//!   the ledger and lost, so of two due at one minute (a morning's mail and
//!   its calendar, both at 8:00) one never ran. It waits in one line for
//!   the lamp, at most [`MOST_WAITING`] in it (one being handed over from
//!   it among them), first in first out, each at most [`LONGEST_WAIT`]
//!   from when it began to wait, and is handed over again as soon as the hand-over
//!   under way ends ([`Line::ended`]) or after [`RETRY_EVERY`]. A task due
//!   while one waits goes behind it, so none jumps the line. Past the
//!   bound or the wait it is told ([`dropped_line`]); a skill whose last
//!   task waits already is only journalled. It waits on its own hand-over
//!   thread, never the program's loop, and holds nothing while it waits
//!   (no token, no file; it is counted only as it is handed over, and a
//!   `busy` try is taken back off the ledger as before), so a stop of
//!   lamp-skills just ends it.
//! - **Off**: one try, nothing said of a failure, as before, word for word.
use crate::service_write::Unrun;
use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

/// The switch, in `lamp-skills` alone. `on` only.
pub const ENVIRONMENT: &str = "LAMP_SKILL_TASK_SURE";

#[cfg(test)]
thread_local! {
    /// The switch as a test of this thread sets it.
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether [`ENVIRONMENT`] is on in this process.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The switch for this test's thread alone.
#[cfg(test)]
pub fn set_on_here(on: Option<bool>) {
    ON_HERE.with(|here| here.set(on));
}

/// The most tasks in line at once, one being handed over from it among
/// them: so never more than this wait, even when that one is refused
/// `busy` again and waits once more.
pub const MOST_WAITING: usize = 4;
/// The longest a task waits for its turn, from when it began to wait.
pub const LONGEST_WAIT: Duration = Duration::from_secs(15 * 60);
/// How often the first in line is handed over again with no end seen, when
/// lamp-tasks said `busy` with no hand-over of lamp-skills' under way.
pub const RETRY_EVERY: Duration = Duration::from_secs(10);

/// A skill's name as said: its dashes and underscores spaces, its first
/// letter a capital ("morning-mail", "Morning mail").
pub fn spoken_name(skill: &str) -> String {
    let words = skill.replace(['-', '_'], " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => words,
    }
}

/// What is said of `skill`'s run lamp-tasks' answer (`sent`, as
/// `lamp-skills` read it) says failed, or none for a run that ended well or
/// one refused `busy` (that one waits its turn, [`hand_in_turn`]). One
/// fixed line a reason, never the task's words nor lamp-tasks' message.
pub fn failed_line(skill: &str, sent: &Result<String, Unrun>) -> Option<String> {
    let name = spoken_name(skill);
    let not_run = || Some(format!("{name} couldn't run this time."));
    let not_finished = || Some(format!("{name} didn't finish this time."));
    let answer = match sent {
        Ok(answer) => answer,
        // Not reached: nothing of it began.
        Err(Unrun::NotTaken(_)) => return not_run(),
        // Written and no answer read: it may have begun.
        Err(Unrun::Unknown(_)) => return not_finished(),
        // Any other way a run wasn't made (#7109 adds `Unrun::NotMade`,
        // Harness never made the run): nothing of it began. Kept so this
        // builds with and without that variant, hence the allow.
        #[allow(unreachable_patterns)]
        Err(_) => return not_run(),
    };
    let Ok(answer) = serde_json::from_str::<serde_json::Value>(answer.trim()) else {
        return not_finished();
    };
    if answer.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return None;
    }
    match answer["error"].as_str().unwrap_or("") {
        "busy" => None,
        "missing_secret" => Some(format!(
            "{name} couldn't run: it needs a key added in the app, on its page."
        )),
        "off" | "bad_request" | "too_long" | "no_token" | "not_started" | "no_secrets" => not_run(),
        _ => not_finished(),
    }
}

/// Whether lamp-tasks refused the run `busy`: another skill's runs.
pub fn is_busy(sent: &Result<String, Unrun>) -> bool {
    sent.as_ref().is_ok_and(|answer| {
        serde_json::from_str::<serde_json::Value>(answer.trim())
            .is_ok_and(|answer| answer["error"] == "busy")
    })
}

/// What one hand-over try's `send` saw (whether lamp-tasks said `busy`, and
/// the failed run's line, [`failed_line`]), made afresh at each try
/// ([`Tried::once`]): a try that returns before its `send` (its folder
/// unreadable, its yes changed or withdrawn, its bounds spent) never keeps
/// an earlier try's `busy`, so it leaves the line at once with its own
/// reason (Twelve's HOLD on #7239).
#[derive(Default)]
pub struct Tried {
    busy: std::cell::Cell<bool>,
    failed: std::cell::Cell<Option<String>>,
}

impl Tried {
    /// lamp-tasks' answer to this try (`sent`), as `send` read it.
    pub fn sent(&self, skill: &str, sent: &Result<String, Unrun>) {
        self.busy.set(is_busy(sent));
        self.failed.set(failed_line(skill, sent));
    }

    /// One try through `hand`, which may return before its `send`: its
    /// journal line, and whether this try, never an earlier, was refused
    /// `busy`.
    pub fn once(&self, hand: impl FnOnce() -> String) -> (String, bool) {
        self.busy.set(false);
        self.failed.set(None);
        let said = hand();
        (said, self.busy.get())
    }

    /// The last try's failed line, or one [`Tried::tell`] kept, taken once.
    pub fn failed(&self) -> Option<String> {
        self.failed.take()
    }

    /// `line` to tell instead (a task let go, [`dropped_line`]).
    pub fn tell(&self, line: String) {
        self.failed.set(Some(line));
    }
}

/// What is said of `skill`'s task that never got its turn ([`Dropped::tells`]).
pub fn dropped_line(skill: &str) -> String {
    format!(
        "{} didn't run this time: the lamp was busy with other skills.",
        spoken_name(skill)
    )
}

/// Why a task waiting for its turn was let go: the journal's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// [`MOST_WAITING`] wait already.
    Full,
    /// The same skill's last task waits already: that one runs.
    Waits,
    /// Its turn didn't come within [`LONGEST_WAIT`].
    Lapsed,
}

impl Dropped {
    /// The journal's words for it.
    pub fn words(self) -> String {
        match self {
            Self::Full => format!(
                "do.task: not handed over: lamp-tasks is busy and {MOST_WAITING} tasks wait already ({ENVIRONMENT})"
            ),
            Self::Waits => format!(
                "do.task: not handed over: its last task waits its turn already ({ENVIRONMENT})"
            ),
            Self::Lapsed => format!(
                "do.task: not handed over: its turn didn't come in {} minutes ({ENVIRONMENT})",
                LONGEST_WAIT.as_secs() / 60
            ),
        }
    }

    /// Whether the owner is told ([`dropped_line`]): not for a skill whose
    /// last task still waits, which runs.
    pub fn tells(self) -> bool {
        !matches!(self, Self::Waits)
    }
}

/// The tasks waiting for lamp-tasks, in order.
struct Waiting {
    places: VecDeque<Waiter>,
    next: u64,
    /// Counted up as each hand-over ends ([`Line::ended`]) and each place
    /// is let go: the first in line tries again on a change.
    ends: u64,
}

struct Waiter {
    id: u64,
    skill: String,
    /// Being handed over now: first in line, still counted in the bound.
    trying: bool,
}

/// One line of tasks waiting for their turn at lamp-tasks: at most `most`
/// in it, one being handed over among them, first in first out, each at most `longest`, the first tried
/// again on an end or after `every`. The program's is [`LAMP`].
pub struct Line {
    waiting: Mutex<Waiting>,
    changed: Condvar,
    most: usize,
    longest: Duration,
    every: Duration,
}

/// The lamp's one line, for every skill's task ([`MOST_WAITING`],
/// [`LONGEST_WAIT`], [`RETRY_EVERY`]).
pub static LAMP: Line = Line::new(MOST_WAITING, LONGEST_WAIT, RETRY_EVERY);

impl Line {
    pub const fn new(most: usize, longest: Duration, every: Duration) -> Self {
        Self {
            waiting: Mutex::new(Waiting {
                places: VecDeque::new(),
                next: 0,
                ends: 0,
            }),
            changed: Condvar::new(),
            most,
            longest,
            every,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Waiting> {
        self.waiting
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Whether any task is in line, waiting or being handed over from it.
    pub fn anyone(&self) -> bool {
        !self.lock().places.is_empty()
    }

    /// `skill`'s task put last in line, or why not.
    pub fn enter(&self, skill: &str) -> Result<Place<'_>, Dropped> {
        let mut waiting = self.lock();
        if waiting.places.iter().any(|place| place.skill == skill) {
            return Err(Dropped::Waits);
        }
        // Every place counts, the one being tried too: refused `busy`
        // again, it waits once more, so at most `most` ever wait.
        if waiting.places.len() >= self.most {
            return Err(Dropped::Full);
        }
        let id = waiting.next;
        waiting.next = waiting.next.wrapping_add(1);
        waiting.places.push_back(Waiter {
            id,
            skill: skill.to_owned(),
            trying: false,
        });
        Ok(Place {
            line: self,
            id,
            since: Instant::now(),
            tried: None,
        })
    }

    /// A hand-over to lamp-tasks ended: the first in line tries now.
    pub fn ended(&self) {
        let mut waiting = self.lock();
        waiting.ends = waiting.ends.wrapping_add(1);
        drop(waiting);
        self.changed.notify_all();
    }

    /// How many tasks are in line, for tests.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().places.len()
    }
}

/// A task's place in a [`Line`], let go when dropped.
pub struct Place<'a> {
    line: &'a Line,
    id: u64,
    since: Instant,
    /// When its last try was refused `busy`, and the line's ends then.
    tried: Option<(Instant, u64)>,
}

impl Place<'_> {
    /// Waits for its turn: first in line, `free` (no hand-over of
    /// lamp-skills' under way) and, after a `busy`, a hand-over ended since
    /// or [`Line`]'s `every` past; then it is being handed over. Its wait
    /// past the line's `longest` from when it entered, [`Dropped::Lapsed`].
    pub fn turn(&mut self, free: impl Fn() -> bool) -> Result<(), Dropped> {
        let line = self.line;
        let mut waiting = line.lock();
        loop {
            let now = Instant::now();
            let waited = now.duration_since(self.since);
            if waited >= line.longest {
                return Err(Dropped::Lapsed);
            }
            let first = waiting
                .places
                .front()
                .is_some_and(|place| place.id == self.id);
            let again = self.tried.is_none_or(|(at, ends)| {
                waiting.ends != ends || now.duration_since(at) >= line.every
            });
            if first && again && free() {
                if let Some(place) = waiting.places.front_mut() {
                    place.trying = true;
                }
                return Ok(());
            }
            let mut wait = (line.longest - waited).min(line.every);
            if let Some((at, _)) = self.tried {
                let left = line.every.saturating_sub(now.duration_since(at));
                if !left.is_zero() {
                    wait = wait.min(left);
                }
            }
            waiting = line
                .changed
                .wait_timeout(waiting, wait.max(Duration::from_millis(1)))
                .unwrap_or_else(|poison| poison.into_inner())
                .0;
        }
    }

    /// Its try was refused `busy`: it waits again, first in line, tried
    /// again on the next end or after `every`.
    pub fn busy(&mut self) {
        let mut waiting = self.line.lock();
        if let Some(place) = waiting.places.iter_mut().find(|place| place.id == self.id) {
            place.trying = false;
        }
        self.tried = Some((Instant::now(), waiting.ends));
    }
}

impl Drop for Place<'_> {
    fn drop(&mut self) {
        let mut waiting = self.line.lock();
        waiting.places.retain(|place| place.id != self.id);
        waiting.ends = waiting.ends.wrapping_add(1);
        drop(waiting);
        self.line.changed.notify_all();
    }
}

/// `skill`'s task handed over through `try_hand` (the journal's line for
/// the try, and whether lamp-tasks refused it `busy`), with [`ENVIRONMENT`]
/// on (`sure`) in its turn in `line`: behind any task already in line, and
/// after a `busy`, waiting there until [`Place::turn`] (with `free`, no
/// hand-over of lamp-skills' under way) and tried again. The last try's
/// journal line, or why it was let go. `journal` is told each `busy` try
/// and each wait, never the task's words. Off: one try, as before.
pub fn hand_in_turn(
    sure: bool,
    line: &Line,
    skill: &str,
    free: impl Fn() -> bool,
    mut try_hand: impl FnMut() -> (String, bool),
    mut journal: impl FnMut(&str),
) -> Result<String, Dropped> {
    if !sure {
        return Ok(try_hand().0);
    }
    let mut place = None;
    if line.anyone() {
        place = Some(line.enter(skill)?);
        journal(&format!(
            "do.task: waits its turn behind the tasks in line ({ENVIRONMENT})"
        ));
    }
    loop {
        if let Some(place) = place.as_mut() {
            place.turn(&free)?;
        }
        let (said, busy) = try_hand();
        if !busy {
            return Ok(said);
        }
        journal(&said);
        match place.as_mut() {
            Some(place) => place.busy(),
            None => {
                let mut entered = line.enter(skill)?;
                entered.busy();
                place = Some(entered);
            }
        }
        journal(&format!(
            "do.task: waits its turn, lamp-tasks busy with another skill's ({ENVIRONMENT})"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    fn refused(code: &str, message: &str) -> Result<String, Unrun> {
        Ok(serde_json::json!({"ok": false, "error": code, "message": message}).to_string())
    }

    /// Every way lamp-tasks' answer says a run failed is one fixed line:
    /// a key not added says what to do in the app, a refusal before its agent began that it couldn't run, a run begun
    /// and failed (or no answer) that it didn't finish. A run that ended
    /// well, or one refused `busy`, says nothing here; no line carries the
    /// task's words or lamp-tasks' message.
    #[test]
    fn a_failed_run_is_one_fixed_line_a_reason() {
        assert_eq!(
            failed_line(
                "photo-booth",
                &refused(
                    "missing_secret",
                    "photo booth didn't run: it needs TELEGRAM_BOT_TOKEN added in the app"
                )
            )
            .as_deref(),
            Some("Photo booth couldn't run: it needs a key added in the app, on its page.")
        );
        for code in [
            "off",
            "bad_request",
            "too_long",
            "no_token",
            "not_started",
            "no_secrets",
        ] {
            assert_eq!(
                failed_line("morning-mail", &refused(code, "Read my unread mail.")).as_deref(),
                Some("Morning mail couldn't run this time."),
                "{code}"
            );
        }
        for code in ["run_failed", "no_answer", "something_new"] {
            let line = failed_line("morning-mail", &refused(code, "Read my unread mail."));
            assert_eq!(
                line.as_deref(),
                Some("Morning mail didn't finish this time."),
                "{code}"
            );
            assert!(!line.unwrap().contains("unread"));
        }
        assert_eq!(
            failed_line("m", &Err(Unrun::NotTaken("not reached".into()))).as_deref(),
            Some("M couldn't run this time.")
        );
        assert_eq!(
            failed_line("m", &Err(Unrun::Unknown("no answer".into()))).as_deref(),
            Some("M didn't finish this time.")
        );
        assert_eq!(
            failed_line("m", &Ok("not json".into())).as_deref(),
            Some("M didn't finish this time.")
        );
        assert_eq!(failed_line("m", &Ok(r#"{"ok":true,"id":3}"#.into())), None);
        assert_eq!(failed_line("m", &refused("busy", "one at a time")), None);
        assert!(is_busy(&refused("busy", "one at a time")));
        assert!(!is_busy(&refused("run_failed", "")));
        assert!(!is_busy(&Err(Unrun::NotTaken("busy".into()))));
        assert_eq!(
            dropped_line("morning_agenda"),
            "Morning agenda didn't run this time: the lamp was busy with other skills."
        );
    }

    /// At most `most` in line, the one being handed over among them, so
    /// no more ever wait, even when it is refused `busy` again; a skill
    /// whose last task waits isn't put in line twice.
    #[test]
    fn the_line_holds_at_most_its_bound() {
        let line = Line::new(4, Duration::from_secs(60), Duration::from_secs(10));
        let mut first = line.enter("a").unwrap();
        let _b = line.enter("b").unwrap();
        let _c = line.enter("c").unwrap();
        let _d = line.enter("d").unwrap();
        assert_eq!(line.enter("e").err(), Some(Dropped::Full));
        assert_eq!(line.enter("b").err(), Some(Dropped::Waits));
        assert!(Dropped::Full.tells() && Dropped::Lapsed.tells() && !Dropped::Waits.tells());
        // The first being handed over keeps its place: refused `busy`
        // again, it waits with the three, never a fifth.
        first.turn(|| true).unwrap();
        assert_eq!(line.enter("e").err(), Some(Dropped::Full));
        first.busy();
        assert_eq!(line.len(), 4);
        assert_eq!(line.enter("e").err(), Some(Dropped::Full));
        drop(first);
        let e = line.enter("e").unwrap();
        assert_eq!(line.len(), 4);
        drop(e);
        assert_eq!(line.len(), 3);
    }

    /// A task never let take its turn is let go after the line's longest,
    /// and leaves the line.
    #[test]
    fn a_wait_past_its_longest_is_let_go() {
        let line = Line::new(4, Duration::from_millis(120), Duration::from_millis(30));
        let begun = Instant::now();
        let mut place = line.enter("a").unwrap();
        place.busy();
        assert_eq!(place.turn(|| false), Err(Dropped::Lapsed));
        assert!(begun.elapsed() >= Duration::from_millis(120));
        assert!(begun.elapsed() < Duration::from_secs(5));
        drop(place);
        assert!(!line.anyone());
    }

    /// After a `busy` with no end seen, the first in line tries again
    /// after `every`, not before.
    #[test]
    fn a_busy_try_is_tried_again_after_every() {
        let line = Line::new(4, Duration::from_secs(10), Duration::from_millis(150));
        let mut place = line.enter("a").unwrap();
        place.turn(|| true).unwrap();
        place.busy();
        let begun = Instant::now();
        place.turn(|| true).unwrap();
        assert!(
            begun.elapsed() >= Duration::from_millis(140),
            "{:?}",
            begun.elapsed()
        );
    }

    /// Three in line take their turns in the order they came, whatever
    /// order their threads ask in.
    #[test]
    fn the_line_is_first_in_first_out() {
        let line: &'static Line = Box::leak(Box::new(Line::new(
            4,
            Duration::from_secs(10),
            Duration::from_secs(10),
        )));
        let places: Vec<Place<'static>> = ["a", "b", "c"]
            .iter()
            .map(|skill| line.enter(skill).unwrap())
            .collect();
        let order = Arc::new(Mutex::new(Vec::new()));
        let threads: Vec<_> = places
            .into_iter()
            .zip(["a", "b", "c"])
            .rev()
            .map(|(mut place, skill)| {
                let order = order.clone();
                std::thread::spawn(move || {
                    place.turn(|| true).unwrap();
                    order.lock().unwrap().push(skill);
                    std::thread::sleep(Duration::from_millis(20));
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(*order.lock().unwrap(), ["a", "b", "c"]);
    }

    /// A fake lamp-tasks that runs one at a time: each try `busy` while
    /// one runs, else it runs for `run` and ends, as lamp-skills'
    /// `Handing` counts and [`Line::ended`] tells.
    struct Tasks {
        running: AtomicBool,
        handing: AtomicUsize,
        run: Duration,
        ran: Mutex<Vec<(String, Instant, Instant)>>,
    }

    impl Tasks {
        fn try_hand(&self, line: &Line, skill: &str) -> (String, bool) {
            self.handing.fetch_add(1, Ordering::SeqCst);
            let outcome = if self
                .running
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                (
                    "do.task: lamp-tasks did not take it: busy; not counted".to_owned(),
                    true,
                )
            } else {
                let began = Instant::now();
                std::thread::sleep(self.run);
                self.running.store(false, Ordering::SeqCst);
                self.ran
                    .lock()
                    .unwrap()
                    .push((skill.to_owned(), began, Instant::now()));
                (
                    "do.task: handed to lamp-tasks: its run ended".to_owned(),
                    false,
                )
            };
            self.handing.fetch_sub(1, Ordering::SeqCst);
            line.ended();
            outcome
        }
    }

    fn tasks(run: Duration) -> &'static Tasks {
        Box::leak(Box::new(Tasks {
            running: AtomicBool::new(false),
            handing: AtomicUsize::new(0),
            run,
            ran: Mutex::new(Vec::new()),
        }))
    }

    /// Two skills due at once (mail and calendar at 8:00): the second is
    /// refused `busy`, waits its turn, and is handed over as soon as the
    /// first ends, well before `every`; both ran, one after the other.
    #[test]
    fn a_busy_task_is_handed_over_after_the_first_ends() {
        let line: &'static Line = Box::leak(Box::new(Line::new(
            4,
            Duration::from_secs(30),
            Duration::from_secs(10),
        )));
        let tasks = tasks(Duration::from_millis(300));
        let first = std::thread::spawn(move || {
            hand_in_turn(
                true,
                line,
                "morning-mail",
                || tasks.handing.load(Ordering::SeqCst) == 0,
                || tasks.try_hand(line, "morning-mail"),
                |_| {},
            )
        });
        std::thread::sleep(Duration::from_millis(50));
        let journal = Arc::new(Mutex::new(Vec::new()));
        let kept = journal.clone();
        let second = hand_in_turn(
            true,
            line,
            "morning-agenda",
            || tasks.handing.load(Ordering::SeqCst) == 0,
            || tasks.try_hand(line, "morning-agenda"),
            move |words| kept.lock().unwrap().push(words.to_owned()),
        );
        assert_eq!(
            second.as_deref(),
            Ok("do.task: handed to lamp-tasks: its run ended")
        );
        assert!(first.join().unwrap().is_ok());
        let ran = tasks.ran.lock().unwrap();
        assert_eq!(ran.len(), 2);
        assert_eq!(ran[0].0, "morning-mail");
        assert_eq!(ran[1].0, "morning-agenda");
        assert!(
            ran[1].1 >= ran[0].2,
            "the second began after the first ended"
        );
        assert!(
            ran[1].1.duration_since(ran[0].2) < Duration::from_secs(2),
            "handed over on the first's end, not after every"
        );
        let journal = journal.lock().unwrap();
        assert_eq!(journal.len(), 2, "{journal:?}");
        assert!(journal[0].contains("busy; not counted"));
        assert!(journal[1].contains("waits its turn"));
        assert!(!line.anyone());
    }

    /// The line full, a task refused `busy` is let go at once and said to
    /// be; one due while a task waits goes behind it, never first.
    #[test]
    fn a_task_past_the_bound_is_let_go_and_none_jumps_the_line() {
        let line = Line::new(1, Duration::from_secs(30), Duration::from_secs(10));
        let _waits = line.enter("a").unwrap();
        let mut tries = 0;
        let full = hand_in_turn(
            true,
            &line,
            "b",
            || true,
            || {
                tries += 1;
                ("busy".to_owned(), true)
            },
            |_| {},
        );
        // "a" waits, so "b" goes behind it and the line is full: no try.
        assert_eq!(full, Err(Dropped::Full));
        assert_eq!(tries, 0);
        let line = Line::new(1, Duration::from_secs(30), Duration::from_secs(10));
        let mut tries = 0;
        let full = hand_in_turn(
            true,
            &line,
            "b",
            || true,
            || {
                tries += 1;
                // A task due meanwhile fills the line.
                std::mem::forget(line.enter("c").unwrap());
                ("busy".to_owned(), true)
            },
            |_| {},
        );
        assert_eq!(full, Err(Dropped::Full));
        assert_eq!(tries, 1);
    }

    /// Calendar refused `busy` waits; by its turn its yes changed (updated
    /// from the Store), so its retry returns before `send`. That try is no
    /// `busy`: it leaves the line at once with its own reason, and nothing
    /// is told as let go.
    #[test]
    fn a_try_refused_before_its_send_is_no_busy() {
        let line = Line::new(4, Duration::from_secs(30), Duration::from_millis(50));
        let tried = Tried::default();
        let mut tries = 0;
        let begun = Instant::now();
        let said = hand_in_turn(
            true,
            &line,
            "morning-agenda",
            || true,
            || {
                tries += 1;
                tried.once(|| {
                    if tries == 1 {
                        tried.sent(
                            "morning-agenda",
                            &refused("busy", "a skill's task is already running"),
                        );
                        "do.task: lamp-tasks did not take it: busy; not counted".to_owned()
                    } else {
                        crate::task::CHANGED.to_owned()
                    }
                })
            },
            |_| {},
        );
        assert_eq!(said.as_deref(), Ok(crate::task::CHANGED));
        assert_eq!(tries, 2);
        assert!(
            begun.elapsed() < Duration::from_secs(5),
            "{:?}",
            begun.elapsed()
        );
        assert!(!line.anyone());
        assert_eq!(tried.failed(), None);
        // A try that sent and failed keeps its own line, once.
        let (_, busy) = tried.once(|| {
            tried.sent("morning-agenda", &refused("run_failed", "x"));
            String::new()
        });
        assert!(!busy);
        assert_eq!(
            tried.failed().as_deref(),
            Some("Morning agenda didn't finish this time.")
        );
        assert_eq!(tried.failed(), None);
    }

    /// Off: one try, its line as it is, whatever it says; the line is
    /// never touched and nothing more is journalled.
    #[test]
    fn off_it_is_one_try_as_before() {
        let line = Line::new(4, Duration::from_secs(30), Duration::from_secs(10));
        let _waits = line.enter("a").unwrap();
        let mut tries = 0;
        let mut journalled = 0;
        let said = hand_in_turn(
            false,
            &line,
            "b",
            || false,
            || {
                tries += 1;
                (
                    "do.task: lamp-tasks did not take it: busy; not counted".to_owned(),
                    true,
                )
            },
            |_| journalled += 1,
        );
        assert_eq!(
            said.as_deref(),
            Ok("do.task: lamp-tasks did not take it: busy; not counted")
        );
        assert_eq!((tries, journalled), (1, 0));
        assert_eq!(line.len(), 1);
        set_on_here(None);
        assert!(!on() || std::env::var(ENVIRONMENT).as_deref() == Ok("on"));
        set_on_here(Some(true));
        assert!(on());
        set_on_here(None);
    }
}
