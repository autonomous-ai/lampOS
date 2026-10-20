//! The lamp's health as a program: `lamp-health serve` puts every part's
//! report together and publishes how the lamp is, and `lamp-health` shows
//! it to a person.
//!
//! `serve` runs until it is killed. The standard library cannot catch a
//! signal and this crate takes in nothing that can, so SIGTERM ends it
//! where it stands. Nothing is half done by that: each value on the bus is
//! put in place whole. Its socket is left for the next report to remove,
//! it stops being listed as a publisher when the bus's lease runs out, and
//! the two values it kept mean nothing fifteen seconds later.
//!
//! What `serve` prints is about itself and the lamp's level
//! (`lamp_health::serve::Note`): never a line for each message.

use lamp_bus::Bus;
use lamp_health::{
    board::{ClockSource, Places},
    parts,
    serve::{self, DEFAULT_STALE_AFTER, Options, Times},
    show,
};
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    sync::atomic::AtomicBool,
    time::Duration,
};

const USAGE: &str = "usage: lamp-health serve [--bus-root ABSOLUTE_DIR] [--expect NAME]... [--stale-ms N] [--judge-zone BEGINNING]... [--proc DIR] [--sys DIR] [--run DIR] [--disk PATH] [--clock timesync|kernel] [--online on|off] [--reach-probe on|off] [--reach-idle on|off] [--needs NAME=PATH]...\n       lamp-health [show] [--bus-root ABSOLUTE_DIR] [--json]\n       lamp-health --help";
const HELP: &str = "How the lamp is. See docs/health.md.

serve: listens to every part's report on the bus (diagnostics), reads the board, and
publishes how the whole lamp is (lamp/health) and the board's numbers (board/state).
  --bus-root  the bus's directory (default LAMP_BUS_ROOT, or /run/lampos-bus)
  --expect    a part that should be reporting; may be given more than once. One that
              never reports is listed as missing
  --needs     NAME=PATH: the part NAME runs only where the file PATH is, so while it is
              not there the part is listed as off, never missing (tasks needs Harness)
  --stale-ms  after how long without a report a part is stale, from 100 to 600000
              (default 5000)
  --proc      where the system's numbers are read from (default /proc)
  --sys       where the temperatures are read from (default /sys)
  --run       where the clock's state is read from (default /run)
  --disk      a path on the disk whose free room is watched (default /)
A wrong option prints the usage and ends with status 2.

show, or no command: prints how the lamp is, from what serve last published.
  --bus-root  as above
  --json      the two values as one JSON object, {\"health\":...,\"board\":...}, with null
              for one that is not there
It ends with status 0 if the lamp is ok, 1 for a warning, 2 for an error, and 3 if a part
is stale or missing or nobody is reporting the lamp's health at all. A wrong option ends
with status 64, so that it cannot be mistaken for a level.";

const SERVE_USAGE_STATUS: u8 = 2;

#[derive(Debug, Eq, PartialEq)]
struct ServeArguments {
    bus: Option<PathBuf>,
    expected: Vec<String>,
    stale_after: Duration,
    places: Places,
    /// Publish `lamp/online` too.
    online: bool,
    /// While the voice's link is down, ask the internet itself (#830);
    /// only with `--online on`.
    reach_probe: bool,
    reach_idle: bool,
    /// Parts off while the file each needs is not on the lamp.
    needs: Vec<(String, PathBuf)>,
}

#[derive(Debug, Eq, PartialEq)]
struct ShowArguments {
    bus: Option<PathBuf>,
    json: bool,
}

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Help,
    Serve(ServeArguments),
    Show(ShowArguments),
}

/// Why the arguments were refused, and the status to end with: 2 for
/// `serve`, as the lamp's other programs, and 64 for `show`, whose 2 means
/// that the lamp has an error.
#[derive(Debug, Eq, PartialEq)]
struct Refused {
    why: String,
    status: u8,
}

