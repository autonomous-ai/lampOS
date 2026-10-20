use lamp_controls::{
    engine::Config,
    input::{Level, Observation, PinReader},
    mpr121,
    pins::{Pin, PinsConfig, Pull},
    touch_pins::{HEAD_CANDIDATES, HEAD_ITEM, HeadLines, NoGpio, TouchPinsConfig},
};
use lamp_msgs::{ClockStamp, ControlFault};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

fn pin(name: &str, line: u32) -> Pin {
    Pin {
        name: name.into(),
        line,
        active_low: true,
        pull: Pull::Up,
    }
}

// An explicit host-only register profile; never opened or presented as per-unit
// calibration. Full register tests live in mpr121.rs.
fn base() -> mpr121::Config {
    serde_json::from_value(json!({
        "bus":0,"address":90,"every_ms":10,"pads":[11,10,9,8,7,6,5,4,3,2,1,0],
        "profile":{
            "touch_thresholds":([12;12]),"release_thresholds":([6;12]),
            "baseline":{
                "rising":{"mhd":1,"nhd":1,"ncl":14,"fdl":0},
                "falling":{"mhd":1,"nhd":1,"ncl":255,"fdl":2},
                "touched":{"nhd":0,"ncl":0,"fdl":0}
            },
            "ffi":34,"sfi":10,"esi_ms":1,"global_cdc":16,"global_cdt":1,
            "cdc":([0;12]),"cdt":([0;12]),"debounce_touch":2,"debounce_release":2,
            "ecr":143,"autoconfig":{"mode":"disabled"}
        }
    }))
    .unwrap()
}

fn wiring() -> TouchPinsConfig {
    TouchPinsConfig {
        privacy: None,
        chip: "/dev/gpiochip0".into(),
        buttons: vec![pin("primary", 99)],
        head_pads: vec![pin("head-one", 37), pin("head-two", 96)],
        every_ms: 10,
        reset: Some(pin("reset", 100)),
        base: Some(base()),
        head_qualify_ms: None,
        head_tap_min_ms: None,
        head_gap_ms: None,
        head_rest: None,
        legacy_head: false,
    }
}

fn config() -> Config {
    Config {
        enabled: true,
        buttons: vec!["primary".into()],
        head_pads: vec!["head-one".into(), "head-two".into()],
        base_selected_mask: Some(4095),
    }
}

fn observation() -> Observation {
    Observation {
        stamp: ClockStamp {
            clock_id: "test-clock".into(),
            at_us: 123_456,
            sequence: 7,
        },
        session_id: "test-reader".into(),
        // Conversion is by identity, not list position.
        buttons: vec![
            Level {
                name: "reset".into(),
                active: true,
            },
            Level {
                name: "primary".into(),
                active: false,
            },
        ],
        head_pads: vec![
            Level {
                name: "head-one".into(),
                active: true,
            },
            Level {
                name: "head-two".into(),
                active: false,
            },
        ],
        base_mask: None,
    }
}

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = if Path::new("/private/tmp").is_dir() {
            PathBuf::from("/private/tmp")
        } else {
            fs::canonicalize(std::env::temp_dir()).unwrap()
        };
        let path = parent.join(format!(
            "ctp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn explicit_roles_keep_reset_separate_and_strip_out_of_gpio_snapshots() {
    let wiring = wiring();
    let touch = wiring.check(&config()).unwrap();
    assert_eq!(touch.primary.as_deref(), Some("primary"));
    assert_eq!(touch.reset.as_deref(), Some("reset"));
    assert_eq!(touch.strip_pads, Some((0..12).rev().collect()));
    let (gpio, controls) = wiring.gpio_config().unwrap();
    gpio.check(&controls).unwrap();
    assert_eq!(
        gpio.buttons.iter().map(|p| p.line).collect::<Vec<_>>(),
        [99, 100]
    );
    assert!(!controls.enabled);
    assert_eq!(controls.base_selected_mask, None);
    assert_eq!(gpio.head_pads, wiring.head_pads);
}

#[test]
fn all_four_head_candidates_require_explicit_name_order_and_polarity() {
    let mut wiring = wiring();
    wiring.head_pads = HEAD_CANDIDATES
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let mut pin = pin(&format!("head-{i}"), line);
            pin.active_low = i % 2 == 0;
            pin.pull = if i % 2 == 0 { Pull::Up } else { Pull::Down };
            pin
        })
        .collect();
    let mut config = config();
    config.head_pads = wiring.head_pads.iter().map(|p| p.name.clone()).collect();
    wiring.check(&config).unwrap();
    wiring.head_pads.swap(0, 1);
    assert!(wiring.check(&config).is_err());
}

