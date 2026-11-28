//! One explicit persistence owner, with one outstanding immutable checkpoint.
//!
//! Open/load a [`Store`] before admitting observations, then move it here. Opening
//! storage, capturing/cloning a [`Recorder`], and starting a thread are startup or
//! maintenance work: none is hidden in [`Persistence::try_checkpoint`] or `poll`.
//! Admission only moves an owned snapshot; polling exchanges bounded messages. The
//! worker alone serializes, validates and saves it, including filesystem syncs.
//!
//! A ticket identifies an accepted save attempt in this worker lifetime, not an
//! event, source revision, upload acknowledgement or checkpoint format version.
//! Callers retain their live recorder and track which mutation revision they
//! captured. A successful older save does not make a newer live state durable.
//!
//! Any save error means unknown durability: rename may already have happened.
//! It latches the worker closed; recovery requires confirmed shutdown followed
//! by the existing Store recovery/load path, not a second concurrent writer.
//! There is no retry, coalescing, cancellation deadline or implicit final save.
//! A slow/kernel-stalled write remains outstanding. Stop drains accepted work;
//! only observed thread exit confirms that the worker and its Store are gone.
//! Dropping this handle never waits, but an old write can still finish afterward.
//! Hold the handle and poll exit when the caller needs a shutdown confirmation.
//! Poll joins only after `is_finished`, never waiting for an unfinished save. Rust
//! also joins thread-local teardown; this is a measured soft latency boundary,
//! not a hard real-time guarantee for shutdown or polling.
//!
//! Reuses the one-flight/matching-completion pattern in voice onboarding storage;
//! this caller-neutral primitive installs no service, adapter or product switch.

use std::fmt;
use std::io;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};

use crate::{Recorder, store::Store};

/// Local save-attempt identity. It is never reused by the same owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ticket(u64);

impl Ticket {
    pub fn sequence(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    WorkerStart,
}

impl fmt::Display for StartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("persistence_worker_start_failed")
    }
}
impl std::error::Error for StartError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Busy,
    Closed,
    Exhausted,
}

/// Rejection returns the exact owned snapshot; nothing was queued or saved.
pub struct Refused {
    pub reason: Refusal,
    pub recorder: Recorder,
}

