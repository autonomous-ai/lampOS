//! An up-to-date round's additions (#7018): the product units the build a
//! lamp runs carries and it lacks, because the updater that switched to it
//! was an older one, which added only the units it knew. The new updater's
//! first round finds the lamp up to date and adds them once, as a switch
//! adds them, leaving the build, `previous` and every unit the lamp has as
//! they are.
use super::product_adds::{
    LEFT_OUT, MOOD, RECORDS_TIMER, SOCKET, all, link, mood, product_lamp, publish, publish_all,
    records_timer,
};
use super::*;

const STATE: &str = "var/lib/lampos-update/state.json";

/// Build 2 taken by an older updater: its round added nothing (a build
/// below the level that adds, here), and it knew no `adds_tried`. The build
/// before it, 1, is kept as `previous`.
fn taken_by_an_older_updater(lamp: &mut Lamp, mood: &str) {
    publish_all(lamp, manifest::COMPANIONS_UPDATER, mood);
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert!(!lamp.exists(&format!("/etc/systemd/system/{MOOD}")));
    let mut state = lamp.state();
    let object = state.as_object_mut().unwrap();
    object.remove("adds_tried");
    object.insert(
        "previous".into(),
        serde_json::json!({"sequence": 1, "commit": format!("{:040}", 1)}),
    );
    fs::write(lamp.root.join(STATE), state.to_string()).unwrap();
}

fn said(lamp: &Lamp, line: &str) -> bool {
    lamp.said.iter().any(|said| said == line)
}

#[test]
fn an_up_to_date_round_adds_the_units_its_build_carries_that_an_older_updater_did_not() {
    let mut lamp = product_lamp();
    taken_by_an_older_updater(&mut lamp, "mood 1");
    let before = lamp.state();
    // The same build, as the updater it carries reads it: units of level 8.
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    for (name, text) in all().into_iter().take(5) {
        assert_eq!(
            lamp.read(&format!("/etc/systemd/system/{name}")),
            text,
            "{name}"
        );
    }
    assert_eq!(link(&lamp, "/opt/lampOS/mood/current"), Some(sha("mood 1")));
    assert_eq!(
        link(
            &lamp,
            &format!("/etc/systemd/system/lampos.target.wants/{MOOD}")
        ),
        Some(format!("/etc/systemd/system/{MOOD}"))
    );
    // Written, reloaded, and the new part started once the watch passed.
    assert_eq!(
        lamp.calls(),
        ["reload", &format!("start --no-block {MOOD}")]
    );
    assert!(
        lamp.said.iter().any(
            |line| line.starts_with("build 2, which the lamp runs, adds ")
                && line.contains(MOOD)
                && line.ends_with(": the updater that switched to it did not carry them (#7018)")
        ),
        "{:?}",
        lamp.said
    );
    assert!(
        said(&lamp, &format!("started {MOOD}, added by build 2")),
        "{:?}",
        lamp.said
    );
    assert!(!said(&lamp, "switching to build 2"), "{:?}", lamp.said);
    // The build stays the lamp's, and the one before it its way back.
    let state = lamp.state();
    assert_eq!(state["installed"], before["installed"]);
    assert_eq!(state["previous"], before["previous"]);
    assert_eq!(state["refused"], serde_json::json!([]));
    assert_eq!(state["adds_tried"], 2);
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    // Tried once: the next round makes no call.
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(lamp.calls().len(), 2, "{:?}", lamp.calls());
}

/// A unit the lamp has is never written, whatever the owner made of it:
/// a socket edited and disabled, a timer masked. The others are added.
#[test]
fn an_up_to_date_round_leaves_a_unit_the_lamp_has_edited_disabled_or_masked() {
    let mut lamp = product_lamp();
    taken_by_an_older_updater(&mut lamp, "mood 1");
    let units = lamp.root.join("etc/systemd/system");
    let edited = "[Socket]\nListenStream=/run/the-owners.sock\nAccept=yes\n";
    fs::write(units.join(SOCKET), edited).unwrap();
    std::os::unix::fs::symlink("/dev/null", units.join(RECORDS_TIMER)).unwrap();
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(&format!("/etc/systemd/system/{SOCKET}")), edited);
    assert!(!lamp.exists(&format!(
        "/etc/systemd/system/sockets.target.wants/{SOCKET}"
    )));
    assert_eq!(
        link(&lamp, &format!("/etc/systemd/system/{RECORDS_TIMER}")),
        Some("/dev/null".into())
    );
    assert!(
        fs::symlink_metadata(units.join(format!("timers.target.wants/{RECORDS_TIMER}"))).is_err()
    );
    assert_eq!(lamp.read(&format!("/etc/systemd/system/{MOOD}")), mood());
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
}

