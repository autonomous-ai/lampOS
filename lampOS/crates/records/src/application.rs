use lamp_records::{
    REPORT_SCHEMA_VERSION, RunReport,
    config::{Config, absolute_path, invalid},
    json_file::{self, CONFIG_BYTES, ReportRead},
    report::{RefusedInARow, Reporter, diagnostic_after},
    run,
};
use std::{
    env, io,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const USAGE: &str = "lamp-records defaults | check CONFIG [--apply] [--report RESULT] | report CONFIG RESULT [--seconds N]\ncheck is a dry run unless --apply is present; apply also requires enabled=true in CONFIG.\nDry runs write only stdout. The independent report process publishes the last result every second.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Defaults,
    Check {
        config: PathBuf,
        apply: bool,
        report: Option<PathBuf>,
    },
    Report {
        config: PathBuf,
        result: PathBuf,
        seconds: Option<u64>,
    },
}

fn arguments(args: &[String]) -> io::Result<Command> {
    match args {
        [one] if one == "defaults" => Ok(Command::Defaults),
        [one, config, rest @ ..] if one == "check" => {
            let mut apply = false;
            let mut report = None;
            let mut options = rest.iter();
            while let Some(option) = options.next() {
                match option.as_str() {
                    "--apply" if !apply => apply = true,
                    "--report" if report.is_none() => {
                        report = Some(PathBuf::from(
                            options
                                .next()
                                .ok_or_else(|| invalid("--report needs a path"))?,
                        ));
                    }
                    _ => return Err(invalid(USAGE)),
                }
            }
            if report.is_some() && !apply {
                return Err(invalid("a dry run cannot write a report file"));
            }
            absolute_path(&PathBuf::from(config))?;
            if let Some(path) = &report {
                absolute_path(path)?;
            }
            Ok(Command::Check {
                config: config.into(),
                apply,
                report,
            })
        }
        [one, config, result, rest @ ..] if one == "report" => {
            let seconds = match rest {
                [] => None,
                [flag, value] if flag == "--seconds" => {
                    let count = value
                        .parse::<u64>()
                        .map_err(|_| invalid("seconds must be an integer"))?;
                    if !(1..=86_400).contains(&count) {
                        return Err(invalid("seconds must be 1..86400"));
                    }
                    Some(count)
                }
                _ => return Err(invalid(USAGE)),
            };
            absolute_path(&PathBuf::from(config))?;
            absolute_path(&PathBuf::from(result))?;
            Ok(Command::Report {
                config: config.into(),
                result: result.into(),
                seconds,
            })
        }
        _ => Err(invalid(USAGE)),
    }
}

fn now() -> io::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs())
}

fn print(value: &impl serde::Serialize) -> io::Result<()> {
    use io::Write;
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, value).map_err(io::Error::other)?;
    writeln!(stdout)?;
    Ok(())
}

fn synthetic(config: &Config, at_unix_s: u64, error: Option<String>) -> RunReport {
    RunReport {
        schema_version: REPORT_SCHEMA_VERSION,
        config_fingerprint: config.fingerprint(),
        at_unix_s,
        mode: "apply".into(),
        enabled: config.enabled,
        roots: vec![],
        error,
    }
}

fn persist(config: &Config, path: &Path, result: &mut RunReport) {
    if let Err(error) = json_file::write_report(path, result) {
        // An oversized/invalid result must not poison the next timer run.
        // Keep detailed outcomes on stdout and try a small, valid failure
        // receipt for the live reporter.
        result.error = Some(format!("cannot persist full cleanup result: {error}"));
        let failure = synthetic(
            config,
            result.at_unix_s,
            Some("cannot persist full cleanup result; see cleanup stdout".into()),
        );
        if json_file::write_report(path, &failure).is_err() {
            eprintln!("lamp-records: cannot persist even the failure receipt");
        }
    }
}

/// Completed work immediately before the original reporting wait. The result
/// covers publisher construction and publication together: an error does not
/// claim that publish was called, and success does not prove delivery or exit.
#[derive(Debug)]
pub struct ReportCycle {
    pub started: Instant,
    pub host_started: Instant,
    pub host_seconds: Option<u64>,
    pub construction_or_publish: io::Result<()>,
}

/// Explicit cadence for the synchronous report loop. Reads must be pure and
/// nondecreasing; only the caller may advance a fixture's clock. All other
/// clocks, including the independent host lifetime, remain unchanged.
pub trait ReportSchedule {
    fn now(&self) -> Instant;

    /// Called after the actual work and host lifetime check, with the original
    /// one-second residual. Zero is not a positive parked boundary. A fixture
    /// must bound its own wait by the supplied real host lifetime as well.
    fn wait(&mut self, cycle: ReportCycle, remaining: Duration) -> io::Result<()>;
}