// Never format checkpoint contents or local event identities on an error path.
impl fmt::Debug for Refused {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Refused")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    SaveFailed,
    WorkerPanicked,
    WorkerExited,
    CompletionMismatch,
    CompletionLost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Store::save returned Ok, including its final directory sync.
    Saved,
    /// This attempt is unconfirmed; it may have replaced the previous file.
    DurabilityUnknown(Failure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Completion {
    pub ticket: Ticket,
    pub outcome: Outcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// Includes a stop-requested worker still draining or stuck in native I/O.
    Running,
    Confirmed,
    Panicked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// Controller's last observed admission state, not an independent liveness
    /// probe. `status` does not poll; a disconnected offer is returned intact.
    pub accepting: bool,
    pub outstanding: Option<Ticket>,
    /// A past confirmed save, not a claim that the current file/live state is it.
    pub last_confirmed: Option<Ticket>,
    pub failure: Option<Failure>,
    pub stop_requested: bool,
    pub exit: Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Poll {
    /// Each matching completion is delivered once, never consumed by offer.
    pub completion: Option<Completion>,
    pub status: Status,
}

struct Job {
    ticket: Ticket,
    recorder: Recorder,
}

/// Single, non-cloneable caller handle. No recorder lock is shared with the worker.
pub struct Persistence {
    sender: Option<SyncSender<Job>>,
    completed: Receiver<Completion>,
    worker: Option<JoinHandle<()>>,
    next_ticket: Option<u64>,
    outstanding: Option<Ticket>,
    last_confirmed: Option<Ticket>,
    failure: Option<Failure>,
    stop_requested: bool,
    exit: Exit,
}

// A private test seam, not another public storage protocol.
trait Backend: Send + 'static {
    fn save(&mut self, recorder: &Recorder) -> io::Result<()>;
}

impl Backend for Store {
    fn save(&mut self, recorder: &Recorder) -> io::Result<()> {
        Store::save(self, recorder)
    }
}

impl Persistence {
    /// Moves the existing exclusive store into one worker. Call outside ingress.
    /// The Store is released if thread creation fails. This does not load/reset it.
    pub fn start(store: Store) -> Result<Self, StartError> {
        Self::spawn(store)
    }

    fn spawn(mut backend: impl Backend) -> Result<Self, StartError> {
        let (sender, jobs) = mpsc::sync_channel::<Job>(1);
        let (done, completed) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("evals-persistence".into())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    let Job { ticket, recorder } = job;
                    let outcome = match backend.save(&recorder) {
                        Ok(()) => Outcome::Saved,
                        Err(_) => Outcome::DurabilityUnknown(Failure::SaveFailed),
                    };
                    // Release the completed snapshot before its acknowledgement
                    // allows the caller to offer another. Deallocation stays here.
                    drop(recorder);
                    // With one outstanding job this slot is necessarily empty.
                    // An invariant failure closes the worker rather than waiting.
                    if done.try_send(Completion { ticket, outcome }).is_err()
                        || outcome != Outcome::Saved
                    {
                        break;
                    }
                }
            })
            .map_err(|_| StartError::WorkerStart)?;
        Ok(Self {
            sender: Some(sender),
            completed,
            worker: Some(worker),
            next_ticket: Some(1),
            outstanding: None,
            last_confirmed: None,
            failure: None,
            stop_requested: false,
            exit: Exit::Running,
        })
    }

    /// Non-waiting admission of an already captured snapshot. No clone, encoding,
    /// I/O, logging or acknowledgement consumption occurs here. A completion must
    /// be polled before another snapshot is accepted, even if the save is done.
    // Return the owned Recorder inline, deliberately avoiding allocation merely
    // to refuse admission. The public caller must receive its exact snapshot.
    #[allow(clippy::result_large_err)]
    pub fn try_checkpoint(&mut self, recorder: Recorder) -> Result<Ticket, Refused> {
        let refuse = |reason, recorder| Err(Refused { reason, recorder });
        let Some(sender) = self.sender.as_ref() else {
            return refuse(Refusal::Closed, recorder);
        };
        if self.outstanding.is_some() {
            return refuse(Refusal::Busy, recorder);
        }
        let Some(sequence) = self.next_ticket else {
            return refuse(Refusal::Exhausted, recorder);
        };
        let ticket = Ticket(sequence);
        match sender.try_send(Job { ticket, recorder }) {
            Ok(()) => {
                self.outstanding = Some(ticket);
                self.next_ticket = sequence.checked_add(1);
                Ok(ticket)
            }
            Err(TrySendError::Full(job)) => refuse(Refusal::Busy, job.recorder),
            Err(TrySendError::Disconnected(job)) => {
                self.fail(Failure::WorkerExited);
                refuse(Refusal::Closed, job.recorder)
            }
        }
    }

    pub fn status(&self) -> Status {
        Status {
            accepting: self.sender.is_some()
                && self.outstanding.is_none()
                && self.next_ticket.is_some(),
            outstanding: self.outstanding,
            last_confirmed: self.last_confirmed,
            failure: self.failure,
            stop_requested: self.stop_requested,
            exit: self.exit,
        }
    }

    /// Stop new admission and drain the sole accepted job. This is a request,
    /// never confirmation, cancellation, a final checkpoint or a wait for I/O.
    pub fn request_stop(&mut self) {
        self.stop_requested = true;
        self.sender.take();
    }

    fn fail(&mut self, failure: Failure) {
        self.failure.get_or_insert(failure);
        self.sender.take();
    }

    fn accept_completion(&mut self, completion: Completion) -> Option<Completion> {
        if self.outstanding != Some(completion.ticket) {
            self.fail(Failure::CompletionMismatch);
            return None;
        }
        self.outstanding = None;
        let outcome = match self.failure {
            Some(failure) => Outcome::DurabilityUnknown(failure),
            None => completion.outcome,
        };
        match outcome {
            Outcome::Saved => self.last_confirmed = Some(completion.ticket),
            Outcome::DurabilityUnknown(failure) => self.fail(failure),
        }
        Some(Completion {
            ticket: completion.ticket,
            outcome,
        })
    }

    fn receive(&mut self) -> Option<Completion> {
        match self.completed.try_recv() {
            Ok(completion) => self.accept_completion(completion),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }

    /// At most one matching result and one exit observation. Never joins before
    /// `is_finished`; final thread-local teardown has no hard time guarantee.
    /// The second receive only covers completion racing the
    /// first receive and observed exit. A returned status is a local observation;
    /// later worker progress does not retroactively change it.
    pub fn poll(&mut self) -> Poll {
        let mut completion = self.receive();
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            let panicked = self
                .worker
                .take()
                .is_some_and(|worker| worker.join().is_err());
            self.exit = if panicked {
                Exit::Panicked
            } else {
                Exit::Confirmed
            };
            self.sender.take();
            if completion.is_none() {
                completion = self.receive();
            }
            if panicked {
                self.fail(Failure::WorkerPanicked);
            } else if !self.stop_requested && self.failure.is_none() {
                self.fail(Failure::WorkerExited);
            }
            if let Some(ticket) = self.outstanding.take() {
                let failure = self.failure.unwrap_or(Failure::CompletionLost);
                self.fail(failure);
                completion = Some(Completion {
                    ticket,
                    outcome: Outcome::DurabilityUnknown(failure),
                });
            }
        }
        Poll {
            completion,
            status: self.status(),
        }
    }
}

