use super::*;
use serde_json::json;

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn five(op: &str, name: &str, grants: Value) -> Value {
    json!({"op": op, "name": name, "version": "1.0.0", "sha256": SHA, "grants": grants, "net": false, "logs": false})
}

fn ask(op: &str, value: Value) -> Map<String, Value> {
    let mut asked = Map::new();
    asked.insert(op.to_owned(), value);
    asked
}

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lampos-appyes-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An installed app `name` whose settings are `settings`.
fn install(apps: &Path, name: &str, settings: Value) {
    let dir = apps.join(name).join("current");
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = json!({
        "format": 2, "kind": "app", "name": name, "title": "T", "version": "1.0.0",
        "min_build": 1, "runtime": {"name": "python", "entry": "src/main.py"},
        "grants": ["speak"], "triggers": ["hello"], "settings": settings, "files": []
    });
    std::fs::write(dir.join("lamp-skill.json"), manifest.to_string()).unwrap();
}

fn run(
    yeses: &mut Yeses,
    live: &Live,
    place: &Place<'_>,
    op: &str,
    value: Value,
    peer: Peer,
    on: bool,
) -> Value {
    answer(
        yeses,
        live,
        place,
        &ask(op, value),
        peer,
        on,
        Instant::now(),
    )
    .expect("an op of this module")
}

#[test]
fn off_refuses_every_op_and_other_ops_are_not_ours() {
    let mut yeses = Yeses::default();
    let live = Live::default();
    let place = Place {
        apps: Path::new("/nonexistent"),
        state: None,
    };
    for op in [RECORD, TAKE, STATE, SETTINGS_SET] {
        let got = run(&mut yeses, &live, &place, op, json!({}), Peer::Voice, false);
        assert_eq!(got, json!({"refused": OFF}), "{op}");
    }
    assert!(
        answer(
            &mut yeses,
            &live,
            &place,
            &ask("clients", json!(1)),
            Peer::Voice,
            true,
            Instant::now()
        )
        .is_none()
    );
}

#[test]
fn the_yes_is_used_once_and_matches_exactly() {
    let (mut yeses, live) = (Yeses::default(), Live::default());
    let place = Place {
        apps: Path::new("/nonexistent"),
        state: None,
    };
    let yes = five("install", "demo", json!(["speak", "light"]));
    let got = run(
        &mut yeses,
        &live,
        &place,
        RECORD,
        yes.clone(),
        Peer::Cloud,
        true,
    );
    assert_eq!(got, json!({"ok": true}));
    // Other grants (and an order) never match, and do not spend the yes.
    let other = five("install", "demo", json!(["speak"]));
    let got = run(&mut yeses, &live, &place, TAKE, other, Peer::Helper, true);
    assert_eq!(got, json!({"refused": "key does not match"}));
    // The internet turned on is another key, though no grant is new.
    let mut net = yes.clone();
    net["net"] = json!(true);
    let got = run(&mut yeses, &live, &place, TAKE, net, Peer::Helper, true);
    assert_eq!(got, json!({"refused": "key does not match"}));
    let mut logs = yes.clone();
    logs["logs"] = json!(true);
    let got = run(&mut yeses, &live, &place, TAKE, logs, Peer::Helper, true);
    assert_eq!(got, json!({"refused": "key does not match"}));
    // The grants in another order are the same grants.
    let reordered = five("install", "demo", json!(["light", "speak", "light"]));
    let got = run(
        &mut yeses,
        &live,
        &place,
        TAKE,
        reordered.clone(),
        Peer::Helper,
        true,
    );
    assert_eq!(got, json!({"ok": true}));
    let got = run(
        &mut yeses,
        &live,
        &place,
        TAKE,
        reordered,
        Peer::Helper,
        true,
    );
    assert_eq!(got, json!({"refused": "no record"}));
}

#[test]
fn who_may_write_and_take() {
    let (mut yeses, live) = (Yeses::default(), Live::default());
    let place = Place {
        apps: Path::new("/nonexistent"),
        state: None,
    };
    let yes = five("remove", "demo", json!([]));
    for peer in [Peer::Helper, Peer::Other] {
        let got = run(&mut yeses, &live, &place, RECORD, yes.clone(), peer, true);
        assert!(got.get("refused").is_some(), "{peer:?}");
    }
    assert_eq!(
        run(
            &mut yeses,
            &live,
            &place,
            RECORD,
            yes.clone(),
            Peer::Voice,
            true
        ),
        json!({"ok": true})
    );
    for peer in [Peer::Voice, Peer::Cloud, Peer::Other] {
        let got = run(&mut yeses, &live, &place, TAKE, yes.clone(), peer, true);
        assert_eq!(got, json!({"refused": "not the apps helper"}), "{peer:?}");
    }
    assert_eq!(yeses.waiting(), 1);
}

#[test]
fn a_yes_expires_is_replaced_and_is_few() {
    let key = |op: &str, name: &str| key_of(&five(op, name, json!([]))).unwrap();
    let t0 = Instant::now();
    let mut yeses = Yeses::default();
    yeses.record(key("install", "a"), "voice", t0);
    assert_eq!(yeses.take(&key("install", "a"), t0 + LIVES), Err("expired"));
    yeses.record(key("install", "a"), "voice", t0);
    yeses.record(key("install", "a"), "app", t0);
    assert_eq!(yeses.waiting(), 1);
    assert_eq!(yeses.take(&key("install", "a"), t0), Ok("app"));
    for n in 0..MOST + 3 {
        yeses.record(key("install", &format!("app-{n}")), "voice", t0);
    }
    assert_eq!(yeses.waiting(), MOST);
    assert_eq!(yeses.take(&key("install", "app-0"), t0), Err("no record"));
}

