//! Rules over the crate's own source (no uid, no user made, nothing recursive).
use std::fs;

fn sources() -> Vec<(String, String)> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| {
            (
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            )
        })
        .collect()
}

#[test]
fn no_uid_is_stored_or_compared_and_no_user_is_made() {
    // Spelled in pieces so this file does not trip its own rule.
    let words = [
        ["getpw", "nam"].concat(),
        ["uid", "_of"].concat(),
        ["ch", "own"].concat(),
        ["user", "add"].concat(),
        ["user", "del"].concat(),
        ["group", "add"].concat(),
    ];
    for (path, text) in sources() {
        for word in &words {
            // The one place that hands a folder to `lampos`: `folders.rs`, a fixed program and a fixed pair of
            // names for the staging folder, checked by its own test below.
            if path.ends_with("/folders.rs") && word.as_str() == ["ch", "own"].concat() {
                continue;
            }
            let hits: Vec<&str> = text
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .filter(|line| line.contains(word.as_str()))
                .collect();
            assert!(hits.is_empty(), "{path}: `{word}`: {hits:?}");
        }
    }
}

/// `net` is the internet only, never the home network (the first builder,
/// #7415, 13:58): the unit's own IP filter denies every private, loopback,
/// link-local and multicast range, v4 and v6; the plain unit has no network.
#[test]
fn the_net_unit_denies_the_home_network_and_the_plain_one_has_no_network() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy/product/");
    let net = std::fs::read_to_string(format!("{dir}lamp-app-net@.service")).unwrap();
    let deny = net
        .lines()
        .find_map(|line| line.strip_prefix("IPAddressDeny="))
        .expect("an IPAddressDeny= line");
    for range in [
        "127.0.0.0/8",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "169.254.0.0/16",
        "100.64.0.0/10",
        "224.0.0.0/4",
        "::1/128",
        "fc00::/7",
        "fe80::/10",
        "ff00::/8",
    ] {
        assert!(deny.split_whitespace().any(|r| r == range), "{range}");
    }
    assert!(!deny.contains("any"), "never the allow-all form");
    // A home's global IPv6 addresses (the lamp's, the NAS's, the router's) are in no deny list above, so the unit
    // leaves no v6 path at all: no AF_INET6 socket (App Only's and Heron's HOLD at e05bc64ce2, 20 Nov).
    let families = net
        .lines()
        .find_map(|line| line.strip_prefix("RestrictAddressFamilies="))
        .expect("a RestrictAddressFamilies= line");
    let mut families: Vec<&str> = families.split_whitespace().collect();
    families.sort_unstable();
    assert_eq!(families, ["AF_INET", "AF_UNIX"], "no AF_INET6");
    assert!(
        !net.lines()
            .any(|line| line.starts_with("RestrictAddressFamilies=") && line.contains("INET6")),
        "no line allows a v6 socket"
    );
    let plain = std::fs::read_to_string(format!("{dir}lamp-app@.service")).unwrap();
    assert!(plain.lines().any(|l| l == "PrivateNetwork=yes"));
    assert!(!plain.contains("AF_INET"));
}

/// The lines of one `[Section]` of a unit file, without comments.
fn unit_section(text: &str, name: &str) -> Vec<String> {
    let header = format!("[{name}]");
    let mut inside = false;
    let mut lines = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == header;
        } else if inside && !line.is_empty() && !line.starts_with('#') {
            lines.push(line.to_owned());
        }
    }
    lines
}

/// systemd reads `StartLimitIntervalSec=` and `StartLimitBurst=` only in
/// `[Unit]`; under `[Service]` they are an unknown key, dropped, and the
/// window stays at its 10 s default, so a crashing app restarts for ever
/// (Thirteen's HOLD on #7813). Both app templates carry the cap in `[Unit]`.
#[test]
fn the_apps_restart_cap_is_in_unit_and_never_in_service() {
    for file in ["lamp-app@.service", "lamp-app-net@.service"] {
        let path = format!("{}/../../deploy/product/{file}", env!("CARGO_MANIFEST_DIR"));
        let text = fs::read_to_string(&path).unwrap();
        let unit = unit_section(&text, "Unit");
        let service = unit_section(&text, "Service");
        assert!(
            unit.contains(&"StartLimitIntervalSec=600".to_owned()),
            "{file}"
        );
        assert!(unit.contains(&"StartLimitBurst=5".to_owned()), "{file}");
        assert!(
            !service.iter().any(|line| line.starts_with("StartLimit")),
            "{file}: StartLimit* under [Service] is dropped by systemd"
        );
        assert!(service.contains(&"Restart=on-failure".to_owned()), "{file}");
    }
}

