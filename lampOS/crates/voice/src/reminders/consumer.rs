//! Disk and socket work runs here, outside Voice's urgent loop. This module
//! never grants audio authority and never infers completion from a timeout.
use super::{FromWorker, LEASE_US, Offer, ToWorker, transport::WorkerTransport};
#[cfg(test)]
use crate::scenario_clock::monotonic_us;
use lamp_bus::{Bus, Kind, Message};
#[cfg(not(test))]
use lamp_ipc::monotonic_us;
use lamp_msgs::ScheduleDue;
use lamp_schedule::{
    client,
    delivery::{
        AbsentEvidence, AckEvidence, Correlation, DeliveryKey, DeliveryLedger, Evidence,
        ItemEvidence, PendingEvidence, PreparedToken, QualifiedRetirement, Reconciled,
        RetirementBasis,
    },
    engine::Persistence,
    protocol::Request,
    store::Store,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    path::Path,
    time::{Duration, Instant},
};

const DUE_AGE_US: u64 = 3_000_000;
const TICK: Duration = Duration::from_millis(200);
const REQUEST_BUDGET: Duration = Duration::from_millis(250);
const PACE_US: u64 = 5_000_000;
const QUEUE: usize = 4;

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}
/// One tick's socket budget. It also spans the ledger's durable writes
/// between exchanges, so a slow disk spends it (lampOS#935). A stepped test
/// scenario spends no logical time inside one tick, so there the budget cannot
/// run out, and the scope's host watchdog bounds the real waits instead.
fn request_deadline() -> Instant {
    #[cfg(test)]
    if let Some(deadline) = crate::scenario_clock::host_deadline() {
        return deadline;
    }
    Instant::now() + REQUEST_BUDGET
}
fn exchange(socket: &Path, request: &Request, deadline: Instant) -> io::Result<Value> {
    client::exchange(socket, &serde_json::to_value(request)?, deadline).map_err(invalid)
}

#[derive(Default)]
struct Freshness {
    publisher: Option<(String, u64, u64, u64)>,
    payload: Vec<u8>,
    retired: Vec<String>,
}
impl Freshness {
    fn accept(&mut self, message: &Message, clock: &str, now: u64) -> io::Result<ScheduleDue> {
        let due: ScheduleDue = message.parse()?;
        due.check().map_err(invalid)?;
        let header = &message.header;
        if message.expired
            || message.payload.len() > 4096
            || header.topic != ScheduleDue::TOPIC
            || header.kind != Kind::Latest
            || header.source != "schedule"
            || header.type_name.as_deref() != Some(ScheduleDue::TYPE)
            || header.expires_ms != Some(3_000)
            || due.stamp.clock_id != clock
            || now
                .checked_sub(due.stamp.at_us)
                .is_none_or(|age| age > DUE_AGE_US)
            || due.stamp.sequence == 0
            || !due.enabled
            || !due.ready
        {
            return Err(invalid("Schedule due authority is unavailable"));
        }
        if let Some((boot, header_seq, stamp_seq, stamp_at)) = &self.publisher {
            if boot == &header.boot {
                if header.seq < *header_seq
                    || due.stamp.sequence < *stamp_seq
                    || due.stamp.at_us < *stamp_at
                {
                    return Err(invalid("Schedule due ordering regressed"));
                }
                if header.seq > *header_seq && due.stamp.sequence <= *stamp_seq {
                    return Err(invalid("Schedule due typed sequence reused"));
                }
                if header.seq == *header_seq
                    && (due.stamp.sequence != *stamp_seq
                        || due.stamp.at_us != *stamp_at
                        || message.payload != self.payload)
                {
                    return Err(invalid("Schedule due publisher reused a sequence"));
                }
            } else {
                if self.retired.len() >= 8 || self.retired.contains(&header.boot) {
                    return Err(invalid(
                        "Schedule due publisher history exhausted or replayed",
                    ));
                }
                self.retired.push(boot.clone());
            }
        }
        self.publisher = Some((
            header.boot.clone(),
            header.seq,
            due.stamp.sequence,
            due.stamp.at_us,
        ));
        self.payload.clone_from(&message.payload);
        Ok(due)
    }
    fn read(&mut self, bus: &Bus, clock: &str, now: u64) -> io::Result<ScheduleDue> {
        if bus.declaration(ScheduleDue::TOPIC)? != Some(ScheduleDue::declaration()) {
            return Err(invalid("Schedule due declaration differs"));
        }
        let message = bus
            .latest(ScheduleDue::TOPIC)?
            .ok_or_else(|| invalid("Schedule due is missing"))?;
        let publishers = bus.publishers(ScheduleDue::TOPIC)?;
        if publishers.len() != 1
            || publishers[0].name != "schedule"
            || publishers[0].boot != message.header.boot
        {
            return Err(invalid("Schedule due needs one current publisher"));
        }
        self.accept(&message, clock, now)
    }
}

