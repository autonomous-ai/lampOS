//! Inactive, exclusive writer for an already approved repair and one priority
//! forget. No runtime calls this module; it adds no socket, flag or authority.
//!
//! Stage the target and obtain `CommitWork` from the exact `People` instance
//! before transferring that instance here. It is never cloned or reopened.
//! Future integration must also route reservation, recognition and every ordinary
//! mutation through this ownership boundary; those commands are not added here.
//!
//! Admission and status target below 1 ms on the host. They try a
//! bounded in-memory lock; filesystem work never holds it. A missed response
//! deadline is not a storage result. Blocking I/O cannot be cancelled: the same
//! worker runs accepted forget next when repair returns, even after close or
//! caller loss. A permanent I/O stall leaves forget pending. Restart loses this
//! RAM queue and means unknown, not rollback or a completed forget.
use crate::{
    face_repair::{CommitWork, Completion},
    who::{Forgot, People, RepairError, RepairStep, plain_name, same_name},
};
use std::{
    collections::VecDeque,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

const MOST_RECEIPTS: usize = 4;
// Forty Unicode name characters, at most four UTF-8 bytes each. Check before
// normalization so a hostile-sized input cannot allocate on the admission path.
const MOST_NAME_BYTES: usize = 160;
type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// Process-local read/ack key, bound to this writer; no app/account authority.
#[derive(Clone)]
pub struct RepairId {
    owner: Arc<()>,
    sequence: u64,
}

/// Monotonic operation number assigned by the trusted serialized runtime.
/// This is neither an account credential nor a public opaque-token contract.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ForgetId(u64);
impl ForgetId {
    pub fn from_trusted_runtime(sequence: u64) -> Option<Self> {
        (sequence != 0).then_some(Self(sequence))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Accepted,
    Existing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    Busy,
    Contended,
    Closed,
    WorkerFailed,
    InvalidName,
    Conflict,
    StaleId,
    Exhausted,
}

/// Preserve the single work/completion owner when admission is refused. No Debug
/// implementation: the work contains the target snapshot and replacement prints.
pub struct RejectedRepair {
    pub reason: AdmissionError,
    pub work: Box<CommitWork>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    Contended,
    Unknown,
    WorkerFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairStorage {
    Pending,
    Finished(Result<(), RepairError>),
    /// Worker failure before its actual result was retained. Never NotWritten.
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepairStatus {
    pub deadline_exceeded: bool,
    pub storage: RepairStorage,
}

/// Actual completion, taken once and passed to `Controller::complete` once.
/// Lateness does not change `result`, even for a successful visible replacement.
pub struct RepairCompletion {
    pub completion: Completion,
    pub result: Result<(), RepairError>,
    pub deadline_exceeded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForgetError {
    /// Actual forget returned an error. It may have changed the canonical file
    /// before a set-aside failure; this is not an unchanged-store assertion.
    Storage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForgetStorage {
    Pending,
    Finished(Result<Forgot, ForgetError>),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForgetStatus {
    pub deadline_exceeded: bool,
    pub storage: ForgetStorage,
}

/// The actual existing store result, including unresolved set-aside records.
/// `forgot` alone is not a complete all-copies guarantee when `unsearched > 0`.
/// The stable ID is captured before deletion and the by-name guard afterward,
/// under the same exclusive owner. No memory/rules operation is performed here.
/// Deliberately not Debug or serializable: the ID is private context.
pub struct ForgetCompletion {
    pub result: Forgot,
    pub stable_id: Option<String>,
    pub name_id_free: bool,
    pub deadline_exceeded: bool,
}

struct Repair {
    id: u64,
    deadline: Instant,
    finished: Option<Instant>,
    phase: RepairPhase,
}
enum RepairPhase {
    Queued(Box<CommitWork>),
    Running,
    Finished(RepairCompletion),
}
impl Repair {
    fn status(&self, now: Instant, failed: bool) -> RepairStatus {
        RepairStatus {
            deadline_exceeded: self.finished.unwrap_or(now) >= self.deadline,
            storage: match &self.phase {
                RepairPhase::Finished(done) => RepairStorage::Finished(done.result),
                _ if failed => RepairStorage::Unknown,
                _ => RepairStorage::Pending,
            },
        }
    }
}

struct Forget {
    id: ForgetId,
    name: String,
    asked_unix_s: u64,
    deadline: Instant,
    finished: Option<Instant>,
    phase: ForgetPhase,
}
enum ForgetPhase {
    Queued,
    Running,
    Finished(Result<ForgetCompletion, ForgetError>),
}
impl Forget {
    fn status(&self, now: Instant, failed: bool) -> ForgetStatus {
        ForgetStatus {
            deadline_exceeded: self.finished.unwrap_or(now) >= self.deadline,
            storage: match &self.phase {
                ForgetPhase::Finished(done) => ForgetStorage::Finished(
                    done.as_ref()
                        .map(|done| done.result)
                        .map_err(|error| *error),
                ),
                _ if failed => ForgetStorage::Unknown,
                _ => ForgetStorage::Pending,
            },
        }
    }
}

// Acknowledged receipts contain only operation IDs and fixed status/counts.
// Copy prevents an owned String, snapshot or completion from being retained.
#[derive(Clone, Copy)]
enum Receipt {
    Repair { id: u64, status: RepairStatus },
    Forget { id: ForgetId, status: ForgetStatus },
}
struct State {
    repair: Option<Repair>,
    forget: Option<Forget>,
    receipts: VecDeque<Receipt>,
    next_repair: u64,
    highest_forget: u64,
}
impl State {
    fn retain(&mut self, receipt: Receipt) {
        if self.receipts.len() == MOST_RECEIPTS {
            self.receipts.pop_front();
        }
        self.receipts.push_back(receipt);
    }
}
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    closing: AtomicBool,
    failed: AtomicBool,
    stopped: AtomicBool,
    repair_cancelled: AtomicBool,
}

/// One fixed worker with one exclusively owned `People`. Not Clone and no
/// exposed receiver whose disconnection could cancel accepted work. Results
/// stay pinned until explicitly taken; only acknowledged receipts can evict.
pub struct Writer {
    owner: Arc<()>,
    shared: Arc<Shared>,
    clock: Clock,
}

impl Writer {
    /// Starts one off-capture thread. Startup and shutdown are not admission
    /// latency measurements. A spawn error drops the supplied owner without
    /// changing its file. No fallback worker or automatic reopen is attempted.
    pub fn start(people: People) -> io::Result<Self> {
        Self::start_with(people, Arc::new(Instant::now), |_| Ok(()))
    }

    fn start_with(
        people: People,
        clock: Clock,
        hook: impl FnMut(Checkpoint) -> io::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                repair: None,
                forget: None,
                receipts: VecDeque::with_capacity(MOST_RECEIPTS),
                next_repair: 1,
                highest_forget: 0,
            }),
            wake: Condvar::new(),
            closing: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            repair_cancelled: AtomicBool::new(false),
        });
        let worker_shared = shared.clone();
        let worker_clock = clock.clone();
        // Dropping the join handle does not cancel the thread. Never join it on
        // the input path; close requests draining and stopped confirms exit.
        let _worker = std::thread::Builder::new()
            .name("eyes-repair-writer".into())
            .spawn(move || {
                if catch_unwind(AssertUnwindSafe(|| {
                    run(people, &worker_shared, &worker_clock, hook)
                }))
                .is_err()
                {
                    let _state = worker_shared
                        .state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    worker_shared.failed.store(true, Ordering::Release);
                }
                // run's People has been dropped before exit is published.
                worker_shared.stopped.store(true, Ordering::Release);
            })?;
        Ok(Self {
            owner: Arc::new(()),
            shared,
            clock,
        })
    }

    fn state(&self) -> Result<MutexGuard<'_, State>, ReadError> {
        self.shared.state.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => ReadError::Contended,
            TryLockError::Poisoned(_) => ReadError::WorkerFailed,
        })
    }

    fn admit(&self) -> Result<MutexGuard<'_, State>, AdmissionError> {
        if self.shared.failed.load(Ordering::Acquire) {
            return Err(AdmissionError::WorkerFailed);
        }
        if self.shared.closing.load(Ordering::Acquire) {
            return Err(AdmissionError::Closed);
        }
        let state = self.state().map_err(|error| match error {
            ReadError::Contended => AdmissionError::Contended,
            _ => AdmissionError::WorkerFailed,
        })?;
        // close takes the same lock, so admission cannot race through close.
        if self.shared.failed.load(Ordering::Acquire) {
            return Err(AdmissionError::WorkerFailed);
        }
        if self.shared.closing.load(Ordering::Acquire) {
            return Err(AdmissionError::Closed);
        }
        Ok(state)
    }

    pub fn submit_repair(
        &self,
        work: CommitWork,
        deadline: Instant,
    ) -> Result<RepairId, RejectedRepair> {
        let mut state = match self.admit() {
            Ok(state) => state,
            Err(reason) => {
                return Err(RejectedRepair {
                    reason,
                    work: Box::new(work),
                });
            }
        };
        if state.repair.is_some() || state.forget.is_some() {
            return Err(RejectedRepair {
                reason: AdmissionError::Busy,
                work: Box::new(work),
            });
        }
        let Some(next) = state.next_repair.checked_add(1) else {
            return Err(RejectedRepair {
                reason: AdmissionError::Exhausted,
                work: Box::new(work),
            });
        };
        let sequence = state.next_repair;
        state.next_repair = next;
        self.shared.repair_cancelled.store(false, Ordering::Release);
        state.repair = Some(Repair {
            id: sequence,
            deadline,
            finished: None,
            phase: RepairPhase::Queued(Box::new(work)),
        });
        self.shared.wake.notify_one();
        Ok(RepairId {
            owner: self.owner.clone(),
            sequence,
        })
    }

    /// Caller must invalidate staged controller authority at trusted forget
    /// entry *before* this admission, and suppress stale identity publication.
    /// An accepted request survives its response deadline. Equal IDs with the
    /// same normalized name refer to the first operation only while it is live
    /// or its result is untaken; its original time and deadline are retained.
    /// After acknowledgement, use forget_status: the same ID is StaleId for
    /// mutation regardless of the new name, even before its receipt is evicted.
    pub fn queue_forget(
        &self,
        id: ForgetId,
        name: &str,
        asked_unix_s: u64,
        deadline: Instant,
    ) -> Result<Admission, AdmissionError> {
        let mut state = self.admit()?;
        let live = state.forget.as_ref().is_some_and(|forget| forget.id == id);
        if !live && id.0 <= state.highest_forget {
            return Err(AdmissionError::StaleId);
        }
        if name.len() > MOST_NAME_BYTES {
            return Err(AdmissionError::InvalidName);
        }
        let name = plain_name(name).ok_or(AdmissionError::InvalidName)?;
        let retry = |original: &str| {
            if same_name(original, &name) {
                Ok(Admission::Existing)
            } else {
                Err(AdmissionError::Conflict)
            }
        };
        if let Some(forget) = &state.forget
            && forget.id == id
        {
            return retry(&forget.name);
        }
        if state.forget.is_some() {
            return Err(AdmissionError::Busy);
        }
        state.highest_forget = id.0;
        state.forget = Some(Forget {
            id,
            name,
            asked_unix_s,
            deadline,
            finished: None,
            phase: ForgetPhase::Queued,
        });
        // Latch until a *new* repair is admitted. If forget runs before a queued
        // repair, even a failed forget cannot let that older repair start later.
        self.shared.repair_cancelled.store(true, Ordering::Release);
        self.shared.wake.notify_one();
        Ok(Admission::Accepted)
    }

    pub fn repair_status(&self, id: &RepairId) -> Result<RepairStatus, ReadError> {
        if !Arc::ptr_eq(&self.owner, &id.owner) {
            return Err(ReadError::Unknown);
        }
        let state = self.state()?;
        if let Some(repair) = &state.repair
            && repair.id == id.sequence
        {
            return Ok(repair.status((self.clock)(), self.shared.failed.load(Ordering::Acquire)));
        }
        state
            .receipts
            .iter()
            .find_map(|receipt| match receipt {
                Receipt::Repair { id: kept, status } if *kept == id.sequence => Some(*status),
                _ => None,
            })
            .ok_or(ReadError::Unknown)
    }

    pub fn forget_status(&self, id: ForgetId) -> Result<ForgetStatus, ReadError> {
        let state = self.state()?;
        if let Some(forget) = &state.forget
            && forget.id == id
        {
            return Ok(forget.status((self.clock)(), self.shared.failed.load(Ordering::Acquire)));
        }
        state
            .receipts
            .iter()
            .find_map(|receipt| match receipt {
                Receipt::Forget { id: kept, status } if *kept == id => Some(*status),
                _ => None,
            })
            .ok_or(ReadError::Unknown)
    }

    pub fn take_repair(&self, id: &RepairId) -> Result<Option<RepairCompletion>, ReadError> {
        if !Arc::ptr_eq(&self.owner, &id.owner) {
            return Err(ReadError::Unknown);
        }
        let mut state = self.state()?;
        if let Some(repair) = &state.repair
            && repair.id == id.sequence
        {
            if !matches!(repair.phase, RepairPhase::Finished(_)) {
                return if self.shared.failed.load(Ordering::Acquire) {
                    Err(ReadError::WorkerFailed)
                } else {
                    Ok(None)
                };
            }
            let repair = state.repair.take().expect("matching finished repair");
            let status = repair.status((self.clock)(), false);
            let RepairPhase::Finished(done) = repair.phase else {
                unreachable!()
            };
            state.retain(Receipt::Repair {
                id: repair.id,
                status,
            });
            return Ok(Some(done));
        }
        if state.receipts.iter().any(
            |receipt| matches!(receipt, Receipt::Repair { id: kept, .. } if *kept == id.sequence),
        ) {
            Ok(None)
        } else {
            Err(ReadError::Unknown)
        }
    }

    pub fn take_forget(
        &self,
        id: ForgetId,
    ) -> Result<Option<Result<ForgetCompletion, ForgetError>>, ReadError> {
        let mut state = self.state()?;
        if let Some(forget) = &state.forget
            && forget.id == id
        {
            if !matches!(forget.phase, ForgetPhase::Finished(_)) {
                return if self.shared.failed.load(Ordering::Acquire) {
                    Err(ReadError::WorkerFailed)
                } else {
                    Ok(None)
                };
            }
            let forget = state.forget.take().expect("matching finished forget");
            let status = forget.status((self.clock)(), false);
            let ForgetPhase::Finished(done) = forget.phase else {
                unreachable!()
            };
            state.retain(Receipt::Forget { id, status });
            return Ok(Some(done));
        }
        if state
            .receipts
            .iter()
            .any(|receipt| matches!(receipt, Receipt::Forget { id: kept, .. } if *kept == id))
        {
            Ok(None)
        } else {
            Err(ReadError::Unknown)
        }
    }

    /// Stop admitting and drain accepted operations. This shutdown call takes
    /// only the short state lock, never waits for filesystem work or joins.
    /// Do not reopen the store until stopped (or the old process exited).
    pub fn close(&self) {
        let _state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.shared.closing.store(true, Ordering::Release);
        self.shared.wake.notify_one();
    }

    pub fn stopped(&self) -> bool {
        self.shared.stopped.load(Ordering::Acquire)
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.close();
    }
}