/// An app sees its own door and nothing else under `/run` (the first
/// builder's 00:21 read of #7813, 00:28 yes): a tmpfs over `/run` with the
/// apps' socket folder bound in read-only; and only the net unit has a
/// resolver of its own, the helper's, bound over `/etc/resolv.conf`.
#[test]
fn an_app_sees_only_its_door_under_run_and_only_a_net_app_has_a_resolver() {
    let read = |file: &str| {
        let path = format!("{}/../../deploy/product/{file}", env!("CARGO_MANIFEST_DIR"));
        unit_section(&fs::read_to_string(path).unwrap(), "Service")
    };
    let resolver = "BindReadOnlyPaths=/opt/lampOS/apps/.resolv.conf:/etc/resolv.conf".to_owned();
    for (file, net) in [
        ("lamp-app@.service", false),
        ("lamp-app-net@.service", true),
    ] {
        let service = read(file);
        assert!(
            service.contains(&"TemporaryFileSystem=/run".to_owned()),
            "{file}"
        );
        assert!(
            service.contains(&"BindReadOnlyPaths=/run/lampos-apps".to_owned()),
            "{file}: the folder, not the socket file"
        );
        assert_eq!(
            service.contains(&resolver),
            net,
            "{file}: the resolver is the net unit's alone"
        );
        // No other path of /run is let in, and nothing is writable.
        let binds: Vec<_> = service
            .iter()
            .filter(|line| line.starts_with("BindPaths=") || line.starts_with("BindReadOnlyPaths="))
            .collect();
        assert_eq!(binds.len(), if net { 2 } else { 1 }, "{file}: {binds:?}");
        assert!(service.contains(&"ReadWritePaths=".to_owned()), "{file}");
    }
}

/// #7920 on lamp-75c9 (16:36, 17:24): systemd refuses a unit whose
/// `ReadWritePaths=` names a path that is not there (226/NAMESPACE), even for a
/// "+" step, so the helper's two units keep `strict` and their tight list only
/// because a unit of the part makes the three folders first. That unit runs the
/// helper itself (`lamp-apps-manage folders`): a round carries a unit only when
/// it is binary-backed, so one that ran `install` was never added (#7956's row,
/// 17:23). And until the folders are there, nothing may sit failed: the helper's
/// units `Want` it (a missing unit is no failure) and are skipped by their
/// conditions.
#[test]
fn the_apps_folders_are_made_by_a_unit_of_the_part_and_the_helpers_skip_until_they_are_there() {
    let read = |file: &str| {
        let path = format!("{}/../../deploy/product/{file}", env!("CARGO_MANIFEST_DIR"));
        fs::read_to_string(path).unwrap()
    };
    let folders = read("lamp-apps-folders.service");
    let service = unit_section(&folders, "Service");
    for line in [
        "Type=oneshot",
        "RemainAfterExit=yes",
        "User=root",
        "ExecStart=/opt/lampOS/apps-manage/current/lamp-apps-manage folders",
        "CapabilityBoundingSet=CAP_CHOWN CAP_FOWNER",
        "NoNewPrivileges=yes",
    ] {
        assert!(service.contains(&line.to_owned()), "{line}");
    }
    assert!(
        !service
            .iter()
            .any(|line| line.starts_with("ProtectSystem=strict")
                || line.starts_with("ReadWritePaths=")
                || line.starts_with("ExecStartPre=")),
        "the folders unit names no path it makes: {service:?}"
    );
    for file in ["lamp-apps-start.service", "lamp-apps-manage@.service"] {
        let text = read(file);
        let unit = unit_section(&text, "Unit");
        let service = unit_section(&text, "Service");
        assert!(
            unit.contains(&"Wants=lamp-apps-folders.service".to_owned()),
            "{file}: Wants, not Requires: a missing unit is no failure"
        );
        assert!(
            !unit.iter().any(|line| line.starts_with("Requires=")),
            "{file}"
        );
        assert!(
            unit.iter().any(
                |line| line.starts_with("After=") && line.contains("lamp-apps-folders.service")
            ),
            "{file}: ordered after the folders"
        );
        for folder in ["/opt/lampOS/apps", "/var/lib/lampos-apps-state"] {
            assert!(
                unit.contains(&format!("ConditionPathIsDirectory={folder}")),
                "{file}: skipped, not failed, while {folder} is absent"
            );
        }
        assert!(
            service.contains(&"ProtectSystem=strict".to_owned()),
            "{file}"
        );
        for line in service
            .iter()
            .filter(|line| line.starts_with("ReadWritePaths="))
        {
            for entry in line["ReadWritePaths=".len()..].split_whitespace() {
                let made = ["/opt/lampOS/apps", "/var/lib/lampos-apps-state"].contains(&entry);
                assert!(
                    made || entry.starts_with('-'),
                    "{file}: {entry} may be missing at start"
                );
            }
        }
        assert!(
            !service.iter().any(|line| line.starts_with("ExecStartPre=")),
            "{file}: the folders are not made inside this sandbox"
        );
    }
}

/// The one `chown` of the crate hands the staging folder to `lampos` and nothing else: a fixed program, a fixed pair
/// of names, no environment, and no other path or user in the file.
#[test]
fn the_only_ownership_change_is_the_staging_folder_to_lampos() {
    let source =
        fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/folders.rs")).unwrap();
    let word = ["ch", "own"].concat();
    let calls: Vec<&str> = source
        .lines()
        .filter(|line| line.contains(&format!("\"/usr/bin/{word}\"")))
        .collect();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(source.contains(".arg(\"lampos:lampos\")"), "fixed names");
    assert!(source.contains(".env_clear()"), "no environment");
    assert!(
        source.contains("give(&layout.staging)?"),
        "the staging folder is the one path handed over"
    );
    assert_eq!(source.matches("give(").count(), 1, "called once");
}
