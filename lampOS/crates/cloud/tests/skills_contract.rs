//! `skills.list` as the app's skills page reads it (#1722, mobile's design
//! and the first builder's 17:21): the reply's fields by name and type, in
//! V1's envelope through JSON as the broker carries it, from the real lamp
//! (`Real`) reading lampOS's own skills folder. The page reads them
//! leniently, so a renamed or retyped field would go blank there with
//! nothing failing: it fails here instead. A field removed, renamed or
//! retyped here is `version` 2. `skills.remove` answers with the same
//! reply, the page as it now is, and its refusals in the same envelope.
use lamp_cloud::answer::answer;
use lamp_cloud::lamp::{Places, Real, Ring, Texts, Wifi};
use lamp_cloud::skills;
use lamp_cloud::wire::{Identity, not_here};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
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
        faces: None,
        schedule: None,
    }
}

fn sent(real: &mut Real) -> Value {
    asked(real, "skills.list", &Value::Null)
}

/// `kind` asked with `data`, as the broker carries the answer back.
fn asked(real: &mut Real, kind: &str, data: &Value) -> Value {
    let me = Identity {
        device: "lamp".into(),
        id: "d-42".into(),
        ..Identity::default()
    };
    let result = answer(real, kind, data);
    serde_json::from_str(&me.data("t", kind, &result).to_string()).unwrap()
}

/// `from`'s files and folders copied into `to`.
fn copied(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            copied(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
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
        manage: root.join("m.sock"),
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

#[test]
fn removing_answers_the_page_as_it_now_is_and_refuses_in_the_envelope() {
    // Off: refused as every kind this lamp lacks.
    let off = asked(
        &mut real(None),
        "skills.remove",
        &json!({"name": "desk-tutor"}),
    );
    assert_eq!(
        (&off["type"], &off["kind"], &off["status"]),
        (&json!("data"), &json!("skills.remove"), &json!("failure"))
    );
    assert_eq!(off["error"], "not on this lamp: skills.remove");
    assert_eq!(off["error"], not_here("skills.remove"));
    assert!(off.get("data").is_none());

    // On, over a copy of lampOS's own skills.
    let root = PathBuf::from(format!("/tmp/lcsr-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let folder = root.join("skills");
    copied(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills"),
        &folder,
    );
    // The manage helper on a real socket on a short path, each connection
    // answered by `lamp-skills asked`'s own code, as systemd hands it one.
    let manage = root.join("m.sock");
    let listener = UnixListener::bind(&manage).unwrap();
    let served = folder.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = lamp_skills::package::asked(&served, &stream, &stream);
        }
    });
    let mut real = real(Some(skills::Places {
        folder: folder.clone(),
        off: root.join("off"),
        manage,
    }));
    let before = sent(&mut real)["data"]["skills"].as_array().unwrap().len();

    let core = asked(&mut real, "skills.remove", &json!({"name": "dance"}));
    assert_eq!(
        (&core["kind"], &core["status"], &core["error"]),
        (
            &json!("skills.remove"),
            &json!("failure"),
            &json!("dance is a core skill: it comes with the lamp, so it can't be removed")
        )
    );
    let trick = asked(&mut real, "skills.remove", &json!({"name": "../skills"}));
    assert_eq!(trick["error"], skills::NOT_A_NAME);
    assert!(folder.join("dance").exists());

    let removed = asked(&mut real, "skills.remove", &json!({"name": "desk-tutor"}));
    assert_eq!(
        (&removed["type"], &removed["kind"], &removed["status"]),
        (&json!("data"), &json!("skills.remove"), &json!("success")),
        "{removed}"
    );
    // The same reply as `skills.list`, field for field, without the skill.
    let reply = &removed["data"];
    let mut agreed: Vec<&str> = REPLY.iter().map(|(field, _)| *field).collect();
    agreed.sort();
    let mut got = fields_of(reply);
    got.sort();
    assert_eq!(got, agreed);
    assert_eq!(reply["version"], skills::VERSION);
    assert_eq!(reply, &sent(&mut real)["data"]);
    let rows = reply["skills"].as_array().unwrap();
    assert_eq!(rows.len(), before - 1);
    for row in rows {
        for (field, kinds) in SKILL {
            assert!(kinds.contains(&kind_of(&row[field])), "{field}: {row}");
        }
    }
    assert!(rows.iter().all(|row| row["name"] != "desk-tutor"));
    assert!(!folder.join("desk-tutor").exists());
    let again = asked(&mut real, "skills.remove", &json!({"name": "desk-tutor"}));
    assert_eq!(again["error"], "no skill desk-tutor on this lamp");
    let _ = fs::remove_dir_all(&root);
}
