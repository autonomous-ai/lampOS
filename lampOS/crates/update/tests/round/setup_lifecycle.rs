//! A completed product Setup may still serve the phone during its grace
//! period. A restart after completion stops it and its condition skips start.
use super::*;

const SETUP: &str = "lamp-setup.service";
const MARKER: &str = "var/lib/lampos-setup/done";
const PROPERTIES: &str = "Id=lamp-setup.service\nNames=lamp-setup.service\nLoadState=loaded\nFragmentPath=/etc/systemd/system/lamp-setup.service\nDropInPaths=\n";
const PRODUCT: &str = include_str!("../../../../deploy/product/lamp-setup.service");

fn setup(lamp: &mut Lamp, completed: bool) {
    lamp.systemd.setup_properties = Some(PROPERTIES.into());
    let folder = lamp
        .root
        .join(format!("opt/lampOS/setup/{}", sha("setup 1")));
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("lamp-setup"), "setup 1").unwrap();
    std::os::unix::fs::symlink(sha("setup 1"), folder.parent().unwrap().join("current")).unwrap();
    fs::write(lamp.root.join("etc/systemd/system").join(SETUP), PRODUCT).unwrap();
    lamp.systemd.start(SETUP);
    if completed {
        complete(lamp);
    }
}

fn complete(lamp: &mut Lamp) {
    let marker = lamp.root.join(MARKER);
    fs::create_dir_all(marker.parent().unwrap()).unwrap();
    fs::write(marker, "done\n").unwrap();
    // Independent simulation of systemd's condition rejection, not the
    // implementation's product-unit recognition.
    lamp.systemd.condition_skipped.push(SETUP.into());
}

fn publish(lamp: &mut Lamp) {
    lamp.publish(2, &[("lamp-setup", "setup 2")], &[]);
}

fn setup_restarts(lamp: &Lamp) -> usize {
    lamp.calls()
        .iter()
        .filter(|call| *call == &format!("restart {SETUP}"))
        .count()
}

