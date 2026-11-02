//! Final-only Deepgram transcript assembly; no clean-text fallback.
use crate::{Result, audio, hash, invalid, model::Reply};
use serde_json::Value;
#[derive(Clone)]
struct Segment {
    start: u64,
    end: u64,
    text: String,
    confidence: Option<f64>,
}
pub(super) struct Stream {
    expected_arch: String,
    expected_revision: Option<String>,
    segments: Vec<Segment>,
    pending: Vec<(u64, u64)>,
    pub request_id: Option<String>,
    pub model_name: Option<String>,
    pub model_arch: Option<String>,
    pub revision: Option<String>,
    pub model_uuid: Option<String>,
    pub duration: Option<audio::Usage>,
    pub metadata_seen: bool,
}
impl Stream {
    pub fn new(arch: &str, revision: Option<&str>) -> Self {
        Self {
            expected_arch: arch.into(),
            expected_revision: revision.map(str::to_owned),
            segments: vec![],
            pending: vec![],
            request_id: None,
            model_name: None,
            model_arch: None,
            revision: None,
            model_uuid: None,
            duration: None,
            metadata_seen: false,
        }
    }
    fn metadata(&mut self, value: &Value, terminal: bool) -> Result<()> {
        if !value.is_object() {
            return Err(invalid("invalid speech metadata"));
        }
        identity(&mut self.request_id, value.get("request_id"))?;
        identity(&mut self.model_uuid, value.get("model_uuid"))?;
        if terminal && let Some(models) = value.get("models") {
            let models = models
                .as_array()
                .filter(|models| models.len() == 1)
                .ok_or_else(|| invalid("speech terminal model list differs from mono request"))?;
            identity(&mut self.model_uuid, models.first())?;
        }
        if let Some(info) = value.get("model_info") {
            let map = info
                .as_object()
                .ok_or_else(|| invalid("invalid speech model metadata"))?;
            let info = if terminal
                && !map.contains_key("name")
                && !map.contains_key("arch")
                && !map.contains_key("version")
            {
                if map.len() != 1 {
                    return Err(invalid("speech terminal model map must contain one model"));
                }
                let (uuid, info) = map
                    .iter()
                    .next()
                    .ok_or_else(|| invalid("empty speech model map"))?;
                identity(&mut self.model_uuid, Some(&Value::String(uuid.clone())))?;
                if !info.is_object() {
                    return Err(invalid("invalid terminal speech model identity"));
                }
                info
            } else {
                info
            };
            identity(&mut self.model_name, info.get("name"))?;
            identity(&mut self.model_arch, info.get("arch"))?;
            identity(&mut self.revision, info.get("version"))?;
        }
        if self
            .model_arch
            .as_ref()
            .is_some_and(|arch| arch != &self.expected_arch)
            || self.expected_revision.as_ref().is_some_and(|expected| {
                self.revision
                    .as_ref()
                    .is_some_and(|actual| actual != expected)
            })
        {
            return Err(invalid("speech model differs from pinned configuration"));
        }
        Ok(())
    }
    /// True is a Finalize response, not proof of normal stream closure.
    pub fn feed(&mut self, value: &Value) -> Result<bool> {
        if !value.is_object() {
            return Err(invalid("invalid speech service frame"));
        }
        match value.get("type").and_then(Value::as_str) {
            Some("Results") => {
                if self.metadata_seen {
                    return Err(invalid("speech results after terminal metadata"));
                }
                if value.get("channel_index") != Some(&serde_json::json!([0, 1])) {
                    return Err(invalid("speech channel differs from mono request"));
                }
                if let Some(metadata) = value.get("metadata") {
                    self.metadata(metadata, false)?;
                }
                let finalized = value
                    .get("from_finalize")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let final_result = value
                    .get("is_final")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| invalid("speech final flag missing"))?;
                let alternatives = value
                    .pointer("/channel/alternatives")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid("speech alternatives missing"))?;
                if alternatives.len() != 1 {
                    return Err(invalid("speech requires one transcript alternative"));
                }
                let alternative = &alternatives[0];
                let text = alternative
                    .get("transcript")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("speech transcript missing"))?
                    .to_owned();
                let confidence = match alternative.get("confidence").filter(|c| !c.is_null()) {
                    Some(value) => Some(
                        value
                            .as_f64()
                            .filter(|c| c.is_finite() && (0.0..=1.0).contains(c))
                            .ok_or_else(|| invalid("invalid speech confidence"))?,
                    ),
                    None => None,
                };
                let start = micros(
                    value
                        .get("start")
                        .ok_or_else(|| invalid("speech segment start missing"))?,
                )?;
                let duration = micros(
                    value
                        .get("duration")
                        .ok_or_else(|| invalid("speech segment duration missing"))?,
                )?;
                let end = start
                    .checked_add(duration)
                    .ok_or_else(|| invalid("speech segment time overflow"))?;
                if !final_result {
                    if !text.is_empty()
                        && self
                            .segments
                            .last()
                            .is_none_or(|s| end > s.end.saturating_add(1000))
                    {
                        self.note_pending(
                            self.segments
                                .last()
                                .map_or(start, |last| start.max(last.end)),
                            end,
                        )?;
                    }
                    return Ok(false);
                }
                if let Some(prior) = self
                    .segments
                    .iter()
                    .find(|s| s.start == start && s.end == end)
                {
                    if prior.text != text || prior.confidence != confidence {
                        return Err(invalid("conflicting repeated speech final"));
                    }
                    return Ok(finalized);
                }
                if self
                    .segments
                    .last()
                    .is_some_and(|last| start.saturating_add(1000) < last.end)
                {
                    return Err(invalid("overlapping or out-of-order speech final"));
                }
                if self.segments.len() >= 4096
                    || self
                        .segments
                        .iter()
                        .map(|s| s.text.len() + 1)
                        .sum::<usize>()
                        + text.len()
                        > 32_768
                {
                    return Err(invalid("speech transcript exceeds bound"));
                }
                self.resolve_pending(start, end);
                self.segments.push(Segment {
                    start,
                    end,
                    text,
                    confidence,
                });
                Ok(finalized)
            }
            Some("Metadata") => {
                // Preserve an explicit terminal duration even if identity validation
                // subsequently rejects the transcript. It is still a known charge.
                self.metadata_seen = true;
                if let Some(duration) = value.get("duration").filter(|d| !d.is_null()) {
                    let reported = micros(duration)?;
                    let changed = self
                        .duration
                        .as_ref()
                        .is_some_and(|old| old.audio_micros != reported);
                    let highest = self
                        .duration
                        .as_ref()
                        .map_or(reported, |old| old.audio_micros.max(reported));
                    self.duration = Some(audio::Usage {
                        audio_micros: highest,
                    });
                    if changed {
                        return Err(invalid("speech duration changed within response"));
                    }
                }
                self.metadata(value, true)?;
                if value.get("channels").is_some_and(|c| c.as_u64() != Some(1)) {
                    return Err(invalid("speech terminal channel mismatch"));
                }
                Ok(false)
            }
            Some("SpeechStarted") => {
                let start = micros(
                    value
                        .get("timestamp")
                        .ok_or_else(|| invalid("speech start timestamp missing"))?,
                )?;
                if self
                    .segments
                    .last()
                    .is_none_or(|s| start > s.end.saturating_add(1000))
                {
                    self.note_pending(start, start.saturating_add(1))?;
                }
                Ok(false)
            }
            Some("UtteranceEnd") => Ok(false),
            Some("Error") => Err(invalid("speech service refused request")),
            _ => Err(invalid("unsupported speech service frame")),
        }
    }
    fn note_pending(&mut self, start: u64, end: u64) -> Result<()> {
        self.pending.push((start, end));
        self.pending.sort_unstable();
        let mut merged: Vec<(u64, u64)> = vec![];
        for (start, end) in self.pending.drain(..) {
            if let Some(last) = merged.last_mut()
                && start <= last.1.saturating_add(1000)
            {
                last.1 = last.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        self.pending = merged;
        if self.pending.len() > 4096 {
            return Err(invalid("speech pending intervals exceed bound"));
        }
        Ok(())
    }
    fn resolve_pending(&mut self, start: u64, end: u64) {
        let mut remaining = vec![];
        for (left, right) in self.pending.drain(..) {
            if end.saturating_add(1000) < left || start > right.saturating_add(1000) {
                remaining.push((left, right));
                continue;
            }
            if start > left.saturating_add(1000) {
                remaining.push((left, start.min(right)));
            }
            if end.saturating_add(1000) < right {
                remaining.push((end.max(left), right));
            }
        }
        self.pending = remaining;
    }
    pub fn text(&self) -> String {
        self.segments
            .iter()
            .filter(|s| !s.text.is_empty())
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
    pub fn confidence(&self) -> Option<f64> {
        let mut confidence = None;
        for segment in self.segments.iter().filter(|s| !s.text.is_empty()) {
            let next = segment.confidence?;
            confidence = Some(confidence.map_or(next, |old: f64| old.min(next)));
        }
        confidence
    }
    pub fn evidence(&self, input: &audio::Input, samples_sent: u64) -> audio::Evidence {
        audio::Evidence {
            pcm_sha256: input.pcm_sha256().into(),
            samples_sent,
            transcript_sha256: hash(self.text().as_bytes()),
            confidence: self.confidence(),
            model_revision: self.revision.clone(),
            model_uuid: self.model_uuid.clone(),
            duration_usage: self.duration.clone(),
        }
    }
    pub fn reply(&self, input: &audio::Input) -> Result<Reply> {
        if self.request_id.is_none()
            || self.model_name.is_none()
            || self.model_arch.as_deref() != Some(&self.expected_arch)
            || self.revision.is_none()
            || self.segments.is_empty()
            || !self.pending.is_empty()
            || self.segments.last().is_some_and(|s| {
                s.end > input.reserve_micros() + 50_000
                    || s.end.saturating_add(50_000) < input.reserve_micros()
            })
        {
            return Err(invalid(
                "speech response lacks complete pinned identity or audio coverage",
            ));
        }
        Ok(Reply {
            content: Some(self.text()),
            ..Reply::default()
        })
    }
}
fn identity(target: &mut Option<String>, value: Option<&Value>) -> Result<()> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let text = value
        .as_str()
        .ok_or_else(|| invalid("speech identity is not text"))?;
    if text.is_empty()
        || text.len() > 256
        || text.chars().any(char::is_control)
        || target.as_ref().is_some_and(|prior| prior != text)
    {
        return Err(invalid("speech identity changed or exceeds bound"));
    }
    *target = Some(text.into());
    Ok(())
}
fn micros(value: &Value) -> Result<u64> {
    let seconds = value
        .as_f64()
        .filter(|s| s.is_finite() && *s >= 0.0 && *s < 1_000_000_000_000.0)
        .ok_or_else(|| invalid("invalid speech time"))?;
    if seconds == 0.0 {
        return Ok(0);
    }
    // JSON is decimal. Multiplying an f64 then taking ceil adds a microsecond
    // to exact values such as 2.007, incorrectly tripping the spending stop.
    let text = value.to_string();
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (
            mantissa,
            exponent
                .parse::<i32>()
                .map_err(|_| invalid("invalid speech time exponent"))?,
        ),
        None => (text.as_str(), 0),
    };
    let fraction = mantissa.split_once('.').map_or(0, |(_, tail)| tail.len());
    let digits = mantissa.replace('.', "");
    let coefficient = digits
        .parse::<u128>()
        .map_err(|_| invalid("invalid speech decimal"))?;
    let scale = exponent - fraction as i32 + 6;
    let micros = if scale >= 0 {
        coefficient
            .checked_mul(
                10u128
                    .checked_pow(scale as u32)
                    .ok_or_else(|| invalid("speech time overflow"))?,
            )
            .ok_or_else(|| invalid("speech time overflow"))?
    } else if let Some(divisor) = 10u128.checked_pow(scale.unsigned_abs()) {
        coefficient.div_ceil(divisor)
    } else {
        1
    };
    u64::try_from(micros).map_err(|_| invalid("speech time overflow"))
}

#[cfg(test)]
mod tests {
    use super::micros;
    #[test]
    fn decimal_durations_do_not_gain_a_binary_float_microsecond() {
        for (source, expected) in [
            ("2.007", 2_007_000),
            ("0.04", 40_000),
            ("1e-9", 1),
            ("1.0000001", 1_000_001),
            ("2.007e1", 20_070_000),
            ("-0.0", 0),
        ] {
            assert_eq!(
                micros(&serde_json::from_str(source).unwrap()).unwrap(),
                expected,
                "{source}"
            );
        }
        for source in ["-0.1", "1e12", "null", "true", "\"2.007\""] {
            assert!(micros(&serde_json::from_str(source).unwrap()).is_err());
        }
    }
}
