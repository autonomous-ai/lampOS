//! Following each task handed over through Harness's own `watch` (`serve
//! --push-watch`, #5230): one `harness watch --json` child a task, at most
//! [`MOST_FOLLOWED`] at once, beside the CLI worker's one lane. An event
//! that says the agent settled (`done`, `failed`, `needs_person`, `idle`,
//! `stopped`) has its task read at once ([`Engine::push`]), as a held
//! `wait` does (`serve --push-wait`). The watch's own looks stay, as the
//! backstop, and a task this does not follow (past the cap, given up, or a
//! Harness without `watch`) is left to `wait` and the looks.
use crate::{
    engine::{Clock as ClockSample, Engine, Error as EngineError, Watched},
    harness::{Cli, WatchEnd, WatchEvent},
    store::Store,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// No more `harness watch` children than this at once: each is a Node
/// process of about 130 MB on a lamp (the held `wait`'s, #1157).
pub const MOST_FOLLOWED: usize = 4;
/// A task's watch that failed is started again after these, one after
/// each of the first failures in a row; at [`FOLLOW_TRIES`] in a row the task is
/// given up, and left to `wait` and the looks.
pub const FOLLOW_BACKOFF: [Duration; 2] = [Duration::from_secs(3), Duration::from_secs(10)];
pub const FOLLOW_TRIES: u32 = 3;
/// After a watch ended on the agent settling (`done`, `failed`,
/// `stopped`), the task is followed again no sooner than this, if it is
/// still watched: the push read found it at work again.
pub const FOLLOW_AGAIN: Duration = Duration::from_secs(3);

/// Harness's own `watch`, held beside the adapter's one lane: several may
/// run at once, one a task.
pub trait WatchLane: Send + Sync + 'static {
    /// Held until the agent settles for good (`done`, `failed`: the child is
    /// killed then), it stops, its time passes, it fails, or `cancel` is set.
    fn watch(
        &self,
        machine_id: &str,
        agent_id: &str,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(WatchEvent),
    ) -> WatchEnd;
}

/// The production lane: each watch on a fresh adapter on the same CLI and
/// settings ([`Cli::lane`]), so one child that cannot be cleaned up stops
/// only its own adapter, never another task's watch or the CLI worker.
pub struct CliWatch(pub Cli);
impl WatchLane for CliWatch {
    fn watch(
        &self,
        machine_id: &str,
        agent_id: &str,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(WatchEvent),
    ) -> WatchEnd {
        match self.0.lane() {
            Ok(lane) => lane.watch(machine_id, agent_id, cancel, on_event),
            Err(_) => WatchEnd::Failed("no_lane".into()),
        }
    }
}

enum Note {
    Event(WatchEvent),
    End(WatchEnd),
}
/// What one child said: its task, its number, and the event or its end.
type Message = (u64, u64, Note);

struct Child {
    number: u64,
    cancel: Arc<AtomicBool>,
    began: Instant,
    /// A line came from it: Harness has `watch` and it follows the task.
    heard: bool,
    handle: JoinHandle<()>,
}

/// The `watch` children held, one a task, and the tasks they say settled
/// that are still to be read.
pub struct Follows {
    lane: Option<Arc<dyn WatchLane>>,
    held: BTreeMap<u64, Child>,
    /// When a task's next watch may start.
    rest: BTreeMap<u64, Instant>,
    /// Failures in a row, by task.
    failures: BTreeMap<u64, u32>,
    /// Tasks whose watch failed [`FOLLOW_TRIES`] times in a row.
    given_up: BTreeSet<u64>,
    /// Harness answered `watch` with a usage error: none is started again
    /// while the part runs.
    missing: bool,
    /// Tasks whose agent settled, to be read at once.
    pushes: BTreeSet<u64>,
    notes: (Sender<Message>, Receiver<Message>),
    /// Each child started is numbered, so a cancelled one that answers late
    /// is never taken for the one that replaced it.
    next: u64,
    /// Children cancelled and not yet joined.
    ending: Vec<JoinHandle<()>>,
}

impl Follows {
    pub fn new(lane: Option<Arc<dyn WatchLane>>) -> Self {
        Self {
            lane,
            held: BTreeMap::new(),
            rest: BTreeMap::new(),
            failures: BTreeMap::new(),
            given_up: BTreeSet::new(),
            missing: false,
            pushes: BTreeSet::new(),
            notes: mpsc::channel(),
            next: 0,
            ending: Vec::new(),
        }
    }

