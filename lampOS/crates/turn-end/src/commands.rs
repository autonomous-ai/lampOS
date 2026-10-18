//! `lamp-turn-end bench` and `lamp-turn-end agree`: measurements that need
//! only the worker, the model file and, for the fast model, its library.
use crate::{Speed, bound_resources, bounds, cpu_us, keep_on_fast_cores, load, memory_kib, ms};
use lamp_turn_end::{FAST_THREADS, FastTurn, FeatureExtractor, SMART_TURN_V3_2, SmartTurn};
use lamp_turn_end::{clips, cores, fast, reference};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

/// Runs of `bench` unless told otherwise.
const BENCH_RUNS: usize = 20;

/// A list of cores as a JSON array, or `null` when unknown.
pub fn json_cores(cores: Option<Vec<usize>>) -> String {
    cores.map_or("null".into(), |cores| json!(cores).to_string())
}

fn fail(reason: impl std::fmt::Display) -> ExitCode {
    eprintln!("lamp-turn-end: {reason}");
    ExitCode::from(2)
}

/// Which cores `bench` keeps itself on.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Cores {
    /// Wherever the scheduler puts it (the exact model's default).
    All,
    /// The cores with the largest capacity (the fast model's default).
    Fast,
    List(Vec<usize>),
}

#[derive(Debug, PartialEq, Eq)]
struct Bench {
    model: PathBuf,
    runs: usize,
    speed: Speed,
    cores: Cores,
    priority: Option<u8>,
}

const BENCH_USAGE: &str = "usage: lamp-turn-end bench PATH [RUNS] [--fast] [--threads N] \
                           [--cores fast|all|LIST] [--runtime LIBRARY] [--no-spin] \
                           [--realtime-priority N]";

fn parse_bench(arguments: &[OsString]) -> Result<Bench, String> {
    let mut words = arguments
        .iter()
        .map(|word| word.to_str().ok_or(BENCH_USAGE));
    let model = PathBuf::from(words.next().ok_or(BENCH_USAGE)??);
    let (mut runs, mut fast, mut threads, mut cores, mut runtime) = (None, false, None, None, None);
    let (mut spin, mut priority) = (true, None);
    while let Some(word) = words.next() {
        let word = word?;
        let mut value = || words.next().ok_or(BENCH_USAGE).and_then(|value| value);
        match word {
            "--fast" if !fast => fast = true,
            "--no-spin" if spin => spin = false,
            "--realtime-priority" if priority.is_none() => {
                priority = Some(crate::realtime_priority(Some(value()?))?);
            }
            "--threads" if threads.is_none() => {
                let count = value()?.parse::<usize>().map_err(|_| "invalid --threads")?;
                threads = Some(count).filter(|&count| count > 0);
                threads.ok_or("--threads must be at least 1")?;
            }
            "--cores" if cores.is_none() => {
                cores = Some(match value()? {
                    "fast" => Cores::Fast,
                    "all" => Cores::All,
                    list => Cores::List(
                        list.split(',')
                            .map(|cpu| cpu.trim().parse::<usize>())
                            .collect::<Result<_, _>>()
                            .map_err(|_| "invalid --cores")?,
                    ),
                })
            }
            "--runtime" if runtime.is_none() => runtime = Some(PathBuf::from(value()?)),
            number if runs.is_none() && !number.starts_with('-') => {
                runs = Some(number.parse::<usize>().map_err(|_| BENCH_USAGE)?);
            }
            _ => return Err(BENCH_USAGE.into()),
        }
    }
    let runs = runs.unwrap_or(BENCH_RUNS);
    if runs < 2 {
        return Err("bench needs at least 2 runs: the first is cold".into());
    }
    if !fast && (threads.is_some() || runtime.is_some() || !spin) {
        return Err("--threads, --runtime and --no-spin are only used with --fast".into());
    }
    let speed = match (fast, spin) {
        (false, _) => Speed::Exact,
        (true, true) => Speed::fast(runtime, threads.unwrap_or(FAST_THREADS))?,
        (true, false) => Speed::fast(runtime, threads.unwrap_or(FAST_THREADS))?.without_spinning(),
    };
    let cores = cores.unwrap_or(if fast { Cores::Fast } else { Cores::All });
    Ok(Bench {
        model,
        runs,
        speed,
        cores,
        priority,
    })
}

