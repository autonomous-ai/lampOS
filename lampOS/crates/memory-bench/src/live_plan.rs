//! Public campaign policy. Approval is explicit input, never inferred here.
use crate::{Result, audio, campaign, files, hash, invalid, live_http, live_speech, model::Prices};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    pub revision: String,
    pub executable_sha256: String,
    pub arguments: Vec<String>,
    pub budget_microusd: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub profile: String,
    pub setup_order: Vec<String>,
    pub registration_revision: String,
    pub registration_sha256: String,
    pub audio_manifest_sha256: String,
    pub speech_budget_microusd: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: u32,
    pub runner_revision: String,
    pub runner_sha256: String,
    pub evaluation_sha256: String,
    pub chat: live_http::Config,
    pub chat_prices: Prices,
    pub speech: live_speech::Config,
    pub speech_prices: audio::Prices,
    pub repetitions: u32,
    pub fixtures: BTreeMap<String, Fixture>,
    pub adapters: BTreeMap<String, Adapter>,
    /// A one-run look at four setups, never a registered result. Omitted when
    /// false, so registered policy hashes are unchanged by this field.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preliminary: bool,
}
/// Setups of the preliminary run, in their registered relative order.
pub const PRELIMINARY_ORDER: [&str; 4] = ["none", "full_history", "lampos_today_partial", "tiers"];
/// Written on every preliminary manifest, result and report.
pub const PRELIMINARY_LABEL: &str =
    "preliminary: one run, development month, not the registered score";
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub fixture: String,
    pub arm: String,
    pub repetition: u32,
    pub fixture_directory: PathBuf,
    pub adapter_executable: PathBuf,
}
pub fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn revision(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
impl Policy {
    pub fn hash(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }
    pub fn validate(&self, fixture_transport: bool) -> Result<()> {
        self.chat.validate(fixture_transport)?;
        self.speech.validate(fixture_transport)?;
        self.speech_prices.validate()?;
        // The preliminary kind runs once, on one month fixture, and never feeds
        // the registered card; the registered kind keeps its three runs.
        let repetitions = if self.preliminary { 1 } else { 3 };
        if self.version != 1
            || self.repetitions != repetitions
            || (self.preliminary
                && (self.fixtures.len() != 1 || !self.fixtures.contains_key("development")))
            || !revision(&self.runner_revision)
            || !digest(&self.runner_sha256)
            || self.evaluation_sha256 != hash(&serde_json::to_vec(&crate::evaluation::policy())?)
            || self.fixtures.is_empty()
            || self.fixtures.len() > 3
            || self.chat_prices.source.is_empty()
            || self.chat_prices.date.is_empty()
            || self.chat_prices.input_microusd_per_million == 0
            || self.chat_prices.output_microusd_per_million == 0
        {
            return Err(invalid("invalid or stale public live policy"));
        }
        let month: Vec<_> = if self.preliminary {
            PRELIMINARY_ORDER.map(str::to_owned).to_vec()
        } else {
            ["none", "full_history", "lampos_today_partial"]
                .into_iter()
                .chain(files::ARMS)
                .map(str::to_owned)
                .collect()
        };
        let aging = vec!["tiers".to_owned(), "tiers_minus_fading".to_owned()];
        let mut admitted = BTreeSet::new();
        let mut months = 0;
        let mut aging_count = 0;
        for (name, f) in &self.fixtures {
            let expected = match f.profile.as_str() {
                "month" => {
                    months += 1;
                    &month
                }
                "aging" if !self.preliminary => {
                    aging_count += 1;
                    &aging
                }
                _ => return Err(invalid("unregistered fixture profile")),
            };
            if !crate::protocol::valid_id(name)
                || !revision(&f.registration_revision)
                || !digest(&f.registration_sha256)
                || !digest(&f.audio_manifest_sha256)
                || f.speech_budget_microusd == 0
                || &f.setup_order != expected
            {
                return Err(invalid(
                    "invalid fixture pin, setup order or speech allowance",
                ));
            }
            admitted.extend(f.setup_order.iter());
        }
        if months > 2
            || aging_count > 1
            || self.adapters.keys().collect::<BTreeSet<_>>() != admitted
        {
            return Err(invalid("adapter inventory differs from admitted fixtures"));
        }
        for adapter in self.adapters.values() {
            if !revision(&adapter.revision)
                || !digest(&adapter.executable_sha256)
                || adapter.budget_microusd == 0
                || adapter.arguments.len() > 16
                || adapter
                    .arguments
                    .iter()
                    .any(|a| a.len() > 4096 || a.contains('\0'))
            {
                return Err(invalid("invalid adapter pin or allowance"));
            }
        }
        let full = &self.adapters["tiers"];
        for name in files::ARMS {
            let Some(other) = self.adapters.get(name) else {
                continue;
            };
            if other.revision != full.revision
                || other.executable_sha256 != full.executable_sha256
                || other.arguments != full.arguments
            {
                return Err(invalid("all Tiers setups must use one agreed build"));
            }
        }
        self.planned_microusd()?;
        Ok(())
    }
    pub fn planned_microusd(&self) -> Result<u64> {
        self.fixtures.values().try_fold(0u64, |total, f| {
            let setup = f.setup_order.iter().try_fold(0u64, |n, key| {
                let arm = self
                    .adapters
                    .get(key)
                    .ok_or_else(|| invalid("fixture references an unpinned setup"))?;
                n.checked_add(arm.budget_microusd)
                    .ok_or_else(|| invalid("plan allowance overflow"))
            })?;
            setup
                .checked_mul(u64::from(self.repetitions))
                .and_then(|n| n.checked_add(f.speech_budget_microusd))
                .and_then(|n| n.checked_add(total))
                .ok_or_else(|| invalid("plan allowance overflow"))
        })
    }
    pub fn check_approval(&self, approved: &campaign::Plan, fixture_transport: bool) -> Result<()> {
        self.validate(fixture_transport)?;
        if approved.policy_sha256 != self.hash()?
            || self.planned_microusd()? > approved.approved_cap_microusd
        {
            return Err(invalid(
                "public plan differs from approval or exceeds its cap",
            ));
        }
        Ok(())
    }
    pub fn admitted(&self, run: &Run) -> Result<(&Fixture, &Adapter, String)> {
        let fixture = self
            .fixtures
            .get(&run.fixture)
            .ok_or_else(|| invalid("fixture is outside approved scope"))?;
        let adapter = self
            .adapters
            .get(&run.arm)
            .ok_or_else(|| invalid("setup is outside approved scope"))?;
        if !fixture.setup_order.contains(&run.arm)
            || !(1..=self.repetitions).contains(&run.repetition)
        {
            return Err(invalid("invalid repetition or cache pin"));
        }
        // Hashing gives bounded, stable accounting identity even with long names.
        let id = format!(
            "run-{}",
            hash(&serde_json::to_vec(&json!([
                run.fixture,
                run.arm,
                run.repetition
            ]))?)
        );
        Ok((fixture, adapter, id))
    }
}
