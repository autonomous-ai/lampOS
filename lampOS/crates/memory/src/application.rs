// Shared application body; the ordinary binary supplies Reporter::start.
use lamp_memory::{
    health::{CountSource, Reporter, State},
    server::Server,
    store::{LocalTime, Options, Store, private_dir},
};
use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

const USAGE: &str = "usage: lamp-memory --dir ABSOLUTE_DIR --socket ABSOLUTE_SOCKET_PATH [--keep-days N] [--utc-offset-minutes M | --zone system|NAME] [--memory-days] [--bus-root ABSOLUTE_DIR]";

const HELP: &str = "Keeps what the lamp remembers under --dir, a directory only its owner can enter (made\nwith mode 0700 if it is not there; refused if its group or everyone can enter it).\nThe default stores a journal file for each local day and a kept file per person.\n--memory-days opts into SQLite, ranked search and linked observations, importing and\nremoving the old files before serving. That directory then requires --memory-days;\nno model is configured or called. See docs/memory-days.md before enabling it.\nAnswers one JSON object per line on --socket (mode 0600), with ready beside the\nsocket while taking requests. --keep-days N (default 30, at least 1): delete journal\ndays more than N days before today. --utc-offset-minutes M (default 0, -720 to 840):\nminutes east of UTC for day boundaries, the same all year. --zone system takes the\nlamp's own time zone (/etc/localtime, written at setup) at each moment, across daylight\nsaving and a zone setup changes while it runs; --zone NAME names one. Not both.\n--bus-root ABSOLUTE_DIR opts into a memory health report once a second. The bus\nis optional and cannot hold the request loop; without this flag no reports run.\nSee docs/memory.md.";