impl Drop for Persistence {
    fn drop(&mut self) {
        // The JoinHandle subsequently detaches; native writes may still finish.
        self.request_stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const CANARY: &str = "private_words_location_device_path";
    const END: u64 = 500_000;

    enum Action {
        Save,
        Fail,
        Panic,
    }

    struct Controlled {
        entered: SyncSender<()>,
        release: Receiver<Action>,
        dropped: SyncSender<()>,
    }
    impl Backend for Controlled {
        fn save(&mut self, _: &Recorder) -> io::Result<()> {
            self.entered.send(()).unwrap();
            match self.release.recv().unwrap() {
                Action::Save => Ok(()),
                Action::Fail => Err(io::Error::other(CANARY)),
                Action::Panic => panic!("synthetic persistence worker failure"),
            }
        }
    }
    impl Drop for Controlled {
        fn drop(&mut self) {
            let _ = self.dropped.try_send(());
        }
    }

    struct Control {
        entered: Receiver<()>,
        release: SyncSender<Action>,
        dropped: Receiver<()>,
    }

    fn controlled() -> (Persistence, Control) {
        let (entered_tx, entered) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let (dropped_tx, dropped) = mpsc::sync_channel(1);
        let owner = Persistence::spawn(Controlled {
            entered: entered_tx,
            release: released,
            dropped: dropped_tx,
        })
        .unwrap();
        (
            owner,
            Control {
                entered,
                release,
                dropped,
            },
        )
    }

    fn fixture() -> Recorder {
        Recorder::new(END).unwrap()
    }

    fn until(owner: &mut Persistence, predicate: impl Fn(&Poll) -> bool) -> Poll {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let result = owner.poll();
            if predicate(&result) {
                return result;
            }
            assert!(
                Instant::now() < deadline,
                "worker did not reach the test boundary"
            );
            thread::yield_now();
        }
    }

    fn entered(control: &Control) {
        control
            .entered
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
    }

    fn stop(owner: &mut Persistence) -> Status {
        owner.request_stop();
        until(owner, |result| result.status.exit != Exit::Running).status
    }

    #[test]
    fn blocked_io_keeps_one_snapshot_and_returns_busy_ownership() {
        let (mut owner, control) = controlled();
        let ticket = owner.try_checkpoint(fixture()).unwrap();
        assert_eq!(ticket.sequence(), 1);
        entered(&control);
        let before = fixture().checkpoint().unwrap();
        let refused = owner.try_checkpoint(fixture()).unwrap_err();
        assert_eq!(refused.reason, Refusal::Busy);
        assert_eq!(refused.recorder.checkpoint().unwrap(), before);
        for _ in 0..1000 {
            assert_eq!(owner.poll().status.outstanding, Some(ticket));
        }
        assert!(control.dropped.try_recv().is_err());
        control.release.send(Action::Save).unwrap();
        let result = until(&mut owner, |result| result.completion.is_some());
        assert_eq!(
            result.completion,
            Some(Completion {
                ticket,
                outcome: Outcome::Saved
            })
        );
        assert_eq!(result.status.last_confirmed, Some(ticket));
        assert!(owner.poll().completion.is_none());
        assert_eq!(stop(&mut owner).failure, None);
    }

