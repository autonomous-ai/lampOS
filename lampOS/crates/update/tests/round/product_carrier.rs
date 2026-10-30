//! Signed product service units use the existing quiet switch and fallback.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn carried(
    lamp: &mut Lamp,
    text: &str,
    eyes: &str,
    specific: Option<&str>,
    settings: Vec<Setting>,
) {
    let mut build = lamp.manifest(
        2,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
            ("lamp-eyes", eyes),
        ],
        &specific
            .map(|unit| vec![("lamp-eyes.service", unit)])
            .unwrap_or_default(),
    );
    let unit = Unit {
        lamp: manifest::PRODUCT_LAMP.into(),
        unit: "lamp-eyes.service".into(),
        sha256: sha(text),
        size: text.len() as u64,
    };
    lamp.channel.files.insert(
        format!("{}{}", build.files_url, unit.file_name()),
        text.as_bytes().to_vec(),
    );
    // Put the less-specific entry last: manifest order cannot override a lamp.
    build.units.push(unit);
    build.settings = settings;
    build.min_updater = Some(manifest::PRODUCT_UNITS_UPDATER);
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&serde_json::to_vec(&build).unwrap(), &lamp.key).unwrap(),
    );
}

fn new_eyes_unit() -> String {
    format!("{}Environment=LAMP_BUS_ROOT=/run/lampos-bus\n", eyes_unit())
}

#[test]
fn a_product_unit_switches_with_its_link_and_leaves_local_dropins_alone() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    let own = lamp.read("/etc/systemd/system/lamp-update.service");
    let dropins = lamp.root.join("etc/systemd/system/lamp-eyes.service.d");
    fs::create_dir(&dropins).unwrap();
    fs::write(dropins.join("local.conf"), "[Service]\nNice=4\n").unwrap();
    let new = new_eyes_unit();
    carried(&mut lamp, &new, "eyes 2", None, Vec::new());
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), new);
    assert_eq!(lamp.eyes_link(), sha("eyes 2"));
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-eyes.service.d/local.conf"),
        "[Service]\nNice=4\n"
    );
    assert_eq!(lamp.read("/etc/systemd/system/lamp-update.service"), own);
    assert_eq!(lamp.calls(), ["reload", "restart lamp-eyes.service"]);
}

#[test]
fn the_lamps_specific_unit_wins_over_the_product_unit() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    let specific = format!("{}Environment=LAMP_BUS_ROOT=/run/this-lamp\n", eyes_unit());
    carried(
        &mut lamp,
        &new_eyes_unit(),
        "eyes 2",
        Some(&specific),
        Vec::new(),
    );
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), specific);
}

#[test]
fn an_unhealthy_product_build_restores_the_exact_unit_and_link() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    let before = lamp.lamp_files();
    carried(
        &mut lamp,
        &new_eyes_unit(),
        "eyes 2 which crashes",
        None,
        Vec::new(),
    );
    assert!(
        matches!(
            lamp.round(),
            Outcome::RolledBack {
                sequence: 2,
                after: Ok(()),
                ..
            }
        ),
        "{:?}",
        lamp.said
    );
    assert_eq!(lamp.lamp_files(), before);
    assert_eq!(lamp.eyes_link(), sha("eyes 1"));
}

#[test]
fn a_corrupt_product_unit_is_refused_before_any_write() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    let before = lamp.lamp_files();
    carried(&mut lamp, &new_eyes_unit(), "eyes 2", None, Vec::new());
    lamp.channel.files.insert(
        "https://channel.test/build-2/product.lamp-eyes.service".into(),
        b"changed after signing".to_vec(),
    );
    assert!(matches!(lamp.round(), Outcome::Refused(_)));
    assert_eq!(lamp.lamp_files(), before);
    assert!(lamp.calls().is_empty());
}

#[test]
fn a_product_template_does_not_replace_a_pinned_lamps_unit() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    let pinned = eyes_unit().replace("/current/", &format!("/{}/", sha("eyes 1")));
    fs::write(
        lamp.root.join("etc/systemd/system/lamp-eyes.service"),
        &pinned,
    )
    .unwrap();
    carried(&mut lamp, &new_eyes_unit(), "eyes 2", None, Vec::new());
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-eyes.service"),
        pinned.replace(&sha("eyes 1"), &sha("eyes 2"))
    );
    assert!(
        !lamp
            .channel
            .fetched
            .iter()
            .any(|url| url.ends_with("product.lamp-eyes.service"))
    );
}

