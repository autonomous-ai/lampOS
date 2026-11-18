use super::*;
use crate::harness_install::product::{Source, built_in};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

fn none(_: &str) -> Option<String> {
    None
}

fn folder(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lamp-update-preinstall-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(path.join("records")).unwrap();
    path
}

fn json(path: &Path) -> serde_json::Value {
    let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "{}", path.display());
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// A product lamp with nothing set: Harness on by this build's default and
/// pinned to this build's manifest, Hermes and OpenClaw on, each shown the
/// skills, and the tasks and the one run path as lamp-update's switches
/// read them, exactly as `serve` asks for them.
#[test]
fn a_product_lamp_wants_all_three_by_the_builds_defaults() {
    for (tasks, one_path) in [(false, false), (true, false), (true, true)] {
        let switches = UpdateSwitches {
            engines: true,
            tasks,
            one_path,
        };
        let wanted = wanted(true, switches, &none);
        assert!(wanted.harness.on);
        assert_eq!(wanted.harness.source, Source::Build);
        assert_eq!(wanted.harness.pin, Ok(Some(built_in())));
        assert_eq!(
            wanted.hermes,
            (
                true,
                hermes_install::Shown {
                    skills: true,
                    tasks,
                    one_path,
                }
            )
        );
        assert_eq!(
            wanted.openclaw,
            (true, openclaw_install::shown(true, one_path))
        );
    }
}

/// Each switch set to `off` in lamp-update's environment turns its part
/// off; a lamp that is not the product's wants none.
#[test]
fn a_switch_off_or_a_lamp_not_the_products_wants_less() {
    let switches = UpdateSwitches::default();
    let off = |name: &'static str| move |asked: &str| (asked == name).then(|| "off".to_owned());
    assert!(
        !wanted(true, switches, &off(harness_install::SWITCH))
            .harness
            .on
    );
    assert!(
        !wanted(true, switches, &off(hermes_install::SWITCH))
            .hermes
            .0
    );
    assert!(
        !wanted(true, switches, &off(openclaw_install::SWITCH))
            .openclaw
            .0
    );
    let skills_off = wanted(true, switches, &off(hermes_install::SKILLS_SWITCH));
    assert!(skills_off.hermes.0 && !skills_off.hermes.1.skills);
    let elsewhere = wanted(false, switches, &none);
    assert!(!elsewhere.harness.on && !elsewhere.hermes.0 && !elsewhere.openclaw.0);
}

/// On an image with none of them, each part gets the ask its installer
/// writes, as a round would hand it over: Harness's with the pin, Hermes's
/// and OpenClaw's with what their namespace shows. 0600, in the one-shot's
/// order.
#[test]
fn an_image_with_none_asks_for_all_three_with_the_rounds_asks() {
    let root = folder("none");
    let records = root.join("records");
    let switches = UpdateSwitches {
        engines: true,
        tasks: true,
        one_path: true,
    };
    let wanted = wanted(true, switches, &none);
    let asked = ask_missing(&root, &records, &wanted).unwrap();
    assert_eq!(asked, ["Harness", "Hermes", "OpenClaw"]);

    let harness = json(&records.join(harness_install::ASK));
    assert_eq!(harness["version"], 1);
    assert_eq!(
        harness["pin"],
        serde_json::to_value(built_in()).unwrap(),
        "{harness}"
    );
    let hermes = json(&records.join(hermes_install::ASK));
    assert_eq!(
        (
            hermes["version"].clone(),
            hermes["skills"].clone(),
            hermes["tasks"].clone()
        ),
        (1.into(), true.into(), true.into()),
        "{hermes}"
    );
    assert_eq!(hermes["one_path"], true, "{hermes}");
    assert_eq!(hermes.get("remove"), None, "{hermes}");
    let openclaw = json(&records.join(openclaw_install::ASK));
    assert_eq!(
        (
            openclaw["version"].clone(),
            openclaw["skills"].clone(),
            openclaw["one_path"].clone()
        ),
        (1.into(), true.into(), true.into()),
        "{openclaw}"
    );
    assert_eq!(openclaw.get("remove"), None, "{openclaw}");
    // Nothing is there yet, so the check names the first part missing.
    assert_eq!(check(&root, &wanted), Err("Harness is not there".into()));
    fs::remove_dir_all(&root).unwrap();
}

/// Harness already on the image is not asked for again; Hermes and
/// OpenClaw still are.
#[test]
fn a_part_already_there_is_not_asked_for() {
    let root = folder("harness");
    let records = root.join("records");
    let harness = root.join(crate::install::HARNESS.trim_start_matches('/'));
    fs::create_dir_all(harness.parent().unwrap()).unwrap();
    fs::write(&harness, b"").unwrap();
    let wanted = wanted(true, UpdateSwitches::default(), &none);
    let asked = ask_missing(&root, &records, &wanted).unwrap();
    assert_eq!(asked, ["Hermes", "OpenClaw"]);
    assert!(!records.join(harness_install::ASK).exists());
    assert_eq!(check(&root, &wanted), Err("Hermes is not there".into()));
    fs::remove_dir_all(&root).unwrap();
}