fn absolute(option: &str, value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    path.is_absolute()
        .then_some(path)
        .ok_or_else(|| format!("{option} must be an absolute path"))
}

fn parse_serve(mut arguments: impl Iterator<Item = String>) -> Result<Command, String> {
    let (mut bus, mut stale_after) = (None, None);
    let (mut proc, mut sys, mut run, mut disk) = (None, None, None, None);
    let mut expected: Vec<String> = Vec::new();
    let mut needs: Vec<(String, PathBuf)> = Vec::new();
    let mut judged: Vec<String> = Vec::new();
    let mut clock = None;
    let mut online = None;
    let mut reach_probe = None;
    let mut reach_idle = None;
    while let Some(option) = arguments.next() {
        if option == "--help" || option == "-h" {
            return Ok(Command::Help);
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("{option} needs a value"))?;
        let twice = match option.as_str() {
            "--bus-root" => bus.replace(absolute(&option, &value)?).is_some(),
            "--proc" => proc.replace(absolute(&option, &value)?).is_some(),
            "--sys" => sys.replace(absolute(&option, &value)?).is_some(),
            "--run" => run.replace(absolute(&option, &value)?).is_some(),
            "--disk" => disk.replace(absolute(&option, &value)?).is_some(),
            "--judge-zone" => {
                if value.is_empty() || value.len() > 48 || value.chars().any(char::is_control) {
                    return Err(
                        "--judge-zone is how a zone's name begins: one line, of 48 bytes at most"
                            .into(),
                    );
                }
                if judged.len() == 16 {
                    return Err("--judge-zone was given more than 16 times".into());
                }
                judged.push(value);
                false
            }
            "--expect" => {
                if !parts::is_name(&value) {
                    return Err(format!(
                        "--expect is a part's name: something on one line, of {} bytes at most",
                        parts::MOST_NAME_BYTES
                    ));
                }
                if expected.len() == parts::MOST_PARTS {
                    return Err(format!(
                        "--expect was given more than {} times",
                        parts::MOST_PARTS
                    ));
                }
                if expected.contains(&value) {
                    return Err(format!("--expect {value} was given twice"));
                }
                expected.push(value);
                false
            }
            "--needs" => {
                let (name, path) = value
                    .split_once('=')
                    .filter(|(name, path)| parts::is_name(name) && path.starts_with('/'))
                    .ok_or("--needs is NAME=PATH: a part's name and an absolute file")?;
                if needs.len() == parts::MOST_PARTS {
                    return Err(format!(
                        "--needs was given more than {} times",
                        parts::MOST_PARTS
                    ));
                }
                if needs.iter().any(|(given, _)| given == name) {
                    return Err(format!("--needs {name} was given twice"));
                }
                needs.push((name.to_owned(), PathBuf::from(path)));
                false
            }
            "--clock" => clock
                .replace(
                    value
                        .parse::<ClockSource>()
                        .map_err(|()| "--clock is timesync or kernel")?,
                )
                .is_some(),
            "--online" => online
                .replace(match value.as_str() {
                    "on" => true,
                    "off" => false,
                    _ => return Err("--online is on or off".into()),
                })
                .is_some(),
            "--reach-probe" => reach_probe
                .replace(match value.as_str() {
                    "on" => true,
                    "off" => false,
                    _ => return Err("--reach-probe is on or off".into()),
                })
                .is_some(),
            "--reach-idle" => reach_idle
                .replace(match value.as_str() {
                    "on" => true,
                    "off" => false,
                    _ => return Err("--reach-idle is on or off".into()),
                })
                .is_some(),
            "--stale-ms" => {
                let milliseconds = value
                    .parse::<u64>()
                    .ok()
                    .filter(|milliseconds| (100..=600_000).contains(milliseconds))
                    .ok_or("--stale-ms is a whole number from 100 to 600000")?;
                stale_after
                    .replace(Duration::from_millis(milliseconds))
                    .is_some()
            }
            other => return Err(format!("unknown option {other}")),
        };
        if twice {
            return Err(format!("{option} was given twice"));
        }
    }
    // The probe is for `lamp/online` and nothing else.
    if reach_probe == Some(true) && online != Some(true) {
        return Err("--reach-probe on needs --online on".into());
    }
    if reach_idle == Some(true) && reach_probe != Some(true) {
        return Err("--reach-idle on needs --reach-probe on".into());
    }
    let places = Places::default();
    Ok(Command::Serve(ServeArguments {
        bus,
        expected,
        stale_after: stale_after.unwrap_or(DEFAULT_STALE_AFTER),
        places: Places {
            proc: proc.unwrap_or(places.proc),
            sys: sys.unwrap_or(places.sys),
            run: run.unwrap_or(places.run),
            disk: disk.unwrap_or(places.disk),
            judged,
            clock: clock.unwrap_or(places.clock),
        },
        online: online.unwrap_or(false),
        reach_probe: reach_probe.unwrap_or(false),
        reach_idle: reach_idle.unwrap_or(false),
        needs,
    }))
}

