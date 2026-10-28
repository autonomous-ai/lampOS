//! Isolated #2948 review regression: a settings update of an existing
//! product lamp must retain a reader for its head pads.
use super::*;
use lamp_controls::touch_pins::TouchPinsConfig;
use lamp_update::install;

const PINS: &str = "/etc/lampOS/controls/controls-pins.json";
const OLD_PINS: &str = include_str!("../fixtures/pr2948-before/controls-pins.json");
const OLD_UNITS: [(&str, &str); 4] = [
    (
        "lamp-voice.service",
        include_str!("../fixtures/pr2948-before/lamp-voice.service"),
    ),
    (
        "lamp-expression.service",
        include_str!("../fixtures/pr2948-before/lamp-expression.service"),
    ),
    (
        "lamp-controls.service",
        include_str!("../fixtures/pr2948-before/lamp-controls.service"),
    ),
    (
        "lamp-body.service",
        include_str!("../fixtures/pr2948-before/lamp-body.service"),
    ),
];

fn old_product(settings: bool) -> Lamp {
    let mut lamp = Lamp::installed();
    assert!(!lamp.config.settings);
    assert!(!apply::on(None));
    // Install uses today's defaults; this fixture deliberately restores the
    // pre-carrier product updater, whose settings switch was absent.
    let updater = lamp.read("/etc/systemd/system/lamp-update.service");
    assert_eq!(
        unit::environment(&updater, apply::SWITCH).as_deref(),
        Some("on"),
        "the current factory product explicitly enables settings updates"
    );
    let setting = "Environment=LAMP_UPDATE_APPLY=on\n";
    assert_eq!(updater.matches(setting).count(), 1);
    fs::write(
        lamp.root.join("etc/systemd/system/lamp-update.service"),
        updater.replacen(setting, "", 1),
    )
    .unwrap();
    assert_eq!(
        unit::environment(
            &lamp.read("/etc/systemd/system/lamp-update.service"),
            apply::SWITCH
        ),
        None,
        "the product has not enabled settings updates by default"
    );
    // Keep install's real current links, binaries, settings and records;
    // restore the exact pre-#2948 head units and their recorded pins.
    for (name, old) in OLD_UNITS {
        assert!(unit::linked_program(old, &lamp.root).is_some(), "{name}");
        fs::write(lamp.root.join("etc/systemd/system").join(name), old).unwrap();
        lamp.systemd.start(name);
    }
    fs::write(lamp.root.join(PINS.trim_start_matches('/')), OLD_PINS).unwrap();
    let mut state = lamp.state();
    state["settings"][PINS] = sha(OLD_PINS).into();
    fs::write(lamp.records().join("state.json"), state.to_string()).unwrap();
    let pins = TouchPinsConfig::load(&lamp.root.join(PINS.trim_start_matches('/'))).unwrap();
    assert!(pins.legacy_head);
    assert_eq!(pins.head_pads.len(), 4);
    assert!(!head_voice(&lamp));
    assert!(!head_expression(&lamp));
    assert!(!head_body(&lamp));
    lamp.config.settings = settings;
    let (root, key) = (lamp.root.clone(), lamp.config.key);
    lamp.systemd.applier = Some(Box::new(move || {
        apply::run(
            &root,
            &root.join("var/lib/lampos-update"),
            &key,
            &mut |_| Ok(()),
            &mut |_| {},
        )
    }));
    lamp.calls.borrow_mut().clear();
    lamp
}

fn head_voice(lamp: &Lamp) -> bool {
    let voice = lamp.read("/etc/systemd/system/lamp-voice.service");
    unit::environment(&voice, "LAMP_HEAD_TOUCH").as_deref() == Some("on")
        && voice.lines().any(|line| {
            line.starts_with("ExecStart=")
                && line
                    .split_whitespace()
                    .any(|word| word == "--controls-mute")
        })
}

fn head_expression(lamp: &Lamp) -> bool {
    lamp.read("/etc/systemd/system/lamp-expression.service")
        .lines()
        .any(|line| {
            line.starts_with("ExecStart=")
                && line.split_whitespace().any(|word| word == "--head-touch")
        })
}

fn head_body(lamp: &Lamp) -> bool {
    unit::environment(
        &lamp.read("/etc/systemd/system/lamp-body.service"),
        "LAMP_HEAD_TOUCH",
    )
    .as_deref()
        == Some("on")
}

fn publish_carry(lamp: &mut Lamp) {
    let mut build = product_build(lamp, 2);
    // publish::make carries per-lamp, hash-addressed units, not product
    // current-link units. The round therefore gets no replacement units.
    assert!(build.units.is_empty());
    build.settings = install::product_settings();
    // Freeze the proposed #2948 pins independently of product defaults.
    let pins = build
        .settings
        .iter_mut()
        .find(|setting| setting.path == PINS)
        .unwrap();
    *pins = Setting::new(PINS, &OLD_PINS.replace("  \"legacy_head\": true,\n", ""));
    let new_pins = build
        .settings
        .iter()
        .find(|setting| setting.path == PINS)
        .unwrap();
    assert_ne!(new_pins.sha256.as_deref(), Some(sha(OLD_PINS).as_str()));
    needing(lamp, build, manifest::SETTINGS_UPDATER);
}

#[test]
fn pr2948_settings_off_preserves_the_old_head_reader() {
    let mut lamp = old_product(false);
    publish_carry(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(PINS), OLD_PINS);
    for (name, old) in OLD_UNITS {
        assert_eq!(lamp.read(&format!("/etc/systemd/system/{name}")), old);
    }
    assert!(
        !lamp
            .calls()
            .iter()
            .any(|call| call == &format!("restart {}", apply::UNIT))
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("settings are not written") && line.contains(apply::SWITCH))
    );
}