#[derive(Debug, Eq, PartialEq)]
struct Arguments {
    dir: PathBuf,
    socket: PathBuf,
    keep_days: u32,
    utc_offset_minutes: i32,
    zone: Option<String>,
    memory_days: bool,
    bus_root: Option<PathBuf>,
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Arguments>, &'static str> {
    let mut args = args.into_iter();
    let (mut dir, mut socket, mut keep_days, mut utc_offset_minutes) = (None, None, None, None);
    let mut memory_days = false;
    let (mut bus_root, mut zone) = (None, None);
    while let Some(key) = args.next() {
        if key == "--help" {
            let alone = dir.is_none()
                && socket.is_none()
                && keep_days.is_none()
                && utc_offset_minutes.is_none()
                && zone.is_none()
                && !memory_days
                && bus_root.is_none()
                && args.next().is_none();
            return if alone {
                Ok(None)
            } else {
                Err("--help must be used alone")
            };
        }
        if key == "--memory-days" {
            if memory_days {
                return Err("an option was given twice");
            }
            memory_days = true;
            continue;
        }
        let known = matches!(
            key.to_str(),
            Some(
                "--dir" | "--socket" | "--keep-days" | "--utc-offset-minutes" | "--zone" | "--bus-root"
            )
        );
        if !known {
            return Err("unknown option");
        }
        let value = args.next().ok_or("every option requires a value")?;
        match key.to_str() {
            Some("--dir") if dir.is_none() => dir = Some(PathBuf::from(value)),
            Some("--socket") if socket.is_none() => socket = Some(PathBuf::from(value)),
            Some("--bus-root") if bus_root.is_none() => bus_root = Some(PathBuf::from(value)),
            Some("--zone") if zone.is_none() => {
                let name = value.to_str().filter(|name| !name.is_empty());
                zone = Some(
                    name.ok_or("--zone is system or a time zone's name")?
                        .to_owned(),
                );
            }
            Some("--keep-days") if keep_days.is_none() => {
                let days = value.to_str().and_then(|value| value.parse::<u32>().ok());
                keep_days = Some(
                    days.filter(|days| *days >= 1)
                        .ok_or("--keep-days is a whole number of days, at least 1")?,
                );
            }
            Some("--utc-offset-minutes") if utc_offset_minutes.is_none() => {
                let minutes = value.to_str().and_then(|value| value.parse::<i32>().ok());
                utc_offset_minutes = Some(
                    minutes
                        .filter(|minutes| (-720..=840).contains(minutes))
                        .ok_or("--utc-offset-minutes is a whole number from -720 to 840")?,
                );
            }
            _ => return Err("an option was given twice"),
        }
    }
    if zone.is_some() && utc_offset_minutes.is_some() {
        return Err("--zone and --utc-offset-minutes are one or the other");
    }
    let arguments = Arguments {
        dir: dir.ok_or("--dir is required")?,
        socket: socket.ok_or("--socket is required")?,
        keep_days: keep_days.unwrap_or(30),
        utc_offset_minutes: utc_offset_minutes.unwrap_or(0),
        zone,
        memory_days,
        bus_root,
    };
    if !arguments.dir.is_absolute() || !arguments.socket.is_absolute() {
        return Err("--dir and --socket are absolute paths");
    }
    if arguments.socket.file_name().is_none() {
        return Err("--socket is the path of a socket file");
    }
    if arguments
        .bus_root
        .as_ref()
        .is_some_and(|root| !root.is_absolute())
    {
        return Err("--bus-root is an absolute path");
    }
    Ok(Some(arguments))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// One line to the journal. A journal that has gone away is no reason for
/// the memory to stop.
fn log(line: &str) {
    let _ = writeln!(io::stderr(), "lamp-memory: {line}");
}

fn run(
    arguments: &Arguments,
    start_reporter: impl FnOnce(PathBuf, CountSource) -> io::Result<Reporter>,
) -> io::Result<()> {
    // The socket first, so that a second process started by mistake stops
    // here, before it has touched the store; the store before `ready`.
    let server = Server::bind(&arguments.socket)?;
    let mut reporter = arguments.bus_root.as_ref().and_then(|root| {
        start_reporter(
            root.clone(),
            if arguments.memory_days {
                CountSource::Days
            } else {
                CountSource::Legacy(arguments.dir.clone())
            },
        )
        .inspect_err(|_| log("health worker could not start; serving without reports"))
        .ok()
    });
    if reporter.is_none() && arguments.bus_root.is_none() {
        // Said once, from what was made, so that the journal, and a test,
        // can tell that reports are off rather than wait to see none.
        log("health reports off: no --bus-root");
    }
    let result = serve(arguments, server, reporter.as_ref().map(Reporter::state));
    if let Some(reporter) = &mut reporter {
        reporter.finish(result.is_err());
    }
    result
}

/// The zone, or the offset (0 unless given): an unknown zone ends the start,
/// never a silent UTC (#820).
fn local(arguments: &Arguments) -> io::Result<LocalTime> {
    match &arguments.zone {
        Some(zone) => LocalTime::zone(zone),
        None => Ok(LocalTime::Offset(arguments.utc_offset_minutes)),
    }
}

/// The start line with `--memory-days`: it names the zone when `--zone` is
/// given, and is the same as before otherwise (#820).
fn days_start_line(arguments: &Arguments) -> String {
    let zone = arguments
        .zone
        .as_ref()
        .map_or_else(String::new, |zone| format!(" zone={zone}"));
    format!("started memory_days=true{zone}; the model adapter is not configured")
}

fn serve(arguments: &Arguments, server: Server, health: Option<&State>) -> io::Result<()> {
    if arguments.memory_days {
        let mut store = lamp_memory::days::Days::open(
            Options {
                dir: arguments.dir.clone(),
                keep_days: arguments.keep_days,
                local: local(arguments)?,
            },
            now_unix_ms(),
        )
        .map_err(io::Error::other)?;
        if let Some(health) = health {
            health.set_aside(store.set_aside());
            let counts = store
                .enable_health_counts()
                .and_then(|()| store.health_counts());
            health.counts(counts.map_err(io::Error::other));
        }
        log(&days_start_line(arguments));
        return server.run_reporting(&mut store, &mut now_unix_ms, &mut log, health);
    }
    let mut store = Store::open(
        Options {
            dir: arguments.dir.clone(),
            keep_days: arguments.keep_days,
            local: local(arguments)?,
        },
        now_unix_ms(),
    )?;
    let found = store.found();
    log(&format!(
        "started dir={} socket={} keep_days={} {} people={} skipped_lines={} expired_days={} unfinished_files={}",
        arguments.dir.display(),
        arguments.socket.display(),
        arguments.keep_days,
        match &arguments.zone {
            Some(zone) => format!("zone={zone}"),
            None => format!("utc_offset_minutes={}", arguments.utc_offset_minutes),
        },
        found.people,
        found.skipped_lines,
        found.expired_days,
        found.unfinished_files,
    ));
    server.run_reporting(&mut store, &mut now_unix_ms, &mut log, health)
}

fn entrypoint(
    start_reporter: impl FnOnce(PathBuf, CountSource) -> io::Result<Reporter>,
) -> ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    let arguments = match parse(std::env::args_os().skip(1)) {
        Ok(Some(arguments)) => arguments,
        Ok(None) => {
            println!("{USAGE}\n\n{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprintln!("lamp-memory: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    // Before anything else is made: a directory others can enter is the
    // caller's mistake, like a wrong option.
    if let Err(error) = private_dir(&arguments.dir) {
        eprintln!("lamp-memory: {error}");
        return ExitCode::from(2);
    }
    match run(&arguments, start_reporter) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-memory: {error}");
            ExitCode::FAILURE
        }
    }
}
