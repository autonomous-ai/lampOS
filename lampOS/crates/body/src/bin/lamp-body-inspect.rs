//! Finite read-only diagnostic, never a motion controller or daemon.

use lamp_body::calibration::UnitId;
use std::{ffi::OsString, path::PathBuf};

const USAGE: &str = "lamp-body-inspect --device ABSOLUTE_PATH --unit-id EXPLICIT_ID --lock-directory PRIVATE_ABSOLUTE_DIRECTORY [--status-faults abort|report]\n\nRead-only Linux inspection: five pings and five five-servo register reads.\nNo register writes or torque/motion commands. Stop all foreign bus owners first.\nA reply with a status fault flag ends the plan (abort, the default) or is recorded with its flags while the plan goes on (report).\nThe lock directory must already exist as 0700 under trusted non-writable ancestors.\nOpening an unqualified serial adapter may change modem lines. Hardware review is required.\n";

#[derive(Debug)]
struct Arguments {
    device: PathBuf,
    unit_id: UnitId,
    lock_directory: PathBuf,
    report_status_faults: bool,
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Arguments>, &'static str> {
    let mut args = args.into_iter();
    let mut device = None;
    let mut unit_id = None;
    let mut lock_directory = None;
    let mut status_faults = None;
    while let Some(key) = args.next() {
        if key == "--help" {
            if device.is_some()
                || unit_id.is_some()
                || lock_directory.is_some()
                || status_faults.is_some()
                || args.next().is_some()
            {
                return Err("--help must be used alone");
            }
            return Ok(None);
        }
        let value = args.next().ok_or("every option requires a value")?;
        match key.to_str() {
            Some("--device") if device.is_none() => device = Some(PathBuf::from(value)),
            Some("--lock-directory") if lock_directory.is_none() => {
                lock_directory = Some(PathBuf::from(value))
            }
            Some("--unit-id") if unit_id.is_none() => {
                unit_id = Some(
                    UnitId::new(value.to_str().ok_or("unit ID must be UTF-8")?)
                        .map_err(|_| "invalid explicit unit ID")?,
                );
            }
            Some("--status-faults") if status_faults.is_none() => {
                status_faults = Some(match value.to_str() {
                    Some("abort") => false,
                    Some("report") => true,
                    _ => return Err("--status-faults must be abort or report"),
                });
            }
            _ => return Err("unknown or duplicate option"),
        }
    }
    let result = Arguments {
        device: device.ok_or("--device is required")?,
        unit_id: unit_id.ok_or("--unit-id is required")?,
        lock_directory: lock_directory.ok_or("--lock-directory is required")?,
        report_status_faults: status_faults.unwrap_or(false),
    };
    for path in [&result.device, &result.lock_directory] {
        if !path.is_absolute()
            || path.as_os_str().len() > 4096
            || path.to_str().is_none()
            || path.components().count() > 64
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(
                "device and lock directory must be bounded absolute UTF-8 paths without parent traversal",
            );
        }
    }
    Ok(Some(result))
}

fn main() -> std::process::ExitCode {
    let arguments = match parse(std::env::args_os().skip(1)) {
        Ok(None) => {
            print!("{USAGE}");
            return std::process::ExitCode::SUCCESS;
        }
        Ok(Some(arguments)) => arguments,
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            return std::process::ExitCode::from(2);
        }
    };
    match run(arguments) {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(error) => {
            eprintln!("motor inspection failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn run(arguments: Arguments) -> Result<bool, Box<dyn std::error::Error>> {
    // The host parser is testable; no device backend is compiled on this host.
    let _ = (arguments.unit_id, arguments.report_status_faults);
    Err("read-only motor inspection requires a unix system; no device was opened".into())
}

#[cfg(unix)]
fn run(arguments: Arguments) -> Result<bool, Box<dyn std::error::Error>> {
    use lamp_body::linux::{InspectionConfig, inspect_once, inspect_once_reporting_faults};
    use lamp_body::stop_signal::StopSignal;
    use std::io::Write;

    // This finite CLI is single-threaded. Mask restoration follows device close.
    let config = InspectionConfig::new(
        arguments.device,
        arguments.unit_id,
        arguments.lock_directory,
    )?;
    let signal = StopSignal::new()?;
    let mut cancelled = false;
    let poll = || {
        if !cancelled {
            cancelled = signal.requested()?;
        }
        Ok(cancelled)
    };
    let result = if arguments.report_status_faults {
        inspect_once_reporting_faults(&config, poll)
    } else {
        inspect_once(&config, poll)
    };
    // Print only the bounded inspection report, after the serial descriptor is closed.
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, &result)?;
    writeln!(output)?;
    output.flush()?;
    signal.restore()?;
    // A redundant set on Drop is safe; it owns no hardware and performs no wait.
    Ok(result.completed)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_implicit_device_or_unit_and_no_unknown_controls() {
        for values in [
            vec![],
            vec!["--device", "/dev/example"],
            vec![
                "--device",
                "/dev/example",
                "--unit-id",
                "lamp",
                "--lock-directory",
                "/home/test/private",
                "--torque",
                "on",
            ],
            vec![
                "--device",
                "/dev/a",
                "--device",
                "/dev/b",
                "--unit-id",
                "lamp",
                "--lock-directory",
                "/home/test/private",
            ],
        ] {
            assert!(parse(args(&values)).is_err());
        }
    }

    #[test]
    fn explicit_bounded_arguments_and_help_are_valid_without_any_open() {
        let value = parse(args(&[
            "--device",
            "/dev/example",
            "--unit-id",
            "lamp-test",
            "--lock-directory",
            "/home/test/private",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(value.unit_id.as_str(), "lamp-test");
        assert_eq!(value.device, PathBuf::from("/dev/example"));
        assert!(!value.report_status_faults);
        let fixed = [
            "--device",
            "/dev/example",
            "--unit-id",
            "lamp-test",
            "--lock-directory",
            "/home/test/private",
            "--status-faults",
        ];
        let with = |choice: &'static str| parse(args(&[&fixed[..], &[choice]].concat()));
        assert!(with("report").unwrap().unwrap().report_status_faults);
        assert!(!with("abort").unwrap().unwrap().report_status_faults);
        assert!(with("ignore").is_err());
        assert!(
            parse(args(
                &[&fixed[..], &["report", "--status-faults", "report"]].concat()
            ))
            .is_err()
        );
        assert!(parse(args(&["--help"])).unwrap().is_none());
        assert!(parse(args(&["--help", "--device", "/dev/a"])).is_err());
        assert!(
            parse(args(&[
                "--device",
                "relative",
                "--unit-id",
                "lamp-test",
                "--lock-directory",
                "/home/test/private"
            ]))
            .is_err()
        );
    }
}
