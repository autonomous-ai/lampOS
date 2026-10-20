use lamp_controls::mpr121::{AutoConfig, CONFIG_PARTS, Config, Device, Error, RegisterIo};
use serde_json::{Value, json};
use std::{cell::RefCell, io, rc::Rc};

// First's lamp-4ace register snapshot, 2026-10-14T10:45:53Z. This is an
// authored host fixture, not a universal profile or a Third measurement.
const PROFILE: &str = r#"{
  "bus":0,"address":90,"every_ms":10,
  "pads":[11,10,9,8,7,6,5,4,3,2,1,0],
  "profile":{
    "touch_thresholds":[6,6,6,6,6,6,6,6,6,6,6,6],
    "release_thresholds":[4,4,4,4,4,4,4,4,4,4,4,4],
    "baseline":{
      "rising":{"mhd":1,"nhd":1,"ncl":14,"fdl":0},
      "falling":{"mhd":1,"nhd":1,"ncl":255,"fdl":2},
      "touched":{"nhd":0,"ncl":0,"fdl":0}
    },
    "ffi":34,"sfi":10,"esi_ms":1,
    "global_cdc":16,"global_cdt":1,
    "cdc":[53,57,55,51,31,50,49,42,31,41,46,47],
    "cdt":[1,1,1,1,1,1,1,1,1,1,1,1],
    "debounce_touch":2,"debounce_release":2,"ecr":143,
    "autoconfig":{
      "mode":"enabled","upper_limit":200,"lower_limit":130,"target":180,
      "reconfigure":true,"skip_cdt_search":false
    }
  }
}"#;

fn config() -> Config {
    serde_json::from_str(PROFILE).unwrap()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Read(u8, usize),
    Write(u8, u8),
    Delay(u64),
}

struct Rig {
    registers: [u8; 129],
    events: Vec<Event>,
    operations: usize,
    fail_at: Option<usize>,
    ignore_write: Option<u8>,
    ignore_reset: bool,
    after_run: Vec<(u8, u8)>,
    mutate_at: Option<(usize, Vec<(u8, u8)>)>,
}

impl Rig {
    fn new() -> Self {
        let mut rig = Self {
            registers: [0; 129],
            events: Vec::new(),
            operations: 0,
            fail_at: None,
            ignore_write: None,
            ignore_reset: false,
            after_run: Vec::new(),
            mutate_at: None,
        };
        rig.reset();
        rig
    }

    fn reset(&mut self) {
        self.registers.fill(0);
        self.registers[0x5c] = 0x10;
        self.registers[0x5d] = 0x24;
    }

