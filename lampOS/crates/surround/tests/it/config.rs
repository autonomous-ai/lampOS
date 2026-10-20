use lamp_surround::{atomic, config::*};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

fn private_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap())
        .unwrap()
}
fn payload() -> ProviderPayload {
    serde_json::from_value(serde_json::json!({
        "endpoint":"wss://provider.example/live", "model":"gemini-3.8-live",
        "voice":"Kore", "credential_file":"/private/provider.key"
    }))
    .unwrap()
}

#[test]
fn voice_file_has_exact_payload_without_surround_metadata_or_secrets() {
    let dir = private_dir();
    let doc = ProviderDocument::new(payload()).unwrap();
    write_document(&dir.path().join("document.json"), &doc).unwrap();
    write_voice_payload(&dir.path().join("voice.json"), &doc.provider).unwrap();
    let wrapped: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("document.json")).unwrap()).unwrap();
    let voice: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("voice.json")).unwrap()).unwrap();
    assert_eq!(wrapped["provider"], voice);
    assert!(voice.get("schema_version").is_none());
    assert!(voice.get("provider").is_none());
    assert_eq!(voice["authentication"], "api_key");
    assert_eq!(voice["resumption"], "retain");
    assert_eq!(
        fs::metadata(dir.path().join("voice.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn unknown_fields_and_versions_fail_without_echoing_private_input() {
    for extra in [
        "\"provider\":{},\"private-key-material\":true",
        "\"provider\":{},\"schema_version\":2",
    ] {
        let bad = format!("{{\"schema_version\":1,{extra}}}");
        let error = ProviderDocument::parse(bad.as_bytes()).unwrap_err();
        assert!(!error.to_string().contains("private-key-material"));
    }
    let mut doc = serde_json::json!({"schema_version":1,"provider":payload()});
    doc["provider"]["api_key"] = "do-not-echo".into();
    assert!(ProviderDocument::parse(&serde_json::to_vec(&doc).unwrap()).is_err());
    doc["provider"].as_object_mut().unwrap().remove("api_key");
    doc["schema_version"] = 2.into();
    assert!(ProviderDocument::parse(&serde_json::to_vec(&doc).unwrap()).is_err());
}

#[test]
fn provider_validation_reuses_known_thinking_and_context_constraints() {
    let mut p = payload();
    p.thinking_level = Some(lamp_gemini::ThinkingLevel::Low);
    assert!(p.validate().is_err());
    p.thinking_level = None;
    p.thinking_budget = Some(32_769);
    assert!(p.validate().is_err());
    p.thinking_budget = Some(0);
    p.context_trigger_tokens = Some(100);
    assert!(p.validate().is_err());
    p.context_target_tokens = Some(100);
    assert!(p.validate().is_err());
    p.context_target_tokens = Some(50);
    assert!(p.validate().is_ok());
    p.language_code = Some("en--US".into());
    assert!(p.validate().is_err());
}

#[test]
fn endpoint_credentials_and_timeout_errors_are_rejected_before_write() {
    let dir = private_dir();
    let path = dir.path().join("provider.json");
    let good = ProviderDocument::new(payload()).unwrap();
    write_document(&path, &good).unwrap();
    let original = fs::read(&path).unwrap();
    for endpoint in [
        "ws://provider.example/live",
        "wss://user:secret@provider.example/live",
        "wss://provider.example/?key=secret",
        "wss://provider.example/#secret",
    ] {
        let mut p = payload();
        p.endpoint = endpoint.into();
        assert!(write_voice_payload(&path, &p).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    let mut p = payload();
    p.response_timeout_ms = Some(1_999);
    assert!(p.validate().is_err());
    p.response_timeout_ms = Some(2_000);
    p.no_reply_after_ms = Some(2_000);
    assert!(p.validate().is_err());
    p.no_reply_after_ms = Some(1_000);
    assert!(p.validate().is_ok());
}

#[test]
fn private_paths_and_secret_bounds_are_enforced() {
    let mut p = payload();
    for path in ["relative.key", "/a/../key", "/a\nkey"] {
        p.credential_file = path.into();
        assert!(p.validate().is_err());
    }
    for bytes in [
        vec![],
        vec![b'x'; MAX_SECRET_BYTES + 1],
        b"key\n".to_vec(),
        vec![0xff],
        b"key\0".to_vec(),
    ] {
        assert!(Secret::new(bytes).is_err());
    }
    let secret = Secret::new(b" private-token ".to_vec()).unwrap();
    assert!(payload().validate_credential(&secret).is_err());
    let secret = Secret::new(b"private-token".to_vec()).unwrap();
    assert!(payload().validate_credential(&secret).is_ok());
    let dir = private_dir();
    write_secret(&dir.path().join("provider.key"), &secret).unwrap();
    assert_eq!(
        atomic::read(&dir.path().join("provider.key"), MAX_SECRET_BYTES)
            .unwrap()
            .unwrap(),
        b"private-token"
    );
}

#[test]
fn malformed_oversized_and_symlink_documents_are_never_accepted() {
    let dir = private_dir();
    let path = dir.path().join("document.json");
    atomic::write(&path, b"{", MAX_DOCUMENT_BYTES).unwrap();
    assert!(read_document(&path).is_err());
    assert!(ProviderDocument::parse(&vec![b' '; MAX_DOCUMENT_BYTES + 1]).is_err());
    let link = dir.path().join("link.json");
    symlink(&path, &link).unwrap();
    assert!(write_document(&link, &ProviderDocument::new(payload()).unwrap()).is_err());
    assert_eq!(fs::read(path).unwrap(), b"{");
}

#[test]
fn private_debug_hides_endpoint_paths_and_model() {
    let doc = ProviderDocument::new(payload()).unwrap();
    let debug = format!("{doc:?}");
    for private in [
        "provider.example",
        "private/provider.key",
        "gemini-3.8-live",
    ] {
        assert!(!debug.contains(private));
    }
}

/// #1505: a generation's `document.json` as a later build writes it, with a
/// field this build doesn't know at its top and in the provider. The same
/// document without them reads; with them, `read_document` refuses it today,
/// and a candidate checked at open stops lamp-surround starting. No fix yet,
/// and no unit starts lamp-surround on a lamp: whoever makes it read one
/// release forward flips this to a plain test (Tests).
#[test]
#[should_panic(expected = "invalid provider configuration")]
fn a_document_from_a_later_build_is_read() {
    let dir = private_dir();
    let path = dir.path().join("document.json");
    let doc = ProviderDocument::new(payload()).unwrap();
    write_document(&path, &doc).unwrap();
    assert!(read_document(&path).unwrap().is_some());
    let mut later: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    later["a_field_from_a_later_build"] = serde_json::json!(true);
    later["provider"]["a_field_from_a_later_build"] = serde_json::json!(true);
    fs::write(&path, serde_json::to_vec(&later).unwrap()).unwrap();
    assert!(read_document(&path).unwrap().is_some());
}