    /// Whether this follows anything at all.
    pub fn on(&self) -> bool {
        self.lane.is_some() && !self.missing
    }

    /// The tasks a held watch has been heard from: `wait` leaves them out.
    pub fn followed(&self) -> BTreeSet<u64> {
        self.held
            .iter()
            .filter(|(_, child)| child.heard)
            .map(|(id, _)| *id)
            .collect()
    }

    /// How many children are held now.
    pub fn held(&self) -> usize {
        self.held.len()
    }

    /// Takes what the children said, and holds one watch a task on the
    /// tasks the engine watches, at most [`MOST_FOLLOWED`], those held first.
    /// A task gone from the watch has its child cancelled. A task pushed and
    /// not yet read, or whose latest read was a push (`pushed`), gets no new
    /// watch until the engine's own next read of it: a watch started again
    /// on a settled agent says `done` again at once.
    pub fn tend(&mut self, watched: &[Watched], pushed: &BTreeSet<u64>, now: Instant) {
        if self.lane.is_none() {
            return;
        }
        self.take_notes(now);
        let watching: BTreeSet<u64> = watched.iter().map(|task| task.task_id).collect();
        // A task no longer watched, or given up, lets its child go.
        let gone: Vec<u64> = self
            .held
            .keys()
            .filter(|id| self.missing || !watching.contains(id) || self.given_up.contains(id))
            .copied()
            .collect();
        for id in gone {
            self.let_go(id);
        }
        self.rest.retain(|id, _| watching.contains(id));
        self.failures.retain(|id, _| watching.contains(id));
        self.given_up.retain(|id| watching.contains(id));
        self.pushes.retain(|id| watching.contains(id));
        self.reap();
        if self.missing {
            return;
        }
        for task in watched {
            if self.held.len() >= MOST_FOLLOWED {
                break;
            }
            let id = task.task_id;
            if self.held.contains_key(&id)
                || self.given_up.contains(&id)
                || pushed.contains(&id)
                || self.pushes.contains(&id)
                || self.rest.get(&id).is_some_and(|until| now < *until)
            {
                continue;
            }
            self.start(task, now);
        }
    }

    fn start(&mut self, task: &Watched, now: Instant) {
        let Some(lane) = self.lane.clone() else {
            return;
        };
        let (id, number) = (task.task_id, self.next);
        self.next = self.next.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let sender = self.notes.0.clone();
        let (machine, agent, flag) = (
            task.machine_id.clone(),
            task.agent_id.clone(),
            cancel.clone(),
        );
        let started = thread::Builder::new()
            .name("tasks-follow".into())
            .spawn(move || {
                let events = sender.clone();
                let end = lane.watch(&machine, &agent, &flag, &mut |event| {
                    let _ = events.send((id, number, Note::Event(event)));
                });
                let _ = sender.send((id, number, Note::End(end)));
            });
        match started {
            Ok(handle) => {
                self.held.insert(
                    id,
                    Child {
                        number,
                        cancel,
                        began: Instant::now(),
                        heard: false,
                        handle,
                    },
                );
            }
            Err(_) => {
                eprintln!("tasks: watch of task {id} could not start");
                self.failed(id, now);
            }
        }
    }

    fn take_notes(&mut self, now: Instant) {
        while let Ok((id, number, note)) = self.notes.1.try_recv() {
            let Some(child) = self.held.get_mut(&id).filter(|c| c.number == number) else {
                continue;
            };
            let took = child.began.elapsed().as_millis();
            match note {
                Note::Event(event) => {
                    child.heard = true;
                    self.failures.remove(&id);
                    eprintln!(
                        "tasks: watch of task {id}: {} after {took} ms",
                        event.name()
                    );
                    if event.settled() {
                        self.pushes.insert(id);
                    }
                }
                Note::End(end) => {
                    if let Some(child) = self.held.remove(&id) {
                        self.ending.push(child.handle);
                    }
                    match end {
                        WatchEnd::Settled => {
                            self.rest.insert(id, now + FOLLOW_AGAIN);
                        }
                        WatchEnd::Stopped => {
                            eprintln!("tasks: watch of task {id} ended stopped after {took} ms");
                            self.pushes.insert(id);
                            self.rest.insert(id, now + FOLLOW_AGAIN);
                        }
                        WatchEnd::Timeout | WatchEnd::Cancelled => {}
                        WatchEnd::Missing => {
                            eprintln!(
                                "tasks: Harness has no watch (usage error); tasks are followed by wait and the watch's looks"
                            );
                            self.missing = true;
                        }
                        WatchEnd::Failed(code) => {
                            eprintln!("tasks: watch of task {id} failed after {took} ms ({code})");
                            self.failed(id, now);
                        }
                    }
                }
            }
        }
    }

