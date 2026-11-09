//! The office reader's installation boundary is distinct from the full-mode unit.
use std::path::Path;

#[test]
fn office_unit_has_only_the_dedicated_session_reader_identity_and_network() {
    let lines: Vec<_> = include_str!("../../../deploy/units/lamp-peek.service")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    for required in [
        "Environment=LAMP_PEEK=sessions-only",
        "User=lamp-peek",
        "Group=lamp-peek",
        "SupplementaryGroups=lamp-session-readers",
        "NoNewPrivileges=yes",
        "ProtectSystem=strict",
        "ProtectHome=yes",
        "PrivateTmp=yes",
        "PrivateDevices=yes",
        "RestrictAddressFamilies=AF_INET AF_INET6",
        "ConditionPathExists=/var/lib/lampos-setup/done",
        "After=local-fs.target lamp-setup.service",
        "Environment=LAMP_PEEK_SESSIONS=/var/lib/lampOS/voice/sessions",
        "ExecStart=/opt/lampOS/peek/@PEEK_SHA256@/lamp-peek",
        "AmbientCapabilities=CAP_NET_BIND_SERVICE",
        "CapabilityBoundingSet=CAP_NET_BIND_SERVICE",
    ] {
        assert!(lines.contains(&required), "missing {required}");
    }
    for key in [
        "User=",
        "Group=",
        "SupplementaryGroups=",
        "RestrictAddressFamilies=",
    ] {
        assert_eq!(lines.iter().filter(|line| line.starts_with(key)).count(), 1);
    }
    for forbidden in [
        "ReadWritePaths=",
        "Environment=LAMP_BUS",
        "Environment=LAMP_PEEK=on",
        "Conflicts=",
        "PartOf=lampos.target",
    ] {
        assert!(
            !lines.iter().any(|line| line.starts_with(forbidden)),
            "{forbidden}"
        );
    }
    let inaccessible = lines
        .iter()
        .find(|line| line.starts_with("InaccessiblePaths="))
        .unwrap();
    for private in [
        "/run/lampos-bus",
        "/run/log/journal",
        "/var/log/journal",
        "/run/systemd/private",
        "/run/dbus/system_bus_socket",
    ] {
        assert!(inaccessible.contains(private), "{private}");
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy");
    assert!(!root.join("product/lamp-peek.service").exists());
    let target = std::fs::read_to_string(root.join("units/lampos.target")).unwrap();
    assert!(!target.contains("lamp-peek"));
}
