//! Every event the door sends from the bus validates against
//! docs/control.schema.json's `event`, which the SDKs ship and parse against
//! (the Reviewer-lead's P2 on #3868: three events didn't). As `doc_examples`
//! does for requests.
//!
//! The schema is checked by a small validator for the keywords `event` uses
//! (`type`, `properties`, `required`, `allOf` with `if`/`then`, `anyOf`,
//! `const`, `enum`, `minimum`, `maximum`, `$ref`, `format: date-time`); any
//! other keyword it meets on a value fails the test instead of passing it.

use lamp_control::protocol::EventKind;
use lamp_control::server::rfc3339_utc;
use lamp_control::watch::{Out, Watch};
use serde_json::{Value, json};

const S: u64 = 1_000_000;

fn schema() -> Value {
    serde_json::from_str(include_str!("../../../docs/control.schema.json")).unwrap()
}

fn resolve<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    match schema["$ref"].as_str() {
        Some(path) => {
            let name = path.strip_prefix("#/$defs/").expect("a local $ref");
            &root["$defs"][name]
        }
        None => schema,
    }
}

fn type_holds(kind: &str, value: &Value) -> bool {
    match kind {
        "object" => value.is_object(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        other => panic!("type {other} not known to this validator"),
    }
}

fn date_time(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() >= 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && (text.ends_with('Z') || text[19..].contains('+') || text[19..].contains('-'))
}

/// Whether `value` holds against `schema`; `why` names the first miss.
fn holds(root: &Value, schema: &Value, value: &Value, at: &str) -> Result<(), String> {
    let schema = resolve(root, schema);
    let Some(rules) = schema.as_object() else {
        return Ok(());
    };
    for (keyword, rule) in rules {
        match keyword.as_str() {
            "description" | "$ref" => {}
            "type" => {
                let ok = match rule {
                    Value::String(kind) => type_holds(kind, value),
                    Value::Array(kinds) => kinds
                        .iter()
                        .any(|kind| type_holds(kind.as_str().unwrap(), value)),
                    _ => panic!("type {rule}"),
                };
                if !ok {
                    return Err(format!("{at}: {value} is not {rule}"));
                }
            }
            "const" => {
                if value != rule {
                    return Err(format!("{at}: {value} is not {rule}"));
                }
            }
            "enum" => {
                if !rule.as_array().unwrap().contains(value) {
                    return Err(format!("{at}: {value} is not one of {rule}"));
                }
            }
            "minimum" => {
                if value.as_f64().is_some_and(|v| v < rule.as_f64().unwrap()) {
                    return Err(format!("{at}: {value} < {rule}"));
                }
            }
            "maximum" => {
                if value.as_f64().is_some_and(|v| v > rule.as_f64().unwrap()) {
                    return Err(format!("{at}: {value} > {rule}"));
                }
            }
            "format" => {
                assert_eq!(rule, "date-time", "format {rule}");
                if value.as_str().is_some_and(|text| !date_time(text)) {
                    return Err(format!("{at}: {value} is not a date-time"));
                }
            }
            "required" => {
                for name in rule.as_array().unwrap() {
                    if value.get(name.as_str().unwrap()).is_none() {
                        return Err(format!("{at}: {name} is required"));
                    }
                }
            }
            "properties" => {
                for (name, sub) in rule.as_object().unwrap() {
                    if let Some(field) = value.get(name) {
                        holds(root, sub, field, &format!("{at}.{name}"))?;
                    }
                }
            }
            "anyOf" => {
                if !rule
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|sub| holds(root, sub, value, at).is_ok())
                {
                    return Err(format!("{at}: {value} matches none of anyOf"));
                }
            }
            "allOf" => {
                for sub in rule.as_array().unwrap() {
                    holds(root, sub, value, at)?;
                }
            }
            "if" => {
                if holds(root, rule, value, at).is_ok()
                    && let Some(then) = schema.get("then")
                {
                    holds(root, then, value, at)?;
                }
            }
            "then" => {}
            other => panic!("{at}: keyword {other} not known to this validator"),
        }
    }
    Ok(())
}

/// The event as the server sends it (`Shared::deliver`): its fields, `ev`
/// and `at`.
fn as_sent(kind: EventKind, fields: Value) -> Value {
    let mut event = fields;
    event["ev"] = serde_json::to_value(kind).unwrap();
    event["at"] = Value::String(rfc3339_utc(std::time::SystemTime::now()));
    event
}

