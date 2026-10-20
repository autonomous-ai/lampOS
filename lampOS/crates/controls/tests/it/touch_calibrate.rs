use lamp_controls::{
    mpr121::{self, Charge, Config, Signals},
    touch_calibrate::{self, Chip, Clock, Plan, Verdict, quiet_swing, thresholds},
};
use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-controls");

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

/// lamp-4ace's ten working electrodes, with its authored example profile.
fn base() -> Config {
    let pins: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture("strip-711-working-pins-example.json")).unwrap())
            .unwrap();
    serde_json::from_value(pins["base"].clone()).unwrap()
}

/// Time moves only when the calibration waits, and by 1 ms for each read.
struct FakeClock(Rc<Cell<u64>>);

impl Clock for FakeClock {
    fn now_us(&mut self) -> u64 {
        self.0.get()
    }
    fn sleep_us(&mut self, microseconds: u64) {
        self.0.set(self.0.get() + microseconds);
    }
}

/// Answers each read with the deltas `script` gives for the time of the read.
struct FakeChip<F: FnMut(u64) -> [i32; 12]> {
    clock: Rc<Cell<u64>>,
    script: F,
    reads: u64,
    fail_after: Option<u64>,
    charge: Charge,
}

impl<F: FnMut(u64) -> [i32; 12]> Chip for FakeChip<F> {
    fn signals(&mut self) -> Result<Signals, mpr121::Error> {
        self.reads += 1;
        if self.fail_after.is_some_and(|after| self.reads > after) {
            return Err(mpr121::Error::OverCurrent);
        }
        let deltas = (self.script)(self.clock.get());
        self.clock.set(self.clock.get() + 1_000);
        let mut signals = Signals {
            touched: 0,
            filtered: [0; 12],
            baseline: [512; 12],
        };
        for (electrode, delta) in deltas.into_iter().enumerate() {
            signals.filtered[electrode] = (512 - delta) as u16;
            if delta >= 6 {
                signals.touched |= 1 << electrode;
            }
        }
        Ok(signals)
    }
    fn charge(&mut self) -> Result<Charge, mpr121::Error> {
        Ok(self.charge)
    }
}

const PLAN: Plan = Plan {
    settle: 10,
    quiet: 5,
    swipe: 30,
    touch_percent: 40,
    release_percent: 20,
};
const S: u64 = 1_000_000;

fn run<F: FnMut(u64) -> [i32; 12]>(
    script: F,
    fail_after: Option<u64>,
) -> Result<(touch_calibrate::Report, Vec<String>), mpr121::Error> {
    let base = base();
    let time = Rc::new(Cell::new(7 * S));
    let mut clock = FakeClock(time.clone());
    let mut chip = FakeChip {
        clock: time,
        script,
        reads: 0,
        fail_after,
        charge: Charge {
            cdc: [53, 57, 55, 51, 31, 50, 49, 42, 31, 41, 46, 47],
            cdt: [1; 12],
        },
    };
    let mut said = Vec::new();
    let report = touch_calibrate::measure(
        &mut chip,
        &mut clock,
        &base.pads,
        &base.profile,
        base.every_ms,
        PLAN,
        |step| said.push(step.to_owned()),
    )?;
    Ok((report, said))
}

/// Settle: the baseline is still far off on every electrode. Quiet: ±2 on
/// the working pads, ±5 on E6. Swipe: each working pad in turn, one second
/// each, three passes peaking at 40, 24 and 32, except E9 and E10, which
/// peak at the given value every pass (0: never touched). The not-working
/// E4 and E8 swing wildly throughout and must be ignored.
fn script(e9: i32, e10: i32) -> impl FnMut(u64) -> [i32; 12] {
    move |t| lamp(t, e9, e10)
}

