//! `lamp-turn-end --model PATH [--parent PID] [--fast [--runtime LIBRARY]]`
//! `lamp-turn-end --check-model PATH [--fast [--runtime LIBRARY]]`
//! (installation self-check)
//! `lamp-turn-end bench PATH [RUNS] [--fast] [--threads N] [--cores fast|all|LIST]
//! [--runtime LIBRARY]` (timing on the device)
//! `lamp-turn-end agree PATH DIRECTORY... [--runtime LIBRARY] [--csv FILE]`
//! (fast against exact verdicts on labelled speech)
//!
//! Serves Smart Turn v3.2 verdicts for one interaction core over stdin and
//! stdout (see `lamp_turn_end_wire::frame`). It verifies the pinned model, runs
//! one cold inference, reports `Ready` and then answers one request at a time.
//! It writes no files. After each request it zeroes its request and
//! feature-extraction buffers; freed allocations and the inference runtime's
//! internal memory are not scrubbed. On Linux it dies with the core that
//! started it; an input end of stream is an ordinary stop. Before it reads the
//! model on Linux, it disables core files, caps its address space and lowers
//! its CPU priority.
//!
//! Without `--fast` it runs the exact model under tract on one thread, as it
//! always has. With `--fast` it keeps itself on the fastest cores, rewrites
//! the model's weight multiplications to 8 bits in memory and runs them with
//! ONNX Runtime on two threads; the library is the pinned file beside the
//! worker unless `--runtime` names another copy of it.
mod commands;

use lamp_turn_end::{
    FAST_THREADS, FastTurn, FeatureExtractor, Network, SMART_TURN_V3_2, SmartTurn, classify, cores,
    fast, reference,
};
use lamp_turn_end_wire::{cores as wire_cores, serve};
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

/// Niceness the worker adds on Linux. Under CPU contention, the fair
/// scheduler gives each capture, playback and provider thread about nine
/// times the worker's share; a verdict that arrives late is answered by the
/// core's silence fallback.
#[cfg(target_os = "linux")]
const NICE: i32 = 10;
/// Linux address-space cap: 2.2 times the 113.5 MiB `VmPeak` the self-check
/// measured on ARM64 Linux (docs/endpoint-detection.md). An allocation beyond
/// it ends the worker, which the core treats like any other worker failure.
#[cfg(target_os = "linux")]
const ADDRESS_SPACE_LIMIT: u64 = 256 << 20;
/// The fast model's cap: 2.2 times the 243.5 MiB `VmPeak` its `bench` and
/// self-check measured on ARM64 Linux with two threads (docs/endpoint-pilot.md,
/// "The fast model"). ONNX Runtime maps its 25 MB library and reserves a
/// stack and a heap arena for its second thread.
#[cfg(target_os = "linux")]
const FAST_ADDRESS_SPACE_LIMIT: u64 = 544 << 20;
/// Classifications of the reference signal in `--check-model`. The first is
/// cold; the rest report the warm cost.
const CHECK_RUNS: usize = 10;

/// How the worker runs the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Speed {
    /// 32-bit floats under tract, one thread, wherever the scheduler puts it.
    Exact,
    /// 8-bit weight multiplications under ONNX Runtime, on the fastest cores.
    Fast { runtime: PathBuf, threads: usize },
}

impl Speed {
    fn fast(runtime: Option<PathBuf>, threads: usize) -> Result<Self, String> {
        let runtime = match runtime {
            Some(runtime) => runtime,
            None => beside_worker(fast::runtime_asset().map_err(|e| e.to_string())?.file_name)?,
        };
        Ok(Self::Fast { runtime, threads })
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Fast { .. } => "fast",
        }
    }
}

/// A file installed in the worker's own directory.
fn beside_worker(file_name: &str) -> Result<PathBuf, String> {
    let worker = std::env::current_exe().map_err(|error| format!("worker path: {error}"))?;
    Ok(worker.with_file_name(file_name))
}

