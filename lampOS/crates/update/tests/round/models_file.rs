//! The models switch as a file a round carries (`manifest::MODELS_ENV`,
//! same-voice step 2): read by lamp-update itself when its environment
//! doesn't set `LAMP_UPDATE_MODELS`, so a lamp in a home turns the models on
//! without its own unit changing. The environment always wins.
use super::*;

const ON: &str = include_str!("../../../../deploy/product/settings/models.env");

/// A product lamp writing its build's settings (`lamp_with_settings`), with
/// the eyes' face model install placed (`with_models`), and no
/// `LAMP_UPDATE_MODELS` in lamp-update's environment.
fn lamp() -> Lamp {
    let (lamp, _) = lamp_with_settings();
    let before = lamp.state();
    let mut lamp = lamp.with_models();
    let mut state = lamp.state();
    state["installed"] = before["installed"].clone();
    for (path, sha) in before["settings"].as_object().unwrap() {
        state["settings"][path] = sha.clone();
    }
    fs::write(lamp.records().join("state.json"), state.to_string()).unwrap();
    lamp.config.models = false;
    lamp.config.models_file = true;
    lamp
}

/// Build `sequence`: eyes `eyes`, these models and these settings, signed.
fn publish(lamp: &mut Lamp, sequence: u64, eyes: &str, models: Vec<Model>, settings: Vec<Setting>) {
    let mut manifest = lamp.manifest(
        sequence,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
            ("lamp-eyes", eyes),
        ],
        &[],
    );
    manifest.min_updater = Some(manifest::MODELS_ENV_UPDATER);
    manifest.models = models;
    manifest.settings = settings;
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let envelope = manifest::seal(&bytes, &lamp.key).unwrap();
    lamp.channel
        .files
        .insert(format!("{CHANNEL}{}", manifest::ENVELOPE), envelope);
}

fn put(lamp: &Lamp, text: &str) {
    let at = lamp.root.join(manifest::MODELS_ENV.trim_start_matches('/'));
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    fs::write(at, text).unwrap();
}

fn waits(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::WaitsForModels(why)
        if why.contains("LAMP_UPDATE_MODELS=on") && why.contains(manifest::MODELS_ENV))
}

/// The build that first carries the file, on, takes its models in the same
/// round, and writes the file; a later build without it is read from the
/// lamp's file, and its models are fetched too.
#[test]
fn the_file_on_with_no_environment_fetches_the_models() {
    let mut lamp = lamp();
    assert!(!lamp.exists(manifest::MODELS_ENV));
    let face = lamp.model(FACE, "face 2");
    publish(
        &mut lamp,
        2,
        "eyes 2",
        vec![face],
        vec![Setting::new(manifest::MODELS_ENV, ON)],
    );
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(FACE), "face 2");
    assert!(lamp.is_link(FACE));
    assert_eq!(lamp.read(manifest::MODELS_ENV), ON);
    assert_eq!(lamp.fetched_models().len(), 1);

    let face = lamp.model(FACE, "face 3");
    lamp.publish_with_models(3, "eyes 2", vec![face]);
    assert_eq!(lamp.round(), Outcome::Switched(3), "{:?}", lamp.said);
    assert_eq!(lamp.read(FACE), "face 3");
    assert_eq!(lamp.fetched_models().len(), 2);
}

/// `LAMP_UPDATE_MODELS=off` (or any value) in the environment beats the
/// file, carried or on the lamp: the build waits, nothing fetched, and the
/// file the build carries is not even read for the switch.
#[test]
fn the_environment_off_beats_the_file() {
    for on_lamp in [false, true] {
        let mut lamp = lamp();
        lamp.config.models_file = false;
        if on_lamp {
            put(&lamp, ON);
        }
        let face = lamp.model(FACE, "face 2");
        publish(
            &mut lamp,
            2,
            "eyes 2",
            vec![face],
            vec![Setting::new(manifest::MODELS_ENV, ON)],
        );
        let before = lamp.lamp_files();
        let outcome = lamp.round();
        assert!(waits(&outcome), "on the lamp {on_lamp}: {outcome:?}");
        assert!(lamp.fetched_models().is_empty());
        assert_eq!(lamp.lamp_files(), before);
        assert_eq!(lamp.read(FACE), "face 1");
    }
}