fn median_and_worst(values: &[u64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    json!({
        "median": ms(sorted[sorted.len() / 2]),
        "worst": ms(sorted[sorted.len() - 1]),
    })
}

/// Times the features and the network separately, each run, on the 9 s
/// reference signal (a full 8 s window, the longest request), under the
/// worker's own bounds.
pub fn bench(arguments: &[OsString]) -> ExitCode {
    let bench = match parse_bench(arguments) {
        Ok(bench) => bench,
        Err(reason) => return fail(reason),
    };
    // Before any thread starts, as the worker does (#1608).
    if let Err(reason) = crate::take_priority(bench.priority) {
        return fail(reason);
    }
    if let Err(reason) = bound_resources(&bench.speed) {
        return fail(reason);
    }
    // The fast model keeps itself on the fast cores while loading; an
    // explicit choice here overrides that before any thread starts.
    let pinned = match &bench.cores {
        Cores::All => Ok(None),
        Cores::Fast => keep_on_fast_cores(),
        Cores::List(list) => cores::keep_on(list).map(|()| Some(list.clone())),
    };
    if let Err(reason) = pinned {
        return fail(reason);
    }
    let started = Instant::now();
    let speed = match (&bench.speed, &bench.cores) {
        // `load` would pin to the fastest cores again; honour the choice.
        (
            Speed::Fast {
                runtime,
                threads,
                spin,
            },
            Cores::All | Cores::List(_),
        ) => match FastTurn::load_with(&bench.model, &SMART_TURN_V3_2, runtime, *threads, *spin) {
            Ok(model) => Ok(Box::new(model) as Box<dyn lamp_turn_end::Network>),
            Err(error) => Err(format!("fast model {}: {error}", bench.model.display())),
        },
        _ => load(&bench.model, &bench.speed),
    };
    let mut model = match speed {
        Ok(model) => model,
        Err(reason) => return fail(reason),
    };
    let load_ms = started.elapsed().as_secs_f64() * 1e3;
    let (signal, mut extractor) = (reference::voiced(), FeatureExtractor::default());
    let (mut features, mut network) = (Vec::new(), Vec::new());
    let cpu_started = cpu_us();
    let mut probability = 0.0;
    for run in 1..=bench.runs {
        match lamp_turn_end::classify(model.as_mut(), &mut extractor, &signal) {
            Ok(classified) => {
                probability = classified.probability;
                features.push(classified.features_us);
                network.push(classified.inference_us);
                println!(
                    "run {run} features_ms {:.1} network_ms {:.1}",
                    ms(classified.features_us),
                    ms(classified.inference_us)
                );
            }
            Err(reason) => return fail(reason),
        }
    }
    let cpu_per_run = (cpu_us() - cpu_started) / bench.runs as u64;
    let totals = features
        .iter()
        .zip(&network)
        .map(|(f, n)| f + n)
        .collect::<Vec<_>>();
    let [address_space, nice] = bounds();
    let [peak, hwm, rss] = memory_kib();
    let (threads, spin, runtime) = match &bench.speed {
        Speed::Exact => (1, Value::Null, json!("tract 0.23.8")),
        Speed::Fast { threads, spin, .. } => (
            *threads,
            json!(spin),
            json!(format!(
                "onnxruntime {}",
                fast::runtime_asset().map(|a| a.version).unwrap_or("?")
            )),
        ),
    };
    let summary = json!({
        "speed": bench.speed.name(),
        "runtime": runtime,
        "threads": threads,
        "spin": spin,
        "scheduling": serde_json::from_str::<Value>(&crate::scheduling()).unwrap_or(Value::Null),
        "cores": serde_json::from_str::<Value>(&json_cores(cores::allowed())).unwrap_or(Value::Null),
        "cpus": cpu_description(),
        "runs": bench.runs,
        "load_ms": (load_ms * 10.0).round() / 10.0,
        "cold_ms": ms(totals[0]),
        "warm": {
            "features_ms": median_and_worst(&features[1..]),
            "network_ms": median_and_worst(&network[1..]),
            "total_ms": median_and_worst(&totals[1..]),
        },
        "cpu_per_run_ms": ms(cpu_per_run),
        "probability": probability,
        "vm_peak_kib": peak,
        "vm_hwm_kib": hwm,
        "vm_rss_kib": rss,
        "address_space_limit_kib": serde_json::from_str::<Value>(&address_space).unwrap_or(Value::Null),
        "nice": serde_json::from_str::<Value>(&nice).unwrap_or(Value::Null),
    });
    println!("{summary}");
    ExitCode::SUCCESS
}

/// Each core's number and part from Linux `/proc/cpuinfo`, to tell a
/// Cortex-A76 (`0xd0b`) from a Cortex-A55 (`0xd05`).
fn cpu_description() -> Value {
    let info = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let mut parts = serde_json::Map::new();
    let mut processor = None;
    for line in info.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "processor" => processor = Some(value.trim().to_owned()),
            "CPU part" => {
                if let Some(processor) = processor.take() {
                    parts.insert(processor, json!(value.trim()));
                }
            }
            _ => {}
        }
    }
    Value::Object(parts)
}