#[test]
fn a_product_template_cannot_change_a_parts_binary_or_build_folder() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    let before = lamp.lamp_files();
    let wrong = eyes_unit().replace("eyes/current/lamp-eyes", "voice/current/lamp-voice");
    carried(&mut lamp, &wrong, "eyes 2", None, Vec::new());
    assert!(matches!(lamp.round(), Outcome::Refused(_)));
    assert_eq!(lamp.lamp_files(), before);
    assert!(lamp.calls().is_empty());
}

fn settings_lamp(auto: bool, managed: bool) -> Lamp {
    let (mut lamp, _) = lamp_with_settings();
    lamp = lamp.with_linked_eyes("eyes 1");
    lamp.config.settings = false;
    lamp.config.product_settings = auto;
    lamp.config.self_switch = true;
    if managed {
        let hash = sha("update 1");
        let folder = lamp.root.join(format!("opt/lampOS/update/{hash}"));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("lamp-update"), "update 1").unwrap();
        std::os::unix::fs::symlink(hash, lamp.root.join("opt/lampOS/update/current")).unwrap();
        fs::write(
            lamp.root.join("etc/systemd/system/lamp-update.service"),
            "[Service]\nExecStart=/opt/lampOS/update/current/lamp-update serve\n",
        )
        .unwrap();
    }
    lamp
}

#[test]
fn a_managed_product_manifest_can_bootstrap_the_existing_settings_helper() {
    let mut lamp = settings_lamp(true, true);
    carried(
        &mut lamp,
        &new_eyes_unit(),
        "eyes 1",
        None,
        vec![Setting::new(CONTROLS_JSON, "controls 2")],
    );
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(CONTROLS_JSON), "controls 2");
    assert!(lamp.calls().contains(&format!("restart {}", apply::UNIT)));
}

#[test]
fn explicit_off_and_an_unmanaged_updater_do_not_bootstrap_settings() {
    for (auto, managed) in [(false, true), (true, false)] {
        let mut lamp = settings_lamp(auto, managed);
        carried(
            &mut lamp,
            &new_eyes_unit(),
            "eyes 1",
            None,
            vec![Setting::new(CONTROLS_JSON, "controls 2")],
        );
        assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
        assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");
        assert!(!lamp.calls().iter().any(|call| call.contains(apply::UNIT)));
    }
}

#[test]
fn automatic_settings_do_not_change_an_older_non_product_manifest() {
    let mut lamp = settings_lamp(true, true);
    publish_settings(
        &mut lamp,
        2,
        vec![Setting::new(CONTROLS_JSON, "controls 2")],
    );
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");
    assert!(!lamp.calls().iter().any(|call| call.contains(apply::UNIT)));
}