    fn operation(&mut self, event: Event) -> io::Result<()> {
        self.events.push(event);
        self.operations += 1;
        if self.fail_at == Some(self.operations) {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "injected partial transfer",
            ));
        }
        if self
            .mutate_at
            .as_ref()
            .is_some_and(|(at, _)| *at == self.operations)
        {
            for (register, value) in self.mutate_at.take().unwrap().1 {
                self.registers[usize::from(register)] = value;
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Io(Rc<RefCell<Rig>>);

impl RegisterIo for Io {
    fn read(&mut self, register: u8, bytes: &mut [u8]) -> io::Result<()> {
        let mut rig = self.0.borrow_mut();
        rig.operation(Event::Read(register, bytes.len()))?;
        assert!(bytes.len() <= 85);
        let start = usize::from(register);
        bytes.copy_from_slice(&rig.registers[start..start + bytes.len()]);
        Ok(())
    }

    fn write(&mut self, register: u8, value: u8) -> io::Result<()> {
        let mut rig = self.0.borrow_mut();
        rig.operation(Event::Write(register, value))?;
        if register == 0x80 && value == 0x63 {
            if !rig.ignore_reset {
                rig.reset();
            }
            return Ok(());
        }
        if register != 0x5e {
            assert_eq!(
                rig.registers[0x5e] & 0x3f,
                0,
                "configuration write while running"
            );
        }
        if rig.ignore_write != Some(register) {
            rig.registers[usize::from(register)] = value;
        }
        if register == 0x5e && value & 0x3f != 0 {
            for (register, value) in rig.after_run.clone() {
                rig.registers[usize::from(register)] = value;
            }
        }
        Ok(())
    }
}

fn rig() -> Io {
    Io(Rc::new(RefCell::new(Rig::new())))
}

fn initialize(io: &Io, config: Config) -> Result<Device<Io>, Error> {
    let shared = io.0.clone();
    Device::initialize(io.clone(), config, &mut move |us| {
        shared.borrow_mut().events.push(Event::Delay(us))
    })
}

fn device() -> (Io, Device<Io>) {
    let io = rig();
    let device = initialize(&io, config()).unwrap();
    (io, device)
}

/// The transfers of `count` served reads, the first checking part `first`
/// of the static configuration. Initialization's own validation read
/// checks part 0, so a device's first served read checks part 1.
fn served(first: usize, count: usize) -> Vec<Event> {
    (first..first + count)
        .flat_map(|part| {
            let part = (part % CONFIG_PARTS) as u8;
            [
                Event::Read(0, 4),
                Event::Read(0x2b + 17 * part, 17),
                Event::Read(0, 4),
                Event::Read(0x5e, 1),
            ]
        })
        .collect()
}

/// The first fault in a whole turn of the configuration's parts.
fn fault_within_a_turn(device: &mut Device<Io>) -> Error {
    for _ in 0..CONFIG_PARTS {
        if let Err(error) = device.read() {
            return error;
        }
    }
    panic!("no fault within {CONFIG_PARTS} reads");
}

/// A whole turn of reads, each `Ok(mask)`.
fn a_turn_reads(device: &mut Device<Io>, mask: u16) {
    for _ in 0..CONFIG_PARTS {
        assert_eq!(device.read().unwrap(), mask);
    }
}

fn mismatch(error: Error, register: u8) {
    assert!(
        matches!(error, Error::RegisterMismatch { register: found, .. } if found == register),
        "{error}"
    );
}

#[test]
fn explicit_profile_roundtrips_without_hidden_defaults() {
    let config = config();
    config.check().unwrap();
    assert_eq!(
        serde_json::from_value::<Config>(serde_json::to_value(&config).unwrap()).unwrap(),
        config
    );
    let mut value: Value = serde_json::from_str(PROFILE).unwrap();
    value["profile"]
        .as_object_mut()
        .unwrap()
        .remove("autoconfig");
    assert!(serde_json::from_value::<Config>(value).is_err());
}

/// #1680: the strip's section of a newer build's pins file is read one
/// release forward. A field this build doesn't know, at every layer, is
/// read past and named by its path; what it knows reads as before.
#[test]
fn unknown_fields_at_every_configuration_layer_are_read_past_and_named() {
    let ours: Config = serde_json::from_str(PROFILE).unwrap();
    let file = std::path::Path::new("/etc/lampOS/controls/controls-pins.json");
    for (pointer, named) in [
        ("", "typo"),
        ("/profile", "profile.typo"),
        ("/profile/baseline", "profile.baseline.typo"),
        ("/profile/baseline/rising", "profile.baseline.rising.typo"),
        ("/profile/baseline/falling", "profile.baseline.falling.typo"),
        ("/profile/baseline/touched", "profile.baseline.touched.typo"),
        ("/profile/autoconfig", "profile.autoconfig.typo"),
    ] {
        let mut value: Value = serde_json::from_str(PROFILE).unwrap();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("typo".into(), json!(1));
        let bytes = serde_json::to_vec(&value).unwrap();
        let (read, line) = lamp_controls::forward::read::<Config>(file, &bytes).unwrap();
        assert_eq!(read, ours, "{pointer}");
        assert!(
            line.unwrap().ends_with(&format!("ignored: {named}")),
            "{pointer}"
        );
    }
    let mut value: Value = serde_json::from_str(PROFILE).unwrap();
    value["profile"]["autoconfig"] = json!({"mode":"disabled","target":180});
    let bytes = serde_json::to_vec(&value).unwrap();
    let (read, line) = lamp_controls::forward::read::<Config>(file, &bytes).unwrap();
    assert_eq!(read.profile.autoconfig, AutoConfig::Disabled {});
    assert!(
        line.unwrap()
            .ends_with("ignored: profile.autoconfig.target")
    );
    // A mode it doesn't know is a known field's bad value: refused.
    value["profile"]["autoconfig"] = json!({"mode":"adaptive"});
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(lamp_controls::forward::read::<Config>(file, &bytes).is_err());
}

#[test]
fn malformed_numeric_enum_array_and_missing_fields_are_rejected() {
    for (pointer, replacement) in [
        ("/bus", json!(-1)),
        ("/bus", json!(256)),
        ("/bus", json!(false)),
        ("/address", json!(90.5)),
        ("/every_ms", json!("10")),
        ("/profile/touch_thresholds", json!([6, 6])),
        (
            "/profile/cdt",
            json!([1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]),
        ),
        ("/profile/autoconfig/mode", json!("guessed")),
        ("/profile/global_cdc", json!(256)),
    ] {
        let mut value: Value = serde_json::from_str(PROFILE).unwrap();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            serde_json::from_value::<Config>(value).is_err(),
            "{pointer}"
        );
    }
    for field in ["bus", "address", "every_ms", "pads", "profile"] {
        let mut value: Value = serde_json::from_str(PROFILE).unwrap();
        value.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<Config>(value).is_err(), "{field}");
    }
}

#[test]
fn invalid_settings_are_rejected_before_any_io_or_delay() {
    let original: Value = serde_json::from_str(PROFILE).unwrap();
    for (pointer, replacement) in [
        ("/address", json!(0x59)),
        ("/address", json!(0x5e)),
        ("/every_ms", json!(4)),
        ("/every_ms", json!(51)),
        ("/pads", json!([0, 1])),
        ("/pads", json!([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10])),
        ("/pads", json!([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12])),
        ("/profile/release_thresholds/5", json!(6)),
        ("/profile/touch_thresholds/5", json!(0)),
        ("/profile/baseline/rising/mhd", json!(0)),
        ("/profile/baseline/rising/nhd", json!(64)),
        ("/profile/baseline/falling/mhd", json!(255)),
        ("/profile/baseline/falling/nhd", json!(0)),
        ("/profile/baseline/touched/nhd", json!(64)),
        ("/profile/ffi", json!(7)),
        ("/profile/sfi", json!(5)),
        ("/profile/esi_ms", json!(0)),
        ("/profile/esi_ms", json!(3)),
        ("/profile/global_cdc", json!(0)),
        ("/profile/global_cdc", json!(64)),
        ("/profile/global_cdt", json!(0)),
        ("/profile/global_cdt", json!(8)),
        ("/profile/cdc/0", json!(64)),
        ("/profile/cdt/11", json!(8)),
        ("/profile/debounce_touch", json!(8)),
        ("/profile/debounce_release", json!(8)),
        ("/profile/ecr", json!(0)),
        ("/profile/ecr", json!(0x0f)),
        ("/profile/ecr", json!(0x9f)),
        ("/profile/ecr", json!(0xcf)),
        ("/profile/autoconfig/upper_limit", json!(255)),
        ("/profile/autoconfig/upper_limit", json!(180)),
        ("/profile/autoconfig/lower_limit", json!(0)),
        ("/profile/autoconfig/lower_limit", json!(180)),
        ("/profile/autoconfig/target", json!(201)),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        let config = serde_json::from_value::<Config>(value).unwrap();
        let io = rig();
        assert!(
            matches!(initialize(&io, config), Err(Error::InvalidConfig(_))),
            "{pointer}"
        );
        assert!(io.0.borrow().events.is_empty(), "{pointer}");
    }
}

