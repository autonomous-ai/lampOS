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
        app_of_cgroup(&line(unit)),
        Some(("english-lesson".to_owned(), false))
    );
    // An app with `net: true` runs in its own unit, with the same exactness.
    assert_eq!(
        app_of_cgroup(&line(
            "/lampos.slice/lampos-apps.slice/lampapp-net@english-lesson.service"
        )),
        Some(("english-lesson".to_owned(), true))
    );
    for other in [
        "/lampos.slice/lampos-apps.slice/lampapp@english-lesson.service/sub",
        "/lampos.slice/lampos-apps.slice/lampapp@Bad.service",
        "/lampos.slice/lampos-apps.slice/lampapp-net@english-lesson.service/sub",
        "/lampos.slice/lampos-apps.slice/lampapp-net@.service",
        "/lampos.slice/lampos-parts.slice/lampapp-net@demo.service",
        "/lampos.slice/lampos-apps.slice/lampapp-netx@demo.service",
        "/lampos.slice/lampos-apps.slice/lampapp@.service",
        "/lampos.slice/lampos-parts.slice/lampapp@demo.service",
        "/user.slice/lampapp@demo.service",
        "/lampos.slice/lampos-apps.slice/lamp-apps-manage@demo.service",
    ] {
        assert_eq!(app_of_cgroup(&line(other)), None, "{other}");
    }
    assert_eq!(app_of(None, 0), None);
    // The id is the name's, never a pid's.
    assert_eq!(app_client("demo"), app_client("demo"));
    assert_ne!(app_client("demo"), app_client("demo2"));
    assert!(app_client("demo").starts_with("c_") && app_client("demo").len() == 18);
}

/// Reads this process's own `/proc` entry, so on Linux only (macOS has none).
#[cfg(target_os = "linux")]
#[test]
fn the_handle_read_of_proc_gives_a_cgroup_for_its_own_uid_and_nothing_for_anothers() {
    // A pid whose process is not the uid the kernel named is no peer.
    assert_eq!(app_of(Some(std::process::id() as i32), u32::MAX), None);
    // The handle read gives this process's own cgroup for its own uid and
    // nothing for another's: the uid check bites, and the `openat` path works.
    let me = std::process::id() as i32;
    let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata("/proc/self").unwrap());
    let text = cgroup_text(Some(me), Some(uid)).expect("its own cgroup");
    assert!(text.contains(':'), "{text}");
    assert_eq!(cgroup_text(Some(me), Some(uid.wrapping_add(1))), None);
    assert_eq!(cgroup_text(Some(me), None), Some(text));
    assert_eq!(cgroup_text(Some(0), None), None);
}

