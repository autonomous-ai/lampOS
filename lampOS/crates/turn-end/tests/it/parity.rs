//! Rust features and tract inference versus Pipecat's pinned reference, and
//! the real `lamp-turn-end` process behind the core's client.
//! Model tests are ignored by default: install the pinned model under
//! `.cache/endpoint/` (or set `LAMP_SMART_TURN_MODEL`), then run
//! `cargo test -p lamp-turn-end -- --include-ignored`.
use lamp_turn_end::{FeatureExtractor, SMART_TURN_V3_2, SmartTurn, WINDOW_SAMPLES, reference};
use lamp_turn_end_wire::{
    Availability, Classifier, Limits, ProcessClassifier, Submit, Teardown, Token, WorkerCommand,
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const REFERENCE: &str = include_str!("../fixtures/pipecat-reference.json");

fn xorshift(seed: u32) -> impl FnMut() -> u32 {
    let mut state = seed;
    move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    }
}

/// The integer signals recorded in `fixtures/README.md`.
fn signal(name: &str) -> Vec<i16> {
    let noise = |seed, count, modulus: u32, offset: i32| {
        let mut next = xorshift(seed);
        (0..count)
            .map(|_| (next() % modulus) as i32 - offset)
            .map(|value| value as i16)
            .collect::<Vec<_>>()
    };
    match name {
        "noise_2s" => noise(1, 32_000, 12_001, 6_000),
        "short_0p3s" => noise(7, 4_800, 6_001, 3_000),
        "voiced_9s" => {
            let mut next = xorshift(2);
            (0..144_000)
                .map(|i| {
                    let pulse = if i % 100 < 3 { 9_000 } else { 0 };
                    let value = pulse * ((i / 1_600) % 10 + 1) / 10 + (next() % 801) as i32 - 400;
                    value as i16
                })
                .collect()
        }
        "silence" => Vec::new(),
        other => panic!("unknown reference signal {other}"),
    }
}

fn reference() -> Value {
    serde_json::from_str(REFERENCE).unwrap()
}

fn model_path() -> PathBuf {
    std::env::var_os("LAMP_SMART_TURN_MODEL").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.cache/endpoint")
                .join(SMART_TURN_V3_2.file_name)
        },
        PathBuf::from,
    )
}

#[test]
fn features_match_pipecat_reference() {
    let reference = reference();
    assert_eq!(reference["model_sha256"], SMART_TURN_V3_2.sha256);
    let mut extractor = FeatureExtractor::default();
    for (name, case) in reference["cases"].as_object().unwrap() {
        let pcm = signal(name);
        assert_eq!(pcm.len() as u64, case["samples"].as_u64().unwrap());
        let features = extractor.compute(&pcm);
        let stride = case["stride"].as_u64().unwrap() as usize;
        let mut worst = 0f64;
        for (index, expected) in case["sampled"].as_array().unwrap().iter().enumerate() {
            let actual = f64::from(features[index * stride]);
            worst = worst.max((actual - expected.as_f64().unwrap()).abs());
        }
        let sum = features.iter().map(|&v| f64::from(v)).sum::<f64>();
        let abs_sum = features.iter().map(|&v| f64::from(v).abs()).sum::<f64>();
        eprintln!("{name}: max sampled difference {worst:e}");
        assert!(worst < 1e-4, "{name}: sampled feature difference {worst}");
        // 64,000 values: a mean absolute difference below 1e-6.
        assert!(
            (sum - case["sum"].as_f64().unwrap()).abs() < 0.064,
            "{name} sum"
        );
        assert!(
            (abs_sum - case["abs_sum"].as_f64().unwrap()).abs() < 0.064,
            "{name} abs sum"
        );
    }
}

#[test]
fn self_check_reference_is_the_fixture_signal() {
    assert_eq!(reference::voiced(), signal("voiced_9s"));
    let expected = reference()["cases"]["voiced_9s"]["probability"]
        .as_f64()
        .unwrap();
    assert!((f64::from(reference::VOICED_PROBABILITY) - expected).abs() < 1e-7);
}