#[test]
fn an_older_updater_climbs_before_the_product_switch_and_fallback() {
    for crashes in [false, true] {
        let mut lamp = settings_lamp(true, true);
        lamp.config.updater = manifest::PRODUCT_UNITS_UPDATER - 1;
        lamp.systemd.self_recovery = Ok(());
        let recovery = lamp.root.join(own::BACK.trim_start_matches('/'));
        fs::create_dir_all(recovery.parent().unwrap()).unwrap();
        fs::write(&recovery, "the independently installed recovery executable").unwrap();
        fs::set_permissions(&recovery, fs::Permissions::from_mode(0o755)).unwrap();
        let own_unit = lamp.read("/etc/systemd/system/lamp-update.service");
        let new_eyes = if crashes {
            "eyes 2 which crashes"
        } else {
            "eyes 2"
        };
        carried(
            &mut lamp,
            &new_eyes_unit(),
            new_eyes,
            None,
            vec![Setting::new(CONTROLS_JSON, "controls 2")],
        );
        let envelope = &lamp.channel.files[&format!("{CHANNEL}{}", manifest::ENVELOPE)];
        let bytes = manifest::signed(envelope, &lamp.config.key).unwrap();
        let mut build = manifest::parse(&bytes).unwrap();
        let updater = "update 2";
        build.files.push(File {
            name: "lamp-update".into(),
            sha256: sha(updater),
            size: updater.len() as u64,
        });
        lamp.channel.files.insert(
            format!("{}lamp-update", build.files_url),
            updater.as_bytes().to_vec(),
        );
        lamp.channel.files.insert(
            format!("{CHANNEL}{}", manifest::ENVELOPE),
            manifest::seal(&serde_json::to_vec(&build).unwrap(), &lamp.key).unwrap(),
        );

        assert_eq!(
            lamp.round(),
            Outcome::Climbs {
                sequence: 2,
                needed: manifest::PRODUCT_UNITS_UPDATER,
                from: sha("update 1"),
                to: sha(updater)
            },
            "{:?}",
            lamp.said
        );
        assert_eq!(lamp.eyes_link(), sha("eyes 1"));
        assert_eq!(
            lamp.read("/etc/systemd/system/lamp-eyes.service"),
            eyes_unit()
        );
        assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");

        // A dry confirmation validates units and settings, but writes neither.
        lamp.config.updater = manifest::UPDATER;
        assert_eq!(
            lamp.confirm(&sha(updater)),
            own::Confirm::Confirmed,
            "{:?}",
            lamp.said
        );
        assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");
        assert_eq!(
            lamp.read("/etc/systemd/system/lamp-eyes.service"),
            eyes_unit()
        );
        let outcome = lamp.round();
        if crashes {
            assert!(
                matches!(
                    outcome,
                    Outcome::RolledBack {
                        sequence: 2,
                        after: Ok(()),
                        ..
                    }
                ),
                "{outcome:?}: {:?}",
                lamp.said
            );
            assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");
            assert_eq!(
                lamp.read("/etc/systemd/system/lamp-eyes.service"),
                eyes_unit()
            );
            assert_eq!(lamp.eyes_link(), sha("eyes 1"));
        } else {
            assert_eq!(outcome, Outcome::Switched(2), "{:?}", lamp.said);
            assert_eq!(lamp.read(CONTROLS_JSON), "controls 2");
            assert_eq!(
                lamp.read("/etc/systemd/system/lamp-eyes.service"),
                new_eyes_unit()
            );
            assert_eq!(lamp.eyes_link(), sha(new_eyes));
        }
        assert_eq!(lamp.own_build(), sha(updater));
        assert!(lamp.pending().is_none());
        assert_eq!(
            lamp.read("/etc/systemd/system/lamp-update.service"),
            own_unit
        );
    }
}

/// second-P1: an updater of level 8, which restarts lamp-controls before
/// the voice, is offered a build that asks for `ORDER_UPDATER`. It takes
/// only the lamp-update the build carries and switches no part; once that
/// one confirms itself, it switches the parts in its own order.
#[test]
fn an_updater_of_level_eight_climbs_before_it_switches_any_part() {
    let mut lamp = settings_lamp(true, true);
    lamp.config.updater = manifest::ADDS_UPDATER;
    lamp.systemd.self_recovery = Ok(());
    let recovery = lamp.root.join(own::BACK.trim_start_matches('/'));
    fs::create_dir_all(recovery.parent().unwrap()).unwrap();
    fs::write(&recovery, "the independently installed recovery executable").unwrap();
    fs::set_permissions(&recovery, fs::Permissions::from_mode(0o755)).unwrap();
    carried(
        &mut lamp,
        &new_eyes_unit(),
        "eyes 2",
        None,
        vec![Setting::new(CONTROLS_JSON, "controls 2")],
    );
    let envelope = &lamp.channel.files[&format!("{CHANNEL}{}", manifest::ENVELOPE)];
    let bytes = manifest::signed(envelope, &lamp.config.key).unwrap();
    let mut build = manifest::parse(&bytes).unwrap();
    build.min_updater = Some(manifest::ORDER_UPDATER);
    let updater = "update 2";
    build.files.push(File {
        name: "lamp-update".into(),
        sha256: sha(updater),
        size: updater.len() as u64,
    });
    lamp.channel.files.insert(
        format!("{}lamp-update", build.files_url),
        updater.as_bytes().to_vec(),
    );
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&serde_json::to_vec(&build).unwrap(), &lamp.key).unwrap(),
    );

    assert_eq!(
        lamp.round(),
        Outcome::Climbs {
            sequence: 2,
            needed: manifest::ORDER_UPDATER,
            from: sha("update 1"),
            to: sha(updater)
        },
        "{:?}",
        lamp.said
    );
    // Nothing of the parts moved under the old updater.
    assert_eq!(lamp.eyes_link(), sha("eyes 1"));
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-eyes.service"),
        eyes_unit()
    );
    assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");
    assert!(
        !lamp.calls().iter().any(|call| call.contains("restart")),
        "{:?}",
        lamp.calls()
    );

    // The climbed updater confirms itself, then switches the parts.
    lamp.config.updater = manifest::UPDATER;
    assert_eq!(
        lamp.confirm(&sha(updater)),
        own::Confirm::Confirmed,
        "{:?}",
        lamp.said
    );
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.eyes_link(), sha("eyes 2"));
    assert_eq!(lamp.read(CONTROLS_JSON), "controls 2");
    assert_eq!(lamp.own_build(), sha(updater));
}

