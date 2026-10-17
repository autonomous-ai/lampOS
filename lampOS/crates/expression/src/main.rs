use lamp_bus::Bus;
use lamp_expression::{Catalogue, Engine, TICK_US, replay, runtime};

fn bounds(long: bool) -> replay::Bounds {
    if long {
        replay::LONG_BOUNDS
    } else {
        replay::DEFAULT_BOUNDS
    }
}
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::{self, BufReader, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    process::ExitCode,
    sync::atomic::Ordering,
    thread,
    time::Duration,
};

const USAGE: &str = "lamp-expression serve [--enable] [--bus-root PATH] [--clock-id ID] [--catalogue FILE] [--record FILE] [--seconds N] [--utc-offset-minutes N | --zone system|NAME] [--kernel-clock] [--head-touch] [--offline-face]\n\
lamp-expression replay --input FILE [--output FILE] --clock-id ID [--enable] [--catalogue FILE] [--until-us N] [--long]\n\
lamp-expression import --clock-id ID [--long] [--utc-offset-minutes N | --zone system|NAME] FILE...\n\
lamp-expression catalogue\n\
lamp-expression clock [--clock-id ID]\n\n\
Default is shadow mode: no body or light command is published.\n\
Replay writes one JSON line per injected 20 ms clock step and never touches a bus.\n\
Live timestamps use lamp-ipc CLOCK_MONOTONIC microseconds; Linux defaults to its boot ID.\n\
Output files are created new, never overwritten. Live --record is bounded to 64 MiB.\n\
--long lets replay and import take an hour (docs/alive.md). --utc-offset-minutes is the\n\
local time's minutes east of UTC (420 in Vietnam), the same all year; --zone system reads the\n\
lamp's own time zone (or names one), follows its daylight saving and takes up a zone setup\n\
changes without a restart. A catalogue with sleep needs one.\n\
--kernel-clock gives the time of day only while the kernel says the clock is set.\n\
--head-touch reads the head's requests from --touch Controls: a hand kept on the head is leaned\n\
into, a three-second hold puts it to sleep, a tap wakes it (docs/alive.md, #711).\n\
--offline-face draws listening as not ready while lamp/online says the lamp is offline.\n\
No command opens hardware, launches a provider, or changes a device's service settings.";

#[derive(Debug, Default)]
struct Arguments {
    command: String,
    enabled: bool,
    kernel_clock: bool,
    offline_face: bool,
    bus_root: Option<PathBuf>,
    clock: Option<String>,
    catalogue: Option<PathBuf>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    record: Option<PathBuf>,
    seconds: Option<u64>,
    until: Option<u64>,
    imports: Vec<PathBuf>,
    long: bool,
    utc_offset_minutes: Option<i32>,
    head_touch: bool,
    zone: Option<String>,
}

fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Option<Arguments>, String> {
    let mut arguments = arguments.into_iter();
    let Some(command) = arguments.next() else {
        return Err("a command is required".into());
    };
    if ["--help", "-h"].contains(&command.as_str()) {
        return Ok(None);
    }
    if !["serve", "replay", "import", "catalogue", "clock"].contains(&command.as_str()) {
        return Err(format!("unknown command {command}"));
    }
    let mut out = Arguments {
        command,
        ..Arguments::default()
    };
    let mut seen = BTreeSet::new();
    while let Some(option) = arguments.next() {
        if ["--help", "-h"].contains(&option.as_str()) {
            return Ok(None);
        }
        if out.command == "import" && !option.starts_with('-') {
            out.imports.push(option.into());
            continue;
        }
        if !seen.insert(option.clone()) {
            return Err(format!("{option} was given twice"));
        }
        let allowed = match out.command.as_str() {
            "serve" => [
                "--enable",
                "--bus-root",
                "--clock-id",
                "--catalogue",
                "--record",
                "--seconds",
                "--utc-offset-minutes",
                "--kernel-clock",
                "--head-touch",
                "--zone",
                "--offline-face",
            ]
            .contains(&option.as_str()),
            "replay" => [
                "--enable",
                "--clock-id",
                "--catalogue",
                "--input",
                "--output",
                "--until-us",
                "--long",
            ]
            .contains(&option.as_str()),
            "import" => ["--clock-id", "--long", "--utc-offset-minutes", "--zone"]
                .contains(&option.as_str()),
            "clock" => option == "--clock-id",
            _ => false,
        };
        if !allowed {
            return Err(format!("{} does not accept {option}", out.command));
        }
        if option == "--enable" {
            out.enabled = true;
            continue;
        }
        if option == "--offline-face" {
            out.offline_face = true;
            continue;
        }
        if option == "--kernel-clock" {
            out.kernel_clock = true;
            continue;
        }
        if option == "--long" {
            out.long = true;
            continue;
        }
        if option == "--head-touch" {
            out.head_touch = true;
            continue;
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("{option} needs a value"))?;
        match option.as_str() {
            "--clock-id" => out.clock = Some(value),
            "--bus-root" => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return Err("--bus-root must be absolute".into());
                }
                out.bus_root = Some(path);
            }
            "--catalogue" => out.catalogue = Some(value.into()),
            "--input" => out.input = Some(value.into()),
            "--output" => out.output = Some(value.into()),
            "--record" => out.record = Some(value.into()),
            "--seconds" => {
                out.seconds = Some(
                    value
                        .parse::<u64>()
                        .ok()
                        .filter(|v| (1..=86_400).contains(v))
                        .ok_or("--seconds must be 1..86400")?,
                )
            }
            "--utc-offset-minutes" => {
                out.utc_offset_minutes = Some(
                    value
                        .parse::<i32>()
                        .ok()
                        .filter(|v| (-720..=840).contains(v))
                        .ok_or("--utc-offset-minutes must be -720..840")?,
                )
            }
            "--zone" => {
                if value.is_empty() || value.len() > 64 {
                    return Err("--zone is system or a time zone's name".into());
                }
                out.zone = Some(value)
            }
            "--until-us" => {
                out.until = Some(
                    value
                        .parse()
                        .map_err(|_| "--until-us must be an unsigned integer")?,
                )
            }
            _ => unreachable!(),
        }
    }
    if out.command == "replay" && out.input.is_none() {
        return Err("replay needs --input".into());
    }
    if out.command == "import" && out.imports.is_empty() {
        return Err("import needs bus recording files".into());
    }
    if ["replay", "import"].contains(&out.command.as_str()) && out.clock.is_none() {
        return Err("replay/import needs the recording's --clock-id".into());
    }
    if out.zone.is_some() && out.utc_offset_minutes.is_some() {
        return Err("--zone and --utc-offset-minutes are one or the other".into());
    }
    Ok(Some(out))
}

/// The local time `--zone` or `--utc-offset-minutes` gives, if either (#820).
fn local_time(
    args: &Arguments,
) -> Result<Option<lamp_expression::local_time::LocalTime>, Box<dyn std::error::Error>> {
    use lamp_expression::local_time::LocalTime;
    Ok(match (&args.zone, args.utc_offset_minutes) {
        (Some(zone), _) => Some(LocalTime::zone(zone)?),
        (None, Some(offset)) => Some(LocalTime::Offset(offset)),
        (None, None) => None,
    })
}

/// ` zone=NAME` for the start line when `--zone` is given, as the Policy says
/// it; nothing otherwise, so a start without the flag reads as before (#820).
fn zone_note(args: &Arguments) -> String {
    args.zone
        .as_ref()
        .map_or_else(String::new, |zone| format!(" zone={zone}"))
}

fn catalogue(path: Option<&PathBuf>) -> Result<Catalogue, Box<dyn std::error::Error>> {
    let Some(path) = path else {
        return Ok(Catalogue::builtin());
    };
    let mut text = String::new();
    File::open(path)?.take(131_073).read_to_string(&mut text)?;
    if text.len() > 131_072 {
        return Err("catalogue is over 128 KiB".into());
    }
    let catalogue = Catalogue::from_json(&text)?;
    if catalogue
        .speaking
        .is_some_and(|speaking| speaking.sway_roll.is_some())
    {
        // #955's head roll: ignored, so the rest of Expression still runs.
        eprintln!(
            "lamp-expression: speaking.sway_roll is no longer used and is ignored; no sway between sentences without sway_yaw"
        );
    }
    Ok(catalogue)
}