#[test]
fn completed_product_setup_keeps_serving_without_restart_or_false_rollback() {
    let mut lamp = Lamp::new();
    setup(&mut lamp, true);
    let pid = lamp.systemd.units[SETUP].pid;
    publish(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(setup_restarts(&lamp), 0);
    assert_eq!(lamp.systemd.units[SETUP].pid, pid);
    assert_eq!(
        fs::read_link(lamp.root.join("opt/lampOS/setup/current")).unwrap(),
        PathBuf::from(sha("setup 2"))
    );
}

#[test]
fn incomplete_product_setup_still_restarts_and_is_watched() {
    let mut lamp = Lamp::new();
    setup(&mut lamp, false);
    publish(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(setup_restarts(&lamp), 1);

    let mut lamp = Lamp::new();
    setup(&mut lamp, false);
    lamp.systemd.crashing.push(SETUP.into());
    publish(&mut lamp);
    assert!(matches!(
        lamp.round(),
        Outcome::RolledBack { after: Err(_), .. }
    ));
    assert_eq!(setup_restarts(&lamp), 2);
}

#[test]
fn the_exact_product_exit_note_and_merged_usr_layout_keep_the_exception() {
    let mut lamp = Lamp::new();
    setup(&mut lamp, true);
    let dropin = lamp.root.join("etc/systemd/system/lamp-setup.service.d");
    fs::create_dir_all(&dropin).unwrap();
    fs::write(
        dropin.join("exit-note.conf"),
        lamp_update::install::EXIT_NOTE,
    )
    .unwrap();
    lamp.systemd.setup_properties = Some(PROPERTIES.replace(
        "DropInPaths=",
        "DropInPaths=/etc/systemd/system/lamp-setup.service.d/exit-note.conf",
    ));
    fs::create_dir_all(lamp.root.join("usr/lib/systemd/system")).unwrap();
    std::os::unix::fs::symlink("usr/lib", lamp.root.join("lib")).unwrap();
    publish(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(setup_restarts(&lamp), 0);
}

#[test]
fn custom_base_units_and_candidate_changes_keep_normal_watch_rules() {
    for candidate in [false, true] {
        let mut lamp = Lamp::new();
        setup(&mut lamp, true);
        let custom = format!("{PRODUCT}\n[Unit]\nConditionPathExists=\n");
        if candidate {
            lamp.publish(2, &[("lamp-setup", "setup 2")], &[(SETUP, &custom)]);
        } else {
            fs::write(lamp.root.join("etc/systemd/system").join(SETUP), custom).unwrap();
            publish(&mut lamp);
        }
        assert!(matches!(lamp.round(), Outcome::RolledBack { .. }));
        assert!(setup_restarts(&lamp) >= 1);
    }
}

#[test]
fn unknown_dropins_at_each_scope_keep_normal_watch_rules() {
    for relative in [
        "etc/systemd/system/lamp-setup.service.d/reset.conf",
        "run/systemd/system/lamp-setup.service.d/reset.conf",
        "usr/lib/systemd/system/lamp-setup.service.d/reset.conf",
        "etc/systemd/system/service.d/reset.conf",
        "etc/systemd/system/lamp-.service.d/reset.conf",
        "run/systemd/generator.early/lamp-setup.service.d/reset.conf",
        "etc/systemd/system/lamp-setup.service.d/exit-note.conf",
    ] {
        let mut lamp = Lamp::new();
        setup(&mut lamp, true);
        let path = lamp.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "[Unit]\nConditionPathExists=\n").unwrap();
        publish(&mut lamp);
        assert!(
            matches!(lamp.round(), Outcome::RolledBack { after: Err(_), .. }),
            "{relative}"
        );
        assert_eq!(setup_restarts(&lamp), 2, "{relative}");
    }
}

#[test]
fn uncertain_override_files_do_not_hang_or_grant_the_exception() {
    for kind in ["symlink", "fifo", "oversize", "directory", "too-many"] {
        let mut lamp = Lamp::new();
        setup(&mut lamp, true);
        let dir = lamp.root.join("etc/systemd/system/lamp-setup.service.d");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("exit-note.conf");
        match kind {
            "symlink" => std::os::unix::fs::symlink("missing", &path).unwrap(),
            "fifo" => {
                // Do not inherit another round's records lock across spawn.
                // Release this guard before the round takes its own guard.
                let _forking = forking();
                assert!(
                    std::process::Command::new("mkfifo")
                        .arg(&path)
                        .status()
                        .unwrap()
                        .success()
                );
            }
            "oversize" => fs::write(&path, "#".repeat(16 * 1024 + 1)).unwrap(),
            "directory" => fs::create_dir(&path).unwrap(),
            "too-many" => {
                for n in 0..129 {
                    fs::write(dir.join(format!("{n}.ignored")), "").unwrap();
                }
            }
            _ => unreachable!(),
        }
        publish(&mut lamp);
        assert!(
            matches!(lamp.round(), Outcome::RolledBack { after: Err(_), .. }),
            "{kind}"
        );
        assert_eq!(setup_restarts(&lamp), 2, "{kind}");
    }
}

#[test]
fn a_different_failed_part_still_rolls_back_without_restarting_completed_setup() {
    let mut lamp = Lamp::new();
    setup(&mut lamp, true);
    let pid = lamp.systemd.units[SETUP].pid;
    lamp.publish(
        2,
        &[("lamp-setup", "setup 2"), ("lamp-body", "body crash")],
        &[],
    );
    assert!(matches!(
        lamp.round(),
        Outcome::RolledBack { after: Ok(()), .. }
    ));
    assert_eq!(setup_restarts(&lamp), 0);
    assert_eq!(lamp.systemd.units[SETUP].pid, pid);
    assert_eq!(
        fs::read_link(lamp.root.join("opt/lampOS/setup/current")).unwrap(),
        PathBuf::from(sha("setup 1"))
    );
}

#[test]
fn recovery_keeps_the_true_active_journal_but_does_not_restart_completed_setup() {
    for legacy in [false, true] {
        let mut lamp = Lamp::new();
        setup(&mut lamp, true);
        publish(&mut lamp);
        let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            lamp.round_observing(&mut |line| {
                assert_ne!(line, "switching to build 2", "cut after the link moved");
            })
        }));
        assert!(cut.is_err());
        assert_eq!(
            fs::read_link(lamp.root.join("opt/lampOS/setup/current")).unwrap(),
            PathBuf::from(sha("setup 2"))
        );
        let path = lamp.root.join("var/lib/lampos-update/journal.json");
        let mut journal: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(journal["units"][0]["unit"], SETUP);
        assert_eq!(journal["units"][0]["was_active"], true);
        if legacy {
            journal.as_object_mut().unwrap().remove("rollback_order");
            fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
        }
        lamp.channel.offline = true;
        assert!(matches!(
            lamp.round(),
            Outcome::RolledBack { after: Ok(()), .. }
        ));
        assert_eq!(setup_restarts(&lamp), 0);
        assert!(!path.exists());
        assert_eq!(
            fs::read_link(lamp.root.join("opt/lampOS/setup/current")).unwrap(),
            PathBuf::from(sha("setup 1"))
        );
    }
}