#[test]
fn a_comment_only_product_unit_is_an_intended_noop() {
    let mut lamp = settings_lamp(false, true);
    let before = eyes_unit();
    let comment = format!("{before}# carrier no-op control\n");
    assert!(unit::same(&before, &comment));
    carried(&mut lamp, &comment, "eyes 1", None, Vec::new());
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), before);
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
    assert!(lamp.calls().is_empty());
}

// The shared fake starts text binaries, not an argument parser. Only this
// adapter models the BAD unit's rejected argument; restored GOOD starts normally.
struct RejectCarrierArgument<'a>(&'a mut FakeSystemd);

impl Systemd for RejectCarrierArgument<'_> {
    fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String> {
        self.0.states(units)
    }

    fn self_recovery_ready(&mut self) -> Result<(), String> {
        self.0.self_recovery_ready()
    }

    fn reload(&mut self) -> Result<(), String> {
        self.0.reload()
    }

    fn restart(&mut self, name: &str) -> Result<(), String> {
        let text = fs::read_to_string(self.0.root.join("etc/systemd/system").join(name)).unwrap();
        let rejects = text.lines().any(|line| {
            line.starts_with("ExecStart=") && line.ends_with(" --product-carrier-test-invalid")
        });
        if rejects {
            self.0.crashing.push(name.into());
        }
        let result = self.0.restart(name);
        if rejects {
            assert_eq!(self.0.crashing.pop().as_deref(), Some(name));
        }
        result
    }

    fn last_line(&mut self, name: &str) -> Option<String> {
        self.0.last_line(name)
    }
}

