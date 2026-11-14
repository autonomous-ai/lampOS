//! One bounded model worker per run. Even a slow audit write cannot hold a
//! recall caller beyond its deadline. Accounting still finishes monotonically.
use crate::{
    Result, invalid,
    model::{Call, Client, Provider, Receipt, Reply},
};
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender},
    },
    thread::JoinHandle,
    time::Instant,
};

#[derive(Clone, Default)]
pub struct Account {
    pub charged_microusd: u64,
    pub receipts: Vec<Receipt>,
    pub stopped: bool,
}
enum Job {
    Call(Call, Instant, SyncSender<Result<Reply>>),
    Barrier(SyncSender<()>),
}
pub struct Worker {
    sender: Option<SyncSender<Job>>,
    worker: Option<JoinHandle<()>>,
    account: Arc<Mutex<Account>>,
}
impl Worker {
    pub fn start<P: Provider + Send + 'static>(mut client: Client<P>) -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let account = Arc::new(Mutex::new(Account::default()));
        let shared = account.clone();
        let worker = std::thread::Builder::new()
            .name("memory-model-client".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    match job {
                        Job::Call(call, deadline, reply) => {
                            let result = client.call(&call, deadline);
                            if let Ok(mut account) = shared.lock() {
                                account.charged_microusd = client.charged_microusd();
                                let seen = account.receipts.len();
                                account
                                    .receipts
                                    .extend_from_slice(&client.receipts()[seen..]);
                                account.stopped = client.stopped();
                            }
                            let _ = reply.send(result);
                        }
                        Job::Barrier(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
            account,
        })
    }
    pub fn stopped(&self) -> bool {
        self.account.lock().map_or(true, |a| a.stopped)
    }
    pub fn account(&self) -> Result<Account> {
        self.account
            .lock()
            .map(|a| a.clone())
            .map_err(|_| invalid("model account poisoned"))
    }
    pub fn call(&mut self, call: &Call, deadline: Instant) -> Result<Reply> {
        if self.stopped() || Instant::now() >= deadline {
            return Err(invalid("model stopped or expired before queue"));
        }
        let (reply, result) = mpsc::sync_channel(1);
        self.sender
            .as_ref()
            .ok_or_else(|| invalid("model worker closed"))?
            .try_send(Job::Call(call.clone(), deadline, reply))
            .map_err(|_| invalid("model worker busy or stopped; request was not queued"))?;
        result
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| invalid("model caller deadline expired; queued accounting remains live"))?
    }
    /// Required before a terminal run ledger/scorecard. A timed-out call may
    /// still be reconciling usage; no complete result is reported until drained.
    pub fn settle(&mut self, deadline: Instant) -> Result<Account> {
        let (done, finished) = mpsc::sync_channel(1);
        self.sender
            .as_ref()
            .ok_or_else(|| invalid("model worker closed"))?
            .try_send(Job::Barrier(done))
            .map_err(|_| invalid("model queue busy while settling"))?;
        finished
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| invalid("model accounting did not settle"))?;
        self.account()
    }
    pub fn finish(mut self, deadline: Instant) -> Result<Account> {
        let account = self.settle(deadline)?;
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| invalid("model worker panicked"))?;
        }
        if account.stopped {
            return Err(invalid("model accounting stopped the run"));
        }
        Ok(account)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.sender.take();
        // On cancellation, do not block the caller on an OS-level fsync. This
        // sole worker owns no new queue; it exits after the bounded provider
        // request/audit finishes. Such a run stays incomplete until settled.
        if self.worker.as_ref().is_some_and(|w| w.is_finished())
            && let Some(worker) = self.worker.take()
        {
            let _ = worker.join();
        }
    }
}
