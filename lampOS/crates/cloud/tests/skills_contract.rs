//! `skills.list` as the app's skills page reads it (#1722, mobile's design
//! and the first builder's 17:21): the reply's fields by name and type, in
//! V1's envelope through JSON as the broker carries it, from the real lamp
//! (`Real`) reading lampOS's own skills folder. The page reads them
//! leniently, so a renamed or retyped field would go blank there with
//! nothing failing: it fails here instead. A field removed, renamed or
//! retyped here is `version` 2.
use lamp_cloud::answer::answer;
use lamp_cloud::lamp::{Places, Real, Ring, Texts, Wifi};
use lamp_cloud::skills;
use lamp_cloud::wire::{Identity, not_here};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Instant;

/// `value`'s JSON type, as the page takes it.
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

/// The reply's own fields.
const REPLY: [(&str, &str); 3] = [
    ("version", "number"),
    ("skills", "array"),
    ("left_out", "number"),
];

/// Each skill's fields, and the types each may take.
const SKILL: [(&str, &[&str]); 11] = [
    ("name", &["string"]),
    ("title", &["string"]),
    ("description", &["string"]),
    ("group", &["string"]),
    ("version", &["string", "null"]),
    ("can_remove", &["bool"]),
    ("can_turn_off", &["bool"]),
    ("off", &["bool", "null"]),
    ("loads", &["bool"]),
    ("why", &["string", "null"]),
    ("may_use", &["array"]),
];

/// Each of `may_use`'s entries.
const USE: [(&str, &[&str]); 3] = [
    ("name", &["string"]),
    ("says", &["string"]),
    ("privacy", &["string", "null"]),
];

fn real(skills: Option<skills::Places>) -> Real {
    Real {
        places: Places::default(),
        bus: None,
        device_id: "d-42".into(),
        build: String::new(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::default(),
        reports: None,
        skills,
        changes: None,
        photos: None,
    }
}

fn sent(real: &mut Real) -> Value {
    let me = Identity {
        device: "lamp".into(),
        id: "d-42".into(),
        ..Identity::default()
    };
    let result = answer(real, "skills.list", &Value::Null);
    serde_json::from_str(&me.data("t", "skills.list", &result).to_string()).unwrap()
}

fn fields_of(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}

#[test]
fn the_skills_page_reads_the_agreed_reply_and_its_refusal() {
    // Off: the refusal the page hides its Skills row on.
    let off = sent(&mut real(None));
    assert_eq!(
        (&off["type"], &off["kind"], &off["status"]),
        (&json!("data"), &json!("skills.list"), &json!("failure"))
    );
    assert_eq!(off["error"], "not on this lamp: skills.list");
    assert_eq!(off["error"], not_here("skills.list"));
    assert!(off.get("data").is_none());

    // On, over lampOS's own skills, read in place.
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills");
    let root = PathBuf::from(format!("/tmp/lcsc-{}", std::process::id()));
    let mut real = real(Some(skills::Places {
        folder,
        off: root.join("off"),
    }));
    let started = Instant::now();
    let on = sent(&mut real);
    let took = started.elapsed();
    assert_eq!(
        (&on["type"], &on["kind"], &on["status"]),
        (&json!("data"), &json!("skills.list"), &json!("success")),
        "{on}"
    );
    let reply = &on["data"];
    let mut agreed: Vec<&str> = REPLY.iter().map(|(field, _)| *field).collect();
    agreed.sort();
    let mut got = fields_of(reply);
    got.sort();
    assert_eq!(got, agreed);
    for (field, kind) in REPLY {
        assert_eq!(kind_of(&reply[field]), kind, "{field}");
    }
    assert_eq!(reply["version"], skills::VERSION);
    let rows = reply["skills"].as_array().unwrap();
    assert!(rows.len() >= 6, "{reply}");
    for row in rows {
        let mut agreed: Vec<&str> = SKILL.iter().map(|(field, _)| *field).collect();
        agreed.sort();
        let mut got = fields_of(row);
        got.sort();
        assert_eq!(got, agreed, "{row}");
        for (field, kinds) in SKILL {
            assert!(kinds.contains(&kind_of(&row[field])), "{field}: {row}");
        }
        assert!(
            ["core", "installed", "made_here"].contains(&row["group"].as_str().unwrap()),
            "{row}"
        );
        assert_eq!(row["can_remove"], row["group"] != "core", "{row}");
        for used in row["may_use"].as_array().unwrap() {
            let mut agreed: Vec<&str> = USE.iter().map(|(field, _)| *field).collect();
            agreed.sort();
            let mut got = fields_of(used);
            got.sort();
            assert_eq!(got, agreed, "{used}");
            for (field, kinds) in USE {
                assert!(kinds.contains(&kind_of(&used[field])), "{field}: {used}");
            }
        }
    }
    // The product's core skills, as the page shows them.
    let dance = rows.iter().find(|row| row["name"] == "dance").unwrap();
    assert_eq!(
        (&dance["title"], &dance["group"], &dance["loads"]),
        (&json!("Dance"), &json!("core"), &json!(true))
    );
    let tutor = rows.iter().find(|row| row["name"] == "desk-tutor").unwrap();
    assert_eq!(
        (&tutor["group"], &tutor["can_remove"]),
        (&json!("installed"), &json!(true))
    );
    assert!(
        tutor["may_use"].as_array().unwrap().contains(&json!({
            "name": "camera",
            "says": "uses the camera when it runs",
            "privacy": "the privacy switch stops it",
        })),
        "{tutor}"
    );
    // Core rows first, then the installed ones.
    let groups: Vec<&str> = rows
        .iter()
        .map(|row| row["group"].as_str().unwrap())
        .collect();
    let mut sorted = groups.clone();
    sorted.sort_by_key(|group| {
        ["core", "installed", "made_here"]
            .iter()
            .position(|g| g == group)
    });
    assert_eq!(groups, sorted);
    eprintln!(
        "skills.list over {} skills took {} ms on this machine",
        rows.len(),
        took.as_millis()
    );
}