#[test]
fn a_same_binary_product_unit_records_good_only_after_climb_and_confirmation() {
    let mut lamp = settings_lamp(false, true);
    lamp.config.updater = manifest::PRODUCT_UNITS_UPDATER - 1;
    lamp.systemd.self_recovery = Ok(());
    let recovery = lamp.root.join(own::BACK.trim_start_matches('/'));
    fs::create_dir_all(recovery.parent().unwrap()).unwrap();
    fs::write(&recovery, "the independently installed recovery executable").unwrap();
    fs::set_permissions(&recovery, fs::Permissions::from_mode(0o755)).unwrap();
    let before = eyes_unit();
    let good = before.replace(
        "[Service]\n",
        "[Service]\nEnvironment=LAMP_CARRIER_TEST=1\n",
    );
    assert_eq!(unit::environment(&before, "LAMP_CARRIER_TEST"), None);
    assert!(!unit::same(&before, &good));
    let exec = |text: &str| {
        text.lines()
            .find(|line| line.starts_with("ExecStart="))
            .unwrap()
            .to_owned()
    };
    assert_eq!(exec(&before), exec(&good));
    assert_eq!(lamp.state()["installed"]["sequence"], 1);
    carried(&mut lamp, &good, "eyes 1", None, Vec::new());
    let envelope = &lamp.channel.files[&format!("{CHANNEL}{}", manifest::ENVELOPE)];
    let bytes = manifest::signed(envelope, &lamp.config.key).unwrap();
    let mut build = manifest::parse(&bytes).unwrap();
    let updater = "update 2";
    build.files.push(File {
        name: "lamp-update".into(),
        sha256: sha(updater),
        size: updater.len() as u64,
    });
    lamp.channel.files.insert(
        format!("{}lamp-update", build.files_url),
        updater.as_bytes().to_vec(),
    );
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&serde_json::to_vec(&build).unwrap(), &lamp.key).unwrap(),
    );
    assert_eq!(
        lamp.round(),
        Outcome::Climbs {
            sequence: 2,
            needed: manifest::PRODUCT_UNITS_UPDATER,
            from: sha("update 1"),
            to: sha(updater),
        },
        "{:?}",
        lamp.said
    );
    assert_eq!(lamp.state()["installed"]["sequence"], 1);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), before);
    assert_eq!(lamp.eyes_link(), sha("eyes 1"));
    lamp.config.updater = manifest::UPDATER;
    assert_eq!(lamp.confirm(&sha(updater)), own::Confirm::Confirmed);
    assert_eq!(lamp.state()["installed"]["sequence"], 1);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), before);
    assert_eq!(lamp.eyes_link(), sha("eyes 1"));
    lamp.calls.borrow_mut().clear();
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), good);
    assert_eq!(lamp.eyes_link(), sha("eyes 1"));
    assert_eq!(lamp.calls(), ["reload", "restart lamp-eyes.service"]);

    // Same binary again: only BAD's argument fails startup, so rollback must
    // restore the exact GOOD unit and retain GOOD's installed sequence.
    let bad = good.replace(
        &exec(&good),
        &format!("{} --product-carrier-test-invalid", exec(&good)),
    );
    assert!(!unit::same(&good, &bad));
    build.sequence = 3;
    build.commit = format!("{:040}", 3);
    let old_url = build.files_url.clone();
    build.files_url = "https://channel.test/build-3/".into();
    for file in &build.files {
        let bytes = lamp.channel.files[&format!("{old_url}{}", file.name)].clone();
        lamp.channel
            .files
            .insert(format!("{}{}", build.files_url, file.name), bytes);
    }
    assert_eq!(build.units.len(), 1);
    build.units[0].sha256 = sha(&bad);
    build.units[0].size = bad.len() as u64;
    lamp.channel.files.insert(
        format!("{}{}", build.files_url, build.units[0].file_name()),
        bad.as_bytes().to_vec(),
    );
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&serde_json::to_vec(&build).unwrap(), &lamp.key).unwrap(),
    );
    lamp.calls.borrow_mut().clear();
    let outcome = {
        let _rounding = rounding();
        let clock = FakeClock(Rc::clone(&lamp.now));
        let mut ears = FakeEars {
            now: Rc::clone(&lamp.now),
            talking: Box::new(|_| false),
            root: lamp.root.clone(),
        };
        let mut systemd = RejectCarrierArgument(&mut lamp.systemd);
        let disk = FakeDisk(lamp.free);
        let mut say = |line: &str| lamp.said.push(line.to_owned());
        let mut phase = |now: &'static str| lamp.phases.push(now);
        let mut world = World {
            channel: &mut lamp.channel,
            systemd: &mut systemd,
            ears: &mut ears,
            disk: &disk,
            clock: &clock,
            say: &mut say,
            phase: &mut phase,
        };
        round(&lamp.config, &mut world)
    };
    assert!(
        matches!(
            outcome,
            Outcome::RolledBack {
                sequence: 3,
                after: Ok(()),
                ..
            }
        ),
        "{outcome:?}: {:?}",
        lamp.said
    );
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
    assert_eq!(lamp.state()["refused"][0]["sequence"], 3);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), good);
    assert_eq!(lamp.eyes_link(), sha("eyes 1"));
    assert_eq!(lamp.read("/opt/lampOS/eyes/current/lamp-eyes"), "eyes 1");
    assert_eq!(lamp.systemd.units["lamp-eyes.service"].active, "active");
    assert_eq!(
        lamp.calls(),
        [
            "reload",
            "restart lamp-eyes.service",
            "reload",
            "restart lamp-eyes.service"
        ]
    );
    assert_eq!(lamp.own_build(), sha(updater));
    assert!(lamp.pending().is_none());
}

// A signed fleet build can carry current-linked product units beside the
// older pinned units kept for development lamps. Layout precedes specificity.
fn product_first(lamp: &mut Lamp, reverse: bool) {
    if !reverse {
        return;
    }
    let envelope = &lamp.channel.files[&format!("{CHANNEL}{}", manifest::ENVELOPE)];
    let bytes = manifest::signed(envelope, &lamp.config.key).unwrap();
    let mut build = manifest::parse(&bytes).unwrap();
    build.units.reverse();
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&serde_json::to_vec(&build).unwrap(), &lamp.key).unwrap(),
    );
}

fn pinned_eyes_unit() -> String {
    eyes_unit().replace("/current/", &format!("/{}/", sha("eyes 1")))
}

