use super::*;
use std::{collections::BTreeMap, fs, path::Path};

/// `deploy/product/`, as the publisher reads it.
fn product() -> BTreeMap<String, String> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    fs::read_dir(folder)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, fs::read_to_string(entry.path()).unwrap_or_default())
        })
        .collect()
}

fn linked(part: &str) -> Option<(String, String)> {
    Some((format!("/opt/lampOS/{part}/"), format!("lamp-{part}")))
}

#[test]
fn names_of_a_parts_other_units_are_known_and_nothing_else_is() {
    assert_eq!(kind("lamp-setup-wifi@.service"), Some(Kind::Template));
    assert_eq!(kind("lamp-setup-wifi.socket"), Some(Kind::Socket));
    assert_eq!(kind("lamp-records.timer"), Some(Kind::Timer));
    for name in [
        "lamp-voice.service",
        "lampos.target",
        "lampos-parts.slice",
        "lampos-setup-done.path",
        "lamp-x@1.service",
        "lamp-@.service",
        "lamp-X.socket",
        "../lamp-x.timer",
        "lamp-x.timer.d",
    ] {
        assert_eq!(kind(name), None, "{name}");
    }
}

#[test]
fn each_shipped_template_socket_and_timer_belongs_to_its_part_or_none() {
    let product = product();
    let found: BTreeMap<&str, Option<(String, String)>> = product
        .iter()
        .filter(|(name, _)| kind(name).is_some())
        .map(|(name, text)| {
            let part = part(name, text, |unit| product.get(unit).map(String::as_str));
            (name.as_str(), part)
        })
        .collect();
    let expected = BTreeMap::from([
        ("lamp-eyes-camera.socket", linked("eyes")),
        ("lamp-eyes-camera@.service", linked("eyes")),
        ("lamp-keeper-restart.socket", None),
        ("lamp-keeper-restart@.service", None),
        ("lamp-records.timer", linked("records")),
        ("lamp-selftest.timer", linked("selftest")),
        ("lamp-setup-change.socket", linked("setup")),
        ("lamp-setup-change@.service", linked("setup")),
        ("lamp-setup-keys.socket", linked("setup")),
        ("lamp-setup-keys@.service", linked("setup")),
        ("lamp-setup-wifi.socket", linked("setup")),
        ("lamp-setup-wifi@.service", linked("setup")),
        ("lamp-skills-manage.socket", linked("skills")),
        ("lamp-skills-manage@.service", linked("skills")),
        ("lamp-update-back.timer", None),
    ]);
    assert_eq!(found, expected);
}

#[test]
fn what_a_socket_or_timer_starts_is_its_named_unit_else_its_own_name() {
    assert_eq!(
        starts("lamp-a.socket", "[Socket]\nAccept=yes\n").as_deref(),
        Some("lamp-a@.service")
    );
    assert_eq!(
        starts("lamp-a.socket", "[Socket]\nAccept=no\n").as_deref(),
        Some("lamp-a.service")
    );
    assert_eq!(
        starts("lamp-a.socket", "[Socket]\nService=lamp-b.service\n").as_deref(),
        Some("lamp-b.service")
    );
    assert_eq!(
        starts("lamp-a.timer", "[Timer]\nOnCalendar=hourly\n").as_deref(),
        Some("lamp-a.service")
    );
    assert_eq!(
        starts(
            "lamp-a.timer",
            "[Timer]\nUnit=lamp-b.service\nUnit=lamp-c.service\n"
        )
        .as_deref(),
        Some("lamp-c.service")
    );
    // As systemd reads it, with spaces around `=`.
    assert_eq!(
        starts("lamp-a.timer", "[Timer]\nUnit = lamp-b.service\n").as_deref(),
        Some("lamp-b.service")
    );
    assert_eq!(
        starts("lamp-a.socket", "[Socket]\nAccept = yes\n").as_deref(),
        Some("lamp-a@.service")
    );
    // Something that is not a part's unit is not followed.
    assert_eq!(
        starts("lamp-a.timer", "[Timer]\nUnit=lampos.target\n"),
        None
    );
    assert_eq!(starts("lamp-a.timer", "[Timer]\nUnit=\n"), None);
    assert_eq!(starts("lamp-a@.service", "[Service]\n"), None);
}

#[test]
fn a_socket_or_timer_whose_unit_is_not_there_or_not_linked_has_no_part() {
    let pinned = "[Service]\nExecStart=/opt/lampOS/a/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/lamp-a\n";
    assert_eq!(part("lamp-a.timer", "[Timer]\n", |_| None), None);
    assert_eq!(part("lamp-a.timer", "[Timer]\n", |_| Some(pinned)), None);
    assert_eq!(
        part("lamp-a.timer", "[Timer]\n", |_| Some(
            "[Service]\nExecStart=/opt/lampOS/a/current/lamp-a run\n"
        )),
        linked("a")
    );
}

#[test]
fn a_socket_belongs_to_a_part_only_through_a_template_and_a_timer_only_through_a_service() {
    let linked_text = "[Service]\nExecStart=/opt/lampOS/a/current/lamp-a run\n";
    // One listener handed to a running service: never restarted by a round.
    assert_eq!(
        part("lamp-a.socket", "[Socket]\n", |_| Some(linked_text)),
        None
    );
    assert_eq!(
        part("lamp-a.socket", "[Socket]\nAccept=yes\n", |name| (name
            == "lamp-a@.service")
            .then_some(linked_text)),
        linked("a")
    );
    assert_eq!(
        part("lamp-a.timer", "[Timer]\nUnit=lamp-a@.service\n", |_| Some(
            linked_text
        )),
        None
    );
}
