//! Readiness is evidence from the body and Expression, never a tool admission.
use lamp_body::unplug::{CANCEL_TOPIC, Phase, Request, State};
use lamp_bus::{Bus, Message};
use serde_json::Value;
use std::io;

// The operation is the `prepare_unplug` core grant's ([`crate::CORE_GRANTS`]),
// never a skill's or a tool's name.

pub const EXPRESSION_TOPIC: &str = "expression/unplug_state";
pub const RING_TOPIC: &str = "ring/unplug_state";
/// Reads made by the composite operation, including reuse of the kept latch
/// and whether it was cancelled.
pub const READ_TOPICS: [&str; 5] = [
    Request::TOPIC,
    State::TOPIC,
    EXPRESSION_TOPIC,
    RING_TOPIC,
    CANCEL_TOPIC,
];
/// The second reserved tool of `prepare-unplug`: "never mind". It is
/// declared whenever the skill is loaded and refused with no latch in force.
pub const STAY_TOOL: &str = "stay_plugged_in";
const STAY_DESCRIPTION: &str = "Call only when, after you began preparing to be unplugged, the person says they will not unplug you after all (for example: never mind, don't unplug, I'm not unplugging you, stay on). The lamp says it will stay, then stands up again by itself. Do not say anything yourself.";
/// Unbroken minutes Ready, with Skills still up, before the lamp gets back up.
pub const STAY_ENVIRONMENT: &str = "LAMP_UNPLUG_STAY_MIN";
const STAY_DEFAULT_MIN: u64 = 10;
pub const WAIT_US: u64 = 32_000_000;
const FRESH_US: u64 = 500_000;

/// A source's current observation, without the bus's expiry grace. All three
/// processes use the same kernel monotonic clock on one lamp.
fn fresh(message: &Message, source: &str, since: u64, now: u64) -> bool {
    !message.expired
        && message.header.source == source
        && message.header.at_us >= since
        && message.header.at_us <= now
        && now - message.header.at_us <= FRESH_US
        && message
            .header
            .expires_ms
            .is_some_and(|ms| ms > 0 && ms <= 500)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Progress {
    Waiting,
    Ready,
    Failed(String),
}

/// Both receipts must be live at the same instant. Neither the initial tool
/// answer, a trajectory fraction, nor an old asleep picture is readiness.
pub fn progress(
    request: &str,
    since: u64,
    now: u64,
    body: Option<&Message>,
    expression: Option<&Message>,
    ring: Option<&Message>,
) -> Progress {
    let Some(body) = body.filter(|message| fresh(message, "body", since, now)) else {
        return Progress::Waiting;
    };
    let Ok(state) = body.parse::<State>() else {
        return Progress::Waiting;
    };
    if state.request != request {
        return Progress::Waiting;
    }
    match state.phase {
        Phase::Failed => Progress::Failed(state.why),
        Phase::Folding => Progress::Waiting,
        Phase::Ready => {
            let asleep = [(expression, "expression"), (ring, "ring")]
                .into_iter()
                .all(|(message, source)| {
                    message
                        .filter(|message| fresh(message, source, since, now))
                        .and_then(|message| message.parse::<Value>().ok())
                        .is_some_and(|value| value["request"] == request && value["asleep"] == true)
                });
            if asleep {
                Progress::Ready
            } else {
                Progress::Waiting
            }
        }
    }
}

/// Do not replace an existing latch, including one from before a skills restart.
/// A repeated request observes that preparation instead of inventing a new one.
pub fn begin(bus: &Bus, may: &crate::Permissions, now: u64) -> io::Result<(String, u64)> {
    // Check before reading or writing the latch, including repeated calls.
    if let Some(why) = may.refuses(&crate::Call::PrepareUnplug) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, why));
    }
    if let Some(message) = bus.latest(Request::TOPIC)? {
        let request = message.parse::<Request>()?;
        if !request.valid() {
            return Err(io::Error::other("invalid kept unplug request"));
        }
        // A cancelled request is over: the next unplug gets a fresh one.
        if !lamp_body::unplug::cancelled(bus, &request) {
            return Ok((request.request, message.header.at_us));
        }
    }
    let mut out = bus.publisher_declared(Request::TOPIC, &Request::declaration(), "skills")?;
    let request = Request {
        request: format!("{}-{now}", out.boot()),
    };
    out.publish_json(now, &request)?;
    Ok((request.request, now))
}

/// The "never mind" tool, declared whenever the unplug skill is loaded.
pub fn stay_tool() -> Value {
    serde_json::json!({"name": STAY_TOOL, "description": STAY_DESCRIPTION})
}