/// A companion of a part neither here nor added, and a unit `install`
/// leaves out: nothing added, nothing started, and the build kept.
#[test]
fn an_up_to_date_round_adds_no_companion_of_an_absent_part_nor_a_unit_install_leaves_out() {
    let mut lamp = product_lamp();
    taken_by_an_older_updater(&mut lamp, "mood 1");
    publish(
        &mut lamp,
        manifest::ADDS_UPDATER,
        &[("lamp-eyes", "eyes 1")],
        &[(RECORDS_TIMER, records_timer())],
    );
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert!(!lamp.exists(&format!("/etc/systemd/system/{RECORDS_TIMER}")));
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
    // Looked at for this build: not fetched again at the next round.
    let fetched = |lamp: &Lamp| {
        lamp.channel
            .fetched
            .iter()
            .filter(|url| url.ends_with(RECORDS_TIMER))
            .count()
    };
    let once = fetched(&lamp);
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(fetched(&lamp), once);

    let mut lamp = product_lamp();
    taken_by_an_older_updater(&mut lamp, "mood 1");
    let keeper = "[Socket]\nListenStream=/run/lampos-keeper-restart/restart.sock\nAccept=yes\n[Install]\nWantedBy=lampos.target\n";
    publish(
        &mut lamp,
        manifest::ADDS_UPDATER,
        &[("lamp-eyes", "eyes 1")],
        &[(LEFT_OUT, keeper.into())],
    );
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert!(!lamp.exists(&format!("/etc/systemd/system/{LEFT_OUT}")));
    assert!(
        !lamp
            .channel
            .fetched
            .iter()
            .any(|url| url.ends_with(LEFT_OUT))
    );
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
}

/// A part added that fails its watch: everything the pass added is taken
/// away, the build is not refused, and the pass is not made again at this
/// build.
#[test]
fn an_up_to_date_rounds_addition_that_fails_its_watch_is_taken_away_and_not_tried_again() {
    let mut lamp = product_lamp();
    // Started by something else already: restarted and watched as any
    // running part.
    lamp.systemd.units.insert(
        MOOD.into(),
        Running {
            active: "active".into(),
            sub: "running".into(),
            pid: 7,
            ..Running::default()
        },
    );
    taken_by_an_older_updater(&mut lamp, "mood 1 which crashes");
    let files = lamp.lamp_files();
    let before = lamp.state();
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1 which crashes");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert!(lamp.calls().contains(&format!("restart {MOOD}")));
    assert!(lamp.calls().contains(&format!("stop {MOOD}")));
    assert_eq!(lamp.lamp_files(), files);
    assert!(
        lamp.said.iter().any(|line| line.contains(MOOD)
            && line.contains(", added to build 2, was taken away: ")
            && line
                .ends_with("; build 2 is kept, and its units are tried again with the next build")),
        "{:?}",
        lamp.said
    );
    let state = lamp.state();
    assert_eq!(state["installed"], before["installed"]);
    assert_eq!(state["previous"], before["previous"]);
    assert_eq!(state["refused"], serde_json::json!([]));
    assert_eq!(state["adds_tried"], 2);
    let calls = lamp.calls().len();
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(lamp.calls().len(), calls, "{:?}", lamp.calls());
    assert!(!lamp.exists(&format!("/etc/systemd/system/{MOOD}")));
}