fn parse_show(mut arguments: impl Iterator<Item = String>) -> Result<Command, String> {
    let (mut bus, mut json) = (None, false);
    while let Some(option) = arguments.next() {
        let twice = match option.as_str() {
            "--help" | "-h" => return Ok(Command::Help),
            "--json" => std::mem::replace(&mut json, true),
            "--bus-root" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| format!("{option} needs a value"))?;
                bus.replace(absolute(&option, &value)?).is_some()
            }
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other => return Err(format!("unknown command {other}")),
        };
        if twice {
            return Err(format!("{option} was given twice"));
        }
    }
    Ok(Command::Show(ShowArguments { bus, json }))
}

fn parse(arguments: impl Iterator<Item = String>) -> Result<Command, Refused> {
    let mut arguments = arguments.peekable();
    let refused = |status| move |why| Refused { why, status };
    match arguments.peek().map(String::as_str) {
        Some("serve") => {
            arguments.next();
            parse_serve(arguments).map_err(refused(SERVE_USAGE_STATUS))
        }
        Some("show") => {
            arguments.next();
            parse_show(arguments).map_err(refused(show::STATUS_USAGE))
        }
        // No command is `show`, and whatever else comes first is a
        // mistake made in asking how the lamp is.
        _ => parse_show(arguments).map_err(refused(show::STATUS_USAGE)),
    }
}

fn bus(root: Option<&PathBuf>) -> io::Result<Bus> {
    match root {
        Some(root) => Bus::new(root),
        None => Bus::from_environment(),
    }
}

/// One line to the journal. A journal that has gone away is no reason to
/// stop.
fn log(line: &str) {
    let _ = writeln!(io::stderr(), "lamp-health: {line}");
}

fn serve(arguments: &ServeArguments) -> io::Result<()> {
    let bus = bus(arguments.bus.as_ref())?;
    let places = &arguments.places;
    log(&format!(
        "started bus_root={} expect={} stale_ms={} proc={} sys={} run={} disk={}",
        bus.root().display(),
        if arguments.expected.is_empty() {
            "none".to_owned()
        } else {
            arguments.expected.join(",")
        },
        arguments.stale_after.as_millis(),
        places.proc.display(),
        places.sys.display(),
        places.run.display(),
        places.disk.display(),
    ));
    let options = Options {
        expected: arguments.expected.clone(),
        stale_after: arguments.stale_after,
        times: Times::with_stale_after(arguments.stale_after),
        online: arguments.online,
        reach: arguments.reach_probe.then(lamp_health::reach::targets),
        reach_idle: arguments.reach_idle,
        needs: arguments.needs.clone(),
    };
    // Nothing sets the flag: the process runs until it is killed.
    let stop = AtomicBool::new(false);
    serve::run(
        &bus,
        &options,
        &mut serve::board_reader(places.clone()),
        &stop,
        &mut |note| log(&note.to_string()),
    )
    .map(|_| ())
}