struct Arguments {
    model: PathBuf,
    parent: Option<u32>,
    speed: Speed,
}

const USAGE: &str = "usage: lamp-turn-end --model PATH [--parent PID] [--fast [--runtime LIBRARY]]";

fn parse(mut arguments: impl Iterator<Item = OsString>) -> Result<Arguments, String> {
    let (mut model, mut parent, mut fast, mut runtime) = (None, None, false, None);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--fast") if !fast => {
                fast = true;
                continue;
            }
            Some("--model" | "--parent" | "--runtime") => {}
            _ => return Err(USAGE.into()),
        }
        let value = arguments.next().ok_or("missing option value")?;
        match argument.to_str() {
            Some("--model") if model.is_none() => model = Some(PathBuf::from(value)),
            Some("--runtime") if runtime.is_none() => runtime = Some(PathBuf::from(value)),
            Some("--parent") if parent.is_none() => {
                let pid = value.to_str().and_then(|pid| pid.parse().ok());
                parent = Some(pid.ok_or("invalid parent process ID")?);
            }
            _ => return Err(USAGE.into()),
        }
    }
    if runtime.is_some() && !fast {
        return Err("--runtime is only used with --fast".into());
    }
    let speed = if fast {
        Speed::fast(runtime, FAST_THREADS)?
    } else {
        Speed::Exact
    };
    Ok(Arguments {
        model: model.ok_or("missing --model")?,
        parent,
        speed,
    })
}

/// A hung worker must not outlive the core that can no longer kill it.
#[cfg(target_os = "linux")]
fn follow_parent(parent: Option<u32>) -> Result<(), String> {
    use rustix::process::{Signal, getppid, set_parent_process_death_signal};
    set_parent_process_death_signal(Some(Signal::KILL))
        .map_err(|error| format!("parent death signal: {error}"))?;
    // The core may have exited before the signal was armed.
    match (parent, getppid()) {
        (Some(expected), Some(actual)) if actual.as_raw_pid() as u32 != expected => {
            Err("parent process already exited".into())
        }
        _ => Ok(()),
    }
}

#[cfg(not(target_os = "linux"))]
fn follow_parent(_parent: Option<u32>) -> Result<(), String> {
    Ok(())
}

/// The lowest of an inherited soft limit, an inherited hard limit (`None` is
/// unlimited) and the worker's own cap.
#[cfg(any(target_os = "linux", test))]
fn lowest_limit(current: Option<u64>, maximum: Option<u64>, cap: u64) -> u64 {
    [current, maximum].into_iter().flatten().fold(cap, u64::min)
}

/// Bounds the worker before it reads the model. Never raises a soft or hard
/// limit the service manager already set lower.
#[cfg(target_os = "linux")]
fn bound_resources(speed: &Speed) -> Result<(), String> {
    use rustix::process::{Resource, Rlimit, getrlimit, nice, setrlimit};
    let cap = |resource, bytes: u64| {
        let inherited = getrlimit(resource);
        let bytes = lowest_limit(inherited.current, inherited.maximum, bytes);
        setrlimit(
            resource,
            Rlimit {
                current: Some(bytes),
                maximum: Some(bytes),
            },
        )
    };
    let address_space = match speed {
        Speed::Exact => ADDRESS_SPACE_LIMIT,
        Speed::Fast { .. } => FAST_ADDRESS_SPACE_LIMIT,
    };
    // A crash must not write retained speech to a core file.
    cap(Resource::Core, 0).map_err(|error| format!("core file limit: {error}"))?;
    cap(Resource::As, address_space).map_err(|error| format!("address space limit: {error}"))?;
    nice(NICE).map_err(|error| format!("priority: {error}"))?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn bound_resources(_speed: &Speed) -> Result<(), String> {
    Ok(())
}

/// The effective address-space limit in KiB and niceness, as JSON values.
#[cfg(target_os = "linux")]
fn bounds() -> [String; 2] {
    use rustix::process::{Resource, getpriority_process, getrlimit};
    let limit = getrlimit(Resource::As).current;
    [
        limit.map_or("null".into(), |bytes| (bytes >> 10).to_string()),
        getpriority_process(None).map_or("null".into(), |nice| nice.to_string()),
    ]
}

#[cfg(not(target_os = "linux"))]
fn bounds() -> [String; 2] {
    ["null".into(), "null".into()]
}

/// `VmPeak`, `VmHWM` and `VmRSS` in KiB, from Linux `/proc/self/status`.
fn memory_kib() -> [Option<u64>; 3] {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    ["VmPeak:", "VmHWM:", "VmRSS:"].map(|key| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
    })
}

