//! The report as the backend reads it (#1683, agreed on #1683 by the
//! backend): the topic, the fields by name and type for each kind, `id` as
//! `info` carries it, and `sent_at` set once and kept on every send again,
//! since the backend drops a duplicate by `(device, seq, sent_at)`. A field
//! renamed or retyped here is `lamp-report/2`.
use lamp_cloud::reports::{
    ID_MOST, Lamp, MOST_BYTES, Places, RECOVERY_NOT_READY, Reports, Seen, Sources, UNITS, topic,
};
use serde_json::{Value, json};
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
    for field in [
        "level",
        "parts",
        "missing",
        "other_parts",
        "restarts",
        "failed_units",
    ] {
        assert!(health.get(field).is_some(), "{field}");
    }
    assert_eq!(health["failed_units"], json!([]));
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

#[test]
fn failed_units_are_canonical_in_schema_one_and_kept_on_retransmit() {
    let places = places("failed-units");
    let root = places.own.parent().unwrap().to_owned();
    let mut reports = Reports::open(places);
    let mut seen = Seen {
        // Even a forged public Seen cannot add names or duplicates to the wire.
        failed_units: UNITS.iter().rev().copied().collect(),
        restarts: [("lamp-update.service", 0)].into(),
        waits: [("lamp-update.service", RECOVERY_NOT_READY)].into(),
        ..Seen::default()
    };
    seen.failed_units
        .extend(["PRIVATE_FAILED_UNIT", "lamp-update.service"]);
    reports.health(1_792_382_400, None, &seen).unwrap();
    let id = "x".repeat(ID_MOST);
    let first = reports.due(&id);
    assert_eq!(first.len(), 1);
    assert!(first[0].1.len() <= MOST_BYTES);
    assert!(!first[0].1.contains("PRIVATE_FAILED_UNIT"));
    let health: Value = serde_json::from_str(&first[0].1).unwrap();
    let mut expected = UNITS.to_vec();
    expected.sort_unstable();
    assert_eq!(health["failed_units"], json!(expected));
    assert_eq!(health["schema"], "lamp-report/1");
    assert_eq!(
        health["waits"],
        json!({"lamp-update.service": RECOVERY_NOT_READY})
    );
    reports.unhand(None);
    assert_eq!(reports.due(&id), first);
    reports.acked(first[0].0);

    // The next observation has recovered; restart counts and waits have
    // independent meanings and stay exactly as observed.
    seen.failed_units.clear();
    reports.health(1_792_383_300, None, &seen).unwrap();
    let next = reports.due(&id);
    assert_eq!(next.len(), 1);
    let recovered: Value = serde_json::from_str(&next[0].1).unwrap();
    assert_eq!(recovered["failed_units"], json!([]));
    assert_eq!(recovered["restarts"], health["restarts"]);
    assert_eq!(recovered["waits"], health["waits"]);
    let _ = std::fs::remove_dir_all(root);
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
        changes: None,
        photos: None,
        faces: None,
        schedule: None,
        update_page: false,
        update_checks: Default::default(),
        restart: None,
        guard: None,
        make_skill: None,
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

/// A health report from a lamp whose systemd shows `timer` for
/// `lamp-update-back.timer`, with the self-update switch as given (#2709's
/// follow-up): a stand-in `systemctl` prints it.
fn health_with_timer(name: &str, update_self: bool, timer: &str) -> Value {
    use std::os::unix::fs::PermissionsExt;
    let places = places(name);
    let systemctl = places.boot_id.with_file_name("systemctl");
    std::fs::write(&systemctl, format!("#!/bin/sh\nprintf '{timer}'\n")).unwrap();
    std::fs::set_permissions(&systemctl, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut lamp = Lamp::new(None);
    lamp.systemctl = systemctl;
    // The stand-in shell took up to 1.7 s to start on a loaded Mac, near
    // the product's 2 s, so a late query read as none and no `waits`
    // (#1569): give it as long as it needs.
    lamp.systemctl_within = std::time::Duration::from_secs(30);
    lamp.update_self = update_self;
    let seen = Seen {
        waits: lamp.waits(),
        ..Seen::default()
    };
    let mut reports = Reports::open(places);
    reports.health(1_792_382_400, None, &seen).unwrap();
    serde_json::from_str(&reports.due("dev-42")[0].1).unwrap()
}

const NO_TIMER: &str = "LoadState=not-found\\nUnitFileState=\\nActiveState=inactive\\n";
const ARMED: &str = "LoadState=loaded\\nUnitFileState=enabled\\nActiveState=active\\n";

#[test]
fn a_lamp_without_the_armed_way_back_says_lamp_update_waits() {
    let health = health_with_timer("wait-no-timer", true, NO_TIMER);
    assert_eq!(
        health["waits"],
        serde_json::json!({
            "lamp-update.service":
                "independent recovery is not ready: no armed lamp-update-back.timer"
        })
    );
    assert_eq!(health["schema"], "lamp-report/1");
    // Enabled but not started: still not armed.
    let stopped = ARMED.replace("ActiveState=active", "ActiveState=inactive");
    let health = health_with_timer("wait-stopped", true, &stopped);
    assert!(health["waits"].is_object());
}

#[test]
fn a_lamp_with_the_armed_way_back_or_the_switch_off_says_nothing_waits() {
    assert!(
        health_with_timer("wait-armed", true, ARMED)
            .get("waits")
            .is_none()
    );
    assert!(
        health_with_timer("wait-off", false, NO_TIMER)
            .get("waits")
            .is_none()
    );
}