fn linked_product_beside_pinned_template(reverse: bool) {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    let product = new_eyes_unit();
    carried(
        &mut lamp,
        &product,
        "eyes 2",
        Some(&pinned_eyes_unit()),
        Vec::new(),
    );
    product_first(&mut lamp, reverse);
    let outcome = lamp.round();
    assert_eq!(
        outcome,
        Outcome::Switched(2),
        "reverse={reverse}: {:?}",
        lamp.said
    );
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), product);
    assert_eq!(lamp.eyes_link(), sha("eyes 2"));
    assert_eq!(lamp.calls(), ["reload", "restart lamp-eyes.service"]);
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
}

#[test]
fn a_linked_part_uses_product_before_its_pinned_lamp_template() {
    linked_product_beside_pinned_template(true);
}

#[test]
fn a_linked_part_uses_product_after_its_pinned_lamp_template() {
    linked_product_beside_pinned_template(false);
}

#[test]
fn a_compatible_linked_lamp_template_still_wins_in_either_manifest_order() {
    for reverse in [false, true] {
        let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
        let specific = format!("{}Environment=LAMP_BUS_ROOT=/run/this-lamp\n", eyes_unit());
        carried(
            &mut lamp,
            &new_eyes_unit(),
            "eyes 2",
            Some(&specific),
            Vec::new(),
        );
        product_first(&mut lamp, reverse);
        assert_eq!(
            lamp.round(),
            Outcome::Switched(2),
            "reverse={reverse}: {:?}",
            lamp.said
        );
        assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), specific);
        assert_eq!(lamp.eyes_link(), sha("eyes 2"));
    }
}

#[test]
fn a_pinned_part_keeps_its_pinned_lamp_template_beside_product_in_either_order() {
    for reverse in [false, true] {
        let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
        let pinned = pinned_eyes_unit();
        fs::write(
            lamp.root.join("etc/systemd/system/lamp-eyes.service"),
            &pinned,
        )
        .unwrap();
        carried(
            &mut lamp,
            &new_eyes_unit(),
            "eyes 2",
            Some(&pinned),
            Vec::new(),
        );
        product_first(&mut lamp, reverse);
        assert_eq!(
            lamp.round(),
            Outcome::Switched(2),
            "reverse={reverse}: {:?}",
            lamp.said
        );
        assert_eq!(
            lamp.read("/etc/systemd/system/lamp-eyes.service"),
            pinned.replace(&sha("eyes 1"), &sha("eyes 2"))
        );
        // The old current link is irrelevant to this pinned installation.
        assert_eq!(lamp.eyes_link(), sha("eyes 1"));
        assert!(
            !lamp
                .channel
                .fetched
                .iter()
                .any(|url| url.ends_with("product.lamp-eyes.service"))
        );
    }
}

fn climbed_updater_confirms_mixed_layout(reverse: bool) {
    let mut lamp = settings_lamp(false, true);
    lamp.config.updater = manifest::PRODUCT_UNITS_UPDATER - 1;
    lamp.systemd.self_recovery = Ok(());
    let recovery = lamp.root.join(own::BACK.trim_start_matches('/'));
    fs::create_dir_all(recovery.parent().unwrap()).unwrap();
    fs::write(&recovery, "the independently installed recovery executable").unwrap();
    fs::set_permissions(&recovery, fs::Permissions::from_mode(0o755)).unwrap();
    let before = eyes_unit();
    let product = new_eyes_unit();
    carried(
        &mut lamp,
        &product,
        "eyes 2",
        Some(&pinned_eyes_unit()),
        Vec::new(),
    );
    let envelope = &lamp.channel.files[&format!("{CHANNEL}{}", manifest::ENVELOPE)];
    let bytes = manifest::signed(envelope, &lamp.config.key).unwrap();
    let mut build = manifest::parse(&bytes).unwrap();
    let updater = "update 2";
    build.files.push(File {
        name: "lamp-update".into(),
        sha256: sha(updater),
        size: updater.len() as u64,
    });
    lamp.channel.files.insert(
        format!("{}lamp-update", build.files_url),
        updater.as_bytes().to_vec(),
    );
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&serde_json::to_vec(&build).unwrap(), &lamp.key).unwrap(),
    );
    product_first(&mut lamp, reverse);
    assert_eq!(
        lamp.round(),
        Outcome::Climbs {
            sequence: 2,
            needed: manifest::PRODUCT_UNITS_UPDATER,
            from: sha("update 1"),
            to: sha(updater),
        },
        "reverse={reverse}: {:?}",
        lamp.said
    );
    assert_eq!(lamp.state()["installed"]["sequence"], 1);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), before);
    assert_eq!(lamp.eyes_link(), sha("eyes 1"));
    assert!(lamp.pending().is_some());
    lamp.config.updater = manifest::UPDATER;
    assert_eq!(
        lamp.confirm(&sha(updater)),
        own::Confirm::Confirmed,
        "reverse={reverse}: {:?}",
        lamp.said
    );
    assert!(lamp.pending().is_none());
    assert_eq!(lamp.state()["installed"]["sequence"], 1);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), before);
    assert_eq!(lamp.eyes_link(), sha("eyes 1"));
    assert_eq!(
        lamp.round(),
        Outcome::Switched(2),
        "reverse={reverse}: {:?}",
        lamp.said
    );
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), product);
    assert_eq!(lamp.eyes_link(), sha("eyes 2"));
    assert_eq!(lamp.own_build(), sha(updater));
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
}