#[test]
fn pr2948_settings_on_keeps_a_head_reader_available() {
    let mut lamp = old_product(true);
    publish_carry(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    let pins = TouchPinsConfig::load(&lamp.root.join(PINS.trim_start_matches('/'))).unwrap();
    assert_eq!(
        pins.head_pads.len(),
        4,
        "the physical head inputs still exist"
    );
    assert!(
        lamp.calls()
            .iter()
            .any(|call| call == &format!("restart {}", apply::UNIT))
    );
    assert!(
        lamp.calls()
            .iter()
            .any(|call| call == "restart lamp-controls.service")
    );
    assert_eq!(lamp.state()["settings"][PINS], sha(&lamp.read(PINS)));
    for (name, old) in OLD_UNITS {
        assert_eq!(lamp.read(&format!("/etc/systemd/system/{name}")), old);
    }
    let voice = head_voice(&lamp);
    let expression = head_expression(&lamp);
    assert!(
        pins.legacy_head || (voice && expression),
        "settings apply left no head reader: legacy_head={}, Voice head+mute={voice}, Expression head={expression}; pins={} and product units stayed at efc8db9cb",
        pins.legacy_head,
        sha(&lamp.read(PINS)),
    );
}

/// Freeze the signed proposal independently of whatever install defaults the
/// build currently uses. Only real service directives gain the new readers.
fn reader_unit(name: &str, head: bool, mute: bool) -> String {
    let old = OLD_UNITS.iter().find(|(unit, _)| *unit == name).unwrap().1;
    let mut text = String::new();
    for line in old.lines() {
        if line.starts_with("ExecStart=")
            && matches!(name, "lamp-voice.service" | "lamp-body.service")
            && head
        {
            text.push_str("Environment=LAMP_HEAD_TOUCH=on\n");
        }
        text.push_str(line);
        if line.starts_with("ExecStart=") {
            if name == "lamp-voice.service" && mute {
                text.push_str(" --controls-mute /var/lib/lampOS/voice/mute/record.json");
            }
            if name == "lamp-expression.service" && head {
                text.push_str(" --head-touch");
            }
        }
        text.push('\n');
    }
    text
}

fn publish_readers(lamp: &mut Lamp, voice: bool, expression: bool, mute: bool) -> Manifest {
    let mut build = product_build(lamp, 2);
    build.settings = vec![Setting::new(PINS, OLD_PINS)];
    for (name, text) in [
        (
            "lamp-voice.service",
            reader_unit("lamp-voice.service", voice, mute),
        ),
        (
            "lamp-expression.service",
            reader_unit("lamp-expression.service", expression, false),
        ),
        (
            "lamp-body.service",
            reader_unit("lamp-body.service", voice, false),
        ),
    ] {
        let unit = Unit {
            lamp: lamp.config.lamp.clone(),
            unit: name.into(),
            sha256: sha(&text),
            size: text.len() as u64,
        };
        lamp.channel.files.insert(
            format!("{}{}", build.files_url, unit.file_name()),
            text.into_bytes(),
        );
        build.units.push(unit);
    }
    needing(lamp, build, manifest::SETTINGS_UPDATER)
}

fn etc_files(lamp: &Lamp) -> BTreeMap<String, Vec<u8>> {
    lamp.lamp_files()
        .into_iter()
        .filter(|(name, _)| name.starts_with("etc/"))
        .collect()
}

fn refused_without_writes(lamp: &mut Lamp) {
    let files = etc_files(lamp);
    let settings = lamp.state()["settings"].clone();
    let outcome = lamp.round();
    assert!(
        matches!(outcome, Outcome::Refused(_) | Outcome::WaitsForUnits(_)),
        "{outcome:?}; {:?}",
        lamp.said
    );
    assert_eq!(etc_files(lamp), files);
    assert_eq!(lamp.state()["settings"], settings);
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
}

#[test]
fn paired_head_readers_disable_legacy_even_when_signed_pins_enable_it() {
    let mut lamp = old_product(true);
    publish_readers(&mut lamp, true, true, true);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(head_voice(&lamp));
    assert!(head_expression(&lamp));
    assert!(head_body(&lamp));
    let pins = TouchPinsConfig::load(&lamp.root.join(PINS.trim_start_matches('/'))).unwrap();
    assert!(
        !pins.legacy_head,
        "paired readers must not run beside legacy gestures"
    );
    assert_eq!(pins.head_pads.len(), 4);
    assert_eq!(lamp.state()["settings"][PINS], sha(&lamp.read(PINS)));
}

#[test]
fn mixed_head_readers_are_refused_before_settings_or_units_are_written() {
    for (voice, expression) in [(true, false), (false, true)] {
        let mut lamp = old_product(true);
        publish_readers(&mut lamp, voice, expression, true);
        refused_without_writes(&mut lamp);
    }
}

#[test]
fn paired_head_readers_without_controls_mute_are_refused_before_writes() {
    let mut lamp = old_product(true);
    publish_readers(&mut lamp, true, true, false);
    refused_without_writes(&mut lamp);
}

#[test]
fn settings_off_refuses_paired_head_readers_with_legacy_pins() {
    let mut lamp = old_product(false);
    publish_readers(&mut lamp, true, true, true);
    refused_without_writes(&mut lamp);
}

#[test]
fn custom_head_wiring_is_kept_or_the_reader_transition_is_refused() {
    let mut lamp = old_product(true);
    let custom = OLD_PINS
        .replace("\"line\": 37", "\"line\": 38")
        .replace("\"legacy_head\": true", "\"legacy_head\": false");
    assert_ne!(custom, OLD_PINS);
    fs::write(lamp.root.join(PINS.trim_start_matches('/')), &custom).unwrap();
    let files = etc_files(&lamp);
    publish_readers(&mut lamp, true, true, true);
    let outcome = lamp.round();
    assert_eq!(
        lamp.read(PINS),
        custom,
        "factory wiring must remain byte-for-byte"
    );
    match outcome {
        Outcome::Switched(2) => {
            assert!(head_voice(&lamp));
            assert!(head_expression(&lamp));
            assert!(head_body(&lamp));
            let pins =
                TouchPinsConfig::load(&lamp.root.join(PINS.trim_start_matches('/'))).unwrap();
            assert!(!pins.legacy_head);
        }
        Outcome::Refused(_) | Outcome::WaitsForUnits(_) => {
            assert_eq!(etc_files(&lamp), files);
            assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
        }
        other => panic!("unexpected custom-pins outcome: {other:?}; {:?}", lamp.said),
    }
}

#[test]
fn failed_head_reader_candidate_restores_exact_old_pins_and_units() {
    let mut lamp = old_product(true);
    let files = etc_files(&lamp);
    let settings = lamp.state()["settings"].clone();
    let mut build = publish_readers(&mut lamp, true, true, true);
    // Only the candidate crashes; the old Expression can recover normally.
    let failed = "lamp-expression 2 crash";
    let file = build
        .files
        .iter_mut()
        .find(|file| file.name == "lamp-expression")
        .unwrap();
    file.sha256 = sha(failed);
    file.size = failed.len() as u64;
    lamp.channel.files.insert(
        format!("{}lamp-expression", build.files_url),
        failed.as_bytes().to_vec(),
    );
    needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, after: Ok(()), .. }
            if why.contains("lamp-expression.service")),
        "{outcome:?}; {:?}",
        lamp.said,
    );
    assert_eq!(lamp.read(PINS), OLD_PINS);
    assert_eq!(etc_files(&lamp), files);
    assert_eq!(lamp.state()["settings"], settings);
    assert!(!head_voice(&lamp));
    assert!(!head_expression(&lamp));
    assert!(!head_body(&lamp));
}

