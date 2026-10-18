//! One serialized owner of desired values and retry receipts. A save error may
//! follow an atomic rename: poison this owner and reopen before trusting state.
use crate::{
    model::{Catalog, Entry, Mutation, Snapshot, json_bytes, valid_request_id},
    store::{MAX_STATE_BYTES, Storage},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fmt};

pub const MAX_RECEIPTS: usize = 16;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Commit {
    pub store_id: String,
    pub revision: u64,
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineError {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for EngineError {}

fn error(code: &'static str, message: impl Into<String>) -> EngineError {
    EngineError {
        code,
        message: message.into(),
    }
}

fn invalid_state() -> EngineError {
    error(
        "invalid_state",
        "saved settings or retry receipts are invalid; no defaults were substituted",
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Receipt {
    store_id: String,
    request_id: String,
    previous_revision: u64,
    revision: u64,
    digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct State {
    snapshot: Snapshot,
    receipts: Vec<Receipt>,
}

impl State {
    fn check(&self) -> Result<(), EngineError> {
        self.snapshot.check().map_err(|_| invalid_state())?;
        let retained = self.snapshot.revision.min(MAX_RECEIPTS as u64) as usize;
        if self.receipts.len() != retained {
            return Err(invalid_state());
        }
        let mut request_ids = BTreeSet::new();
        // Receipts retain a request fingerprint, not previous setting values.
        // Current values are validated by Snapshot, not reconstructed history.
        for (index, receipt) in self.receipts.iter().enumerate() {
            let expected = self.snapshot.revision - retained as u64 + index as u64 + 1;
            if receipt.revision != expected
                || receipt.previous_revision.checked_add(1) != Some(receipt.revision)
                || receipt.store_id != self.snapshot.store_id
                || !valid_request_id(&receipt.request_id)
                || !request_ids.insert(&receipt.request_id)
                || receipt.digest.len() != 64
                || !receipt
                    .digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(invalid_state());
            }
        }
        Ok(())
    }

    fn bytes(&self) -> Result<Vec<u8>, EngineError> {
        self.check()?;
        json_bytes(self, MAX_STATE_BYTES).map_err(|_| {
            error(
                "invalid",
                "settings and retry receipts exceed the persisted byte bound or cannot be encoded",
            )
        })
    }
}

pub struct Engine<S: Storage> {
    storage: S,
    state: State,
    poisoned: bool,
}

impl<S: Storage> Engine<S> {
    pub fn open(mut storage: S, catalog: Catalog) -> Result<Self, EngineError> {
        catalog
            .check()
            .map_err(|message| error("invalid", message))?;
        let saved = storage
            .load()
            .map_err(|_| error("storage", "settings storage cannot be read"))?;
        let state = match saved {
            Some(bytes) => {
                if bytes.len() > MAX_STATE_BYTES {
                    return Err(invalid_state());
                }
                let state: State = serde_json::from_slice(&bytes).map_err(|_| invalid_state())?;
                state.check()?;
                if state.snapshot.catalog != catalog {
                    return Err(error(
                        "catalog_mismatch",
                        "saved catalog differs from the supplied declarations",
                    ));
                }
                state
            }
            None => {
                let mut identity = [0_u8; 16];
                getrandom::fill(&mut identity).map_err(|_| {
                    error("unavailable", "a settings store identity cannot be created")
                })?;
                let store_id = identity.iter().map(|byte| format!("{byte:02x}")).collect();
                let values = catalog
                    .declarations
                    .iter()
                    .map(|item| Entry {
                        name: item.name.clone(),
                        value: item.default.clone(),
                    })
                    .collect();
                let state = State {
                    snapshot: Snapshot {
                        version: 1,
                        store_id,
                        revision: 0,
                        catalog,
                        values,
                    },
                    receipts: Vec::new(),
                };
                let bytes = state.bytes()?;
                storage.save(&bytes).map_err(|_| {
                    error(
                        "indeterminate",
                        "settings initialization could not be confirmed; reopen before retrying",
                    )
                })?;
                state
            }
        };
        Ok(Self {
            storage,
            state,
            poisoned: false,
        })
    }

    /// Last confirmed in-memory values. After poisoned() becomes true these
    /// may differ from storage; expose that uncertainty beside any snapshot.
    pub fn snapshot(&self) -> Snapshot {
        self.state.snapshot.clone()
    }

    pub fn poisoned(&self) -> bool {
        self.poisoned
    }

    /// Persist all changes together before acknowledging. A no-op still earns
    /// one revision and receipt, so a retry has an unambiguous durable result.
    pub fn mutate(&mut self, mutation: Mutation) -> Result<Commit, EngineError> {
        if self.poisoned {
            return Err(error(
                "restart_required",
                "settings storage is uncertain; reopen this owner",
            ));
        }
        let bytes = mutation
            .canonical_bytes(&self.state.snapshot.catalog)
            .map_err(|message| error("invalid", message))?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if let Some(receipt) = self
            .state
            .receipts
            .iter()
            .find(|receipt| receipt.request_id == mutation.guard.request_id)
        {
            if receipt.digest != digest {
                return Err(error(
                    "conflict",
                    "request identity was already used for a different mutation",
                ));
            }
            return Ok(Commit {
                store_id: self.state.snapshot.store_id.clone(),
                revision: receipt.revision,
                replayed: true,
            });
        }
        if mutation.guard.store_id != self.state.snapshot.store_id
            || mutation.guard.revision != self.state.snapshot.revision
        {
            return Err(error(
                "conflict",
                "settings store identity or revision changed",
            ));
        }
        let revision = self
            .state
            .snapshot
            .revision
            .checked_add(1)
            .ok_or_else(|| error("invalid", "settings revision limit reached"))?;
        let mut next = self.state.clone();
        for change in &mutation.changes {
            let entry = next
                .snapshot
                .values
                .iter_mut()
                .find(|entry| entry.name == change.name)
                .ok_or_else(invalid_state)?;
            entry.value = change.value.clone();
        }
        next.snapshot.revision = revision;
        next.receipts.push(Receipt {
            store_id: mutation.guard.store_id,
            request_id: mutation.guard.request_id,
            previous_revision: mutation.guard.revision,
            revision,
            digest,
        });
        if next.receipts.len() > MAX_RECEIPTS {
            next.receipts.remove(0);
        }
        let bytes = next.bytes()?;
        if self.storage.save(&bytes).is_err() {
            self.poisoned = true;
            return Err(error(
                "indeterminate",
                "settings write could not be confirmed; reopen before trusting saved values",
            ));
        }
        self.state = next;
        Ok(Commit {
            store_id: self.state.snapshot.store_id.clone(),
            revision,
            replayed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Declaration, Guard, MAX_CATALOG_BYTES, MAX_MUTATION_BYTES, Rule, Value};
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Copy, Default)]
    enum Failure {
        #[default]
        None,
        Before,
        After,
    }
    #[derive(Default)]
    struct Saved {
        bytes: Option<Vec<u8>>,
        writes: usize,
        failure: Failure,
    }
    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<Saved>>);
    impl Storage for Memory {
        fn load(&mut self) -> io::Result<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().bytes.clone())
        }
        fn save(&mut self, bytes: &[u8]) -> io::Result<()> {
            let mut saved = self.0.lock().unwrap();
            saved.writes += 1;
            if matches!(saved.failure, Failure::Before) {
                return Err(io::Error::other("injected failure before replacement"));
            }
            saved.bytes = Some(bytes.to_vec());
            if matches!(saved.failure, Failure::After) {
                return Err(io::Error::other("injected failure after replacement"));
            }
            Ok(())
        }
    }

    fn mutation(engine: &Engine<Memory>, id: &str, level: i64) -> Mutation {
        let snapshot = engine.snapshot();
        Mutation {
            guard: Guard {
                store_id: snapshot.store_id,
                revision: snapshot.revision,
                request_id: id.into(),
            },
            changes: vec![Entry {
                name: "light.level".into(),
                value: Value::Integer(level),
            }],
        }
    }

    /// #1505: settings as a later build saves them, with a field this build
    /// doesn't know. The same state without it reopens; with it, it is
    /// refused today (`invalid_state`). No fix yet, and no unit starts
    /// lamp-settings on a lamp: whoever makes it read one release forward
    /// flips this to a plain test (Tests).
    #[test]
    #[should_panic(expected = "invalid_state")]
    fn a_state_from_a_later_build_is_read() {
        let memory = Memory::default();
        let mut engine = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
        let change = mutation(&engine, "r1", 2);
        engine.mutate(change).unwrap();
        let snapshot = engine.snapshot();
        let reopened = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
        assert_eq!(reopened.snapshot(), snapshot);
        let mut later: serde_json::Value =
            serde_json::from_slice(memory.0.lock().unwrap().bytes.as_ref().unwrap()).unwrap();
        later["a_field_from_a_later_build"] = serde_json::json!(true);
        memory.0.lock().unwrap().bytes = Some(serde_json::to_vec(&later).unwrap());
        let read = Engine::open(memory, Catalog::first_three()).unwrap();
        assert_eq!(read.snapshot(), snapshot);
    }

    #[test]
    fn missing_state_gets_defaults_and_reopening_preserves_identity() {
        let memory = Memory::default();
        let engine = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
        let snapshot = engine.snapshot();
        snapshot.check().unwrap();
        assert_eq!(snapshot.revision, 0);
        assert_eq!(memory.0.lock().unwrap().writes, 1);
        let reopened = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
        assert_eq!(reopened.snapshot(), snapshot);
        assert_eq!(memory.0.lock().unwrap().writes, 1);
    }

    #[test]
    fn invalid_atomic_batch_and_read_only_values_leave_all_state_unchanged() {
        let memory = Memory::default();
        let mut catalog = Catalog::first_three();
        catalog.declarations[2].read_only = true;
        let mut engine = Engine::open(memory.clone(), catalog).unwrap();
        let before = engine.snapshot();
        for bad in [
            Entry {
                name: "ears.rise_db".into(),
                value: Value::Number(61.0),
            },
            Entry {
                name: "records.voice_max_age_seconds".into(),
                value: Value::Integer(10),
            },
            Entry {
                name: "unknown.key".into(),
                value: Value::Bool(true),
            },
            Entry {
                name: "light.level".into(),
                value: Value::Integer(4),
            },
        ] {
            let mut request = mutation(&engine, "invalid-batch", 2);
            request.changes.push(bad);
            assert_eq!(engine.mutate(request).unwrap_err().code, "invalid");
            assert_eq!(engine.snapshot(), before);
        }
        assert_eq!(memory.0.lock().unwrap().writes, 1);
        assert!(!engine.poisoned());
    }

    #[test]
    fn same_revision_rivals_have_one_winner_and_retry_is_durable() {
        let memory = Memory::default();
        let mut engine = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
        let first = mutation(&engine, "first", 2);
        let rival = mutation(&engine, "rival", 3);
        assert_eq!(engine.mutate(first.clone()).unwrap().revision, 1);
        assert_eq!(engine.mutate(rival).unwrap_err().code, "conflict");
        let second = mutation(&engine, "second", 4);
        engine.mutate(second).unwrap();
        let mut reopened = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
        let replay = reopened.mutate(first.clone()).unwrap();
        assert_eq!((replay.revision, replay.replayed), (1, true));
        assert_eq!(reopened.snapshot().revision, 2);
        assert_eq!(memory.0.lock().unwrap().writes, 3);
        let mut reused = first;
        reused.changes[0].value = Value::Integer(5);
        assert_eq!(reopened.mutate(reused).unwrap_err().code, "conflict");
    }

    #[test]
    fn reordered_atomic_changes_have_the_same_request_fingerprint() {
        let mut engine = Engine::open(Memory::default(), Catalog::first_three()).unwrap();
        let mut request = mutation(&engine, "same-batch", 2);
        request.changes.push(Entry {
            name: "ears.rise_db".into(),
            value: Value::Number(45.0),
        });
        engine.mutate(request.clone()).unwrap();
        request.changes.reverse();
        assert!(engine.mutate(request).unwrap().replayed);
        assert_eq!(engine.snapshot().revision, 1);
    }

    #[test]
    fn no_op_has_a_receipt_and_evicted_retries_cannot_pass_the_stale_guard() {
        let memory = Memory::default();
        let mut engine = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
        let first = mutation(&engine, "first", 1);
        assert_eq!(engine.mutate(first.clone()).unwrap().revision, 1);
        for n in 2..=MAX_RECEIPTS + 1 {
            let request = mutation(&engine, &format!("request-{n}"), 1);
            engine.mutate(request).unwrap();
        }
        assert_eq!(engine.mutate(first).unwrap_err().code, "conflict");
        let state: State =
            serde_json::from_slice(memory.0.lock().unwrap().bytes.as_ref().unwrap()).unwrap();
        assert_eq!(state.receipts.len(), MAX_RECEIPTS);
        let reopened = Engine::open(memory, Catalog::first_three()).unwrap();
        assert_eq!(reopened.snapshot().revision, 17);
    }

    #[test]
    fn failed_save_before_or_after_replacement_poisoned_until_reopen() {
        for failure in [Failure::Before, Failure::After] {
            let memory = Memory::default();
            let mut engine = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
            let before = engine.snapshot();
            let request = mutation(&engine, "uncertain", 3);
            memory.0.lock().unwrap().failure = failure;
            assert_eq!(
                engine.mutate(request.clone()).unwrap_err().code,
                "indeterminate"
            );
            assert!(engine.poisoned());
            assert_eq!(engine.snapshot(), before);
            assert_eq!(
                engine.mutate(request.clone()).unwrap_err().code,
                "restart_required"
            );
            memory.0.lock().unwrap().failure = Failure::None;
            let mut reopened = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
            let replay = reopened.mutate(request).unwrap();
            assert_eq!(replay.replayed, matches!(failure, Failure::After));
            assert_eq!(replay.revision, 1);
            assert!(!reopened.poisoned());
        }
    }

    #[test]
    fn malformed_and_inconsistent_state_is_never_reset() {
        let memory = Memory::default();
        let mut engine = Engine::open(memory.clone(), Catalog::first_three()).unwrap();
        let request = mutation(&engine, "first", 2);
        engine.mutate(request).unwrap();
        let good = memory.0.lock().unwrap().bytes.clone().unwrap();
        let mut state: State = serde_json::from_slice(&good).unwrap();
        state.receipts[0].revision = 2;
        let mut wrong_value: State = serde_json::from_slice(&good).unwrap();
        wrong_value.snapshot.values[1].value = Value::Integer(6);
        let mut wrong_digest: State = serde_json::from_slice(&good).unwrap();
        wrong_digest.receipts[0].digest = "G".repeat(64);
        let mut wrong_store: State = serde_json::from_slice(&good).unwrap();
        wrong_store.receipts[0].store_id = "invalid".into();
        let mut wrong_previous: State = serde_json::from_slice(&good).unwrap();
        wrong_previous.receipts[0].previous_revision = 1;
        for bytes in [
            b"{".to_vec(),
            vec![b' '; MAX_STATE_BYTES + 1],
            serde_json::to_vec(&state).unwrap(),
            serde_json::to_vec(&wrong_value).unwrap(),
            serde_json::to_vec(&wrong_digest).unwrap(),
            serde_json::to_vec(&wrong_store).unwrap(),
            serde_json::to_vec(&wrong_previous).unwrap(),
        ] {
            memory.0.lock().unwrap().bytes = Some(bytes.clone());
            let writes = memory.0.lock().unwrap().writes;
            assert!(Engine::open(memory.clone(), Catalog::first_three()).is_err());
            let saved = memory.0.lock().unwrap();
            assert_eq!(saved.bytes.as_ref(), Some(&bytes));
            assert_eq!(saved.writes, writes);
        }
        memory.0.lock().unwrap().bytes = Some(good);
        let mut catalog = Catalog::first_three();
        catalog.declarations[0].default = Value::Number(31.0);
        assert!(
            matches!(Engine::open(memory, catalog), Err(error) if error.code == "catalog_mismatch")
        );
    }

    #[test]
    fn sixteen_maximum_wire_mutations_fit_with_a_large_admitted_catalog() {
        let catalog = Catalog {
            version: 1,
            declarations: (0..16)
                .map(|n| Declaration {
                    name: format!("{}.v{n:031}", "a".repeat(31)),
                    owner: "a".repeat(31),
                    description: "\"".repeat(512),
                    unit: "\"".repeat(32),
                    rule: Rule::Text {
                        max_bytes: 256,
                        choices: vec![],
                    },
                    default: Value::Text("x".repeat(256)),
                    read_only: false,
                })
                .collect(),
        };
        catalog.check().unwrap();
        assert!(serde_json::to_vec(&catalog).unwrap().len() > MAX_CATALOG_BYTES / 2);
        let memory = Memory::default();
        let mut engine = Engine::open(memory.clone(), catalog.clone()).unwrap();
        for n in 0..MAX_RECEIPTS {
            let before = engine.snapshot();
            let mut request = Mutation {
                guard: Guard {
                    store_id: before.store_id.clone(),
                    revision: before.revision,
                    request_id: format!("{n:064}"),
                },
                changes: catalog
                    .declarations
                    .iter()
                    .map(|entry| Entry {
                        name: entry.name.clone(),
                        value: Value::Text("x".repeat(256)),
                    })
                    .collect(),
            };
            // Escaped quotes enlarge JSON without exceeding any decoded text
            // bound. Fill the entire mutation allowance for every request.
            let mut remaining = MAX_MUTATION_BYTES - serde_json::to_vec(&request).unwrap().len();
            for entry in &mut request.changes {
                let quotes = remaining.min(256);
                entry.value = Value::Text(format!(
                    "{}{}",
                    "\"".repeat(quotes),
                    "x".repeat(256 - quotes)
                ));
                remaining -= quotes;
            }
            assert_eq!(remaining, 0);
            assert_eq!(
                request.canonical_bytes(&catalog).unwrap().len(),
                MAX_MUTATION_BYTES
            );
            assert_eq!(engine.mutate(request).unwrap().revision, n as u64 + 1);
            assert!(memory.0.lock().unwrap().bytes.as_ref().unwrap().len() <= MAX_STATE_BYTES);
        }
        let state: State =
            serde_json::from_slice(memory.0.lock().unwrap().bytes.as_ref().unwrap()).unwrap();
        assert_eq!(state.receipts.len(), MAX_RECEIPTS);
        assert!(serde_json::to_vec(&state.receipts).unwrap().len() < 8192);
        let receipt = serde_json::to_value(&state.receipts[0]).unwrap();
        assert!(receipt.get("mutation").is_none() && receipt.get("changes").is_none());
        let reopened = Engine::open(memory, catalog).unwrap();
        assert_eq!(reopened.snapshot(), engine.snapshot());
    }

    #[test]
    fn oversized_library_mutation_fails_before_storage_without_poisoning() {
        let catalog = Catalog {
            version: 1,
            declarations: (0..16)
                .map(|n| Declaration {
                    name: format!("example.value{n}"),
                    owner: "example".into(),
                    description: "Bounded text".into(),
                    unit: String::new(),
                    rule: Rule::Text {
                        max_bytes: 256,
                        choices: vec![],
                    },
                    default: Value::Text(String::new()),
                    read_only: false,
                })
                .collect(),
        };
        let memory = Memory::default();
        let mut engine = Engine::open(memory.clone(), catalog.clone()).unwrap();
        let before = engine.snapshot();
        let request = Mutation {
            guard: Guard {
                store_id: before.store_id.clone(),
                revision: before.revision,
                request_id: "too-large".into(),
            },
            changes: catalog
                .declarations
                .iter()
                .map(|entry| Entry {
                    name: entry.name.clone(),
                    value: Value::Text("\"".repeat(256)),
                })
                .collect(),
        };
        assert_eq!(engine.mutate(request).unwrap_err().code, "invalid");
        assert_eq!(engine.snapshot(), before);
        assert_eq!(memory.0.lock().unwrap().writes, 1);
        assert!(!engine.poisoned());
    }
}
