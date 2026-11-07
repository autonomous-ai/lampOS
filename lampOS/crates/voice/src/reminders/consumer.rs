//! Disk and socket work runs here, outside Voice's urgent loop. This module
//! never grants audio authority and never infers completion from a timeout.
use super::{
    FromWorker, JOINED_TEXT_BYTES, Joined, LEASE_US, MAX_JOINED, Offer, ToWorker, json_bytes,
    transport::WorkerTransport,
};
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
        ItemEvidence, PendingEvidence, PreparedToken, QualifiedCompletion, QualifiedRetirement,
        Reconciled, RetirementBasis,
    },
    engine::Persistence,
    protocol::Request,
    store::Store,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const DUE_AGE_US: u64 = 3_000_000;
const TICK: Duration = Duration::from_millis(200);
const REQUEST_BUDGET: Duration = Duration::from_millis(250);
const PACE_US: u64 = 5_000_000;
const QUEUE: usize = 4;
/// What of a tick's budget joining late ones leaves the offer (29e7).
const JOIN_SPARE: Duration = Duration::from_millis(100);

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

/// Said late enough to join others and wait for a pause in the talk, as
/// [`Offer::late_line`] says it.
fn late_line(late: bool, late_by_ms: Option<u64>) -> bool {
    late_by_ms.is_none_or(|ms| late && ms >= lamp_schedule::engine::SAID_LATE_MS)
}

/// A due item given up with nothing proven a second time: it may have been
/// heard twice.
const HELD: &str = "held: an attempt may have been heard and was never proven said, \
                    so it is not offered again";
/// A due item the person stopped over its ring, a spoken stop, the head or
/// the button, after some of it played: they heard it (Seventh's P3 on
/// #5258, with `LAMP_RING_STOP_ENDS_IT`).
const STOPPED_HELD: &str = "held: stopped by the person over its ring, so it is not \
                            offered again";
/// A fenced cut-in after some of its audio: once more, as a repeat.
const CUT_ONCE_MORE: &str = "not completed: this attempt can no longer speak, cut after some of \
                             it played: to be offered again once, said as a repeat";
/// A line cut in on an attempt after some of its audio, less than a real
/// share (an echo's or a cough's cut, Heron's P2 on #5509): offered again,
/// not counted toward the hold, this many times for each item; the next
/// light cut holds it ([`LIGHT_CUT_HELD`]).
const LIGHT_CUT_RETRIES: u8 = 3;
/// Its held line.
const LIGHT_CUT_HELD: &str = "held: cut early by a line each time it was offered, \
                              so it is not offered again";
/// A due item given up with nothing proven the first time (#5219).
const ONCE_MORE: &str = "not completed: given up before it was proven said, \
                         to be offered once more";