/// The request a cancel may name now, or why not. Skills checks what the body
/// checks again itself: a live latch, and the body's own fresh, matching state
/// in Folding (spoken only) or Ready, never Failed.
pub fn may_stay(bus: &Bus, now: u64, ready_only: bool) -> Result<String, String> {
    let Ok(Some(request)) = lamp_body::unplug::in_force(bus, Some("skills")) else {
        return Err("There is no unplug preparation to cancel.".into());
    };
    let state = bus
        .latest(State::TOPIC)
        .ok()
        .flatten()
        .filter(|message| fresh(message, "body", 0, now))
        .and_then(|message| message.parse::<State>().ok())
        .filter(|state| state.request == request.request);
    match state.map(|state| state.phase) {
        Some(Phase::Ready) => Ok(request.request),
        Some(Phase::Folding) if !ready_only => Ok(request.request),
        Some(Phase::Failed) => {
            Err("The fold failed: it stays down until it is inspected. Keep it plugged in.".into())
        }
        _ => Err("The body has not confirmed where it is; it stays down for now.".into()),
    }
}

/// Publishes the kept cancel the body checks against its accepted request.
pub fn cancel(bus: &Bus, request: &str, now: u64) -> io::Result<()> {
    bus.publisher_declared(CANCEL_TOPIC, &Request::cancel_declaration(), "skills")?
        .publish_json(
            now,
            &Request {
                request: request.to_owned(),
            },
        )?;
    Ok(())
}

/// `LAMP_UNPLUG_STAY_MIN`, whole minutes over zero; 10 otherwise.
pub fn stay_minutes(value: Option<&str>) -> u64 {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|minutes| *minutes > 0)
        .unwrap_or(STAY_DEFAULT_MIN)
}