/// A due item given up with nothing proven: it may have been heard.
const HELD: &str = "held: an attempt may have been heard and was never proven said, \
                    so it is not offered again";

/// What the consumer did with each due item, and why due work is not
/// available when it is not: a line in the journal when either changes,
/// never one per tick (the first builder, 2026-10-16: due reminders on
/// lamp-4ace were never said and nothing said where they stopped).
struct Journal {
    /// Each item's last line, and when it was written, in lines.
    items: BTreeMap<DeliveryKey, (String, u64)>,
    written: u64,
    /// Why due work was last unavailable; at start, that it is not read
    /// yet, so the first good read is said.
    unavailable: Option<String>,
    #[cfg(test)]
    lines: Vec<String>,
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            items: BTreeMap::new(),
            written: 0,
            unavailable: Some("not read yet".into()),
            #[cfg(test)]
            lines: Vec::new(),
        }
    }
}
impl Journal {
    /// Items remembered at once; past it the item written longest ago is
    /// forgotten, and its next line may repeat.
    const ITEMS: usize = 64;

    fn item(&mut self, key: &DeliveryKey, what: impl Into<String>) {
        let what = what.into();
        if self.items.get(key).is_some_and(|(said, _)| said == &what) {
            return;
        }
        if self.items.len() >= Self::ITEMS
            && !self.items.contains_key(key)
            && let Some(oldest) = self
                .items
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .map(|(key, _)| key.clone())
        {
            self.items.remove(&oldest);
        }
        self.say(format!(
            "reminder id {} delivery {}: {what}",
            key.id, key.delivery
        ));
        self.written += 1;
        self.items.insert(key.clone(), (what, self.written));
    }
    /// Due work is unavailable for `why`, or available again with `None`.
    fn due(&mut self, why: Option<String>) {
        if self.unavailable == why {
            return;
        }
        match &why {
            Some(why) => self.say(format!("due reminders unavailable: {why}")),
            None => self.say("due reminders available".into()),
        }
        self.unavailable = why;
    }
    fn say(&mut self, line: String) {
        eprintln!("lamp-voice schedule-consumer: {line}");
        #[cfg(test)]
        self.lines.push(line);
    }
}