/// Attempts given up before any of their audio came are offered again this
/// many times for each item, without counting toward the once more; the
/// next is held ([`NO_AUDIO_HELD`]).
const NO_AUDIO_RETRIES: u8 = 5;
/// Its journal line, the `n`th time of [`NO_AUDIO_RETRIES`].
fn no_audio_again(n: u8) -> String {
    format!(
        "not completed: given up before any of it was played, to be offered again \
         ({n} of {NO_AUDIO_RETRIES})"
    )
}
/// A due item given up before any of its audio came more often than
/// [`NO_AUDIO_RETRIES`] allows: whatever keeps it from being said is not
/// passing, and it waits to be resolved.
const NO_AUDIO_HELD: &str = "held: given up again before any of it was played, \
                             after every try allowed, so it is not offered again";

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
    /// Items an attempt of was given up with nothing proven after some of
    /// its audio came, in this process ([`Self::abandoned`]): a second is
    /// held.
    abandoned: BTreeSet<DeliveryKey>,
    /// How often each item was offered again after an attempt given up
    /// before any of its audio came ([`NO_AUDIO_RETRIES`]).
    unvoiced: BTreeMap<DeliveryKey, u8>,
    /// How often each item was offered again after a light cut
    /// ([`LIGHT_CUT_RETRIES`]).
    light_cuts: BTreeMap<DeliveryKey, u8>,
    /// The attempts the last cut put on the retry, its own first and those
    /// joined to it: a stop the person said for that cut holds them
    /// ([`ToWorker::RingStopped`]).
    last_cut: Vec<PreparedToken>,
    next_offer_us: u64,
    cursor: u64,
    recovery: usize,
    outgoing: VecDeque<FromWorker>,
    /// A line was heard after a reminder: Schedule is told once every
    /// completed one is acknowledged ([`Self::tell_noticed`]).
    notice_owed: bool,
    /// With `LAMP_LEDGER_REFENCE` on, the file kept while the ledger refuses
    /// every item, so a timer is not set to never ring (#5560's fallback),
    /// and whether it was last kept (`None`: not yet judged).
    cannot_ring: Option<(PathBuf, Option<bool>)>,
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
            abandoned: BTreeSet::new(),
            unvoiced: BTreeMap::new(),
            light_cuts: BTreeMap::new(),
            last_cut: Vec::new(),
            next_offer_us: 0,
            cursor: 0,
            recovery: 0,
            outgoing: VecDeque::new(),
            notice_owed: false,
            cannot_ring: None,
        }
    }
    /// Keeps [`Self::cannot_ring`]'s file exactly while an item at the
    /// store's revision would be refused for the ledger's own fence.
    fn judge_cannot_ring(&mut self, due: &ScheduleDue) -> io::Result<()> {
        let Some((file, kept)) = &mut self.cannot_ring else {
            return Ok(());
        };
        let refuses = self.ledger.refuses(&due.store_id, due.revision);
        if *kept == Some(refuses) {
            return Ok(());
        }
        if refuses {
            std::fs::write(&*file, b"")?;
        } else if let Err(error) = std::fs::remove_file(&*file)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error);
        }
        *kept = Some(refuses);
        self.journal.say(
            if refuses {
                "the ledger refuses every item (its fence is ahead of the store): new timers and reminders are refused"
            } else {
                "the ledger takes items: timers and reminders may be set"
            }
            .into(),
        );
        Ok(())
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
            | ToWorker::Abandon { token, .. } => token,
            ToWorker::Noticed {} => {
                self.notice_owed = true;
                return Ok(());
            }
            ToWorker::RingStopped { token } => {
                self.ring_stopped(token.clone());
                return Ok(());
            }
        };
        if self
            .current
            .as_ref()
            .is_none_or(|offer| &offer.token != token)
        {
            return Err(invalid("consumer command has no matching current delivery"));
        }
        let token = token.clone();
        let joined = self
            .current
            .as_ref()
            .map(|offer| offer.also.clone())
            .unwrap_or_default();
        self.last_cut = std::iter::once(token.clone())
            .chain(joined.iter().map(|joined| joined.token.clone()))
            .collect();
        match command {
            ToWorker::Complete { proof, .. } => {
                self.ledger.complete(&token, &proof).map_err(invalid)?;
                self.journal.item(
                    &token.key,
                    "said; its completion is kept, to be acknowledged",
                );
                // The others said in the same line, on the same proof:
                // each under its own attempt.
                for joined in joined {
                    let proof = QualifiedCompletion {
                        correlation: joined.token.correlation.clone(),
                        ..proof.clone()
                    };
                    match self.ledger.complete(&joined.token, &proof) {
                        Ok(_) => self.journal.item(
                            &joined.token.key,
                            "said in one line with another late one; its completion is kept, to be acknowledged",
                        ),
                        Err(error) => self
                            .journal
                            .item(&joined.token.key, format!("{HELD} ({error})")),
                    }
                }
            }
            ToWorker::Retire {
                proof,
                heard_some,
                light_cut,
                ..
            } => {
                // Validate the exact correlation now; durable retry validates
                // all qualification fields again before a later admission.
                if proof.correlation != token.correlation
                    || proof.retired_by_session != self.session
                {
                    return Err(invalid("consumer retirement correlation differs"));
                }
                if heard_some || light_cut {
                    self.prune_abandoned();
                }
                for joined in joined {
                    let proof = QualifiedRetirement {
                        correlation: joined.token.correlation.clone(),
                        retired_by_session: proof.retired_by_session.clone(),
                        basis: proof.basis.clone(),
                    };
                    self.retire(joined.token, proof, heard_some, light_cut);
                }
                self.retire(token.clone(), proof, heard_some, light_cut);
            }
            ToWorker::Abandon {
                no_audio, stopped, ..
            } => {
                for joined in joined {
                    self.abandoned(joined.token, no_audio, stopped);
                }
                self.abandoned(token.clone(), no_audio, stopped);
            }
            ToWorker::Noticed {} | ToWorker::RingStopped { .. } => {
                unreachable!("answered above")
            }
        }
        self.current = None;
        self.outgoing
            .retain(|old| !matches!(old, FromWorker::Offer { .. } | FromWorker::Withdraw { .. }));
        self.next_offer_us = now.saturating_add(PACE_US);
        self.push(FromWorker::Settled { token })
    }
    /// The voice gave `token`'s attempt up with nothing proven: its reply
    /// ended, cut by talk or otherwise, with no completion and no fenced
    /// retirement, so it may have been heard. The first time, it is offered
    /// once more under a new attempt, after the pacing, as a retired one is
    /// (stable 9980 on lamp-52e6, #5219: a one-minute timer cut by talk was
    /// held and never rang); the second time it is held, so it is said
    /// twice at most. One given up before any of its audio came (`no_audio`:
    /// lamp-9adf, build 10012, a ring whose chat had not begun in 3 s ended
    /// `GaveUp`) was certainly not heard: it is offered again without
    /// counting toward that bound, [`NO_AUDIO_RETRIES`] times, then held.
    /// Counted in this process only: a later voice offers every attempt an
    /// earlier one left again ([`Self::retire_earlier_voices`]), held ones
    /// too.
    /// The person's stop was said for the ring `token`'s attempt, which their
    /// line cut and put on the retry (a cut-in, `Retire`, or a cut by talk,
    /// `Abandon`): they heard it. That attempt and those joined to it, not
    /// offered again yet, are taken off the retry and held. One already
    /// offered again, or a stop for another attempt, changes nothing.
    fn ring_stopped(&mut self, token: PreparedToken) {
        if self.last_cut.first() != Some(&token) {
            return;
        }
        for cut in std::mem::take(&mut self.last_cut) {
            if self
                .retry
                .get(&cut.key)
                .is_some_and(|(waiting, _)| waiting == &cut)
            {
                self.retry.remove(&cut.key);
                self.abandoned.insert(cut.key.clone());
                self.journal.item(&cut.key, STOPPED_HELD);
            }
        }
    }
    /// Only items still waiting to be said are counted, so the counts stay
    /// within the ledger's bound.
    fn prune_abandoned(&mut self) {
        let ledger = &self.ledger;
        let waiting = |key: &DeliveryKey| {
            ledger.view(key).map_or(true, |view| {
                view.is_some_and(|view| view.completion.is_none() && view.resolution.is_none())
            })
        };
        self.abandoned.retain(|key| waiting(key));
        self.unvoiced.retain(|key, _| waiting(key));
        self.light_cuts.retain(|key, _| waiting(key));
    }
    /// A fenced retirement of `token`'s attempt: offered again. One a line
    /// cut in on after some of its audio came (`heard_some`) is offered
    /// once more only, as a repeat ([`Offer::again`]), as an abandon with
    /// audio is: the second such cut holds it (06c1, 5 Nov 17:04).
    /// A light cut (`light_cut`: some audio, under a real share, as an
    /// echo's or a cough's cut makes) never counts toward that hold: it is
    /// offered again as before, at most [`LIGHT_CUT_RETRIES`] times for each
    /// item, then held (Heron's P2 on #5509).
    fn retire(
        &mut self,
        token: PreparedToken,
        proof: QualifiedRetirement,
        heard_some: bool,
        light_cut: bool,
    ) {
        if heard_some && !self.abandoned.insert(token.key.clone()) {
            self.journal.item(&token.key, HELD);
            return;
        }
        if light_cut && !heard_some {
            let tries = self.light_cuts.entry(token.key.clone()).or_default();
            if *tries >= LIGHT_CUT_RETRIES {
                self.journal.item(&token.key, LIGHT_CUT_HELD);
                return;
            }
            *tries += 1;
        }
        self.journal.item(
            &token.key,
            if heard_some {
                CUT_ONCE_MORE
            } else {
                "not completed: this attempt can no longer speak, to be offered again"
            },
        );
        self.retry.insert(token.key.clone(), (token, proof));
    }
    fn abandoned(&mut self, token: PreparedToken, no_audio: bool, stopped: bool) {
        self.prune_abandoned();
        // Stopped by the person over its ring: heard, and not rung again.
        if stopped && !no_audio {
            self.abandoned.insert(token.key.clone());
            self.journal.item(&token.key, STOPPED_HELD);
            return;
        }
        let line = if no_audio {
            let tries = self.unvoiced.entry(token.key.clone()).or_default();
            if *tries >= NO_AUDIO_RETRIES {
                self.journal.item(&token.key, NO_AUDIO_HELD);
                return;
            }
            *tries += 1;
            no_audio_again(*tries)
        } else {
            if !self.abandoned.insert(token.key.clone()) {
                self.journal.item(&token.key, HELD);
                return;
            }
            ONCE_MORE.to_owned()
        };
        let proof = QualifiedRetirement {
            correlation: token.correlation.clone(),
            retired_by_session: self.session.clone(),
            basis: RetirementBasis::Abandoned {},
        };
        self.journal.item(&token.key, line);
        self.retry.insert(token.key.clone(), (token, proof));
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
        let (token, also) = if let Some(current) = &self.current {
            if &current.token.key != pending.key() || current.text != text {
                return Err(invalid("Schedule occurrence changed during speech"));
            }
            (current.token.clone(), current.also.clone())
        } else {
            let token = self.prepare(&pending)?;
            // Said late: the others as late are said with it, in one line
            // (29e7, 2 Nov).
            let room = JOINED_TEXT_BYTES.saturating_sub(json_bytes(&text));
            let also = if late_line(late, late_by_ms) && room > 0 {
                self.join_late(socket, &token.key, room, deadline)
            } else {
                Vec::new()
            };
            (token, also)
        };
        let expires_at_us = now
            .saturating_add(LEASE_US)
            .min(due.stamp.at_us.saturating_add(DUE_AGE_US));
        // Cut after some of it played and offered once more: a repeat.
        let again = std::iter::once(&token.key)
            .chain(also.iter().map(|joined| &joined.token.key))
            .any(|key| self.abandoned.contains(key));
        let offer = Offer {
            token,
            text,
            late,
            late_by_ms,
            observed_at_us: now,
            expires_at_us,
            also,
            again,
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
                "due, offered to the voice (attempt {}{}{}{})",
                offer.token.attempt,
                if offer.late { ", late" } else { "" },
                match offer.also.len() {
                    0 => String::new(),
                    joined => format!(", {joined} more as late in the same line"),
                },
                if offer.again {
                    ", said as a repeat"
                } else {
                    ""
                }
            ),
        );
        self.current = Some(offer.clone());
        self.push(FromWorker::Offer { offer })
    }
    /// A new attempt for `pending`: again, under a new correlation, for one
    /// retired before, or a first one.
    fn prepare(&mut self, pending: &PendingEvidence) -> io::Result<PreparedToken> {
        if let Some((previous, proof)) = self.retry.remove(pending.key()) {
            let correlation = self.correlation()?;
            match self
                .ledger
                .retry_interrupted(&previous, pending, &proof, correlation)
            {
                Ok(token) => Ok(token),
                Err(error) => {
                    // Kept for the next tick, not held for ever.
                    self.retry.insert(pending.key().clone(), (previous, proof));
                    Err(invalid(error))
                }
            }
        } else {
            let correlation = self.correlation()?;
            self.ledger.prepare(pending, correlation).map_err(invalid)
        }
    }

    /// The other pending reminders as late as a late line ([`late_line`]),
    /// oldest first, at most [`MAX_JOINED`] and `room` bytes of text
    /// ([`JOINED_TEXT_BYTES`]), each with its own attempt prepared, to be
    /// said in one line with `main` (29e7, 2 Nov). One the
    /// ledger holds, or that can't be read or prepared now, is left to be
    /// said on its own, as before.
    fn join_late(
        &mut self,
        socket: &Path,
        main: &DeliveryKey,
        mut room: usize,
        deadline: Instant,
    ) -> Vec<Joined> {
        let request = Request::List {
            after_id: 0,
            limit: 8,
            pending_only: true,
        };
        let Ok(page) = exchange(socket, &request, deadline) else {
            return Vec::new();
        };
        if page["ok"] != true || page["store_id"].as_str() != Some(main.store_id.as_str()) {
            return Vec::new();
        }
        let mut joined = Vec::new();
        // Said in the order they were due.
        let mut items: Vec<&Value> = page["items"].as_array().into_iter().flatten().collect();
        items.sort_by_key(|item| item["due_unix_ms"].as_i64());
        for item in items {
            // Never at the cost of the line itself: joining stops while the
            // tick's budget still has room for the offer's own freshness.
            if joined.len() >= MAX_JOINED
                || deadline.saturating_duration_since(Instant::now()) < JOIN_SPARE
            {
                break;
            }
            let (Some(id), Some(delivery)) = (item["id"].as_u64(), item["delivery"].as_u64())
            else {
                continue;
            };
            let late_by_ms = item["late_by_ms"].as_u64();
            if id == main.id
                || item["state"] != "pending"
                || !late_line(item["late"] == true, late_by_ms)
                || (late_by_ms.is_none() && !item["late_by_ms"].is_null())
            {
                continue;
            }
            let key = DeliveryKey {
                store_id: main.store_id.clone(),
                id,
                delivery,
            };
            let unheld = self.retry.contains_key(&key)
                || self.ledger.view(&key).is_ok_and(|view| view.is_none());
            if !unheld {
                continue;
            }
            let request = Request::Get { id };
            let Ok(value) = exchange(socket, &request, deadline) else {
                continue;
            };
            let Ok(pending) = PendingEvidence::from_get(&request, &value) else {
                continue;
            };
            let Some(text) = value["item"]["text"].as_str().map(str::to_owned) else {
                continue;
            };
            if pending.key() != &key || json_bytes(&text) > room {
                continue;
            }
            match self.prepare(&pending) {
                Ok(token) => {
                    room -= json_bytes(&text);
                    joined.push(Joined { token, text });
                }
                Err(error) => self
                    .journal
                    .item(&key, format!("not joined to a late line: {error}")),
            }
        }
        joined
    }

    /// Schedule told a line was heard after a reminder ([`Request::Noticed`]),
    /// once each one said is acknowledged, so the answer finds it kept.
    fn tell_noticed(&mut self, socket: &Path, deadline: Instant) -> io::Result<()> {
        if !self.notice_owed {
            return Ok(());
        }
        let unacknowledged = self
            .ledger
            .entries()
            .map_err(invalid)?
            .iter()
            .any(|view| view.completion.is_some() && view.resolution.is_none());
        if unacknowledged {
            return Ok(());
        }
        if let Ok(reply) = exchange(socket, &Request::Noticed {}, deadline) {
            // An older Schedule refuses it: it kept none to notice.
            self.notice_owed = false;
            if reply["noticed"].as_u64().is_some_and(|n| n > 0) {
                self.journal.say(format!(
                    "a line was heard after {} reminder(s) said late: noticed",
                    reply["noticed"]
                ));
            }
        }
        Ok(())
    }

    fn tick(&mut self, socket: &Path, due: Option<&ScheduleDue>, now: u64) -> io::Result<()> {
        let deadline = request_deadline();
        self.tell_noticed(socket, deadline)?;
        let Some(due) = due else {
            return self.withdraw();
        };
        self.judge_cannot_ring(due)?;
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
        // The oldest is tried first; one that cannot be offered now (refused
        // by the ledger, e.g. `stale_revision`, or by its own evidence) is
        // journaled and passed over like a held one, never allowed to
        // silence every later item (#5498).
        let tried = if self.known(&key)? {
            self.held(&key)?;
            None
        } else if self.offered(socket, due, &key, now, deadline)? {
            return Ok(());
        } else {
            Some(key)
        };
        // A crash-uncertain or unofferable oldest occurrence is not allowed to
        // starve every other pending item. Scan one bounded page per tick; Get
        // still binds the chosen exact current occurrence.
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
            if tried.as_ref() == Some(&key) {
                continue;
            }
            if !self.known(&key)? {
                if self.offered(socket, due, &key, now, deadline)? {
                    return Ok(());
                }
                continue;
            }
            self.held(&key)?;
        }
        self.cursor = page["next_after_id"]
            .as_u64()
            .filter(|id| *id == last && *id > self.cursor)
            .unwrap_or(0);
        Ok(())
    }
    /// Offer `key`; true when this tick is done with offers: it was offered,
    /// or an offer it left current was withdrawn. False when it failed with
    /// nothing current, journaled once per distinct reason, so the scan
    /// moves on to the next due item.
    fn offered(
        &mut self,
        socket: &Path,
        due: &ScheduleDue,
        key: &DeliveryKey,
        now: u64,
        deadline: Instant,
    ) -> io::Result<bool> {
        let Err(error) = self.offer(socket, due, key, now, deadline) else {
            return Ok(true);
        };
        self.journal.item(key, format!("not offered: {error}"));
        if self.current.is_none() {
            return Ok(false);
        }
        self.withdraw()?;
        Ok(true)
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
    let mut ledger = DeliveryLedger::open(Store::open(ledger_dir)?, &session).map_err(invalid)?;
    let refence = super::refence_on(std::env::var(super::REFENCE_VARIABLE).ok().as_deref());
    if refence {
        ledger.refence_at_start();
    }
    let mut consumer = Consumer::new(ledger, session);
    if refence {
        consumer.cannot_ring = Some((ledger_dir.join(super::CANNOT_RING_FILE), None));
    }
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