/// #1798: the head pads are the pins file's. The product's pins name all
/// four lines, and a lamp's own settings file, kept by install, names two:
/// Controls still starts and reads all four. The reverse starts too, with
/// the pins' two. A pad both name, named otherwise or moved, is refused.
#[test]
fn the_head_pads_are_the_pins_and_the_settings_may_name_fewer_or_more() {
    let mut four = wiring();
    four.head_pads = HEAD_CANDIDATES
        .into_iter()
        .enumerate()
        .map(|(i, line)| pin(&format!("head-{}", i + 1), line))
        .collect();
    let mut two = config();
    two.head_pads = vec!["head-1".into(), "head-2".into()];
    assert_eq!(
        four.check(&two).unwrap().head_pads,
        ["head-1", "head-2", "head-3", "head-4"]
    );
    let mut fewer = four.clone();
    fewer.head_pads.truncate(2);
    let mut all = two.clone();
    all.head_pads = four.head_pads.iter().map(|p| p.name.clone()).collect();
    assert_eq!(fewer.check(&all).unwrap().head_pads, ["head-1", "head-2"]);
    let mut renamed = four.clone();
    renamed.head_pads[1].name = "head-9".into();
    assert!(renamed.check(&two).is_err());
    let mut moved = four.clone();
    moved.head_pads.swap(1, 2);
    assert!(moved.check(&two).is_err());
}

/// #1798: the probe at Controls' start tells a line a pad drives (the same
/// with either pull) from one with nothing on it (it follows the pull).
/// lamp-52e6's old pins, 37 and 96, are bare there: a warning that the head
/// reads no touch, where before nothing was said. The four lines with its
/// pads on 97 and 98 are well, and say which. A line that couldn't be
/// probed, or a pad left out since the start, is named; the latter warns.
#[test]
fn the_probe_at_start_says_which_head_lines_a_pad_drives() {
    // lamp-52e6: 97 and 98 driven low at rest; 37 and 96 follow the pull.
    let lamp_52e6 = |line: u32| -> std::io::Result<(bool, bool)> {
        Ok(if [97, 98].contains(&line) {
            (false, false)
        } else {
            (false, true)
        })
    };
    let pads = |lines: &[u32]| -> Vec<Pin> {
        lines
            .iter()
            .enumerate()
            .map(|(i, &line)| pin(&format!("head-{}", i + 1), line))
            .collect()
    };
    let value = |report: &lamp_msgs::Diagnostic, key: &str| {
        report
            .values
            .iter()
            .find(|value| value.key == key)
            .map(|value| value.value.clone())
            .unwrap()
    };
    let old = HeadLines::probed(&pads(&[37, 96]), lamp_52e6);
    assert_eq!((old.wired.len(), &old.bare[..]), (0, &[37, 96][..]));
    let report = old.health(&[]);
    assert_eq!(report.name, HEAD_ITEM);
    assert_eq!(report.level, lamp_msgs::Level::Warn);
    assert_eq!(
        report.message,
        "nothing wired on lines 37, 96: the head reads no touch"
    );
    let four = HeadLines::probed(&pads(&HEAD_CANDIDATES), lamp_52e6);
    let report = four.health(&[]);
    assert_eq!(report.level, lamp_msgs::Level::Ok);
    assert_eq!(report.message, "wired on lines 97, 98; nothing on 37, 96");
    assert_eq!(value(&report, "wired"), "97,98");
    assert_eq!(value(&report, "bare"), "37,96");
    // A pad drives its line high under a hand too: still wired.
    let touched = HeadLines::probed(&pads(&[97]), |_| Ok((true, true)));
    assert_eq!(touched.wired, [97]);
    // Held by another program: not probed, and said.
    let held = HeadLines::probed(&pads(&[97, 98]), |line| {
        if line == 98 {
            Err(std::io::Error::other("busy"))
        } else {
            lamp_52e6(line)
        }
    });
    assert_eq!(held.said(), "wired on lines 97; lines 98 not probed");
    let none = HeadLines::probed(&pads(&[97]), |_| Err(std::io::Error::other("busy")));
    assert_eq!(none.health(&[]).level, lamp_msgs::Level::Warn);
    // A pad left out since the start warns, with its line.
    let report = four.health(&[97]);
    assert_eq!(report.level, lamp_msgs::Level::Warn);
    assert_eq!(
        report.message,
        "wired on lines 97, 98; nothing on 37, 96; line 97 touched since Controls started: left out until let go"
    );
    assert_eq!(value(&report, "left_out"), "97");
}