#[test]
fn all_addresses_poll_boundaries_and_all12_ecr_encodings_are_valid() {
    for address in 0x5a..=0x5d {
        for every_ms in [5, 10, 20, 50] {
            for ecr in 0x8c..=0x8f {
                let mut config = config();
                config.address = address;
                config.every_ms = every_ms;
                config.profile.ecr = ecr;
                initialize(&rig(), config).unwrap();
            }
        }
    }
}

#[test]
fn working_sets_accept_three_through_twelve_with_shared_geometry_validation() {
    for count in 3..=12 {
        let mut config = config();
        config.pads = (0..count).collect();
        for _ in 0..2 {
            config.check().unwrap();
            let serialized = serde_json::to_string(&config).unwrap();
            assert_eq!(serde_json::from_str::<Config>(&serialized).unwrap(), config);
            config.pads.reverse();
        }
    }
    let mut config = config();
    config.pads = vec![0, 5, 11];
    config.check().unwrap();
    config.pads.reverse();
    config.check().unwrap();
    // Existing complete physical permutations retain their old meaning.
    config.pads = vec![4, 0, 11, 2, 1, 8, 6, 10, 9, 7, 5, 3];
    initialize(&rig(), config).unwrap();
}

#[test]
fn invalid_working_sets_fail_before_any_io_or_delay() {
    for pads in [
        vec![],
        vec![0],
        vec![0, 11],
        vec![0, 4, 4],
        vec![0, 4, 12],
        vec![0, 5, 255],
        vec![0, 11, 5],
        vec![11, 0, 5],
        (0..13).collect(),
    ] {
        let mut config = config();
        config.pads = pads;
        let io = rig();
        assert!(matches!(
            initialize(&io, config),
            Err(Error::InvalidConfig(_))
        ));
        assert!(io.0.borrow().events.is_empty());
    }
}

#[test]
fn sparse_selection_does_not_change_electrical_scan_or_startup_wait() {
    let all = rig();
    initialize(&all, config()).unwrap();
    let mut config = config();
    config.pads = vec![0, 5, 11];
    assert_eq!(config.settle_us().unwrap(), 1_044_480);
    let sparse = rig();
    initialize(&sparse, config).unwrap();
    assert_eq!(sparse.0.borrow().events, all.0.borrow().events);
    assert_eq!(sparse.0.borrow().registers, all.0.borrow().registers);
}

#[test]
fn sparse_thresholds_stay_indexed_by_electrode_including_excluded_channels() {
    let mut config = config();
    config.pads = vec![11, 10, 9, 7, 6, 5, 3, 2, 1, 0];
    // Artificial distinct values exercise addressing, not lamp calibration.
    config.profile.touch_thresholds = [12, 21, 32, 45, 56, 67, 78, 89, 90, 101, 112, 123];
    config.profile.release_thresholds = [3, 5, 8, 11, 14, 17, 20, 23, 26, 29, 32, 35];
    let io = rig();
    initialize(&io, config.clone()).unwrap();
    let writes: Vec<_> =
        io.0.borrow()
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Write(register @ 0x41..=0x58, value) => Some((*register, *value)),
                _ => None,
            })
            .collect();
    assert_eq!(writes.len(), 24);
    for electrode in 0..12 {
        assert_eq!(
            writes[2 * electrode],
            (
                0x41 + 2 * electrode as u8,
                config.profile.touch_thresholds[electrode]
            )
        );
        assert_eq!(
            writes[2 * electrode + 1],
            (
                0x42 + 2 * electrode as u8,
                config.profile.release_thresholds[electrode]
            )
        );
    }
}

#[test]
fn excluded_thresholds_are_still_required_and_validated() {
    let mut value: Value = serde_json::from_str(PROFILE).unwrap();
    value["pads"] = json!([0, 5, 11]);
    value["profile"]["touch_thresholds"] = json!([12, 67, 123]);
    assert!(serde_json::from_value::<Config>(value).is_err());
    for excluded in [4, 8] {
        let mut config = config();
        config.pads = vec![0, 5, 11];
        config.profile.release_thresholds[excluded] = config.profile.touch_thresholds[excluded];
        let io = rig();
        assert!(matches!(
            initialize(&io, config),
            Err(Error::InvalidConfig(_))
        ));
        assert!(io.0.borrow().events.is_empty());
    }
}

