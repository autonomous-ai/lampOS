//! The fast model: the pinned model rewritten to 8 bits and run by ONNX
//! Runtime, in process and as the real `lamp-turn-end --fast` worker behind
//! the core's client.
//!
//! Tests that need the model and the library are ignored by default: install
//! the pinned model and ONNX Runtime library under `.cache/endpoint/` (or set
//! `LAMP_SMART_TURN_MODEL` and `LAMP_ONNX_RUNTIME`), then run
//! `cargo test -p lamp-turn-end --test fast -- --include-ignored`.
use lamp_turn_end::{
    FAST_THREADS, FastTurn, FeatureExtractor, ONNX_RUNTIME, SMART_TURN_V3_2, SmartTurn, reference,
};
use lamp_turn_end_wire::{
    Availability, Classifier, Limits, ProcessClassifier, Submit, Teardown, Token, WorkerCommand,
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn cache(file_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.cache/endpoint")
        .join(file_name)
}

fn model_path() -> PathBuf {
    std::env::var_os("LAMP_SMART_TURN_MODEL")
        .map_or_else(|| cache(SMART_TURN_V3_2.file_name), PathBuf::from)
}

fn runtime_path() -> PathBuf {
    std::env::var_os("LAMP_ONNX_RUNTIME").map_or_else(
        || cache(ONNX_RUNTIME.expect("no pinned ONNX Runtime here").file_name),
        PathBuf::from,
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("lamp-turn-end-fast-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_missing_or_different_library_is_refused_before_it_is_loaded() {
    let Some(asset) = ONNX_RUNTIME else {
        return;
    };
    let dir = scratch("library");
    let missing = dir.join(asset.file_name);
    let error = FastTurn::load(&model_path(), &SMART_TURN_V3_2, &missing, FAST_THREADS)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("ONNX Runtime library"), "{error}");
    assert!(error.contains("No such file"), "{error}");
    // Any other bytes, of whatever size, never reach the loader.
    std::fs::write(&missing, b"not a library").unwrap();
    let error = FastTurn::load(&model_path(), &SMART_TURN_V3_2, &missing, FAST_THREADS)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("unexpected size"), "{error}");
    let error = FastTurn::load(&model_path(), &SMART_TURN_V3_2, &missing, 0)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("at least one thread"), "{error}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// The rewrite's shape on the real model, its probability on the self-check
/// signal, and how far that is from the exact model's. ONNX Runtime can be
/// loaded once per process, so this is one test.
#[test]
#[ignore = "requires the pinned model and ONNX Runtime; see docs/endpoint-pilot.md"]
fn fast_model_gives_its_recorded_probability_close_to_the_exact_one() {
    let mut fast = FastTurn::load(
        &model_path(),
        &SMART_TURN_V3_2,
        &runtime_path(),
        FAST_THREADS,
    )
    .unwrap();
    let rewrite = fast.rewrite();
    eprintln!("{rewrite:?}");
    // Four layers of six weight multiplications and the pooling's first,
    // less the one kept in floats, whose input then needs no quantizing.
    assert_eq!((rewrite.multiplications, rewrite.activations), (24, 16));
    let exact = SmartTurn::load(&model_path(), &SMART_TURN_V3_2).unwrap();
    let mut extractor = FeatureExtractor::default();
    let features = extractor.compute(&reference::voiced());
    let probability = fast.probability(features.clone()).unwrap();
    let exact_probability = exact.probability(features).unwrap();
    eprintln!("fast {probability:.7} exact {exact_probability:.7}");
    assert!(
        (probability - reference::VOICED_PROBABILITY_FAST).abs() < reference::TOLERANCE_FAST,
        "{probability}"
    );
    assert!((probability - exact_probability).abs() < 0.01);
    // The same request twice gives the same answer.
    let again = fast
        .probability(extractor.compute(&reference::voiced()))
        .unwrap();
    assert_eq!(probability, again);
    // Silence, a short burst and noise all give a probability.
    for pcm in [
        Vec::new(),
        vec![3_000; 4_800],
        reference::voiced()[..32_000].to_vec(),
    ] {
        let p = fast.probability(extractor.compute(&pcm)).unwrap();
        assert!((0.0..=1.0).contains(&p));
    }
    assert!(fast.probability(vec![f32::NAN; 64_000]).is_err());
    assert!(fast.probability(vec![0.0; 10]).is_err());
}

fn fast_worker(runtime: &Path) -> ProcessClassifier {
    let mut command =
        WorkerCommand::smart_turn_fast(env!("CARGO_BIN_EXE_lamp-turn-end").into(), model_path());
    command.args.push("--runtime".into());
    command.args.push(runtime.into());
    ProcessClassifier::spawn(command, Limits::default())
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
    utterance: 7,
    generation: 1,
    check: 0,
};

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

/// User plus system CPU time of a process in milliseconds, from Linux
/// `/proc/PID/stat` (clock ticks of 10 ms).
fn cpu_ms(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields = stat[stat.rfind(')')? + 2..].split(' ').collect::<Vec<_>>();
    let ticks = fields[11].parse::<u64>().ok()? + fields[12].parse::<u64>().ok()?;
    Some(ticks * 10)
}

/// A worker that cannot find its library fails once, at startup, with the
/// reason, and is never restarted: every pause keeps the silence window.
#[test]
#[ignore = "requires the pinned model; see docs/endpoint-pilot.md"]
fn a_fast_worker_without_its_library_fails_once_and_is_not_restarted() {
    let dir = scratch("worker");
    let mut classifier = fast_worker(&dir.join("libonnxruntime-missing"));
    assert_eq!(classifier.submit(TOKEN, &[1; 160]), Submit::Unavailable);
    let Availability::Failed(reason) = settle(&classifier, Duration::from_secs(10)) else {
        panic!("the worker did not fail");
    };
    assert!(reason.contains("ONNX Runtime library"), "{reason}");
    assert_eq!(
        classifier.wait_finished(Duration::from_secs(2)),
        Teardown::Reaped
    );
    let report = classifier.report();
    assert_eq!((report.spawned, report.ready, report.kills), (1, 0, 0));
    std::fs::remove_dir_all(dir).unwrap();
}

/// The fast worker reports the same model identity, answers with the fast
/// model's probability, stays within its bounds, and uses no processor time
/// while it waits.
#[test]
#[ignore = "requires the pinned model and ONNX Runtime; see docs/endpoint-pilot.md"]
fn fast_worker_answers_is_bounded_and_idles_without_spinning() {
    let mut classifier = fast_worker(&runtime_path());
    assert_eq!(
        settle(&classifier, Duration::from_secs(30)),
        Availability::Ready
    );
    let report = classifier.report();
    assert_eq!(report.model_sha256.as_deref(), Some(SMART_TURN_V3_2.sha256));
    eprintln!(
        "fast worker load {} ms, cold {} ms, spawn to ready {} ms",
        report.load_us.unwrap() / 1_000,
        report.cold_us.unwrap() / 1_000,
        report.ready_after_us.unwrap() / 1_000
    );
    let pid = report.pid.unwrap();
    let close = |p: f32| (p - reference::VOICED_PROBABILITY_FAST).abs() < reference::TOLERANCE_FAST;
    assert!(close(classify_reference(&mut classifier, 0)));
    if cfg!(target_os = "linux") {
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
        assert_eq!(limit("Max address space"), ["570425344", "570425344"]);
        // Let the pool's threads finish spinning, then measure a quiet spell.
        std::thread::sleep(Duration::from_millis(500));
        let before = cpu_ms(pid).unwrap();
        std::thread::sleep(Duration::from_secs(3));
        let idle = cpu_ms(pid).unwrap() - before;
        eprintln!("fast worker processor time over 3 s of quiet: {idle} ms");
        assert!(idle <= 30, "{idle} ms of processor time while idle");
    }
    for check in 1..=30 {
        assert!(close(classify_reference(&mut classifier, check)));
    }
    let report = classifier.report();
    assert_eq!(
        (report.spawned, report.kills, report.errors, report.verdicts),
        (1, 0, 0, 31)
    );
    classifier.close();
    assert_eq!(
        classifier.wait_finished(Duration::from_secs(5)),
        Teardown::Reaped
    );
}
