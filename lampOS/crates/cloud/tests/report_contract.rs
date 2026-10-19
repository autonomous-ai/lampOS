//! The report as the backend reads it (#1683, agreed on #1683 by the
//! backend): the topic, the fields by name and type for each kind, `id` as
//! `info` carries it, and `sent_at` set once and kept on every send again,
//! since the backend drops a duplicate by `(device, seq, sent_at)`. A field
//! renamed or retyped here is `lamp-report/2`.
use lamp_cloud::reports::{Places, Reports, Seen, topic};
use serde_json::Value;
use std::path::PathBuf;

fn places(name: &str) -> Places {
    let root = PathBuf::from(format!("/tmp/lcrc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("boot_id"), "b\n").unwrap();
    std::fs::write(
        root.join("setup.json"),
        r#"{"share":true,"asked":1,"at":"2026-10-19T04:00:00Z"}"#,
    )
    .unwrap();
    Places {
        setup: root.join("setup.json"),
        own: root.join("own.json"),
        queue: root.join("reports"),
        exits: root.join("exits"),
        boot_id: root.join("boot_id"),
    }
}

/// `field`'s JSON type, as the backend's reader takes it.
fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

const EVERY: [(&str, &str); 11] = [
    ("type", "string"),
    ("schema", "string"),
    ("device", "string"),
    ("generation", "string"),
    ("id", "string"),
    ("kind", "string"),
    ("seq", "number"),
    ("sent_at", "string"),
    ("uptime_s", "number"),
    ("dropped", "object"),
    ("build", "null"),
];

#[test]
fn every_report_carries_the_agreed_fields_and_sent_at_is_set_once() {
    assert_eq!(topic("fd/dev-42"), "fd/dev-42/report");
    let mut reports = Reports::open(places("fields"));
    reports
        .health(1_792_382_400, None, &Seen::default())
        .unwrap();
    let sent = reports.due("dev-42");
    let health: Value = serde_json::from_str(&sent[0].1).unwrap();
    for (field, kind) in EVERY {
        assert_eq!(kind_of(&health[field]), kind, "{field}");
    }
    assert_eq!(health["type"], "report");
    assert_eq!(health["schema"], "lamp-report/1");
    assert_eq!(health["device"], "lamp");
    assert_eq!(health["generation"], "lamp-v2");
    assert_eq!(health["id"], "dev-42");
    assert_eq!(health["kind"], "health");
    for field in ["level", "parts", "missing", "other_parts", "restarts"] {
        assert!(health.get(field).is_some(), "{field}");
    }
    // Sent again after a drop: the same message, `sent_at` and all, though
    // time has gone on.
    reports.unhand(None);
    assert_eq!(reports.due("dev-42"), sent);
    // The last word: `stopped`, with the same fields and nothing more.
    reports.acked(sent[0].0);
    reports.stop(1_792_382_500, false).unwrap();
    let stopped: Value = serde_json::from_str(&reports.due("dev-42")[0].1).unwrap();
    assert_eq!(stopped["kind"], "stopped");
    let mut fields: Vec<&str> = stopped
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    let mut agreed: Vec<&str> = EVERY.iter().map(|(field, _)| *field).collect();
    agreed.push("clock_synced");
    agreed.sort_unstable();
    assert_eq!(fields, agreed);
}

/// What the app's device-page switch reads (mobile on #1683): `reports.get`
/// and `reports.set` in V1's envelope, `share` a flag, `since` text or null,
/// `queued` a whole number; and without the switch, the refusal the app
/// hides its row on, worded as every kind this lamp lacks.
#[test]
fn the_apps_switch_reads_the_agreed_reply_and_its_refusal() {
    use lamp_cloud::answer::answer;
    use lamp_cloud::lamp::{Places as LampPlaces, Real, Ring, Texts, Wifi};
    use lamp_cloud::reports::Shared;
    use lamp_cloud::wire::{Identity, not_here};
    use serde_json::json;
    let me = Identity {
        device: "lamp".into(),
        id: "d-42".into(),
        ..Identity::default()
    };
    let mut real = Real {
        places: LampPlaces::default(),
        bus: None,
        device_id: "d-42".into(),
        build: String::new(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::default(),
        reports: None,
        skills: None,
    };
    for kind in ["reports.get", "reports.set"] {
        let result = answer(&mut real, kind, &json!({"share": true}));
        let sent: Value = serde_json::from_str(&me.data("t", kind, &result).to_string()).unwrap();
        assert_eq!(sent["status"], "failure");
        assert_eq!(sent["error"], not_here(kind));
        assert_eq!(sent["error"], format!("not on this lamp: {kind}"));
    }
    let mut places = places("switch");
    places.setup = places.own.with_file_name("none.json");
    real.reports = Some(Shared::new(Reports::open(places)));
    for (kind, data, share) in [
        ("reports.get", Value::Null, false),
        ("reports.set", json!({"share": true}), true),
        ("reports.get", Value::Null, true),
        ("reports.set", json!({"share": false}), false),
    ] {
        let result = answer(&mut real, kind, &data);
        let sent: Value = serde_json::from_str(&me.data("t", kind, &result).to_string()).unwrap();
        assert_eq!(
            (&sent["type"], &sent["kind"], &sent["status"]),
            (&json!("data"), &json!(kind), &json!("success")),
            "{sent}"
        );
        let reply = &sent["data"];
        assert_eq!(reply["share"], share, "{kind}: {reply}");
        assert!(reply["since"].is_string() || reply["since"].is_null());
        assert!(reply["queued"].is_u64());
    }
}