fn writer(path: Option<&PathBuf>) -> io::Result<Box<dyn Write>> {
    match path {
        Some(path) => Ok(Box::new(
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(path)?,
        )),
        None => Ok(Box::new(io::stdout())),
    }
}

fn run(args: Arguments) -> Result<(), Box<dyn std::error::Error>> {
    if args.command == "catalogue" {
        println!("{}", include_str!("../behaviours.json"));
        return Ok(());
    }
    let clock = runtime::clock_id(args.clock.as_deref())?;
    if args.command == "clock" {
        println!(
            "{}",
            serde_json::json!({"clock_id":clock,"at_us":lamp_ipc::monotonic_us()})
        );
        return Ok(());
    }
    if args.command == "import" {
        let mut imported = replay::import_bus_bounded(&args.imports, &clock, bounds(args.long))?;
        if let Some(local) = local_time(&args)? {
            replay::add_local_daytime(&mut imported, &local)?;
        }
        let mut stdout = io::stdout().lock();
        for record in imported.records {
            serde_json::to_writer(&mut stdout, &record)?;
            stdout.write_all(b"\n")?;
        }
        for warning in imported.warnings {
            eprintln!("lamp-expression: {warning:?}");
        }
        return Ok(());
    }
    let catalogue = catalogue(args.catalogue.as_ref())?;
    if args.command == "serve"
        && catalogue
            .alive
            .as_ref()
            .is_some_and(|alive| alive.sleep.is_some())
        && args.utc_offset_minutes.is_none()
        && args.zone.is_none()
    {
        return Err(
            "a catalogue with sleep needs --zone or --utc-offset-minutes, or it never knows it is night"
                .into(),
        );
    }
    if args.command == "replay" {
        let mut engine = Engine::new(catalogue, args.enabled, clock)?;
        let summary = replay::run_bounded(
            BufReader::new(File::open(args.input.as_ref().expect("validated"))?),
            writer(args.output.as_ref())?,
            &mut engine,
            args.until,
            bounds(args.long),
        )?;
        eprintln!("lamp-expression: replay {summary:?}; simulated request timing only");
        return Ok(());
    }
    // Read before `args` is taken apart: a zone that cannot be found ends the
    // start before anything else.
    let local = local_time(&args)?;
    let zone = zone_note(&args);
    let bus = match args.bus_root {
        Some(path) => Bus::new(path)?,
        None => Bus::from_environment()?,
    };
    let recorder = args
        .record
        .as_ref()
        .map(|path| runtime::Recorder::new(path, 64 * 1024 * 1024))
        .transpose()?;
    let mut service = runtime::Service::new(&bus, catalogue, args.enabled, &clock)?;
    if let Some(local) = local {
        service = service.with_local(local);
    }
    if args.kernel_clock {
        service = service.with_clock_check(lamp_ipc::clock_synced);
    }
    if args.head_touch {
        service = service.with_head_touch(&bus)?;
    }
    if args.offline_face {
        service = service.with_offline_face(&bus);
    }
    eprintln!(
        "lamp-expression: started mode={} clock_id={} root={}{}{}",
        if args.enabled { "enabled" } else { "shadow" },
        clock,
        bus.root().display(),
        zone,
        if args.head_touch {
            " head_touch=true"
        } else {
            ""
        }
    );
    let began = lamp_ipc::monotonic_us();
    loop {
        let started = lamp_ipc::monotonic_us();
        service.counts.records_lost = recorder
            .as_ref()
            .map_or(0, |r| r.lost.load(Ordering::Relaxed));
        let output = service.step(lamp_ipc::monotonic_us, runtime::unix_ms());
        if let Some(recorder) = &recorder {
            recorder.record(output.state.stamp.at_us, output);
        }
        if args
            .seconds
            .is_some_and(|seconds| started.saturating_sub(began) >= seconds * 1_000_000)
        {
            break;
        }
        // No catch-up storm after a stalled host. Engine and command leases
        // are evaluated against the current shared clock on the next step.
        thread::sleep(Duration::from_micros(
            TICK_US.saturating_sub(lamp_ipc::monotonic_us().saturating_sub(started)),
        ));
    }
    eprintln!(
        "lamp-expression: stopped steps={} refused={} dropped={} output_errors={} longest_step_us={} records_lost={}",
        service.counts.steps,
        service.counts.refused,
        service.counts.dropped,
        service.counts.output_errors,
        service.counts.longest_step_us,
        recorder
            .as_ref()
            .map_or(0, |r| r.lost.load(Ordering::Relaxed))
    );
    Ok(())
}

