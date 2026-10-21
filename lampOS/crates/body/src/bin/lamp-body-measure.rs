//! The factory's per-lamp body step (#1682): one read-only inspection of the
//! lamp at rest, torque off, made into the body's own `lamp.env`
//! (`lamp_body::poses`). Never a motion controller: it writes no register.

use lamp_body::calibration::UnitId;
use std::{ffi::OsString, path::PathBuf};

const USAGE: &str = "lamp-body-measure --device ABSOLUTE_PATH --unit-id EXPLICIT_ID --lock-directory PRIVATE_ABSOLUTE_DIRECTORY [--write ABSOLUTE_FILE] [--seated]\n\nThe factory's per-lamp body step: reads the lamp at rest with torque off (lamp-body-inspect's\nread-only plan: no register writes, no torque, no motion) and prints this lamp's own\nlamp.env (BODY_POSES: --home, --folded-arm, --stand, --pitch-window, --torque). With --write\nit also writes that file, which must not exist yet (/etc/lampOS/body/lamp.env on the product).\nRefused, with why, where the body would refuse the poses. {home}--gaze is not measured here (body.md).\nWith --seated the file holds only --home, --pitch-window and --torque on: a lamp whose first stand\nno person has confirmed yet, whose body then holds and moves the head and base only, never the arm (#2163).\nStop the body and every other bus owner first. Linux only.\n\nlamp-body-measure first-stand --device ABSOLUTE_PATH --unit-id EXPLICIT_ID --lock-directory PRIVATE_ABSOLUTE_DIRECTORY --body-user USER [--bus-root DIR] [--presence-file FILE] [--say-socket FILE] [--lamp-env FILE] [--record FILE]\n\nA seated lamp's first stand, confirmed at the lamp (lamp_body::first_stand, #2396): once someone is near\nit asks through the voice's say service, and a touch on the head or a button held a second is the yes;\nthen the body is stopped, the arm stood at 30% and 100% as the factory's steps stand it, this arm's\nstand written into --lamp-env (/etc/lampOS/body/lamp.env), and the body started again. A second touch\nwhile it moves stops it, and nothing is written. Nothing moves without the yes. As root.\n";

/// [`USAGE`] with the home pitch clamp said from the code's own numbers
/// (`Limits::FOLDED.pitch_home`, `poses::HOME_PITCH_TOLERANCE`), never a
/// copy of them. Linux only, as the body and the read are.
#[cfg(target_os = "linux")]
fn usage() -> String {
    let (low, high) = lamp_body::body::Limits::FOLDED.pitch_home;
    let tolerance = lamp_body::poses::HOME_PITCH_TOLERANCE;
    USAGE.replacen(
        "{home}",
        &format!(
            "A home pitch\nwithin {tolerance} counts outside the body's home range ({low}..{high}) is the head on its stop: --home\ntakes the nearest edge and the file says so in a comment line; farther out it is refused, a head\noff its stop: fold it onto its stop and measure again. The clamp helps a body that wakes (lamp-body's\n--wake, on unless given off): its start check also reads the live pitch, and lets it through\nonly to wake it home. "
        ),
        1,
    )
}

#[cfg(not(target_os = "linux"))]
fn usage() -> String {
    USAGE.replacen("{home}", "", 1)
}

#[derive(Debug)]
struct Arguments {
    device: PathBuf,
    unit_id: UnitId,
    lock_directory: PathBuf,
    write: Option<PathBuf>,
    /// Only the head's poses: no stand, no folded arm (#2163).
    seated: bool,
}