fn reporting_with(
    config_path: PathBuf,
    result: PathBuf,
    seconds: Option<u64>,
    schedule: &mut impl ReportSchedule,
    mut make_reporter: impl FnMut(&Path) -> io::Result<Reporter>,
) -> io::Result<()> {
    let mut config: Config = json_file::read(&config_path, CONFIG_BYTES)?;
    config.validate()?;
    let mut config_stamp = None;
    let mut result_stamp = None;
    let mut cached: Option<RunReport> = None;
    let mut publisher: Option<Reporter> = None;
    let mut refusals = RefusedInARow::default();
    let began = Instant::now();
    let mut last_error = None;
    loop {
        let step = schedule.now();
        let at = now()?;
        let config_result =
            json_file::read_changed::<Config>(&config_path, CONFIG_BYTES, config_stamp.as_ref());
        let config_error = match config_result {
            Ok(Some((next, stamp))) => match next.validate() {
                Ok(()) => {
                    if next.bus_root != config.bus_root {
                        publisher = None;
                    }
                    config = next;
                    config_stamp = Some(stamp);
                    None
                }
                Err(_) => Some("records configuration is invalid".to_owned()),
            },
            Ok(None) => None,
            Err(_) => Some("records configuration cannot be read".to_owned()),
        };
        let fallback;
        // Cleanups in a row refused for an unknown voice worker, counted
        // from the results themselves; a made-up status counts none (#1071).
        let mut in_a_row = 0;
        let status = if let Some(error) = config_error {
            fallback = synthetic(&config, at, Some(error));
            &fallback
        } else if !config.enabled {
            fallback = synthetic(&config, at, None);
            &fallback
        } else {
            match json_file::read_report_changed(&result, result_stamp.as_ref(), at) {
                Ok(Some((ReportRead::Current(report), stamp))) => {
                    cached = Some(report);
                    result_stamp = Some(stamp);
                }
                Ok(None) => {}
                Ok(Some((ReportRead::SetAside, _))) | Err(_) => {
                    cached = None;
                    result_stamp = None;
                }
            }
            if let Some(report) = cached
                .as_ref()
                .filter(|report| report.config_fingerprint == config.fingerprint())
            {
                in_a_row = refusals.observe(report);
                report
            } else {
                fallback = synthetic(
                    &config,
                    at,
                    Some("no readable cleanup result for the current configuration".into()),
                );
                &fallback
            }
        };
        // A file/transport failure never controls or restarts any producer.
        let published = (|| -> io::Result<()> {
            if publisher.is_none() {
                publisher = Some(make_reporter(&config.bus_root)?);
            }
            publisher
                .as_mut()
                .unwrap()
                .publish(&diagnostic_after(Some(status), at, in_a_row))
        })();
        if published.is_err() {
            publisher = None;
            if last_error.is_none_or(|time: Instant| time.elapsed() >= Duration::from_secs(60)) {
                eprintln!(
                    "lamp-records: diagnostics unavailable; will retry, cleanup is independent"
                );
                last_error = Some(Instant::now());
            }
        }
        if seconds.is_some_and(|limit| began.elapsed() >= Duration::from_secs(limit)) {
            break;
        }
        let remaining =
            Duration::from_secs(1).saturating_sub(schedule.now().saturating_duration_since(step));
        schedule.wait(
            ReportCycle {
                started: step,
                host_started: began,
                host_seconds: seconds,
                construction_or_publish: published,
            },
            remaining,
        )?;
    }
    Ok(())
}

fn command_with_reporting(
    command: Command,
    reporting: impl FnOnce(PathBuf, PathBuf, Option<u64>) -> io::Result<()>,
) -> io::Result<u8> {
    match command {
        Command::Defaults => {
            print(&Config::customer_default())?;
            Ok(0)
        }
        Command::Check {
            config: config_path,
            apply,
            report,
        } => {
            let config: Config = json_file::read(&config_path, CONFIG_BYTES)?;
            config.validate()?;
            if let Some(path) = &report {
                let temporary = json_file::temporary_path(path)?;
                if path == &config_path
                    || temporary == config_path
                    || json_file::same_file(&config_path, path)?
                    || json_file::same_file(&config_path, &temporary)?
                {
                    return Err(invalid("report file must not replace the configuration"));
                }
                // Result output is explicitly separate from every retention
                // root and the live bus. Never replace records with a receipt.
                if config.roots.iter().any(|root| path.starts_with(&root.path))
                    || path.starts_with(&config.bus_root)
                {
                    return Err(invalid(
                        "report file must be outside record roots and the bus",
                    ));
                }
                // Retain a newer schema whole. Same-schema unknown values,
                // corruption and unrelated files still require review before
                // any cleanup runs.
                match json_file::read_report_changed(path, None, now()?) {
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(_) => {
                        return Err(invalid(
                            "existing report file is not a recognized cleanup result",
                        ));
                    }
                }
            }
            let mut result = run::execute(&config, now()?, apply);
            if let Some(path) = report {
                persist(&config, &path, &mut result);
            }
            print(&result)?;
            Ok(run::exit_code(&result))
        }
        Command::Report {
            config,
            result,
            seconds,
        } => {
            reporting(config, result, seconds)?;
            Ok(0)
        }
    }
}

fn entrypoint(run_command: impl FnOnce(Command) -> io::Result<u8>) -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    if args == ["--help"] || args == ["-h"] {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match arguments(&args).and_then(run_command) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("lamp-records: {error}");
            ExitCode::from(64)
        }
    }
}