const AGREE_USAGE: &str =
    "usage: lamp-turn-end agree PATH DIRECTORY... [--runtime LIBRARY] [--csv FILE]";

/// How each clip is put to the model: as recorded, and as the voice asks
/// 200 ms into a pause.
const PROTOCOLS: [(&str, usize); 2] = [("as_recorded", 0), ("pause_200_ms", 200)];

/// Classifies every clip of each directory with the exact and the fast
/// model and reports how far the verdicts agree, per directory and protocol
/// and in all.
pub fn agree(arguments: &[OsString]) -> ExitCode {
    let mut words = arguments.iter().map(|word| word.to_str());
    let Some(Some(model)) = words.next() else {
        return fail(AGREE_USAGE);
    };
    let (mut directories, mut runtime, mut csv) = (Vec::new(), None, None);
    while let Some(word) = words.next() {
        match word {
            Some("--runtime") if runtime.is_none() => {
                runtime = words.next().flatten().map(PathBuf::from)
            }
            Some("--csv") if csv.is_none() => csv = words.next().flatten().map(PathBuf::from),
            Some(directory) if !directory.starts_with('-') => {
                directories.push(PathBuf::from(directory))
            }
            _ => return fail(AGREE_USAGE),
        }
    }
    if directories.is_empty() {
        return fail(AGREE_USAGE);
    }
    match compare(Path::new(model), runtime, &directories, csv.as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(reason) => fail(reason),
    }
}