// Narrow synchronization/fault seam; no alternate filesystem or I/O framework.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Checkpoint {
    Started,
    Repair(RepairStep),
    RepairReturned,
    Forget,
}
enum Job {
    Repair {
        work: Box<CommitWork>,
        deadline: Instant,
    },
    Forget {
        name: String,
        asked_unix_s: u64,
        deadline: Instant,
    },
}

fn run(
    mut people: People,
    shared: &Shared,
    clock: &Clock,
    mut hook: impl FnMut(Checkpoint) -> io::Result<()>,
) {
    let _ = hook(Checkpoint::Started);
    loop {
        let job = {
            let mut state = shared.state.lock().expect("writer state");
            loop {
                // One priority slot, selected under the same lock as admission.
                // No acknowledgement/poll from the requester is needed.
                if let Some(forget) = &mut state.forget
                    && matches!(forget.phase, ForgetPhase::Queued)
                {
                    forget.phase = ForgetPhase::Running;
                    break Job::Forget {
                        name: forget.name.clone(),
                        asked_unix_s: forget.asked_unix_s,
                        deadline: forget.deadline,
                    };
                }
                if let Some(repair) = &mut state.repair
                    && matches!(repair.phase, RepairPhase::Queued(_))
                {
                    let RepairPhase::Queued(work) =
                        std::mem::replace(&mut repair.phase, RepairPhase::Running)
                    else {
                        unreachable!()
                    };
                    break Job::Repair {
                        work,
                        deadline: repair.deadline,
                    };
                }
                if shared.closing.load(Ordering::Acquire) {
                    return;
                }
                state = shared.wake.wait(state).expect("writer state");
            }
        };
        match job {
            Job::Repair { work, deadline } => {
                let (target, prints, completion) = (*work).into_parts();
                let result =
                    if clock() >= deadline || shared.repair_cancelled.load(Ordering::Acquire) {
                        Err(RepairError::NotWritten)
                    } else {
                        people.replace_prints_at(target, prints, |step| {
                            hook(Checkpoint::Repair(step))?;
                            // This can refuse before rename, not interrupt an I/O
                            // already in progress. After rename RAM/directory sync
                            // must keep their actual success/indeterminate meaning.
                            if step != RepairStep::SyncDirectory
                                && (clock() >= deadline
                                    || shared.repair_cancelled.load(Ordering::Acquire))
                            {
                                return Err(io::Error::new(
                                    io::ErrorKind::Interrupted,
                                    "repair no longer timely",
                                ));
                            }
                            Ok(())
                        })
                    };
                // A test may hold delivery after real store return. Its error
                // cannot change that already known result.
                let _ = hook(Checkpoint::RepairReturned);
                let finished = clock();
                let mut state = shared.state.lock().expect("writer state");
                let repair = state.repair.as_mut().expect("running repair");
                repair.finished = Some(finished);
                repair.phase = RepairPhase::Finished(RepairCompletion {
                    completion,
                    result,
                    deadline_exceeded: finished >= deadline,
                });
            }
            Job::Forget {
                name,
                asked_unix_s,
                deadline,
            } => {
                let stable_id = people.id_of(&name);
                let result = if hook(Checkpoint::Forget).is_err() {
                    Err(ForgetError::Storage)
                } else {
                    people
                        .forget_at(&name, asked_unix_s)
                        .map_err(|_| ForgetError::Storage)
                };
                let finished = clock();
                let result = result.map(|result| ForgetCompletion {
                    result,
                    stable_id,
                    name_id_free: people.name_id_free(&name),
                    deadline_exceeded: finished >= deadline,
                });
                // Release the worker's execution copy before the result can be
                // acknowledged. Only the live record keeps a retry name now.
                drop(name);
                let mut state = shared.state.lock().expect("writer state");
                let forget = state.forget.as_mut().expect("running forget");
                forget.finished = Some(finished);
                forget.phase = ForgetPhase::Finished(result);
            }
        }
    }
}

#[cfg(test)]
mod tests;
