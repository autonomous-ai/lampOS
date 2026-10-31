use crate::{Error, LineKind, Result, SleepProposal};
use serde::{Deserialize, Serialize};

/// Registered before scores. Proof and forget enforcement are never optional.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    #[default]
    Tiers,
    TiersMinusEpisodes,
    TiersMinusLifeStory,
    TiersMinusFacts,
    TiersMinusOwnerModel,
    TiersMinusHowToBe,
    TiersMinusAhead,
    TiersMinusFeelings,
    TiersMinusNightlySleep,
    TiersMinusImportance,
    TiersMinusFading,
}
impl Arm {
    pub const ALL: [Self; 11] = [
        Self::Tiers,
        Self::TiersMinusEpisodes,
        Self::TiersMinusLifeStory,
        Self::TiersMinusFacts,
        Self::TiersMinusOwnerModel,
        Self::TiersMinusHowToBe,
        Self::TiersMinusAhead,
        Self::TiersMinusFeelings,
        Self::TiersMinusNightlySleep,
        Self::TiersMinusImportance,
        Self::TiersMinusFading,
    ];
    pub fn name(self) -> String {
        serde_json::to_value(self)
            .expect("enum serialization")
            .as_str()
            .expect("string enum")
            .into()
    }
    pub fn parse(name: &str) -> Result<Self> {
        serde_json::from_value(serde_json::Value::String(name.into()))
            .map_err(|_| Error::Invalid("unknown registered arm"))
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Tiers => "ours (tiers)",
            Self::TiersMinusEpisodes => "episodes not recalled",
            Self::TiersMinusLifeStory => "chapters and events removed",
            Self::TiersMinusFacts => "facts not recalled; kept only as evidence",
            Self::TiersMinusOwnerModel => "owner model removed",
            Self::TiersMinusHowToBe => "taught rules and learned patterns removed",
            Self::TiersMinusAhead => "what is ahead removed",
            Self::TiersMinusFeelings => "feeling lines removed",
            Self::TiersMinusNightlySleep => "nightly sleep disabled; weekly reflection retained",
            Self::TiersMinusImportance => "importance does not affect ranking or fading",
            Self::TiersMinusFading => "episode detail does not fade",
        }
    }
    pub fn includes_line(self, kind: LineKind) -> bool {
        !matches!(
            (self, kind),
            (
                Self::TiersMinusLifeStory,
                LineKind::Chapter | LineKind::Event
            ) | (Self::TiersMinusFacts, LineKind::Person | LineKind::Place)
                | (Self::TiersMinusOwnerModel, LineKind::OwnerModel)
                | (
                    Self::TiersMinusHowToBe,
                    LineKind::TaughtRule | LineKind::LearnedPattern
                )
                | (Self::TiersMinusAhead, LineKind::Ahead)
                | (Self::TiersMinusFeelings, LineKind::Feeling)
        )
    }
    /// Ablations remove a component's outputs, not its provenance checks.
    /// The same bounded proposal is generated before filtering in every arm.
    pub fn filter_proposal(self, json: &str) -> Result<String> {
        let mut proposal = SleepProposal::parse(json)?;
        proposal.lines.retain(|line| self.includes_line(line.kind));
        if self == Self::TiersMinusEpisodes {
            proposal.episodes.clear();
        }
        if self == Self::TiersMinusImportance {
            for episode in &mut proposal.episodes {
                episode.importance = 0.0;
            }
        }
        crate::types::encode(&proposal)
    }
}

impl crate::Store {
    /// Apply the registered mechanism policy without changing source records.
    pub fn maintain(&mut self, now_ms: i64, arm: Arm) -> Result<usize> {
        self.expire(now_ms)?;
        if arm == Arm::TiersMinusFading {
            return Ok(0);
        }
        if arm == Arm::TiersMinusImportance {
            // Treat every episode equally; recency and recall protection stay.
            return Ok(self.conn.execute("UPDATE episodes SET detail='' WHERE detail<>'' AND ended_ms<? AND last_recalled_ms<?", rusqlite::params![now_ms.saturating_sub(90*crate::DAY_MS),now_ms.saturating_sub(90*crate::DAY_MS)])?);
        }
        self.fade(now_ms)
    }
}
