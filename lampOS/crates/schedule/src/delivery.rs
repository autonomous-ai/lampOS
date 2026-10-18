//! A separate durable consumer ledger; this is not Schedule's item database.
//!
//! Use a different `Store` directory. All returned admission/acknowledgement
//! intents have been committed first. The caller owns fresh socket observations,
//! the single-consumer claim, audio authority, and qualification of whole-turn
//! completion/retirement. In particular, one retired PCM generation is not a
//! completed reply. No timeout, restart, or missing Get response completes audio.
use crate::{
    engine::Persistence,
    protocol::{Guard, MAX_REPLY_BYTES, Request},
    store::MAX_STATE_BYTES,
    time::{MAX_UNIX_MS, MIN_UNIX_MS, Repeat},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{collections::BTreeSet, fmt, io, io::Write};

mod saved;

pub const MAX_ENTRIES: usize = 128;
pub const MAX_SESSION_BYTES: usize = 80;
const KIND: &str = "lamp_schedule_delivery_ledger";
// Harmless additive metadata may keep this version. New enum values, required
// fields, larger bounds, or fields that change admission/completion/ack meaning
// must advance it. An exact saved Ack request remains strict even at this version.
const SCHEMA: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code)
    }
}
impl std::error::Error for Error {}
fn error(code: &'static str) -> Error {
    Error { code }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct DeliveryKey {
    pub store_id: String,
    pub id: u64,
    pub delivery: u64,
}
impl DeliveryKey {
    pub fn validate(&self) -> Result<(), Error> {
        if !identity(&self.store_id) || self.id == 0 || self.delivery == 0 {
            return Err(error("invalid_key"));
        }
        Ok(())
    }
}

/// An opaque mailbox correlation, allocated before Voice creates a turn.
/// The caller must never reuse a session/counter pair. Tokens additionally have
/// a ledger-owned, persisted nonwrapping attempt number.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Correlation {
    pub consumer_session: String,
    pub sequence: u64,
}
impl Correlation {
    fn check(&self) -> bool {
        session(&self.consumer_session) && self.sequence > 0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PreparedToken {
    pub key: DeliveryKey,
    pub attempt: u64,
    pub correlation: Correlation,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompletedToken {
    pub key: DeliveryKey,
    pub attempt: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AckAttempt {
    pub key: DeliveryKey,
    pub attempt: u64,
    /// Retransmit this entire request. A fresh guard is a different CAS attempt.
    pub request: Request,
}

/// Construct only at the successful, settled whole-reply boundary, after the
/// final owner has fenced cancellation/privacy and joined all provider-origin
/// playback retirements. These fields retain actual owner/speaker identities;
/// this storage module cannot independently establish audible completion.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QualifiedCompletion {
    pub correlation: Correlation,
    pub owner_boot: [u8; 16],
    pub turn: u64,
    pub microphone_generation: u64,
    pub provider_session: u64,
    pub playback_sequence: u64,
    pub final_epoch: u64,
    pub final_frame: u64,
    pub retired_generations: u64,
}
impl QualifiedCompletion {
    fn check(&self) -> bool {
        self.correlation.check()
            && self.owner_boot != [0; 16]
            && self.turn > 0
            && self.microphone_generation > 0
            && self.provider_session > 0
            && self.playback_sequence > 0
            && self.final_frame > 0
            && self.retired_generations > 0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RetirementBasis {
    NeverAdmitted {},
    OwnerFenced {
        owner_boot: [u8; 16],
        turn: u64,
        microphone_generation: u64,
    },
    PreviousProcessExited {},
}
/// An assertion by the final owner/supervisor that the old correlation can no
/// longer create sound. `PreviousProcessExited` requires verified old-process
/// exit; a timeout, stale PID, or merely reopening this ledger is insufficient.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QualifiedRetirement {
    pub correlation: Correlation,
    pub retired_by_session: String,
    pub basis: RetirementBasis,
}
impl QualifiedRetirement {
    fn check(&self) -> bool {
        self.correlation.check()
            && session(&self.retired_by_session)
            && match self.basis {
                RetirementBasis::OwnerFenced {
                    owner_boot,
                    turn,
                    microphone_generation,
                } => owner_boot != [0; 16] && turn > 0 && microphone_generation > 0,
                _ => true,
            }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Resolution {
    AckConfirmed { revision: u64 },
    Superseded { revision: u64, delivery: u64 },
    Gone { revision: u64 },
}
impl Resolution {
    fn revision(&self) -> u64 {
        match self {
            Self::AckConfirmed { revision }
            | Self::Superseded { revision, .. }
            | Self::Gone { revision } => *revision,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reconciled {
    Unresolved,
    Resolved(Resolution),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryView {
    pub prepared: PreparedToken,
    #[serde(deserialize_with = "required_option")]
    pub completion: Option<QualifiedCompletion>,
    #[serde(deserialize_with = "required_option")]
    pub ack: Option<AckAttempt>,
    #[serde(deserialize_with = "required_option")]
    pub resolution: Option<Resolution>,
}
impl DeliveryView {
    pub fn completed_token(&self) -> Option<CompletedToken> {
        (self.completion.is_some() && self.resolution.is_none()).then(|| CompletedToken {
            key: self.prepared.key.clone(),
            attempt: self.prepared.attempt,
        })
    }
}

/// Evidence constructors verify the existing response shape and exact query
/// binding. The caller still owns transport authenticity and receipt freshness.
/// None of these values renew a Voice admission lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemEvidence {
    key: DeliveryKey,
    revision: u64,
    pending: bool,
}
impl ItemEvidence {
    pub fn from_get(request: &Request, reply: &Value) -> Result<Self, Error> {
        let Request::Get { id } = request else {
            return Err(error("wrong_query"));
        };
        let reply: GetReply = parse_reply(reply)?;
        check_identity(reply.ok, &reply.store_id)?;
        reply.item.check()?;
        if *id == 0 || reply.item.id != *id || reply.revision == 0 {
            return Err(error("wrong_item"));
        }
        Ok(Self {
            key: DeliveryKey {
                store_id: reply.store_id,
                id: *id,
                delivery: reply.item.delivery,
            },
            revision: reply.revision,
            pending: reply.item.state == ItemState::Pending,
        })
    }
    pub fn key(&self) -> &DeliveryKey {
        &self.key
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn is_pending(&self) -> bool {
        self.pending
    }
    pub fn pending(&self) -> Result<PendingEvidence, Error> {
        if !self.pending {
            return Err(error("not_pending"));
        }
        Ok(PendingEvidence(self.clone()))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingEvidence(ItemEvidence);
impl PendingEvidence {
    pub fn from_get(request: &Request, reply: &Value) -> Result<Self, Error> {
        ItemEvidence::from_get(request, reply)?.pending()
    }
    pub fn key(&self) -> &DeliveryKey {
        self.0.key()
    }
    pub fn revision(&self) -> u64 {
        self.0.revision()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbsentEvidence {
    key: DeliveryKey,
    revision: u64,
}
impl AbsentEvidence {
    /// Only an unfiltered, identity-bearing page immediately preceding this ID
    /// proves absence. A failed Get, even beside a successful Status, does not.
    pub fn from_list(key: &DeliveryKey, request: &Request, reply: &Value) -> Result<Self, Error> {
        key.validate()?;
        if request
            != &(Request::List {
                after_id: key.id - 1,
                limit: 1,
                pending_only: false,
            })
        {
            return Err(error("wrong_query"));
        }
        let reply: ListReply = parse_reply(reply)?;
        check_identity(reply.ok, &reply.store_id)?;
        if reply.store_id != key.store_id || reply.items.len() > 1 {
            return Err(error("invalid_evidence"));
        }
        for item in &reply.items {
            item.check()?;
            if item.id <= key.id {
                return Err(error("not_absent"));
            }
        }
        if let Some(cursor) = reply.next_after_id
            && reply.items.first().is_none_or(|item| item.id != cursor)
        {
            return Err(error("invalid_evidence"));
        }
        Ok(Self {
            key: key.clone(),
            revision: reply.revision,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AckEvidence {
    attempt: AckAttempt,
    committed_revision: u64,
    /// A direct exact replay can return a historical receipt. Only Status's
    /// outer revision is a current store observation.
    current_revision: Option<u64>,
}
impl AckEvidence {
    pub fn from_reply(attempt: &AckAttempt, reply: &Value) -> Result<Self, Error> {
        attempt.key.validate()?;
        let reply: AckReply = parse_reply(reply)?;
        let (guard, id, delivery) = ack_parts(&attempt.request)?;
        if !reply.ok
            || !reply.acknowledged
            || attempt.attempt == 0
            || guard.store_id != attempt.key.store_id
            || id != attempt.key.id
            || delivery != attempt.key.delivery
            || !request_id(&guard.request_id)
            || reply.store_id != guard.store_id
            || reply.id != id
            || reply.delivery != delivery
            || guard.revision.checked_add(1) != Some(reply.revision)
        {
            return Err(error("invalid_ack"));
        }
        Ok(Self {
            attempt: attempt.clone(),
            committed_revision: reply.revision,
            current_revision: None,
        })
    }
    pub fn from_status(attempt: &AckAttempt, reply: &Value) -> Result<Self, Error> {
        let status: StatusReply = parse_reply(reply)?;
        check_identity(status.ok, &status.store_id)?;
        let (guard, _, _) = ack_parts(&attempt.request)?;
        if status.store_id != attempt.key.store_id
            || status.items > 128
            || status.pending > status.items
            || status.waiting_for_clock > status.items - status.pending
            || status.max_items != 128
            || status.zone.is_empty()
            || status.zone.len() > 128
        {
            return Err(error("invalid_evidence"));
        }
        let _ = status.wall_trusted;
        let receipt = status
            .last_receipt
            .ok_or_else(|| error("no_matching_receipt"))?;
        if receipt.request_id != guard.request_id || receipt.op != "ack" {
            return Err(error("no_matching_receipt"));
        }
        let mut evidence = Self::from_reply(attempt, &receipt.response)?;
        if status.revision < evidence.committed_revision {
            return Err(error("invalid_evidence"));
        }
        evidence.current_revision = Some(status.revision);
        Ok(evidence)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evidence {
    Item(ItemEvidence),
    Absent(AbsentEvidence),
    Ack(AckEvidence),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoreFence {
    store_id: String,
    revision: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RetirementRecord {
    attempt: u64,
    #[serde(deserialize_with = "saved::Retirement::deserialize")]
    proof: QualifiedRetirement,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    #[serde(deserialize_with = "saved::View::deserialize")]
    view: DeliveryView,
    first_attempt: u64,
    prepared_revision: u64,
    pending_revision: u64,
    #[serde(deserialize_with = "required_option")]
    previous: Option<RetirementRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    kind: String,
    schema: u32,
    ledger_id: String,
    revision: u64,
    next_attempt: u64,
    #[serde(deserialize_with = "required_option")]
    schedule: Option<StoreFence>,
    entries: Vec<Entry>,
}
impl State {
    fn check(&self) -> Result<(), Error> {
        let invalid = || error("invalid_snapshot");
        if self.kind != KIND
            || self.schema != SCHEMA
            || !identity(&self.ledger_id)
            || self.next_attempt == 0
            || self.next_attempt - 1 > self.revision
            || self.entries.len() > MAX_ENTRIES
        {
            return Err(invalid());
        }
        let Some(fence) = &self.schedule else {
            return if self.entries.is_empty() && self.next_attempt == 1 && self.revision == 0 {
                Ok(())
            } else {
                Err(invalid())
            };
        };
        if !identity(&fence.store_id) || self.next_attempt == 1 || self.revision == 0 {
            return Err(invalid());
        }
        let mut last_key = None;
        let mut counters = BTreeSet::new();
        let mut correlations = BTreeSet::new();
        for entry in &self.entries {
            let view = &entry.view;
            let prepared = &view.prepared;
            let key = &prepared.key;
            if key.validate().is_err()
                || key.store_id != fence.store_id
                || last_key.is_some_and(|last| last >= key)
                || !prepared.correlation.check()
                || !correlations.insert(&prepared.correlation)
                || entry.first_attempt == 0
                || prepared.attempt < entry.first_attempt
                || prepared.attempt >= self.next_attempt
                || !counters.insert(entry.first_attempt)
                || (prepared.attempt != entry.first_attempt && !counters.insert(prepared.attempt))
                || entry.prepared_revision == 0
                || entry.pending_revision < entry.prepared_revision
                || entry.pending_revision > fence.revision
            {
                return Err(invalid());
            }
            last_key = Some(key);
            match &entry.previous {
                Some(previous)
                    if previous.attempt >= entry.first_attempt
                        && previous.attempt < prepared.attempt
                        && previous.proof.check()
                        && correlations.insert(&previous.proof.correlation)
                        && (previous.attempt == entry.first_attempt
                            || counters.insert(previous.attempt)) => {}
                None if prepared.attempt == entry.first_attempt => {}
                _ => return Err(invalid()),
            }
            if let Some(proof) = &view.completion
                && (!proof.check() || proof.correlation != prepared.correlation)
            {
                return Err(invalid());
            }
            if let Some(ack) = &view.ack {
                let (guard, id, delivery) = ack_parts(&ack.request).map_err(|_| invalid())?;
                if view.completion.is_none()
                    || ack.key != *key
                    || ack.attempt != prepared.attempt
                    || id != key.id
                    || delivery != key.delivery
                    || guard.store_id != key.store_id
                    || guard.request_id != ack_id(&self.ledger_id, entry.first_attempt)
                    || guard.revision < entry.prepared_revision
                    || guard.revision > entry.pending_revision
                    || guard.revision.checked_add(1).is_none()
                {
                    return Err(invalid());
                }
            }
            if let Some(resolution) = &view.resolution {
                if resolution.revision() <= entry.pending_revision
                    || resolution.revision() > fence.revision
                {
                    return Err(invalid());
                }
                match resolution {
                    Resolution::AckConfirmed { revision } => {
                        let Some(ack) = &view.ack else {
                            return Err(invalid());
                        };
                        if ack.request.guard().and_then(|g| g.revision.checked_add(1))
                            != Some(*revision)
                        {
                            return Err(invalid());
                        }
                    }
                    Resolution::Superseded { delivery, .. } if *delivery <= key.delivery => {
                        return Err(invalid());
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
    fn index(&self, key: &DeliveryKey) -> Result<usize, Error> {
        self.entries
            .binary_search_by(|entry| entry.view.prepared.key.cmp(key))
            .map_err(|_| error("unknown_delivery"))
    }
    fn observe(&mut self, store_id: &str, revision: u64) -> Result<(), Error> {
        match &mut self.schedule {
            Some(fence) if fence.store_id != store_id => Err(error("store_changed")),
            Some(fence) if revision < fence.revision => Err(error("stale_revision")),
            Some(fence) => {
                fence.revision = revision;
                Ok(())
            }
            None => {
                self.schedule = Some(StoreFence {
                    store_id: store_id.into(),
                    revision,
                });
                Ok(())
            }
        }
    }
}

pub struct DeliveryLedger<P: Persistence> {
    persistence: P,
    state: State,
    failed: bool,
    set_aside: Option<String>,
}
impl<P: Persistence> DeliveryLedger<P> {
    pub fn open(mut persistence: P, new_ledger_id: &str) -> Result<Self, Error> {
        // Inspect the envelope before reading enum variants from a newer build.
        // Decode supported state from the original bytes, retaining duplicate
        // known-field errors rather than normalizing JSON through Value.
        #[derive(Deserialize)]
        struct Header {
            kind: String,
            schema: u64,
        }
        let mut newer = None;
        let existing = match persistence.load().map_err(|_| error("storage"))? {
            Some(bytes) => {
                if bytes.is_empty() || bytes.len() > MAX_STATE_BYTES {
                    return Err(error("invalid_snapshot"));
                }
                let header: Header =
                    serde_json::from_slice(&bytes).map_err(|_| error("invalid_snapshot"))?;
                if header.kind != KIND {
                    return Err(error("invalid_snapshot"));
                }
                if header.schema > u64::from(SCHEMA) {
                    newer = Some(header.schema);
                    None
                } else {
                    let state: State =
                        serde_json::from_slice(&bytes).map_err(|_| error("invalid_snapshot"))?;
                    state.check()?;
                    Some(state)
                }
            }
            None => None,
        };
        let mut set_aside = None;
        let state = match existing {
            Some(state) => state,
            None => {
                let state = State {
                    kind: KIND.into(),
                    schema: SCHEMA,
                    ledger_id: new_ledger_id.into(),
                    revision: 0,
                    next_attempt: 1,
                    schedule: None,
                    entries: Vec::new(),
                };
                state.check()?;
                let bytes = bounded_json(&state, MAX_STATE_BYTES)?;
                if let Some(schema) = newer {
                    let since = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |since| since.as_secs());
                    let kept = persistence
                        .set_aside(&format!("newer-{schema}-{since}"))
                        .map_err(|_| error("storage"))?;
                    set_aside = Some(kept);
                }
                persistence.commit(&bytes).map_err(|_| error("storage"))?;
                if let (Some(schema), Some(kept)) = (newer, &set_aside) {
                    eprintln!(
                        "schedule delivery: ledger was written by a newer build (schema {schema}, this build reads {SCHEMA}); it is set aside as {kept}, and this build starts with an empty ledger"
                    );
                }
                state
            }
        };
        Ok(Self {
            persistence,
            state,
            failed: false,
            set_aside,
        })
    }
    /// The preserved newer-schema database, if this open began a fresh ledger.
    pub fn set_aside(&self) -> Option<&str> {
        self.set_aside.as_deref()
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn revision(&self) -> u64 {
        self.state.revision
    }
    pub fn schedule_revision(&self) -> Option<u64> {
        self.state.schedule.as_ref().map(|fence| fence.revision)
    }
    pub fn view(&self, key: &DeliveryKey) -> Result<Option<DeliveryView>, Error> {
        self.healthy()?;
        key.validate()?;
        Ok(self
            .state
            .index(key)
            .ok()
            .map(|i| self.state.entries[i].view.clone()))
    }
    /// Views are recovery information, not new permission to submit audio.
    pub fn entries(&self) -> Result<Vec<DeliveryView>, Error> {
        self.healthy()?;
        Ok(self
            .state
            .entries
            .iter()
            .map(|entry| entry.view.clone())
            .collect())
    }
    pub fn prepare(
        &mut self,
        pending: &PendingEvidence,
        correlation: Correlation,
    ) -> Result<PreparedToken, Error> {
        self.healthy()?;
        self.check_correlation(&correlation)?;
        let key = pending.key();
        if self.state.index(key).is_ok() {
            return Err(error("already_known"));
        }
        if self.state.entries.len() >= MAX_ENTRIES {
            return Err(error("full"));
        }
        let mut next = self.state.clone();
        next.observe(&key.store_id, pending.revision())?;
        let prepared = allocate(&mut next, key, correlation)?;
        next.entries.push(Entry {
            view: DeliveryView {
                prepared: prepared.clone(),
                completion: None,
                ack: None,
                resolution: None,
            },
            first_attempt: prepared.attempt,
            prepared_revision: pending.revision(),
            pending_revision: pending.revision(),
            previous: None,
        });
        next.entries
            .sort_by(|a, b| a.view.prepared.key.cmp(&b.view.prepared.key));
        self.save(next)?;
        Ok(prepared)
    }
    pub fn retry_interrupted(
        &mut self,
        prepared: &PreparedToken,
        pending: &PendingEvidence,
        retirement: &QualifiedRetirement,
        correlation: Correlation,
    ) -> Result<PreparedToken, Error> {
        self.healthy()?;
        self.check_correlation(&correlation)?;
        let index = self.prepared_index(prepared)?;
        let entry = &self.state.entries[index];
        if entry.view.completion.is_some() || entry.view.resolution.is_some() {
            return Err(error("not_prepared"));
        }
        if !retirement.check() || retirement.correlation != prepared.correlation {
            return Err(error("invalid_retirement"));
        }
        check_pending(&prepared.key, pending)?;
        let mut next = self.state.clone();
        next.observe(&prepared.key.store_id, pending.revision())?;
        let replacement = allocate(&mut next, &prepared.key, correlation)?;
        let entry = &mut next.entries[index];
        entry.view.prepared = replacement.clone();
        entry.prepared_revision = pending.revision();
        entry.pending_revision = pending.revision();
        entry.previous = Some(RetirementRecord {
            attempt: prepared.attempt,
            proof: retirement.clone(),
        });
        self.save(next)?;
        Ok(replacement)
    }
    pub fn complete(
        &mut self,
        prepared: &PreparedToken,
        proof: &QualifiedCompletion,
    ) -> Result<CompletedToken, Error> {
        self.healthy()?;
        let index = self.prepared_index(prepared)?;
        if !proof.check() || proof.correlation != prepared.correlation {
            return Err(error("invalid_completion"));
        }
        let view = &self.state.entries[index].view;
        if view.resolution.is_some() {
            return Err(error("resolved"));
        }
        let completed = CompletedToken {
            key: prepared.key.clone(),
            attempt: prepared.attempt,
        };
        if let Some(previous) = &view.completion {
            return if previous == proof {
                Ok(completed)
            } else {
                Err(error("completion_conflict"))
            };
        }
        let mut next = self.state.clone();
        next.entries[index].view.completion = Some(proof.clone());
        self.save(next)?;
        Ok(completed)
    }
    pub fn plan_ack(
        &mut self,
        completed: &CompletedToken,
        pending: &PendingEvidence,
    ) -> Result<AckAttempt, Error> {
        self.healthy()?;
        let index = self.state.index(&completed.key)?;
        let view = &self.state.entries[index].view;
        if completed.attempt != view.prepared.attempt || view.completion.is_none() {
            return Err(error("not_completed"));
        }
        if view.resolution.is_some() {
            return Err(error("resolved"));
        }
        check_pending(&completed.key, pending)?;
        let mut next = self.state.clone();
        next.observe(&completed.key.store_id, pending.revision())?;
        if let Some(ack) = &view.ack {
            if ack
                .request
                .guard()
                .is_some_and(|guard| guard.revision == pending.revision())
            {
                return Ok(ack.clone());
            }
            return Err(error("rebase_required"));
        }
        let ack = make_ack(&next, index, pending.revision())?;
        next.entries[index].pending_revision = pending.revision();
        next.entries[index].view.ack = Some(ack.clone());
        self.save(next)?;
        Ok(ack)
    }
    pub fn pending_ack(&self, key: &DeliveryKey) -> Result<Option<AckAttempt>, Error> {
        self.healthy()?;
        let view = &self.state.entries[self.state.index(key)?].view;
        Ok(if view.resolution.is_none() {
            view.ack.clone()
        } else {
            None
        })
    }
    pub fn rebase_ack(
        &mut self,
        key: &DeliveryKey,
        pending: &PendingEvidence,
    ) -> Result<AckAttempt, Error> {
        self.healthy()?;
        let index = self.state.index(key)?;
        let view = &self.state.entries[index].view;
        let ack = view.ack.as_ref().ok_or_else(|| error("no_ack_intent"))?;
        if view.resolution.is_some() {
            return Err(error("resolved"));
        }
        check_pending(key, pending)?;
        if pending.revision() <= ack.request.guard().expect("validated Ack").revision {
            return Err(error("not_newer"));
        }
        let mut next = self.state.clone();
        next.observe(&key.store_id, pending.revision())?;
        let replacement = make_ack(&next, index, pending.revision())?;
        next.entries[index].pending_revision = pending.revision();
        next.entries[index].view.ack = Some(replacement.clone());
        self.save(next)?;
        Ok(replacement)
    }
    pub fn reconcile(
        &mut self,
        key: &DeliveryKey,
        evidence: &Evidence,
    ) -> Result<Reconciled, Error> {
        self.healthy()?;
        let index = self.state.index(key)?;
        let entry = &self.state.entries[index];
        let mut next = self.state.clone();
        let resolution = match evidence {
            Evidence::Item(item) => {
                if item.key.store_id != key.store_id || item.key.id != key.id {
                    return Err(error("wrong_item"));
                }
                next.observe(&key.store_id, item.revision)?;
                if item.key.delivery < key.delivery {
                    return Err(error("stale_delivery"));
                }
                if item.key.delivery == key.delivery {
                    if !item.pending || entry.view.resolution.is_some() {
                        return Err(error("inconsistent_item"));
                    }
                    next.entries[index].pending_revision = item.revision;
                    if item.revision > entry.pending_revision {
                        self.save(next)?;
                    }
                    return Ok(Reconciled::Unresolved);
                }
                Resolution::Superseded {
                    revision: item.revision,
                    delivery: item.key.delivery,
                }
            }
            Evidence::Absent(absent) => {
                if absent.key != *key {
                    return Err(error("wrong_item"));
                }
                next.observe(&key.store_id, absent.revision)?;
                Resolution::Gone {
                    revision: absent.revision,
                }
            }
            Evidence::Ack(ack) => {
                if entry.view.ack.as_ref() != Some(&ack.attempt) {
                    return Err(error("wrong_ack_attempt"));
                }
                if let Some(revision) = ack.current_revision {
                    next.observe(&key.store_id, revision)?;
                } else {
                    let fence = next.schedule.as_mut().expect("entry has a store fence");
                    fence.revision = fence.revision.max(ack.committed_revision);
                }
                Resolution::AckConfirmed {
                    revision: ack.committed_revision,
                }
            }
        };
        if resolution.revision() <= entry.pending_revision {
            return Err(error("inconsistent_revision"));
        }
        if let Some(previous) = &entry.view.resolution {
            if previous == &resolution {
                let previous = previous.clone();
                if next.schedule.as_ref().map(|fence| fence.revision) != self.schedule_revision() {
                    self.save(next)?;
                }
                return Ok(Reconciled::Resolved(previous));
            }
            // A later positive matching receipt can clarify uncertainty, but a
            // different absent/advanced view must never downgrade an Ack.
            if !matches!(resolution, Resolution::AckConfirmed { .. })
                || matches!(previous, Resolution::AckConfirmed { .. })
            {
                return Err(error("already_resolved"));
            }
        }
        next.entries[index].view.resolution = Some(resolution.clone());
        self.save(next)?;
        Ok(Reconciled::Resolved(resolution))
    }
    /// Never removes unresolved records. Keep the revision fence and monotonic
    /// counters so a delayed pre-resolution offer cannot recreate an admission.
    pub fn compact_resolved(&mut self) -> Result<usize, Error> {
        self.healthy()?;
        let mut next = self.state.clone();
        next.entries.retain(|entry| entry.view.resolution.is_none());
        let removed = self.state.entries.len() - next.entries.len();
        if removed > 0 {
            self.save(next)?;
        }
        Ok(removed)
    }
    fn healthy(&self) -> Result<(), Error> {
        if self.failed {
            Err(error("storage_failed"))
        } else {
            Ok(())
        }
    }
    fn prepared_index(&self, prepared: &PreparedToken) -> Result<usize, Error> {
        let index = self.state.index(&prepared.key)?;
        if self.state.entries[index].view.prepared != *prepared {
            return Err(error("stale_attempt"));
        }
        Ok(index)
    }
    fn check_correlation(&self, correlation: &Correlation) -> Result<(), Error> {
        if !correlation.check() {
            return Err(error("invalid_correlation"));
        }
        if self.state.entries.iter().any(|entry| {
            entry.view.prepared.correlation == *correlation
                || entry
                    .previous
                    .as_ref()
                    .is_some_and(|old| old.proof.correlation == *correlation)
        }) {
            return Err(error("reused_correlation"));
        }
        Ok(())
    }
    fn save(&mut self, mut next: State) -> Result<(), Error> {
        next.revision = self
            .state
            .revision
            .checked_add(1)
            .ok_or_else(|| error("exhausted"))?;
        next.check()?;
        let bytes = bounded_json(&next, MAX_STATE_BYTES)?;
        if self.persistence.commit(&bytes).is_err() {
            self.failed = true;
            return Err(error("storage"));
        }
        self.state = next;
        Ok(())
    }
}

fn allocate(
    state: &mut State,
    key: &DeliveryKey,
    correlation: Correlation,
) -> Result<PreparedToken, Error> {
    let attempt = state.next_attempt;
    state.next_attempt = attempt.checked_add(1).ok_or_else(|| error("exhausted"))?;
    Ok(PreparedToken {
        key: key.clone(),
        attempt,
        correlation,
    })
}
fn check_pending(key: &DeliveryKey, pending: &PendingEvidence) -> Result<(), Error> {
    if pending.key() != key {
        Err(error("wrong_delivery"))
    } else {
        Ok(())
    }
}
fn make_ack(state: &State, index: usize, revision: u64) -> Result<AckAttempt, Error> {
    revision.checked_add(1).ok_or_else(|| error("exhausted"))?;
    let entry = &state.entries[index];
    let prepared = &entry.view.prepared;
    Ok(AckAttempt {
        key: prepared.key.clone(),
        attempt: prepared.attempt,
        request: Request::Ack {
            guard: Guard {
                store_id: prepared.key.store_id.clone(),
                revision,
                request_id: ack_id(&state.ledger_id, entry.first_attempt),
            },
            id: prepared.key.id,
            delivery: prepared.key.delivery,
        },
    })
}
fn ack_id(ledger_id: &str, attempt: u64) -> String {
    format!("due.{ledger_id}.{attempt}")
}
fn ack_parts(request: &Request) -> Result<(&Guard, u64, u64), Error> {
    match request {
        Request::Ack {
            guard,
            id,
            delivery,
        } => Ok((guard, *id, *delivery)),
        _ => Err(error("wrong_query")),
    }
}
fn identity(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn session(value: &str) -> bool {
    (1..=MAX_SESSION_BYTES).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-.:".contains(&c))
}
fn request_id(value: &str) -> bool {
    value.len() <= 64 && session(value)
}
fn check_identity(ok: bool, store_id: &str) -> Result<(), Error> {
    if ok && identity(store_id) {
        Ok(())
    } else {
        Err(error("invalid_evidence"))
    }
}
fn parse_reply<T: DeserializeOwned>(reply: &Value) -> Result<T, Error> {
    let bytes = bounded_json(reply, MAX_REPLY_BYTES)?;
    serde_json::from_slice(&bytes).map_err(|_| error("invalid_evidence"))
}
fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
}
fn bounded_json(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, Error> {
    struct Bounded {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit - self.bytes.len() {
                return Err(io::Error::other("bounded JSON capacity exceeded"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Bounded {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| error("too_large"))?;
    Ok(writer.bytes)
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ItemState {
    Pending,
    Scheduled,
    WaitingForClock,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemReply {
    id: u64,
    delivery: u64,
    text: String,
    request_id: String,
    state: ItemState,
    due_unix_ms: i64,
    local: String,
    original_due_unix_ms: i64,
    #[serde(deserialize_with = "required_option")]
    repeat: Option<Repeat>,
    snoozed: bool,
    #[serde(deserialize_with = "required_option")]
    remaining_ms: Option<u64>,
    late: bool,
    #[serde(deserialize_with = "required_option")]
    late_by_ms: Option<u64>,
}
impl ItemReply {
    fn check(&self) -> Result<(), Error> {
        if self.id == 0
            || self.delivery == 0
            || self.text.trim().is_empty()
            || self.text.len() > 300
            || self.text.chars().any(char::is_control)
            || !request_id(&self.request_id)
            || !(MIN_UNIX_MS..=MAX_UNIX_MS).contains(&self.due_unix_ms)
            || !(MIN_UNIX_MS..=MAX_UNIX_MS).contains(&self.original_due_unix_ms)
            || self.local.is_empty()
            || self.local.len() > 256
            || self.local.chars().any(char::is_control)
            || (self.state != ItemState::Pending && (self.late || self.late_by_ms.is_some()))
            || (self.state == ItemState::WaitingForClock && self.remaining_ms.is_some())
        {
            return Err(error("invalid_evidence"));
        }
        // These fields are parsed strictly but have no delivery-ledger policy.
        let _ = (&self.repeat, self.snoozed);
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetReply {
    ok: bool,
    store_id: String,
    revision: u64,
    item: ItemReply,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListReply {
    ok: bool,
    store_id: String,
    revision: u64,
    items: Vec<ItemReply>,
    #[serde(deserialize_with = "required_option")]
    next_after_id: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AckReply {
    ok: bool,
    store_id: String,
    revision: u64,
    acknowledged: bool,
    id: u64,
    delivery: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusReceipt {
    request_id: String,
    op: String,
    response: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusReply {
    ok: bool,
    store_id: String,
    revision: u64,
    items: u64,
    pending: u64,
    waiting_for_clock: u64,
    wall_trusted: bool,
    zone: String,
    max_items: u64,
    #[serde(deserialize_with = "required_option")]
    last_receipt: Option<StatusReceipt>,
}
