use crate::{Error, Result};
use serde::{Deserialize, Serialize};

pub const DAY_MS: i64 = 86_400_000;
pub const MAX_RECORD_BYTES: usize = 16 * 1024;
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 32 * 1024;
pub const MAX_RECORDS: usize = 128;
pub const MAX_PROPOSALS: usize = 32;
pub const MAX_OWNER_MODEL_BYTES: usize = 2048;
pub const MAX_TEXT_BYTES: usize = 600;
pub const MAX_CITATIONS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Speaker {
    Owner,
    Assistant,
    Unsure,
    Visitor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    Said,
    Maybe,
    Plan,
}
impl Strength {
    pub(crate) fn combine(self, other: Self) -> Self {
        if self == Self::Maybe || other == Self::Maybe {
            Self::Maybe
        } else if self == Self::Plan || other == Self::Plan {
            Self::Plan
        } else {
            Self::Said
        }
    }
    /// A conservative fallback, not a semantic classifier. The caller's source
    /// annotation is authoritative and can only become less certain here.
    pub(crate) fn constrained_by_words(self, text: &str) -> Self {
        let words: Vec<_> = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect();
        if words
            .iter()
            .any(|w| matches!(w.as_str(), "maybe" | "perhaps" | "might" | "possibly"))
        {
            self.combine(Self::Maybe)
        } else if words.iter().any(|w| {
            matches!(
                w.as_str(),
                "plan" | "planning" | "intend" | "intending" | "will"
            )
        }) {
            self.combine(Self::Plan)
        } else {
            self
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactKind {
    Identity,
    Preference,
    Habit,
    Statement,
    Plan,
    Appointment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    OwnerModel,
    Chapter,
    Event,
    Person,
    Place,
    TaughtRule,
    LearnedPattern,
    Ahead,
    Feeling,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: i64,
    pub episode_id: i64,
    pub text: String,
    pub speaker: Speaker,
    pub at_ms: i64,
    pub stt_confidence: Option<f64>,
    pub strength: Strength,
}

/// Input from an authorized ingestion boundary. A visitor, overheard line or
/// paused session is not retained. Unknown speaker lines are also dropped;
/// only an attributed owner line can become evidence.
#[derive(Clone, Debug)]
pub struct Incoming {
    pub text: String,
    pub speaker: Speaker,
    pub at_ms: i64,
    pub stt_confidence: Option<f64>,
    pub strength: Strength,
    pub addressed_to_lamp: bool,
    pub topic: String,
    pub remember: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quote {
    pub record_id: i64,
    pub text: String,
    pub speaker: Speaker,
    pub at_ms: i64,
    pub stt_confidence: Option<f64>,
    pub strength: Strength,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub id: i64,
    pub claim: String,
    pub strength: Strength,
    pub kind: FactKind,
    pub true_since_ms: i64,
    pub true_until_ms: Option<i64>,
    pub expires_ms: Option<i64>,
    pub supersedes: Option<i64>,
    pub active: bool,
    pub quotes: Vec<Quote>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryLine {
    pub id: i64,
    pub kind: LineKind,
    pub text: String,
    pub strength: Strength,
    pub fact_ids: Vec<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Episode {
    pub id: i64,
    pub topic: String,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub importance: f64,
    pub gist: String,
    pub detail: String,
    pub record_ids: Vec<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassKind {
    Nightly,
    Weekly,
}

/// Fields are read-only to callers: only Store prepares an applicable batch.
/// Keep the serialized input private; it contains the owner's source words.
#[derive(Clone, Serialize)]
pub struct SleepInput {
    #[serde(skip)]
    pub(crate) validity: tokio::sync::watch::Receiver<u64>,
    #[serde(skip)]
    pub(crate) epoch: u64,
    pub(crate) owner: String,
    pub(crate) generation: i64,
    pub(crate) revision: i64,
    pub(crate) now_ms: i64,
    pub(crate) cycle: i64,
    pub(crate) time_zone: String,
    pub(crate) kind: PassKind,
    pub(crate) records: Vec<Record>,
    pub(crate) facts: Vec<Fact>,
    pub(crate) lines: Vec<MemoryLine>,
    pub(crate) episodes: Vec<Episode>,
}
impl SleepInput {
    pub fn records(&self) -> &[Record] {
        &self.records
    }
    pub fn facts(&self) -> &[Fact] {
        &self.facts
    }
    pub fn lines(&self) -> &[MemoryLine] {
        &self.lines
    }
    pub fn episodes(&self) -> &[Episode] {
        &self.episodes
    }
    pub fn json(&self) -> Result<String> {
        self.check_current()?;
        let json =
            serde_json::to_string(self).map_err(|_| Error::Invalid("cannot encode sleep input"))?;
        if json.len() > MAX_INPUT_BYTES {
            return Err(Error::Invalid("sleep input too large"));
        }
        Ok(json)
    }
    pub(crate) fn check_current(&self) -> Result<()> {
        if self.validity.has_changed().is_err() || *self.validity.borrow() != self.epoch {
            return Err(Error::Stale);
        }
        Ok(())
    }
}

/// This schema has no quotes or deletion operation. Unknown fields are errors.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SleepProposal {
    pub facts: Vec<FactProposal>,
    pub lines: Vec<LineProposal>,
    pub episodes: Vec<EpisodeProposal>,
}
impl SleepProposal {
    pub fn parse(json: &str) -> Result<Self> {
        if json.len() > MAX_OUTPUT_BYTES {
            return Err(Error::Invalid("sleep output too large"));
        }
        let proposal: Self =
            serde_json::from_str(json).map_err(|_| Error::Invalid("invalid sleep JSON schema"))?;
        if proposal.facts.len() > MAX_PROPOSALS
            || proposal.lines.len() > MAX_PROPOSALS
            || proposal.episodes.len() > MAX_PROPOSALS
        {
            return Err(Error::Invalid("too many proposals"));
        }
        Ok(proposal)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactProposal {
    pub key: String,
    pub claim: String,
    pub record_ids: Vec<i64>,
    pub strength: Strength,
    pub kind: FactKind,
    pub true_since_ms: i64,
    pub true_until_ms: Option<i64>,
    pub supersedes: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineProposal {
    pub replaces: Option<i64>,
    pub kind: LineKind,
    pub text: String,
    /// Existing fact ids and/or keys from this response's facts.
    pub fact_ids: Vec<i64>,
    pub fact_keys: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpisodeProposal {
    pub episode_id: i64,
    pub record_ids: Vec<i64>,
    pub gist: String,
    pub importance: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ApplyReport {
    pub fact_ids: Vec<i64>,
    pub line_ids: Vec<i64>,
    pub records_processed: usize,
    pub pending: bool,
}

/// Public reads are deny-by-default. The product boundary must establish who
/// is asking; this library never guesses from names or model-generated text.
#[derive(Clone, Copy, Debug, Default)]
pub struct Access {
    pub owner: bool,
    pub alone: bool,
}
impl Access {
    pub const OWNER_ALONE: Self = Self {
        owner: true,
        alone: true,
    };
    pub(crate) fn permits(self) -> bool {
        self.owner && self.alone
    }
}

pub(crate) fn text_valid(text: &str, max: usize) -> Result<()> {
    if text.trim().is_empty() || text.len() > max || text.contains('\0') {
        return Err(Error::Invalid("empty, excessive or NUL-containing text"));
    }
    Ok(())
}
pub(crate) fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::Invalid("cannot encode value"))
}
pub(crate) fn decode<T: for<'a> Deserialize<'a>>(value: &str) -> Result<T> {
    serde_json::from_str(value).map_err(|_| Error::Invalid("invalid stored value"))
}
