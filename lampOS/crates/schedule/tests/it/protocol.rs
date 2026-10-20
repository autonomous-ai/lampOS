use lamp_schedule::protocol::{MAX_LINE_BYTES, Request};

#[test]
fn one_line_requests_reject_unknown_and_duplicate_fields() {
    for line in [
        r#"{"op":"status","extra":true}"#,
        r#"{"op":"list","limit":8,"limit":1}"#,
        r#"{"op":"other"}"#,
        r#"{"op":"get","id":-1}"#,
        r#"{"op":"get","id":1.0}"#,
        r#"[]"#,
        r#"null"#,
    ] {
        assert!(Request::parse(line.as_bytes()).is_err(), "{line}");
    }
}
#[test]
fn mutations_require_a_complete_caller_guard() {
    for line in [
        r#"{"op":"cancel","id":1}"#,
        r#"{"op":"cancel","id":1,"guard":{"revision":0,"request_id":"a"}}"#,
        r#"{"op":"cancel","id":1,"guard":{"store_id":"x","revision":0,"request_id":"a","extra":false}}"#,
    ] {
        assert!(Request::parse(line.as_bytes()).is_err());
    }
}
#[test]
fn request_size_and_utf8_are_checked_without_lossy_conversion() {
    assert_eq!(
        Request::parse(&vec![b' '; MAX_LINE_BYTES + 1]),
        Err("too_long")
    );
    assert!(Request::parse(&[0xff]).is_err());
    assert!(Request::parse(br#"{"op":"status"} trailing"#).is_err());
}
#[test]
fn list_defaults_are_bounded_and_roundtrip_preserves_request_identity() {
    let r = Request::parse(br#"{"op":"list"}"#).unwrap();
    assert_eq!(
        r,
        Request::List {
            after_id: 0,
            limit: 8,
            pending_only: false
        }
    );
    assert_eq!(Request::parse(&serde_json::to_vec(&r).unwrap()).unwrap(), r);
}