#[test]
fn a_climbed_updater_confirms_product_before_its_pinned_lamp_template() {
    climbed_updater_confirms_mixed_layout(true);
}

#[test]
fn a_climbed_updater_confirms_product_after_its_pinned_lamp_template() {
    climbed_updater_confirms_mixed_layout(false);
}

#[test]
fn skipping_a_pinned_host_logs_only_its_unit_host_and_installed_link() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    let sentinel = "private-unit-environment-must-not-be-journaled";
    let pinned = format!(
        "{}Environment=LAMP_CARRIER_TEST_SECRET={sentinel}\n",
        pinned_eyes_unit()
    );
    let product = new_eyes_unit();
    carried(&mut lamp, &product, "eyes 2", Some(&pinned), Vec::new());
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    let expected = format!(
        "skipping pinned lamp-eyes.service for {}: the installed part uses /opt/lampOS/eyes/current",
        lamp.config.lamp
    );
    assert_eq!(
        lamp.said.iter().filter(|line| **line == expected).count(),
        1
    );
    assert!(!lamp.said.iter().any(|line| line.contains(sentinel)));
    assert!(
        !lamp
            .said
            .iter()
            .any(|line| line.contains("LAMP_CARRIER_TEST_SECRET"))
    );
    assert_eq!(lamp.read("/etc/systemd/system/lamp-eyes.service"), product);
}

#[test]
fn an_invalid_linked_host_is_refused_instead_of_falling_back_to_product() {
    for specific in [
        eyes_unit().replace("eyes/current/lamp-eyes", "other/current/lamp-eyes"),
        eyes_unit().replace("eyes/current/lamp-eyes", "eyes/current/lamp-voice"),
        "[Service]\nExecStart=/usr/bin/false\n".to_owned(),
    ] {
        let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
        let before = lamp.lamp_files();
        carried(
            &mut lamp,
            &new_eyes_unit(),
            "eyes 2",
            Some(&specific),
            Vec::new(),
        );
        let outcome = lamp.round();
        assert!(matches!(outcome, Outcome::Refused(_)), "{outcome:?}");
        assert_eq!(lamp.lamp_files(), before);
        assert!(lamp.calls().is_empty());
        assert!(
            !lamp
                .said
                .iter()
                .any(|line| line.starts_with("skipping pinned "))
        );
        assert!(
            !lamp
                .channel
                .fetched
                .iter()
                .any(|url| url.ends_with("product.lamp-eyes.service"))
        );
    }
}

#[test]
fn a_pinned_host_without_a_product_candidate_keeps_the_existing_refusal() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    let before = lamp.lamp_files();
    carried(
        &mut lamp,
        &new_eyes_unit(),
        "eyes 2",
        Some(&pinned_eyes_unit()),
        Vec::new(),
    );
    let envelope = &lamp.channel.files[&format!("{CHANNEL}{}", manifest::ENVELOPE)];
    let bytes = manifest::signed(envelope, &lamp.config.key).unwrap();
    let mut build = manifest::parse(&bytes).unwrap();
    build
        .units
        .retain(|unit| unit.lamp != manifest::PRODUCT_LAMP);
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&serde_json::to_vec(&build).unwrap(), &lamp.key).unwrap(),
    );
    let outcome = lamp.round();
    assert!(
        matches!(outcome, Outcome::Refused(ref why) if why == "the build's lamp-eyes.service does not start /opt/lampOS/eyes/ through its current link"),
        "{outcome:?}"
    );
    assert_eq!(lamp.lamp_files(), before);
    assert!(lamp.calls().is_empty());
    assert!(
        !lamp
            .said
            .iter()
            .any(|line| line.starts_with("skipping pinned "))
    );
    assert!(
        !lamp
            .channel
            .fetched
            .iter()
            .any(|url| url.ends_with("product.lamp-eyes.service"))
    );
}

