use lamp_apps::yes::{Op, YesKey, parse_reply, request_line};

fn key() -> YesKey {
    YesKey {
        op: Op::Update,
        name: "english-lessons".into(),
        version: "1.2.0".into(),
        sha256: "ab".repeat(32),
        grants: vec!["speak".into(), "hear".into()],
        net: true,
        logs: false,
    }
}

#[test]
fn the_request_is_one_line_with_exactly_the_key() {
    let line = request_line(&key());
    assert!(!line.contains('\n'));
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    let take = &value["app_yes_take"];
    assert_eq!(take["op"], "update");
    assert_eq!(take["name"], "english-lessons");
    assert_eq!(take["version"], "1.2.0");
    assert_eq!(take["sha256"], "ab".repeat(32));
    assert_eq!(
        take["grants"],
        serde_json::json!(["hear", "speak"]),
        "sorted"
    );
    assert_eq!(value.as_object().unwrap().len(), 1);
    assert_eq!(take["net"], true);
    assert_eq!(take["logs"], false);
    assert_eq!(take.as_object().unwrap().len(), 7);
}

#[test]
fn the_ops_are_named_as_the_wire_names_them() {
    for (op, word) in [
        (Op::Install, "install"),
        (Op::Update, "update"),
        (Op::Remove, "remove"),
    ] {
        let mut key = key();
        key.op = op;
        assert!(request_line(&key).contains(&format!("\"op\":\"{word}\"")));
    }
}

#[test]
fn only_an_ok_true_reply_is_a_yes() {
    assert_eq!(parse_reply("{\"ok\":true}"), Ok(()));
    assert_eq!(parse_reply("{\"ok\":true}\n"), Ok(()));
    for bad in [
        "",
        "{}",
        "{\"ok\":false}",
        "{\"ok\":\"true\"}",
        "{\"ok\":1}",
        "ok",
        "{\"ok\":true,\"x\":1}",
        "{\"refused\":\"no\"}",
        "[true]",
        "{\"ok\":true}{\"ok\":true}",
    ] {
        assert!(parse_reply(bad).is_err(), "{bad:?}");
    }
    assert_eq!(
        parse_reply("{\"refused\":\"expired\"}"),
        Err("expired".to_owned())
    );
}