#[test]
fn all_ecr_values_are_checked_without_reinterpreting_software_selection() {
    let mut config = config();
    config.pads = vec![0, 5, 11];
    for ecr in 0..=u8::MAX {
        config.profile.ecr = ecr;
        assert_eq!(
            config.check().is_ok(),
            matches!(ecr, 0x8c..=0x8f),
            "ECR=0x{ecr:02x}"
        );
    }
}

#[test]
fn explicit_afe_charge_and_debounce_fields_keep_exact_register_bit_positions() {
    let mut config = config();
    config.pads = vec![0, 5, 11];
    config.profile.autoconfig = AutoConfig::Disabled {};
    config.profile.global_cdc = 37;
    config.profile.global_cdt = 3;
    config.profile.debounce_touch = 0;
    config.profile.debounce_release = 0;
    config.profile.cdc = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 63];
    config.profile.cdt = [1, 2, 3, 4, 5, 6, 7, 0, 1, 2, 3, 4];
    for (ffi, ffi_bits) in [(6, 0x00), (10, 0x40), (18, 0x80), (34, 0xc0)] {
        for (sfi, sfi_bits) in [(4, 0x00), (6, 0x08), (10, 0x10), (18, 0x18)] {
            for (esi_ms, esi_bits) in [
                (1, 0),
                (2, 1),
                (4, 2),
                (8, 3),
                (16, 4),
                (32, 5),
                (64, 6),
                (128, 7),
            ] {
                config.profile.ffi = ffi;
                config.profile.sfi = sfi;
                config.profile.esi_ms = esi_ms;
                let io = rig();
                initialize(&io, config.clone()).unwrap();
                let rig = io.0.borrow();
                assert_eq!(rig.registers[0x5c], ffi_bits | 37);
                assert_eq!(rig.registers[0x5d], 0x60 | sfi_bits | esi_bits);
                assert_eq!(rig.registers[0x5f..0x6b], config.profile.cdc);
                assert_eq!(
                    rig.registers[0x6c..0x72],
                    [0x21, 0x43, 0x65, 0x07, 0x21, 0x43]
                );
            }
        }
    }
    for (touch, release, encoded) in [(7, 0, 0x07), (0, 7, 0x70), (3, 5, 0x53)] {
        config.profile.ffi = 6;
        config.profile.sfi = 4;
        config.profile.esi_ms = 1;
        config.profile.debounce_touch = touch;
        config.profile.debounce_release = release;
        let io = rig();
        initialize(&io, config.clone()).unwrap();
        assert_eq!(io.0.borrow().registers[0x5b], encoded);
    }
}

#[test]
fn settle_uses_filter_scan_debounce_and_auto_search_bounds() {
    let mut config = config();
    assert_eq!(config.settle_us().unwrap(), 1_044_480);
    config.profile.autoconfig = AutoConfig::Disabled {};
    assert_eq!(config.settle_us().unwrap(), 100_000);
    config.profile.debounce_release = 7;
    config.profile.esi_ms = 4;
    assert_eq!(config.settle_us().unwrap(), 360_000);
    config.profile.cdt[5] = 7;
    assert_eq!(config.settle_us().unwrap(), 2_350_080);
    config.profile.cdt.fill(0);
    config.profile.global_cdt = 7;
    assert_eq!(config.settle_us().unwrap(), 2_350_080);
}

#[test]
fn sensing_wait_over_five_seconds_is_rejected_before_io() {
    let mut config = config();
    config.profile.autoconfig = AutoConfig::Disabled {};
    config.profile.sfi = 18;
    config.profile.esi_ms = 128;
    config.profile.debounce_touch = 0;
    config.profile.debounce_release = 0;
    assert_eq!(config.settle_us().unwrap(), 4_608_000);
    config.profile.debounce_release = 1;
    let io = rig();
    assert!(matches!(
        initialize(&io, config),
        Err(Error::InvalidConfig(_))
    ));
    assert!(io.0.borrow().events.is_empty());
}

#[test]
fn reset_stop_profile_readback_then_run_and_settle_are_ordered() {
    let io = rig();
    io.0.borrow_mut().registers[0x5e] = 0x8f;
    initialize(&io, config()).unwrap();
    let rig = io.0.borrow();
    assert_eq!(
        &rig.events[..4],
        &[
            Event::Read(0, 4),
            Event::Write(0x80, 0x63),
            Event::Delay(1_000),
            Event::Write(0x5e, 0)
        ]
    );
    let writes: Vec<_> = rig
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Write(register, value) => Some((*register, *value)),
            _ => None,
        })
        .collect();
    assert_eq!(&writes[..2], &[(0x80, 0x63), (0x5e, 0)]);
    assert_eq!(writes.last(), Some(&(0x5e, 0x8f)));
    assert_eq!(
        writes
            .iter()
            .filter(|(register, _)| *register == 0x5e)
            .count(),
        2
    );
    assert_eq!(
        rig.registers[0x2b..0x36],
        [1, 1, 14, 0, 1, 1, 255, 2, 0, 0, 0]
    );
    assert_eq!(rig.registers[0x5b..0x5f], [0x22, 0xd0, 0x30, 0x8f]);
    assert_eq!(rig.registers[0x7b..0x80], [0xcb, 0, 200, 130, 180]);
    assert_eq!(
        rig.registers[0x5f..0x6b],
        [53, 57, 55, 51, 31, 50, 49, 42, 31, 41, 46, 47]
    );
    assert_eq!(rig.registers[0x6c..0x72], [0x11; 6]);
    let run = rig
        .events
        .iter()
        .position(|event| *event == Event::Write(0x5e, 0x8f))
        .unwrap();
    assert_eq!(rig.events[run - 1], Event::Read(0x2b, 85));
    assert_eq!(rig.events[run + 1], Event::Delay(1_044_480));
    assert!(
        !rig.events[run + 1..]
            .iter()
            .any(|event| matches!(event, Event::Write(..)))
    );
}

