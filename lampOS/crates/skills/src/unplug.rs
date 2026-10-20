//! Readiness is evidence from the body and Expression, never a tool admission.
use lamp_body::unplug::{Phase, Request, State};
use lamp_bus::{Bus, Message};
use serde_json::Value;
use std::io;

pub const SKILL: &str = "prepare-unplug";
pub const TOOL: &str = "prepare_to_unplug";

/// This operation belongs only to the installed core skill, never an alias.
pub fn core_identity(skill: &str, tool: &str) -> bool {
    skill == SKILL && tool == TOOL
}

pub const EXPRESSION_TOPIC: &str = "expression/unplug_state";
pub const RING_TOPIC: &str = "ring/unplug_state";
/// Reads made by the composite operation, including reuse of the kept latch.
pub const READ_TOPICS: [&str; 4] = [Request::TOPIC, State::TOPIC, EXPRESSION_TOPIC, RING_TOPIC];
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
pub fn begin(bus: &Bus, skill: &str, tool: &str, now: u64) -> io::Result<(String, u64)> {
    // Check before reading or writing the latch, including repeated calls.
    if !core_identity(skill, tool) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unplug preparation belongs only to prepare-unplug / prepare_to_unplug",
        ));
    }
    if let Some(message) = bus.latest(Request::TOPIC)? {
        let request = message.parse::<Request>()?;
        if !request.valid() {
            return Err(io::Error::other("invalid kept unplug request"));
        }
        return Ok((request.request, message.header.at_us));
    }
    let mut out = bus.publisher_declared(Request::TOPIC, &Request::declaration(), "skills")?;
    let request = Request {
        request: format!("{}-{now}", out.boot()),
    };
    out.publish_json(now, &request)?;
    Ok((request.request, now))
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
        for (skill, tool) in [("desk-rest", TOOL), (SKILL, "desk_rest")] {
            assert_eq!(
                begin(&bus, skill, tool, 100).unwrap_err().kind(),
                io::ErrorKind::PermissionDenied
            );
            assert!(bus.latest(Request::TOPIC).unwrap().is_none());
        }
        let kept = begin(&bus, SKILL, TOOL, 100).unwrap();
        let original = bus.latest(Request::TOPIC).unwrap().unwrap();
        assert!(begin(&bus, "desk-rest", TOOL, 200).is_err());
        assert_eq!(bus.latest(Request::TOPIC).unwrap().unwrap(), original);
        assert_eq!(begin(&bus, SKILL, TOOL, 200).unwrap(), kept);
    }

    #[test]
    fn repeated_calls_keep_the_original_latch() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Bus::new(dir.path()).unwrap();
        let first = begin(&bus, SKILL, TOOL, 100).unwrap();
        assert_eq!(begin(&bus, SKILL, TOOL, 200).unwrap(), first);
    }
}
