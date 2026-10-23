//! Restart order comes from the units being started, including the saved
//! units on a put-back. Binaries are text interpreted only by FakeSystemd.
use super::*;

fn name(part: &str) -> String {
    format!("lamp-{part}.service")
}

fn text(part: &str, binary: &str, declarations: &str) -> String {
    format!(
        "[Unit]\n{declarations}\n[Service]\nExecStart=/opt/lampOS/{part}/{}/lamp-{part}\nRestart=on-failure\n",
        sha(binary)
    )
}

fn install_part(lamp: &mut Lamp, part: &str, binary: &str, declarations: &str, active: bool) {
    // The real updater can execute Skills to validate its catalogue. These
    // fixtures exercise ordering only and must never supply a fake Skills.
    assert_ne!(part, "skills");
    let folder = lamp.root.join(format!("opt/lampOS/{part}/{}", sha(binary)));
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join(format!("lamp-{part}")), binary).unwrap();
    fs::write(
        lamp.root.join("etc/systemd/system").join(name(part)),
        text(part, binary, declarations),
    )
    .unwrap();
    if active {
        lamp.systemd.start(&name(part));
    } else {
        lamp.systemd.units.remove(&name(part));
    }
}

fn publish_parts(lamp: &mut Lamp, parts: &[(&str, &str, &str)]) {
    let binaries: Vec<_> = parts
        .iter()
        .map(|(part, binary, _)| (format!("lamp-{part}"), *binary))
        .collect();
    let units: Vec<_> = parts
        .iter()
        .map(|(part, binary, declarations)| (name(part), text(part, binary, declarations)))
        .collect();
    lamp.publish(
        2,
        &binaries
            .iter()
            .map(|(name, binary)| (name.as_str(), *binary))
            .collect::<Vec<_>>(),
        &units
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect::<Vec<_>>(),
    );
}

fn restarts(lamp: &Lamp) -> Vec<String> {
    lamp.calls()
        .into_iter()
        .filter_map(|call| call.strip_prefix("restart ").map(str::to_owned))
        .collect()
}

/// Only a literal section of the checked-in product fixture, independently
/// of the updater's ordering parser. It contains no folded lines today.
fn unit_section(text: &str) -> String {
    let mut inside = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == "[Unit]";
        } else if inside {
            assert!(
                !trimmed.ends_with('\\'),
                "extend this fixture for a folded line"
            );
            lines.push(line);
        }
    }
    assert!(
        !lines.is_empty(),
        "the product fixture has no [Unit] section"
    );
    lines.join("\n")
}

#[test]
fn after_and_before_order_the_forward_restarts_instead_of_unit_names() {
    let mut lamp = Lamp::new();
    for part in ["alpha", "middle", "zulu"] {
        install_part(&mut lamp, part, &format!("{part} 1"), "", true);
    }
    publish_parts(
        &mut lamp,
        &[
            ("alpha", "alpha 2", "After=lamp-middle.service"),
            ("middle", "middle 2", ""),
            ("zulu", "zulu 2", "Before=lamp-middle.service"),
        ],
    );
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(
        restarts(&lamp),
        [name("zulu"), name("middle"), name("alpha")]
    );
}

#[test]
fn the_product_voices_declared_managed_prerequisites_restart_before_it() {
    let product = lamp_update::install::PRODUCT;
    let voice = product
        .iter()
        .find(|(name, _)| *name == "lamp-voice.service")
        .unwrap()
        .1;
    let declarations = unit_section(voice);
    let mut prerequisites: Vec<&str> = declarations
        .lines()
        .filter_map(|line| line.trim().strip_prefix("After="))
        .flat_map(str::split_whitespace)
        .filter(|wanted| {
            product
                .iter()
                .any(|(name, text)| name == wanted && unit::linked(text).is_some())
        })
        .collect();
    prerequisites.sort_unstable();
    prerequisites.dedup();
    assert!(!prerequisites.is_empty());

    let mut lamp = Lamp::new();
    let mut parts = Vec::new();
    for unit in prerequisites.iter().copied().chain(["lamp-voice.service"]) {
        let part = unit
            .strip_prefix("lamp-")
            .unwrap()
            .strip_suffix(".service")
            .unwrap();
        let product_text = product.iter().find(|(name, _)| *name == unit).unwrap().1;
        let declarations = unit_section(product_text);
        install_part(&mut lamp, part, &format!("{part} 1"), &declarations, true);
        parts.push((part, format!("{part} 2"), declarations));
    }
    publish_parts(
        &mut lamp,
        &parts
            .iter()
            .map(|(part, binary, declarations)| (*part, binary.as_str(), declarations.as_str()))
            .collect::<Vec<_>>(),
    );
    assert_eq!(lamp.round(), Outcome::Switched(2));
    let restarted = restarts(&lamp);
    assert_eq!(restarted.len(), prerequisites.len() + 1, "{restarted:?}");
    let voice_at = restarted
        .iter()
        .position(|unit| unit == "lamp-voice.service")
        .unwrap();
    for prerequisite in prerequisites {
        let at = restarted
            .iter()
            .position(|unit| unit == prerequisite)
            .unwrap();
        assert!(
            at < voice_at,
            "{prerequisite} must precede Voice: {restarted:?}"
        );
    }
}

