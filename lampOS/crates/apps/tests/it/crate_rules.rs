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
    let net = std::fs::read_to_string(format!("{dir}lampapp-net@.service")).unwrap();
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
    let plain = std::fs::read_to_string(format!("{dir}lampapp@.service")).unwrap();
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
    for file in ["lampapp@.service", "lampapp-net@.service"] {
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
    for (file, net) in [("lampapp@.service", false), ("lampapp-net@.service", true)] {
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
