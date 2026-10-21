//! #2700: a completed parts update never hands over without independent
//! recovery. The real filesystem check runs; systemd's answer is a fixture.
use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn not_taken(lamp: &Lamp, old: &str, build: &Manifest) {
    assert_eq!(lamp.own_build(), old);
    assert!(lamp.pending().is_none());
    assert!(own::refused(&lamp.records()).is_empty());
    let new = &build
        .files
        .iter()
        .find(|file| file.name == own::BINARY)
        .unwrap()
        .sha256;
    assert!(!lamp.exists(&format!("/opt/lampOS/update/{new}")));
    assert!(
        !lamp
            .channel
            .fetched
            .contains(&format!("{}{}", build.files_url, own::BINARY)),
        "the updater must not be fetched before recovery is ready: {:?}",
        lamp.channel.fetched
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("independent recovery is not ready")),
        "{:?}",
        lamp.said
    );
}

#[test]
fn an_absent_or_unrunnable_fallback_keeps_the_completed_parts_and_old_updater() {
    for kind in ["missing", "not executable", "directory", "symlink"] {
        let mut lamp = Lamp::installed_switching_itself();
        let old = lamp.own_build();
        let back = lamp.config.on(own::BACK);
        fs::remove_file(&back).unwrap();
        match kind {
            "not executable" => {
                fs::write(&back, "lamp-update 1").unwrap();
                fs::set_permissions(&back, fs::Permissions::from_mode(0o600)).unwrap();
            }
            "directory" => fs::create_dir(&back).unwrap(),
            "symlink" => symlink(lamp.config.on(own::CURRENT).join(own::BINARY), &back).unwrap(),
            _ => {}
        }
        let build = product_build_with_updater(&mut lamp, 2, "lamp-update 2");
        assert_eq!(
            lamp.round(),
            Outcome::Switched(2),
            "{kind}: {:?}",
            lamp.said
        );
        assert_eq!(lamp.state()["installed"]["sequence"], 2);
        not_taken(&lamp, &old, &build);
        assert_eq!(lamp.systemd.self_recovery_queries, 0, "{kind}");
    }
}

#[test]
fn unavailable_recovery_keeps_the_parts_and_can_be_retried_when_armed() {
    for why in [
        "lamp-update-back.timer is not active",
        "systemctl query failed",
    ] {
        let mut lamp = Lamp::installed_switching_itself();
        let old = lamp.own_build();
        lamp.systemd.self_recovery = Err(why.into());
        let build = product_build_with_updater(&mut lamp, 2, "lamp-update 2");
        assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
        assert_eq!(lamp.state()["installed"]["sequence"], 2);
        not_taken(&lamp, &old, &build);
        assert_eq!(lamp.systemd.self_recovery_queries, 1);

        // The refusal did not ban this binary or undo the parts. A later
        // round sees the parts up to date and can still take the updater.
        lamp.systemd.self_recovery = Ok(());
        assert_eq!(
            lamp.round(),
            Outcome::SelfSwitched {
                sequence: 2,
                from: old,
                to: sha("lamp-update 2"),
            },
            "{:?}",
            lamp.said
        );
        assert_eq!(lamp.systemd.self_recovery_queries, 2);
        assert_eq!(lamp.confirm(&sha("lamp-update 2")), own::Confirm::Confirmed);
    }
}

#[test]
fn missing_recovery_also_defers_an_updater_first_climb_without_touching_parts() {
    for missing_binary in [true, false] {
        let mut lamp = Lamp::installed_switching_itself();
        lamp.config.updater = 1;
        let old = lamp.own_build();
        if missing_binary {
            fs::remove_file(lamp.config.on(own::BACK)).unwrap();
        } else {
            lamp.systemd.self_recovery =
                Err("lamp-update-back.timer is enabled only at runtime".into());
        }
        let before = parts_files(&lamp);
        let build = product_build_with_updater(&mut lamp, 2, "lamp-update 2");
        needing(&mut lamp, build.clone(), 2);
        let outcome = lamp.round();
        assert!(matches!(outcome, Outcome::Refused(_)), "{outcome:?}");
        assert_eq!(parts_files(&lamp), before);
        assert!(lamp.calls().is_empty());
        not_taken(&lamp, &old, &build);
        assert_eq!(
            lamp.systemd.self_recovery_queries,
            usize::from(!missing_binary)
        );
    }
}

#[test]
fn an_unchanged_or_refused_updater_needs_no_recovery_query() {
    for (unchanged, missing_binary) in [(true, true), (true, false), (false, true), (false, false)]
    {
        let mut lamp = Lamp::installed_switching_itself();
        let old = lamp.own_build();
        if missing_binary {
            fs::remove_file(lamp.config.on(own::BACK)).unwrap();
        }
        lamp.systemd.self_recovery = Err("not ready".into());
        let updater = if unchanged {
            "lamp-update 1"
        } else {
            "lamp-update 2"
        };
        if !unchanged {
            fs::write(
                lamp.records().join("self-refused"),
                format!("{}\n", sha(updater)),
            )
            .unwrap();
        }
        product_build_with_updater(&mut lamp, 2, updater);
        assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
        assert_eq!(lamp.own_build(), old);
        assert!(lamp.pending().is_none());
        assert_eq!(lamp.systemd.self_recovery_queries, 0);
        assert!(
            !lamp
                .said
                .iter()
                .any(|line| line.contains("independent recovery is not ready")),
            "an unchanged/refused candidate must skip the whole recovery check: {:?}",
            lamp.said
        );
    }
}
