//! Validate operation order before an adapter can mutate state.
use crate::{Result, invalid, protocol::*};
use std::collections::BTreeSet;

#[derive(Clone, Default)]
pub struct Session {
    initialized: bool,
    closed: bool,
    at: u64,
    probe: Option<(String, u64, BTreeSet<String>)>,
    active_records: BTreeSet<String>,
    version: u32,
    request_ids: BTreeSet<String>,
    // Source identities are never reused, even when their private probe is gone.
    record_ids: BTreeSet<String>,
}
impl Session {
    pub fn has_source(&self, id: &str) -> bool {
        self.active_records.contains(id)
    }
    fn advance(&mut self, at: u64) -> Result<()> {
        if at < self.at {
            return Err(invalid("logical clock moved backward"));
        }
        self.at = at;
        Ok(())
    }
    pub fn prepare(&self, request: &Request) -> Result<Self> {
        request.validate()?;
        if self.closed
            || self.request_ids.contains(&request.id)
            || self.request_ids.len() >= 1_000_000
        {
            return Err(invalid(
                "closed session, duplicate request, or operation limit",
            ));
        }
        if !self.initialized && request.op != Op::Init {
            return Err(invalid("init must be first"));
        }
        if self.initialized && request.v != self.version {
            return Err(invalid("protocol version changed mid-run"));
        }
        let mut next = self.clone();
        match request.op {
            Op::Init => {
                let init: Init = request.decode()?;
                if self.initialized
                    || !valid_id(&init.run_id)
                    || !valid_id(&init.arm)
                    || !valid_id(&init.owner_id)
                    || !std::path::Path::new(&init.state_dir).is_absolute()
                    || !init.config.is_object()
                {
                    return Err(invalid("invalid or repeated initialization"));
                }
                jiff::tz::TimeZone::get(&init.timezone)
                    .map_err(|_| invalid("invalid fixture timezone"))?;
                next.initialized = true;
                next.version = request.v;
                next.advance(init.at_unix_ms)?;
            }
            Op::Ingest => {
                let input: Ingest = request.decode()?;
                input.event.validate()?;
                next.advance(input.event.at_unix_ms)?;
                next.active_records.insert(input.event.record_id.clone());
                if !next.record_ids.insert(input.event.record_id) {
                    return Err(invalid("duplicate source record"));
                }
            }
            Op::Sleep => {
                let input: Sleep = request.decode()?;
                if !["nightly", "weekly"].contains(&input.cadence.as_str()) {
                    return Err(invalid("unknown sleep cadence"));
                }
                next.advance(input.at_unix_ms)?;
            }
            Op::Forget => {
                let input: Forget = request.decode()?;
                if !valid_id(&input.person_id)
                    || input.record_ids.iter().any(|id| !valid_id(id))
                    || input.record_ids.is_empty() && input.text.trim().is_empty()
                {
                    return Err(invalid("invalid forget request"));
                }
                next.advance(input.at_unix_ms)?;
            }
            Op::ProbeBegin => {
                let input: ProbeBegin = request.decode()?;
                if self.probe.is_some() || !valid_id(&input.probe_id) {
                    return Err(invalid("invalid or nested probe"));
                }
                next.advance(input.at_unix_ms)?;
                next.probe = Some((input.probe_id, self.at, self.active_records.clone()));
            }
            Op::ProbeEnd => {
                let input: ProbeEnd = request.decode()?;
                let (id, at, records) = self
                    .probe
                    .as_ref()
                    .ok_or_else(|| invalid("no open probe"))?;
                if &input.probe_id != id {
                    return Err(invalid("mismatched probe end"));
                }
                next.at = *at;
                next.active_records = records.clone();
                next.probe = None;
            }
            Op::Snapshot | Op::Recall => {
                let input: QueryData = request.decode()?;
                input.query.validate()?;
                if self.probe.as_ref().map(|p| p.0.as_str()) != Some(input.query.probe_id.as_str())
                    || request.op == Op::Snapshot && !input.query.heard.is_empty()
                {
                    return Err(invalid(
                        "query outside its probe or nonempty snapshot query",
                    ));
                }
                next.advance(input.query.at_unix_ms)?;
            }
            Op::Tool => {
                let input: Tool = request.decode()?;
                if self.probe.as_ref().map(|p| p.0.as_str()) != Some(input.probe_id.as_str())
                    || !valid_id(&input.name)
                    || !input.arguments.is_object()
                {
                    return Err(invalid("tool outside its probe or invalid arguments"));
                }
            }
            Op::Inspect => {
                let input: Inspect = request.decode()?;
                if !valid_id(&input.person_id) {
                    return Err(invalid("invalid inspection scope"));
                }
            }
            Op::Close => {
                if self.probe.is_some() || request.data.as_object().is_none_or(|d| !d.is_empty()) {
                    return Err(invalid("close requires empty data and no open probe"));
                }
                next.closed = true;
            }
        }
        next.request_ids.insert(request.id.clone());
        Ok(next)
    }
}
