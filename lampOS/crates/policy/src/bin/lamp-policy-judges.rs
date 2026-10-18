//! A finite, explicitly enabled qualification of the two shadow judges.
//! The original lamp-policy binary and its defaults are unchanged.
use lamp_bus::Bus;
use lamp_policy::{
    decide::{Asked, DEFAULT_ENDPOINT, DEFAULT_MODEL, Decider, Endpoint, Failed, Jev, Question},
    gather::PAUSE,
    policy::{self, Note, judges, local::Weights},
    record::private_dir,
};
use std::{
    collections::BTreeSet,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const USAGE: &str = "usage: lamp-policy-judges --local-judge (--offline | --key-file ABSOLUTE_PATH) --log-dir ABSOLUTE_DIR [--weights ABSOLUTE_FILE] [--bus-root ABSOLUTE_DIR] [--deadline-ms 100] [--request-ms 3000] [--seconds 60] [--show-judges]";
const HELP: &str = "Always shadow; never commands an actuator. Use one Policy owner on the bus.\n\
--local-judge explicitly enables the new path; without it nothing starts.\n\
--offline uses a deliberately unavailable slow judge and contacts no service.\n\
--key-file uses an already provisioned Jev key; --endpoint and --model are optional.\n\
--log-dir holds heard text and model evidence in private JSONL files.\n\
--weights loads a fitted weights file; omitted means the authored, unfitted seed.\n\
--deadline-ms is the final-choice budget (1..1000); --request-ms is the separate\n\
slow request timeout (1..60000). --seconds runs for 1..600 seconds, then drains\n\
logs for at most 100 ms. A stalled append or slow request may remain unconfirmed.\n\
--show-judges prints seven numeric answers, choices and timing, never speech.\n\
Network mode sends heard text to the selected service. This command is for\n\
already authorized played scenes; see docs/policy.md for privacy and limits.";

#[derive(Debug, PartialEq)]
struct Arguments {
    offline: bool,
    key: Option<PathBuf>,
    log: PathBuf,
    weights: Option<PathBuf>,
    bus: Option<PathBuf>,
    endpoint: String,
    model: String,
    deadline: Duration,
    request: Duration,
    seconds: u64,
    show: bool,
}

fn absolute(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err("file and directory paths must be absolute".into())
    }
}

fn number(value: &str, maximum: u64) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|n| (1..=maximum).contains(n))
        .ok_or_else(|| format!("expected a whole number from 1 to {maximum}"))
}

fn parse(args: impl Iterator<Item = String>) -> Result<Option<Arguments>, String> {
    let mut args = args;
    let (mut enabled, mut offline, mut show) = (false, false, false);
    let (mut key, mut log, mut weights, mut bus) = (None, None, None, None);
    let (mut endpoint, mut model) = (DEFAULT_ENDPOINT.to_owned(), DEFAULT_MODEL.to_owned());
    let (mut deadline, mut request, mut seconds) = (100, 3000, 60);
    let mut seen = BTreeSet::new();
    while let Some(option) = args.next() {
        if option == "--help" || option == "-h" {
            return Ok(None);
        }
        if !seen.insert(option.clone()) {
            return Err("an option was given twice".into());
        }
        match option.as_str() {
            "--local-judge" => enabled = true,
            "--offline" => offline = true,
            "--show-judges" => show = true,
            "--key-file" | "--log-dir" | "--weights" | "--bus-root" | "--endpoint" | "--model"
            | "--deadline-ms" | "--request-ms" | "--seconds" => {
                let value = args.next().ok_or("an option needs a value")?;
                match option.as_str() {
                    "--key-file" => key = Some(absolute(&value)?),
                    "--log-dir" => log = Some(absolute(&value)?),
                    "--weights" => weights = Some(absolute(&value)?),
                    "--bus-root" => bus = Some(absolute(&value)?),
                    "--endpoint" => {
                        Endpoint::parse(&value)?;
                        endpoint = value;
                    }
                    "--model" => {
                        if value.is_empty()
                            || value.len() > 128
                            || value.chars().any(char::is_control)
                        {
                            return Err("model must be a nonempty name of at most 128 bytes".into());
                        }
                        model = value;
                    }
                    "--deadline-ms" => deadline = number(&value, 1000)?,
                    "--request-ms" => request = number(&value, 60000)?,
                    "--seconds" => seconds = number(&value, 600)?,
                    _ => unreachable!(),
                }
            }
            _ => return Err("unknown option; use --help".into()),
        }
    }
    if !enabled {
        return Err("--local-judge is required; the second judge is off by default".into());
    }
    if offline && (key.is_some() || seen.contains("--endpoint") || seen.contains("--model")) {
        return Err("--offline cannot be combined with service or key options".into());
    }
    if !offline && key.is_none() {
        return Err("choose --offline or supply --key-file".into());
    }
    Ok(Some(Arguments {
        offline,
        key,
        log: log.ok_or("--log-dir is required")?,
        weights,
        bus,
        endpoint,
        model,
        deadline: Duration::from_millis(deadline),
        request: Duration::from_millis(request),
        seconds,
        show,
    }))
}

