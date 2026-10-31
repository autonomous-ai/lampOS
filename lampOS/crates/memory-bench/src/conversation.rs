//! The common immediate conversation, separate from each arm's durable memory.
use crate::{
    Result, invalid,
    protocol::{Event, EventKind, ForgetEffect, Person, Query},
};
use serde_json::{Value, json};
use std::collections::VecDeque;

#[derive(Clone, Default)]
pub struct Conversation {
    exchanges: VecDeque<Event>,
}
impl Conversation {
    pub fn apply_effect(&mut self, effect: ForgetEffect) -> Result<()> {
        effect.validate()?;
        if effect.ran {
            self.exchanges.clear();
        }
        Ok(())
    }
    /// Apply the actual action before retaining the new exchange. A forget
    /// confirmation cannot teach its own deleted content to the next turn.
    pub fn record(&mut self, event: &Event, effect: ForgetEffect) -> Result<()> {
        event.validate()?;
        self.apply_effect(effect)?;
        if effect.suppress_confirmation || event.kind != EventKind::Utterance {
            return Ok(());
        }
        if self
            .exchanges
            .back()
            .is_some_and(|e| e.at_unix_ms > event.at_unix_ms)
        {
            return Err(invalid("recent conversation clock moved backward"));
        }
        self.exchanges
            .retain(|e| event.at_unix_ms.saturating_sub(e.at_unix_ms) <= 600_000);
        // Match the four-person scope bound, with twelve exchanges per scope.
        // Unknown identities never establish a reusable private conversation.
        if matches!(event.person, Person::Unsure | Person::Visitor) || event.person_id.is_none() {
            return Ok(());
        }
        let scopes = self
            .exchanges
            .iter()
            .filter_map(|e| e.person_id.as_deref())
            .collect::<std::collections::BTreeSet<_>>();
        if scopes.len() >= 4 && !scopes.contains(event.person_id.as_deref().unwrap_or_default()) {
            let oldest = self.exchanges.front().and_then(|e| e.person_id.clone());
            self.exchanges.retain(|e| e.person_id != oldest);
        }
        self.exchanges.push_back(event.clone());
        if self
            .exchanges
            .iter()
            .filter(|e| e.person_id == event.person_id)
            .count()
            > 12
            && let Some(at) = self
                .exchanges
                .iter()
                .position(|e| e.person_id == event.person_id)
        {
            self.exchanges.remove(at);
        }
        Ok(())
    }
    fn selected(&self, query: &Query) -> Result<Vec<&Event>> {
        query.validate()?;
        let mut selected = self
            .exchanges
            .iter()
            .rev()
            .filter(|e| {
                e.at_unix_ms <= query.at_unix_ms
                    && query.at_unix_ms - e.at_unix_ms <= 600_000
                    && e.person == query.person
                    && e.person_id == query.person_id
                    && e.presence == query.presence
            })
            .take(12)
            .collect::<Vec<_>>();
        selected.reverse();
        Ok(selected)
    }
    pub fn messages(&self, query: &Query) -> Result<Vec<Value>> {
        let mut messages = Vec::new();
        for event in self.selected(query)? {
            messages.push(json!({"role":"user","content":event.heard}));
            if !event.said.is_empty() {
                messages.push(json!({"role":"assistant","content":event.said}));
            }
        }
        Ok(messages)
    }
    /// Same scope/window as ordinary history, with source IDs exposed so the
    /// common answerer can cite an earlier completed exchange honestly.
    pub fn evidence_messages(&self, query: &Query) -> Result<Vec<Value>> {
        let mut messages = Vec::new();
        for event in self.selected(query)? {
            messages.push(json!({"role":"user","content":serde_json::to_string(&json!({"source_id":event.record_id,"at_unix_ms":event.at_unix_ms,"speaker":event.person,"person_id":event.person_id,"presence":event.presence,"heard":event.heard}))?}));
            if !event.said.is_empty() {
                messages.push(json!({"role":"assistant","content":event.said}));
            }
        }
        Ok(messages)
    }
}
