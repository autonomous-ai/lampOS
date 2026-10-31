use lamp_memory_tiers::{RecallResult, TierPopulation, memory_tier};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Calls {
    recall: u64,
    snapshot: u64,
}

/// Observation-only accounting belongs to the adapter, never a probe store.
#[derive(Default)]
pub(super) struct Usage(BTreeMap<&'static str, Calls>);

impl Usage {
    pub(super) fn observe(&mut self, result: &RecallResult, snapshot: bool) {
        if result.timed_out {
            return;
        }
        let tiers: BTreeSet<_> = result
            .items
            .iter()
            .filter(|item| !item.text.is_empty())
            .filter_map(|item| memory_tier(&item.kind))
            .collect();
        for tier in tiers {
            let calls = self.0.entry(tier).or_default();
            if snapshot {
                calls.snapshot += 1;
            } else {
                calls.recall += 1;
            }
        }
    }

    pub(super) fn inspect(&self, population: BTreeMap<String, TierPopulation>) -> Value {
        let tiers: BTreeMap<_, _> = population
            .into_iter()
            .map(|(tier, population)| {
                let calls = self.0.get(tier.as_str());
                let metrics = json!({
                    "items":population.items,
                    "text_bytes":population.text_bytes,
                    "recall_calls":calls.map_or(0, |c| c.recall),
                    "snapshot_calls":calls.map_or(0, |c| c.snapshot)
                });
                (tier, metrics)
            })
            .collect();
        json!({"schema":"tier-usage-v1","tiers":tiers})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_memory_tiers::{Recalled, Strength};

    #[test]
    fn one_use_per_returned_tier_and_no_use_for_empty_or_late_results() {
        let mut usage = Usage::default();
        let mut result = RecallResult::default();
        for kind in ["chapter", "event", "fact"] {
            result.items.push(Recalled {
                kind: kind.into(),
                text: "Synthetic text".into(),
                source_ids: vec![1],
                strength: Strength::Said,
            });
        }
        usage.observe(&result, false);
        assert_eq!(usage.0["life_story"].recall, 1);
        assert_eq!(usage.0["facts"].recall, 1);
        result.timed_out = true;
        usage.observe(&result, false);
        assert_eq!(usage.0["facts"].recall, 1);
        usage.observe(&RecallResult::default(), true);
        assert_eq!(usage.0["facts"].snapshot, 0);
    }
}