#[test]
fn old_apply_helper_pins_mismatch_is_put_back_before_readers_restart() {
    let mut lamp = old_product(true);
    let files = etc_files(&lamp);
    let settings = lamp.state()["settings"].clone();
    publish_readers(&mut lamp, true, true, true);
    let (root, key, calls) = (lamp.root.clone(), lamp.config.key, Rc::clone(&lamp.calls));
    let recovered = Rc::new(Cell::new(false));
    let saw_back = Rc::clone(&recovered);
    lamp.systemd.applier = Some(Box::new(move || {
        let records = root.join("var/lib/lampos-update");
        let folder = records.join(apply::FOLDER);
        let request: apply::Request =
            serde_json::from_slice(&fs::read(folder.join("request.json")).unwrap()).unwrap();
        if request.op == apply::Op::Back {
            assert!(
                !calls.borrow().iter().any(|call| {
                    call == "restart lamp-voice.service"
                        || call == "restart lamp-expression.service"
                        || call == "restart lamp-controls.service"
                        || call == "restart lamp-body.service"
                }),
                "unsafe reader units restarted before pins mismatch was put back: {:?}",
                calls.borrow(),
            );
        }
        apply::run(&root, &records, &key, &mut |_| Ok(()), &mut |_| {})?;
        if request.op == apply::Op::Apply {
            // Emulate an old helper that faithfully writes the signed legacy
            // pins, with matching bookkeeping, rather than deriving the pair.
            fs::write(root.join(PINS.trim_start_matches('/')), OLD_PINS).unwrap();
            let back_path = folder.join("back.json");
            let mut back: apply::Back =
                serde_json::from_slice(&fs::read(&back_path).unwrap()).unwrap();
            let pins = back
                .files
                .iter_mut()
                .find(|file| file.path == PINS)
                .unwrap();
            pins.wrote = Some(sha(OLD_PINS));
            fs::write(back_path, serde_json::to_vec(&back).unwrap()).unwrap();
            let done_path = folder.join("done.json");
            let mut done: apply::Done =
                serde_json::from_slice(&fs::read(&done_path).unwrap()).unwrap();
            done.records.insert(PINS.into(), Some(sha(OLD_PINS)));
            fs::write(done_path, serde_json::to_vec(&done).unwrap()).unwrap();
        } else {
            assert_eq!(
                fs::read_to_string(root.join(PINS.trim_start_matches('/'))).unwrap(),
                OLD_PINS
            );
            saw_back.set(true);
        }
        Ok(())
    }));
    let outcome = lamp.round();
    assert!(
        matches!(
            &outcome,
            Outcome::RolledBack {
                sequence: 2,
                after: Ok(()),
                ..
            }
        ),
        "{outcome:?}; {:?}",
        lamp.said
    );
    assert!(
        recovered.get(),
        "the mismatched helper's settings must be put back"
    );
    assert_eq!(etc_files(&lamp), files);
    assert_eq!(lamp.state()["settings"], settings);
}

#[test]
fn cut_after_pins_apply_restarts_unchanged_controls_after_restoring_old_pins() {
    let mut lamp = old_product(true);
    let files = etc_files(&lamp);
    let settings = lamp.state()["settings"].clone();
    let links: Vec<_> = ["voice", "expression", "controls", "body"]
        .into_iter()
        .map(|part| {
            let path = lamp.root.join(format!("opt/lampOS/{part}/current"));
            let target = fs::read_link(&path).unwrap();
            (path, target)
        })
        .collect();
    // Sequence 2 carries exactly the installed binaries, with no new units.
    // Only the pins poll interval changes, so recovery must not mistake
    // Controls' unchanged unit and link for an already recovered reader.
    let mut build = product_build(&mut lamp, 1);
    build.sequence = 2;
    assert!(build.units.is_empty());
    let candidate = OLD_PINS.replace("\"every_ms\": 5", "\"every_ms\": 6");
    assert_ne!(candidate, OLD_PINS);
    build.settings = vec![Setting::new(PINS, &candidate)];
    needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
    #[derive(Debug)]
    struct AfterApply;
    let first_apply = Rc::new(Cell::new(true));
    let cut_once = Rc::clone(&first_apply);
    let (root, key) = (lamp.root.clone(), lamp.config.key);
    lamp.systemd.applier = Some(Box::new(move || {
        let records = root.join("var/lib/lampos-update");
        apply::run(&root, &records, &key, &mut |_| Ok(()), &mut |_| {})?;
        if cut_once.replace(false) {
            std::panic::panic_any(AfterApply);
        }
        Ok(())
    }));
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lamp.round()));
    match cut {
        Err(payload) if payload.is::<AfterApply>() => {}
        Err(other) => std::panic::resume_unwind(other),
        Ok(outcome) => panic!("apply cut was not reached: {outcome:?}; {:?}", lamp.said),
    }
    assert!(!first_apply.get());
    let pins: serde_json::Value = serde_json::from_str(&lamp.read(PINS)).unwrap();
    assert_eq!(pins["every_ms"], 6);
    assert!(lamp.exists("/var/lib/lampos-update/journal.json"));
    for (name, old) in OLD_UNITS {
        assert_eq!(lamp.read(&format!("/etc/systemd/system/{name}")), old);
    }
    for (path, target) in &links {
        assert_eq!(fs::read_link(path).unwrap(), *target);
    }
    lamp.calls.borrow_mut().clear();
    let outcome = lamp.round();
    assert!(
        matches!(
            &outcome,
            Outcome::RolledBack {
                sequence: 2,
                after: Ok(()),
                ..
            }
        ),
        "{outcome:?}; {:?}",
        lamp.said,
    );
    assert_eq!(etc_files(&lamp), files);
    assert_eq!(lamp.state()["settings"], settings);
    let calls = lamp.calls();
    let applied = calls
        .iter()
        .position(|call| call == &format!("restart {}", apply::UNIT))
        .unwrap();
    let controls = calls
        .iter()
        .position(|call| call == "restart lamp-controls.service")
        .expect("unchanged Controls must reload restored pins");
    assert!(applied < controls, "{calls:?}");
    for (path, target) in links {
        assert_eq!(fs::read_link(path).unwrap(), target);
    }
}