#[test]
fn every_event_from_the_bus_validates_against_the_schema() {
    let root = schema();
    let mut watch = Watch::new();
    let face = json!({"ymin": 0.2, "ymax": 0.6, "facing": true});
    let messages = [
        ("humans/faces/tracked", json!({"faces": [face]}), 10 * S),
        ("humans/faces/tracked", json!({"faces": []}), 11 * S),
        ("humans/faces/tracked", json!({"faces": []}), 17 * S),
        ("room/stood_up", json!({"side": "left"}), 18 * S),
        ("room/stood_up", json!({}), 18 * S),
        ("name/heard", json!({}), 19 * S),
        (
            "ears/classes",
            json!({"scores": {"music": 0.9, "door": 0.7}}),
            20 * S,
        ),
        ("controls/gesture", json!({"gesture": "tap"}), 21 * S),
        ("controls/gesture", json!({"gesture": "double_tap"}), 21 * S),
        ("controls/gesture", json!({"gesture": "multi_tap"}), 21 * S),
        ("controls/gesture", json!({"gesture": "pet"}), 21 * S),
        ("controls/gesture", json!({"gesture": "long_press"}), 21 * S),
        ("voice/state", json!({"state": "listening"}), 22 * S),
        ("voice/state", json!({"state": "resting"}), 23 * S),
        ("lamp/pause", json!({"paused": true}), 24 * S),
        (
            "voice/mute",
            json!({"muted": true, "reason": "sleep"}),
            25 * S,
        ),
        ("voice/privacy", json!({"private": true}), 26 * S),
        (
            "humans/hands/tracked",
            json!({"x": 0.42, "y": 0.61, "size": 0.18, "open": true, "confidence": 0.93, "t": 1}),
            27 * S,
        ),
    ];
    let mut kinds = std::collections::BTreeSet::new();
    let mut outs: Vec<(String, Value, Out)> = Vec::new();
    for (topic, payload, at) in messages {
        for out in watch.take(topic, &payload, at) {
            outs.push((topic.to_owned(), payload.clone(), out));
        }
    }
    // The hand gone: by time passing, with no message.
    for out in watch.lapse(29 * S) {
        outs.push(("(lapse)".to_owned(), Value::Null, out));
    }
    let mut hands = 0;
    for (topic, payload, out) in outs {
        {
            if let Out::Event(kind, fields) = out {
                let event = as_sent(kind, fields);
                let checked = holds(&root, &root["$defs"]["event"], &event, "event");
                assert!(checked.is_ok(), "{topic} {payload}: {event}: {checked:?}");
                if event["ev"] == "person.arrived" {
                    // Rounded to two places, never 0.39999999999999997.
                    assert_eq!(event["near"], json!(0.4), "{event}");
                }
                if event["ev"] == "hand" {
                    hands += 1;
                }
                kinds.insert(event["ev"].as_str().unwrap().to_owned());
            }
        }
    }
    assert_eq!(hands, 2, "seen and gone");
    // Every kind the bus gives was seen and checked.
    for kind in [
        "person.arrived",
        "person.left",
        "person.facing",
        "person.stood_up",
        "name.heard",
        "sound.heard",
        "touch",
        "button",
        "lamp.changed",
        "hand",
    ] {
        assert!(kinds.contains(kind), "{kind} not seen: {kinds:?}");
    }
}

#[test]
fn the_validator_catches_what_the_reviewer_lead_found() {
    let root = schema();
    let event = &root["$defs"]["event"];
    let check = |value: Value| holds(&root, event, &value, "event");
    assert!(check(json!({"ev": "person.left"})).is_err());
    assert!(check(json!({"ev": "touch", "kind": "double_tap"})).is_err());
    assert!(check(json!({"ev": "lamp.changed", "talking": true})).is_err());
    assert!(check(json!({"ev": "person.left", "count": 0})).is_ok());
    // A hand seen says where; gone says only that.
    assert!(check(json!({"ev": "hand", "seen": true, "open": true})).is_err());
    assert!(check(json!({"ev": "hand"})).is_err());
    assert!(check(json!({"ev": "hand", "seen": false})).is_ok());
}