fn cpu_us() -> u64 {
    let now = rustix::time::clock_gettime(rustix::time::ClockId::ProcessCPUTime);
    (now.tv_sec as u64) * 1_000_000 + (now.tv_nsec as u64) / 1_000
}

fn ms(us: u64) -> f64 {
    us as f64 / 1_000.0
}

/// Keeps the worker on the fastest cores when the cores differ, less the
/// capture worker's when the voice passed `LAMP_CAPTURE_CORES` and capture
/// takes them (both run at RR 20; `lamp_turn_end_wire::cores::kept`).
/// Returns the threads to run: one a kept core beside capture, otherwise
/// `threads`.
fn keep_on_fast_cores(threads: usize) -> Result<usize, String> {
    let Some(fast) = cores::fastest(&cores::capacities(Path::new("/sys/devices/system/cpu")))
    else {
        return Ok(threads);
    };
    let capture = std::env::var(wire_cores::CAPTURE_CORES).ok();
    let allowed = cores::allowed().map(|allowed| allowed.into_iter().collect());
    let (kept, threads) = wire_cores::kept(&fast, capture.as_deref(), allowed.as_ref(), threads);
    cores::keep_on(&kept)?;
    Ok(threads)
}

/// Loads the model the way `speed` says, with the ONNX threads it runs
/// (`None` for `Exact`). With `Fast`, the calling thread is first kept on the
/// fastest cores so that ONNX Runtime's threads start there.
fn load(model: &Path, speed: &Speed) -> Result<(Box<dyn Network>, Option<usize>), String> {
    match speed {
        Speed::Exact => SmartTurn::load(model, &SMART_TURN_V3_2)
            .map(|model| (Box::new(model) as Box<dyn Network>, None))
            .map_err(|error| format!("model {}: {error}", model.display())),
        Speed::Fast { runtime, threads } => {
            // Beside capture on one fast core the model runs one thread on
            // the other (213 against 148 ms a verdict on lamp-4ace,
            // docs/endpoint-pilot.md).
            let threads = keep_on_fast_cores(*threads)?;
            FastTurn::load(model, &SMART_TURN_V3_2, runtime, threads)
                .map(|model| (Box::new(model) as Box<dyn Network>, Some(threads)))
                .map_err(|error| format!("fast model {}: {error}", model.display()))
        }
    }
}