struct Consumer<P: Persistence> {
    journal: Journal,
    ledger: DeliveryLedger<P>,
    session: String,
    sequence: u64,
    current: Option<Offer>,
    retry: BTreeMap<DeliveryKey, (PreparedToken, QualifiedRetirement)>,
    next_offer_us: u64,
    cursor: u64,
    recovery: usize,
    outgoing: VecDeque<FromWorker>,
}
impl<P: Persistence> Consumer<P> {
    fn new(ledger: DeliveryLedger<P>, session: String) -> Self {
        Self {
            journal: Journal::default(),
            ledger,
            session,
            sequence: 0,
            current: None,
            retry: BTreeMap::new(),
            next_offer_us: 0,
            cursor: 0,
            recovery: 0,
            outgoing: VecDeque::new(),
        }
    }
    fn correlation(&mut self) -> io::Result<Correlation> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("consumer sequence exhausted"))?;
        Ok(Correlation {
            consumer_session: self.session.clone(),
            sequence: self.sequence,
        })
    }
    fn push(&mut self, message: FromWorker) -> io::Result<()> {
        // A newer lease may replace a queued older lease, never a settlement.
        if matches!(message, FromWorker::Offer { .. }) {
            self.outgoing
                .retain(|old| !matches!(old, FromWorker::Offer { .. }));
        }
        if self.outgoing.len() >= QUEUE {
            return Err(invalid("consumer output queue full"));
        }
        self.outgoing.push_back(message);
        Ok(())
    }
    fn withdraw(&mut self) -> io::Result<()> {
        if let Some(current) = &self.current {
            let token = current.token.clone();
            self.outgoing.retain(|old| {
                !matches!(old, FromWorker::Offer { .. } | FromWorker::Withdraw { .. })
            });
            self.push(FromWorker::Withdraw { token })?;
        }
        Ok(())
    }
    fn command(&mut self, command: ToWorker, now: u64) -> io::Result<()> {
        let token = match &command {
            ToWorker::Complete { token, .. }
            | ToWorker::Retire { token, .. }
            | ToWorker::Abandon { token } => token,
        };
        if self
            .current
            .as_ref()
            .is_none_or(|offer| &offer.token != token)
        {
            return Err(invalid("consumer command has no matching current delivery"));
        }
        let token = token.clone();
        match command {
            ToWorker::Complete { proof, .. } => {
                self.ledger.complete(&token, &proof).map_err(invalid)?;
                self.journal.item(
                    &token.key,
                    "said; its completion is kept, to be acknowledged",
                );
            }
            ToWorker::Retire { proof, .. } => {
                // Validate the exact correlation now; durable retry validates
                // all qualification fields again before a later admission.
                if proof.correlation != token.correlation
                    || proof.retired_by_session != self.session
                {
                    return Err(invalid("consumer retirement correlation differs"));
                }
                self.retry.insert(token.key.clone(), (token.clone(), proof));
                self.journal.item(
                    &token.key,
                    "not completed: this attempt can no longer speak, to be offered again",
                );
            }
            ToWorker::Abandon { .. } => {
                self.journal.item(&token.key, HELD);
            }
        }
        self.current = None;
        self.outgoing
            .retain(|old| !matches!(old, FromWorker::Offer { .. } | FromWorker::Withdraw { .. }));
        self.next_offer_us = now.saturating_add(PACE_US);
        self.push(FromWorker::Settled { token })
    }
    /// Attempts an earlier voice prepared and never completed are offered
    /// again (the first builder, 2026-10-16: ids 12 and 26 on lamp-4ace were
    /// prepared by voices since restarted, and stayed pending for ever).
    /// Call only while holding the consumer claim: the claim goes to the
    /// voice and is released only when it exits, so holding it proves that
    /// every earlier voice, and any sound it could make, has ended.
    /// An attempt that was heard without its completion being proven is
    /// offered again too, and may be said twice; one that is never said
    /// again is worse.
    fn retire_earlier_voices(&mut self) -> io::Result<()> {
        for view in self.ledger.entries().map_err(invalid)? {
            let token = view.prepared;
            if view.completion.is_some()
                || view.resolution.is_some()
                || token.correlation.consumer_session == self.session
            {
                continue;
            }
            let proof = QualifiedRetirement {
                correlation: token.correlation.clone(),
                retired_by_session: self.session.clone(),
                basis: RetirementBasis::PreviousProcessExited {},
            };
            self.journal.item(
                &token.key,
                "prepared by an earlier voice and never said: to be offered again",
            );
            self.retry.insert(token.key.clone(), (token, proof));
        }
        Ok(())
    }
    fn known(&self, key: &DeliveryKey) -> io::Result<bool> {
        Ok(self.ledger.view(key).map_err(invalid)?.is_some() && !self.retry.contains_key(key))
    }
    /// Say why a due item the ledger already knows is not offered.
    fn held(&mut self, key: &DeliveryKey) -> io::Result<()> {
        let Some(view) = self.ledger.view(key).map_err(invalid)? else {
            return Ok(());
        };
        let why = match (&view.completion, &view.ack, &view.resolution) {
            (_, _, Some(resolution)) => format!("resolved ({resolution:?}), not offered"),
            (Some(_), _, None) => "said, waiting to be acknowledged".to_owned(),
            (None, _, None) => HELD.to_owned(),
        };
        self.journal.item(key, why);
        Ok(())
    }
    fn offer(
        &mut self,
        socket: &Path,
        due: &ScheduleDue,
        expected: &DeliveryKey,
        now: u64,
        deadline: Instant,
    ) -> io::Result<()> {
        let request = Request::Get { id: expected.id };
        let value = exchange(socket, &request, deadline)?;
        let pending = PendingEvidence::from_get(&request, &value).map_err(invalid)?;
        if pending.key() != expected
            || pending.key().store_id != due.store_id
            || pending.revision() < due.revision
        {
            return Err(invalid("Schedule store/revision changed before offer"));
        }
        let item = &value["item"];
        let text = item["text"]
            .as_str()
            .ok_or_else(|| invalid("Schedule item has no text"))?
            .to_owned();
        let late = item["late"]
            .as_bool()
            .ok_or_else(|| invalid("Schedule item has no late flag"))?;
        let late_by_ms = match &item["late_by_ms"] {
            Value::Null => None,
            value => Some(
                value
                    .as_u64()
                    .ok_or_else(|| invalid("Schedule lateness is invalid"))?,
            ),
        };
        let token = if let Some(current) = &self.current {
            if &current.token.key != pending.key() || current.text != text {
                return Err(invalid("Schedule occurrence changed during speech"));
            }
            current.token.clone()
        } else if let Some((previous, proof)) = self.retry.remove(pending.key()) {
            let correlation = self.correlation()?;
            match self
                .ledger
                .retry_interrupted(&previous, &pending, &proof, correlation)
            {
                Ok(token) => token,
                Err(error) => {
                    // Kept for the next tick, not held for ever.
                    self.retry.insert(pending.key().clone(), (previous, proof));
                    return Err(invalid(error));
                }
            }
        } else {
            let correlation = self.correlation()?;
            self.ledger
                .prepare(&pending, correlation)
                .map_err(invalid)?
        };
        let expires_at_us = now
            .saturating_add(LEASE_US)
            .min(due.stamp.at_us.saturating_add(DUE_AGE_US));
        let offer = Offer {
            token,
            text,
            late,
            late_by_ms,
            observed_at_us: now,
            expires_at_us,
        };
        // Disk latency cannot refresh the observation that preceded it.
        if !offer.fresh(monotonic_us()) {
            self.journal.item(
                &offer.token.key,
                "withdrawn: its due notice aged while the ledger was written",
            );
            self.current = Some(offer);
            return self.withdraw();
        }
        self.journal.item(
            &offer.token.key,
            format!(
                "due, offered to the voice (attempt {}{})",
                offer.token.attempt,
                if offer.late { ", late" } else { "" }
            ),
        );
        self.current = Some(offer.clone());
        self.push(FromWorker::Offer { offer })
    }
    fn tick(&mut self, socket: &Path, due: Option<&ScheduleDue>, now: u64) -> io::Result<()> {
        let deadline = request_deadline();
        let Some(due) = due else {
            return self.withdraw();
        };
        if let Some(current) = &self.current {
            let key = current.token.key.clone();
            if let Err(error) = self.offer(socket, due, &key, now, deadline) {
                self.journal.item(&key, format!("withdrawn: {error}"));
                self.withdraw()?;
            }
            return Ok(());
        }
        // Completed-but-unacknowledged work always recovers without speech.
        self.recover_one(socket, deadline)?;
        if self.ledger.failed() {
            return Err(invalid("consumer ledger is unavailable"));
        }
        if now < self.next_offer_us {
            return Ok(());
        }
        let Some(oldest) = &due.oldest else {
            return Ok(());
        };
        let key = DeliveryKey {
            store_id: due.store_id.clone(),
            id: oldest.id,
            delivery: oldest.delivery,
        };
        if !self.known(&key)? {
            if let Err(error) = self.offer(socket, due, &key, now, deadline) {
                self.journal.item(&key, format!("not offered: {error}"));
                self.withdraw()?;
            }
            return Ok(());
        }
        self.held(&key)?;
        // A crash-uncertain oldest occurrence is held, not silently replayed
        // and not allowed to starve every other pending item. Scan one bounded
        // page per tick; Get still binds the chosen exact current occurrence.
        let request = Request::List {
            after_id: self.cursor,
            limit: 8,
            pending_only: true,
        };
        let Ok(page) = exchange(socket, &request, deadline) else {
            return Ok(());
        };
        if page["ok"] != true || page["store_id"].as_str() != Some(&due.store_id) {
            return Ok(());
        }
        let Some(items) = page["items"].as_array().filter(|items| items.len() <= 8) else {
            return Ok(());
        };
        let mut last = self.cursor;
        for item in items {
            let (Some(id), Some(delivery)) = (item["id"].as_u64(), item["delivery"].as_u64())
            else {
                return Ok(());
            };
            if id <= last || item["state"] != "pending" {
                return Ok(());
            }
            last = id;
            let key = DeliveryKey {
                store_id: due.store_id.clone(),
                id,
                delivery,
            };
            if !self.known(&key)? {
                if let Err(error) = self.offer(socket, due, &key, now, deadline) {
                    self.journal.item(&key, format!("not offered: {error}"));
                    self.withdraw()?;
                }
                return Ok(());
            }
            self.held(&key)?;
        }
        self.cursor = page["next_after_id"]
            .as_u64()
            .filter(|id| *id == last && *id > self.cursor)
            .unwrap_or(0);
        Ok(())
    }
    fn reconciled(&mut self, key: &DeliveryKey, reconciled: &Reconciled) {
        if let Reconciled::Resolved(resolution) = reconciled {
            self.journal
                .item(key, format!("resolved ({resolution:?}), not offered"));
        }
    }
    fn recover_one(&mut self, socket: &Path, deadline: Instant) -> io::Result<()> {
        let entries = self.ledger.entries().map_err(invalid)?;
        if entries.is_empty() {
            return Ok(());
        }
        let entry = &entries[self.recovery % entries.len()];
        self.recovery = self.recovery.wrapping_add(1);
        let key = &entry.prepared.key;
        if entry.resolution.is_some() {
            self.ledger.compact_resolved().map_err(invalid)?;
            return Ok(());
        }
        if let Some(ack) = &entry.ack {
            if let Ok(value) = exchange(socket, &ack.request, deadline)
                && let Ok(proof) = AckEvidence::from_reply(ack, &value)
            {
                let reconciled = self
                    .ledger
                    .reconcile(key, &Evidence::Ack(proof))
                    .map_err(invalid)?;
                self.reconciled(key, &reconciled);
                return Ok(());
            }
            if let Ok(value) = exchange(socket, &Request::Status {}, deadline)
                && let Ok(proof) = AckEvidence::from_status(ack, &value)
            {
                let reconciled = self
                    .ledger
                    .reconcile(key, &Evidence::Ack(proof))
                    .map_err(invalid)?;
                self.reconciled(key, &reconciled);
                return Ok(());
            }
        }
        let request = Request::Get { id: key.id };
        let Ok(value) = exchange(socket, &request, deadline) else {
            return Ok(());
        };
        if let Ok(item) = ItemEvidence::from_get(&request, &value) {
            match self.ledger.reconcile(key, &Evidence::Item(item.clone())) {
                Ok(Reconciled::Unresolved) => {
                    if let Some(completed) = entry.completed_token() {
                        let pending = item.pending().map_err(invalid)?;
                        // Current exact still-pending evidence proves a prior
                        // Ack did not advance this occurrence. Persist any new
                        // revision guard before transmission; retain request_id.
                        let ack = if let Some(existing) = &entry.ack {
                            if pending.revision()
                                == existing
                                    .request
                                    .guard()
                                    .expect("persisted Ack has guard")
                                    .revision
                            {
                                Ok(existing.clone())
                            } else {
                                self.ledger.rebase_ack(key, &pending)
                            }
                        } else {
                            self.ledger.plan_ack(&completed, &pending)
                        }
                        .map_err(invalid)?;
                        if let Ok(reply) = exchange(socket, &ack.request, deadline)
                            && let Ok(proof) = AckEvidence::from_reply(&ack, &reply)
                        {
                            let reconciled = self
                                .ledger
                                .reconcile(key, &Evidence::Ack(proof))
                                .map_err(invalid)?;
                            self.reconciled(key, &reconciled);
                        }
                    }
                }
                Ok(Reconciled::Resolved(resolution)) => {
                    self.journal
                        .item(key, format!("resolved ({resolution:?}), not offered"));
                    self.retry.remove(key);
                }
                Err(_) => {} // Different store/stale evidence never evicts it.
            }
        } else if value["ok"] == false && value["error"] == "not_found" {
            let request = Request::List {
                after_id: key.id - 1,
                limit: 1,
                pending_only: false,
            };
            if let Ok(value) = exchange(socket, &request, deadline)
                && let Ok(absent) = AbsentEvidence::from_list(key, &request, &value)
            {
                let reconciled = self
                    .ledger
                    .reconcile(key, &Evidence::Absent(absent))
                    .map_err(invalid)?;
                self.reconciled(key, &reconciled);
                self.retry.remove(key);
            }
        }
        Ok(())
    }
}