    #[test]
    fn completed_save_remains_busy_until_its_matching_ack_is_polled() {
        let (mut owner, control) = controlled();
        let first = owner.try_checkpoint(fixture()).unwrap();
        entered(&control);
        control.release.send(Action::Save).unwrap();
        // Without polling it is still busy, irrespective of worker scheduling.
        assert_eq!(
            owner.try_checkpoint(fixture()).unwrap_err().reason,
            Refusal::Busy
        );
        assert_eq!(
            until(&mut owner, |p| p.completion.is_some())
                .completion
                .unwrap()
                .ticket,
            first
        );
        let second = owner.try_checkpoint(fixture()).unwrap();
        assert_eq!(second.sequence(), 2);
        entered(&control);
        assert_eq!(owner.status().last_confirmed, Some(first));
        control.release.send(Action::Save).unwrap();
        assert_eq!(
            until(&mut owner, |p| p.completion.is_some())
                .completion
                .unwrap()
                .ticket,
            second
        );
        assert_eq!(stop(&mut owner).last_confirmed, Some(second));
    }

    #[test]
    fn write_failure_is_unknown_and_latched_without_retry_or_raw_error() {
        let (mut owner, control) = controlled();
        let ticket = owner.try_checkpoint(fixture()).unwrap();
        entered(&control);
        control.release.send(Action::Fail).unwrap();
        let result = until(&mut owner, |p| p.completion.is_some());
        assert_eq!(
            result.completion.unwrap().outcome,
            Outcome::DurabilityUnknown(Failure::SaveFailed)
        );
        assert_eq!(result.status.last_confirmed, None);
        assert_eq!(result.status.failure, Some(Failure::SaveFailed));
        let refused = owner.try_checkpoint(fixture()).unwrap_err();
        assert_eq!(refused.reason, Refusal::Closed);
        assert!(!format!("{result:?} {refused:?}").contains(CANARY));
        let ended = stop(&mut owner);
        assert_eq!(ended.exit, Exit::Confirmed);
        assert_eq!(ended.failure, Some(Failure::SaveFailed));
        assert_eq!(ticket.sequence(), 1);
        assert!(control.entered.try_recv().is_err());
    }

    #[test]
    fn a_later_failed_attempt_does_not_erase_a_past_confirmed_save() {
        let (mut owner, control) = controlled();
        let first = owner.try_checkpoint(fixture()).unwrap();
        entered(&control);
        control.release.send(Action::Save).unwrap();
        until(&mut owner, |p| p.completion.is_some());
        let second = owner.try_checkpoint(fixture()).unwrap();
        entered(&control);
        control.release.send(Action::Fail).unwrap();
        let result = until(&mut owner, |p| p.completion.is_some());
        assert_eq!(result.completion.unwrap().ticket, second);
        assert_eq!(result.status.last_confirmed, Some(first));
        assert_eq!(stop(&mut owner).failure, Some(Failure::SaveFailed));
    }

    #[test]
    fn stopping_an_idle_owner_confirms_exit_without_an_implicit_save() {
        let (mut owner, control) = controlled();
        let ended = stop(&mut owner);
        assert_eq!(ended.exit, Exit::Confirmed);
        assert_eq!(ended.last_confirmed, None);
        assert_eq!(ended.failure, None);
        assert_eq!(
            owner.try_checkpoint(fixture()).unwrap_err().reason,
            Refusal::Closed
        );
        assert!(control.entered.try_recv().is_err());
    }

    #[test]
    fn panic_yields_one_unknown_completion_and_confirmed_panic_exit() {
        let (mut owner, control) = controlled();
        let ticket = owner.try_checkpoint(fixture()).unwrap();
        entered(&control);
        control.release.send(Action::Panic).unwrap();
        let ended = until(&mut owner, |p| p.status.exit != Exit::Running);
        assert_eq!(ended.status.exit, Exit::Panicked);
        assert_eq!(
            ended.completion,
            Some(Completion {
                ticket,
                outcome: Outcome::DurabilityUnknown(Failure::WorkerPanicked)
            })
        );
        assert_eq!(
            owner.try_checkpoint(fixture()).unwrap_err().reason,
            Refusal::Closed
        );
        assert!(owner.poll().completion.is_none());
    }

    #[test]
    fn stop_is_a_request_until_the_accepted_job_and_worker_finish() {
        let (mut owner, control) = controlled();
        let ticket = owner.try_checkpoint(fixture()).unwrap();
        entered(&control);
        owner.request_stop();
        for _ in 0..100 {
            let result = owner.poll();
            assert_eq!(result.status.exit, Exit::Running);
            assert_eq!(result.status.outstanding, Some(ticket));
            assert!(result.status.stop_requested);
            assert!(result.completion.is_none());
        }
        assert_eq!(
            owner.try_checkpoint(fixture()).unwrap_err().reason,
            Refusal::Closed
        );
        control.release.send(Action::Save).unwrap();
        assert_eq!(
            until(&mut owner, |p| p.completion.is_some())
                .completion
                .unwrap()
                .outcome,
            Outcome::Saved
        );
        assert_eq!(stop(&mut owner).exit, Exit::Confirmed);
        assert!(control.entered.try_recv().is_err());
    }

