//! Bounded, atomic transfer of private computer labels from worker to serve.
//! Each receiver belongs to one worker boot; replacing that boot replaces it.
use crate::agent_tasks::{MachineNames, machine_names_after};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const CHUNK_BYTES: usize = 6000;
const MAX_CHUNKS: usize = 16;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamesChunk {
    pub revision: u64,
    pub index: u8,
    pub count: u8,
    pub enabled: bool,
    pub pairs: Vec<(String, String)>,
}

impl std::fmt::Debug for NamesChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NamesChunk")
            .field("revision", &self.revision)
            .field("index", &self.index)
            .field("count", &self.count)
            .field("enabled", &self.enabled)
            .field("pairs", &self.pairs.len())
            .finish()
    }
}

fn chunks(revision: u64, snapshot: MachineNames) -> Option<Vec<NamesChunk>> {
    if !snapshot.valid() {
        return None;
    }
    let mut result = Vec::new();
    let mut chunk = NamesChunk {
        revision,
        index: 0,
        count: MAX_CHUNKS as u8,
        enabled: snapshot.enabled,
        pairs: Vec::new(),
    };
    let mut size = serde_json::to_vec(&chunk).ok()?.len();
    for pair in snapshot.names {
        // Serialize each escaped pair once, never the growing chunk.
        let pair_bytes = serde_json::to_vec(&pair).ok()?.len();
        let added = pair_bytes + usize::from(!chunk.pairs.is_empty());
        if size + added > CHUNK_BYTES {
            if chunk.pairs.is_empty() {
                return None;
            }
            result.push(chunk);
            if result.len() >= MAX_CHUNKS {
                return None;
            }
            chunk = NamesChunk {
                revision,
                index: result.len() as u8,
                count: MAX_CHUNKS as u8,
                enabled: snapshot.enabled,
                pairs: Vec::new(),
            };
            size = serde_json::to_vec(&chunk).ok()?.len();
        }
        size += pair_bytes + usize::from(!chunk.pairs.is_empty());
        if size > CHUNK_BYTES {
            return None;
        }
        chunk.pairs.push(pair);
    }
    result.push(chunk);
    let count = result.len() as u8;
    for chunk in &mut result {
        chunk.count = count;
    }
    Some(result)
}

/// The caller sends `next()` outside cache locks, then calls `sent()` only
/// after that exact chunk was successfully sent. Failed sends retry it.
#[derive(Default)]
pub(crate) struct NamesPublisher {
    observed: Option<u64>,
    pending: Vec<NamesChunk>,
    at: usize,
    completed: Option<u64>,
}
impl NamesPublisher {
    pub(crate) fn next(&mut self) -> std::io::Result<Option<&NamesChunk>> {
        if let Some((revision, snapshot)) = machine_names_after(self.observed) {
            self.observed = Some(revision);
            // Coalesce a new complete source revision, never mix its pairs
            // into an older partial transfer.
            self.pending = chunks(revision, snapshot).ok_or_else(invalid)?;
            self.at = 0;
        }
        Ok(self.pending.get(self.at))
    }
    pub(crate) fn sent(&mut self) {
        if let Some(chunk) = self.pending.get(self.at) {
            self.at += 1;
            if self.at == self.pending.len() {
                self.completed = Some(chunk.revision);
                self.pending.clear();
                self.at = 0;
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn completed_revision(&self) -> Option<u64> {
        self.completed
    }
}

fn invalid() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "invalid machine-name snapshot",
    )
}

/// A complete snapshot whose pair, aggregate-size and transfer checks passed.
/// The private field cannot be populated by raw IPC deserialization. Default
/// is the valid disabled/empty state for a newly created worker mirror.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ValidatedNames(MachineNames);

impl ValidatedNames {
    pub(crate) fn snapshot(&self) -> &MachineNames {
        &self.0
    }
}

struct Assembly {
    revision: u64,
    count: u8,
    next: u8,
    enabled: bool,
    names: BTreeMap<String, String>,
    serialized_bytes: usize,
}

/// Invalid or incomplete transfers never replace the visible snapshot.
/// A rejected revision is consumed: a sender must publish a newer one.
#[derive(Default)]
pub(crate) struct NamesReceiver {
    highest: Option<u64>,
    pending: Option<Assembly>,
}
impl std::fmt::Debug for NamesReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NamesReceiver")
            .field("highest", &self.highest)
            .field(
                "pending",
                &self
                    .pending
                    .as_ref()
                    .map(|p| (p.next, p.count, p.names.len())),
            )
            .finish()
    }
}