#[test]
fn a_peer_is_the_installed_manifests_and_a_service_grant_is_refused() {
    let apps = folder("peer");
    install(&apps, "demo", json!(["speak", "light"]), json!([]));
    let peer = peer_for(&apps, "demo", false).unwrap();
    assert_eq!(peer.version, "2.0.1");
    assert_eq!(peer.grants.len(), 2);
    assert!(peer_for(&apps, "ghost", false).is_err());
    // The unit must be the manifest's: the net unit for a plain app is refused.
    assert!(peer_for(&apps, "demo", true).is_err());
    install(&apps, "odd", json!(["gmail"]), json!([]));
    assert!(peer_for(&apps, "odd", false).is_err());
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
    // Ten reads a second, not more.
    let again = ask(
        &mut live,
        json!({"id": 20, "op": "app.settings.get"}),
        true,
        t0 + Duration::from_millis(10),
    );
    assert_eq!(again["error"]["code"], "rate", "{again}");
    assert!(again["error"]["retry_ms"].as_u64().unwrap() <= 100);
    let after = ask(
        &mut live,
        json!({"id": 21, "op": "app.settings.get"}),
        true,
        t0 + Duration::from_millis(200),
    );
    assert_eq!(after["settings"], json!({"loud": 3}), "{after}");
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
    // The rev goes on and so does the minute: a minute on it is free.
    assert_eq!(
        ask(&mut live, panel("Lesson", json!([])), true, later)["error"]["code"],
        "rate"
    );
    assert_eq!(
        ask(
            &mut live,
            panel("Lesson", json!([])),
            true,
            later + Duration::from_secs(61)
        )["rev"],
        3
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

#[test]
fn phrases_set_is_the_doors_subscribe_and_only_the_reply_is_the_apps() {
    let set =
        json!({"id": 4, "op": "phrases.set", "phrases": ["start the lesson", "begin the lesson"]});
    let PhrasesSet::Door(door, count) = phrases_set_as_subscribe(&set.to_string()) else {
        panic!("a phrases.set");
    };
    assert_eq!(count, 2);
    let door: Value = serde_json::from_str(&door).unwrap();
    assert_eq!(door["op"], "subscribe");
    assert_eq!(door["events"], json!(["phrase.heard"]));
    assert_eq!(door["id"], 4);
    assert_eq!(door["phrases"].as_array().unwrap().len(), 2);
    let PhrasesSet::Door(door, count) =
        phrases_set_as_subscribe(&json!({"id": 5, "op": "phrases.set", "phrases": []}).to_string())
    else {
        panic!("an empty phrases.set");
    };
    assert_eq!(count, 0);
    assert!(door.contains("\"unsubscribe\""));
    // Another op, no id, or no JSON: the door's own to parse.
    for text in [
        json!({"id": 1, "op": "gesture", "name": "nod"}).to_string(),
        json!({"op": "phrases.set", "phrases": []}).to_string(),
        "not json".to_owned(),
    ] {
        assert_eq!(
            phrases_set_as_subscribe(&text),
            PhrasesSet::NotOurs,
            "{text}"
        );
    }
    // Refused here, in its own words, never the door's parser's.
    for (text, why) in [
        (
            json!({"id": 1, "op": "phrases.set", "phrases": ["a"], "x": 1}),
            "phrases.set takes only phrases",
        ),
        (
            json!({"id": 1, "op": "phrases.set", "phrases": [1]}),
            "phrases: a list of text",
        ),
        (
            json!({"id": 1, "op": "phrases.set"}),
            "phrases: a list of text",
        ),
        (
            json!({"id": 1, "op": "phrases.set", "phrases": ["lesson time", "lesson time"]}),
            "phrases: no phrase twice",
        ),
    ] {
        let PhrasesSet::Refused(reply) = phrases_set_as_subscribe(&text.to_string()) else {
            panic!("{text} was not refused");
        };
        assert_eq!(reply["error"]["code"], "bad_request", "{reply}");
        assert_eq!(reply["error"]["why"], why, "{reply}");
        assert_eq!(reply["re"], 1);
    }
    let mut ok = json!({"re": 4, "ok": true, "events": ["phrase.heard"]});
    phrases_reply(&mut ok, 2);
    assert_eq!(ok, json!({"re": 4, "ok": true, "phrases": 2}));
    let mut no = json!({"re": 4, "ok": false, "error": {"code": "bad_request", "why": "x"}});
    let before = no.clone();
    phrases_reply(&mut no, 2);
    assert_eq!(no, before);
}

#[test]
fn the_panel_rate_survives_a_reconnect_and_only_a_remove_forgets_it() {
    let apps = folder("reconnect");
    install(&apps, "demo", json!(["speak"]), json!([]));
    let place = Place {
        apps: &apps,
        state: None,
    };
    let mut live = Live::default();
    let t0 = Instant::now();
    let panel = |title: &str| json!({"id": 6, "op": "app.panel.set", "title": title, "lines": []});
    let ask = |live: &mut Live, value: Value, at: Instant| {
        answer(live, &place, "demo", &frame(value), true, at).unwrap()
    };
    assert_eq!(ask(&mut live, panel("One"), t0)["rev"], 1);
    // The app's last connection closes and it reconnects: still one a minute.
    live.connection("demo", true);
    live.connection("demo", false);
    let again = ask(&mut live, panel("Two"), t0 + Duration::from_secs(5));
    assert_eq!(again["error"]["code"], "rate", "{again}");
    // A remove forgets all of it.
    live.forget_app("demo");
    assert_eq!(
        ask(&mut live, panel("Three"), t0 + Duration::from_secs(6))["rev"],
        1
    );
    let _ = std::fs::remove_dir_all(&apps);
}

#[test]
fn took_is_ten_a_second_and_a_panel_cannot_carry_a_line_break_or_a_bidi_mark() {
    let apps = folder("took");
    install(&apps, "demo", json!(["speak"]), json!([]));
    let place = Place {
        apps: &apps,
        state: None,
    };
    let mut live = Live::default();
    let t0 = Instant::now();
    let mut ask = |value: Value, at: Instant| {
        answer(&mut live, &place, "demo", &frame(value), true, at).unwrap()
    };
    let took = |id: u64| json!({"id": id, "op": "app.took", "turn": "t1"});
    assert_eq!(ask(took(1), t0)["ok"], true);
    assert_eq!(
        ask(took(2), t0 + Duration::from_millis(10))["error"]["code"],
        "rate"
    );
    assert_eq!(ask(took(3), t0 + Duration::from_millis(150))["ok"], true);
    for bad in [
        "a\u{2028}b",
        "a\u{2029}b",
        "a\u{202E}b",
        "a\u{2066}b",
        "a\u{200F}b",
        "a\u{061C}b",
    ] {
        let panel = json!({"id": 9, "op": "app.panel.set", "title": "T", "lines": [bad]});
        let reply = ask(panel, t0 + Duration::from_secs(61));
        assert_eq!(reply["error"]["code"], "bad_request", "{bad:?}: {reply}");
    }
    let _ = std::fs::remove_dir_all(&apps);
}