#[test]
fn privacy_chip_and_wrong_primary_reset_or_head_lines_are_rejected() {
    let original = serde_json::to_value(wiring()).unwrap();
    for (pointer, replacement) in [
        ("/chip", json!("/dev/gpiochip1")),
        ("/chip", json!("/dev/gpiochip2")),
        ("/chip", json!("/tmp/gpiochip0")),
        ("/buttons/0/line", json!(100)),
        ("/buttons/0/line", json!(98)),
        ("/reset/line", json!(99)),
        ("/reset/active_low", json!(false)),
        ("/head_pads/0/line", json!(99)),
        ("/head_pads/0/line", json!(100)),
        ("/head_pads/0/line", json!(0)),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        let wiring: TouchPinsConfig = serde_json::from_value(value).unwrap();
        assert!(wiring.check(&config()).is_err(), "{pointer}");
    }
}

#[test]
fn duplicated_names_lines_and_unmatched_configured_inputs_are_rejected() {
    for change in 0..7 {
        let mut wiring = wiring();
        match change {
            0 => wiring.buttons.push(pin("other", 99)),
            1 => wiring.head_pads[1].line = wiring.head_pads[0].line,
            2 => wiring.head_pads[1].name = wiring.head_pads[0].name.clone(),
            3 => wiring.reset.as_mut().unwrap().name = "primary".into(),
            4 => wiring.buttons.clear(),
            5 => wiring.head_pads[0].name = "unknown".into(),
            _ => wiring.reset.as_mut().unwrap().name = "bad\nname".into(),
        }
        assert!(wiring.check(&config()).is_err(), "change {change}");
    }
}

#[test]
fn gpio_and_strip_poll_bounds_are_checked_independently() {
    for every in [2, 50] {
        let mut wiring = wiring();
        wiring.every_ms = every;
        wiring.check(&config()).unwrap();
    }
    for every in [0, 1, 51, u64::MAX] {
        let mut wiring = wiring();
        wiring.every_ms = every;
        assert!(wiring.check(&config()).is_err());
    }
    for every in [5, 50] {
        let mut wiring = wiring();
        wiring.base.as_mut().unwrap().every_ms = every;
        wiring.check(&config()).unwrap();
    }
    for every in [2, 51] {
        let mut wiring = wiring();
        wiring.base.as_mut().unwrap().every_ms = every;
        assert!(wiring.check(&config()).is_err());
    }
}

