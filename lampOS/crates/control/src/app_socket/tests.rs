use super::*;
use std::path::PathBuf;
use std::time::Duration;

fn frame(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lampos-appsock-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn install(apps: &Path, name: &str, grants: Value, settings: Value) {
    let dir = apps.join(name).join("current");
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = json!({
        "format": 2, "kind": "app", "name": name, "title": "T", "version": "2.0.1",
        "min_build": 1, "runtime": {"name": "python", "entry": "src/main.py"},
        "grants": grants, "triggers": [], "settings": settings, "files": []
    });
    std::fs::write(dir.join("lamp-skill.json"), manifest.to_string()).unwrap();
}

#[test]
fn the_app_is_the_unit_its_cgroup_names_and_nothing_else() {
    let line = |path: &str| format!("0::{path}\n");
    let unit = "/lampos.slice/lampos-apps.slice/lampapp@english-lesson.service";
    assert_eq!(
        app_of_cgroup(&line(unit)).as_deref(),
        Some("english-lesson")
    );
    for other in [
        "/lampos.slice/lampos-apps.slice/lampapp@english-lesson.service/sub",
        "/lampos.slice/lampos-apps.slice/lampapp@Bad.service",
        "/lampos.slice/lampos-apps.slice/lampapp@.service",
        "/lampos.slice/lampos-parts.slice/lampapp@demo.service",
        "/user.slice/lampapp@demo.service",
        "/lampos.slice/lampos-apps.slice/lamp-apps-manage@demo.service",
    ] {
        assert_eq!(app_of_cgroup(&line(other)), None, "{other}");
    }
    assert_eq!(app_of(None), None);
    // The id is the name's, never a pid's.
    assert_eq!(app_client("demo"), app_client("demo"));
    assert_ne!(app_client("demo"), app_client("demo2"));
    assert!(app_client("demo").starts_with("c_") && app_client("demo").len() == 18);
}

#[test]
fn a_peer_is_the_installed_manifests_and_a_service_grant_is_refused() {
    let apps = folder("peer");
    install(&apps, "demo", json!(["speak", "light"]), json!([]));
    let peer = peer_for(&apps, "demo").unwrap();
    assert_eq!(peer.version, "2.0.1");
    assert_eq!(peer.grants.len(), 2);
    assert!(peer_for(&apps, "ghost").is_err());
    install(&apps, "odd", json!(["gmail"]), json!([]));
    assert!(peer_for(&apps, "odd").is_err());
    let _ = std::fs::remove_dir_all(&apps);
}

#[test]
fn the_app_ops_are_answered_checked_and_off_means_off() {
    let apps = folder("ops");
    let state = folder("ops-state");
    install(
        &apps,
        "demo",
        json!(["speak"]),
        json!([{"key": "loud", "type": "number", "label": "L", "default": 3, "min": 1, "max": 10}]),
    );
    let place = Place {
        apps: &apps,
        state: Some(&state),
    };
    let mut live = Live::default();
    let t0 = Instant::now();
    let ask = |live: &mut Live, value: Value, on: bool, now: Instant| {
        answer(live, &place, "demo", &frame(value), on, now).unwrap()
    };
    assert!(is_app_op(&frame(json!({"id": 1, "op": "app.took"}))));
    assert!(!is_app_op(&frame(json!({"id": 1, "op": "gesture"}))));
    let off = ask(
        &mut live,
        json!({"id": 1, "op": "app.settings.get"}),
        false,
        t0,
    );
    assert_eq!(off["error"]["code"], "off");
    assert_eq!(off["error"]["why"], OFF);
    let got = ask(
        &mut live,
        json!({"id": 2, "op": "app.settings.get"}),
        true,
        t0,
    );
    assert_eq!(got["settings"], json!({"loud": 3}));
    assert_eq!(got["re"], 2);
    let extra = ask(
        &mut live,
        json!({"id": 3, "op": "app.settings.get", "x": 1}),
        true,
        t0,
    );
    assert_eq!(extra["error"]["code"], "bad_request");
    assert_eq!(
        ask(
            &mut live,
            json!({"id": 4, "op": "app.took", "turn": "t-1"}),
            true,
            t0
        )["ok"],
        true
    );
    for bad in [
        json!({"id": 5, "op": "app.took"}),
        json!({"id": 5, "op": "app.took", "turn": ""}),
        json!({"id": 5, "op": "app.took", "turn": "x".repeat(65)}),
    ] {
        assert_eq!(
            ask(&mut live, bad, true, t0)["error"]["code"],
            "bad_request"
        );
    }
    // A panel: bounds, one a minute with the wait said.
    let panel = |title: &str, lines: Value| json!({"id": 6, "op": "app.panel.set", "title": title, "lines": lines});
    assert_eq!(
        ask(&mut live, panel("Lesson", json!(["one", "two"])), true, t0)["rev"],
        1
    );
    let again = ask(
        &mut live,
        panel("Lesson", json!(["x"])),
        true,
        t0 + Duration::from_secs(10),
    );
    assert_eq!(again["error"]["code"], "rate");
    assert!(again["error"]["retry_ms"].as_u64().unwrap() <= 50_000);
    let later = t0 + Duration::from_secs(61);
    for bad in [
        panel("", json!([])),
        panel(&"t".repeat(41), json!([])),
        panel("T", json!(["a", "b", "c", "d", "e", "f", "g"])),
        panel("T", json!([""])),
        panel("T", json!(["a\nb"])),
        panel("T", json!(["x".repeat(81)])),
        panel("T", json!([1])),
    ] {
        assert_eq!(
            ask(&mut live, bad.clone(), true, later)["error"]["code"],
            "bad_request",
            "{bad}"
        );
    }
    assert_eq!(
        ask(
            &mut live,
            panel("Lesson", json!(["x".repeat(80)])),
            true,
            later
        )["rev"],
        2
    );
    // The panel goes with the app's last connection.
    live.connection("demo", true);
    live.connection("demo", false);
    assert_eq!(
        ask(&mut live, panel("Lesson", json!([])), true, later)["rev"],
        1
    );
    let _ = std::fs::remove_dir_all(&apps);
    let _ = std::fs::remove_dir_all(&state);
}

#[test]
fn a_settings_event_says_what_changed_and_the_whole_map() {
    let mut settings = Map::new();
    settings.insert("loud".into(), json!(4));
    let event = settings_event(&["loud".to_owned()], &settings);
    assert_eq!(
        event,
        json!({"ev": "app.settings", "changed": ["loud"], "settings": {"loud": 4}})
    );
}