struct Offline;
impl Decider for Offline {
    fn ask(&mut self, _: &str, _: &[(String, Question)], _: Duration) -> Result<Asked, Failed> {
        Err(Failed::Unreachable {
            during: "offline qualification",
            kind: io::ErrorKind::NotConnected,
        })
    }
    fn name(&self) -> &str {
        "offline-unavailable"
    }
}

fn log(note: impl std::fmt::Display) {
    let _ = writeln!(io::stderr(), "lamp-policy-judges: {note}");
}

fn run<D: Decider + Send + 'static>(
    args: &Arguments,
    decider: D,
    weights: Weights,
) -> io::Result<()> {
    let bus = match &args.bus {
        Some(root) => Bus::new(root)?,
        None => Bus::from_environment()?,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let timer_stop = Arc::clone(&stop);
    let seconds = args.seconds;
    let (cancel, cancelled) = mpsc::channel();
    let timer = thread::Builder::new()
        .name("judge-stop".into())
        .spawn(move || {
            if matches!(
                cancelled.recv_timeout(Duration::from_secs(seconds)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                timer_stop.store(true, Ordering::Relaxed);
            }
        })?;
    log(format_args!(
        "started shadow=true offline={} decision_ms={} request_ms={} seconds={}",
        args.offline,
        args.deadline.as_millis(),
        args.request.as_millis(),
        seconds
    ));
    let result = policy::run_with_local(
        &bus,
        decider,
        &policy::Options {
            log_dir: args.log.clone(),
            deadline: args.request,
            pause: PAUSE,
            act_attention: false,
            report_every: policy::REPORT_EVERY,
        },
        &stop,
        &mut |note| {
            if args.show || !matches!(note, Note::Judge(_)) {
                log(note);
            }
        },
        judges::Options {
            weights,
            deadline: args.deadline,
            ..judges::Options::default()
        },
    );
    // An early startup error cancels this timer instead of waiting a minute.
    let _ = cancel.send(());
    let _ = timer.join();
    result.map(|totals| {
        log(format_args!(
            "stopped decided={} unpublished={} final_log_queue_rejections={}",
            totals.decided, totals.unpublished, totals.unlogged
        ))
    })
}

fn start(args: &Arguments) -> io::Result<()> {
    let weights = match &args.weights {
        Some(path) => Weights::load(path)?,
        None => Weights::seed(),
    };
    private_dir(&args.log)?;
    if args.offline {
        run(args, Offline, weights)
    } else {
        let jev = Jev::new(
            &args.endpoint,
            &args.model,
            args.key.as_ref().expect("validated key path"),
        )?;
        run(args, jev, weights)
    }
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(None) => {
            println!("{USAGE}\n\n{HELP}");
            return ExitCode::SUCCESS;
        }
        Ok(Some(args)) => args,
        Err(error) => {
            log(format_args!("{error}\n{USAGE}"));
            return ExitCode::from(2);
        }
    };
    match start(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            log(error);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn of(args: &str) -> Result<Option<Arguments>, String> {
        parse(args.split_whitespace().map(str::to_owned))
    }
    #[test]
    fn explicit_switch_and_mode_are_required() {
        assert!(
            of("--offline --log-dir /tmp/private")
                .unwrap_err()
                .contains("off by default")
        );
        assert!(of("--local-judge --log-dir /tmp/private").is_err());
        let args = of("--local-judge --offline --log-dir /tmp/private")
            .unwrap()
            .unwrap();
        assert!(args.offline);
        assert_eq!(args.deadline, Duration::from_millis(100));
        assert_eq!(args.request, Duration::from_millis(3000));
        assert_eq!(args.seconds, 60);
        assert_eq!(args.weights, None);
        assert!(!args.show);
        assert!(of("--help").unwrap().is_none());
    }
    #[test]
    fn reject_ambiguous_unbounded_and_relative_options() {
        let base = "--local-judge --offline --log-dir /tmp/private";
        for extra in [
            "--local-judge",
            "--offline",
            "--key-file /key",
            "--endpoint http://localhost/",
            "--model fake",
            "--seconds 0",
            "--seconds 601",
            "--deadline-ms 0",
            "--deadline-ms 1001",
            "--request-ms 60001",
            "--weights relative",
            "--bus-root relative",
            "--seconds",
            "--active",
        ] {
            assert!(of(&format!("{base} {extra}")).is_err(), "{extra}");
        }
        assert!(of("--local-judge --offline --log-dir relative").is_err());
        assert!(
            of("--local-judge --key-file /key --log-dir /logs --endpoint http://remote.invalid/")
                .is_err()
        );
    }
    #[test]
    fn service_options_and_numeric_boundaries_are_preserved() {
        let args = of("--local-judge --key-file /key --log-dir /logs --weights /weights --bus-root /bus --endpoint http://127.0.0.1:8080/decisions --model stand-in --show-judges --deadline-ms 1 --request-ms 60000 --seconds 600").unwrap().unwrap();
        assert!(!args.offline);
        assert!(args.show);
        assert_eq!(args.deadline, Duration::from_millis(1));
        assert_eq!(args.request, Duration::from_secs(60));
        assert_eq!(args.seconds, 600);
        assert_eq!(args.weights, Some("/weights".into()));
    }
}