#[test]
fn completion_after_the_activity_snapshot_is_rechecked_before_restart() {
    let mut lamp = Lamp::new();
    setup(&mut lamp, false);
    // The initial activity snapshot is active. While the updater fetches,
    // Setup can complete and eventually end before switching starts.
    lamp.systemd.condition_skipped.push(SETUP.into());
    let marker = lamp.root.join(MARKER);
    publish(&mut lamp);
    let mut completed = false;
    let outcome = lamp.round_observing(&mut |line| {
        if line.starts_with("installed /opt/lampOS/setup/") {
            fs::create_dir_all(marker.parent().unwrap()).unwrap();
            fs::write(&marker, "done\n").unwrap();
            completed = true;
        }
    });
    assert!(completed);
    assert_eq!(outcome, Outcome::Switched(2));
    assert_eq!(setup_restarts(&lamp), 0);
}

#[test]
fn reset_removing_completion_before_restart_retains_the_normal_watch() {
    let mut lamp = Lamp::new();
    setup(&mut lamp, true);
    let marker = lamp.root.join(MARKER);
    publish(&mut lamp);
    let mut removed = false;
    let outcome = lamp.round_observing(&mut |line| {
        if line == "switching to build 2" {
            fs::remove_file(&marker).unwrap();
            removed = true;
        }
    });
    assert!(removed);
    // This independent fake still rejects the start. The absence of the
    // marker must therefore expose the ordinary watcher failure, not skip it.
    assert!(matches!(outcome, Outcome::RolledBack { after: Err(_), .. }));
    assert_eq!(setup_restarts(&lamp), 2);
}