#[test]
fn a_bad_request_is_refused() {
    for bad in [
        json!({"op": "install"}),
        five("destroy", "demo", json!([])),
        five("install", "Demo", json!([])),
        five("install", "demo", json!(["root"])),
        five("remove", "demo", json!(["speak"])),
        json!("text"),
    ] {
        assert!(key_of(&bad).is_err(), "{bad}");
    }
    let mut no_net = five("install", "demo", json!([]));
    no_net.as_object_mut().unwrap().remove("net");
    assert!(key_of(&no_net).is_err());
    let mut text_net = five("install", "demo", json!([]));
    text_net["net"] = json!("yes");
    assert!(key_of(&text_net).is_err());
    let mut remove_net = five("remove", "demo", json!([]));
    remove_net["net"] = json!(true);
    assert!(key_of(&remove_net).is_err());
    let mut extra = five("install", "demo", json!([]));
    extra["more"] = json!(1);
    assert!(key_of(&extra).is_err());
}

#[test]
fn the_peer_is_known_by_its_cgroup() {
    let line = |path: &str| format!("0::{path}\n");
    assert_eq!(peer_of_cgroup(&line(LAMP_CLOUD)), Peer::Cloud);
    assert_eq!(
        peer_of_cgroup(&line(
            "/lampos.slice/lampos-parts.slice/lamp-apps-manage@12-3.service"
        )),
        Peer::Helper
    );
    for other in [
        "/lampos.slice/lampos-parts.slice/lamp-apps-manage@12.service/x",
        "/lampos.slice/lampos-parts.slice/lamp-apps-manage@.service",
        "/lampos.slice/lampos-parts.slice/lamp-apps-manage@A.service",
        "/lampapps.slice/lamp-apps-manage@1.service",
        "/user.slice/lamp-cloud.service",
    ] {
        assert_eq!(peer_of_cgroup(&line(other)), Peer::Other, "{other}");
    }
    assert_eq!(peer_of(None), Peer::Other);
}

fn settings() -> Value {
    json!([
        {"key": "name", "type": "text", "label": "N", "default": "x", "max": 5},
        {"key": "loud", "type": "number", "label": "L", "default": 3, "min": 1, "max": 10},
        {"key": "on", "type": "toggle", "label": "O", "default": false},
        {"key": "mode", "type": "choice", "label": "M", "default": "a",
         "options": [{"value": "a", "label": "A"}, {"value": "b", "label": "B"}]}
    ])
}

#[test]
fn settings_are_checked_whole_kept_and_read_back() {
    let apps = folder("apps");
    let state = folder("state");
    install(&apps, "demo", settings());
    let (mut yeses, live) = (Yeses::default(), Live::default());
    let place = Place {
        apps: &apps,
        state: Some(&state),
    };
    // One bad value keeps none.
    let bad = json!({"name": "demo", "values": {"loud": 4, "mode": "z"}});
    assert!(
        run(
            &mut yeses,
            &live,
            &place,
            SETTINGS_SET,
            bad,
            Peer::Cloud,
            true
        )
        .get("refused")
        .is_some()
    );
    let got = run(
        &mut yeses,
        &live,
        &place,
        STATE,
        json!({"name": "demo"}),
        Peer::Cloud,
        true,
    );
    assert_eq!(got["settings"]["loud"], json!(3));
    for bad in [
        json!({"name": "demo", "values": {"loud": 11}}),
        json!({"name": "demo", "values": {"name": "toolong"}}),
        json!({"name": "demo", "values": {"on": "yes"}}),
        json!({"name": "demo", "values": {"nope": 1}}),
        json!({"name": "ghost", "values": {}}),
    ] {
        assert!(
            run(
                &mut yeses,
                &live,
                &place,
                SETTINGS_SET,
                bad.clone(),
                Peer::Cloud,
                true
            )
            .get("refused")
            .is_some(),
            "{bad}"
        );
    }
    let good = json!({"name": "demo", "values": {"loud": 4, "mode": "b"}});
    let got = run(
        &mut yeses,
        &live,
        &place,
        SETTINGS_SET,
        good,
        Peer::Cloud,
        true,
    );
    assert_eq!(got, json!({"ok": true, "changed": ["loud", "mode"]}));
    let got = run(
        &mut yeses,
        &live,
        &place,
        STATE,
        json!({}),
        Peer::Cloud,
        true,
    );
    assert_eq!(
        got["apps"]["demo"]["settings"],
        json!({"name": "x", "loud": 4, "on": false, "mode": "b"})
    );
    assert_eq!(got["apps"]["demo"]["socket"], json!(false));
    // Only lamp-cloud reads or writes them.
    for peer in [Peer::Voice, Peer::Helper, Peer::Other] {
        let got = run(&mut yeses, &live, &place, STATE, json!({}), peer, true);
        assert_eq!(got, json!({"refused": "not the cloud"}));
    }
    let _ = std::fs::remove_dir_all(&apps);
    let _ = std::fs::remove_dir_all(&state);
}

#[test]
fn the_panel_is_one_a_minute_and_the_socket_is_counted() {
    let mut live = Live::default();
    let t0 = Instant::now();
    assert_eq!(live.set_panel("demo", "T", &[], t0), Ok(1));
    assert!(
        live.set_panel("demo", "T", &[], t0 + Duration::from_secs(30))
            .is_err()
    );
    assert_eq!(live.set_panel("demo", "T", &[], t0 + PANEL_EVERY), Ok(2));
    live.connection("demo", true);
    assert!(live.socket("demo"));
    live.connection("demo", false);
    live.connection("demo", false);
    assert!(!live.socket("demo"));
}