/// The same through `recover`: a pass cut short is put back at the next
/// start, refuses nothing, and is not made again at this build.
#[test]
fn an_up_to_date_rounds_addition_cut_short_is_taken_away_at_the_next_start_and_not_tried_again() {
    let mut lamp = product_lamp();
    taken_by_an_older_updater(&mut lamp, "mood 1");
    let files = lamp.lamp_files();
    let before = lamp.state();
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    lamp.systemd.cut_at_reload = true;
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lamp.round()));
    assert!(cut.is_err());
    assert!(lamp.exists(&format!("/etc/systemd/system/{MOOD}")));
    assert!(lamp.exists("/var/lib/lampos-update/journal.json"));
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.lamp_files(), files);
    assert!(
        lamp.said.iter().any(|line| line.contains(MOOD)
            && line.contains(", added to build 2, was taken away: the switch was cut short")),
        "{:?}",
        lamp.said
    );
    assert!(
        !lamp
            .said
            .iter()
            .any(|line| line.starts_with("putting build 2 back")),
        "{:?}",
        lamp.said
    );
    let state = lamp.state();
    assert_eq!(state["installed"], before["installed"]);
    assert_eq!(state["previous"], before["previous"]);
    assert_eq!(state["refused"], serde_json::json!([]));
    assert_eq!(state["adds_tried"], 2);
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert!(!lamp.exists(&format!("/etc/systemd/system/{MOOD}")));
}

/// A unit of lamp-update's own (the region's, #6386) is added beside the
/// folder its own unit starts from, and `update/current` is left as it is.
#[test]
fn an_up_to_date_round_adds_lamp_updates_own_unit_beside_its_folder_and_leaves_its_link() {
    let mut lamp = product_lamp();
    taken_by_an_older_updater(&mut lamp, "mood 1");
    let region =
        "[Service]\nType=oneshot\nExecStart=/opt/lampOS/update/current/lamp-update region\n"
            .to_string();
    let mut units = all();
    units.push(("lamp-region.service", region.clone()));
    publish(
        &mut lamp,
        manifest::ADDS_UPDATER,
        &[
            ("lamp-eyes", "eyes 1"),
            ("lamp-mood", "mood 1"),
            ("lamp-records", "records 1"),
            ("lamp-update", "update 1"),
        ],
        &units,
    );
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-region.service"), region);
    assert_eq!(
        link(&lamp, "/opt/lampOS/update/current"),
        Some(sha("update 1"))
    );
    assert!(
        !lamp
            .said
            .iter()
            .any(|line| line.contains("no unit of this lamp starts through it")),
        "{:?}",
        lamp.said
    );
}

/// Without `--apply`, the additions are listed as a switch's are, and
/// nothing is written or asked of systemd.
#[test]
fn a_dry_up_to_date_round_lists_the_additions() {
    let mut lamp = product_lamp();
    taken_by_an_older_updater(&mut lamp, "mood 1");
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    lamp.config.apply = false;
    let before = lamp.files();
    let outcome = lamp.round();
    let Outcome::Would(lines) = &outcome else {
        panic!("{outcome:?}");
    };
    assert!(lines.iter().any(|line| line.contains(MOOD)), "{lines:?}");
    assert_eq!(lamp.files(), before);
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
}

/// The second hop (the first builder's P2 on #8400): build 2 was taken by an
/// updater with this pass, which set `adds_tried` for 2 but did not carry
/// the units build 2 adds. Build 2's own updater, run from another folder,
/// still looks once; its own next round does not.
#[test]
fn the_switched_builds_own_updater_looks_once_though_the_switching_updater_set_adds_tried() {
    let mut lamp = product_lamp();
    lamp.config.running = Some("/opt/lampOS/update/switching/".into());
    publish_all(&mut lamp, manifest::COMPANIONS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert!(!lamp.exists(&format!("/etc/systemd/system/{MOOD}")));
    let state = lamp.state();
    assert_eq!(state["adds_tried"], 2);
    assert_eq!(state["adds_tried_by"], "/opt/lampOS/update/switching/");
    // Build 2's own updater, from its own folder, carries the units.
    lamp.config.running = Some("/opt/lampOS/update/own/".into());
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    for (name, text) in all().into_iter().take(5) {
        assert_eq!(
            lamp.read(&format!("/etc/systemd/system/{name}")),
            text,
            "{name}"
        );
    }
    let state = lamp.state();
    assert_eq!(state["adds_tried"], 2);
    assert_eq!(state["adds_tried_by"], "/opt/lampOS/update/own/");
    // Looked once: its next round makes no call.
    let calls = lamp.calls().len();
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(lamp.calls().len(), calls, "{:?}", lamp.calls());
}