fn compare(
    model: &Path,
    runtime: Option<PathBuf>,
    directories: &[PathBuf],
    csv: Option<&Path>,
) -> Result<(), String> {
    let exact = SmartTurn::load(model, &SMART_TURN_V3_2).map_err(|e| e.to_string())?;
    let Speed::Fast {
        runtime, threads, ..
    } = Speed::fast(runtime, FAST_THREADS)?
    else {
        unreachable!()
    };
    let mut fast =
        FastTurn::load(model, &SMART_TURN_V3_2, &runtime, threads).map_err(|e| e.to_string())?;
    let mut table = match csv {
        Some(path) => {
            let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
            writeln!(
                file,
                "directory,id,dataset,language,finished,protocol,exact,fast"
            )
            .map_err(|e| e.to_string())?;
            Some(file)
        }
        None => None,
    };
    let mut extractor = FeatureExtractor::default();
    let mut total = PROTOCOLS.map(|_| clips::Agreement::default());
    for directory in directories {
        let mut here = PROTOCOLS.map(|_| clips::Agreement::default());
        for clip in clips::load(directory).map_err(|e| format!("{}: {e}", directory.display()))? {
            let pcm = clips::read(&clip).map_err(|e| e.to_string())?;
            for (index, (protocol, pause_ms)) in PROTOCOLS.iter().enumerate() {
                let features = extractor.compute(&clips::with_pause(&pcm, *pause_ms));
                let exact = exact
                    .probability(features.clone())
                    .map_err(|e| e.to_string())?;
                let fast = fast.probability(features).map_err(|e| e.to_string())?;
                here[index].add(exact, fast, clip.finished);
                total[index].add(exact, fast, clip.finished);
                if let Some(file) = table.as_mut() {
                    writeln!(
                        file,
                        "{},{},{},{},{},{protocol},{exact:.7},{fast:.7}",
                        directory.display(),
                        clip.id,
                        clip.dataset,
                        clip.language,
                        clip.finished
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        for (index, (protocol, _)) in PROTOCOLS.iter().enumerate() {
            println!(
                "{}",
                json!({"directory": directory.display().to_string(), "protocol": protocol, "agreement": here[index].summary()})
            );
        }
    }
    for (index, (protocol, _)) in PROTOCOLS.iter().enumerate() {
        println!(
            "{}",
            json!({"directory": "all", "protocol": protocol, "agreement": total[index].summary()})
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<OsString> {
        text.split_whitespace().map(OsString::from).collect()
    }

    #[test]
    fn bench_defaults_time_the_worker_as_it_is() {
        let bench = parse_bench(&words("m.onnx")).unwrap();
        assert_eq!(
            bench,
            Bench {
                model: "m.onnx".into(),
                runs: BENCH_RUNS,
                speed: Speed::Exact,
                cores: Cores::All,
                priority: None,
            }
        );
        let bench = parse_bench(&words("m.onnx 50 --cores fast")).unwrap();
        assert_eq!((bench.runs, bench.cores), (50, Cores::Fast));
        let bench = parse_bench(&words("m.onnx --cores 6,7")).unwrap();
        assert_eq!(bench.cores, Cores::List(vec![6, 7]));
    }

    #[test]
    fn bench_fast_defaults_to_two_threads_on_the_fast_cores() {
        let bench =
            parse_bench(&words("m.onnx --fast --runtime /x/lib.so --threads 1 30")).unwrap();
        assert_eq!(bench.runs, 30);
        assert_eq!(bench.cores, Cores::Fast);
        assert_eq!(
            bench.speed,
            Speed::Fast {
                runtime: "/x/lib.so".into(),
                threads: 1,
                spin: true,
            }
        );
        // #1608: `--no-spin` only with the fast model.
        let quiet = parse_bench(&words("m.onnx --fast --runtime /x/lib.so --no-spin")).unwrap();
        assert!(matches!(quiet.speed, Speed::Fast { spin: false, .. }));
        assert!(parse_bench(&words("m.onnx --no-spin")).is_err());
        // #1608: and below the voice's real-time priority, 1 to 99.
        let below = parse_bench(&words("m.onnx --realtime-priority 10")).unwrap();
        assert_eq!(below.priority, Some(10));
        assert!(parse_bench(&words("m.onnx --realtime-priority 0")).is_err());
        let bench = parse_bench(&words("m.onnx --fast --runtime /x/lib.so --cores all")).unwrap();
        assert_eq!(bench.cores, Cores::All);
        assert!(matches!(bench.speed, Speed::Fast { threads: 2, .. }));
    }

    #[test]
    fn bench_refuses_what_it_cannot_honour() {
        for wrong in [
            "",
            "m.onnx 1",
            "m.onnx --threads 2",
            "m.onnx --runtime /x/lib.so",
            "m.onnx --fast --runtime /x/lib.so --threads 0",
            "m.onnx --cores six",
            "m.onnx 5 6",
            "m.onnx --verbose",
        ] {
            assert!(parse_bench(&words(wrong)).is_err(), "{wrong}");
        }
    }

    #[test]
    fn cores_are_json() {
        assert_eq!(json_cores(None), "null");
        assert_eq!(json_cores(Some(vec![6, 7])), "[6,7]");
    }
}
