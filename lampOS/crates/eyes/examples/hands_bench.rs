//! How long the two MediaPipe hand models take on this machine, under tract
//! (as the eyes run YuNet) and under the pinned ONNX Runtime (as the eyes run
//! Face Mesh V2), on one thread: the measure behind the hand-tracking design
//! note (#5385). Not part of the product: it reads two model files, runs them
//! on synthetic numbers (never a picture) and prints times and CPU. Nothing is
//! published, written or kept.
//!
//! `cargo run --release -p lamp-eyes --example hands_bench -- PALM HAND
//!  [--runtime PATH_TO_libonnxruntime] [--runs N] [--seconds S]`
//!
//! Prints, for each engine: the palm detector (192x192) and the hand
//! landmarks (224x224) timed back to back (median, 90th percentile, worst), then
//! two paced runs for S seconds each, like the eyes would run them: the
//! landmarks 6 times a second, and the palm detector once a second, with the
//! process's CPU over each.
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs::File,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};
use tract_onnx::prelude::*;

type Failure = Box<dyn Error + Send + Sync>;

const PALM_BYTES: u64 = 3_905_734;
const PALM_SHA256: &str = "78ff51c38496b7fc8b8ebdb6cc8c1abb02fa6c38427c6848254cdaba57fcce7c";
const PALM_SIDE: usize = 192;
const HAND_BYTES: u64 = 4_099_621;
const HAND_SHA256: &str = "db0898ae717b76b075d9bf563af315b29562e11f8df5027a1ef07b02bef6d81c";
const HAND_SIDE: usize = 224;
/// The eyes' track rate while a hand is followed, and the palm detector's
/// search rate.
const TRACK_HZ: f64 = 6.0;
const SEARCH_HZ: f64 = 1.0;

/// The model's exact bytes, checked by size and SHA-256 before any parser
/// sees them, as the eyes do for their own models.
fn read_pinned(path: &Path, length: u64, sha256: &str) -> Result<Vec<u8>, Failure> {
    let file = File::open(path)?;
    if file.metadata()?.len() != length {
        return Err(format!("{} has an unexpected size", path.display()).into());
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(length + 1).read_to_end(&mut bytes)?;
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if bytes.len() as u64 != length || digest != sha256 {
        return Err(format!("{} does not match the pinned SHA256", path.display()).into());
    }
    Ok(bytes)
}

/// Numbers in 0 to 1 that are the same on every run: the models' cost does not
/// depend on what they are given.
fn synthetic(side: usize) -> Vec<f32> {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    (0..side * side * 3)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / (1u64 << 24) as f32
        })
        .collect()
}

/// Seconds of CPU this process has used (user and system, all threads), from
/// /proc, or none when /proc can't be read as expected. Linux presents these in
/// USER_HZ, 100 ticks a second on every target the lamp ships.
fn cpu_seconds() -> Option<f64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let after = stat.rsplit(')').next()?;
    let fields: Vec<&str> = after.split_whitespace().collect();
    // After the name: state is field 3, utime 14, stime 15 (1-based).
    let tick = |at: usize| fields.get(at)?.parse::<f64>().ok();
    Some((tick(11)? + tick(12)?) / 100.0)
}

/// The CPU used since `before` (from [`cpu_seconds`]) over `wall` seconds, as
/// a share of one core, or a plain "unavailable".
fn cpu_share(before: Option<f64>, wall: f64) -> String {
    match (before, cpu_seconds()) {
        (Some(before), Some(now)) => format!("{:.1} % of one core", 100.0 * (now - before) / wall),
        _ => "unavailable".to_owned(),
    }
}

/// One model under one engine.
enum Engine {
    Tract(std::sync::Arc<TypedRunnableModel>, usize),
    // Field order is drop order: the session goes before its environment.
    Ort {
        session: Box<ort::session::Session>,
        _environment: Box<ort::environment::Environment>,
        side: usize,
    },
}

impl Engine {
    fn run(&mut self, input: &[f32]) -> Result<usize, Failure> {
        match self {
            Self::Tract(plan, side) => {
                let tensor = Tensor::from_shape(&[1, *side, *side, 3], input)?;
                let outputs = plan.run(tvec!(tensor.into()))?;
                Ok(outputs.iter().map(|output| output.len()).sum())
            }
            Self::Ort { session, side, .. } => {
                let tensor =
                    ort::value::Tensor::from_array(([1usize, *side, *side, 3], input.to_vec()))?;
                let outputs = session.run(ort::inputs![tensor])?;
                let mut length = 0;
                for (_, output) in outputs.iter() {
                    length += output.try_extract_tensor::<f32>()?.1.len();
                }
                Ok(length)
            }
        }
    }
}

fn tract_engine(bytes: &[u8], side: usize) -> Result<Engine, Failure> {
    let plan = tract_onnx::onnx()
        .model_for_read(&mut &bytes[..])?
        .with_input_fact(0, f32::fact([1, side, side, 3]).into())?
        .into_optimized()?
        .into_runnable()?;
    Ok(Engine::Tract(plan, side))
}

/// The pinned ONNX Runtime, loaded once a process (checked against the voice's
/// pin first, as the eyes do for their mesh).
fn ort_environment(runtime: &Path) -> Result<ort::environment::Environment, Failure> {
    let library = lamp_turn_end::ONNX_RUNTIME.ok_or("no ONNX Runtime is pinned for this target")?;
    lamp_turn_end::fast::verify_runtime(runtime, &library)
        .map_err(|error| format!("ONNX Runtime library: {error}"))?;
    ort::init_from(runtime)
        .map_err(|error| error.to_string())?
        .with_name("hands-bench")
        .with_telemetry(false)
        .build()
        .map_err(|error| error.to_string().into())
}