#[test]
fn rollback_uses_the_saved_old_dependencies_not_the_new_graph_or_its_reverse() {
    let mut lamp = Lamp::new();
    // Old startup: alpha, zulu, middle. New startup: middle, alpha, zulu.
    // Neither reversing the new graph nor sorting names gives the old one.
    for (part, declarations) in [
        ("alpha", "Before=lamp-zulu.service"),
        ("middle", "After=lamp-zulu.service"),
        ("zulu", ""),
    ] {
        install_part(&mut lamp, part, &format!("{part} 1"), declarations, true);
    }
    let before = lamp.lamp_files();
    publish_parts(
        &mut lamp,
        &[
            ("alpha", "alpha 2", "After=lamp-middle.service"),
            ("middle", "middle 2", ""),
            ("zulu", "zulu 2, unwell", "After=lamp-alpha.service"),
        ],
    );
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { after: Ok(()), .. }),
        "{outcome:?}"
    );
    assert_eq!(
        restarts(&lamp),
        [
            name("middle"),
            name("alpha"),
            name("zulu"),
            name("alpha"),
            name("zulu"),
            name("middle")
        ]
    );
    assert_eq!(lamp.lamp_files(), before);
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

/// Reconstruct the documented persisted prefix after unit writes, without
/// claiming a physical power cut. `switched` can omit an already-restored
/// unit; inactive members stay in the journal but must not be started.
fn interrupted(
    lamp: &mut Lamp,
    old: &[(&str, &str)],
    switched: &[(&str, &str)],
    inactive: &[&str],
    journal_order: &[&str],
) -> BTreeMap<String, String> {
    let mut saved = BTreeMap::new();
    for (part, declarations) in old {
        install_part(
            lamp,
            part,
            &format!("{part} 1"),
            declarations,
            !inactive.contains(part),
        );
        saved.insert(
            name(part),
            lamp.read(&format!("/etc/systemd/system/{}", name(part))),
        );
    }
    let records = lamp.root.join("var/lib/lampos-update");
    fs::create_dir_all(records.join("previous")).unwrap();
    for (name, text) in &saved {
        fs::write(records.join("previous").join(name), text).unwrap();
    }
    assert_eq!(journal_order.len(), saved.len());
    let units: Vec<_> = journal_order
        .iter()
        .map(|part| {
            assert!(saved.contains_key(&name(part)));
            serde_json::json!({"unit":name(part),"was_active":!inactive.contains(part)})
        })
        .collect();
    fs::write(
        records.join("journal.json"),
        serde_json::to_vec(&serde_json::json!({
            "build":{"sequence":2,"commit":"2".repeat(40)},
            "units":units
        }))
        .unwrap(),
    )
    .unwrap();
    for (part, declarations) in switched {
        install_part(
            lamp,
            part,
            &format!("{part} 2"),
            declarations,
            !inactive.contains(part),
        );
    }
    lamp.channel.offline = true;
    saved
}

