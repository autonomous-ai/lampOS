use lamp_surround::{
    atomic,
    operator::{ErrorCode, Kind, Operation, Phase, View},
    runtime::*,
    setup, startup,
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    sync::Arc,
};

fn dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap())
        .unwrap()
}
fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("../../fixtures/surround.json")).unwrap()
}

#[test]
fn sample_is_disabled_and_no_board_fact_is_invented() {
    let config = Config::parse(include_bytes!("../../fixtures/surround.json")).unwrap();
    assert!(!config.enabled);
    assert!(config.owner_socket.is_none());
    assert!(config.network.is_none());
    assert!(config.release.is_none());
    assert!(
        config
            .startup
            .components
            .iter()
            .all(|c| !c.configured && !c.binary_available)
    );
    config.network_adapter(Arc::new(|| 1)).unwrap();
}

#[test]
fn wrong_versions_unknown_fields_weak_origins_and_bad_paths_fail_closed() {
    for (key, value) in [
        ("schema_version", serde_json::json!(2)),
        ("api_key", serde_json::json!("do-not-echo")),
        ("state_dir", serde_json::json!("/a/../b")),
        (
            "socket",
            serde_json::json!("/".to_owned() + &"a".repeat(110)),
        ),
        ("operator_origin", serde_json::json!("http://lamp.local")),
        ("reset_stores", serde_json::json!(["network", "voice"])),
    ] {
        let mut doc = fixture();
        doc[key] = value;
        let error = Config::parse(&serde_json::to_vec(&doc).unwrap()).unwrap_err();
        assert!(!error.to_string().contains("do-not-echo"));
    }
}

#[test]
fn roots_cannot_overlap_or_share_listener_writer_lock() {
    for path in [
        "/var/lib/lampOS/surround/operations",
        "/var/lib/lampOS/surround/operations/child",
        "/var/lib/lampOS/surround",
        "/run/lampos-surround",
    ] {
        let mut doc = fixture();
        doc["configuration_dir"] = path.into();
        assert!(
            Config::parse(&serde_json::to_vec(&doc).unwrap()).is_err(),
            "{path}"
        );
    }
    for path in [
        "/var/lib/lampOS/surround/configuration/generations/operator.sock",
        "/var/lib/lampOS/surround/operations/sub/operator.sock",
    ] {
        let mut doc = fixture();
        doc["socket"] = path.into();
        assert!(Config::parse(&serde_json::to_vec(&doc).unwrap()).is_err());
    }
    for path in [
        "/var/lib/lampOS/surround/configuration/generations",
        "/var/lib/lampOS/surround/operations",
        "/var/lib/lampOS",
    ] {
        let mut doc = fixture();
        doc["bus_root"] = path.into();
        assert!(Config::parse(&serde_json::to_vec(&doc).unwrap()).is_err());
    }
}

#[test]
fn release_urls_are_fixed_https_bases_not_caller_supplied_credentials() {
    let mut doc = fixture();
    doc["release"] = serde_json::json!({"trusted_root":"/etc/lampos/surround/root.json","metadata_url":"https://updates.example/metadata/","targets_url":"https://updates.example/targets/","state_dir":"/var/lib/lampOS/trust","release_dir":"/opt/lampOS/releases","selection_dir":"/var/lib/lampOS/selection","compatibility":{"board":"stand-in-board","architecture":"aarch64","protocol":1,"data_schema":1}});
    assert!(Config::parse(&serde_json::to_vec(&doc).unwrap()).is_ok());
    for bad in [
        "http://updates.example/",
        "https://user:secret@updates.example/",
        "https://updates.example/?key=secret",
        "https://updates.example/no-trailing-slash",
    ] {
        doc["release"]["metadata_url"] = bad.into();
        assert!(Config::parse(&serde_json::to_vec(&doc).unwrap()).is_err());
    }
}

