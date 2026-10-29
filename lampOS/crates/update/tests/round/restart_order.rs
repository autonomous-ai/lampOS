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
    // And the voice is stopped before any of them restarts (#3689).
    let calls = lamp.calls();
    let stopped = calls
        .iter()
        .position(|call| call == "stop lamp-voice.service")
        .expect("the voice is stopped first");
    let first_restart = calls
        .iter()
        .position(|call| call.starts_with("restart "))
        .unwrap();
    assert!(stopped < first_restart, "{calls:?}");
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

/// Add the proposed wire field to a reconstructed recovery prefix, without
/// requiring the current production Journal type to know it yet.
fn record_rollback_order(lamp: &Lamp, parts: &[&str]) {
    let path = lamp.root.join("var/lib/lampos-update/journal.json");
    let mut journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    journal["rollback_order"] =
        serde_json::json!(parts.iter().map(|part| name(part)).collect::<Vec<_>>());
    fs::write(path, serde_json::to_vec(&journal).unwrap()).unwrap();
}

#[test]
fn saved_rollback_order_survives_a_newly_unreadable_installed_unit() {
    let mut lamp = Lamp::new();
    install_part(
        &mut lamp,
        "alpha",
        "alpha 1",
        "After=lamp-zulu.service",
        true,
    );
    install_part(&mut lamp, "zulu", "zulu 1", "", true);
    install_part(&mut lamp, "middle", "middle 1", "", true);
    let before = lamp.lamp_files();
    let units = lamp.root.join("etc/systemd/system");
    let middle = units.join(name("middle"));
    let middle_before = fs::read(&middle).unwrap();
    let old_alpha = fs::read(units.join(name("alpha"))).unwrap();
    let old_zulu = fs::read(units.join(name("zulu"))).unwrap();
    publish_parts(
        &mut lamp,
        &[("alpha", "alpha 2", ""), ("zulu", "zulu 2, unwell", "")],
    );

    let unreadable = Rc::new(Cell::new(false));
    let repair = Rc::clone(&unreadable);
    let repair_path = middle.clone();
    // FakeEars reads every unit as UTF-8 to make health. Keep the read error
    // through restoration, then repair only after the original changed files
    // are back, on the existing stepped clock before its final health sample.
    lamp.talking = Some(Box::new(move |_| {
        if repair.replace(false) {
            assert_eq!(fs::read(&repair_path).unwrap(), [0xff]);
            assert_eq!(fs::read(units.join(name("alpha"))).unwrap(), old_alpha);
            assert_eq!(fs::read(units.join(name("zulu"))).unwrap(), old_zulu);
            fs::write(&repair_path, &middle_before).unwrap();
        }
        false
    }));
    let mut injected = false;
    let outcome = lamp.round_observing(&mut |line| {
        if line.starts_with("putting build 2 back:") {
            assert!(!injected);
            injected = true;
            fs::write(&middle, [0xff]).unwrap();
            assert!(fs::read_to_string(&middle).is_err());
            unreadable.set(true);
        }
    });
    assert!(injected, "the new build must reach rollback: {outcome:?}");
    assert!(
        matches!(&outcome, Outcome::RolledBack { after: Ok(()), .. }),
        "{outcome:?}"
    );
    assert!(
        !unreadable.get(),
        "the old files must precede fixture repair"
    );
    assert_eq!(
        restarts(&lamp),
        [name("alpha"), name("zulu"), name("zulu"), name("alpha")]
    );
    assert_eq!(lamp.lamp_files(), before);
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

#[test]
fn saved_rollback_order_keeps_transitive_edges_after_an_unchanged_unit_drifts() {
    let mut lamp = Lamp::new();
    install_part(
        &mut lamp,
        "alpha",
        "alpha 1",
        "After=lamp-middle.service",
        true,
    );
    install_part(
        &mut lamp,
        "middle",
        "middle 1",
        "After=lamp-zulu.service",
        true,
    );
    install_part(&mut lamp, "zulu", "zulu 1", "", true);
    let middle_pid = lamp.systemd.units[&name("middle")].pid;
    let units = lamp.root.join("etc/systemd/system");
    let old_alpha = fs::read(units.join(name("alpha"))).unwrap();
    let old_zulu = fs::read(units.join(name("zulu"))).unwrap();
    publish_parts(
        &mut lamp,
        &[("alpha", "alpha 2", ""), ("zulu", "zulu 2, unwell", "")],
    );
    // The old graph is Zulu -> unchanged Middle -> Alpha. This later edit
    // would make Alpha and Middle cyclic if rollback reread the live graph.
    let drift = text("middle", "middle 1", "After=lamp-alpha.service");
    let mut changed = false;
    let outcome = lamp.round_observing(&mut |line| {
        if line.starts_with("putting build 2 back:") {
            assert!(!changed);
            changed = true;
            fs::write(units.join(name("middle")), &drift).unwrap();
        }
    });
    assert!(changed, "{outcome:?}");
    assert!(
        matches!(&outcome, Outcome::RolledBack { after: Ok(()), .. }),
        "{outcome:?}"
    );
    assert_eq!(
        restarts(&lamp),
        [name("alpha"), name("zulu"), name("zulu"), name("alpha")]
    );
    assert_eq!(fs::read(units.join(name("alpha"))).unwrap(), old_alpha);
    assert_eq!(fs::read(units.join(name("zulu"))).unwrap(), old_zulu);
    assert_eq!(
        fs::read_to_string(units.join(name("middle"))).unwrap(),
        drift
    );
    assert_eq!(lamp.systemd.units[&name("middle")].pid, middle_pid);
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

#[test]
fn a_cut_at_reload_saves_the_full_old_projection_before_recovery() {
    let mut lamp = Lamp::new();
    install_part(
        &mut lamp,
        "alpha",
        "alpha 1",
        "After=lamp-middle.service",
        true,
    );
    install_part(
        &mut lamp,
        "middle",
        "middle 1",
        "After=lamp-zulu.service",
        true,
    );
    install_part(&mut lamp, "zulu", "zulu 1", "", true);
    install_part(
        &mut lamp,
        "sleeping",
        "sleeping 1",
        "Before=lamp-zulu.service",
        false,
    );
    let middle_pid = lamp.systemd.units[&name("middle")].pid;
    let units = lamp.root.join("etc/systemd/system");
    let saved: Vec<_> = ["alpha", "sleeping", "zulu"]
        .into_iter()
        .map(|part| (name(part), fs::read(units.join(name(part))).unwrap()))
        .collect();
    publish_parts(
        &mut lamp,
        &[
            ("alpha", "alpha 2", ""),
            ("sleeping", "sleeping 2", ""),
            ("zulu", "zulu 2", ""),
        ],
    );
    lamp.systemd.cut_at_reload = true;
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lamp.round()));
    assert!(
        cut.is_err(),
        "the real write/reload boundary must be reached"
    );
    assert!(restarts(&lamp).is_empty());
    let path = lamp.root.join("var/lib/lampos-update/journal.json");
    let mut journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        journal["rollback_order"],
        serde_json::json!([name("sleeping"), name("zulu"), name("alpha")])
    );
    let switching = journal["units"].as_array_mut().unwrap();
    let mut listed: Vec<_> = switching
        .iter()
        .map(|unit| unit["unit"].as_str().unwrap().to_owned())
        .collect();
    listed.sort();
    assert_eq!(listed, [name("alpha"), name("sleeping"), name("zulu")]);
    assert_eq!(
        switching
            .iter()
            .find(|unit| unit["unit"] == name("sleeping"))
            .unwrap()["was_active"],
        false
    );
    // Journal enumeration is not restart order, and already-written old
    // files remain the recovery source after the unchanged graph drifts.
    switching.reverse();
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    for (unit, bytes) in &saved {
        assert_eq!(
            fs::read(lamp.root.join("var/lib/lampos-update/previous").join(unit)).unwrap(),
            *bytes
        );
        assert_ne!(fs::read(units.join(unit)).unwrap(), *bytes);
    }
    fs::write(
        units.join(name("middle")),
        text("middle", "middle 1", "After=lamp-alpha.service"),
    )
    .unwrap();
    lamp.channel.fetched.clear();
    lamp.channel.offline = true;
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { after: Ok(()), .. }),
        "{outcome:?}"
    );
    assert_eq!(restarts(&lamp), [name("zulu"), name("alpha")]);
    for (unit, bytes) in saved {
        assert_eq!(fs::read(units.join(unit)).unwrap(), bytes);
    }
    assert_eq!(lamp.systemd.units[&name("middle")].pid, middle_pid);
    assert!(!lamp.systemd.units.contains_key(&name("sleeping")));
    assert!(lamp.channel.fetched.is_empty());
    assert!(!path.exists());
}