#[test]
fn strip_presence_working_mask_geometry_and_profile_must_agree() {
    let mut config = config();
    let mut wiring = wiring();
    config.base_selected_mask = None;
    assert!(wiring.check(&config).is_err());
    wiring.base = None;
    wiring.check(&config).unwrap();
    config.base_selected_mask = Some(4095);
    assert!(wiring.check(&config).is_err());
    wiring.base = Some(base());
    for pads in [
        vec![0, 1, 2, 3, 5, 6, 7, 9, 10, 11],
        vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10],
    ] {
        wiring.base.as_mut().unwrap().pads = pads;
        assert!(wiring.check(&config).is_err());
    }
    wiring.base = Some(base());
    wiring.base.as_mut().unwrap().profile.release_thresholds[4] = 12;
    assert!(wiring.check(&config).is_err());
    wiring.base = Some(base());
    config.base_selected_mask = Some(0x0eef);
    assert!(wiring.check(&config).is_err());
}

#[test]
fn sparse_working_electrodes_require_the_exact_configured_mask() {
    for pads in [
        vec![0, 5, 11],
        vec![11, 5, 0],
        vec![0, 1, 2, 3, 5, 6, 7, 9, 10, 11],
        vec![11, 10, 9, 7, 6, 5, 3, 2, 1, 0],
    ] {
        let mut wiring = wiring();
        wiring.base.as_mut().unwrap().pads = pads.clone();
        let selected = pads.iter().fold(0_u16, |mask, pad| mask | (1 << pad));
        let mut config = config();
        config.base_selected_mask = Some(selected);
        let touch = wiring.check(&config).unwrap();
        assert_eq!(touch.strip_pads, Some(pads));
        assert_eq!(touch.primary.as_deref(), Some("primary"));
        assert_eq!(touch.reset.as_deref(), Some("reset"));
        assert_eq!(touch.head_pads, config.head_pads);
        for mismatch in [None, Some(4095), Some(selected ^ 1)] {
            config.base_selected_mask = mismatch;
            assert!(wiring.check(&config).is_err());
        }
    }
}

#[test]
fn sparse_working_configuration_preserves_all_physical_threshold_indices() {
    let mut wiring = wiring();
    let base = wiring.base.as_mut().unwrap();
    base.pads = vec![11, 10, 9, 7, 6, 5, 3, 2, 1, 0];
    // These authored values exercise indexing only, not per-lamp calibration.
    base.profile.touch_thresholds = std::array::from_fn(|pad| 20 + pad as u8);
    base.profile.release_thresholds = std::array::from_fn(|pad| 8 + pad as u8);
    let original = base.profile.clone();
    let mut config = config();
    config.base_selected_mask = Some(0x0eef);
    wiring.check(&config).unwrap();
    let encoded = serde_json::to_vec(&wiring).unwrap();
    let decoded: TouchPinsConfig = serde_json::from_slice(&encoded).unwrap();
    decoded.check(&config).unwrap();
    assert_eq!(decoded.base.as_ref().unwrap().profile, original);
    assert_eq!(original.touch_thresholds[5], 25);
    assert_eq!(original.release_thresholds[9], 17);
    assert_eq!(original.touch_thresholds.len(), 12);
    assert_eq!(original.release_thresholds.len(), 12);
}

#[test]
fn sparse_invalid_or_ambiguous_orders_are_refused_before_wiring_is_accepted() {
    for pads in [
        vec![],
        vec![0, 1],
        (0..13).collect(),
        vec![0, 1, 12],
        vec![0, 1, 1],
        vec![0, 11, 5],
        vec![11, 0, 5],
    ] {
        let mut wiring = wiring();
        wiring.base.as_mut().unwrap().pads = pads.clone();
        let mut config = config();
        config.base_selected_mask = Some(
            pads.iter()
                .filter(|&&pad| pad < 12)
                .fold(0_u16, |mask, pad| mask | (1 << pad)),
        );
        assert!(wiring.check(&config).is_err(), "{pads:?}");
    }
}