fn ort_engine(
    bytes: &[u8],
    side: usize,
    environment: &ort::environment::Environment,
) -> Result<Engine, Failure> {
    let session = ort::session::Session::builder(environment)
        .and_then(|builder| {
            Ok(builder
                .with_optimization_level(ort::session::builder::GraphOptimizationLevel::All)?)
        })
        .and_then(|builder| Ok(builder.with_intra_threads(1)?))
        .and_then(|builder| Ok(builder.with_inter_threads(1)?))
        .and_then(|mut builder| builder.commit_from_memory(bytes))
        .map_err(|error| format!("ONNX Runtime session: {error}"))?;
    Ok(Engine::Ort {
        session: Box::new(session),
        _environment: Box::new(environment.clone()),
        side,
    })
}

fn spread(mut times: Vec<f64>) -> String {
    times.sort_by(f64::total_cmp);
    let at = |share: f64| times[((times.len() - 1) as f64 * share).round() as usize];
    format!(
        "median {:.1} ms, p90 {:.1}, worst {:.1}",
        at(0.5),
        at(0.9),
        at(1.0)
    )
}

/// `engine` run `hz` times a second for `seconds`: its CPU as a share of one core.
fn paced(engine: &mut Engine, input: &[f32], hz: f64, seconds: f64) -> Result<String, Failure> {
    let (wall, cpu) = (Instant::now(), cpu_seconds());
    let period = Duration::from_secs_f64(1.0 / hz);
    let mut runs = 0u32;
    let mut next = Instant::now();
    while wall.elapsed().as_secs_f64() < seconds {
        engine.run(input)?;
        runs += 1;
        next += period;
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    let took = wall.elapsed().as_secs_f64();
    Ok(format!(
        "{runs} runs in {took:.1} s = {:.1} a second, CPU {}",
        f64::from(runs) / took,
        cpu_share(cpu, took)
    ))
}

fn main() -> Result<(), Failure> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [palm, hand, rest @ ..] = args.as_slice() else {
        return Err("usage: hands_bench PALM HAND [--runtime LIB] [--runs N] [--seconds S]".into());
    };
    let option = |name: &str| {
        rest.iter()
            .position(|argument| argument == name)
            .and_then(|at| rest.get(at + 1))
    };
    let runs: usize = option("--runs").map_or(Ok(60), |value| value.parse())?;
    let seconds: f64 = option("--seconds").map_or(Ok(20.0), |value| value.parse())?;
    // Before any model is read: zero runs have no median, and a duration that
    // is not a finite positive number never ends or never runs.
    if runs == 0 || !seconds.is_finite() || seconds <= 0.0 || seconds > 600.0 {
        return Err("--runs is 1 or more and --seconds is above 0 and at most 600".into());
    }
    let palm_bytes = read_pinned(Path::new(palm), PALM_BYTES, PALM_SHA256)?;
    let hand_bytes = read_pinned(Path::new(hand), HAND_BYTES, HAND_SHA256)?;
    let (palm_input, hand_input) = (synthetic(PALM_SIDE), synthetic(HAND_SIDE));
    println!(
        "hands_bench: {runs} timed runs, then {seconds:.0} s paced, one thread, synthetic input"
    );
    let mut engines: Vec<(&str, Engine, Engine)> = Vec::new();
    let started = Instant::now();
    engines.push((
        "tract",
        tract_engine(&palm_bytes, PALM_SIDE)?,
        tract_engine(&hand_bytes, HAND_SIDE)?,
    ));
    println!(
        "tract: both models loaded in {:.0} ms",
        started.elapsed().as_secs_f64() * 1e3
    );
    if let Some(runtime) = option("--runtime") {
        let started = Instant::now();
        let environment = ort_environment(Path::new(runtime))?;
        engines.push((
            "ort",
            ort_engine(&palm_bytes, PALM_SIDE, &environment)?,
            ort_engine(&hand_bytes, HAND_SIDE, &environment)?,
        ));
        println!(
            "ort: both models loaded in {:.0} ms",
            started.elapsed().as_secs_f64() * 1e3
        );
    }
    for (name, palm_engine, hand_engine) in &mut engines {
        for _ in 0..5 {
            palm_engine.run(&palm_input)?;
            hand_engine.run(&hand_input)?;
        }
        let (mut palms, mut hands, mut pairs) = (Vec::new(), Vec::new(), Vec::new());
        let (wall, cpu) = (Instant::now(), cpu_seconds());
        for _ in 0..runs {
            let one = Instant::now();
            palm_engine.run(&palm_input)?;
            let two = Instant::now();
            hand_engine.run(&hand_input)?;
            let three = Instant::now();
            palms.push((two - one).as_secs_f64() * 1e3);
            hands.push((three - two).as_secs_f64() * 1e3);
            pairs.push((three - one).as_secs_f64() * 1e3);
        }
        let share = cpu_share(cpu, wall.elapsed().as_secs_f64());
        println!("{name} palm 192x192:  {}", spread(palms));
        println!("{name} landmarks 224: {}", spread(hands));
        println!(
            "{name} both:          {}   (CPU {share} while timed)",
            spread(pairs)
        );
        println!(
            "{name} paced landmarks {TRACK_HZ} a second: {}",
            paced(hand_engine, &hand_input, TRACK_HZ, seconds)?
        );
        println!(
            "{name} paced palm {SEARCH_HZ} a second:     {}",
            paced(palm_engine, &palm_input, SEARCH_HZ, seconds)?
        );
    }
    Ok(())
}