fn bounded(path: &std::path::Path) -> bool {
    path.is_absolute()
        && path.as_os_str().len() <= 4096
        && path.to_str().is_some()
        && path.components().count() <= 64
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Arguments>, &'static str> {
    let mut args = args.into_iter();
    let (mut device, mut unit_id, mut lock_directory, mut write) = (None, None, None, None);
    let mut seated = false;
    while let Some(key) = args.next() {
        if key == "--seated" {
            if seated {
                return Err("unknown or duplicate option");
            }
            seated = true;
            continue;
        }
        if key == "--help" {
            if device.is_some()
                || unit_id.is_some()
                || lock_directory.is_some()
                || write.is_some()
                || seated
            {
                return Err("--help must be used alone");
            }
            return if args.next().is_some() {
                Err("--help must be used alone")
            } else {
                Ok(None)
            };
        }
        let value = args.next().ok_or("every option requires a value")?;
        match key.to_str() {
            Some("--device") if device.is_none() => device = Some(PathBuf::from(value)),
            Some("--lock-directory") if lock_directory.is_none() => {
                lock_directory = Some(PathBuf::from(value))
            }
            Some("--write") if write.is_none() => write = Some(PathBuf::from(value)),
            Some("--unit-id") if unit_id.is_none() => {
                unit_id = Some(
                    UnitId::new(value.to_str().ok_or("unit ID must be UTF-8")?)
                        .map_err(|_| "invalid explicit unit ID")?,
                );
            }
            _ => return Err("unknown or duplicate option"),
        }
    }
    let result = Arguments {
        device: device.ok_or("--device is required")?,
        unit_id: unit_id.ok_or("--unit-id is required")?,
        lock_directory: lock_directory.ok_or("--lock-directory is required")?,
        write,
        seated,
    };
    if ![&result.device, &result.lock_directory]
        .into_iter()
        .chain(result.write.as_ref())
        .all(|path| bounded(path))
    {
        return Err("paths must be bounded absolute UTF-8 paths without parent traversal");
    }
    Ok(Some(result))
}

/// The file, written once: never over one already there (another lamp's
/// poses, or this one's measured before), readable by the body's unit.
fn write_new(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// When it was measured, for the file's comment: the wall clock in UTC.
fn measured_at() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60
    )
}

/// `first-stand`'s options, with the product's paths where none is given.
fn first_stand_options(
    args: impl IntoIterator<Item = OsString>,
) -> Result<lamp_body::first_stand::Options, &'static str> {
    let mut args = args.into_iter();
    let mut values = std::collections::BTreeMap::new();
    while let Some(key) = args.next() {
        let key = key
            .into_string()
            .map_err(|_| "every option must be UTF-8")?;
        let value = args.next().ok_or("every option requires a value")?;
        let value = value
            .into_string()
            .map_err(|_| "every value must be UTF-8")?;
        if !matches!(
            key.as_str(),
            "--device"
                | "--unit-id"
                | "--lock-directory"
                | "--body-user"
                | "--bus-root"
                | "--presence-file"
                | "--say-socket"
                | "--lamp-env"
                | "--record"
        ) || values.insert(key, value).is_some()
        {
            return Err("unknown or duplicate option");
        }
    }
    let mut take = |key: &str, default: Option<&str>| {
        values
            .remove(key)
            .or_else(|| default.map(str::to_owned))
            .ok_or("--device, --unit-id, --lock-directory and --body-user are required")
    };
    let unit_id = take("--unit-id", None)?;
    UnitId::new(&unit_id).map_err(|_| "invalid explicit unit ID")?;
    let exe = std::env::current_exe().map_err(|_| "cannot find this program's own path")?;
    let options = lamp_body::first_stand::Options {
        unit_id,
        device: take("--device", None)?.into(),
        lock_directory: take("--lock-directory", None)?.into(),
        body_user: take("--body-user", None)?,
        bus_root: take("--bus-root", Some("/run/lampos-bus"))?.into(),
        presence_file: take("--presence-file", Some("/run/lampos-eyes/present"))?.into(),
        say_socket: take("--say-socket", Some("/run/lampos-say/say.sock"))?.into(),
        lamp_env: take("--lamp-env", Some("/etc/lampOS/body/lamp.env"))?.into(),
        record: take("--record", Some("/var/lib/lampos-body/first-stand.json"))?.into(),
        body: exe.with_file_name("lamp-body"),
        measure: exe,
    };
    if ![
        &options.device,
        &options.lock_directory,
        &options.bus_root,
        &options.presence_file,
        &options.say_socket,
        &options.lamp_env,
        &options.record,
    ]
    .into_iter()
    .all(|path| bounded(path))
    {
        return Err("paths must be bounded absolute UTF-8 paths without parent traversal");
    }
    Ok(options)
}