fn lamp(t: u64, e9: i32, e10: i32) -> [i32; 12] {
    let since = t - 7 * S;
    let mut deltas = [0; 12];
    let wobble = if (since / 10_000).is_multiple_of(2) {
        2
    } else {
        -2
    };
    for (electrode, delta) in deltas.iter_mut().enumerate() {
        *delta = match electrode {
            4 | 8 => 200,
            _ if since < 10 * S => 30,
            6 if since < 15 * S => wobble * 5 / 2,
            _ if since < 15 * S => wobble,
            _ => 0,
        };
    }
    if since >= 15 * S {
        let second = (since - 15 * S) / S;
        let pad = [0, 1, 2, 3, 5, 6, 7, 9, 10, 11][(second % 10) as usize];
        deltas[pad] = match pad {
            9 => e9,
            10 => e10,
            _ => [40, 24, 32][(second / 10) as usize],
        };
        // A two-reading flick on E10 is not a swipe.
        if e10 == 0 && since < 15 * S + 20_000 {
            deltas[10] = 30;
        }
    }
    deltas
}

#[test]
fn the_rule_sets_each_threshold_between_noise_and_the_weakest_swipe() {
    assert_eq!(thresholds(2, 6, &PLAN), Err(Verdict::TooCloseToNoise));
    assert_eq!(thresholds(2, 7, &PLAN), Ok((4, 3)));
    assert_eq!(thresholds(3, 43, &PLAN), Ok((19, 11)));
    assert_eq!(thresholds(0, 1023, &PLAN), Ok((255, 204)));
    assert_eq!(thresholds(254, 1023, &PLAN), Err(Verdict::TooCloseToNoise));
    let half = Plan {
        touch_percent: 50,
        release_percent: 25,
        ..PLAN
    };
    assert_eq!(thresholds(4, 44, &half), Ok((24, 14)));
    // Reviewer, #797: at 90% the weakest swipe 7 must still be above touch,
    // since the chip needs its delta strictly above the threshold.
    let ninety = Plan {
        touch_percent: 90,
        ..PLAN
    };
    assert_eq!(thresholds(2, 7, &ninety), Ok((6, 3)));
    // Every accepted pair of percentages keeps noise < release < touch < weakest.
    for touch_percent in 10..=90 {
        for release_percent in 0..touch_percent {
            let plan = Plan {
                touch_percent,
                release_percent,
                ..PLAN
            };
            plan.check().unwrap();
            for noise in [0, 1, 2, 3, 5, 8, 13, 40, 100, 200, 250, 252, 253, 254] {
                let near = noise..noise + 30;
                for weakest in near.chain([noise + 61, noise + 255, 640, 1023]) {
                    match thresholds(noise, weakest, &plan) {
                        Ok((touch, release)) => {
                            let (touch, release) = (u16::from(touch), u16::from(release));
                            assert!(
                                noise < release && release < touch && touch < weakest,
                                "{touch_percent}/{release_percent}: {noise} {weakest} -> {touch}/{release}"
                            );
                        }
                        Err(verdict) => {
                            assert_eq!(verdict, Verdict::TooCloseToNoise);
                            assert!(weakest < noise + 5 || noise > 253, "{noise} {weakest}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn the_noise_counts_the_baseline_bits_the_chip_does_not_show() {
    // The chip keeps a 10-bit baseline and shows its upper 8 bits.
    assert_eq!(quiet_swing(0), 3);
    assert_eq!(quiet_swing(2), 5);
    assert_eq!(quiet_swing(-1), 2);
    assert_eq!(quiet_swing(-2), 2);
    assert_eq!(quiet_swing(-5), 5);
    // Reviewer, #797: internal baseline 515, filtered 512, read baseline 512.
    // The read delta is 0, but the chip compares 3, so a weakest swipe of 5
    // leaves no room for a release above what the quiet room already does.
    let noise = quiet_swing(512 - 512);
    assert_eq!(noise, 3);
    assert_eq!(thresholds(noise, 5, &PLAN), Err(Verdict::TooCloseToNoise));
    // Through the whole calibration: a perfectly flat read, then each pad
    // swiped in turn at 7.
    let (report, _) = run(
        |t| {
            let mut deltas = [0; 12];
            if t >= 22 * S {
                let pad = [0, 1, 2, 3, 5, 6, 7, 9, 10, 11][((t - 22 * S) / S % 10) as usize];
                deltas[pad] = 7;
            }
            deltas
        },
        None,
    )
    .unwrap();
    for pad in &report.pads {
        assert_eq!(pad.noise, 3, "E{}", pad.electrode);
        assert_eq!(pad.weakest, 7, "E{}", pad.electrode);
        assert_eq!(pad.verdict, Verdict::TooCloseToNoise, "E{}", pad.electrode);
    }
    assert_eq!(report.apply(&base().profile), None);
}

#[test]
fn the_three_steps_measure_noise_and_swipes_of_working_pads_only() {
    // E9 is swiped too lightly and E10 never, but for one flick.
    let (report, said) = run(script(9, 0), None).unwrap();
    assert_eq!(said.len(), 3);
    assert!(said[0].starts_with("hands away"), "{said:?}");
    assert!(said[1].contains("noise for 5 s"), "{said:?}");
    assert!(said[2].contains("swipe each working pad"), "{said:?}");
    // Every 10 ms: five and thirty seconds of readings, nothing from the settle.
    assert_eq!(report.quiet_readings, 500);
    assert_eq!(report.swipe_readings, 3_000);
    assert_eq!(report.not_working, [4, 8]);
    assert!(report.rule.text.contains("touch at 40% and release at 20%"));
    assert_eq!(
        (report.rule.touch_percent, report.rule.release_percent),
        (40, 20)
    );
    let electrodes: Vec<u8> = report.pads.iter().map(|pad| pad.electrode).collect();
    assert_eq!(electrodes, [0, 1, 2, 3, 5, 6, 7, 9, 10, 11]);
    for pad in &report.pads {
        let e = usize::from(pad.electrode);
        // Read swings of ±2 (E6: ±5), plus the 3 hidden baseline counts.
        assert_eq!(pad.noise, if e == 6 { 8 } else { 5 }, "E{e}");
        assert_eq!(pad.baseline_quiet, 512);
        assert_eq!(pad.baseline_end, 512);
        assert_eq!(pad.cdc, [53, 57, 55, 51, 31, 50, 49, 42, 31, 41, 46, 47][e]);
        let expected = match e {
            9 => (3, 9, 9, Verdict::TooCloseToNoise),
            10 => (0, 0, 0, Verdict::NotTouched),
            _ => (3, 24, 40, Verdict::Ok),
        };
        assert_eq!(
            (pad.swipes, pad.weakest, pad.strongest, pad.verdict),
            expected,
            "E{e}"
        );
    }
    let set = |e: usize| {
        let pad = report
            .pads
            .iter()
            .find(|pad| usize::from(pad.electrode) == e)
            .unwrap();
        (pad.touch_threshold, pad.release_threshold)
    };
    // Noise 5, weakest 24: 5 + 40% of 19, 5 + 20% of 19, rounded down.
    assert_eq!(set(0), (12, 8));
    // Noise 8, weakest 24: 8 + 40% of 16, 8 + 20% of 16.
    assert_eq!(set(6), (14, 11));
    // A pad without thresholds keeps its old ones, and says why.
    let old = base().profile;
    assert_eq!(set(9), (old.touch_thresholds[9], old.release_thresholds[9]));
    assert!(report.pads[0].touched_old > 0);
    assert!(!report.all_ok());
    assert_eq!(report.apply(&old), None);
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["not_working"], serde_json::json!([4, 8]));
    assert_eq!(json["pads"][8]["verdict"], "not_touched");
    assert_eq!(json["pads"][7]["verdict"], "too_close_to_noise");
}

#[test]
fn only_when_every_working_pad_is_clear_are_its_thresholds_saved() {
    let (report, _) = run(script(40, 36), None).unwrap();
    assert!(report.all_ok(), "{:?}", report.pads);
    let old = base().profile;
    let new = report.apply(&old).unwrap();
    for electrode in 0..12 {
        if electrode == 4 || electrode == 8 {
            assert_eq!(
                new.touch_thresholds[electrode],
                old.touch_thresholds[electrode]
            );
            assert_eq!(
                new.release_thresholds[electrode],
                old.release_thresholds[electrode]
            );
        } else {
            let pad = report
                .pads
                .iter()
                .find(|pad| usize::from(pad.electrode) == electrode)
                .unwrap();
            assert_eq!(new.touch_thresholds[electrode], pad.touch_threshold);
            assert_eq!(new.release_thresholds[electrode], pad.release_threshold);
        }
    }
    // Noise 5: weakest 40 gives 19/12, weakest 36 gives 17/11.
    assert_eq!(
        (new.touch_thresholds[9], new.release_thresholds[9]),
        (19, 12)
    );
    assert_eq!(
        (new.touch_thresholds[10], new.release_thresholds[10]),
        (17, 11)
    );
    // Nothing but the thresholds changes.
    assert_eq!(new.cdc, old.cdc);
    assert_eq!(new.baseline, old.baseline);
    assert_eq!(new.autoconfig, old.autoconfig);
    new.check().unwrap();
}

#[test]
fn a_chip_fault_during_the_swipe_ends_the_calibration_with_nothing_kept() {
    let error = run(script(40, 36), Some(2_000)).unwrap_err();
    assert!(matches!(error, mpr121::Error::OverCurrent), "{error}");
}

#[test]
fn each_step_lasts_one_to_six_hundred_seconds_and_release_is_below_touch() {
    assert!(Plan::DEFAULT.check().is_ok());
    assert_eq!(Plan::DEFAULT, PLAN);
    for plan in [
        Plan { settle: 0, ..PLAN },
        Plan { quiet: 0, ..PLAN },
        Plan { swipe: 0, ..PLAN },
        Plan {
            settle: 601,
            ..PLAN
        },
        Plan {
            touch_percent: 9,
            ..PLAN
        },
        Plan {
            touch_percent: 91,
            ..PLAN
        },
        Plan {
            release_percent: 40,
            ..PLAN
        },
    ] {
        assert!(plan.check().is_err(), "{plan:?}");
    }
}

fn program(args: &[&str]) -> std::process::Output {
    Command::new(PROGRAM).args(args).output().unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("controls-calibrate-{}-{name}", std::process::id()));
    let _ = fs::remove_file(&path);
    path
}

#[test]
fn the_command_needs_pins_and_a_new_output_and_writes_nothing_on_failure() {
    let pins = fixture("strip-711-working-pins-example.json");
    let pins = pins.to_str().unwrap();
    let output = scratch("new-pins.json");
    let out = output.to_str().unwrap();
    for args in [
        vec!["calibrate-touch", "--pins", pins],
        vec!["calibrate-touch", "--output", out],
        vec![
            "calibrate-touch",
            "--pins",
            pins,
            "--output",
            out,
            "--swipe-seconds",
            "0",
        ],
        vec![
            "calibrate-touch",
            "--pins",
            pins,
            "--output",
            out,
            "--quiet-seconds",
            "601",
        ],
        vec![
            "calibrate-touch",
            "--pins",
            pins,
            "--output",
            out,
            "--seconds",
            "5",
        ],
        vec![
            "calibrate-touch",
            "--pins",
            pins,
            "--output",
            out,
            "--touch-percent",
            "95",
        ],
        vec![
            "calibrate-touch",
            "--pins",
            pins,
            "--output",
            out,
            "--release-percent",
            "40",
        ],
        vec![
            "calibrate-touch",
            "--pins",
            pins,
            "--output",
            out,
            "--touch-percent",
            "x",
        ],
    ] {
        let result = program(&args);
        assert!(!result.status.success(), "{args:?}");
        assert!(result.stdout.is_empty(), "{args:?}");
        assert!(!output.exists(), "{args:?}");
    }

    // An existing output is refused before any hardware is opened.
    fs::write(&output, b"keep").unwrap();
    let result = program(&["calibrate-touch", "--pins", pins, "--output", out]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("already exists"));
    assert_eq!(fs::read(&output).unwrap(), b"keep");
    fs::remove_file(&output).unwrap();

    // A pins file without a base strip has nothing to calibrate.
    let bare = scratch("bare-pins.json");
    fs::write(
        &bare,
        br#"{"chip":"/dev/gpiochip0","buttons":[],"head_pads":[],"every_ms":5}"#,
    )
    .unwrap();
    let result = program(&[
        "calibrate-touch",
        "--pins",
        bare.to_str().unwrap(),
        "--output",
        out,
    ]);
    fs::remove_file(&bare).unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("no base strip"));
    assert!(!output.exists());
}

#[cfg(not(target_os = "linux"))]
#[test]
fn off_linux_the_command_says_it_has_no_chip_and_writes_nothing() {
    let output = scratch("off-linux.json");
    let result = program(&[
        "calibrate-touch",
        "--pins",
        fixture("strip-711-working-pins-example.json")
            .to_str()
            .unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("which this system does not have"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!output.exists());
}

/// First's run on lamp-4ace (#797, 18:10): a finger on one pad also raises its
/// neighbours, so every pad seemed to be swiped 40 times and its "weakest
/// swipe" was a neighbour's touch. Here each pad is swiped in turn at 30 and
/// lifts its two neighbours to 7 while it is.
fn coupled(t: u64) -> [i32; 12] {
    let mut deltas = [0; 12];
    if t >= 22 * S {
        let order = [0, 1, 2, 3, 5, 6, 7, 9, 10, 11];
        let at = ((t - 22 * S) / S % 10) as usize;
        let pad = order[at];
        deltas[pad] = 30;
        for neighbour in [at.checked_sub(1), Some(at + 1)].into_iter().flatten() {
            if let Some(&other) = order.get(neighbour) {
                deltas[other] = 7;
            }
        }
    }
    deltas
}

#[test]
fn a_neighbours_touch_is_coupling_not_a_swipe() {
    let (report, _) = run(coupled, None).unwrap();
    for pad in &report.pads {
        let e = pad.electrode;
        assert_eq!(pad.swipes, 3, "E{e}: only its own swipes, three passes");
        assert_eq!(
            (pad.weakest, pad.strongest, pad.level),
            (30, 30, 30),
            "E{e}"
        );
        assert_eq!(
            pad.coupling, 7,
            "E{e}: what it reads while a neighbour is touched"
        );
        assert_eq!(pad.verdict, Verdict::Ok, "E{e}");
        // Noise 3 (flat read), level 30: 3 + 40% of 27, 3 + 20% of 27.
        assert_eq!(
            (pad.touch_threshold, pad.release_threshold),
            (13, 8),
            "E{e}"
        );
    }
    assert!(report.all_ok());
    // With the old thresholds the chip lit the pad and its two neighbours
    // (three at once, as the base's touch needs); the new release threshold
    // (8) is above the coupling (7), so the same readings never light two.
    assert_eq!(
        report.footprint_new[2..],
        [0; 4],
        "{:?}",
        report.footprint_new
    );
    assert!(
        report.footprint_new[1] > 2_900,
        "{:?}",
        report.footprint_new
    );
}

/// The reviewer's case on #797: each pad swiped alone at 30 (touch 13,
/// release 8), then a finger held on E0, E1 and E2 in turn, one second each,
/// with the pads it left at 9: under the touch threshold, above the release.
fn stacked(t: u64) -> [i32; 12] {
    let mut deltas = [0; 12];
    if t >= 22 * S {
        let order = [0, 1, 2, 3, 5, 6, 7, 9, 10, 11];
        match ((t - 22 * S) / S) as usize {
            second @ 0..10 => deltas[order[second]] = 30,
            10 => deltas[0] = 30,
            11 => deltas[..2].copy_from_slice(&[9, 30]),
            12 => deltas[..3].copy_from_slice(&[9, 9, 30]),
            _ => {}
        }
    }
    deltas
}

#[test]
fn the_new_footprint_keeps_a_pad_lit_until_it_falls_below_its_release() {
    let (report, _) = run(stacked, None).unwrap();
    assert!(report.all_ok());
    for pad in &report.pads {
        assert_eq!((pad.touch_threshold, pad.release_threshold), (13, 8));
    }
    // A pad left at 9 is still lit: two pads through E1's second, three
    // through E2's, not one each time as single crossings would say.
    let [_, _, two, three, ..] = report.footprint_new;
    assert!(two >= 95 && three >= 95, "{:?}", report.footprint_new);
}

#[test]
fn the_own_level_ignores_a_swipe_that_only_grazed_the_pad() {
    use lamp_controls::touch_calibrate::own_level;
    assert_eq!(own_level(&[]), None);
    assert_eq!(own_level(&[30]), Some(30));
    // 9 is under half of 33: a graze, not the level.
    assert_eq!(own_level(&[33, 9, 27, 30]), Some(27));
    // So is 16, which rounding half of 33 down would let in; 17 is not.
    assert_eq!(own_level(&[33, 16, 27, 30]), Some(27));
    assert_eq!(own_level(&[33, 17, 27]), Some(17));
    assert_eq!(own_level(&[1, 0]), Some(1));
    assert_eq!(own_level(&[17, 9, 10]), Some(9));
}

#[test]
fn a_kept_session_replays_to_the_same_report_without_a_chip() {
    use lamp_controls::touch_calibrate::{analyse, collect};
    let base = base();
    let time = Rc::new(Cell::new(7 * S));
    let mut clock = FakeClock(time.clone());
    let mut chip = FakeChip {
        clock: time,
        script: coupled,
        reads: 0,
        fail_after: None,
        charge: Charge {
            cdc: [53, 57, 55, 51, 31, 50, 49, 42, 31, 41, 46, 47],
            cdt: [1; 12],
        },
    };
    let session = collect(&mut chip, &mut clock, base.every_ms, PLAN, |_| {}).unwrap();
    let live = analyse(&session, &base.pads, &base.profile, PLAN);
    let kept = serde_json::to_vec(&session).unwrap();
    let back = serde_json::from_slice(&kept).unwrap();
    assert_eq!(analyse(&back, &base.pads, &base.profile, PLAN), live);

    // The command, from that file, with no chip, writes the new pins.
    let file = scratch("session.json");
    fs::write(&file, &kept).unwrap();
    let output = scratch("replayed-pins.json");
    let result = program(&[
        "calibrate-touch",
        "--pins",
        fixture("strip-711-working-pins-example.json")
            .to_str()
            .unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--replay",
        file.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["pads"][0]["coupling"], 7);
    let pins: serde_json::Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(pins["base"]["profile"]["touch_thresholds"][0], 13);
    assert!(
        !String::from_utf8_lossy(&result.stderr).contains("not in it"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    // From a newer build's pins file, the new file is this build's: what it
    // doesn't read is not carried over, and its line says so (the reviewer
    // on #2076).
    let mut newer: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture("strip-711-working-pins-example.json")).unwrap())
            .unwrap();
    newer["head_hold_ms"] = 900.into();
    let newer_pins = scratch("newer-pins.json");
    fs::write(&newer_pins, serde_json::to_vec(&newer).unwrap()).unwrap();
    let from_newer = scratch("from-newer-pins.json");
    let result = program(&[
        "calibrate-touch",
        "--pins",
        newer_pins.to_str().unwrap(),
        "--output",
        from_newer.to_str().unwrap(),
        "--replay",
        file.to_str().unwrap(),
    ]);
    let said = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "{said}");
    assert!(
        said.contains("fields this build does not read, ignored: head_hold_ms"),
        "{said}"
    );
    assert!(
        said.contains("; the fields this build does not read, named above, are not in it"),
        "{said}"
    );
    let written: serde_json::Value =
        serde_json::from_slice(&fs::read(&from_newer).unwrap()).unwrap();
    assert!(written.get("head_hold_ms").is_none());
    fs::remove_file(&newer_pins).unwrap();
    fs::remove_file(&from_newer).unwrap();
    // A kept file is never written over, and record and replay do not mix.
    let again = program(&[
        "calibrate-touch",
        "--pins",
        fixture("strip-711-working-pins-example.json")
            .to_str()
            .unwrap(),
        "--output",
        scratch("unused.json").to_str().unwrap(),
        "--replay",
        file.to_str().unwrap(),
        "--record",
        file.to_str().unwrap(),
    ]);
    assert!(!again.status.success());
    fs::remove_file(&file).unwrap();
    fs::remove_file(&output).unwrap();
}
