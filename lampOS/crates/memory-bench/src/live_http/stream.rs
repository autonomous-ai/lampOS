use crate::{
    Result, invalid,
    model::{Reply, Usage},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Instant};

#[derive(Default)]
struct Tool {
    id: String,
    name: String,
    arguments: String,
    extra: Option<Value>,
}
pub(super) struct Stream {
    expected: String,
    secret: String,
    pub credential_echo: bool,
    pending: Vec<u8>,
    data: String,
    events: usize,
    lines: usize,
    content: String,
    content_seen: bool,
    tools: BTreeMap<usize, Tool>,
    reasoning: Vec<Value>,
    pub model: Option<String>,
    pub id: Option<String>,
    pub fingerprint: Option<String>,
    pub finish: Option<String>,
    pub usage: Option<Usage>,
    pub done: bool,
}
impl Stream {
    pub fn new(expected: &str, secret: &str) -> Self {
        Self {
            expected: expected.into(),
            secret: secret.into(),
            credential_echo: false,
            pending: Vec::new(),
            data: String::new(),
            events: 0,
            lines: 0,
            content: String::new(),
            content_seen: false,
            tools: BTreeMap::new(),
            reasoning: Vec::new(),
            model: None,
            id: None,
            fingerprint: None,
            finish: None,
            usage: None,
            done: false,
        }
    }
    pub fn feed(&mut self, bytes: &[u8], deadline: Instant) -> Result<bool> {
        self.pending.extend_from_slice(bytes);
        let mut consumed = 0;
        while let Some(offset) = self.pending[consumed..].iter().position(|b| *b == b'\n') {
            if Instant::now() >= deadline {
                return Err(invalid("chat parsing deadline expired"));
            }
            let end = consumed + offset;
            if self.lines >= 100_000 || offset > 256 * 1024 {
                return Err(invalid("chat line count or size exceeds bound"));
            }
            self.lines += 1;
            let line = self.pending[consumed..end].to_vec();
            consumed = end + 1;
            let line = std::str::from_utf8(&line)
                .map_err(|_| invalid("non-UTF8 chat frame"))?
                .trim_end_matches('\r');
            if line.is_empty() {
                if !self.data.is_empty() {
                    let data = std::mem::take(&mut self.data);
                    self.event(data.trim_end_matches('\n'))?;
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data.push_str(data.strip_prefix(' ').unwrap_or(data));
                self.data.push('\n');
            } else if !line.starts_with(':')
                && !line.starts_with("event:")
                && !line.starts_with("id:")
                && !line.starts_with("retry:")
            {
                return Err(invalid("invalid chat event-stream field"));
            }
            if self.data.len() > 256 * 1024 {
                return Err(invalid("chat event exceeds bound"));
            }
        }
        self.pending.drain(..consumed);
        if self.pending.len() > 256 * 1024 {
            return Err(invalid("chat line exceeds bound"));
        }
        if self.done && (!self.pending.is_empty() || !self.data.is_empty()) {
            return Err(invalid("data after chat terminal marker"));
        }
        Ok(self.done)
    }
    fn event(&mut self, data: &str) -> Result<()> {
        if self.done || self.events >= 32_768 {
            return Err(invalid("chat event count or terminal boundary violated"));
        }
        self.events += 1;
        if data == "[DONE]" {
            if self.finish.is_none() || self.model.is_none() || self.id.is_none() {
                return Err(invalid("chat terminal identity or finish reason missing"));
            }
            self.done = true;
            return Ok(());
        }
        let chunk: Value =
            serde_json::from_str(data).map_err(|_| invalid("invalid chat JSON frame"))?;
        if super::contains_secret(&chunk, &self.secret) {
            self.credential_echo = true;
            return Err(invalid("provider response echoed a credential"));
        }
        if !chunk.is_object() || chunk.get("error").is_some() {
            return Err(invalid("chat service returned an error frame"));
        }
        identity(&mut self.model, chunk.get("model"))?;
        identity(&mut self.id, chunk.get("id"))?;
        identity(&mut self.fingerprint, chunk.get("system_fingerprint"))?;
        if self.model.as_ref().is_some_and(|m| m != &self.expected) {
            return Err(invalid("provider model differs from pinned identity"));
        }
        let choices = chunk
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("chat choices missing"))?;
        // Early usage may be provisional. Only a finishing choice or a later
        // usage-only event can supply the final debit.
        let terminal_usage = choices.len() == 1
            && choices[0]
                .get("finish_reason")
                .is_some_and(Value::is_string)
            || choices.is_empty() && self.finish.is_some();
        if let Some(usage) = chunk.get("usage").filter(|u| !u.is_null()) {
            let input = usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("chat input usage missing"))?;
            let output = usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("chat output usage missing"))?;
            if usage
                .get("total_tokens")
                .is_some_and(|t| t.as_u64() != input.checked_add(output))
            {
                return Err(invalid("inconsistent chat token totals"));
            }
            if terminal_usage {
                if self
                    .usage
                    .as_ref()
                    .is_some_and(|u| u.input_tokens != input || u.output_tokens != output)
                {
                    return Err(invalid("terminal chat usage changed within response"));
                }
                self.usage = Some(Usage {
                    input_tokens: input,
                    output_tokens: output,
                });
            }
        }
        if choices.is_empty() {
            if chunk.get("usage").is_none_or(Value::is_null) {
                return Err(invalid("empty chat chunk without usage"));
            }
            return Ok(());
        }
        if choices.len() != 1 || choices[0].get("index").and_then(Value::as_u64) != Some(0) {
            return Err(invalid("chat requires exactly one choice"));
        }
        let choice = &choices[0];
        let delta = choice
            .get("delta")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("chat delta missing"))?;
        if self.finish.is_some() && !delta.is_empty() {
            return Err(invalid("chat content after finish"));
        }
        if delta
            .get("role")
            .is_some_and(|r| r.as_str() != Some("assistant"))
        {
            return Err(invalid("invalid chat reply role"));
        }
        if let Some(text) = delta.get("content").filter(|t| !t.is_null()) {
            self.content.push_str(
                text.as_str()
                    .ok_or_else(|| invalid("chat content is not text"))?,
            );
            self.content_seen = true;
        }
        if let Some(details) = delta.get("reasoning_details").filter(|r| !r.is_null()) {
            self.reasoning.extend(
                details
                    .as_array()
                    .ok_or_else(|| invalid("invalid reasoning details"))?
                    .iter()
                    .cloned(),
            );
            if self.reasoning.len() > 256 {
                return Err(invalid("too many reasoning details"));
            }
        }
        if let Some(text) = delta.get("reasoning_content").and_then(Value::as_str)
            && !text.is_empty()
        {
            return Err(invalid(
                "provider enabled reasoning despite disabled setting",
            ));
        }
        if let Some(calls) = delta.get("tool_calls").filter(|c| !c.is_null()) {
            for call in calls
                .as_array()
                .ok_or_else(|| invalid("invalid tool deltas"))?
            {
                let index = call
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("tool delta index missing"))?;
                if index >= 48 {
                    return Err(invalid("tool delta index exceeds bound"));
                }
                let tool = self.tools.entry(index as usize).or_default();
                if let Some(id) = call.get("id") {
                    let id = id.as_str().ok_or_else(|| invalid("invalid tool id"))?;
                    if id.is_empty() || id.len() > 256 || !tool.id.is_empty() && tool.id != id {
                        return Err(invalid("tool id changed or exceeds bound"));
                    }
                    tool.id = id.into();
                }
                if call
                    .get("type")
                    .is_some_and(|t| t.as_str() != Some("function"))
                {
                    return Err(invalid("unsupported tool type"));
                }
                if let Some(extra) = call.get("extra_content") {
                    if !extra.is_object() || tool.extra.as_ref().is_some_and(|old| old != extra) {
                        return Err(invalid("tool extra content changed or invalid"));
                    }
                    tool.extra = Some(extra.clone());
                }
                if let Some(function) = call.get("function") {
                    if !function.is_object() {
                        return Err(invalid("invalid tool function"));
                    }
                    for (name, target) in
                        [("name", &mut tool.name), ("arguments", &mut tool.arguments)]
                    {
                        if let Some(part) = function.get(name) {
                            target.push_str(
                                part.as_str()
                                    .ok_or_else(|| invalid("invalid tool fragment"))?,
                            );
                        }
                    }
                }
                if tool.name.len() > 128 || tool.arguments.len() > 256 * 1024 {
                    return Err(invalid("tool call exceeds bound"));
                }
            }
        }
        if self.content.contains(&self.secret)
            || self
                .tools
                .values()
                .any(|t| t.name.contains(&self.secret) || t.arguments.contains(&self.secret))
        {
            self.credential_echo = true;
            return Err(invalid("provider fragments echoed a credential"));
        }
        identity(&mut self.finish, choice.get("finish_reason"))?;
        if self.content.len() > 512 * 1024
            || serde_json::to_vec(&self.reasoning)?.len() > 256 * 1024
        {
            return Err(invalid("chat aggregate text exceeds bound"));
        }
        Ok(())
    }
    pub fn reply(&self) -> Result<Reply> {
        if !self.done || !matches!(self.finish.as_deref(), Some("stop" | "tool_calls")) {
            return Err(invalid(
                "chat did not finish normally (truncation/filter/refusal)",
            ));
        }
        if (self.finish.as_deref() == Some("tool_calls")) == self.tools.is_empty() {
            return Err(invalid("chat finish reason disagrees with tool calls"));
        }
        let mut calls = Vec::new();
        for (expected, (index, tool)) in self.tools.iter().enumerate() {
            if expected != *index
                || tool.id.is_empty()
                || tool.name.is_empty()
                || tool.arguments.is_empty()
                || calls.iter().any(|call: &Value| call["id"] == tool.id)
            {
                return Err(invalid("incomplete or duplicate tool call"));
            }
            let mut value = json!({"id":tool.id,"type":"function","function":{"name":tool.name,"arguments":tool.arguments}});
            if let Some(extra) = &tool.extra {
                value["extra_content"] = extra.clone();
            }
            calls.push(value);
        }
        if !self.content_seen && calls.is_empty() {
            return Err(invalid("empty chat response"));
        }
        Ok(Reply {
            content: self.content_seen.then(|| self.content.clone()),
            tool_calls: calls,
            reasoning_details: (!self.reasoning.is_empty()).then(|| json!(self.reasoning)),
            usage: self.usage.clone(),
            vectors: vec![],
        })
    }
}
fn identity(target: &mut Option<String>, value: Option<&Value>) -> Result<()> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let value = value
        .as_str()
        .ok_or_else(|| invalid("provider identity is not text"))?;
    if value.is_empty()
        || value.len() > 256
        || value.chars().any(char::is_control)
        || target.as_ref().is_some_and(|old| old != value)
    {
        return Err(invalid("provider identity changed or exceeds bound"));
    }
    *target = Some(value.into());
    Ok(())
}