/// Harness off asks for nothing at all: Hermes and OpenClaw ride on its
/// one-shot.
#[test]
fn harness_off_asks_for_nothing() {
    let root = folder("off");
    let records = root.join("records");
    let off = |asked: &str| (asked == harness_install::SWITCH).then(|| "off".to_owned());
    let wanted = wanted(true, UpdateSwitches::default(), &off);
    assert!(ask_missing(&root, &records, &wanted).is_err());
    assert_eq!(fs::read_dir(&records).unwrap().count(), 0);
    fs::remove_dir_all(&root).unwrap();
}

/// OpenClaw's gateway unit is written as a round writes it, so its install
/// finds the unit this build's; written once, it is not written again. With
/// OpenClaw off, no unit.
#[test]
fn openclaw_s_gateway_unit_is_written_as_a_round_writes_it() {
    let root = folder("unit");
    // An image's root has systemd's folder, as every lamp's does.
    fs::create_dir_all(root.join("etc/systemd/system")).unwrap();
    let switches = UpdateSwitches {
        engines: true,
        tasks: true,
        one_path: true,
    };
    let wanted = wanted(true, switches, &none);
    assert_eq!(write_units(&root, &wanted), Ok(true));
    let unit = root.join("etc/systemd/system").join(openclaw_install::UNIT);
    assert_eq!(
        fs::read_to_string(&unit).unwrap(),
        openclaw_install::unit_text_for(wanted.openclaw.1)
    );
    assert_eq!(write_units(&root, &wanted), Ok(false));
    fs::remove_dir_all(&root).unwrap();

    let root = folder("unit-off");
    fs::create_dir_all(root.join("etc/systemd/system")).unwrap();
    let off = |asked: &str| (asked == openclaw_install::SWITCH).then(|| "off".to_owned());
    let wanted = super::wanted(true, switches, &off);
    assert_eq!(write_units(&root, &wanted), Ok(false));
    assert!(
        !root
            .join("etc/systemd/system")
            .join(openclaw_install::UNIT)
            .exists()
    );
    fs::remove_dir_all(&root).unwrap();
}

/// On an image, a `systemctl` command is said and skipped as done; every
/// other command runs as given.
#[test]
fn on_an_image_systemctl_is_skipped_and_the_rest_runs() {
    let mut ran = Vec::new();
    let mut run = |args: &[String]| {
        ran.push(args.join(" "));
        Ok::<_, String>("out".to_owned())
    };
    let mut skipped = Vec::new();
    {
        let mut image = on_image(&mut run, &mut skipped);
        let args = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(
            image(&args("systemctl restart lamp-openclaw-gateway.service")),
            Ok(String::new())
        );
        assert_eq!(
            image(&args("runuser -u lampos -- true")),
            Ok("out".to_owned())
        );
    }
    assert_eq!(skipped, ["systemctl restart lamp-openclaw-gateway.service"]);
    assert_eq!(ran, ["runuser -u lampos -- true"]);
}

/// On an image, lamp-update's own unit's `Environment=` lines are what a
/// round there runs with, its drop-ins after it in name order: the
/// product's `LAMP_SKILL_TASKS=on` and `LAMP_SKILL_DEFAULT_AGENT=on` give
/// Hermes and OpenClaw the namespace a round on the lamp wants (lamp-dbda,
/// 16 Nov: read from no unit, both were installed shown without them, and
/// the first round found them not this build's).
#[test]
fn the_image_s_own_unit_gives_the_switches_a_round_runs_with() {
    let root = folder("unit-env");
    let units = root.join("etc/systemd/system");
    fs::create_dir_all(units.join("lamp-update.service.d")).unwrap();
    fs::write(
        units.join("lamp-update.service"),
        "[Unit]\nDescription=x\n[Service]\nEnvironment=LAMP_BUS_ROOT=/run/lampos-bus\n\
         Environment=LAMP_SKILL_TASKS=on\nEnvironment=LAMP_SKILL_DEFAULT_AGENT=on\n\
         Environment=LAMP_HERMES_SKILLS=on\nExecStart=/x\n",
    )
    .unwrap();
    fs::write(
        units.join("lamp-update.service.d/zz.conf"),
        "[Service]\nEnvironment=LAMP_HERMES_SKILLS=off\n",
    )
    .unwrap();
    let set = unit_environment(&root);
    let get = |name: &str| {
        set.iter()
            .find(|(set, _)| set == name)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(get("LAMP_SKILL_TASKS").as_deref(), Some("on"), "{set:?}");
    assert_eq!(get("LAMP_SKILL_DEFAULT_AGENT").as_deref(), Some("on"));
    // The drop-in comes after the unit.
    assert_eq!(get("LAMP_HERMES_SKILLS").as_deref(), Some("off"));

    let switches = crate::switches::update_switches(&root, true, |name| {
        get(name).map(std::ffi::OsString::from)
    });
    assert!(switches.tasks && switches.one_path, "{switches:?}");
    let wanted = wanted(true, switches, &get);
    assert_eq!(
        wanted.hermes.1,
        hermes_install::Shown {
            skills: false,
            tasks: true,
            one_path: true,
        }
    );
    assert_eq!(wanted.openclaw.1, openclaw_install::shown(true, true));
    fs::remove_dir_all(&root).unwrap();
}