#[test]
#[ignore = "requires the pinned model; see docs/endpoint-detection.md"]
fn tract_probability_matches_onnx_runtime_reference() {
    let model = SmartTurn::load(&model_path(), &SMART_TURN_V3_2).unwrap();
    let mut extractor = FeatureExtractor::default();
    for (name, case) in reference()["cases"].as_object().unwrap() {
        let probability = model.probability(extractor.compute(&signal(name))).unwrap();
        let expected = case["probability"].as_f64().unwrap();
        eprintln!("{name}: tract {probability:.7} onnxruntime {expected:.7}");
        assert!((f64::from(probability) - expected).abs() < 1e-4, "{name}");
    }
}

#[test]
fn model_load_refuses_wrong_bytes_before_parsing() {
    let dir = std::env::temp_dir().join(format!("lamp-turn-end-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("model.onnx");
    std::fs::write(&path, b"not a model").unwrap();
    let error = SmartTurn::load(&path, &SMART_TURN_V3_2).err().unwrap();
    assert!(error.to_string().contains("unexpected size"), "{error}");
    let mut same_size = SMART_TURN_V3_2;
    same_size.bytes = 11;
    let error = SmartTurn::load(&path, &same_size).err().unwrap();
    assert!(error.to_string().contains("pinned SHA256"), "{error}");
    std::fs::remove_dir_all(dir).unwrap();
}

fn worker(model: &Path) -> ProcessClassifier {
    ProcessClassifier::spawn(
        WorkerCommand::smart_turn(env!("CARGO_BIN_EXE_lamp-turn-end").into(), model.into()),
        Limits::default(),
    )
}

fn settle(classifier: &ProcessClassifier, limit: Duration) -> Availability {
    let started = Instant::now();
    loop {
        let state = classifier.availability();
        if state != Availability::Loading || started.elapsed() > limit {
            return state;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

const TOKEN: Token = Token {
    utterance: 4,
    generation: 2,
    check: 0,
};

#[test]
fn worker_process_refuses_missing_or_wrong_models_without_blocking() {
    let dir = std::env::temp_dir().join(format!("lamp-turn-end-worker-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wrong = dir.join("model.onnx");
    std::fs::write(&wrong, b"not a model").unwrap();
    for (model, expected) in [
        (dir.join("missing.onnx"), "No such file"),
        (wrong, "unexpected size"),
    ] {
        let started = Instant::now();
        let mut classifier = worker(&model);
        assert_eq!(classifier.submit(TOKEN, &[1; 160]), Submit::Unavailable);
        assert!(started.elapsed() < Duration::from_millis(100));
        let Availability::Failed(reason) = settle(&classifier, Duration::from_secs(10)) else {
            panic!("{} did not fail", model.display());
        };
        assert!(reason.contains(expected), "{reason}");
        assert_eq!(
            classifier.wait_finished(Duration::from_secs(2)),
            Teardown::Reaped
        );
        let report = classifier.report();
        assert_eq!((report.spawned, report.ready, report.kills), (1, 0, 0));
        assert_eq!(classifier.submit(TOKEN, &[1; 160]), Submit::Unavailable);
        assert!(classifier.poll().is_none());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "requires the pinned model; see docs/endpoint-detection.md"]
fn worker_process_matches_reference_and_reports_stage_costs() {
    let mut classifier = worker(&model_path());
    assert_eq!(
        settle(&classifier, Duration::from_secs(30)),
        Availability::Ready
    );
    let report = classifier.report();
    assert_eq!(report.model_sha256.as_deref(), Some(SMART_TURN_V3_2.sha256));
    eprintln!(
        "worker load {} ms, cold inference {} ms, spawn to ready {} ms",
        report.load_us.unwrap() / 1_000,
        report.cold_us.unwrap() / 1_000,
        report.ready_after_us.unwrap() / 1_000
    );
    let reference = reference();
    for (name, check) in [("voiced_9s", 0u8), ("noise_2s", 1), ("short_0p3s", 2)] {
        let token = Token { check, ..TOKEN };
        assert_eq!(classifier.submit(token, &signal(name)), Submit::Accepted);
        assert_eq!(
            classifier.submit(Token { check: 9, ..token }, &[0; 160]),
            Submit::Busy
        );
        let started = Instant::now();
        let outcome = loop {
            if let Some(outcome) = classifier.poll() {
                break outcome;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "no result");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!((outcome.token, outcome.worker), (token, 1));
        let verdict = outcome.result.unwrap();
        let expected = reference["cases"][name]["probability"].as_f64().unwrap();
        assert!(
            (f64::from(verdict.probability) - expected).abs() < 1e-4,
            "{name}"
        );
        eprintln!("{name}: {verdict:?}");
    }
    // A long request is truncated to the model window before it is copied.
    let long = vec![0i16; 3 * WINDOW_SAMPLES];
    assert_eq!(classifier.submit(TOKEN, &long), Submit::Accepted);
    let pid = classifier.report().pid.unwrap();
    classifier.close();
    assert_eq!(
        classifier.wait_finished(Duration::from_secs(5)),
        Teardown::Reaped
    );
    assert!(classifier.report().last_exit.is_some(), "{pid} not reaped");
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lamp-turn-end-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A truncated download and a same-size corrupt copy of the pinned model.
fn damaged_models(dir: &Path) -> [(PathBuf, &'static str); 2] {
    let bytes = std::fs::read(model_path()).unwrap();
    let truncated = dir.join("truncated.onnx");
    std::fs::write(&truncated, &bytes[..bytes.len() / 2]).unwrap();
    let mut corrupt = bytes;
    let middle = corrupt.len() / 2;
    corrupt[middle] ^= 0x40;
    let corrupt_path = dir.join("corrupt.onnx");
    std::fs::write(&corrupt_path, corrupt).unwrap();
    [
        (truncated, "unexpected size"),
        (corrupt_path, "pinned SHA256"),
    ]
}

#[test]
#[ignore = "requires the pinned model; see docs/endpoint-detection.md"]
fn check_model_verifies_the_installed_model_and_refuses_damaged_copies() {
    let check = |path: &Path| {
        Command::new(env!("CARGO_BIN_EXE_lamp-turn-end"))
            .arg("--check-model")
            .arg(path)
            .output()
            .unwrap()
    };
    let output = check(&model_path());
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    eprintln!("{report}");
    assert_eq!(report["sha256"], SMART_TURN_V3_2.sha256);
    let probability = report["probability"].as_f64().unwrap();
    assert!((probability - f64::from(reference::VOICED_PROBABILITY)).abs() < 1e-4);
    let dir = scratch("check");
    for (path, expected) in damaged_models(&dir) {
        let output = check(&path);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{stderr}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "requires the pinned model; see docs/endpoint-detection.md"]
fn worker_process_refuses_truncated_and_corrupt_models_once() {
    let dir = scratch("damaged");
    for (path, expected) in damaged_models(&dir) {
        let mut classifier = worker(&path);
        assert_eq!(classifier.submit(TOKEN, &[1; 160]), Submit::Unavailable);
        let Availability::Failed(reason) = settle(&classifier, Duration::from_secs(10)) else {
            panic!("{} did not fail", path.display());
        };
        assert!(reason.contains(expected), "{reason}");
        assert_eq!(
            classifier.wait_finished(Duration::from_secs(2)),
            Teardown::Reaped
        );
        let report = classifier.report();
        assert_eq!((report.spawned, report.ready, report.kills), (1, 0, 0));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// On Linux the serving worker runs without core files, under its 256 MiB
/// address-space cap and at niceness 10 (`/proc/PID/limits` and `stat`).
fn assert_bounded(pid: u32) {
    if !cfg!(target_os = "linux") {
        return;
    }
    let limits = std::fs::read_to_string(format!("/proc/{pid}/limits")).unwrap();
    let limit = |name: &str| {
        let line = limits.lines().find(|line| line.starts_with(name)).unwrap();
        line[name.len()..]
            .split_whitespace()
            .take(2)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(limit("Max core file size"), ["0", "0"]);
    assert_eq!(limit("Max address space"), ["268435456", "268435456"]);
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    // Fields after the parenthesized name: state is field 3, nice field 19.
    let after_name = &stat[stat.rfind(')').unwrap() + 2..];
    assert_eq!(after_name.split(' ').nth(16), Some("10"), "{stat}");
}

/// Resident and peak resident KiB of a child, from Linux `/proc`.
fn child_memory_kib(pid: u32) -> Option<(u64, u64)> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let field = |key: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
    };
    Some((field("VmRSS:")?, field("VmHWM:")?))
}

fn classify_reference(classifier: &mut ProcessClassifier, check: u8) -> f32 {
    let token = Token { check, ..TOKEN };
    assert_eq!(
        classifier.submit(token, &reference::voiced()),
        Submit::Accepted
    );
    let started = Instant::now();
    loop {
        if let Some(outcome) = classifier.poll() {
            assert_eq!(outcome.token, token);
            return outcome.result.unwrap().probability;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "no result");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// `LAMP_ENDPOINT_IDLE_SECS` (default 5) sets the quiet period; the pilot
/// evidence uses 600.
#[test]
#[ignore = "requires the pinned model; see docs/endpoint-detection.md"]
fn worker_process_stays_ready_and_bounded_across_idle_and_repeated_checks() {
    let idle = std::env::var("LAMP_ENDPOINT_IDLE_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5u64);
    let mut classifier = worker(&model_path());
    assert_eq!(
        settle(&classifier, Duration::from_secs(30)),
        Availability::Ready
    );
    let pid = classifier.report().pid.unwrap();
    assert_bounded(pid);
    let close = |probability: f32| {
        (probability - reference::VOICED_PROBABILITY).abs() < reference::TOLERANCE
    };
    assert!(close(classify_reference(&mut classifier, 0)));
    let warm = child_memory_kib(pid);
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(idle) {
        assert_eq!(classifier.availability(), Availability::Ready);
        std::thread::sleep(Duration::from_millis(250));
    }
    let after_idle = child_memory_kib(pid);
    for check in 1..=50 {
        assert!(close(classify_reference(&mut classifier, check)));
    }
    let after_checks = child_memory_kib(pid);
    let report = classifier.report();
    assert_eq!(report.pid, Some(pid));
    assert_eq!(
        (report.spawned, report.kills, report.errors, report.verdicts),
        (1, 0, 0, 51)
    );
    eprintln!(
        "idle {idle} s; worker (VmRSS, VmHWM) KiB warm {warm:?}, after idle {after_idle:?}, after 50 checks {after_checks:?}"
    );
    if let (Some((_, warm_peak)), Some((_, peak))) = (warm, after_checks) {
        // Repeated full-window checks reuse the warm buffers.
        assert!(peak <= warm_peak + 8 * 1024, "{warm_peak} -> {peak} KiB");
    }
    classifier.close();
    assert_eq!(
        classifier.wait_finished(Duration::from_secs(5)),
        Teardown::Reaped
    );
}

/// Child mode of `inherited_lower_address_space_limits_are_kept`. With
/// `LAMP_INHERITED_AS_SOFT` set, this process lowers only its own soft
/// address-space limit and becomes the worker's self-check, which inherits
/// both limits as it would from a service manager. Without it, nothing runs.
#[cfg(target_os = "linux")]
#[test]
fn exec_check_model_under_inherited_limit() {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
    use std::os::unix::process::CommandExt;
    let Some(soft) = std::env::var("LAMP_INHERITED_AS_SOFT")
        .ok()
        .and_then(|value| value.parse().ok())
    else {
        return;
    };
    let hard = getrlimit(Resource::As).maximum;
    setrlimit(
        Resource::As,
        Rlimit {
            current: Some(soft),
            maximum: hard,
        },
    )
    .unwrap();
    let error = Command::new(env!("CARGO_BIN_EXE_lamp-turn-end"))
        .arg("--check-model")
        .arg(model_path())
        .exec();
    panic!("exec: {error}");
}

/// A soft address-space limit inherited below the worker's 256 MiB cap, with
/// a higher hard limit, stays in force: the worker never raises it.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires the pinned model; see docs/endpoint-detection.md"]
fn inherited_lower_address_space_limits_are_kept() {
    let check_under = |soft_mib: u64| {
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "parity::exec_check_model_under_inherited_limit",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("LAMP_INHERITED_AS_SOFT", (soft_mib << 20).to_string())
            .output()
            .unwrap()
    };
    // The worker's JSON follows the test harness's own `test … ... ` prefix.
    let report = |output: &std::process::Output| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| {
                line.find("{\"model\"")
                    .map(|start| line[start..].to_owned())
            })
            .map(|line| serde_json::from_str::<Value>(&line).unwrap())
    };
    // Above the worker's measured 113.5 MiB peak: the check passes and
    // reports the inherited 200 MiB, not the worker's own 256 MiB cap.
    let output = check_under(200);
    assert!(output.status.success(), "{output:?}");
    let limit = report(&output).unwrap()["address_space_limit_kib"].clone();
    assert_eq!(limit, 200 * 1024, "{output:?}");
    // Below the peak, as in the review finding (96 MiB): loading fails
    // instead of running under a raised limit.
    let output = check_under(96);
    assert!(!output.status.success(), "{output:?}");
    assert!(report(&output).is_none(), "{output:?}");
}