/// A missing file, a garbled one on the lamp, or a garbled or off one the
/// build carries: off, the build waits for the switch.
#[test]
fn a_missing_or_garbled_file_reads_as_off() {
    let cases: [(Option<&str>, Option<&str>); 6] = [
        (None, None),
        (Some("LAMP_UPDATE_MODELS=yes\n"), None),
        (Some("\u{0}\u{1} not an env file"), None),
        (Some("LAMP_UPDATE_MODELS=\"on\"\n"), None),
        (None, Some("LAMP_UPDATE_MODELS=on and more\n")),
        (None, Some("LAMP_UPDATE_MODELS=off\n")),
    ];
    for (on_lamp, carried) in cases {
        let mut lamp = lamp();
        if let Some(text) = on_lamp {
            put(&lamp, text);
        }
        let face = lamp.model(FACE, "face 2");
        let settings = carried
            .map(|text| vec![Setting::new(manifest::MODELS_ENV, text)])
            .unwrap_or_default();
        publish(&mut lamp, 2, "eyes 2", vec![face], settings);
        let outcome = lamp.round();
        assert!(waits(&outcome), "{on_lamp:?} {carried:?}: {outcome:?}");
        assert!(lamp.fetched_models().is_empty());
        assert_eq!(lamp.read(FACE), "face 1");
    }
}

/// The file is the lamp's once changed there: a round keeps it, and the
/// switch reads the lamp's, not the build's.
#[test]
fn a_file_changed_on_the_lamp_is_kept_and_decides() {
    let mut lamp = lamp();
    put(&lamp, "LAMP_UPDATE_MODELS=off\n");
    let mut state = lamp.state();
    state["settings"][manifest::MODELS_ENV] = sha(ON).into();
    fs::write(lamp.records().join("state.json"), state.to_string()).unwrap();
    let face = lamp.model(FACE, "face 2");
    publish(
        &mut lamp,
        2,
        "eyes 2",
        vec![face],
        vec![Setting::new(manifest::MODELS_ENV, ON)],
    );
    let outcome = lamp.round();
    assert!(waits(&outcome), "{outcome:?}");
    assert!(lamp.fetched_models().is_empty());
    assert_eq!(lamp.read(manifest::MODELS_ENV), "LAMP_UPDATE_MODELS=off\n");
}

/// Pinned as every setting is, by its SHA-256 in the signed manifest: a
/// text that isn't its SHA-256 is refused before a manifest is even sealed
/// (`manifest::check_settings`), and an envelope whose file was altered on
/// the way, the switch turned on in it, is refused whole, nothing fetched or
/// written.
#[test]
fn the_carried_file_is_hash_pinned_and_refused_if_altered() {
    let mut altered = Setting::new(manifest::MODELS_ENV, "LAMP_UPDATE_MODELS=off\n");
    altered.text = Some(ON.into());
    assert!(
        manifest::check_settings(&[altered])
            .is_err_and(|why| why.0.contains("does not match its SHA-256"))
    );

    use base64::Engine as _;
    let base64 = base64::engine::general_purpose::STANDARD;
    let mut lamp = lamp();
    let face = lamp.model(FACE, "face 2");
    let off = "LAMP_UPDATE_MODELS=of\n";
    publish(
        &mut lamp,
        2,
        "eyes 2",
        vec![face],
        vec![Setting::new(manifest::MODELS_ENV, off)],
    );
    let at = format!("{CHANNEL}{}", manifest::ENVELOPE);
    let mut envelope: serde_json::Value = serde_json::from_slice(&lamp.channel.files[&at]).unwrap();
    let signed = base64
        .decode(envelope["manifest"].as_str().unwrap())
        .unwrap();
    let text = String::from_utf8(signed).unwrap();
    assert!(text.contains("LAMP_UPDATE_MODELS=of\\n"));
    let changed = text.replace("LAMP_UPDATE_MODELS=of\\n", "LAMP_UPDATE_MODELS=on");
    envelope["manifest"] = base64.encode(changed).into();
    lamp.channel
        .files
        .insert(at, serde_json::to_vec(&envelope).unwrap());
    let before = lamp.lamp_files();
    let outcome = lamp.round();
    assert!(matches!(&outcome, Outcome::Refused(_)), "{outcome:?}");
    assert!(lamp.fetched_models().is_empty());
    assert_eq!(lamp.lamp_files(), before);
    assert!(!lamp.exists(manifest::MODELS_ENV));
}