pub fn read(bus: &Bus, request: &str, since: u64, now: u64) -> io::Result<Progress> {
    let body = bus.latest(State::TOPIC)?;
    let expression = bus.latest(EXPRESSION_TOPIC)?;
    let ring = bus.latest(RING_TOPIC)?;
    Ok(progress(
        request,
        since,
        now,
        body.as_ref(),
        expression.as_ref(),
        ring.as_ref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_bus::{Header, Kind};
    use serde_json::json;

    fn message(source: &str, payload: Value, at: u64) -> Message {
        Message {
            header: Header {
                topic: String::new(),
                kind: Kind::Latest,
                enc: "json".into(),
                type_name: None,
                source: source.into(),
                boot: "test".into(),
                seq: 1,
                at_us: at,
                at_unix_ms: 1,
                expires_ms: Some(500),
                clock: None,
            },
            payload: serde_json::to_vec(&payload).unwrap(),
            expired: false,
            gap: 0,
            publisher_restarted: false,
        }
    }

    fn receipts() -> [Message; 3] {
        [
            message(
                "body",
                json!({"request":"r", "phase":"ready", "why":""}),
                1_000_000,
            ),
            message(
                "expression",
                json!({"request":"r", "asleep":true}),
                1_000_000,
            ),
            message("ring", json!({"request":"r", "asleep":true}), 1_000_000),
        ]
    }

    #[test]
    fn readiness_requires_all_three_matching_fresh_receipts() {
        let good = receipts();
        let judge = |r: &[Message; 3]| {
            progress(
                "r",
                900_000,
                1_100_000,
                Some(&r[0]),
                Some(&r[1]),
                Some(&r[2]),
            )
        };
        assert_eq!(judge(&good), Progress::Ready);
        assert_eq!(
            progress(
                "r",
                900_000,
                1_100_000,
                Some(&good[0]),
                Some(&good[1]),
                None
            ),
            Progress::Waiting
        );
        for index in 0..3 {
            for change in 0..6 {
                let mut bad = good.clone();
                match change {
                    0 => bad[index].expired = true,
                    1 => bad[index].header.at_us = 500_000,
                    2 => bad[index].header.at_us = 1_200_000,
                    3 => bad[index].header.source = "another".into(),
                    4 => bad[index].header.expires_ms = None,
                    _ => {
                        let mut value = bad[index].json().unwrap();
                        value["request"] = json!("other");
                        bad[index].payload = serde_json::to_vec(&value).unwrap();
                    }
                }
                assert_eq!(
                    judge(&bad),
                    Progress::Waiting,
                    "receipt {index}, mutation {change}"
                );
            }
        }
    }

    #[test]
    fn failure_wins_over_still_live_asleep_receipts() {
        let mut messages = receipts();
        messages[0] = message(
            "body",
            json!({"request":"r", "phase":"failed", "why":"blocked"}),
            1_000_000,
        );
        assert_eq!(
            progress(
                "r",
                900_000,
                1_100_000,
                Some(&messages[0]),
                Some(&messages[1]),
                Some(&messages[2])
            ),
            Progress::Failed("blocked".into())
        );
    }

    #[test]
    fn ordinary_skill_or_tool_cannot_publish_or_borrow_an_unplug_request() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Bus::new(dir.path()).unwrap();
        let granted = granted();
        let mut ungranted = granted.clone();
        ungranted.core.clear();
        assert_eq!(
            begin(&bus, &ungranted, 100).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(bus.latest(Request::TOPIC).unwrap().is_none());
        let kept = begin(&bus, &granted, 100).unwrap();
        let original = bus.latest(Request::TOPIC).unwrap().unwrap();
        assert!(begin(&bus, &ungranted, 200).is_err());
        assert_eq!(bus.latest(Request::TOPIC).unwrap().unwrap(), original);
        assert_eq!(begin(&bus, &granted, 200).unwrap(), kept);
    }

    /// What the shipped skill declares: the grant, body, speech and reads.
    fn granted() -> crate::Permissions {
        crate::Permissions {
            body: true,
            speak: true,
            topics: READ_TOPICS
                .iter()
                .map(|topic| (*topic).to_owned())
                .collect(),
            core: vec!["prepare_unplug".into()],
            ..crate::Permissions::default()
        }
    }

    fn publish(bus: &Bus, topic: &str, source: &str, value: Value) {
        bus.publisher_declared(topic, &Request::declaration(), source)
            .unwrap()
            .publish_json(1, &value)
            .unwrap();
    }

    /// The reviewer on #2069 (P3 1): declared always, so a Live session has
    /// it; with no latch in force the call is refused.
    #[test]
    fn never_mind_is_refused_unless_a_latch_is_in_force() {
        let no = "There is no unplug preparation to cancel.";
        let dir = tempfile::tempdir().unwrap();
        let bus = Bus::new(dir.path()).unwrap();
        assert_eq!(stay_tool()["name"], STAY_TOOL);
        assert_eq!(may_stay(&bus, 0, false).unwrap_err(), no, "no latch");
        let (request, _) = begin(&bus, &granted(), 100).unwrap();
        assert_ne!(may_stay(&bus, 0, false).unwrap_err(), no, "latched");
        cancel(&bus, &request, 200).unwrap();
        assert_eq!(may_stay(&bus, 0, false).unwrap_err(), no, "cancelled");
        let (next, _) = begin(&bus, &granted(), 300).unwrap();
        assert_ne!(next, request, "the next unplug gets a fresh request");
        let foreign = tempfile::tempdir().unwrap();
        let foreign = Bus::new(foreign.path()).unwrap();
        publish(
            &foreign,
            Request::TOPIC,
            "expression",
            json!({"request": "r1"}),
        );
        assert_eq!(
            may_stay(&foreign, 0, false).unwrap_err(),
            no,
            "not a request Skills made"
        );
    }

    #[test]
    fn a_cancel_needs_the_bodys_fresh_matching_folding_or_ready() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Bus::new(dir.path()).unwrap();
        let now = 10_000_000;
        assert!(may_stay(&bus, now, false).is_err(), "no request");
        publish(&bus, Request::TOPIC, "skills", json!({"request": "r1"}));
        let state = |phase: &str, request: &str, at: u64| {
            let mut out = bus
                .publisher_declared(State::TOPIC, &State::declaration(), "body")
                .unwrap();
            out.publish_json(at, &json!({"request": request, "phase": phase, "why": ""}))
                .unwrap();
        };
        assert!(may_stay(&bus, now, false).is_err(), "no body state");
        state("ready", "r1", now - 600_000);
        assert!(may_stay(&bus, now, false).is_err(), "stale");
        state("ready", "r0", now);
        assert!(may_stay(&bus, now, false).is_err(), "other request");
        state("failed", "r1", now);
        assert!(may_stay(&bus, now, false).is_err(), "failed");
        state("folding", "r1", now);
        assert_eq!(may_stay(&bus, now, false).unwrap(), "r1");
        assert!(
            may_stay(&bus, now, true).is_err(),
            "the timeout waits for Ready"
        );
        state("ready", "r1", now);
        assert_eq!(may_stay(&bus, now, true).unwrap(), "r1");
    }

    #[test]
    fn stay_minutes_default_to_ten() {
        assert_eq!(stay_minutes(None), 10);
        assert_eq!(stay_minutes(Some("0")), 10);
        assert_eq!(stay_minutes(Some("ten")), 10);
        assert_eq!(stay_minutes(Some("1")), 1);
    }

    #[test]
    fn repeated_calls_keep_the_original_latch() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Bus::new(dir.path()).unwrap();
        let first = begin(&bus, &granted(), 100).unwrap();
        assert_eq!(begin(&bus, &granted(), 200).unwrap(), first);
    }
}