impl NamesReceiver {
    pub(crate) fn receive(&mut self, chunk: NamesChunk) -> std::io::Result<Option<ValidatedNames>> {
        if self.highest.is_some_and(|highest| chunk.revision < highest) {
            return Ok(None);
        }
        if self.highest != Some(chunk.revision) {
            self.highest = Some(chunk.revision);
            self.pending = None;
            if chunk.index != 0 {
                return Err(invalid());
            }
            if chunk.index == 0 {
                self.pending = Some(Assembly {
                    revision: chunk.revision,
                    count: chunk.count,
                    next: 0,
                    enabled: chunk.enabled,
                    names: BTreeMap::new(),
                    serialized_bytes: serde_json::to_vec(&MachineNames {
                        enabled: chunk.enabled,
                        names: BTreeMap::new(),
                    })
                    .map_err(|_| invalid())?
                    .len(),
                });
            }
        }
        let valid_chunk = chunk.count > 0
            && usize::from(chunk.count) <= MAX_CHUNKS
            && chunk.index < chunk.count
            && serde_json::to_vec(&chunk).is_ok_and(|bytes| bytes.len() <= CHUNK_BYTES);
        let Some(mut pending) = self.pending.take() else {
            return Ok(None);
        };
        if !valid_chunk
            || pending.revision != chunk.revision
            || pending.next != chunk.index
            || pending.count != chunk.count
            || pending.enabled != chunk.enabled
        {
            return Err(invalid());
        }
        for (key, name) in chunk.pairs {
            if !pending.enabled || !MachineNames::valid_pair(&key, &name) {
                return Err(invalid());
            }
            // [key,value] has two more punctuation bytes than key:value.
            // Account for JSON escaping and the comma between map entries.
            let pair_bytes = serde_json::to_vec(&(&key, &name))
                .map_err(|_| invalid())?
                .len();
            pending.serialized_bytes += pair_bytes - 2 + usize::from(!pending.names.is_empty());
            if pending.serialized_bytes > crate::tools::MAX_TASKS_ANSWER_BYTES as usize
                || pending.names.insert(key, name).is_some()
            {
                return Err(invalid());
            }
        }
        pending.next += 1;
        if pending.next == pending.count {
            return Ok(Some(ValidatedNames(MachineNames {
                enabled: pending.enabled,
                names: pending.names,
            })));
        }
        self.pending = Some(pending);
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(n: usize) -> MachineNames {
        MachineNames {
            enabled: true,
            names: (0..n)
                .map(|i| {
                    (
                        format!("target-{i}"),
                        format!("Synthetic {}", "x".repeat(40)),
                    )
                })
                .collect(),
        }
    }
    #[test]
    fn complete_transfer_is_atomic_and_fits_a_json_output_envelope() {
        let expected = snapshot(800);
        let pieces = chunks(u64::MAX, expected.clone()).unwrap();
        assert!(pieces.len() > 1 && pieces.len() <= MAX_CHUNKS);
        let mut receiver = NamesReceiver::default();
        for (i, chunk) in pieces.iter().enumerate() {
            let envelope = crate::wire::Envelope {
                boot: lamp_turn_owner::BootId::new([77; 16]).unwrap(),
                sequence: u64::MAX,
                sent_at_us: u64::MAX,
                payload: serde_json::json!({"type":"machine_names", "chunk":chunk}),
            };
            let wire = crate::wire::encode(&envelope).unwrap();
            assert!(wire.len() <= lamp_ipc::MAX_DATAGRAM_BYTES);
            let decoded: serde_json::Value = serde_json::from_slice(&wire).unwrap();
            let chunk = serde_json::from_value(decoded["payload"]["chunk"].clone()).unwrap();
            let got = receiver.receive(chunk).unwrap();
            if let Some(pending) = &receiver.pending {
                assert_eq!(
                    pending.serialized_bytes,
                    serde_json::to_vec(&MachineNames {
                        enabled: pending.enabled,
                        names: pending.names.clone(),
                    })
                    .unwrap()
                    .len()
                );
            }
            assert_eq!(
                got.as_ref().map(ValidatedNames::snapshot),
                (i + 1 == pieces.len()).then_some(&expected)
            );
        }
        assert!(receiver.receive(pieces[0].clone()).unwrap().is_none());
    }
    #[test]
    fn missing_reordered_and_replaced_transfers_never_publish_partial_names() {
        let old = chunks(1, snapshot(200)).unwrap();
        assert!(old.len() > 1);
        let mut receiver = NamesReceiver::default();
        assert!(receiver.receive(old[0].clone()).unwrap().is_none());
        let new = chunks(2, snapshot(1)).unwrap();
        assert_eq!(
            receiver
                .receive(new[0].clone())
                .unwrap()
                .as_ref()
                .map(ValidatedNames::snapshot),
            Some(&snapshot(1))
        );
        assert!(receiver.receive(old[1].clone()).unwrap().is_none());
        let mut receiver = NamesReceiver::default();
        assert!(receiver.receive(old[1].clone()).is_err());
        assert!(receiver.receive(old[0].clone()).unwrap().is_none());
    }
    #[test]
    fn individually_small_chunks_cannot_exceed_the_total_snapshot_bound() {
        let mut receiver = NamesReceiver::default();
        for index in 0..16u8 {
            let chunk = NamesChunk {
                revision: 8,
                index,
                count: 16,
                enabled: true,
                pairs: (0..65)
                    .map(|n| (format!("t-{index}-{n}"), "x".repeat(64)))
                    .collect(),
            };
            assert!(serde_json::to_vec(&chunk).unwrap().len() < CHUNK_BYTES);
            assert!(
                !matches!(receiver.receive(chunk), Ok(Some(_))),
                "oversized aggregate was published"
            );
        }
        assert!(receiver.pending.is_none());
    }

    #[test]
    fn publisher_retries_coalesces_and_marks_only_successfully_sent_revisions() {
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        struct Restore(MachineNames);
        impl Drop for Restore {
            fn drop(&mut self) {
                assert!(crate::agent_tasks::apply_parent_snapshot(&self.0));
            }
        }
        let _restore = Restore(machine_names_after(None).unwrap().1);
        assert!(crate::agent_tasks::apply_parent_snapshot(&snapshot(200)));
        let mut publisher = NamesPublisher::default();
        let first = publisher.next().unwrap().unwrap().clone();
        assert!(first.count > 1);
        let retry = publisher.next().unwrap().unwrap();
        assert_eq!(retry.revision, first.revision);
        assert_eq!(retry.index, first.index);
        assert_eq!(retry.pairs, first.pairs);
        assert_eq!(publisher.completed_revision(), None);
        publisher.sent();
        assert_eq!(publisher.completed_revision(), None);
        // Source changes while credit is unavailable: the next publication
        // starts the new coherent snapshot instead of finishing stale chunks.
        assert!(crate::agent_tasks::apply_parent_snapshot(&snapshot(1)));
        let next = publisher.next().unwrap().unwrap().clone();
        assert!(next.revision > first.revision);
        assert_eq!((next.index, next.count), (0, 1));
        publisher.sent();
        assert_eq!(publisher.completed_revision(), Some(next.revision));
        assert!(publisher.next().unwrap().is_none());
        publisher.sent();
        assert_eq!(publisher.completed_revision(), Some(next.revision));
    }

    #[test]
    fn escaped_labels_and_many_small_entries_round_trip_without_truncation() {
        for names in [
            (0..400)
                .map(|i| (format!("t-{i}"), "\"\\".repeat(32)))
                .collect(),
            (0..2000).map(|i| (format!("t-{i}"), "x".into())).collect(),
        ] {
            let expected = MachineNames {
                enabled: true,
                names,
            };
            let pieces = chunks(1, expected.clone()).unwrap();
            let mut receiver = NamesReceiver::default();
            let mut got = None;
            for piece in pieces {
                got = receiver.receive(piece).unwrap();
            }
            assert_eq!(got.as_ref().map(ValidatedNames::snapshot), Some(&expected));
        }
    }

    #[test]
    fn only_completed_validated_names_install_into_parent_cache() {
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        struct Restore(MachineNames);
        impl Drop for Restore {
            fn drop(&mut self) {
                assert!(crate::agent_tasks::apply_parent_snapshot(&self.0));
            }
        }
        let _restore = Restore(machine_names_after(None).unwrap().1);
        assert!(crate::agent_tasks::install_parent_names(
            &ValidatedNames::default()
        ));
        let expected = snapshot(200);
        let pieces = chunks(1, expected.clone()).unwrap();
        let mut receiver = NamesReceiver::default();
        for (index, piece) in pieces.iter().cloned().enumerate() {
            match receiver.receive(piece).unwrap() {
                Some(complete) => {
                    assert_eq!(index + 1, pieces.len());
                    assert!(crate::agent_tasks::install_parent_names(&complete));
                }
                None => assert!(crate::agent_tasks::machine_name("target-0").is_none()),
            }
        }
        assert_eq!(machine_names_after(None).unwrap().1, expected);
        assert!(crate::agent_tasks::install_parent_names(
            &ValidatedNames::default()
        ));
        assert!(crate::agent_tasks::machine_name("target-0").is_none());
    }

    #[test]
    fn empty_off_and_malformed_chunks() {
        let mut receiver = NamesReceiver::default();
        for (revision, expected) in [(1, snapshot(0)), (2, MachineNames::default())] {
            assert_eq!(
                receiver
                    .receive(chunks(revision, expected.clone()).unwrap().remove(0))
                    .unwrap()
                    .as_ref()
                    .map(ValidatedNames::snapshot),
                Some(&expected)
            );
        }
        for kind in 0..5 {
            let mut receiver = NamesReceiver::default();
            let mut bad = chunks(4, snapshot(1)).unwrap().remove(0);
            match kind {
                0 => bad.count = 17,
                1 => bad.pairs[0].1 = "x".repeat(7000),
                2 => bad.pairs[0].0 = "bad alias".into(),
                3 => bad.enabled = false,
                _ => bad.pairs.push(bad.pairs[0].clone()),
            }
            assert!(receiver.receive(bad).is_err());
        }
        assert!(chunks(1, snapshot(2000)).is_none());
        let bad = serde_json::json!({"revision":1,"index":0,"count":1,"enabled":false,"pairs":[],"extra":true});
        assert!(serde_json::from_value::<NamesChunk>(bad).is_err());
        assert!(!format!("{:?}", chunks(1, snapshot(1)).unwrap()[0]).contains("Synthetic"));
    }
    /// Host-only diagnostic: fixture application and transport are excluded.
    #[test]
    #[ignore = "synthetic publisher timing; run explicitly with --nocapture"]
    fn second_names_publisher_timing() {
        use std::time::Instant;
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        struct Restore(MachineNames);
        impl Drop for Restore {
            fn drop(&mut self) {
                assert!(crate::agent_tasks::apply_parent_snapshot(&self.0));
            }
        }
        let _restore = Restore(machine_names_after(None).unwrap().1);
        fn fixture(n: usize) -> MachineNames {
            MachineNames {
                enabled: true,
                names: (0..n)
                    .map(|i| (format!("target-{i:05}"), "x".repeat(64)))
                    .collect(),
            }
        }
        let mut largest = 0;
        // This search and all fixture serialization are outside measured work.
        while fixture(largest + 1).valid() {
            largest += 1;
        }
        assert!(largest > 4);
        fn report(label: &str, boundary: &str, first: u128, samples: &mut [u128]) {
            samples.sort_unstable();
            eprintln!(
                "publisher {label} {boundary}: first={first}ns p50={}ns p95={}ns max={}ns n={}",
                samples[(samples.len() - 1) / 2],
                samples[(samples.len() - 1) * 95 / 100],
                samples[samples.len() - 1],
                samples.len()
            );
        }
        for (label, count) in [("normal4", 4), ("near_maximum", largest)] {
            let mut state = fixture(count);
            let bytes = serde_json::to_vec(&state).unwrap().len();
            assert!(state.valid());
            eprintln!(
                "publisher {label}: entries={count} snapshot_bytes={bytes} source_limit=65536"
            );
            let mut publisher = NamesPublisher::default();
            let (mut prepares, mut drains) = (Vec::with_capacity(200), Vec::with_capacity(200));
            let (mut first_prepare, mut first_drain) = (0, 0);
            for iteration in 0..=200 {
                // Same-size changes force a revision without changing bounds.
                *state.names.get_mut("target-00000").unwrap() =
                    if iteration % 2 == 0 { "a" } else { "b" }.repeat(64);
                assert!(crate::agent_tasks::apply_parent_snapshot(&state));
                let begin = Instant::now();
                let first = publisher.next().unwrap().expect("new revision");
                let (revision, chunks) = (first.revision, first.count);
                let prepare_ns = begin.elapsed().as_nanos();
                let begin = Instant::now();
                // Includes successful-send bookkeeping and subsequent source
                // revision checks; excludes encode, socket I/O and receiver.
                publisher.sent();
                let mut sent = 1;
                while publisher.next().unwrap().is_some() {
                    publisher.sent();
                    sent += 1;
                }
                let drain_ns = begin.elapsed().as_nanos();
                assert_eq!(sent, usize::from(chunks));
                assert_eq!(publisher.completed_revision(), Some(revision));
                if iteration == 0 {
                    first_prepare = prepare_ns;
                    first_drain = drain_ns;
                } else {
                    prepares.push(prepare_ns);
                    drains.push(drain_ns);
                }
            }
            report(label, "next_preparation", first_prepare, &mut prepares);
            report(label, "remaining_next_and_sent", first_drain, &mut drains);
            // Report rather than assert timing: host scheduling is uncontrolled.
            // Typical local work target is <1ms; the maximum-size case is separate.
        }
    }
}