    fn failed(&mut self, id: u64, now: Instant) {
        let failures = self.failures.entry(id).or_default();
        *failures += 1;
        if *failures >= FOLLOW_TRIES {
            eprintln!(
                "tasks: watch of task {id} given up after {FOLLOW_TRIES} failures; it is followed by wait and the watch's looks"
            );
            self.given_up.insert(id);
        } else {
            let after = FOLLOW_BACKOFF[(*failures as usize - 1).min(FOLLOW_BACKOFF.len() - 1)];
            self.rest.insert(id, now + after);
        }
    }

    fn let_go(&mut self, id: u64) {
        if let Some(child) = self.held.remove(&id) {
            child.cancel.store(true, Ordering::Release);
            self.ending.push(child.handle);
        }
    }

    /// Joins the children that have finished, so none is left behind.
    fn reap(&mut self) {
        let (done, running): (Vec<_>, Vec<_>) = self
            .ending
            .drain(..)
            .partition(|handle| handle.is_finished());
        self.ending = running;
        for handle in done {
            let _ = handle.join();
        }
    }

    /// Asks for the read of one task whose agent settled, if the engine can
    /// take it now; the rest wait their turn.
    pub fn push(
        &mut self,
        engine: &mut Engine<Store>,
        now: &ClockSample,
    ) -> Result<(), EngineError> {
        let Some(&id) = self.pushes.iter().next() else {
            return Ok(());
        };
        if engine.push(id, now)? {
            self.pushes.remove(&id);
        }
        Ok(())
    }
}

