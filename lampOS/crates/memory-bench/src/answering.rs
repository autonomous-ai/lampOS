//! Common answer/tool loop. Memory selection stays inside the adapter.
use crate::{
    Result,
    conversation::Conversation,
    evaluation, hash, invalid,
    model::{Call, Purpose, Reply},
    model_worker::Worker,
    protocol::{ForgetEffect, Op, Query, Request},
    runner::Runner,
    score::Answer,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub enum MemoryMode {
    None,
    Bounded,
    FullHistory,
}
#[derive(Serialize)]
pub struct Turn {
    pub answer: Answer,
    pub forget_effect: ForgetEffect,
    pub model_rounds: usize,
    pub tools_executed: usize,
}
/// The loop owns no provider and cannot bypass the run's accounting worker.
pub struct Answerer<'a> {
    pub runner: &'a mut Runner,
    pub worker: &'a mut Worker,
    pub recent: &'a mut Conversation,
    pub timezone: &'a str,
    pub mode: MemoryMode,
}
struct Invocation {
    id: String,
    name: String,
    arguments: Value,
}
fn advertised_tools(tools: &[Value]) -> Result<BTreeSet<&str>> {
    let mut advertised = BTreeSet::new();
    for tool in tools {
        let name = tool
            .pointer("/function/name")
            .and_then(Value::as_str)
            .filter(|s| crate::protocol::valid_id(s))
            .ok_or_else(|| invalid("invalid advertised tool name"))?;
        if tool["type"] != "function" || !advertised.insert(name) {
            return Err(invalid("duplicate or invalid tool schema"));
        }
    }
    Ok(advertised)
}
fn invocations(
    reply: &Reply,
    tools: &[Value],
    used: &mut BTreeSet<String>,
) -> Result<Vec<Invocation>> {
    if reply.tool_calls.len() > 48 {
        return Err(invalid("tool batch exceeds 48 calls"));
    }
    let advertised = advertised_tools(tools)?;
    let mut parsed = Vec::new();
    for raw in &reply.tool_calls {
        let id = raw["id"]
            .as_str()
            .filter(|s| crate::protocol::valid_id(s))
            .ok_or_else(|| invalid("invalid tool call identity"))?;
        let name = raw
            .pointer("/function/name")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("tool call has no name"))?;
        if raw["type"] != "function" || !advertised.contains(name) || !used.insert(id.into()) {
            return Err(invalid("unadvertised or duplicate tool call"));
        }
        let raw_args = raw
            .pointer("/function/arguments")
            .and_then(Value::as_str)
            .filter(|s| s.len() <= 16_384)
            .ok_or_else(|| invalid("unbounded tool arguments"))?;
        let arguments: Value = serde_json::from_str(raw_args)?;
        if !arguments.is_object() {
            return Err(invalid("tool arguments must be an object"));
        }
        parsed.push(Invocation {
            id: id.into(),
            name: name.into(),
            arguments,
        });
    }
    Ok(parsed)
}
// Arm-owned keys and scalar text count; runner-added JSON punctuation and
// provenance fields do not. JSON escapes never inflate the UTF-8 text charge.
fn result_bytes(value: &Value) -> Result<usize> {
    let add = |a: usize, b: usize| {
        a.checked_add(b)
            .ok_or_else(|| invalid("tool text byte overflow"))
    };
    match value {
        Value::String(s) => Ok(s.len()),
        Value::Array(values) => values.iter().try_fold(0, |n, v| add(n, result_bytes(v)?)),
        Value::Object(values) => values
            .iter()
            .try_fold(0, |n, (k, v)| add(add(n, k.len())?, result_bytes(v)?)),
        _ => Ok(value.to_string().len()),
    }
}
const INVALIDATED_RESULT: &str =
    "Earlier memory tool result invalidated by a subsequent memory operation.";
