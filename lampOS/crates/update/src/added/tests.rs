use super::*;

#[test]
fn a_unit_is_enabled_by_the_links_its_install_section_asks_for() {
    let text = "[Unit]\nWantedBy=nothing.target\n[Service]\nExecStart=/x\n[Install]\nWantedBy=lampos.target multi-user.target\nRequiredBy = sockets.target\n";
    assert_eq!(
        wants("lamp-a.socket", text).unwrap(),
        [
            "/etc/systemd/system/lampos.target.wants/lamp-a.socket",
            "/etc/systemd/system/multi-user.target.wants/lamp-a.socket",
            "/etc/systemd/system/sockets.target.requires/lamp-a.socket",
        ]
    );
    assert_eq!(
        target("/etc/systemd/system/lampos.target.wants/lamp-a.socket"),
        Some("lampos.target")
    );
    assert_eq!(
        target("/etc/systemd/system/sockets.target.requires/lamp-a.socket"),
        Some("sockets.target")
    );
    // No [Install]: never enabled.
    assert!(
        wants("lamp-a.service", "[Service]\nExecStart=/x\n")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_install_section_a_round_does_not_write_is_refused() {
    for (name, text) in [
        ("lamp-a.service", "[Install]\nAlias=lamp-b.service\n"),
        ("lamp-a.service", "[Install]\nAlso=lamp-b.socket\n"),
        ("lamp-a@.service", "[Install]\nWantedBy=lampos.target\n"),
        ("lamp-a.service", "[Install]\nWantedBy=../x.target\n"),
        ("lamp-a.service", "[Install]\nWantedBy=%i.target\n"),
    ] {
        assert!(wants(name, text).is_err(), "{name}: {text}");
    }
}

#[test]
fn users_and_groups_the_lamp_lacks_are_named_and_root_needs_none() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("etc")).unwrap();
    fs::write(
        root.path().join("etc/passwd"),
        "lampos:x:999:999::/nonexistent:/usr/sbin/nologin\n",
    )
    .unwrap();
    fs::write(
        root.path().join("etc/group"),
        "lampos:x:999:\naudio:x:29:lampos\n",
    )
    .unwrap();
    let have = "[Service]\nUser=lampos\nGroup=lampos\nSupplementaryGroups=audio\n";
    assert!(missing(root.path(), have).is_empty());
    assert!(missing(root.path(), "[Service]\nUser=root\nGroup=root\n").is_empty());
    assert_eq!(
        missing(
            root.path(),
            "[Service]\nUser=lampos-new\nGroup=lampos-new\nSupplementaryGroups=audio video\n"
        ),
        ["user lampos-new", "group lampos-new", "group video"]
    );
}