#[test]
fn sparse_base_remains_optional_and_does_not_enable_shadow_or_gpio() {
    let mut wiring = wiring();
    wiring.buttons.clear();
    wiring.head_pads.clear();
    wiring.reset = None;
    wiring.base.as_mut().unwrap().pads = vec![0, 1, 2];
    let config = Config {
        enabled: false,
        buttons: vec![],
        head_pads: vec![],
        base_selected_mask: Some(7),
    };
    let touch = wiring.check(&config).unwrap();
    assert!(!touch.enabled);
    assert_eq!(touch.strip_pads, Some(vec![0, 1, 2]));
    assert!(touch.primary.is_none() && touch.reset.is_none() && touch.head_pads.is_empty());
    assert!(wiring.gpio_config().is_none());
    assert_eq!(NoGpio.read(123).unwrap(), None);
}

#[test]
fn base_only_configuration_does_not_invent_gpio_or_released_pads() {
    let mut wiring = wiring();
    wiring.buttons.clear();
    wiring.head_pads.clear();
    wiring.reset = None;
    let config = Config {
        enabled: false,
        buttons: vec![],
        head_pads: vec![],
        base_selected_mask: Some(4095),
    };
    let touch = wiring.check(&config).unwrap();
    assert!(touch.primary.is_none() && touch.reset.is_none() && touch.head_pads.is_empty());
    assert!(wiring.gpio_config().is_none());
    assert_eq!(NoGpio.read(1_000).unwrap(), None);
}

#[test]
fn complete_gpio_conversion_preserves_stamp_session_and_independent_roles() {
    let original = observation();
    let converted = wiring().gpio_sample(original.clone()).unwrap();
    assert_eq!(converted.stamp, original.stamp);
    assert_eq!(converted.session_id, original.session_id);
    assert_eq!(converted.primary, Some(false));
    assert_eq!(converted.reset, Some(true));
    assert_eq!(converted.head_pads, original.head_pads);
}

#[test]
fn partial_or_foreign_gpio_observations_are_not_filled_with_false_levels() {
    for change in 0..5 {
        let mut sample = observation();
        match change {
            0 => {
                sample.buttons.pop();
            }
            1 => sample.buttons[0].name = "unknown-reset".into(),
            2 => sample.buttons.push(Level {
                name: "extra".into(),
                active: false,
            }),
            3 => sample.base_mask = Some(0),
            _ => sample.buttons[1].name = sample.buttons[0].name.clone(),
        }
        let error = wiring().gpio_sample(sample).unwrap_err();
        assert!(matches!(
            error.fault,
            ControlFault::Missing | ControlFault::Invalid
        ));
    }
}

/// #1680: a newer build's field at any depth of the touch path's file is
/// read past, what this build knows reads as before, and the field is named
/// once by its path. The legacy schema stays strict: the touch file is never
/// read as its. A missing known field still refuses.
#[test]
fn new_wiring_reads_a_newer_builds_fields_past_and_the_legacy_schema_stays_strict() {
    let root = Root::new();
    let path = root.0.join("touch.json");
    let original = serde_json::to_value(wiring()).unwrap();
    for (pointer, named) in [
        ("", "unknown"),
        ("/buttons/0", "buttons[0].unknown"),
        ("/reset", "reset.unknown"),
        ("/base", "base.unknown"),
        ("/base/profile", "base.profile.unknown"),
        (
            "/base/profile/autoconfig",
            "base.profile.autoconfig.unknown",
        ),
    ] {
        let mut value = original.clone();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(1));
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let (read, line) = TouchPinsConfig::read(&path).unwrap();
        assert_eq!(read, wiring(), "{pointer}");
        assert_eq!(
            line.as_deref(),
            Some(
                format!(
                    "{}: fields this build does not read, ignored: {named}",
                    path.display()
                )
                .as_str()
            ),
            "{pointer}"
        );
    }
    assert!(serde_json::from_value::<PinsConfig>(original).is_err());
    let mut missing = serde_json::to_value(wiring()).unwrap();
    missing["base"]["profile"]
        .as_object_mut()
        .unwrap()
        .remove("touch_thresholds");
    assert!(serde_json::from_value::<TouchPinsConfig>(missing).is_err());
}