#[test]
fn reset_removing_completion_before_recovery_restarts_the_old_setup() {
    let mut lamp = Lamp::new();
    setup(&mut lamp, true);
    publish(&mut lamp);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            lamp.round_observing(&mut |line| {
                assert_ne!(line, "switching to build 2", "cut after the link moved");
            })
        }))
        .is_err()
    );
    assert_eq!(
        fs::read_link(lamp.root.join("opt/lampOS/setup/current")).unwrap(),
        PathBuf::from(sha("setup 2"))
    );
    let journal: serde_json::Value = serde_json::from_slice(
        &fs::read(lamp.root.join("var/lib/lampos-update/journal.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["units"][0]["unit"], SETUP);
    assert_eq!(journal["units"][0]["was_active"], true);
    fs::remove_file(lamp.root.join(MARKER)).unwrap();
    lamp.systemd.condition_skipped.clear();
    assert!(matches!(
        lamp.round(),
        Outcome::RolledBack { after: Ok(()), .. }
    ));
    assert_eq!(setup_restarts(&lamp), 1);
    assert_eq!(lamp.systemd.units[SETUP].active, "active");
    assert_eq!(
        fs::read_link(lamp.root.join("opt/lampOS/setup/current")).unwrap(),
        PathBuf::from(sha("setup 1"))
    );
}

#[test]
fn an_alias_outside_the_lamp_prefix_cannot_bypass_the_product_check() {
    let mut lamp = Lamp::new();
    setup(&mut lamp, true);
    let units = lamp.root.join("etc/systemd/system");
    std::os::unix::fs::symlink(SETUP, units.join("bootstrap.service")).unwrap();
    let dropin = units.join("bootstrap.service.d");
    fs::create_dir_all(&dropin).unwrap();
    fs::write(
        dropin.join("override.conf"),
        "[Unit]\nConditionPathExists=\n",
    )
    .unwrap();
    lamp.systemd.setup_properties = Some(
        PROPERTIES
            .replace(
                "Names=lamp-setup.service",
                "Names=lamp-setup.service bootstrap.service",
            )
            .replace(
                "DropInPaths=",
                "DropInPaths=/etc/systemd/system/bootstrap.service.d/override.conf",
            ),
    );
    publish(&mut lamp);
    // Unknown effective lifecycle must retain normal restart/watch; this
    // independent fake rejects its start, making a skipped watch visible.
    assert!(matches!(
        lamp.round(),
        Outcome::RolledBack { after: Err(_), .. }
    ));
    assert_eq!(setup_restarts(&lamp), 2);
}

#[test]
fn chained_and_loaded_aliases_retain_normal_restart_even_without_alias_files() {
    for loaded_only in [false, true] {
        let mut lamp = Lamp::new();
        setup(&mut lamp, true);
        if !loaded_only {
            let units = lamp.root.join("etc/systemd/system");
            std::os::unix::fs::symlink(SETUP, units.join("middle.service")).unwrap();
            std::os::unix::fs::symlink("middle.service", units.join("bootstrap.service")).unwrap();
        }
        lamp.systemd.setup_properties = Some(PROPERTIES.replace(
            "Names=lamp-setup.service",
            "Names=lamp-setup.service middle.service bootstrap.service",
        ));
        publish(&mut lamp);
        assert!(matches!(
            lamp.round(),
            Outcome::RolledBack { after: Err(_), .. }
        ));
        assert_eq!(setup_restarts(&lamp), 2);
    }
}

#[test]
fn unknown_manager_identity_and_custom_loaded_paths_keep_normal_watch() {
    for properties in [
        None,
        Some(PROPERTIES.replace("LoadState=loaded", "LoadState=error")),
        Some(PROPERTIES.replace(
            "FragmentPath=/etc/systemd/system/lamp-setup.service",
            "FragmentPath=/custom/systemd/lamp-setup.service",
        )),
        Some(PROPERTIES.replace(
            "DropInPaths=",
            "DropInPaths=/custom/systemd/bootstrap.service.d/override.conf",
        )),
    ] {
        let mut lamp = Lamp::new();
        setup(&mut lamp, true);
        lamp.systemd.setup_properties = properties;
        publish(&mut lamp);
        assert!(matches!(
            lamp.round(),
            Outcome::RolledBack { after: Err(_), .. }
        ));
        assert_eq!(setup_restarts(&lamp), 2);
    }
}

#[test]
fn completion_removed_during_the_manager_query_is_not_used() {
    let mut lamp = Lamp::new();
    setup(&mut lamp, true);
    lamp.systemd.remove_setup_marker_on_query = true;
    publish(&mut lamp);
    assert!(matches!(
        lamp.round(),
        Outcome::RolledBack { after: Err(_), .. }
    ));
    assert_eq!(setup_restarts(&lamp), 2);
}

#[test]
fn recovery_rechecks_effective_aliases_after_restoring_the_old_unit() {
    let mut lamp = Lamp::new();
    setup(&mut lamp, true);
    publish(&mut lamp);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            lamp.round_observing(&mut |line| {
                assert_ne!(line, "switching to build 2", "cut after the link moved");
            })
        }))
        .is_err()
    );
    assert_eq!(
        fs::read_link(lamp.root.join("opt/lampOS/setup/current")).unwrap(),
        PathBuf::from(sha("setup 2"))
    );
    lamp.systemd.setup_properties = Some(PROPERTIES.replace(
        "Names=lamp-setup.service",
        "Names=lamp-setup.service bootstrap.service",
    ));
    assert!(matches!(
        lamp.round(),
        Outcome::RolledBack { after: Err(_), .. }
    ));
    assert_eq!(setup_restarts(&lamp), 1);
    assert_eq!(
        fs::read_link(lamp.root.join("opt/lampOS/setup/current")).unwrap(),
        PathBuf::from(sha("setup 1"))
    );
}