fn assert_mixed_template_fetch_failure(host: bool, unreachable: bool) {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    let before = lamp.lamp_files();
    carried(
        &mut lamp,
        &new_eyes_unit(),
        "eyes 2",
        Some(&pinned_eyes_unit()),
        Vec::new(),
    );
    let asset = if host {
        format!("{}.lamp-eyes.service", lamp.config.lamp)
    } else {
        "product.lamp-eyes.service".to_owned()
    };
    let url = format!("https://channel.test/build-2/{asset}");
    if unreachable {
        // The signed envelope remains reachable: only this exact unit fetch fails.
        lamp.channel.unreachable_under = Some(url.clone());
    } else {
        lamp.channel
            .files
            .insert(url.clone(), b"changed after signing".to_vec());
    }
    let outcome = lamp.round();
    if unreachable {
        assert!(matches!(outcome, Outcome::Unreachable(_)), "{outcome:?}");
    } else {
        assert!(
            matches!(outcome, Outcome::Refused(ref why) if why == &format!("{asset} does not match its SHA-256")),
            "{outcome:?}"
        );
    }
    assert!(lamp.channel.fetched.contains(&url));
    assert_eq!(lamp.lamp_files(), before);
    assert!(lamp.calls().is_empty());
    if host {
        assert!(
            !lamp
                .said
                .iter()
                .any(|line| line.starts_with("skipping pinned "))
        );
        assert!(
            !lamp
                .channel
                .fetched
                .iter()
                .any(|url| url.ends_with("product.lamp-eyes.service"))
        );
    } else {
        assert!(
            lamp.said
                .iter()
                .any(|line| line.starts_with("skipping pinned lamp-eyes.service for "))
        );
    }
}

#[test]
fn the_product_fallback_still_rejects_corrupt_signed_unit_bytes() {
    assert_mixed_template_fetch_failure(false, false);
}

#[test]
fn an_unreachable_product_fallback_does_not_switch_any_part() {
    assert_mixed_template_fetch_failure(false, true);
}

#[test]
fn a_corrupt_pinned_host_is_not_silently_skipped_for_product() {
    assert_mixed_template_fetch_failure(true, false);
}

#[test]
fn an_unreachable_pinned_host_is_not_silently_skipped_for_product() {
    assert_mixed_template_fetch_failure(true, true);
}

#[test]
fn a_product_voice_ignores_its_legacy_pinned_releases_template() {
    // Model the real historical Voice folder difference with Voice itself,
    // not an Eyes unit that starts another binary.
    let mut lamp = Lamp::installed();
    let product = lamp.read("/etc/systemd/system/lamp-voice.service");
    let legacy = voice_unit(&sha("lamp-voice 1"));
    assert_eq!(
        unit::linked(&product),
        Some(("/opt/lampOS/voice/".into(), "lamp-voice".into()))
    );
    let pinned = unit::program(&legacy).unwrap();
    assert_eq!(
        (pinned.builds.as_str(), pinned.binary.as_str()),
        ("/opt/lampOS/releases/", "lamp-voice")
    );
    let mut build = product_build(&mut lamp, 2);
    for (host, text) in [
        (lamp.config.lamp.clone(), legacy),
        (manifest::PRODUCT_LAMP.into(), product.clone()),
    ] {
        let unit = Unit {
            lamp: host,
            unit: "lamp-voice.service".into(),
            sha256: sha(&text),
            size: text.len() as u64,
        };
        lamp.channel.files.insert(
            format!("{}{}", build.files_url, unit.file_name()),
            text.into_bytes(),
        );
        build.units.push(unit);
    }
    build.min_updater = Some(manifest::PRODUCT_UNITS_UPDATER);
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&serde_json::to_vec(&build).unwrap(), &lamp.key).unwrap(),
    );
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-voice.service"), product);
    assert_eq!(
        fs::read_link(lamp.root.join("opt/lampOS/voice/current"))
            .unwrap()
            .to_str(),
        Some(sha("lamp-voice 2").as_str())
    );
    assert!(lamp.said.iter().any(|line| line == &format!("skipping pinned lamp-voice.service for {}: the installed part uses /opt/lampOS/voice/current", lamp.config.lamp)));
}