#[test]
fn head_reader_mode_overrides_are_not_guessed_from_base_units() {
    for override_text in [
        "[Service]\nEnvironment=LAMP_HEAD_TOUCH=off\n",
        "[Service]\nEnvironment = LAMP_HEAD_TOUCH=off\n",
        "[Service]\nEnvironment=OTHER=x \\\n LAMP_HEAD_TOUCH=off\n",
        "[Service]\nEnvironmentFile=/etc/lampOS/local.env\n",
        "[Service]\nExecStart=\nExecStart=/usr/bin/other\n",
    ] {
        let mut lamp = old_product(true);
        publish_readers(&mut lamp, true, true, true);
        let path = lamp
            .root
            .join("etc/systemd/system/lamp-voice.service.d/local.conf");
        fs::write(path, override_text).unwrap();
        refused_without_writes(&mut lamp);
    }
}

#[test]
fn repeated_projected_pins_keep_their_actual_hash_without_restarting_controls() {
    let mut lamp = old_product(true);
    publish_carry(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    let first = lamp.read(PINS);
    assert_eq!(lamp.state()["settings"][PINS], sha(&first));
    // Same installed binaries and signed pins, newer sequence only.
    let mut build = product_build(&mut lamp, 2);
    build.sequence = 3;
    build.settings = vec![Setting::new(
        PINS,
        &OLD_PINS.replace("  \"legacy_head\": true,\n", ""),
    )];
    needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
    lamp.calls.borrow_mut().clear();
    assert_eq!(lamp.round(), Outcome::UpToDate(3));
    assert_eq!(lamp.read(PINS), first);
    assert_eq!(lamp.state()["settings"][PINS], sha(&first));
    assert!(
        !lamp
            .calls()
            .iter()
            .any(|call| call == "restart lamp-controls.service")
    );
}

#[test]
fn invalid_mute_path_and_unknown_reader_are_refused() {
    for replacement in ["--controls-mute relative.json", "--controls-mute"] {
        let mut lamp = old_product(true);
        let mut build = publish_readers(&mut lamp, true, true, true);
        let carried = build
            .units
            .iter_mut()
            .find(|unit| unit.unit == "lamp-voice.service")
            .unwrap();
        let url = format!("{}{}", build.files_url, carried.file_name());
        let text = String::from_utf8(lamp.channel.files[&url].clone())
            .unwrap()
            .replace(
                "--controls-mute /var/lib/lampOS/voice/mute/record.json",
                replacement,
            );
        carried.sha256 = sha(&text);
        carried.size = text.len() as u64;
        lamp.channel.files.insert(url, text.into_bytes());
        needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
        refused_without_writes(&mut lamp);
    }
    let mut lamp = old_product(true);
    fs::write(
        lamp.root.join("etc/systemd/system/lamp-voice.service"),
        "[Service]\nExecStart=/usr/bin/other\n",
    )
    .unwrap();
    publish_carry(&mut lamp);
    refused_without_writes(&mut lamp);
}

#[test]
fn head_pins_cannot_change_without_restarting_their_controls_reader() {
    let mut lamp = old_product(true);
    let mut build = publish_readers(&mut lamp, true, true, true);
    build.files.retain(|file| file.name != "lamp-controls");
    needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
    refused_without_writes(&mut lamp);
}

fn with_exec_suffix(text: &str, suffix: &str) -> String {
    text.lines()
        .map(|line| {
            if line.starts_with("ExecStart=") {
                format!("{line}{suffix}\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect()
}

fn replace_carried_unit(lamp: &mut Lamp, build: &mut Manifest, name: &str, text: String) {
    let carried = if let Some(at) = build.units.iter().position(|unit| unit.unit == name) {
        &mut build.units[at]
    } else {
        build.units.push(Unit {
            lamp: lamp.config.lamp.clone(),
            unit: name.into(),
            sha256: String::new(),
            size: 0,
        });
        build.units.last_mut().unwrap()
    };
    carried.sha256 = sha(&text);
    carried.size = text.len() as u64;
    lamp.channel.files.insert(
        format!("{}{}", build.files_url, carried.file_name()),
        text.into_bytes(),
    );
}

#[test]
fn isolated_voice_controls_bus_is_refused_before_any_write() {
    let mut lamp = old_product(true);
    let mut build = publish_readers(&mut lamp, true, true, true);
    let voice = with_exec_suffix(
        &reader_unit("lamp-voice.service", true, true),
        " --controls-bus-root /run/isolated-controls",
    );
    replace_carried_unit(&mut lamp, &mut build, "lamp-voice.service", voice);
    needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
    refused_without_writes(&mut lamp);
}

#[test]
fn expression_or_controls_bus_mismatch_is_refused_before_any_write() {
    for enabled in [false, true] {
        for name in ["lamp-expression.service", "lamp-controls.service"] {
            let mut lamp = old_product(true);
            let mut build = publish_readers(&mut lamp, enabled, enabled, enabled);
            let text = if name == "lamp-expression.service" {
                reader_unit(name, enabled, false)
            } else {
                OLD_UNITS
                    .iter()
                    .find(|(unit, _)| *unit == name)
                    .unwrap()
                    .1
                    .into()
            };
            assert!(text.contains("--bus-root /run/lampos-bus"));
            let text = text.replace(
                "--bus-root /run/lampos-bus",
                "--bus-root /run/isolated-controls",
            );
            replace_carried_unit(&mut lamp, &mut build, name, text);
            needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
            refused_without_writes(&mut lamp);
        }
    }
}

#[test]
fn matching_explicit_or_default_head_bus_roots_allow_the_paired_transition() {
    for explicit in [false, true] {
        let mut lamp = old_product(true);
        let mut build = publish_readers(&mut lamp, true, true, true);
        for name in [
            "lamp-voice.service",
            "lamp-expression.service",
            "lamp-controls.service",
            "lamp-body.service",
        ] {
            let text = if name == "lamp-controls.service" {
                OLD_UNITS
                    .iter()
                    .find(|(unit, _)| *unit == name)
                    .unwrap()
                    .1
                    .into()
            } else {
                reader_unit(name, true, name == "lamp-voice.service")
            };
            let text = text.replace(" --bus-root /run/lampos-bus", "");
            // Voice's privacy consumer uses LAMP_BUS_ROOT independently of
            // the head consumer's CLI override, so both must see this bus.
            let text = if name == "lamp-body.service" && explicit {
                text.replace(
                    "ExecStart=",
                    "Environment=LAMP_BUS_ROOT=/run/shared-head-controls\nExecStart=",
                )
            } else if name == "lamp-voice.service" {
                if explicit {
                    text.replace(
                        "Environment=LAMP_BUS_ROOT=/run/lampos-bus",
                        "Environment=LAMP_BUS_ROOT=/run/shared-head-controls",
                    )
                } else {
                    text.replace("Environment=LAMP_BUS_ROOT=/run/lampos-bus\n", "")
                }
            } else {
                text
            };
            let suffix = if !explicit || name == "lamp-body.service" {
                ""
            } else if name == "lamp-voice.service" {
                " --controls-bus-root /run/shared-head-controls"
            } else {
                " --bus-root /run/shared-head-controls"
            };
            replace_carried_unit(&mut lamp, &mut build, name, with_exec_suffix(&text, suffix));
        }
        needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
        assert_eq!(
            lamp.round(),
            Outcome::Switched(2),
            "explicit={explicit}: {:?}",
            lamp.said
        );
        let pins = TouchPinsConfig::load(&lamp.root.join(PINS.trim_start_matches('/'))).unwrap();
        assert!(!pins.legacy_head);
    }
}

fn active_build_pins(lamp: &mut Lamp, text: &str) -> String {
    let current = lamp.root.join("opt/lampOS/controls/current");
    let build = fs::read_link(current).unwrap();
    let path = format!("/opt/lampOS/controls/{}/custom-pins.json", build.display());
    fs::write(lamp.root.join(path.trim_start_matches('/')), text).unwrap();
    let unit_path = lamp.root.join("etc/systemd/system/lamp-controls.service");
    let unit = fs::read_to_string(&unit_path).unwrap();
    assert!(unit.contains(&format!("--pins {PINS}")));
    fs::write(
        unit_path,
        unit.replace(&format!("--pins {PINS}"), &format!("--pins {path}")),
    )
    .unwrap();
    path
}

fn installed_reader_pair(lamp: &mut Lamp, enabled: bool) {
    if enabled {
        for name in [
            "lamp-voice.service",
            "lamp-expression.service",
            "lamp-body.service",
        ] {
            let text = reader_unit(name, true, name == "lamp-voice.service");
            fs::write(lamp.root.join("etc/systemd/system").join(name), text).unwrap();
            lamp.systemd.start(name);
        }
        assert!(head_voice(lamp) && head_expression(lamp) && head_body(lamp));
    } else {
        assert!(!head_voice(lamp) && !head_expression(lamp) && !head_body(lamp));
    }
}

#[test]
fn absent_unused_product_pins_do_not_block_active_build_pins_updates() {
    for settings in [false, true] {
        for enabled in [false, true] {
            let mut lamp = old_product(settings);
            installed_reader_pair(&mut lamp, enabled);
            let active_pins = if enabled {
                OLD_PINS.replace("\"legacy_head\": true", "\"legacy_head\": false")
            } else {
                OLD_PINS.to_owned()
            };
            let active = active_build_pins(&mut lamp, &active_pins);
            fs::remove_file(lamp.root.join(PINS.trim_start_matches('/'))).unwrap();
            publish_carry(&mut lamp);
            assert_eq!(
                lamp.round(),
                Outcome::Switched(2),
                "settings={settings}, enabled={enabled}: {:?}",
                lamp.said
            );
            assert_eq!(lamp.read(&active), active_pins);
            assert_eq!(head_voice(&lamp), enabled);
            assert_eq!(head_expression(&lamp), enabled);
            assert_eq!(head_body(&lamp), enabled);
        }
    }
}

#[test]
fn stale_unused_product_pins_do_not_block_active_build_pins_updates() {
    for settings in [false, true] {
        for enabled in [false, true] {
            for stale in [OLD_PINS, "stale unused pins\n"] {
                let mut lamp = old_product(settings);
                installed_reader_pair(&mut lamp, enabled);
                let active_pins = if enabled {
                    OLD_PINS.replace("\"legacy_head\": true", "\"legacy_head\": false")
                } else {
                    OLD_PINS.to_owned()
                };
                let active = active_build_pins(&mut lamp, &active_pins);
                // Both a valid old product default and malformed stale bytes
                // are unused once Controls selects its own build-local pins.
                fs::write(lamp.root.join(PINS.trim_start_matches('/')), stale).unwrap();
                publish_carry(&mut lamp);
                assert_eq!(
                    lamp.round(),
                    Outcome::Switched(2),
                    "settings={settings}, enabled={enabled}, stale={stale:?}: {:?}",
                    lamp.said
                );
                assert_eq!(lamp.read(&active), active_pins);
                assert_eq!(head_voice(&lamp), enabled);
                assert_eq!(head_expression(&lamp), enabled);
                assert_eq!(head_body(&lamp), enabled);
                // A build-owned unused file may follow its signed settings
                // with apply on; a custom stale file must still be kept.
                if !settings || stale != OLD_PINS {
                    assert_eq!(lamp.read(PINS), stale);
                }
            }
        }
    }
}

#[test]
fn custom_active_pins_are_kept_and_checked_against_the_reader_pair() {
    for settings in [false, true] {
        for compatible in [false, true] {
            let mut lamp = old_product(settings);
            let custom = OLD_PINS.replace("\"line\": 37", "\"line\": 38");
            let custom = if compatible {
                custom.replace("\"legacy_head\": true", "\"legacy_head\": false")
            } else {
                custom
            };
            let active = active_build_pins(&mut lamp, &custom);
            // Keep the unused fixed file compatible with the new pair: it
            // cannot stand in for validation of the actual custom pins.
            let unused = OLD_PINS.replace("\"legacy_head\": true", "\"legacy_head\": false");
            fs::write(lamp.root.join(PINS.trim_start_matches('/')), unused).unwrap();
            publish_readers(&mut lamp, true, true, true);
            if compatible {
                assert_eq!(
                    lamp.round(),
                    Outcome::Switched(2),
                    "settings={settings}: {:?}",
                    lamp.said
                );
            } else {
                refused_without_writes(&mut lamp);
            }
            assert_eq!(
                lamp.read(&active),
                custom,
                "active custom wiring must stay byte-for-byte"
            );
        }
    }
}

#[test]
fn environment_only_head_bus_mismatch_is_refused_before_any_write() {
    for name in [
        "lamp-voice.service",
        "lamp-expression.service",
        "lamp-controls.service",
    ] {
        let mut lamp = old_product(true);
        let mut build = publish_readers(&mut lamp, true, true, true);
        let text = if name == "lamp-controls.service" {
            OLD_UNITS
                .iter()
                .find(|(unit, _)| *unit == name)
                .unwrap()
                .1
                .into()
        } else {
            reader_unit(name, true, name == "lamp-voice.service")
        };
        let text = text
            .replace(" --bus-root /run/lampos-bus", "")
            .replace("Environment=LAMP_BUS_ROOT=/run/lampos-bus\n", "")
            .replace(
                "ExecStart=",
                "Environment=LAMP_BUS_ROOT=/run/isolated-controls\nExecStart=",
            );
        replace_carried_unit(&mut lamp, &mut build, name, text);
        needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
        refused_without_writes(&mut lamp);
    }
}

#[test]
fn head_bus_root_dropins_are_not_guessed_from_base_units() {
    for name in [
        "lamp-voice.service",
        "lamp-expression.service",
        "lamp-controls.service",
    ] {
        let mut lamp = old_product(true);
        publish_readers(&mut lamp, true, true, true);
        let folder = lamp.root.join(format!("etc/systemd/system/{name}.d"));
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("bus.conf"),
            "[Service]\nEnvironment=LAMP_BUS_ROOT=/run/isolated-controls\n",
        )
        .unwrap();
        refused_without_writes(&mut lamp);
    }
}

#[test]
fn paired_head_bus_cannot_diverge_from_voices_enabled_privacy_bus() {
    let mut lamp = old_product(true);
    let mut build = publish_readers(&mut lamp, true, true, true);
    for name in [
        "lamp-voice.service",
        "lamp-expression.service",
        "lamp-controls.service",
        "lamp-body.service",
    ] {
        let text = if name == "lamp-controls.service" {
            OLD_UNITS
                .iter()
                .find(|(unit, _)| *unit == name)
                .unwrap()
                .1
                .into()
        } else {
            reader_unit(name, true, name == "lamp-voice.service")
        };
        let text = if name == "lamp-voice.service" {
            assert_eq!(
                unit::environment(&text, "LAMP_PRIVACY_CONTROLS").as_deref(),
                Some("on")
            );
            // Leave the independent privacy consumer on the product bus.
            with_exec_suffix(&text, " --controls-bus-root /run/shared-head-controls")
        } else if name == "lamp-body.service" {
            text.replace(
                "ExecStart=",
                "Environment=LAMP_BUS_ROOT=/run/shared-head-controls\nExecStart=",
            )
        } else {
            text.replace(
                "--bus-root /run/lampos-bus",
                "--bus-root /run/shared-head-controls",
            )
        };
        replace_carried_unit(&mut lamp, &mut build, name, text);
    }
    needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
    refused_without_writes(&mut lamp);
}

// Independent review controls for c062243b6da9b5dad14551d176ef79d81483758e.
// Append inside tests/round/head_touch.rs, using that head's private helpers.

#[test]
fn third_review_paired_head_readers_on_different_buses_are_refused() {
    let mut lamp = old_product(true);
    let mut build = publish_readers(&mut lamp, true, true, true);
    let voice = build
        .units
        .iter_mut()
        .find(|unit| unit.unit == "lamp-voice.service")
        .unwrap();
    let url = format!("{}{}", build.files_url, voice.file_name());
    let before = String::from_utf8(lamp.channel.files[&url].clone()).unwrap();
    assert!(!before.contains("--controls-bus-root"));
    assert_eq!(
        unit::environment(&before, "LAMP_BUS_ROOT").as_deref(),
        Some("/run/lampos-bus")
    );
    // Physical privacy still reads the normal bus. Only Voice's optional
    // hand pump and its voice/mute publisher move away from Controls and
    // Expression; the three enabling flags therefore do not prove a pair.
    let text = before
        .lines()
        .map(|line| {
            if line.starts_with("ExecStart=") {
                format!("{line} --controls-bus-root /run/lampos-voice/isolated-controls")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    voice.sha256 = sha(&text);
    voice.size = text.len() as u64;
    lamp.channel.files.insert(url, text.into_bytes());
    needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
    refused_without_writes(&mut lamp);
}

#[test]
fn third_review_binary_update_checks_active_build_local_head_pins() {
    let mut lamp = old_product(false);
    for (name, text) in [
        (
            "lamp-voice.service",
            reader_unit("lamp-voice.service", true, true),
        ),
        (
            "lamp-expression.service",
            reader_unit("lamp-expression.service", true, false),
        ),
        (
            "lamp-body.service",
            reader_unit("lamp-body.service", true, false),
        ),
    ] {
        fs::write(lamp.root.join("etc/systemd/system").join(name), text).unwrap();
        lamp.systemd.start(name);
    }

    let controls_path = "/etc/systemd/system/lamp-controls.service";
    let controls = lamp.read(controls_path);
    let old = unit::linked_program(&controls, &lamp.root).unwrap();
    let folder = old.folder();
    let active_pins_path = format!("{folder}controls-pins.json");
    let active_config_path = format!("{folder}controls.json");
    let active_pins = OLD_PINS.replace("\"legacy_head\": true", "\"legacy_head\": false");
    let active_config = lamp.read("/etc/lampOS/controls/controls.json");
    assert!(lamp.exists(&format!("{folder}lamp-controls")));
    fs::write(
        lamp.root.join(active_pins_path.trim_start_matches('/')),
        &active_pins,
    )
    .unwrap();
    fs::write(
        lamp.root.join(active_config_path.trim_start_matches('/')),
        &active_config,
    )
    .unwrap();
    // Match the tracked lamp-4ace layout: the executable and both settings
    // live in an actual installed hash folder. The product pins left under
    // /etc are unused and must not veto an unrelated binary update.
    let controls = controls
        .replace(&format!("{}current/", old.builds), &folder)
        .replace(PINS, &active_pins_path)
        .replace("/etc/lampOS/controls/controls.json", &active_config_path);
    assert!(unit::program(&controls).is_some());
    fs::write(
        lamp.root.join(controls_path.trim_start_matches('/')),
        &controls,
    )
    .unwrap();
    lamp.systemd.start("lamp-controls.service");
    assert!(head_voice(&lamp) && head_expression(&lamp) && head_body(&lamp));
    assert!(
        !TouchPinsConfig::load(&lamp.root.join(active_pins_path.trim_start_matches('/')))
            .unwrap()
            .legacy_head
    );
    assert_eq!(lamp.read(PINS), OLD_PINS);

    let build = product_build(&mut lamp, 2);
    assert!(build.settings.is_empty() && build.units.is_empty());
    needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);

    let after = lamp.read(controls_path);
    let new = unit::program(&after).unwrap();
    assert_ne!(new.folder(), folder, "Controls must actually take build 2");
    let new_pins_path = format!("{}controls-pins.json", new.folder());
    let new_config_path = format!("{}controls.json", new.folder());
    assert!(after.contains(&format!("--pins {new_pins_path}")));
    assert!(after.contains(&format!("--config {new_config_path}")));
    assert_eq!(lamp.read(&new_pins_path), active_pins);
    assert_eq!(lamp.read(&new_config_path), active_config);
    assert_eq!(lamp.read(PINS), OLD_PINS);
    assert!(head_voice(&lamp) && head_expression(&lamp) && head_body(&lamp));
    assert!(
        !lamp
            .calls()
            .iter()
            .any(|call| call == &format!("restart {}", apply::UNIT))
    );
}

#[test]
fn semantic_head_reader_paths_cannot_expand_invocation_variables() {
    for (name, flag, before, expanded) in [
        (
            "lamp-controls.service",
            "--pins",
            PINS,
            "/etc/lampOS/controls/${INVOCATION_ID}/pins.json",
        ),
        (
            "lamp-voice.service",
            "--controls-bus-root",
            "",
            "/run/head/${INVOCATION_ID}",
        ),
        (
            "lamp-voice.service",
            "--controls-mute",
            "/var/lib/lampOS/voice/mute/record.json",
            "/var/lib/lampOS/voice/mute/${INVOCATION_ID}/record.json",
        ),
        (
            "lamp-controls.service",
            "--bus-root",
            "/run/lampos-bus",
            "/run/head/${INVOCATION_ID}",
        ),
        (
            "lamp-expression.service",
            "--bus-root",
            "/run/lampos-bus",
            "/run/head/${INVOCATION_ID}",
        ),
    ] {
        let mut lamp = old_product(true);
        let mut build = publish_readers(&mut lamp, true, true, true);
        let text = if name == "lamp-controls.service" {
            OLD_UNITS
                .iter()
                .find(|(unit, _)| *unit == name)
                .unwrap()
                .1
                .to_owned()
        } else {
            reader_unit(name, true, name == "lamp-voice.service")
        };
        let text = if before.is_empty() {
            assert!(!text.contains(flag));
            with_exec_suffix(&text, &format!(" {flag} {expanded}"))
        } else {
            assert!(text.contains(&format!("{flag} {before}")));
            text.replace(&format!("{flag} {before}"), &format!("{flag} {expanded}"))
        };
        if flag == "--pins" {
            // No file exists at the literal spelling. Treating the expanded
            // argument as a missing inactive file would bypass the guard.
            assert!(!lamp.exists(expanded));
        }
        replace_carried_unit(&mut lamp, &mut build, name, text);
        needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
        refused_without_writes(&mut lamp);
    }
}

#[test]
fn textually_equal_expanded_head_bus_roots_are_not_a_verified_pair() {
    let mut lamp = old_product(true);
    let mut build = publish_readers(&mut lamp, true, true, true);
    let expanded = "/run/head/${INVOCATION_ID}";
    for name in [
        "lamp-voice.service",
        "lamp-expression.service",
        "lamp-controls.service",
    ] {
        let text = if name == "lamp-controls.service" {
            OLD_UNITS
                .iter()
                .find(|(unit, _)| *unit == name)
                .unwrap()
                .1
                .to_owned()
        } else {
            reader_unit(name, true, name == "lamp-voice.service")
        };
        let text = text.replace(
            "Environment=LAMP_PRIVACY_CONTROLS=on",
            "Environment=LAMP_PRIVACY_CONTROLS=off",
        );
        let text = if name == "lamp-voice.service" {
            assert_eq!(
                unit::environment(&text, "LAMP_PRIVACY_CONTROLS").as_deref(),
                Some("off")
            );
            with_exec_suffix(&text, &format!(" --controls-bus-root {expanded}"))
        } else {
            text.replace(
                "--bus-root /run/lampos-bus",
                &format!("--bus-root {expanded}"),
            )
        };
        // Each service gets a different systemd invocation identity. Equal
        // source strings therefore cannot establish a shared concrete bus.
        assert!(text.contains(expanded));
        replace_carried_unit(&mut lamp, &mut build, name, text);
    }
    needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
    refused_without_writes(&mut lamp);
}

#[test]
fn malformed_environment_quotes_are_not_reader_evidence() {
    for key in ["LAMP_HEAD_TOUCH", "LAMP_BUS_ROOT"] {
        let mut lamp = old_product(true);
        let mut build = publish_readers(&mut lamp, true, true, true);
        let voice = reader_unit("lamp-voice.service", true, true);
        let assignment = format!("Environment={key}=");
        assert!(voice.contains(&assignment));
        let voice = voice.replace(&assignment, &format!("Environment=\"{key}="));
        replace_carried_unit(&mut lamp, &mut build, "lamp-voice.service", voice);
        needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
        refused_without_writes(&mut lamp);
    }
}

#[test]
fn body_head_requires_an_enabled_carried_or_installed_sleep_reader() {
    for mode in ["off", "unset", "installed"] {
        let mut lamp = old_product(true);
        let mut build = publish_readers(&mut lamp, true, true, true);
        if mode == "installed" {
            // No carried replacement: the installed pre-head Body is off.
            build.units.retain(|unit| unit.unit != "lamp-body.service");
        } else {
            let body = reader_unit("lamp-body.service", true, false);
            let body = if mode == "off" {
                body.replace("LAMP_HEAD_TOUCH=on", "LAMP_HEAD_TOUCH=off")
            } else {
                body.replace("Environment=LAMP_HEAD_TOUCH=on\n", "")
            };
            replace_carried_unit(&mut lamp, &mut build, "lamp-body.service", body);
        }
        needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
        refused_without_writes(&mut lamp);
    }
}

#[test]
fn body_head_mode_and_bus_dropins_are_refused_before_writes() {
    for assignment in ["LAMP_HEAD_TOUCH=off", "LAMP_BUS_ROOT=/run/isolated-body"] {
        let mut lamp = old_product(true);
        publish_readers(&mut lamp, true, true, true);
        let folder = lamp.root.join("etc/systemd/system/lamp-body.service.d");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("30-local.conf"),
            format!("[Service]\nEnvironment={assignment}\n"),
        )
        .unwrap();
        refused_without_writes(&mut lamp);
    }
}

#[test]
fn body_head_on_an_isolated_bus_is_refused_before_writes() {
    let mut lamp = old_product(true);
    let mut build = publish_readers(&mut lamp, true, true, true);
    let body = reader_unit("lamp-body.service", true, false).replace(
        "ExecStart=",
        "Environment=LAMP_BUS_ROOT=/run/isolated-body\nExecStart=",
    );
    replace_carried_unit(&mut lamp, &mut build, "lamp-body.service", body);
    needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
    refused_without_writes(&mut lamp);
}

#[test]
fn body_head_repeated_exec_start_is_refused_before_writes() {
    let mut lamp = old_product(true);
    let mut build = publish_readers(&mut lamp, true, true, true);
    let body = reader_unit("lamp-body.service", true, false)
        .lines()
        .map(|line| {
            if line.starts_with("ExecStart=") {
                format!("{line}\n{line}\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect();
    replace_carried_unit(&mut lamp, &mut build, "lamp-body.service", body);
    needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
    refused_without_writes(&mut lamp);
}

#[test]
fn body_head_calibration_cannot_override_the_sleep_reader() {
    for assignment in ["LAMP_HEAD_TOUCH=off", "LAMP_BUS_ROOT=/run/isolated-body"] {
        let mut lamp = old_product(true);
        let path = lamp.root.join("etc/lampOS/body/lamp.env");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!("BODY_POSES=\"--home 2048,2048,2048\"\n{assignment}\n"),
        )
        .unwrap();
        // This is an existing local calibration, not a carried setting:
        // manifests cannot write Body's calibration environment files.
        publish_readers(&mut lamp, true, true, true);
        refused_without_writes(&mut lamp);
    }
}

#[test]
fn body_head_complete_bundle_preserves_literal_factory_calibration() {
    for environment_file in [
        "/etc/lampOS/body/lamp.env",
        "-/var/lib/lampos-body/measured.env",
    ] {
        let mut lamp = old_product(true);
        let path = environment_file.trim_start_matches('-');
        let calibration = "# Fixture poses, not a hardware qualification.\nBODY_POSES=\"--home 2048,2048,2048 --torque on\"\n";
        let on_lamp = lamp.root.join(path.trim_start_matches('/'));
        fs::create_dir_all(on_lamp.parent().unwrap()).unwrap();
        fs::write(&on_lamp, calibration).unwrap();
        let mut build = publish_readers(&mut lamp, true, true, true);
        let body = reader_unit("lamp-body.service", true, false).replace(
            "EnvironmentFile=/etc/lampOS/body/lamp.env",
            &format!("EnvironmentFile={environment_file}"),
        );
        assert!(body.contains("--unit-id %H") && body.contains("$BODY_POSES"));
        replace_carried_unit(&mut lamp, &mut build, "lamp-body.service", body);
        needing(&mut lamp, build, manifest::SETTINGS_UPDATER);
        assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
        assert!(head_voice(&lamp) && head_expression(&lamp) && head_body(&lamp));
        assert_eq!(lamp.read(path), calibration);
        let installed_body = lamp.read("/etc/systemd/system/lamp-body.service");
        assert!(installed_body.contains(&format!("EnvironmentFile={environment_file}")));
        assert!(installed_body.contains("--unit-id %H") && installed_body.contains("$BODY_POSES"));
        let pins = TouchPinsConfig::load(&lamp.root.join(PINS.trim_start_matches('/'))).unwrap();
        assert!(!pins.legacy_head);
    }
}