#[test]
fn saved_rollback_order_survives_an_interrupted_put_back_and_keeps_inactive_units_off() {
    let mut lamp = Lamp::new();
    let saved = interrupted(
        &mut lamp,
        &[
            ("alpha", "After=lamp-zulu.service"),
            ("zulu", ""),
            ("sleeping", "Before=lamp-zulu.service"),
            ("steady", "Before=lamp-alpha.service"),
        ],
        &[("alpha", ""), ("zulu", ""), ("sleeping", "")],
        &["sleeping"],
        &["zulu", "sleeping", "alpha", "steady"],
    );
    record_rollback_order(&lamp, &["sleeping", "steady", "zulu", "alpha"]);
    let journal = lamp.read("/var/lib/lampos-update/journal.json");
    let units = lamp.root.join("etc/systemd/system");
    let alpha_before = fs::read(units.join(name("alpha"))).unwrap();
    let steady_pid = lamp.systemd.units[&name("steady")].pid;
    // A directory blocks only Alpha's beside-file creation. The earlier
    // old unit writes really complete; this is a logical persisted prefix.
    let blocker = units.join(".lamp-alpha.service.lamp-update");
    fs::create_dir(&blocker).unwrap();
    let first = lamp.round();
    assert!(
        matches!(&first, Outcome::RolledBack { after: Err(_), .. }),
        "{first:?}"
    );
    assert_eq!(lamp.read("/var/lib/lampos-update/journal.json"), journal);
    assert_eq!(fs::read(units.join(name("alpha"))).unwrap(), alpha_before);
    for part in ["zulu", "sleeping", "steady"] {
        assert_eq!(
            fs::read_to_string(units.join(name(part))).unwrap(),
            saved[&name(part)]
        );
    }
    assert_eq!(restarts(&lamp), [name("zulu"), name("alpha")]);
    assert_eq!(lamp.systemd.units[&name("steady")].pid, steady_pid);
    assert!(!lamp.systemd.units.contains_key(&name("sleeping")));

    fs::remove_dir(&blocker).unwrap();
    install_part(
        &mut lamp,
        "middle",
        "middle 1",
        "After=lamp-middle.service",
        true,
    );
    lamp.calls.borrow_mut().clear();
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { after: Ok(()), .. }),
        "{outcome:?}"
    );
    assert_eq!(restarts(&lamp), [name("alpha")]);
    for (unit, text) in saved {
        assert_eq!(fs::read_to_string(units.join(unit)).unwrap(), text);
    }
    assert_eq!(lamp.systemd.units[&name("steady")].pid, steady_pid);
    assert!(!lamp.systemd.units.contains_key(&name("sleeping")));
    assert!(lamp.channel.fetched.is_empty());
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