    #[test]
    fn maximum_ticket_is_accepted_once_and_never_wraps() {
        let (mut owner, control) = controlled();
        owner.next_ticket = Some(u64::MAX);
        let ticket = owner.try_checkpoint(fixture()).unwrap();
        assert_eq!(ticket.sequence(), u64::MAX);
        entered(&control);
        control.release.send(Action::Save).unwrap();
        until(&mut owner, |p| p.completion.is_some());
        for _ in 0..2 {
            let refused = owner.try_checkpoint(fixture()).unwrap_err();
            assert_eq!(refused.reason, Refusal::Exhausted);
            assert_eq!(
                refused.recorder.checkpoint().unwrap(),
                fixture().checkpoint().unwrap()
            );
        }
        assert!(!owner.status().accepting);
        assert_eq!(stop(&mut owner).last_confirmed, Some(ticket));
    }

    #[test]
    fn a_foreign_completion_cannot_confirm_or_replace_the_accepted_ticket() {
        let (mut owner, control) = controlled();
        let ticket = owner.try_checkpoint(fixture()).unwrap();
        entered(&control);
        assert!(
            owner
                .accept_completion(Completion {
                    ticket: Ticket(99),
                    outcome: Outcome::Saved
                })
                .is_none()
        );
        assert_eq!(owner.status().outstanding, Some(ticket));
        assert_eq!(owner.status().last_confirmed, None);
        assert_eq!(
            owner.try_checkpoint(fixture()).unwrap_err().reason,
            Refusal::Closed
        );
        control.release.send(Action::Save).unwrap();
        let result = until(&mut owner, |p| p.completion.is_some());
        assert_eq!(
            result.completion,
            Some(Completion {
                ticket,
                outcome: Outcome::DurabilityUnknown(Failure::CompletionMismatch)
            })
        );
        assert_eq!(stop(&mut owner).last_confirmed, None);
    }

    #[test]
    fn drop_does_not_wait_or_pretend_to_cancel_an_accepted_write() {
        let (mut owner, control) = controlled();
        owner.try_checkpoint(fixture()).unwrap();
        entered(&control);
        drop(owner);
        assert!(control.dropped.try_recv().is_err());
        control.release.send(Action::Save).unwrap();
        control
            .dropped
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
    }

    /// This measures the same nonwaiting path while the backend is held at a
    /// controlled write boundary. No filesystem/clock sleeps or timing assertions.
    #[test]
    #[ignore = "explicit host timing while storage is blocked"]
    #[allow(clippy::print_stdout)]
    fn measure_offer_and_poll_with_blocked_storage() {
        let (mut owner, control) = controlled();
        owner.try_checkpoint(fixture()).unwrap();
        entered(&control);
        let mut recorder = fixture();
        let mut offers = Vec::with_capacity(10_001);
        let mut polls = Vec::with_capacity(10_001);
        for _ in 0..10_001 {
            let start = Instant::now();
            let refused = owner.try_checkpoint(recorder).unwrap_err();
            offers.push(start.elapsed().as_nanos());
            assert_eq!(refused.reason, Refusal::Busy);
            recorder = refused.recorder;
            let start = Instant::now();
            let result = owner.poll();
            polls.push(start.elapsed().as_nanos());
            assert_eq!(result.status.exit, Exit::Running);
            assert!(result.completion.is_none());
        }
        for (label, samples) in [("blocked_offer", offers), ("blocked_poll", polls)] {
            let first_ns = samples[0];
            let mut repeated = samples[1..].to_vec();
            repeated.sort_unstable();
            println!(
                "{label} unit=ns first={first_ns} median={} p95={} p99={} max={} repeated={}",
                repeated[4_999],
                repeated[9_499],
                repeated[9_899],
                repeated[9_999],
                repeated.len()
            );
        }
        control.release.send(Action::Save).unwrap();
        until(&mut owner, |p| p.completion.is_some());
        stop(&mut owner);
    }
}
