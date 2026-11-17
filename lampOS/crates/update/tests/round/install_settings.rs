//! #5984: a round brings each settings file install writes to its build's,
//! by the file's rule (docs/updates.md, "Install's settings in a round").
use super::*;
use lamp_update::install;

const EXPRESSION: &str = "/etc/lampOS/expression/expression.json";
const PINS: &str = "/etc/lampOS/controls/controls-pins.json";

/// A lamp install gave build 1, applying a round's settings as the product
/// does (`LAMP_UPDATE_APPLY=on`, the real `lamp-update-back apply`).
fn installed_applying() -> Lamp {
    let mut lamp = Lamp::installed();
    lamp.config.settings = true;
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

/// `path` on the lamp as `text`, recorded as an install's: what an older
/// install wrote there.
fn as_installed(lamp: &Lamp, path: &str, text: &str) {
    fs::write(lamp.root.join(path.trim_start_matches('/')), text).unwrap();
    let mut state = lamp.state();
    state["settings"][path] = sha(text).into();
    fs::write(lamp.records().join("state.json"), state.to_string()).unwrap();
}

/// `path` on the lamp as `text`, its install's record kept: changed on the
/// lamp since.
fn changed_on_lamp(lamp: &Lamp, path: &str, text: &str) {
    fs::write(lamp.root.join(path.trim_start_matches('/')), text).unwrap();
}

/// Build 2: build 1's binaries and install's own settings, with `edit` on
/// them.
fn publish_with_settings(lamp: &mut Lamp, edit: impl FnOnce(&mut Vec<Setting>)) -> Manifest {
    let mut build = product_build(lamp, 2);
    build.settings = install::product_settings();
    edit(&mut build.settings);
    needing(lamp, build, manifest::SETTINGS_UPDATER)
}

fn setting_text(path: &str) -> String {
    install::SETTINGS
        .iter()
        .find(|(at, _)| *at == path)
        .map(|(_, text)| (*text).to_owned())
        .unwrap()
}

fn object(text: &str) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::from_str(text).unwrap() {
        serde_json::Value::Object(object) => object,
        _ => panic!("not an object: {text}"),
    }
}

/// The build's Expression catalogue with the work light #5959 took out:
/// what lamp-52e6 kept from its 26 Oct install.
fn older_expression() -> String {
    let mut older = object(&setting_text(EXPRESSION));
    older
        .get_mut("alive")
        .and_then(|alive| alive.as_object_mut())
        .unwrap()
        .insert(
            "work".into(),
            serde_json::json!({
                "face_area": 0.04, "still": 0.05, "still_us": 5_000_000, "hold_us": 30_000_000
            }),
        );
    format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::Value::Object(older)).unwrap()
    )
}

/// lamp-52e6's own pins: its head pads on 97 and 98 and their rest, as the
/// factory step writes them, and no privacy section yet.
fn own_pins() -> String {
    let mut pins = object(&setting_text(PINS));
    pins.remove("privacy");
    pins.insert(
        "head_pads".into(),
        serde_json::json!([
            {"name": "head-3", "line": 97, "active_low": false, "pull": "down"},
            {"name": "head-4", "line": 98, "active_low": false, "pull": "down"}
        ]),
    );
    pins.insert(
        "head_rest".into(),
        serde_json::json!([
            {"name": "head-3", "rest": false, "reads": "level"},
            {"name": "head-4", "rest": false, "reads": "level"}
        ]),
    );
    format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::Value::Object(pins)).unwrap()
    )
}

#[test]
fn a_round_over_an_older_installed_expression_gives_the_builds() {
    let mut lamp = installed_applying();
    as_installed(&lamp, EXPRESSION, &older_expression());
    publish_with_settings(&mut lamp, |_| {});
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(EXPRESSION), setting_text(EXPRESSION));
    assert_eq!(
        lamp.state()["settings"][EXPRESSION],
        sha(&setting_text(EXPRESSION))
    );
}

#[test]
fn a_lamps_own_pins_survive_a_round_and_gain_the_builds_new_keys() {
    let mut lamp = installed_applying();
    // Install's record is its default's; the pins are the factory's since.
    changed_on_lamp(&lamp, PINS, &own_pins());
    publish_with_settings(&mut lamp, |_| {});
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    let pins = object(&lamp.read(PINS));
    let own = object(&own_pins());
    assert_eq!(pins["head_pads"], own["head_pads"], "{:?}", lamp.said);
    assert_eq!(pins["head_rest"], own["head_rest"]);
    // The build's key the lamp lacked is added; nothing else changes.
    assert_eq!(pins["privacy"], object(&setting_text(PINS))["privacy"]);
    let mut without = pins.clone();
    without.remove("privacy");
    assert_eq!(without, own);
    assert!(
        lamp.said.iter().any(|line| line
            == &format!(
                "merged {PINS}: added the build's privacy; this lamp's own values are kept"
            )),
        "{:?}",
        lamp.said
    );
    // Still the lamp's own: the next build's pins never replace it whole.
    assert_ne!(lamp.state()["settings"][PINS], sha(&lamp.read(PINS)));
}

/// #7123: lamp-4ace's hand-set pins (no `reset` and no `privacy`)
/// gain the build's privacy section, as install adds it, and
/// never the product's `reset` line, which names the product's wiring.
#[test]
fn a_lamps_own_pins_without_a_reset_line_gain_privacy_and_never_the_builds_reset() {
    let mut lamp = installed_applying();
    let own = include_str!("../../../../deploy/lamp-4ace/controls-pins.json");
    assert!(!object(own).contains_key("reset") && !object(own).contains_key("privacy"));
    assert!(object(&setting_text(PINS)).contains_key("reset"));
    changed_on_lamp(&lamp, PINS, own);
    publish_with_settings(&mut lamp, |_| {});
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    let pins = object(&lamp.read(PINS));
    assert!(!pins.contains_key("reset"), "{pins:?}");
    assert_eq!(pins["privacy"], object(&setting_text(PINS))["privacy"]);
    let mut without = pins.clone();
    without.remove("privacy");
    assert_eq!(without, object(own));
    assert!(
        lamp.said.iter().any(|line| line
            == &format!(
                "merged {PINS}: added the build's privacy; this lamp's own values are kept"
            )),
        "{:?}",
        lamp.said
    );
}

#[test]
fn an_expression_changed_on_the_lamp_is_kept_whole_and_said() {
    let mut lamp = installed_applying();
    let own = format!(
        "{}\n",
        older_expression()
            .trim_end()
            .replace("30000000", "40000000")
    );
    changed_on_lamp(&lamp, EXPRESSION, &own);
    publish_with_settings(&mut lamp, |_| {});
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(EXPRESSION), own);
    assert!(
        lamp.said.iter().any(|line| line
            == &format!("kept {EXPRESSION}: this lamp's own; the build's is not written")),
        "{:?}",
        lamp.said
    );
}