#[test]
fn malformed_present_rollback_orders_retain_the_prefix_and_do_not_fetch() {
    let cases = [
        ("null", r#""rollback_order":null"#),
        ("boolean", r#""rollback_order":false"#),
        ("string", r#""rollback_order":"lamp-zulu.service""#),
        ("object", r#""rollback_order":{}"#),
        (
            "non-string entry",
            r#""rollback_order":["lamp-zulu.service",42]"#,
        ),
        (
            "duplicate entry",
            r#""rollback_order":["lamp-zulu.service","lamp-zulu.service"]"#,
        ),
        ("omission", r#""rollback_order":["lamp-zulu.service"]"#),
        (
            "extra unit",
            r#""rollback_order":["lamp-zulu.service","lamp-alpha.service","lamp-middle.service"]"#,
        ),
        (
            "unsafe name",
            r#""rollback_order":["lamp-zulu.service","../lamp-alpha.service"]"#,
        ),
        ("empty", r#""rollback_order":[]"#),
        (
            "duplicate property",
            r#""rollback_order":["lamp-zulu.service","lamp-alpha.service"],"rollback_order":["lamp-zulu.service","lamp-alpha.service"]"#,
        ),
    ];
    let mut failures = Vec::new();
    for (case, fields) in cases {
        let mut lamp = Lamp::new();
        let saved = interrupted(
            &mut lamp,
            &[("alpha", "After=lamp-zulu.service"), ("zulu", "")],
            &[("alpha", ""), ("zulu", "")],
            &[],
            &["alpha", "zulu"],
        );
        // A reachable valid channel makes a mistaken set-aside-and-proceed
        // visible, as well as a mistaken legacy recovery of this new field.
        publish_parts(
            &mut lamp,
            &[("alpha", "alpha 2", ""), ("zulu", "zulu 2", "")],
        );
        lamp.channel.offline = false;
        let path = lamp.root.join("var/lib/lampos-update/journal.json");
        let old = fs::read_to_string(&path).unwrap();
        let raw = format!("{}, {fields}}}", old.trim_end().strip_suffix('}').unwrap());
        assert!(
            serde_json::from_str::<serde_json::Value>(&raw).is_ok(),
            "{case}"
        );
        fs::write(&path, &raw).unwrap();
        let before = lamp.lamp_files();
        let state_path = lamp.root.join("var/lib/lampos-update/state.json");
        let state_before = fs::read(&state_path).ok();
        let outcome = lamp.round();
        let unchanged = fs::read(&path).ok().as_deref() == Some(raw.as_bytes())
            && lamp.lamp_files() == before
            && fs::read(&state_path).ok() == state_before
            && saved.iter().all(|(unit, text)| {
                fs::read_to_string(lamp.root.join("var/lib/lampos-update/previous").join(unit))
                    .ok()
                    .as_ref()
                    == Some(text)
            });
        if !matches!(
            &outcome,
            Outcome::Failed(_) | Outcome::RolledBack { after: Err(_), .. }
        ) || !unchanged
            || !lamp.calls().is_empty()
            || !lamp.channel.fetched.is_empty()
        {
            failures.push(format!(
                "{case}: {outcome:?}; prefix unchanged={unchanged}; calls={:?}; fetched={:?}",
                lamp.calls(),
                lamp.channel.fetched
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_legacy_journal_uses_saved_declarations_and_unchanged_transitive_dependencies() {
    let mut lamp = Lamp::new();
    let saved = interrupted(
        &mut lamp,
        &[("alpha", "After=lamp-middle.service"), ("zulu", "")],
        &[("alpha", ""), ("zulu", "After=lamp-alpha.service")],
        &[],
        &["alpha", "zulu"],
    );
    install_part(
        &mut lamp,
        "middle",
        "middle 1",
        "After=lamp-zulu.service",
        true,
    );
    let middle_pid = lamp.systemd.units[&name("middle")].pid;
    let journal: serde_json::Value =
        serde_json::from_str(&lamp.read("/var/lib/lampos-update/journal.json")).unwrap();
    assert!(journal.get("rollback_order").is_none());
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { after: Ok(()), .. }),
        "{outcome:?}"
    );
    assert_eq!(restarts(&lamp), [name("zulu"), name("alpha")]);
    for (unit, text) in saved {
        assert_eq!(lamp.read(&format!("/etc/systemd/system/{unit}")), text);
    }
    assert_eq!(lamp.systemd.units[&name("middle")].pid, middle_pid);
    assert!(lamp.channel.fetched.is_empty());
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

#[test]
fn empty_saved_order_recovers_files_and_keeps_live_builds_when_inventory_is_unreadable() {
    let mut lamp = Lamp::new();
    let records = lamp.root.join("var/lib/lampos-update");
    fs::create_dir_all(&records).unwrap();
    // This live build is not one of the switch's saved units. A failed
    // inventory read must not turn pruning's keep-set into an empty one.
    let live = format!("/opt/lampOS/body/{}", sha("body 1"));
    fs::write(
        records.join("state.json"),
        serde_json::to_vec(&serde_json::json!({
            "installed":{"sequence":1,"commit":"1".repeat(40)},
            "previous":null,"refused":[],"made":[live]
        }))
        .unwrap(),
    )
    .unwrap();
    let skill = "/var/lib/lampOS/skills/dance/skill.toml";
    let path = lamp.root.join(skill.trim_start_matches('/'));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "new skill").unwrap();
    fs::write(
        records.join("journal.json"),
        serde_json::to_vec(&serde_json::json!({
            "build":{"sequence":2,"commit":"2".repeat(40)},
            "units":[], "rollback_order":[],
            "skills":[{"path":skill,"before":"old skill","wrote":sha("new skill")}]
        }))
        .unwrap(),
    )
    .unwrap();
    let unreadable = lamp
        .root
        .join("etc/systemd/system/lamp-cmedia-speaker.service");
    fs::write(&unreadable, [0xff]).unwrap();
    lamp.channel.offline = true;
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { after: Ok(()), .. }),
        "{outcome:?}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "old skill");
    assert_eq!(fs::read(&unreadable).unwrap(), [0xff]);
    assert_eq!(lamp.read(&format!("{live}/lamp-body")), "body 1");
    assert_eq!(lamp.state()["made"], serde_json::json!([live]));
    assert!(restarts(&lamp).is_empty());
    assert!(lamp.channel.fetched.is_empty());
    assert!(!records.join("journal.json").exists());
}