fn main() -> ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    let args = match parse(std::env::args().skip(1)) {
        Ok(Some(args)) => args,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprintln!("lamp-expression: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-expression: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(text: &str) -> Result<Option<Arguments>, String> {
        parse(text.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn outputs_require_the_explicit_enable_switch() {
        assert!(!args("serve").unwrap().unwrap().enabled);
        assert!(
            args("serve --enable --bus-root /tmp/bus")
                .unwrap()
                .unwrap()
                .enabled
        );
        assert!(
            !args("replay --input fixture --clock-id test")
                .unwrap()
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn invalid_options_cannot_accidentally_enable_a_service() {
        for input in [
            "",
            "start",
            "serve --enable --enable",
            "serve --seconds 0",
            "serve --seconds 86401",
            "serve --bus-root relative",
            "serve --input replay",
            "replay",
            "replay --input f",
            "replay --input f --clock-id t --until-us -1",
            "import --clock-id t",
            "catalogue --enable",
            "clock --enable",
            "serve --enable true",
            "serve --long",
            "serve --utc-offset-minutes 841",
            "serve --utc-offset-minutes -721",
            "serve --utc-offset-minutes seven",
            "replay --input f --clock-id t --utc-offset-minutes 420",
            "replay --input f --clock-id t --zone system",
            "serve --zone system --utc-offset-minutes 420",
            "serve --zone",
            "clock --long",
        ] {
            assert!(args(input).is_err(), "{input}");
        }
        assert!(args("--help").unwrap().is_none());
    }

    #[test]
    fn the_hour_and_the_time_of_day_are_asked_for_by_name() {
        let serve = args("serve --utc-offset-minutes 420").unwrap().unwrap();
        assert_eq!(serve.utc_offset_minutes, Some(420));
        assert!(!serve.long);
        let replay = args("replay --input f --clock-id t --long")
            .unwrap()
            .unwrap();
        assert!(replay.long);
        let import = args("import --clock-id t --long --utc-offset-minutes -300 a b")
            .unwrap()
            .unwrap();
        assert!(import.long);
        assert_eq!(import.utc_offset_minutes, Some(-300));
        assert_eq!(import.imports.len(), 2);
        // #820: a zone in place of the offset, followed across its clock changes.
        let zoned = args("serve --zone system").unwrap().unwrap();
        assert_eq!(zoned.zone.as_deref(), Some("system"));
        assert_eq!(zoned.utc_offset_minutes, None);
        let named = args("import --clock-id t --zone America/New_York a")
            .unwrap()
            .unwrap();
        assert!(matches!(
            local_time(&named).unwrap(),
            Some(lamp_expression::local_time::LocalTime::Zone(_))
        ));
        assert!(local_time(&args("serve --zone Nowhere/City").unwrap().unwrap()).is_err());
        assert!(matches!(
            local_time(&serve).unwrap(),
            Some(lamp_expression::local_time::LocalTime::Offset(420))
        ));
    }

    #[test]
    fn the_start_line_names_the_zone_only_when_one_is_given() {
        assert_eq!(
            zone_note(&args("serve --zone system").unwrap().unwrap()),
            " zone=system"
        );
        assert_eq!(
            zone_note(&args("serve --zone America/New_York").unwrap().unwrap()),
            " zone=America/New_York"
        );
        assert_eq!(
            zone_note(&args("serve --utc-offset-minutes 420").unwrap().unwrap()),
            ""
        );
        assert_eq!(zone_note(&args("serve").unwrap().unwrap()), "");
    }
}