#[test]
fn interrupted_recovery_orders_saved_units_despite_a_shuffled_partial_journal() {
    let mut lamp = Lamp::new();
    let saved = interrupted(
        &mut lamp,
        &[
            ("alpha", "Before=lamp-zulu.service"),
            ("middle", "After=lamp-zulu.service"),
            ("zulu", ""),
            ("sleeping", "Before=lamp-alpha.service"),
            ("steady", "Before=lamp-zulu.service"),
        ],
        &[
            ("alpha", "After=lamp-middle.service"),
            ("middle", ""),
            ("zulu", "After=lamp-alpha.service"),
            ("sleeping", ""),
        ],
        &["sleeping"],
        &["middle", "steady", "sleeping", "alpha", "zulu"],
    );
    let steady_pid = lamp.systemd.units[&name("steady")].pid;
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { after: Ok(()), .. }),
        "{outcome:?}"
    );
    assert_eq!(
        restarts(&lamp),
        [name("alpha"), name("zulu"), name("middle")]
    );
    for (name, text) in saved {
        assert_eq!(lamp.read(&format!("/etc/systemd/system/{name}")), text);
    }
    assert_eq!(lamp.systemd.units[&name("steady")].pid, steady_pid);
    assert!(!lamp.systemd.units.contains_key(&name("sleeping")));
    assert!(
        lamp.channel.fetched.is_empty(),
        "recovery must not depend on the channel"
    );
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

#[test]
fn ordering_does_not_start_inactive_or_restart_unchanged_dependencies() {
    let mut lamp = Lamp::new();
    let declarations =
        "After=lamp-middle.service lamp-zulu.service lamp-omega.service outside.service";
    install_part(&mut lamp, "alpha", "alpha 1", declarations, true);
    install_part(&mut lamp, "middle", "middle 1", "", false);
    install_part(&mut lamp, "omega", "omega 1", "", true);
    install_part(&mut lamp, "zulu", "zulu 1", "", true);
    let unchanged_pid = lamp.systemd.units[&name("zulu")].pid;
    publish_parts(
        &mut lamp,
        &[
            ("alpha", "alpha 2", declarations),
            ("middle", "middle 2", ""),
            ("omega", "omega 2", ""),
            ("zulu", "zulu 1", ""),
        ],
    );
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(restarts(&lamp), [name("omega"), name("alpha")]);
    assert!(!lamp.systemd.units.contains_key(&name("middle")));
    assert_eq!(lamp.systemd.units[&name("zulu")].pid, unchanged_pid);
    assert_eq!(
        lamp.read(&format!(
            "/opt/lampOS/middle/{}/lamp-middle",
            sha("middle 2")
        )),
        "middle 2",
        "the inactive part is updated without being started"
    );
}

#[test]
fn a_cycle_in_either_candidate_or_rollback_plan_refuses_before_mutation() {
    for old_is_cyclic in [false, true] {
        let mut lamp = Lamp::new();
        let cyclic = ["After=lamp-middle.service", "After=lamp-alpha.service"];
        let acyclic = ["", "After=lamp-alpha.service"];
        let old = if old_is_cyclic { cyclic } else { acyclic };
        let new = if old_is_cyclic { acyclic } else { cyclic };
        install_part(&mut lamp, "alpha", "alpha 1", old[0], true);
        install_part(&mut lamp, "middle", "middle 1", old[1], true);
        let before = lamp.lamp_files();
        publish_parts(
            &mut lamp,
            &[("alpha", "alpha 2", new[0]), ("middle", "middle 2", new[1])],
        );
        let outcome = lamp.round();
        assert!(
            matches!(&outcome, Outcome::Refused(_)),
            "old cycle {old_is_cyclic}: {outcome:?}"
        );
        assert_eq!(lamp.lamp_files(), before, "old cycle {old_is_cyclic}");
        assert!(
            lamp.calls().is_empty(),
            "old cycle {old_is_cyclic}: {:?}",
            lamp.calls()
        );
        assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    }
}

#[test]
fn a_cycle_in_an_older_recovery_journal_is_retained_without_arbitrary_restarts() {
    let mut lamp = Lamp::new();
    interrupted(
        &mut lamp,
        &[
            ("alpha", "After=lamp-middle.service"),
            ("middle", "After=lamp-alpha.service"),
        ],
        &[("alpha", ""), ("middle", "")],
        &[],
        &["middle", "alpha"],
    );
    let journal = lamp.read("/var/lib/lampos-update/journal.json");
    let outcome = lamp.round();
    assert!(
        matches!(
            &outcome,
            Outcome::Failed(_) | Outcome::RolledBack { after: Err(_), .. }
        ),
        "an unresolved recovery order must not report success: {outcome:?}"
    );
    assert!(
        lamp.exists("/var/lib/lampos-update/journal.json"),
        "{outcome:?}"
    );
    assert_eq!(lamp.read("/var/lib/lampos-update/journal.json"), journal);
    assert!(restarts(&lamp).is_empty(), "{:?}", lamp.calls());
    assert!(lamp.channel.fetched.is_empty());
}