pub(super) fn run(ledger_dir: &Path, socket: &Path, root: &Path) -> io::Result<()> {
    client::validate_socket(socket).map_err(invalid)?;
    let mut transport = WorkerTransport::from_stdin()?;
    let session = crate::process::boot_text(crate::process::new_boot()?);
    let clock = lamp_controls::runtime::clock_id(None)?;
    let ledger = DeliveryLedger::open(Store::open(ledger_dir)?, &session).map_err(invalid)?;
    let mut consumer = Consumer::new(ledger, session);
    consumer.journal.say("started".into());
    // The claim taken above moved to the voice that started this consumer,
    // so every earlier voice has exited.
    consumer.retire_earlier_voices()?;
    let bus = Bus::new(root)?;
    let mut freshness = Freshness::default();
    let mut next = Instant::now();
    loop {
        for _ in 0..QUEUE {
            let Some(command) = transport.receive::<ToWorker>()? else {
                break;
            };
            consumer.command(command, lamp_ipc::monotonic_us())?;
        }
        if Instant::now() >= next {
            let now = lamp_ipc::monotonic_us();
            let due = match freshness.read(&bus, &clock, now) {
                Ok(due) => {
                    consumer.journal.due(None);
                    Some(due)
                }
                Err(error) => {
                    consumer.journal.due(Some(error.to_string()));
                    None
                }
            };
            consumer.tick(socket, due.as_ref(), now)?;
            if consumer.outgoing.is_empty() {
                consumer.push(FromWorker::Idle {})?;
            }
            next = Instant::now() + TICK;
        }
        for _ in 0..QUEUE {
            let Some(message) = consumer.outgoing.front() else {
                break;
            };
            if !transport.send(message)? {
                break;
            }
            consumer.outgoing.pop_front();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) mod roundtrip_tests;
