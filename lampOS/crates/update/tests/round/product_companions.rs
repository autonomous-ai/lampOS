//! A part's templates, sockets and timers, carried under the product
//! (#3689 item 2a): switched with their part, and put back with it.
use super::*;

const TEMPLATE: &str = "lamp-eyes-camera@.service";
const SOCKET: &str = "lamp-eyes-camera.socket";
const TIMER: &str = "lamp-eyes.timer";
const KEEPER: &str = "lamp-keeper-restart@.service";

fn template(options: &str) -> String {
    format!("[Service]\nExecStart=/opt/lampOS/eyes/current/lamp-eyes reauthorize-camera{options}\n")
}

fn socket(mode: &str) -> String {
    format!(
        "[Socket]\nListenStream=/run/lampos-eyes-camera/camera.sock\nSocketMode={mode}\nAccept=yes\n"
    )
}

fn timer(every: &str) -> String {
    format!("[Timer]\nOnUnitActiveSec={every}\n")
}

fn keeper(word: &str) -> String {
    format!("[Service]\nExecStart=/usr/local/bin/lamp-keeper {word}\n")
}

/// lamp-4ace with its eyes a product part, and their camera helper's
/// template and socket (listening), a timer of the eyes (waiting) and the
/// keeper's template installed beside them.
fn lamp() -> Lamp {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    for (name, text) in [
        (TEMPLATE, template("")),
        (SOCKET, socket("0660")),
        (TIMER, timer("6h")),
        (KEEPER, keeper("restart-asked")),
    ] {
        fs::write(lamp.root.join("etc/systemd/system").join(name), text).unwrap();
    }
    lamp.systemd.start(SOCKET);
    lamp.systemd.start(TIMER);
    lamp
}

/// Build 2: the eyes' binary `eyes`, and these product units.
fn publish(lamp: &mut Lamp, eyes: &str, units: &[(&str, String)]) {
    let mut build = lamp.manifest(
        2,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
            ("lamp-eyes", eyes),
        ],
        &[],
    );
    for (name, text) in units {
        let unit = Unit {
            lamp: manifest::PRODUCT_LAMP.into(),
            unit: (*name).into(),
            sha256: sha(text),
            size: text.len() as u64,
        };
        lamp.channel.files.insert(
            format!("{}{}", build.files_url, unit.file_name()),
            text.as_bytes().to_vec(),
        );
        build.units.push(unit);
    }
    build.min_updater = Some(manifest::COMPANIONS_UPDATER);
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&serde_json::to_vec(&build).unwrap(), &lamp.key).unwrap(),
    );
}

fn all_new() -> Vec<(&'static str, String)> {
    vec![
        (TEMPLATE, template(" --usb 01da:5875@3-1.1.4")),
        (SOCKET, socket("0600")),
        (TIMER, timer("3h")),
        (KEEPER, keeper("restart")),
    ]
}

fn unit(lamp: &Lamp, name: &str) -> String {
    lamp.read(&format!("/etc/systemd/system/{name}"))
}

#[test]
fn a_parts_template_socket_and_timer_switch_with_it_and_only_the_template_is_not_restarted() {
    let mut lamp = lamp();
    publish(&mut lamp, "eyes 2", &all_new());
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    for (name, text) in all_new().into_iter().take(3) {
        assert_eq!(unit(&lamp, name), text, "{name}");
    }
    // The keeper's program is a fixed path no round switches: left, and
    // never fetched.
    assert_eq!(unit(&lamp, KEEPER), keeper("restart-asked"));
    assert!(!lamp.channel.fetched.iter().any(|url| url.ends_with(KEEPER)));
    assert_eq!(lamp.eyes_link(), sha("eyes 2"));
    assert_eq!(
        lamp.calls(),
        [
            "reload",
            "restart lamp-eyes-camera.socket",
            "restart lamp-eyes.service",
            "restart lamp-eyes.timer",
        ]
    );
}

#[test]
fn only_a_template_changing_is_written_and_reloaded_and_nothing_restarted() {
    let mut lamp = lamp();
    publish(
        &mut lamp,
        "eyes 1",
        &[(TEMPLATE, template(" --usb 01da:5875@3-1.1.4"))],
    );
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(unit(&lamp, TEMPLATE), template(" --usb 01da:5875@3-1.1.4"));
    assert_eq!(lamp.calls(), ["reload"]);
}

#[test]
fn a_build_put_back_puts_its_parts_template_socket_and_timer_back() {
    let mut lamp = lamp();
    let before = lamp.lamp_files();
    publish(&mut lamp, "eyes 2 which crashes", &all_new());
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
fn a_switch_cut_short_is_put_back_with_its_template_and_socket() {
    let mut lamp = lamp();
    let before = lamp.lamp_files();
    publish(&mut lamp, "eyes 2", &all_new());
    lamp.systemd.cut_at_reload = true;
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lamp.round()));
    assert!(cut.is_err());
    assert_eq!(
        unit(&lamp, TEMPLATE),
        all_new()[0].1,
        "written before the cut"
    );
    assert!(lamp.exists("/var/lib/lampos-update/journal.json"));
    let outcome = lamp.round();
    assert!(
        matches!(outcome, Outcome::RolledBack { sequence: 2, .. }),
        "{outcome:?}"
    );
    assert_eq!(lamp.lamp_files(), before);
}

#[test]
fn one_the_lamp_does_not_have_is_neither_fetched_nor_added() {
    let mut lamp = lamp();
    fs::remove_file(lamp.root.join("etc/systemd/system").join(TIMER)).unwrap();
    publish(&mut lamp, "eyes 1", &[(TIMER, timer("3h"))]);
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert!(!lamp.exists(&format!("/etc/systemd/system/{TIMER}")));
    assert!(!lamp.channel.fetched.iter().any(|url| url.ends_with(TIMER)));
    assert!(lamp.calls().is_empty());
}

#[test]
fn one_that_would_start_another_part_or_unit_is_refused_before_any_write() {
    for (name, text) in [
        (
            TEMPLATE,
            template("").replace("eyes/current/lamp-eyes", "voice/current/lamp-voice"),
        ),
        (
            SOCKET,
            format!("{}Service=lamp-eyes.service\n", socket("0660")),
        ),
        (TIMER, format!("{}Unit=lamp-voice.service\n", timer("6h"))),
    ] {
        let mut lamp = lamp();
        let before = lamp.lamp_files();
        publish(&mut lamp, "eyes 2", &[(name, text)]);
        assert!(
            matches!(lamp.round(), Outcome::Refused(ref why) if why.contains(name)),
            "{name}: {:?}",
            lamp.said
        );
        assert_eq!(lamp.lamp_files(), before, "{name}");
        assert!(lamp.calls().is_empty(), "{name}");
    }
}