#[test]
fn no_threshold_or_baseline_value_is_silently_replaced() {
    let mut config = config();
    config.profile.touch_thresholds = [9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
    config.profile.release_thresholds = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    config.profile.baseline.falling.ncl = 87;
    let io = rig();
    initialize(&io, config.clone()).unwrap();
    for electrode in 0..12 {
        assert_eq!(
            io.0.borrow().registers[0x41 + 2 * electrode],
            config.profile.touch_thresholds[electrode]
        );
        assert_eq!(
            io.0.borrow().registers[0x42 + 2 * electrode],
            config.profile.release_thresholds[electrode]
        );
    }
    assert_eq!(io.0.borrow().registers[0x31], 87);
}

#[test]
fn every_initialization_transfer_error_stops_without_retry() {
    let reference = rig();
    initialize(&reference, config()).unwrap();
    let operations = reference.0.borrow().operations;
    for fail_at in 1..=operations {
        let io = rig();
        io.0.borrow_mut().fail_at = Some(fail_at);
        assert!(
            matches!(initialize(&io, config()), Err(Error::Io(_))),
            "transfer {fail_at}"
        );
        assert_eq!(io.0.borrow().operations, fail_at);
    }
}

#[test]
fn active_overcurrent_is_not_cleared_by_reset() {
    let io = rig();
    io.0.borrow_mut().registers[1] = 0x80;
    assert!(matches!(initialize(&io, config()), Err(Error::OverCurrent)));
    assert_eq!(io.0.borrow().events, vec![Event::Read(0, 4)]);
}

#[test]
fn ignored_reset_cannot_pass_with_one_expected_default() {
    let io = rig();
    io.0.borrow_mut().ignore_reset = true;
    // CONFIG2 still looks reset; another retained register exposes the failure.
    io.0.borrow_mut().registers[0x41] = 6;
    mismatch(initialize(&io, config()).err().unwrap(), 0x41);
}

#[test]
fn acknowledged_but_ignored_profile_writes_are_detected_before_run() {
    for register in [
        0x2b, 0x2c, 0x2d, 0x2f, 0x31, 0x41, 0x42, 0x5b, 0x5c, 0x5d, 0x5f, 0x6c, 0x7b, 0x7d, 0x7e,
        0x7f,
    ] {
        let io = rig();
        io.0.borrow_mut().ignore_write = Some(register);
        mismatch(initialize(&io, config()).err().unwrap(), register);
        assert!(!io.0.borrow().events.contains(&Event::Write(0x5e, 0x8f)));
    }
}

#[test]
fn ignored_run_never_returns_a_released_sensor() {
    let io = rig();
    io.0.borrow_mut().ignore_write = Some(0x5e);
    mismatch(initialize(&io, config()).err().unwrap(), 0x5e);
}

#[test]
fn faults_after_entering_run_prevent_initialization_success() {
    for (register, value) in [
        (1, 0x80),
        (2, 1),
        (3, 0x08),
        (3, 0x10),
        (3, 0x40),
        (3, 0x80),
    ] {
        let io = rig();
        io.0.borrow_mut().after_run = vec![(register, value)];
        assert!(initialize(&io, config()).is_err());
        assert!(
            !io.0
                .borrow()
                .events
                .iter()
                .any(|event| matches!(event, Event::Write(0x01, _)))
        );
    }
}

#[test]
fn returns_every_electrode_bit_including_unit_specific_missing_pads() {
    let (io, mut device) = device();
    for mask in [0, 0x0010, 0x0100, 0x0811, 0x0fff] {
        io.0.borrow_mut().registers[0..2].copy_from_slice(&u16::to_le_bytes(mask));
        assert_eq!(device.read().unwrap(), mask);
    }
}

#[test]
fn excluded_electrode_oor_alone_does_not_mask_or_rewrite_raw_touch_bits() {
    let mut config = config();
    config.pads.retain(|pad| ![4, 8].contains(pad));
    let io = rig();
    // Injected isolated OOR bits: this does not claim a faulty physical unit
    // can avoid asserting ACFF or ARFF while all electrodes are still scanned.
    io.0.borrow_mut().after_run = vec![(2, 0x10), (3, 0x01)];
    let mut device = initialize(&io, config).unwrap();
    io.0.borrow_mut().events.clear();
    for mask in [0, 0x0010, 0x0100, 0x0110, 0x0a63, 0x0fff] {
        io.0.borrow_mut().registers[0..2].copy_from_slice(&u16::to_le_bytes(mask));
        assert_eq!(device.read().unwrap(), mask);
    }
    assert_eq!(io.0.borrow().events, served(1, 6));
}

#[test]
fn selected_electrode_oor_is_terminal_even_beside_excluded_oor_bits() {
    for electrode in [0, 5, 11] {
        let mut config = config();
        config.pads = vec![0, 5, 11];
        let io = rig();
        let mut device = initialize(&io, config).unwrap();
        let range = (1_u16 << electrode) | (1 << 4) | (1 << 8);
        io.0.borrow_mut().registers[2..4].copy_from_slice(&range.to_le_bytes());
        assert!(matches!(
            device.read(),
            Err(Error::OutOfRange { electrodes, proximity: false })
                if electrodes == 1 << electrode
        ));
        let operations = io.0.borrow().operations;
        io.0.borrow_mut().registers[2..4].fill(0);
        assert!(matches!(device.read(), Err(Error::Unavailable)));
        assert_eq!(io.0.borrow().operations, operations);
    }
}

#[test]
fn sparse_selection_never_excuses_global_proximity_or_reserved_faults() {
    for (register, bit) in [
        (1, 7),
        (3, 7),
        (3, 6),
        (3, 4),
        (1, 4),
        (1, 5),
        (1, 6),
        (3, 5),
    ] {
        for during_initialization in [false, true] {
            let mut config = config();
            config.pads.retain(|pad| ![4, 8].contains(pad));
            let io = rig();
            let mut status = [0xff, 0x0f, 0x10, 0x01];
            status[register] |= 1 << bit;
            let error = if during_initialization {
                io.0.borrow_mut().after_run = status
                    .iter()
                    .enumerate()
                    .map(|(register, &value)| (register as u8, value))
                    .collect();
                initialize(&io, config).err().unwrap()
            } else {
                let mut device = initialize(&io, config).unwrap();
                io.0.borrow_mut().registers[0..4].copy_from_slice(&status);
                let error = device.read().unwrap_err();
                let operations = io.0.borrow().operations;
                io.0.borrow_mut().registers[0..4].fill(0);
                assert!(matches!(device.read(), Err(Error::Unavailable)));
                assert_eq!(io.0.borrow().operations, operations);
                error
            };
            match (register, bit) {
                (1, 7) => assert!(matches!(error, Error::OverCurrent)),
                (3, 7) => assert!(matches!(error, Error::AutoConfigurationFailed)),
                (3, 6) => assert!(matches!(error, Error::AutoReconfigurationFailed)),
                (3, 4) => assert!(matches!(
                    error,
                    Error::OutOfRange {
                        electrodes: 0,
                        proximity: true
                    }
                )),
                _ => assert!(matches!(error, Error::UnexpectedStatus)),
            }
            assert!(
                !io.0
                    .borrow()
                    .events
                    .iter()
                    .any(|event| matches!(event, Event::Write(0x01, _)))
            );
        }
    }
}

#[test]
fn sparse_oor_policy_is_rechecked_after_the_static_configuration_read() {
    for electrode in [4, 5, 8] {
        let mut config = config();
        config.pads = vec![0, 5, 11];
        let io = rig();
        let mut device = initialize(&io, config).unwrap();
        io.0.borrow_mut().registers[0] = 0x10;
        let next = io.0.borrow().operations + 3;
        io.0.borrow_mut().mutate_at = Some((next, vec![(2 + electrode / 8, 1 << (electrode % 8))]));
        match electrode {
            5 => assert!(matches!(
                device.read(),
                Err(Error::OutOfRange {
                    electrodes: 0x20,
                    proximity: false
                })
            )),
            _ => assert_eq!(device.read().unwrap(), 0x10),
        }
    }
}

#[test]
fn excluded_channel_static_configuration_is_still_verified() {
    for register in [0x49, 0x4a, 0x51, 0x52, 0x63, 0x67, 0x6e, 0x70] {
        let mut config = config();
        config.pads.retain(|pad| ![4, 8].contains(pad));
        config.profile.autoconfig = AutoConfig::Disabled {};
        let io = rig();
        let mut device = initialize(&io, config).unwrap();
        io.0.borrow_mut().registers[usize::from(register)] ^= 1;
        mismatch(fault_within_a_turn(&mut device), register);
    }
}

#[test]
fn startup_touch_is_not_rewritten_as_release() {
    let io = rig();
    io.0.borrow_mut().after_run = vec![(0, 0x70)];
    let mut device = initialize(&io, config()).unwrap();
    assert_eq!(device.read().unwrap(), 0x70);
}

#[test]
fn individual_fault_bits_are_not_hidden_by_touch_masking() {
    for (register, bit) in [
        (1_u8, 7),
        (2, 0),
        (2, 7),
        (3, 0),
        (3, 3),
        (3, 4),
        (3, 6),
        (3, 7),
    ] {
        let (io, mut device) = device();
        io.0.borrow_mut().registers[usize::from(register)] |= 1 << bit;
        let error = device.read().unwrap_err();
        match (register, bit) {
            (1, 7) => assert!(matches!(error, Error::OverCurrent)),
            (3, 7) => assert!(matches!(error, Error::AutoConfigurationFailed)),
            (3, 6) => assert!(matches!(error, Error::AutoReconfigurationFailed)),
            (3, 4) => assert!(matches!(
                error,
                Error::OutOfRange {
                    electrodes: 0,
                    proximity: true
                }
            )),
            _ => assert!(
                matches!(error, Error::OutOfRange { electrodes, proximity:false } if electrodes != 0)
            ),
        }
        let operations = io.0.borrow().operations;
        io.0.borrow_mut().registers[0..4].fill(0);
        assert!(matches!(device.read(), Err(Error::Unavailable)));
        assert_eq!(io.0.borrow().operations, operations);
    }
}

#[test]
fn unexpected_proximity_or_reserved_status_is_not_a_release() {
    for (register, bit) in [(1, 4), (1, 5), (1, 6), (3, 5)] {
        let (io, mut device) = device();
        io.0.borrow_mut().registers[register] = 1 << bit;
        assert!(matches!(device.read(), Err(Error::UnexpectedStatus)));
    }
}

#[test]
fn brownout_defaults_cannot_be_mistaken_for_zero_touch() {
    let (io, mut device) = device();
    io.0.borrow_mut().reset();
    assert!(matches!(device.read(), Err(Error::RegisterMismatch { .. })));
    assert!(matches!(device.read(), Err(Error::Unavailable)));
}

/// Each static register is checked once in every turn of
/// [`CONFIG_PARTS`] reads: its drift is caught at the read of its part,
/// and not before. A turn is at most 250 ms at the slowest `every_ms`, so
/// a fault is caught well within a second.
#[test]
fn static_drift_is_caught_within_one_turn_of_reads() {
    assert!(CONFIG_PARTS as u64 * 50 <= 250);
    for register in (0x2b..=0x5e).chain([
        0x6b, 0x72, 0x73, 0x74, 0x76, 0x77, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f,
    ]) {
        let (io, mut device) = device();
        io.0.borrow_mut().registers[usize::from(register)] ^= 1;
        // The first served read checks part 1. ECR is read on its own
        // every read too, so it is caught at once.
        let part = usize::from(register - 0x2b) / 17;
        let before = match register {
            0x5e => 0,
            _ => (part + CONFIG_PARTS - 1) % CONFIG_PARTS,
        };
        for _ in 0..before {
            device.read().unwrap();
        }
        mismatch(device.read().unwrap_err(), register);
    }
}

#[test]
fn auto_adjusted_charge_registers_are_not_compared_to_initial_values() {
    let io = rig();
    io.0.borrow_mut().after_run = (0x5f..=0x6a)
        .map(|r| (r, 17))
        .chain((0x6c..=0x71).map(|r| (r, 0x22)))
        .collect();
    let mut device = initialize(&io, config()).unwrap();
    a_turn_reads(&mut device, 0);
    io.0.borrow_mut().registers[0x5f] = 41;
    io.0.borrow_mut().registers[0x71] = 0x33;
    a_turn_reads(&mut device, 0);
}

#[test]
fn manual_charge_drift_is_detected() {
    for register in (0x5f..=0x6a).chain(0x6c..=0x71) {
        let mut config = config();
        config.profile.autoconfig = AutoConfig::Disabled {};
        let io = rig();
        let mut device = initialize(&io, config).unwrap();
        assert_eq!(io.0.borrow().registers[0x7b..0x80], [0; 5]);
        io.0.borrow_mut().registers[usize::from(register)] ^= 1;
        mismatch(fault_within_a_turn(&mut device), register);
    }
}

#[test]
fn skipped_charge_time_search_keeps_cdt_static_but_not_cdc() {
    let mut config = config();
    if let AutoConfig::Enabled {
        skip_cdt_search, ..
    } = &mut config.profile.autoconfig
    {
        *skip_cdt_search = true;
    }
    assert_eq!(config.settle_us().unwrap(), 100_000);
    let io = rig();
    let mut device = initialize(&io, config).unwrap();
    assert_eq!(io.0.borrow().registers[0x7c], 0x80);
    io.0.borrow_mut().registers[0x5f] = 41;
    a_turn_reads(&mut device, 0);
    io.0.borrow_mut().registers[0x6c] = 0x12;
    mismatch(fault_within_a_turn(&mut device), 0x6c);
}

#[test]
fn gpio_data_aliases_are_not_mistaken_for_static_configuration() {
    let (io, mut device) = device();
    for register in [0x75, 0x78, 0x79, 0x7a] {
        io.0.borrow_mut().registers[register] = 0xff;
    }
    a_turn_reads(&mut device, 0);
}

#[test]
fn fault_during_static_readback_is_checked_before_returning_mask() {
    let (io, mut device) = device();
    let next = io.0.borrow().operations + 2;
    io.0.borrow_mut().mutate_at = Some((next, vec![(1, 0x80)]));
    assert!(matches!(device.read(), Err(Error::OverCurrent)));
}

#[test]
fn reset_between_static_and_status_reads_is_caught_by_final_ecr() {
    let (io, mut device) = device();
    io.0.borrow_mut().registers[0] = 0x70;
    let next = io.0.borrow().operations + 3;
    io.0.borrow_mut().mutate_at = Some((next, vec![(0, 0), (0x5e, 0)]));
    mismatch(device.read().unwrap_err(), 0x5e);
}

#[test]
fn mask_comes_from_the_final_fault_checked_status_snapshot() {
    let (io, mut device) = device();
    let next = io.0.borrow().operations + 3;
    io.0.borrow_mut().mutate_at = Some((next, vec![(0, 0x70)]));
    assert_eq!(device.read().unwrap(), 0x70);
}

#[test]
fn any_runtime_transfer_failure_is_terminal_and_not_retried() {
    for offset in 1..=4 {
        let (io, mut device) = device();
        let fail_at = io.0.borrow().operations + offset;
        io.0.borrow_mut().fail_at = Some(fail_at);
        assert!(matches!(device.read(), Err(Error::Io(_))));
        assert!(matches!(device.read(), Err(Error::Unavailable)));
        assert_eq!(io.0.borrow().operations, fail_at);
    }
}

#[test]
fn steady_state_work_is_fixed_size_without_delay_or_writes() {
    let (io, mut device) = device();
    io.0.borrow_mut().events.clear();
    for _ in 0..2 * CONFIG_PARTS {
        device.read().unwrap();
    }
    assert_eq!(io.0.borrow().events, served(1, 2 * CONFIG_PARTS));
}

/// I2C bit-clocks for one combined register read of `bytes`: start, the
/// address and register, a repeated start, the address, the bytes, stop.
fn clocks(bytes: usize) -> u64 {
    1 + 9 + 9 + 1 + 9 + 9 * bytes as u64 + 1
}

/// At the 100 kHz the air sensor needs on the same bus (#1368), every
/// served read takes under the strip's fastest pace (`every_ms` 5), so the
/// strip keeps its pace: well inside `base.rs`'s 11 ms and
/// `MAX_SAMPLE_GAP_US`. Reading the whole configuration every sample did
/// not: ~9.7 ms at 100 kHz, before any bus or scheduling delay.
#[test]
fn a_served_read_fits_the_strips_pace_on_a_100_khz_bus() {
    let (io, mut device) = device();
    for _ in 0..CONFIG_PARTS {
        io.0.borrow_mut().events.clear();
        device.read().unwrap();
        let us: u64 =
            io.0.borrow()
                .events
                .iter()
                .map(|event| match event {
                    Event::Read(_, bytes) => clocks(*bytes) * 10,
                    _ => panic!("a served read only reads"),
                })
                .sum();
        assert!(us < 4_000, "{us} µs");
    }
    let whole: u64 = [4, 85, 4, 1].iter().map(|bytes| clocks(*bytes) * 10).sum();
    assert!(whole > 9_000, "{whole} µs");
}

#[test]
fn io_error_display_does_not_copy_arbitrary_transport_text() {
    let error = Error::Io(io::Error::other("private or unbounded underlying error"));
    assert_eq!(error.to_string(), "MPR121 I/O failed (Other)");
}

#[cfg(target_os = "linux")]
#[test]
fn linux_open_checks_configuration_before_opening_device() {
    let mut config = config();
    config.address = 0;
    let error = lamp_controls::mpr121::LinuxIo::open(&config).err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn signals_read_every_electrode_filtered_and_baseline_after_the_checked_mask() {
    let (io, mut device) = device();
    {
        let mut rig = io.0.borrow_mut();
        // E0 touched: filtered 0x2a5, baseline byte 0xac = 0x2b0, delta 11.
        rig.registers[0x00] = 0x01;
        rig.registers[0x04] = 0xa5;
        rig.registers[0x05] = 0x02;
        rig.registers[0x1e] = 0xac;
        // E11: the high byte keeps only its two data bits.
        rig.registers[0x04 + 22] = 0x10;
        rig.registers[0x04 + 23] = 0xff;
        rig.registers[0x1e + 11] = 0x40;
        rig.events.clear();
    }
    let signals = device.read_signals().unwrap();
    assert_eq!(signals.touched, 1);
    assert_eq!(signals.filtered[0], 0x2a5);
    assert_eq!(signals.baseline[0], 0x2b0);
    assert_eq!(signals.delta(0), 11);
    assert_eq!(signals.filtered[11], 0x310);
    assert_eq!(signals.baseline[11], 0x100);
    assert_eq!(signals.delta(11), 0x100 - 0x310);
    // The same checks as a served read, then one more transfer of 39 bytes.
    let mut expected = served(1, 1);
    expected.push(Event::Read(0x04, 39));
    assert_eq!(io.0.borrow().events, expected);
}

#[test]
fn signals_share_the_served_reads_faults_and_stay_unavailable_after_one() {
    let (io, mut device) = device();
    io.0.borrow_mut().registers[0x01] = 0x80;
    assert!(matches!(device.read_signals(), Err(Error::OverCurrent)));
    io.0.borrow_mut().registers[0x01] = 0;
    assert!(matches!(device.read_signals(), Err(Error::Unavailable)));
    assert!(matches!(device.charge(), Err(Error::Unavailable)));

    let (io, mut second) = self::device();
    io.0.borrow_mut().operations = 0;
    io.0.borrow_mut().fail_at = Some(5);
    assert!(matches!(second.read_signals(), Err(Error::Io(_))));
    assert!(matches!(second.read(), Err(Error::Unavailable)));
}

#[test]
fn charge_reads_what_auto_configuration_left_per_electrode() {
    let (io, mut device) = device();
    {
        let mut rig = io.0.borrow_mut();
        rig.registers[0x5f + 3] = 40;
        // 0x6e holds E4 in its low nibble and E5 in its high one.
        rig.registers[0x6e] = 0x63;
        rig.events.clear();
    }
    let charge = device.charge().unwrap();
    assert_eq!(charge.cdc, [53, 57, 55, 40, 31, 50, 49, 42, 31, 41, 46, 47]);
    assert_eq!(charge.cdt, [1, 1, 1, 1, 3, 6, 1, 1, 1, 1, 1, 1]);
    assert_eq!(io.0.borrow().events, [Event::Read(0x5f, 19)]);
}
