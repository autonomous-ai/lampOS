//! One accounting boundary for every model request, including recorded tests.
use crate::{Result, audio, hash, invalid};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, io::BufRead, time::Instant};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Call {
    /// Constructed only from verified local PCM. Arms cannot deserialize or
    /// invoke this variant, and serialized calls contain no PCM or local path.
    #[serde(skip_deserializing)]
    Transcribe { audio: audio::Input },
    Chat {
        purpose: Purpose,
        messages: Vec<Value>,
        max_output_tokens: u64,
        response_format: Option<Value>,
        tools: Vec<Value>,
        tool_choice: String,
    },
    Embedding {
        purpose: Purpose,
        input: Vec<String>,
    },
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Extract,
    Sleep,
    Reflect,
    Retrieve,
    Answer,
    Judge,
    Transcribe,
}
impl Call {
    pub fn validate(&self) -> Result<()> {
        if serde_json::to_vec(self)?.len() > 1024 * 1024 {
            return Err(invalid("model input exceeds one MiB"));
        }
        match self {
            Self::Transcribe { audio } => audio.validate()?,
            Self::Chat {
                messages,
                max_output_tokens,
                tools,
                tool_choice,
                ..
            } => {
                if messages.is_empty()
                    || messages.len() > 256
                    || tools.len() > 48
                    || !(1..=32_768).contains(max_output_tokens)
                    || !["none", "auto", "required"].contains(&tool_choice.as_str())
                {
                    return Err(invalid("invalid bounded chat request"));
                }
                for m in messages {
                    let role = m.get("role").and_then(Value::as_str).unwrap_or("");
                    if !["system", "user", "assistant", "tool"].contains(&role) {
                        return Err(invalid("invalid message role"));
                    }
                    let tool_only = role == "assistant"
                        && m.get("tool_calls")
                            .and_then(Value::as_array)
                            .is_some_and(|calls| !calls.is_empty());
                    if !(m
                        .get("content")
                        .is_some_and(|c| c.is_string() || c.is_null())
                        || tool_only && m.get("content").is_none())
                    {
                        return Err(invalid("message content must be text or null"));
                    }
                }
            }
            Self::Embedding { input, .. } if input.is_empty() || input.len() > 128 => {
                return Err(invalid("invalid embedding batch"));
            }
            Self::Embedding { .. } => {}
        }
        Ok(())
    }
    pub fn arm_permitted(&self) -> bool {
        let purpose = match self {
            Self::Transcribe { .. } => return false,
            Self::Chat { purpose, .. } | Self::Embedding { purpose, .. } => purpose,
        };
        matches!(
            purpose,
            Purpose::Extract | Purpose::Sleep | Purpose::Reflect | Purpose::Retrieve
        )
    }
    pub fn request_hash(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }
    fn reservation_tokens(&self) -> Result<(u64, u64)> {
        let bytes = serde_json::to_vec(self)?.len() as u64;
        let (overhead, output) = match self {
            Self::Transcribe { .. } => return Err(invalid("speech uses duration accounting")),
            Self::Chat {
                messages,
                max_output_tokens,
                ..
            } => (256 + 128 * messages.len() as u64, *max_output_tokens),
            Self::Embedding { input, .. } => (256 + 128 * input.len() as u64, 0),
        };
        Ok((
            bytes
                .checked_add(overhead)
                .ok_or_else(|| invalid("token reservation overflow"))?,
            output,
        ))
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<Value>,
    #[serde(default)]
    pub tool_calls: Vec<Value>,
    #[serde(default)]
    pub vectors: Vec<Vec<f64>>,
    pub usage: Option<Usage>,
}
impl Reply {
    fn validate(&self, call: &Call, dimensions: Option<usize>) -> Result<()> {
        match call {
            Call::Transcribe { .. } => {
                if self.content.is_none()
                    || self.content.as_ref().is_some_and(|t| t.len() > 32_768)
                    || self.usage.is_some()
                    || !self.vectors.is_empty()
                    || self.reasoning_details.is_some()
                    || !self.tool_calls.is_empty()
                {
                    return Err(invalid("invalid transcription response"));
                }
            }
            Call::Chat {
                max_output_tokens, ..
            } => {
                if !self.vectors.is_empty()
                    || self.content.is_none() && self.tool_calls.is_empty()
                    || self
                        .usage
                        .as_ref()
                        .is_some_and(|u| u.output_tokens > *max_output_tokens)
                {
                    return Err(invalid("invalid chat response"));
                }
                for tool in &self.tool_calls {
                    if tool.get("type").and_then(Value::as_str) != Some("function")
                        || !tool.get("id").is_some_and(Value::is_string)
                        || !tool.pointer("/function/name").is_some_and(Value::is_string)
                        || !tool
                            .pointer("/function/arguments")
                            .is_some_and(Value::is_string)
                    {
                        return Err(invalid("invalid structured tool call"));
                    }
                }
            }
            Call::Embedding { input, .. } => {
                let dimensions =
                    dimensions.ok_or_else(|| invalid("embedding dimension not configured"))?;
                if dimensions == 0
                    || self.vectors.len() != input.len()
                    || self
                        .vectors
                        .iter()
                        .any(|v| v.len() != dimensions || v.iter().any(|n| !n.is_finite()))
                    || self.content.is_some()
                    || self.reasoning_details.is_some()
                    || !self.tool_calls.is_empty()
                    || self.usage.as_ref().is_some_and(|u| u.output_tokens != 0)
                {
                    return Err(invalid("invalid embedding response"));
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Prices {
    pub source: String,
    pub date: String,
    pub input_microusd_per_million: u64,
    pub output_microusd_per_million: u64,
}
impl Prices {
    fn charge(&self, input: u64, output: u64) -> Result<u64> {
        let numerator = (u128::from(input) * u128::from(self.input_microusd_per_million))
            .checked_add(u128::from(output) * u128::from(self.output_microusd_per_million))
            .ok_or_else(|| invalid("model price overflow"))?;
        u64::try_from(numerator.div_ceil(1_000_000)).map_err(|_| invalid("model price overflow"))
    }
}
pub trait Provider {
    /// Network providers must enforce this deadline in their transport;
    /// checking only after a blocking call is not a timeout implementation.
    fn invoke(&mut self, call: &Call, deadline: Instant) -> Result<Reply>;
    /// Taken once after an invocation, including a failed request. Kept out of
    /// the arm's Reply wire shape. Recorded providers need no network evidence.
    fn take_provenance(&mut self) -> Option<Provenance> {
        None
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub transport: String,
    pub requested_model: String,
    pub reported_model: Option<String>,
    pub provider_request_id: Option<String>,
    pub system_fingerprint: Option<String>,
    pub finish_reason: Option<String>,
    pub http_status: Option<u16>,
    pub request_sha256: String,
    pub response_sha256: String,
    /// Relative directory inside this run's private provider-artifact folder.
    pub artifact: String,
    pub response_complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<audio::Evidence>,
    pub evidence_complete: bool,
    /// Only terminal provider usage, never inferred from returned text.
    pub usage: Option<Usage>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    pub call_number: u64,
    pub request_hash: String,
    pub reserved_microusd: u64,
    pub charged_microusd: u64,
    pub request_bytes: usize,
    pub outcome: String,
    pub usage: Option<Usage>,
    /// Input rates conservatively include cached input at the ordinary rate.
    /// This is a budget debit, not an assertion about a provider's invoice.
    pub cost_basis: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_usage: Option<audio::Usage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}
type Audit = Box<dyn FnMut(&str, &Receipt) -> Result<()> + Send>;
pub struct Client<P> {
    provider: P,
    chat_prices: Prices,
    embedding_prices: Option<Prices>,
    speech_prices: Option<audio::Prices>,
    dimensions: Option<usize>,
    limit: u64,
    charged: u64,
    next_call: u64,
    poisoned: bool,
    receipts: Vec<Receipt>,
    audit: Option<Audit>,
}
impl<P: Provider> Client<P> {
    pub fn new(provider: P, prices: Prices, limit_microusd: u64) -> Result<Self> {
        if limit_microusd == 0 || prices.source.is_empty() || prices.date.is_empty() {
            return Err(invalid("explicit budget and dated pricing source required"));
        }
        Ok(Self {
            provider,
            chat_prices: prices,
            embedding_prices: None,
            speech_prices: None,
            dimensions: None,
            limit: limit_microusd,
            charged: 0,
            next_call: 0,
            poisoned: false,
            receipts: Vec::new(),
            audit: None,
        })
    }
    /// A run attaches its append-and-sync ledger here. If either write fails,
    /// stop the client; no unrecorded new dispatch is allowed.
    pub fn with_audit(
        mut self,
        audit: impl FnMut(&str, &Receipt) -> Result<()> + Send + 'static,
    ) -> Self {
        self.audit = Some(Box::new(audit));
        self
    }
    pub fn with_speech(mut self, prices: audio::Prices) -> Result<Self> {
        prices.validate()?;
        self.speech_prices = Some(prices);
        Ok(self)
    }
    pub fn with_embeddings(mut self, prices: Prices, dimensions: usize) -> Result<Self> {
        if dimensions == 0
            || dimensions > 32_768
            || prices.source.is_empty()
            || prices.date.is_empty()
        {
            return Err(invalid("invalid embedding configuration"));
        }
        self.embedding_prices = Some(prices);
        self.dimensions = Some(dimensions);
        Ok(self)
    }
    pub fn charged_microusd(&self) -> u64 {
        self.charged
    }
    pub fn receipts(&self) -> &[Receipt] {
        &self.receipts
    }
    pub fn stopped(&self) -> bool {
        self.poisoned
    }
    pub fn call(&mut self, call: &Call, deadline: Instant) -> Result<Reply> {
        if self.poisoned || self.next_call >= 100_000 {
            return Err(invalid(
                "model client stopped after accounting failure or call limit",
            ));
        }
        call.validate()?;
        if Instant::now() >= deadline {
            return Err(invalid("model deadline expired before dispatch"));
        }
        let billing = match call {
            Call::Transcribe { audio } => Billing::Speech {
                prices: self
                    .speech_prices
                    .as_ref()
                    .ok_or_else(|| invalid("speech pricing not configured"))?,
                micros: audio.reserve_micros(),
            },
            Call::Chat { .. } | Call::Embedding { .. } => {
                let prices = if matches!(call, Call::Chat { .. }) {
                    &self.chat_prices
                } else {
                    self.embedding_prices
                        .as_ref()
                        .ok_or_else(|| invalid("embedding not configured"))?
                };
                let (input, output) = call.reservation_tokens()?;
                Billing::Tokens {
                    prices,
                    input,
                    output,
                }
            }
        };
        let reserve = billing.reserve()?;
        let next = self
            .charged
            .checked_add(reserve)
            .ok_or_else(|| invalid("budget overflow"))?;
        if next > self.limit {
            return Err(invalid("model budget refused before dispatch"));
        }
        self.charged = next;
        self.next_call += 1;
        let mut receipt = Receipt {
            call_number: self.next_call,
            request_hash: call.request_hash()?,
            reserved_microusd: reserve,
            charged_microusd: reserve,
            request_bytes: serde_json::to_vec(call)?.len(),
            outcome: "uncertain".into(),
            usage: None,
            cost_basis: if matches!(call, Call::Transcribe { .. }) {
                "conservative_audio_duration_debit"
            } else {
                "conservative_token_debit"
            }
            .into(),
            audio_usage: None,
            provenance: None,
        };
        if let Some(audit) = &mut self.audit
            && let Err(error) = audit("model_reserved", &receipt)
        {
            self.poisoned = true;
            return Err(error);
        }
        let dispatched = if Instant::now() >= deadline {
            self.charged -= reserve;
            receipt.charged_microusd = 0;
            receipt.outcome = "expired_before_dispatch".into();
            Err(invalid(
                "model deadline expired while recording reservation",
            ))
        } else {
            let result = self.provider.invoke(call, deadline);
            receipt.provenance = self.provider.take_provenance();
            result
        };
        receipt.usage = dispatched
            .as_ref()
            .ok()
            .and_then(|r| r.usage.clone())
            .or_else(|| receipt.provenance.as_ref().and_then(|p| p.usage.clone()));
        receipt.audio_usage = receipt
            .provenance
            .as_ref()
            .filter(|p| p.response_complete)
            .and_then(|p| p.audio.as_ref())
            .and_then(|a| a.duration_usage.clone());
        let audio_evidence = receipt.provenance.as_ref().and_then(|p| p.audio.as_ref());
        let audio_mismatch = if let Call::Transcribe { audio } = call {
            receipt.usage.is_some()
                || dispatched.is_ok() && audio_evidence.is_none()
                || audio_evidence.is_some_and(|a| {
                    a.pcm_sha256 != audio.pcm_sha256()
                        || a.samples_sent != audio.samples()
                        || a.confidence
                            .is_some_and(|c| !c.is_finite() || !(0.0..=1.0).contains(&c))
                        || dispatched
                            .as_ref()
                            .ok()
                            .and_then(|r| r.content.as_ref())
                            .is_some_and(|text| a.transcript_sha256 != hash(text.as_bytes()))
                })
        } else {
            audio_evidence.is_some()
        };
        let evidence_failed = audio_mismatch
            || receipt
                .provenance
                .as_ref()
                .is_some_and(|p| !p.evidence_complete);
        let result = (|| {
            let known = match billing.known(&receipt) {
                Ok(known) => known,
                Err(error) => {
                    self.charged = u64::MAX;
                    self.poisoned = true;
                    receipt.charged_microusd = u64::MAX;
                    receipt.outcome = "accounting_overflow".into();
                    return Err(error);
                }
            };
            if let Some((actual, exceeded)) = known {
                let actual = if evidence_failed {
                    actual.max(reserve)
                } else {
                    actual
                };
                self.charged = (self.charged - reserve).saturating_add(actual);
                receipt.charged_microusd = actual;
                receipt.outcome = "accounted".into();
                if exceeded {
                    self.poisoned = true;
                    receipt.outcome = "reservation_exceeded".into();
                    return Err(invalid("provider usage exceeded conservative reservation"));
                }
            } else if dispatched.is_ok() {
                receipt.outcome = "usage_unknown".into();
            }
            if evidence_failed {
                self.poisoned = true;
                receipt.outcome = "evidence_failed".into();
                return Err(invalid("provider evidence failed; accounting stopped"));
            }
            let reply = dispatched?;
            if Instant::now() > deadline {
                return Err(invalid("model response missed deadline"));
            }
            reply.validate(call, self.dimensions)?;
            Ok(reply)
        })();
        let audited = self
            .audit
            .as_mut()
            .map_or(Ok(()), |audit| audit("model_completed", &receipt));
        self.receipts.push(receipt);
        if let Err(error) = audited {
            self.poisoned = true;
            return Err(error);
        }
        result
    }
}

enum Billing<'a> {
    Tokens {
        prices: &'a Prices,
        input: u64,
        output: u64,
    },
    Speech {
        prices: &'a audio::Prices,
        micros: u64,
    },
}
impl Billing<'_> {
    fn reserve(&self) -> Result<u64> {
        match self {
            Self::Tokens {
                prices,
                input,
                output,
            } => prices.charge(*input, *output),
            Self::Speech { prices, micros } => prices.charge(*micros),
        }
    }
    fn known(&self, receipt: &Receipt) -> Result<Option<(u64, bool)>> {
        match self {
            Self::Tokens {
                prices,
                input,
                output,
            } => receipt
                .usage
                .as_ref()
                .map(|u| {
                    Ok((
                        prices.charge(u.input_tokens, u.output_tokens)?,
                        u.input_tokens > *input || u.output_tokens > *output,
                    ))
                })
                .transpose(),
            Self::Speech { prices, micros } => receipt
                .audio_usage
                .as_ref()
                .map(|u| Ok((prices.charge(u.audio_micros)?, u.audio_micros > *micros)))
                .transpose(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    pub request_hash: String,
    pub reply: Reply,
}
pub struct Recorded {
    replies: BTreeMap<String, Reply>,
}
impl Recorded {
    pub fn read(reader: impl BufRead) -> Result<Self> {
        let mut replies = BTreeMap::new();
        let mut reader = reader;
        while let Some(frame) = crate::protocol::read_frame(&mut reader)? {
            let record: Recording = serde_json::from_value(frame)?;
            if record.request_hash.len() != 64
                || !record.request_hash.bytes().all(|b| b.is_ascii_hexdigit())
                || replies.insert(record.request_hash, record.reply).is_some()
            {
                return Err(invalid("invalid or duplicate recorded request hash"));
            }
            if replies.len() > 100_000 {
                return Err(invalid("too many recorded requests"));
            }
        }
        Ok(Self { replies })
    }
}
impl Provider for Recorded {
    fn invoke(&mut self, call: &Call, _deadline: Instant) -> Result<Reply> {
        self.replies
            .get(&call.request_hash()?)
            .cloned()
            .ok_or_else(|| invalid("recorded request miss; live fallback forbidden"))
    }
}
