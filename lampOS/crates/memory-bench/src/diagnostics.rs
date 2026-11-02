//! Metadata-only coverage; never an answer-quality score or memory input.
use crate::{
    Result, invalid,
    protocol::{Diagnostics, TierUsage},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Coverage {
    pub probes: u64,
    pub missing_diagnostics: bool,
    pub tiers: BTreeMap<String, TierUsage>,
    #[serde(skip)]
    last: BTreeMap<String, TierUsage>,
}
fn decoded(value: Option<&Value>) -> Result<Option<Diagnostics>> {
    value
        .filter(|v| !v.is_null())
        .map(|v| {
            let d: Diagnostics = serde_json::from_value(v.clone())?;
            d.validate()?;
            Ok(d)
        })
        .transpose()
}
fn maximum(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    a.zip(b).map(|(a, b)| a.max(b))
}
fn usage(total: Option<u64>, before: Option<u64>, after: Option<u64>) -> Result<Option<u64>> {
    match (total, before, after) {
        (Some(total), Some(before), Some(after)) => Ok(Some(
            total
                .checked_add(
                    after
                        .checked_sub(before)
                        .ok_or_else(|| invalid("diagnostic counters decreased across a probe"))?,
                )
                .ok_or_else(|| invalid("diagnostic counter overflow"))?,
        )),
        _ => Ok(None),
    }
}
impl Coverage {
    pub fn observe(&mut self, before: Option<&Value>, after: Option<&Value>) -> Result<()> {
        let (before, after) = (decoded(before)?, decoded(after)?);
        let missing = before.is_none() || after.is_none();
        let before = before.map(|d| d.tiers).unwrap_or_default();
        let after = after.map(|d| d.tiers).unwrap_or_default();
        let names: BTreeSet<_> = self
            .tiers
            .keys()
            .chain(before.keys())
            .chain(after.keys())
            .cloned()
            .collect();
        for name in names {
            let zero = TierUsage {
                items: Some(0),
                text_bytes: Some(0),
                recall_calls: Some(0),
                snapshot_calls: Some(0),
            };
            let unknown = TierUsage::default();
            let previous =
                self.tiers
                    .get(&name)
                    .unwrap_or(if self.probes == 0 { &zero } else { &unknown });
            let b = before.get(&name).unwrap_or(&unknown);
            let a = after.get(&name).unwrap_or(&unknown);
            if let Some(last) = self.last.get(&name) {
                usage(Some(0), last.recall_calls, b.recall_calls)?;
                usage(Some(0), last.snapshot_calls, b.snapshot_calls)?;
            }
            let next = TierUsage {
                items: maximum(previous.items, b.items),
                text_bytes: maximum(previous.text_bytes, b.text_bytes),
                recall_calls: usage(previous.recall_calls, b.recall_calls, a.recall_calls)?,
                snapshot_calls: usage(previous.snapshot_calls, b.snapshot_calls, a.snapshot_calls)?,
            };
            self.tiers.insert(name, next);
        }
        self.last = after;
        self.probes += 1;
        self.missing_diagnostics |= missing;
        Ok(())
    }
    pub fn exercise(&self, tier: &str) -> &'static str {
        let Some(t) = self.tiers.get(tier) else {
            return "unknown";
        };
        if self.missing_diagnostics {
            return "unknown";
        }
        match (t.items, t.recall_calls, t.snapshot_calls) {
            (Some(0), _, _) | (_, Some(0), Some(0)) => "not exercised",
            (Some(n), Some(r), Some(s)) if n > 0 && (r > 0 || s > 0) => {
                "population and returned text observed; not quality evidence"
            }
            _ => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn sample(items: u64, recall: u64) -> Value {
        json!({"schema":"tier-usage-v1","tiers":{"custom_tier":{"items":items,"text_bytes":12,"recall_calls":recall,"snapshot_calls":0}}})
    }
    #[test]
    fn custom_counts_print_and_questions_cannot_create_population_credit() {
        let mut c = Coverage::default();
        c.observe(Some(&sample(0, 0)), Some(&sample(3, 1))).unwrap();
        assert_eq!(c.exercise("custom_tier"), "not exercised");
        assert_eq!(
            serde_json::to_value(&c).unwrap()["tiers"]["custom_tier"]["items"],
            0
        );
    }
    #[test]
    fn missing_is_unknown_and_probe_accounting_cannot_roll_back() {
        let mut c = Coverage::default();
        c.observe(Some(&sample(2, 0)), Some(&sample(2, 1))).unwrap();
        c.observe(Some(&sample(2, 1)), Some(&sample(2, 2))).unwrap();
        assert_eq!(c.tiers["custom_tier"].recall_calls, Some(2));
        assert!(c.observe(Some(&sample(2, 2)), Some(&sample(2, 0))).is_err());
        assert!(c.observe(Some(&sample(2, 0)), Some(&sample(2, 1))).is_err());
        c.observe(None, None).unwrap();
        assert_eq!(c.exercise("custom_tier"), "unknown");
        assert_eq!(c.exercise("absent"), "unknown");
    }
    #[test]
    fn diagnostics_reject_text_and_negative_counts() {
        let mut s = sample(2, 0);
        s["tiers"]["custom_tier"]["claim"] = json!("not metadata");
        assert!(decoded(Some(&s)).is_err());
        let mut s = sample(2, 0);
        s["tiers"]["custom_tier"]["items"] = json!(-1);
        assert!(decoded(Some(&s)).is_err());
    }
}