#[test]
fn explicit_network_enable_still_needs_build_and_platform_support() {
    let mut doc = fixture();
    doc["enabled"] = true.into();
    doc["network"] = serde_json::json!({"enabled":true,"interface":"wlan0","kind":"wifi","recovery_profile_uuid":null,"station_profile_uuid":null});
    let config = Config::parse(&serde_json::to_vec(&doc).unwrap()).unwrap();
    if !cfg!(all(target_os = "linux", feature = "network-manager")) {
        assert!(config.network_adapter(Arc::new(|| 0)).is_err());
    }
    doc["network"]["interface"] = "wlan0; shell".into();
    assert!(Config::parse(&serde_json::to_vec(&doc).unwrap()).is_err());
}

#[test]
fn private_config_read_refuses_symlinks_and_public_files() {
    let root = dir();
    let path = root.path().join("config.json");
    atomic::write(
        &path,
        include_bytes!("../../fixtures/surround.json"),
        MAX_CONFIG_BYTES,
    )
    .unwrap();
    Config::read(&path).unwrap();
    let link = root.path().join("link.json");
    symlink(&path, &link).unwrap();
    assert!(Config::read(&link).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Config::read(&path).is_err());
}

#[tokio::test]
async fn listener_is_private_single_owner_and_cleans_only_its_own_inode() {
    let root = dir();
    let path = root.path().join("operator.sock");
    let (listener, guard) = listen(&path).await.unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(listen(&path).await.is_err());
    drop(listener);
    drop(guard);
    assert!(!path.exists());
    let (listener, guard) = listen(&path).await.unwrap();
    fs::remove_file(&path).unwrap();
    fs::write(&path, "replacement").unwrap();
    drop(listener);
    drop(guard);
    assert_eq!(fs::read_to_string(path).unwrap(), "replacement");
}

#[tokio::test]
async fn stale_private_socket_recovers_but_files_links_and_live_sockets_are_retained() {
    let root = dir();
    let path = root.path().join("operator.sock");
    let stale = std::os::unix::net::UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    drop(stale);
    let (listener, guard) = listen(&path).await.unwrap();
    drop(listener);
    drop(guard);
    fs::write(&path, "do-not-delete").unwrap();
    assert!(listen(&path).await.is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "do-not-delete");
    fs::remove_file(&path).unwrap();
    let outside = root.path().join("outside");
    fs::write(&outside, "keep").unwrap();
    symlink(&outside, &path).unwrap();
    assert!(listen(&path).await.is_err());
    assert_eq!(fs::read_to_string(outside).unwrap(), "keep");
    fs::remove_file(&path).unwrap();
    let live = std::os::unix::net::UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(listen(&path).await.is_err());
    assert!(path.exists());
    drop(live);
}

#[test]
fn readiness_expires_during_slow_work_and_inhibition_never_becomes_listening() {
    let view = View {
        enabled: true,
        runtime: Some(setup::RuntimeState::Ready),
        runtime_observed_us: Some(1_000_000),
        voice_capability: Some(startup::Capability::Inhibited),
        ..View::default()
    };
    assert_eq!(
        view.at(2_999_999).voice_capability,
        Some(startup::Capability::Inhibited)
    );
    assert_eq!(
        view.at(3_000_000).runtime,
        Some(setup::RuntimeState::Unknown)
    );
    assert_eq!(view.at(999_999).voice_capability, None);
    let report = diagnostic(&view, 3_000_000);
    assert!(
        report
            .values
            .iter()
            .any(|kv| kv.key == "runtime" && kv.value == "Unknown")
    );
}

#[test]
fn diagnostic_separates_surround_health_from_lamp_readiness_and_contains_no_paths() {
    let mut view = View::default();
    assert_eq!(diagnostic(&view, 1).level, lamp_msgs::Level::Warn);
    view.enabled = true;
    assert_eq!(diagnostic(&view, 1).level, lamp_msgs::Level::Ok);
    view.operation = Some(Operation {
        schema_version: 1,
        id: "private-operation".into(),
        kind: Kind::JoinWifi,
        phase: Phase::Failed,
        updated_us: 1,
        error: Some(ErrorCode::Unavailable),
    });
    let report = diagnostic(&view, 1);
    assert_eq!(report.level, lamp_msgs::Level::Warn);
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("private-operation")
    );
}