impl Drop for Follows {
    /// Every child is cancelled, then joined: each kills and reaps its
    /// `harness watch` before its thread ends, so none outlives the part.
    fn drop(&mut self) {
        for child in self.held.values() {
            child.cancel.store(true, Ordering::Release);
        }
        let held = std::mem::take(&mut self.held);
        for child in held.into_values() {
            let _ = child.handle.join();
        }
        for handle in self.ending.drain(..) {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// One agent's script: the events, then the end.
    type Plan = (Vec<WatchEvent>, Option<WatchEnd>);

    /// A lane whose watches say what each agent's script says, then hold
    /// until cancelled, or end as scripted.
    #[derive(Default)]
    struct Scripted {
        /// Per agent: the events, then the end (`None`: held till cancelled).
        plans: Mutex<BTreeMap<String, Plan>>,
        started: Mutex<Vec<String>>,
        live: std::sync::atomic::AtomicUsize,
    }
    impl WatchLane for Scripted {
        fn watch(
            &self,
            _: &str,
            agent: &str,
            cancel: &AtomicBool,
            on_event: &mut dyn FnMut(WatchEvent),
        ) -> WatchEnd {
            self.started.lock().unwrap().push(agent.to_owned());
            self.live.fetch_add(1, Ordering::SeqCst);
            let (events, end) = self
                .plans
                .lock()
                .unwrap()
                .get(agent)
                .cloned()
                .unwrap_or_default();
            for event in events {
                on_event(event);
            }
            let end = match end {
                Some(end) => end,
                None => {
                    while !cancel.load(Ordering::Acquire) {
                        thread::sleep(Duration::from_millis(2));
                    }
                    WatchEnd::Cancelled
                }
            };
            self.live.fetch_sub(1, Ordering::SeqCst);
            end
        }
    }

    fn watched(count: u64) -> Vec<Watched> {
        (1..=count)
            .map(|id| Watched {
                task_id: id,
                machine_id: "0123456789abcdef0123456789abcdef".into(),
                agent_id: format!("agent-{id}"),
                engine: "claude".into(),
                accepted_ago_ms: None,
            })
            .collect()
    }

    fn settle(
        follows: &mut Follows,
        tasks: &[Watched],
        at: Instant,
        until: impl Fn(&Follows) -> bool,
    ) {
        let start = Instant::now();
        while !until(follows) {
            assert!(start.elapsed() < Duration::from_secs(5), "never settled");
            follows.tend(tasks, &BTreeSet::new(), at);
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn no_more_than_the_cap_are_held_and_each_is_let_go_with_its_task() {
        let lane = Arc::new(Scripted::default());
        let mut follows = Follows::new(Some(lane.clone()));
        let tasks = watched(6);
        let now = Instant::now();
        follows.tend(&tasks, &BTreeSet::new(), now);
        assert_eq!(follows.held(), MOST_FOLLOWED);
        // The first four, in the watch's order.
        settle(&mut follows, &tasks, now, |_| {
            lane.live.load(Ordering::SeqCst) == 4
        });
        // Task 1 leaves the watch: its child goes, and task 5 takes the slot.
        follows.tend(&tasks[1..], &BTreeSet::new(), now);
        settle(&mut follows, &tasks[1..], now, |f| f.ending.is_empty());
        assert_eq!(follows.held(), MOST_FOLLOWED);
        assert!(follows.held.contains_key(&5) && !follows.held.contains_key(&1));
        drop(follows);
        // Every child is ended once the part lets go.
        assert_eq!(lane.live.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_settled_event_pushes_its_task_and_a_working_one_does_not() {
        let lane = Arc::new(Scripted::default());
        lane.plans
            .lock()
            .unwrap()
            .insert("agent-1".into(), (vec![WatchEvent::Working], None));
        lane.plans.lock().unwrap().insert(
            "agent-2".into(),
            (
                vec![WatchEvent::Working, WatchEvent::Done],
                Some(WatchEnd::Settled),
            ),
        );
        let mut follows = Follows::new(Some(lane.clone()));
        let tasks = watched(2);
        let now = Instant::now();
        settle(&mut follows, &tasks, now, |f| {
            f.pushes.contains(&2) && !f.held.contains_key(&2)
        });
        assert!(!follows.pushes.contains(&1));
        assert_eq!(follows.followed(), BTreeSet::from([1]));
        // Not followed again within FOLLOW_AGAIN.
        follows.tend(&tasks, &BTreeSet::new(), now);
        assert!(!follows.held.contains_key(&2));
    }

    #[test]
    fn a_stopped_end_pushes_and_failures_back_off_then_give_up() {
        let lane = Arc::new(Scripted::default());
        lane.plans
            .lock()
            .unwrap()
            .insert("agent-1".into(), (vec![], Some(WatchEnd::Stopped)));
        lane.plans.lock().unwrap().insert(
            "agent-2".into(),
            (vec![], Some(WatchEnd::Failed("DISCONNECTED".into()))),
        );
        let mut follows = Follows::new(Some(lane.clone()));
        let tasks = watched(2);
        let mut at = Instant::now();
        settle(&mut follows, &tasks, at, |f| {
            f.pushes.contains(&1) && f.failures.get(&2) == Some(&1)
        });
        // Restarted only after each backoff, given up after the third.
        for (tries, after) in [(2, FOLLOW_BACKOFF[0]), (3, FOLLOW_BACKOFF[1])] {
            follows.tend(&tasks, &BTreeSet::new(), at);
            assert!(
                !follows.held.contains_key(&2),
                "restarted before its backoff"
            );
            at += after;
            settle(&mut follows, &tasks, at, |f| {
                f.failures.get(&2) == Some(&tries) || f.given_up.contains(&2)
            });
        }
        assert!(follows.given_up.contains(&2));
        let starts = lane
            .started
            .lock()
            .unwrap()
            .iter()
            .filter(|a| *a == "agent-2")
            .count();
        assert_eq!(starts, FOLLOW_TRIES as usize);
        follows.tend(&tasks, &BTreeSet::new(), at + Duration::from_secs(600));
        assert!(!follows.held.contains_key(&2));
    }

    #[test]
    fn a_harness_without_watch_turns_it_off_for_the_run() {
        let lane = Arc::new(Scripted::default());
        lane.plans
            .lock()
            .unwrap()
            .insert("agent-1".into(), (vec![], Some(WatchEnd::Missing)));
        let mut follows = Follows::new(Some(lane.clone()));
        let tasks = watched(2);
        let now = Instant::now();
        settle(&mut follows, &tasks, now, |f| !f.on());
        settle(&mut follows, &tasks, now, |f| {
            f.held() == 0 && f.ending.is_empty()
        });
        assert_eq!(lane.live.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_pushed_task_gets_no_new_watch_until_read_again() {
        let lane = Arc::new(Scripted::default());
        let mut follows = Follows::new(Some(lane.clone()));
        let tasks = watched(1);
        follows.tend(&tasks, &BTreeSet::from([1]), Instant::now());
        assert_eq!(follows.held(), 0);
        follows.tend(&tasks, &BTreeSet::new(), Instant::now());
        assert_eq!(follows.held(), 1);
    }
}