fn assistant(reply: &Reply) -> Value {
    let mut message =
        json!({"role":"assistant","content":reply.content,"tool_calls":reply.tool_calls});
    if let Some(details) = &reply.reasoning_details {
        message["reasoning_details"] = details.clone();
    }
    message
}
fn clock_context(query: &Query, timezone: &str) -> Result<Value> {
    let stamp = jiff::Timestamp::from_millisecond(
        i64::try_from(query.at_unix_ms).map_err(|_| invalid("answer clock out of range"))?,
    )
    .map_err(|_| invalid("answer clock out of range"))?;
    let zone = jiff::tz::TimeZone::get(timezone).map_err(|_| invalid("invalid answer timezone"))?;
    Ok(
        json!({"at_unix_ms":query.at_unix_ms,"local_time":stamp.to_zoned(zone).to_string(),"timezone":timezone,
        "speaker":query.person,"person_id":query.person_id,"presence":query.presence,"cues":query.cues}),
    )
}
impl Answerer<'_> {
    pub fn answer(
        &mut self,
        query: &Query,
        mut log: impl FnMut(&str, Value) -> Result<()>,
    ) -> Result<Turn> {
        query.validate()?;
        let context = clock_context(query, self.timezone)?;
        let mut memory_query = query.clone();
        if matches!(self.mode, MemoryMode::FullHistory) {
            memory_query.max_context_bytes = 1_048_576;
        }
        let mut transcript = Vec::new();
        let mut seen = BTreeSet::new();
        let mut combined = ForgetEffect::default();
        let mut executed = 0;
        let mut fresh_tool_bytes = 0usize;
        let turn_cap = memory_query.max_context_bytes;
        for round in 0..evaluation::TOOL_ROUNDS {
            memory_query.max_context_bytes = turn_cap.saturating_sub(fresh_tool_bytes).max(1);
            let memory = self.runner.memory(&memory_query, self.worker)?;
            advertised_tools(&memory.tools)?;
            let combined_bytes = memory
                .memory_text_bytes
                .checked_add(fresh_tool_bytes)
                .ok_or_else(|| invalid("combined memory byte overflow"))?;
            if combined_bytes > turn_cap {
                return Err(invalid("memory and fresh tool result exceed prompt cap"));
            }
            if matches!(self.mode, MemoryMode::None)
                && (memory.memory_text_bytes != 0 || !memory.tools.is_empty())
            {
                return Err(invalid("no-memory setup supplied memory"));
            }
            memory.require_current(self.runner)?;
            log(
                "answer_memory",
                json!({"probe":query.probe_id,"at_unix_ms":query.at_unix_ms,"round":round,"text_hash":hash(memory.text.as_bytes()),"memory_text_bytes":memory.memory_text_bytes,"framing_bytes":memory.framing_bytes,"fresh_tool_text_bytes":fresh_tool_bytes,"combined_memory_text_bytes":combined_bytes,"snapshot_missed":memory.snapshot_missed,"recall_missed":memory.recall_missed,"recall_skipped":memory.recall_skipped}),
            )?;
            let mut messages = vec![
                json!({"role":"system","content":evaluation::ANSWER_PROMPT}),
                json!({"role":"system","content":serde_json::to_string(&context)?}),
            ];
            if !memory.text.is_empty() {
                messages.push(json!({"role":"system","content":memory.text}));
            }
            messages.extend(self.recent.evidence_messages(query)?);
            if combined.ran {
                messages.push(json!({"role":"system","content":"A forget operation was attempted during this answer. Earlier in-turn content has been discarded. Use only the fresh evidence now supplied; do not repeat or reconstruct removed content."}));
            }
            messages.push(json!({"role":"user","content":query.heard}));
            messages.extend(transcript.iter().cloned());
            let call = Call::Chat {
                purpose: Purpose::Answer,
                messages,
                max_output_tokens: evaluation::OUTPUT_TOKENS,
                response_format: Some(json!({"type":"json_object"})),
                tools: memory.tools.clone(),
                tool_choice: if memory.tools.is_empty() {
                    "none"
                } else {
                    "auto"
                }
                .into(),
            };
            call.validate()?;
            let request_hash = call.request_hash()?;
            let reply = self.worker.call(
                &call,
                Instant::now() + Duration::from_millis(crate::protocol::MODEL_MS),
            )?;
            log(
                "answer_reply",
                json!({"probe":query.probe_id,"at_unix_ms":query.at_unix_ms,"round":round,"request_hash":request_hash,"reply":reply}),
            )?;
            if reply.tool_calls.is_empty() {
                let text = reply
                    .content
                    .ok_or_else(|| invalid("answer omitted text"))?;
                let answer: Answer = serde_json::from_str(&text)?;
                if answer.answer.len() > 32_768
                    || answer.evidence_ids.len() > 128
                    || answer
                        .evidence_ids
                        .iter()
                        .any(|id| !crate::protocol::valid_id(id))
                {
                    return Err(invalid("answer exceeds bounded format"));
                }
                return Ok(Turn {
                    answer,
                    forget_effect: combined,
                    model_rounds: round + 1,
                    tools_executed: executed,
                });
            }
            // Validate the whole offered batch before any of its side effects.
            let calls = invocations(&reply, &memory.tools, &mut seen)?;
            if round + 1 == evaluation::TOOL_ROUNDS {
                return Err(invalid("answer exhausted registered tool rounds"));
            }
            transcript.clear();
            fresh_tool_bytes = 0;
            transcript.push(assistant(&reply));
            for (index, tool) in calls.into_iter().enumerate() {
                let request = Request::new(
                    format!(
                        "tool-{}",
                        hash(&serde_json::to_vec(&(
                            &query.probe_id,
                            query.at_unix_ms,
                            round,
                            index
                        ))?)
                    ),
                    Op::Tool,
                    json!({"probe_id":query.probe_id,"name":tool.name,"arguments":tool.arguments}),
                );
                let outcome = self.runner.execute(request.clone(), self.worker)?;
                log(
                    "answer_tool",
                    json!({"request":request,"outcome":outcome,"tool_call_id":tool.id}),
                )?;
                executed += 1;
                let effect: ForgetEffect = serde_json::from_value(
                    outcome
                        .data
                        .get("forget_effect")
                        .cloned()
                        .ok_or_else(|| invalid("tool omitted forgetting effect"))?,
                )?;
                self.recent.apply_effect(effect)?;
                combined.ran |= effect.ran;
                combined.suppress_confirmation |= effect.suppress_confirmation;
                if effect.ran {
                    // Do not execute stale remaining batch calls after forgetting.
                    // Accounting and tool IDs are deliberately not rolled back.
                    transcript.clear();
                    fresh_tool_bytes = 0;
                    log(
                        "answer_forget_restart",
                        json!({"probe":query.probe_id,"at_unix_ms":query.at_unix_ms,"round":round,"status":"attempted; prior round discarded"}),
                    )?;
                    break;
                }
                for prior in &mut transcript {
                    if prior["role"] == "tool" {
                        prior["content"] = json!(INVALIDATED_RESULT);
                    }
                }
                fresh_tool_bytes = result_bytes(&outcome.data["result"])?;
                if fresh_tool_bytes > turn_cap {
                    return Err(invalid("fresh memory tool result exceeds prompt cap"));
                }
                let payload = json!({"result":outcome.data["result"],"source_ids":outcome.data["source_ids"],"forget_effect":effect});
                log(
                    "answer_tool_text",
                    json!({"probe":query.probe_id,"at_unix_ms":query.at_unix_ms,"round":round,"index":index,"text_bytes":fresh_tool_bytes,"framing_bytes":serde_json::to_vec(&payload)?.len().saturating_sub(fresh_tool_bytes)}),
                )?;
                transcript.push(json!({"role":"tool","tool_call_id":tool.id,"content":serde_json::to_string(&payload)?}));
            }
        }
        Err(invalid("answer exhausted registered rounds"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_text_counts_utf8_keys_and_values_without_runner_escaping() {
        let result = json!({"é\"":"雪\\","n":12,"b":true});
        assert_eq!(result_bytes(&result).unwrap(), 3 + 4 + 1 + 2 + 1 + 4);
        assert!(serde_json::to_vec(&result).unwrap().len() > result_bytes(&result).unwrap());
    }
}