fn show(arguments: &ShowArguments) -> ExitCode {
    let bus = match bus(arguments.bus.as_ref()) {
        Ok(bus) => bus,
        // Only a `LAMP_BUS_ROOT` that is no absolute path: the caller's
        // mistake, like a wrong option.
        Err(error) => {
            eprintln!("lamp-health: {error}\n{USAGE}");
            return ExitCode::from(show::STATUS_USAGE);
        }
    };
    let look = show::look(&bus);
    for trouble in &look.troubles {
        eprintln!("lamp-health: {trouble}");
    }
    let printed = if arguments.json {
        show::json(&look)
    } else {
        show::text(&look, bus.root())
    };
    // Whoever reads may have gone: the status says it all the same.
    let _ = writeln!(io::stdout(), "{printed}");
    ExitCode::from(show::status(&look))
}

fn main() -> ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    match parse(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            println!("{USAGE}\n\n{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Command::Serve(arguments)) => match serve(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("lamp-health: {error}");
                ExitCode::FAILURE
            }
        },
        Ok(Command::Show(arguments)) => show(&arguments),
        Err(Refused { why, status }) => {
            eprintln!("lamp-health: {why}\n{USAGE}");
            ExitCode::from(status)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(text: &str) -> Result<Command, Refused> {
        parse(text.split_whitespace().map(str::to_owned))
    }

    fn serving(text: &str) -> ServeArguments {
        match of(text) {
            Ok(Command::Serve(arguments)) => arguments,
            other => panic!("{text:?}: {other:?}"),
        }
    }

    #[test]
    fn serve_reads_its_options_and_the_rest_have_their_defaults() {
        assert_eq!(
            serving(
                "serve --bus-root /run/lampos-bus --expect voice --stale-ms 8000 --proc /p \
                 --judge-zone cpu --expect body --sys /s --run /r --disk /var --judge-zone skin --clock kernel --online on --reach-probe on --needs tasks=/usr/local/bin/harness"
            ),
            ServeArguments {
                bus: Some("/run/lampos-bus".into()),
                expected: vec!["voice".into(), "body".into()],
                stale_after: Duration::from_millis(8_000),
                places: Places {
                    proc: "/p".into(),
                    sys: "/s".into(),
                    run: "/r".into(),
                    disk: "/var".into(),
                    judged: vec!["cpu".into(), "skin".into()],
                    clock: ClockSource::Kernel,
                },
                online: true,
                reach_probe: true,
                reach_idle: false,
                needs: vec![("tasks".into(), "/usr/local/bin/harness".into())],
            }
        );
        assert_eq!(
            serving("serve"),
            ServeArguments {
                bus: None,
                expected: Vec::new(),
                stale_after: Duration::from_millis(5_000),
                places: Places {
                    proc: "/proc".into(),
                    sys: "/sys".into(),
                    run: "/run".into(),
                    disk: "/".into(),
                    judged: Vec::new(),
                    clock: ClockSource::Timesync,
                },
                online: false,
                reach_probe: false,
                reach_idle: false,
                needs: Vec::new(),
            }
        );
        assert!(of("serve --clock ntp").is_err());
        assert!(of("serve --needs tasks").is_err());
        assert!(of("serve --needs tasks=relative/harness").is_err());
        assert!(of("serve --needs tasks=/a --needs tasks=/b").is_err());
        assert!(of("serve --online yes").is_err());
        assert!(of("serve --reach-probe yes").is_err());
        assert!(of("serve --reach-probe on").is_err());
        assert!(of("serve --online on --reach-probe off --reach-probe on").is_err());
        // #1844: the idle probe needs the probe, and takes on or off only.
        assert!(of("serve --online on --reach-idle on").is_err());
        assert!(of("serve --online on --reach-probe on --reach-idle yes").is_err());
        let Command::Serve(idle) =
            of("serve --online on --reach-probe on --reach-idle on").unwrap()
        else {
            panic!("not serve")
        };
        assert!(idle.reach_idle && idle.reach_probe);
        assert!(of("serve --clock kernel --clock kernel").is_err());
        assert_eq!(
            serving("serve --stale-ms 100").stale_after,
            Duration::from_millis(100)
        );
        assert_eq!(
            serving("serve --stale-ms 600000").stale_after,
            Duration::from_secs(600)
        );
        assert_eq!(of("serve --help"), Ok(Command::Help));
        assert_eq!(of("serve --expect voice -h"), Ok(Command::Help));
    }

    #[test]
    fn a_wrong_option_to_serve_is_refused_with_the_reason_and_status_2() {
        for refused in [
            "serve --bus-root relative",
            "serve --proc proc",
            "serve --sys sys",
            "serve --run run",
            "serve --disk disk",
            "serve --stale-ms 99",
            "serve --stale-ms 600001",
            "serve --stale-ms soon",
            "serve --stale-ms 1.5",
            "serve --stale-ms 5000 --stale-ms 5000",
            "serve --bus-root /a --bus-root /b",
            "serve --expect voice --expect voice",
            "serve --expect",
            "serve --json",
            "serve --loud yes",
            "serve extra",
            "serve show",
        ] {
            let Err(Refused { status, .. }) = of(refused) else {
                panic!("{refused:?} was taken");
            };
            assert_eq!(status, 2, "{refused:?}");
        }
        assert_eq!(
            of("serve --loud yes"),
            Err(Refused {
                why: "unknown option --loud".into(),
                status: 2
            })
        );
        // A name is on one line and not too long. Sixty-four may be
        // expected, which is as many as are kept.
        let long = format!("serve --expect {}", "n".repeat(49));
        assert!(of(&long).is_err());
        let refused = |arguments: Vec<String>| parse(arguments.into_iter()).is_err();
        assert!(refused(vec![
            "serve".into(),
            "--expect".into(),
            "vo\nice".into()
        ]));
        assert!(refused(vec!["serve".into(), "--expect".into(), "".into()]));
        let many = |count: usize| {
            let mut arguments = vec!["serve".to_owned()];
            for number in 0..count {
                arguments.extend(["--expect".to_owned(), format!("part{number}")]);
            }
            arguments
        };
        assert!(!refused(many(64)));
        assert!(refused(many(65)));
    }

    #[test]
    fn show_is_what_no_command_means_and_its_mistakes_end_with_64() {
        let plain = Command::Show(ShowArguments {
            bus: None,
            json: false,
        });
        assert_eq!(of(""), Ok(plain));
        assert_eq!(
            of("show"),
            Ok(Command::Show(ShowArguments {
                bus: None,
                json: false
            }))
        );
        let both = || {
            Ok(Command::Show(ShowArguments {
                bus: Some("/run/lampos-bus".into()),
                json: true,
            }))
        };
        assert_eq!(of("show --json --bus-root /run/lampos-bus"), both());
        assert_eq!(of("--bus-root /run/lampos-bus --json"), both());
        for help in ["--help", "-h", "show --help", "--json -h"] {
            assert_eq!(of(help), Ok(Command::Help), "{help:?}");
        }
        for refused in [
            "show --bus-root relative",
            "show --bus-root",
            "show --json --json",
            "show --bus-root /a --bus-root /b",
            "show --expect voice",
            "show --stale-ms 5000",
            "show extra",
            "show serve",
            "--loud",
            "--bus-root relative",
            "status",
            "Serve",
            "--json serve",
        ] {
            let Err(Refused { status, .. }) = of(refused) else {
                panic!("{refused:?} was taken");
            };
            assert_eq!(status, 64, "{refused:?}");
        }
        assert_eq!(
            of("status"),
            Err(Refused {
                why: "unknown command status".into(),
                status: 64
            })
        );
        assert_eq!(
            of("show --expect voice"),
            Err(Refused {
                why: "unknown option --expect".into(),
                status: 64
            })
        );
    }
}