/// The product's settings as the next release might write them.
fn product(name: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../deploy/product/settings")
        .join(name);
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// #2052's audit, the controls row (#1680): the product's `controls.json`
/// and pins file one release ahead, each with a field this build doesn't
/// know, are read as this build reads its own, and the fields are named
/// once. Before, either one stopped the controls at start.
#[test]
fn the_products_files_one_release_ahead_are_read_and_their_new_fields_named() {
    let root = Root::new();
    let mut pins = product("controls-pins.json");
    let ours: TouchPinsConfig = serde_json::from_value(pins.clone()).unwrap();
    pins["head_hold_ms"] = json!(900);
    pins["head_pads"][1]["sensitivity"] = json!("high");
    let path = root.0.join("controls-pins.json");
    fs::write(&path, serde_json::to_vec_pretty(&pins).unwrap()).unwrap();
    let (read, line) = TouchPinsConfig::read(&path).unwrap();
    assert_eq!(read, ours);
    let line = line.unwrap();
    for named in ["head_hold_ms", "head_pads[1].sensitivity"] {
        assert!(line.contains(named), "{line}");
    }
    assert!(!line.contains("high") && !line.contains("900"), "{line}");
    assert_eq!(TouchPinsConfig::load(&path).unwrap(), ours);

    let mut controls = product("controls.json");
    let ours: Config = serde_json::from_value(controls.clone()).unwrap();
    controls["hold_tiers_ms"] = json!([2000, 5000]);
    let path = root.0.join("controls.json");
    let bytes = serde_json::to_vec_pretty(&controls).unwrap();
    let (read, line) = lamp_controls::forward::read::<Config>(&path, &bytes).unwrap();
    assert_eq!(read, ours);
    assert_eq!(
        line.as_deref(),
        Some(
            format!(
                "{}: fields this build does not read, ignored: hold_tiers_ms",
                path.display()
            )
            .as_str()
        )
    );
}

/// Third's design C (#1722): the privacy slide is read only as this build
/// knows it. A field about privacy it doesn't know is refused by name, as
/// any unknown field was before; one inside `privacy` is refused; a file
/// without `privacy` reads as before. A key-like field is refused by name,
/// and a bad known value is refused with serde's category, line and column,
/// never its message, so no value is shown.
#[test]
fn privacy_keys_and_bad_values_are_still_refused_and_no_value_is_shown() {
    let root = Root::new();
    let path = root.0.join("controls-pins.json");
    // The product's pins name the privacy switch since the privacy route
    // went on in the product (9ae36ac4e). These cases start from them
    // without it, a file without `privacy`, and the last reads them whole.
    let mut pins = product("controls-pins.json");
    let switch = pins
        .as_object_mut()
        .unwrap()
        .remove("privacy")
        .expect("the product's pins name the privacy switch");
    let refused = |value: &serde_json::Value| {
        fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
        TouchPinsConfig::read(&path).unwrap_err().to_string()
    };
    for (field, said) in [
        ("privacy_slide", "privacy_slide is about privacy"),
        ("no_slide", "no_slide is about privacy"),
        ("relay_token", "relay_token is named like a key"),
    ] {
        let mut newer = pins.clone();
        newer[field] = json!("sk-not-shown");
        let text = refused(&newer);
        assert!(text.contains(said), "{text}");
        assert!(!text.contains("sk-not-shown"), "{text}");
    }
    // The reviewer on #2076: whatever the value, `false` and `null` too.
    for value in [json!(false), json!(null)] {
        let mut newer = pins.clone();
        newer["mute_on_slide"] = value.clone();
        let text = refused(&newer);
        assert!(
            text.contains("mute_on_slide is about privacy"),
            "{value}: {text}"
        );
    }
    // A reset renamed or misspelt stops the controls, as before, rather
    // than leave the lamp with no reset button.
    let mut renamed = pins.clone();
    let reset = renamed.as_object_mut().unwrap().remove("reset").unwrap();
    renamed["reset_pin"] = reset;
    let text = refused(&renamed);
    assert!(
        text.contains("reset_pin is about the reset button"),
        "{text}"
    );
    let mut misspelt = pins.clone();
    misspelt["rest"] = json!(null);
    misspelt["Reset"] = misspelt["reset"].clone();
    assert!(refused(&misspelt).contains("Reset is about the reset button"));
    let mut inside = pins.clone();
    inside["privacy"] = json!({
        "chip": "/dev/gpiochip1", "line": 9, "active_low": true, "pull": "up",
        "mode": "sk-not-shown"
    });
    let text = refused(&inside);
    assert!(text.contains("not read (data error at line"), "{text}");
    assert!(!text.contains("sk-not-shown"), "{text}");
    let mut bad = pins.clone();
    bad["head_pads"][0]["pull"] = json!("sk-not-shown");
    let text = refused(&bad);
    assert!(text.contains("not read (data error at line"), "{text}");
    assert!(!text.contains("sk-not-shown"), "{text}");
    fs::write(&path, serde_json::to_vec_pretty(&pins).unwrap()).unwrap();
    let (read, line) = TouchPinsConfig::read(&path).unwrap();
    assert_eq!((read.privacy, line), (None, None));
    // As the product ships them, the switch is read with the rest.
    let mut shipped = pins.clone();
    shipped["privacy"] = switch;
    fs::write(&path, serde_json::to_vec_pretty(&shipped).unwrap()).unwrap();
    let (read, line) = TouchPinsConfig::read(&path).unwrap();
    assert!(read.privacy.is_some(), "{read:?}");
    assert_eq!(line, None);
}

#[test]
fn file_size_bound_accepts_exact_limit_and_refuses_over_limit_or_corrupt_input() {
    let root = Root::new();
    let path = root.0.join("touch.json");
    let original = wiring();
    let mut bytes = serde_json::to_vec(&original).unwrap();
    bytes.resize(16_384, b' ');
    fs::write(&path, &bytes).unwrap();
    assert_eq!(TouchPinsConfig::load(&path).unwrap(), original);
    bytes.push(b' ');
    fs::write(&path, &bytes).unwrap();
    assert!(TouchPinsConfig::load(&path).is_err());
    fs::write(&path, b"{broken").unwrap();
    assert!(TouchPinsConfig::load(&path).is_err());
    assert!(TouchPinsConfig::load(&root.0.join("absent.json")).is_err());
}

#[test]
fn empty_configuration_does_not_claim_a_functional_touch_source() {
    let wiring = TouchPinsConfig {
        privacy: None,
        chip: "/dev/gpiochip0".into(),
        buttons: vec![],
        head_pads: vec![],
        every_ms: 10,
        reset: None,
        base: None,
        head_qualify_ms: None,
        head_tap_min_ms: None,
        head_gap_ms: None,
        head_rest: None,
        legacy_head: false,
    };
    assert!(wiring.check(&Config::default()).is_err());
}

/// El on #705 (2026-10-16): on a lamp installed from the product, the reset
/// button did nothing. The product's pins and Controls settings pass the
/// touch path's checks with the reset in its own field, and its unit serves
/// that path, the only one whose reset event `watch-reset` acts on.
#[test]
fn the_products_pins_name_the_reset_and_its_unit_serves_the_touch_path() {
    let pins: TouchPinsConfig = serde_json::from_str(include_str!(
        "../../../deploy/product/settings/controls-pins.json"
    ))
    .unwrap();
    let config: Config = serde_json::from_str(include_str!(
        "../../../deploy/product/settings/controls.json"
    ))
    .unwrap();
    let touch = pins.check(&config).unwrap();
    assert_eq!(touch.reset.as_deref(), Some("reset"));
    assert_eq!(touch.primary.as_deref(), Some("primary"));
    // Every line a head pad is wired to on some unit, as the hardware
    // team's bench test reads them (#1798, #2182).
    assert_eq!(touch.head_pads, ["head-1", "head-2", "head-3", "head-4"]);
    assert_eq!(
        pins.head_pads
            .iter()
            .map(|pin| pin.line)
            .collect::<Vec<_>>(),
        HEAD_CANDIDATES
    );
    assert!(
        pins.head_pads
            .iter()
            .all(|pin| !pin.active_low && pin.pull == Pull::Down),
        "a line with no pad rests low and reads as no touch"
    );
    assert!(pins.head_rest.is_none(), "no unit's baseline is shipped");
    assert!(touch.strip_pads.is_none(), "the base strip waits");
    // A pat stays the old engine's Pet, as the product's voice and
    // Expression read no `controls/touch` (the reviewer on #1167).
    assert!(pins.legacy_head);
    let unit = include_str!("../../../deploy/product/lamp-controls.service");
    let serve = unit
        .lines()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    let words: Vec<&str> = serve.split_whitespace().collect();
    assert_eq!(&words[1..3], ["serve", "--touch"], "{serve}");
    assert!(
        words.contains(&"--pins") && words.contains(&"--enable"),
        "{serve}"
    );
}

/// #711: each lamp's factory step writes its pads' own baseline, which must
/// name the pads in order and agree with their polarity; a pad that pulses
/// is read by its pulses.
#[test]
fn head_rest_names_each_pad_in_order_with_its_polarity_and_how_it_reads() {
    let rest = |rests: serde_json::Value| {
        let mut wiring = serde_json::to_value(wiring()).unwrap();
        wiring["head_rest"] = rests;
        serde_json::from_value::<TouchPinsConfig>(wiring).unwrap()
    };
    // Both pads are active-low, so both rest high.
    let measured = rest(json!([
        {"name":"head-one","rest":"high","reads":"pulses"},
        {"name":"head-two","rest":"high","reads":"level"}
    ]));
    let touch = measured.check(&config()).unwrap();
    assert_eq!(touch.head_pulse_pads, ["head-one"]);
    for (rests, why) in [
        (
            json!([
                {"name":"head-two","rest":"high","reads":"level"},
                {"name":"head-one","rest":"high","reads":"level"}
            ]),
            "out of order",
        ),
        (
            json!([{"name":"head-one","rest":"high","reads":"level"}]),
            "one pad missing",
        ),
        (
            json!([
                {"name":"head-one","rest":"low","reads":"level"},
                {"name":"head-two","rest":"high","reads":"level"}
            ]),
            "resting low but active-low",
        ),
    ] {
        assert!(rest(rests).check(&config()).is_err(), "{why}");
    }
    // A newer build's field in a pad's baseline is read past and named
    // (#1680).
    let mut unknown = serde_json::to_value(&measured).unwrap();
    unknown["head_rest"][0]["idle"] = json!("high");
    let file = Path::new("/etc/lampOS/controls/controls-pins.json");
    let bytes = serde_json::to_vec(&unknown).unwrap();
    let (read, line) = lamp_controls::forward::read::<TouchPinsConfig>(file, &bytes).unwrap();
    assert_eq!(read, measured);
    assert!(line.unwrap().ends_with("ignored: head_rest[0].idle"));
    // Without it, every pad reads its level, as before.
    assert!(
        wiring()
            .check(&config())
            .unwrap()
            .head_pulse_pads
            .is_empty()
    );
}