fn main() -> std::process::ExitCode {
    let mut args = std::env::args_os().skip(1).peekable();
    if args.peek().is_some_and(|first| first == "first-stand") {
        args.next();
        return match first_stand_options(args) {
            Err(error) => {
                eprintln!("{error}\n{}", usage());
                std::process::ExitCode::from(2)
            }
            Ok(options) => first_stand(&options),
        };
    }
    let arguments = match parse(args) {
        Ok(None) => {
            print!("{}", usage());
            return std::process::ExitCode::SUCCESS;
        }
        Ok(Some(arguments)) => arguments,
        Err(error) => {
            eprintln!("{error}\n{}", usage());
            return std::process::ExitCode::from(2);
        }
    };
    match run(arguments) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-body-measure: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
fn first_stand(options: &lamp_body::first_stand::Options) -> std::process::ExitCode {
    match lamp_body::first_stand::run(options) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-first-stand: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn first_stand(_: &lamp_body::first_stand::Options) -> std::process::ExitCode {
    eprintln!("lamp-body-measure: first-stand is Linux only");
    std::process::ExitCode::FAILURE
}

/// The poses from a rest read, printed, and written when asked.
#[cfg(target_os = "linux")]
fn finish(
    arguments: &Arguments,
    report: &lamp_body::inspection::InspectionReport,
) -> Result<(), Box<dyn std::error::Error>> {
    let read = lamp_body::poses::rest_read(report)?;
    let form = if arguments.seated {
        lamp_body::poses::seated_lamp_env
    } else {
        lamp_body::poses::lamp_env
    };
    let text = form(arguments.unit_id.as_str(), &read, &measured_at())?;
    print!("{text}");
    if let Some(path) = &arguments.write {
        write_new(path, &text).map_err(|error| format!("{}: {error}", path.display()))?;
        eprintln!("lamp-body-measure: wrote {}", path.display());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn run(arguments: Arguments) -> Result<(), Box<dyn std::error::Error>> {
    // The host parser is testable (the poses in the library's tests); no
    // device backend here.
    let _ = (
        arguments.unit_id,
        arguments.write,
        arguments.seated,
        write_new as fn(&std::path::Path, &str) -> std::io::Result<()>,
        measured_at as fn() -> String,
    );
    Err("the read-only rest read requires Linux; no device was opened".into())
}

#[cfg(target_os = "linux")]
fn run(arguments: Arguments) -> Result<(), Box<dyn std::error::Error>> {
    use lamp_body::linux::{InspectionConfig, inspect_once};
    let config = InspectionConfig::new(
        arguments.device.clone(),
        arguments.unit_id.clone(),
        arguments.lock_directory.clone(),
    )?;
    let result = inspect_once(&config, || Ok(false));
    let report = result
        .inspection
        .as_ref()
        .filter(|_| result.completed)
        .ok_or("the read-only inspection did not complete; nothing was written")?;
    finish(&arguments, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn it_needs_its_device_unit_and_lock_and_takes_nothing_that_moves() {
        let base = [
            "--device",
            "/dev/ttyACM0",
            "--unit-id",
            "lamp-cb94",
            "--lock-directory",
            "/home/orangepi/locks",
        ];
        assert!(parse(args(&base)).unwrap().unwrap().write.is_none());
        let mut writing = base.to_vec();
        writing.extend(["--write", "/etc/lampOS/body/lamp.env"]);
        assert_eq!(
            parse(args(&writing)).unwrap().unwrap().write.as_deref(),
            Some(std::path::Path::new("/etc/lampOS/body/lamp.env"))
        );
        for bad in [
            vec![],
            base[..4].to_vec(),
            [&base[..], &["--torque", "on"]].concat(),
            [&base[..], &["--write", "lamp.env"]].concat(),
            [&base[..], &["--write", "/etc/../lamp.env"]].concat(),
        ] {
            assert!(parse(args(&bad)).is_err(), "{bad:?}");
        }
        assert!(parse(args(&["--help"])).unwrap().is_none());
    }

    /// #2163: `--seated` takes no value, once, anywhere; the plain form stays the default.
    #[test]
    fn seated_is_a_flag_given_once() {
        let base = [
            "--device",
            "/dev/ttyACM0",
            "--unit-id",
            "lamp-a63e",
            "--lock-directory",
            "/var/lib/lampos-locks/motor",
        ];
        assert!(!parse(args(&base)).unwrap().unwrap().seated);
        let seated = parse(args(
            &[
                &["--seated"][..],
                &base[..],
                &["--write", "/var/lib/lampos-body/measured.env"],
            ]
            .concat(),
        ))
        .unwrap()
        .unwrap();
        assert!(seated.seated);
        assert_eq!(
            seated.write.as_deref(),
            Some(std::path::Path::new("/var/lib/lampos-body/measured.env"))
        );
        for bad in [
            [&base[..], &["--seated", "--seated"]].concat(),
            vec!["--seated", "--help"],
            vec!["--seated"],
        ] {
            assert!(parse(args(&bad)).is_err(), "{bad:?}");
        }
    }

    /// #2163: the generic package's oneshot reads seated, into its own
    /// state folder and never the confirmed file, as the parts' account, and
    /// stays active so the keeper does not wait on it; the generic body
    /// requires it and takes the confirmed poses over the seated ones. Read
    /// from the unit files themselves.
    #[test]
    fn the_generic_units_read_seated_once_and_the_confirmed_poses_win() {
        const MEASURE: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/units/lamp-body-measure.service"
        ));
        const BODY: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/units/lamp-body.service"
        ));
        let settings = |unit: &'static str, key: &str| -> Vec<&'static str> {
            unit.lines()
                .filter_map(|line| line.strip_prefix(key)?.strip_prefix('='))
                .collect()
        };
        assert_eq!(settings(MEASURE, "Type"), ["oneshot"]);
        assert_eq!(settings(MEASURE, "RemainAfterExit"), ["yes"]);
        assert_eq!(settings(MEASURE, "User"), ["@LAMP_USER@"]);
        assert_eq!(settings(MEASURE, "StateDirectory"), ["lampos-body"]);
        let exec = settings(MEASURE, "ExecStart")[0];
        // Nothing read where either file is there.
        for path in [
            "[ -e /etc/lampOS/body/lamp.env ]",
            "[ -e /var/lib/lampos-body/measured.env ]",
        ] {
            assert!(exec.contains(path), "{path}");
        }
        let command = exec
            .split_once("exec ")
            .and_then(|(_, rest)| rest.strip_suffix('\''))
            .expect("the read is exec'd last");
        let words: Vec<&str> = command
            .split_whitespace()
            .skip(1)
            .map(|word| match word {
                "@SERVO_DEVICE@" => "/dev/ttyACM0",
                "@UNIT_ID@" => "lamp-a63e",
                "@MOTOR_LOCK_DIR@" => "/var/lib/lampos-locks/motor",
                word => word,
            })
            .collect();
        let read = parse(args(&words)).unwrap().unwrap();
        assert!(read.seated);
        assert_eq!(
            read.write.as_deref(),
            Some(std::path::Path::new("/var/lib/lampos-body/measured.env"))
        );
        assert!(!MEASURE.contains("--write /etc/"));

        assert!(settings(BODY, "Requires").contains(&"lamp-body-measure.service"));
        assert!(
            settings(BODY, "After")
                .iter()
                .any(|after| after.contains("lamp-body-measure.service"))
        );
        // The later EnvironmentFile wins: the confirmed poses over the seated.
        assert_eq!(
            settings(BODY, "EnvironmentFile"),
            [
                "-/var/lib/lampos-body/measured.env",
                "-/etc/lampOS/body/lamp.env"
            ]
        );
        assert!(settings(BODY, "ExecStart")[0].ends_with(" $BODY_POSES"));
    }

    #[test]
    fn the_file_is_written_once_and_never_over_another() {
        let path = std::env::temp_dir().join(format!("lamp-env-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        write_new(&path, "BODY_POSES=\"--home 1,2,3\"\n").unwrap();
        assert!(write_new(&path, "BODY_POSES=\"--home 4,5,6\"\n").is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "BODY_POSES=\"--home 1,2,3\"\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn measured_at_is_a_utc_minute() {
        let at = measured_at();
        assert_eq!(at.len(), 17, "{at}");
        assert!(at.starts_with("20") && at.ends_with('Z') && at.as_bytes()[10] == b'T');
    }
}