/// Loads the model under the worker's own bounds, classifies the reference
/// signal `CHECK_RUNS` times against its recorded probability, and returns
/// one JSON line of costs.
fn self_check(path: &Path, speed: &Speed) -> Result<String, String> {
    bound_resources(speed)?;
    let (started, cpu_started) = (Instant::now(), cpu_us());
    let (mut model, used) = load(path, speed)?;
    let load_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    let load_cpu_us = cpu_us() - cpu_started;
    let (expected, tolerance, reference_name) = match speed {
        Speed::Exact => (
            reference::VOICED_PROBABILITY,
            reference::TOLERANCE,
            "ONNX Runtime's",
        ),
        Speed::Fast { .. } => (
            reference::VOICED_PROBABILITY_FAST,
            reference::TOLERANCE_FAST,
            "the fast model's recorded",
        ),
    };
    let (mut extractor, signal) = (FeatureExtractor::default(), reference::voiced());
    let (mut costs, mut probability) = (Vec::with_capacity(CHECK_RUNS), 0.0);
    let cpu_started = cpu_us();
    for _ in 0..CHECK_RUNS {
        let classified = classify(model.as_mut(), &mut extractor, &signal)?;
        probability = classified.probability;
        if (probability - expected).abs() > tolerance {
            return Err(format!(
                "reference probability {probability} differs from {reference_name} {expected}"
            ));
        }
        costs.push(classified.features_us + classified.inference_us);
    }
    let cpu_per_check_us = (cpu_us() - cpu_started) / CHECK_RUNS as u64;
    let cold_us = costs[0];
    let mut warm = costs[1..].to_vec();
    warm.sort_unstable();
    let [peak, hwm, rss] = memory_kib().map(|value| value.map_or("null".into(), |v| v.to_string()));
    let [address_space, nice] = bounds();
    let mut line = format!(
        concat!(
            "{{\"model\":\"{}\",\"bytes\":{},\"sha256\":\"{}\",\"probability\":{},",
            "\"reference\":{},\"load_ms\":{:.1},\"load_cpu_ms\":{:.1},\"cold_ms\":{:.1},",
            "\"warm_median_ms\":{:.1},\"warm_max_ms\":{:.1},\"cpu_per_check_ms\":{:.1},",
            "\"vm_peak_kib\":{},\"vm_hwm_kib\":{},\"vm_rss_kib\":{},",
            "\"address_space_limit_kib\":{},\"nice\":{}"
        ),
        SMART_TURN_V3_2.file_name,
        SMART_TURN_V3_2.bytes,
        SMART_TURN_V3_2.sha256,
        probability,
        expected,
        ms(load_us),
        ms(load_cpu_us),
        ms(cold_us),
        ms(warm[warm.len() / 2]),
        ms(warm[warm.len() - 1]),
        ms(cpu_per_check_us),
        peak,
        hwm,
        rss,
        address_space,
        nice,
    );
    if let (Speed::Fast { runtime, .. }, Some(threads)) = (speed, used) {
        let asset = fast::runtime_asset().map_err(|error| error.to_string())?;
        line.push_str(&format!(
            ",\"speed\":\"fast\",\"threads\":{threads},\"cores\":{},\"runtime\":\"{}\",\"runtime_version\":\"{}\",\"runtime_sha256\":\"{}\"",
            commands::json_cores(cores::allowed()),
            runtime.display(),
            asset.version,
            asset.sha256,
        ));
    }
    line.push('}');
    Ok(line)
}

/// `--check-model PATH [--fast [--runtime LIBRARY]]`.
fn check_model(arguments: &[OsString]) -> Option<ExitCode> {
    let (flag, rest) = arguments.split_first()?;
    if flag != "--check-model" {
        return None;
    }
    let path = PathBuf::from(rest.first()?);
    let speed = match &rest[1..] {
        [] => Ok(Speed::Exact),
        [fast] if fast == "--fast" => Speed::fast(None, FAST_THREADS),
        [fast, runtime, library] if fast == "--fast" && runtime == "--runtime" => {
            Speed::fast(Some(PathBuf::from(library)), FAST_THREADS)
        }
        _ => Err("usage: lamp-turn-end --check-model PATH [--fast [--runtime LIBRARY]]".into()),
    };
    Some(match speed.and_then(|speed| self_check(&path, &speed)) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("{}: {reason}", path.display());
            ExitCode::from(2)
        }
    })
}

fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    if let Some(code) = check_model(&arguments) {
        return code;
    }
    match arguments.first().and_then(|first| first.to_str()) {
        Some("bench") => return commands::bench(&arguments[1..]),
        Some("agree") => return commands::agree(&arguments[1..]),
        _ => {}
    }
    let mut output = io::stdout().lock();
    let started = Instant::now();
    let setup = parse(arguments.into_iter()).and_then(|arguments| {
        follow_parent(arguments.parent)?;
        bound_resources(&arguments.speed)?;
        load(&arguments.model, &arguments.speed).map(|(model, _)| model)
    });
    let mut model = match setup {
        Ok(model) => model,
        Err(reason) => {
            let _ = serve::startup_failed(&mut output, &reason);
            return ExitCode::from(2);
        }
    };
    let load_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    let mut extractor = FeatureExtractor::default();
    let cold = match classify(model.as_mut(), &mut extractor, &[]) {
        Ok(cold) => cold.features_us + cold.inference_us,
        Err(reason) => {
            let _ = serve::startup_failed(&mut output, &format!("cold inference: {reason}"));
            return ExitCode::from(2);
        }
    };
    let sha256 = lamp_turn_end_wire::sha256_bytes(SMART_TURN_V3_2.sha256);
    if serve::ready(&mut output, load_us, cold, sha256).is_err() {
        return ExitCode::from(5);
    }
    let stop = serve::serve(&mut io::stdin().lock(), &mut output, |pcm| {
        classify(model.as_mut(), &mut extractor, pcm)
    });
    if !matches!(stop, serve::Stop::Closed) {
        eprintln!("lamp-turn-end: {stop:?}");
    }
    ExitCode::from(stop.exit_code() as u8)
}

#[cfg(test)]
mod tests {
    use super::{Speed, lowest_limit, parse};
    use std::{ffi::OsString, path::PathBuf};

    fn arguments(text: &str) -> impl Iterator<Item = OsString> {
        text.split_whitespace()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn inherited_lower_limits_are_never_raised() {
        const CAP: u64 = 256 << 20;
        // The review finding: a 96 MiB soft limit under a higher hard limit.
        assert_eq!(lowest_limit(Some(96 << 20), Some(512 << 20), CAP), 96 << 20);
        assert_eq!(lowest_limit(Some(96 << 20), None, CAP), 96 << 20);
        assert_eq!(lowest_limit(None, Some(128 << 20), CAP), 128 << 20);
        assert_eq!(
            lowest_limit(Some(300 << 20), Some(200 << 20), CAP),
            200 << 20
        );
        assert_eq!(lowest_limit(None, None, CAP), CAP);
        assert_eq!(lowest_limit(Some(1 << 40), None, CAP), CAP);
        assert_eq!(lowest_limit(None, None, 0), 0);
    }

    #[test]
    fn the_default_command_line_keeps_the_exact_model() {
        let parsed = parse(arguments("--model m.onnx --parent 42")).unwrap();
        assert_eq!(
            (parsed.model, parsed.parent, parsed.speed),
            (PathBuf::from("m.onnx"), Some(42), Speed::Exact)
        );
    }

    #[test]
    fn fast_is_only_chosen_by_its_flag() {
        let parsed = parse(arguments("--model m.onnx --fast --runtime /x/lib.so")).unwrap();
        assert_eq!(
            parsed.speed,
            Speed::Fast {
                runtime: PathBuf::from("/x/lib.so"),
                threads: 2
            }
        );
        // Order does not matter; the library defaults to the worker's own
        // directory.
        match parse(arguments("--fast --parent 7 --model m.onnx")) {
            Ok(parsed) => {
                let Speed::Fast { runtime, .. } = parsed.speed else {
                    panic!("not fast");
                };
                let worker = std::env::current_exe().unwrap();
                assert_eq!(runtime.parent(), worker.parent());
            }
            // A machine no library is pinned for (an Intel Mac, where this
            // was first run beside the lamp): it says so and starts nothing.
            Err(why) => assert_eq!(why.to_string(), "no ONNX Runtime is pinned for this target"),
        }
        for wrong in [
            "--model m.onnx --runtime /x/lib.so",
            "--model m.onnx --fast --fast",
            "--model m.onnx --threads 2",
            "--model m.onnx --model n.onnx",
            "--fast",
        ] {
            assert!(parse(arguments(wrong)).is_err(), "{wrong}");
        }
    }
}
