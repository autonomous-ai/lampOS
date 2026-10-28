use lamp_bus::Bus;
use lamp_controls::{
    engine::Config,
    pace, replay,
    runtime::{self, MissingPins, Service},
};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::{self, BufWriter, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const USAGE: &str = "lamp-controls serve [--config FILE] [--pins FILE] [--touch] [--enable] [--bus-root PATH] [--clock-id ID] [--record FILE] [--seconds N] [--privacy-from FILE] [--touch-from FILE]\n\
lamp-controls replay --input FILE --config FILE --clock-id ID [--touch --pins FILE] [--enable] [--output FILE] [--until-us N]\n\
lamp-controls probe --pins FILE\n\
lamp-controls discover-head --seconds N [--rest-seconds R] [--lines-from FILE]\n\
lamp-controls calibrate-touch --pins FILE --output FILE [--settle-seconds N] [--quiet-seconds N] [--swipe-seconds N] [--touch-percent P] [--release-percent P] [--record FILE | --replay FILE]\n\
lamp-controls clock [--clock-id ID]\n\n\
Shadow is the default. A config file cannot enable the process without --enable.\n\
Without --pins, serve reads nothing and reports Missing. With it, the board's GPIO lines named in FILE are read.\n\
probe reads each line of FILE once with a pull-down and once with a pull-up, and serves nothing.\n\
discover-head samples candidates 37/96/97/98 on chip 0 for 2..60 seconds; it installs no mapping. With --rest-seconds,\n\
its first R seconds after the settle are hands away: each line's changes then are counted, and its pulses only after.\n\
calibrate-touch runs the hardware team's MPR121 steps with a person at the lamp (hands away 10+5 s, then swipe 30 s),\n\
sets touch at 40% and release at 20% of the way from each pad's noise to its own touch level, and writes FILE:\n\
--pins with those thresholds, only if every working pad gave a clear swipe. It serves nothing. --record keeps the\n\
readings; --replay works from kept readings instead of the chip.\n\
--touch selects the new independent head/strip path and its pins schema. It configures the selected I2C chip even in shadow.\n\
The base strip (MPR121) is off unless LAMP_CONTROLS_STRIP=on: serve keeps a pins file's base section unread and starts no strip worker; the head pads, button and reset are served as before.\n\
Without --touch, no new reader, touch topic or gesture interpretation is selected.\n\
Only LAMP_PRIVACY_CONTROLS=on selects an explicit privacy section in --pins; absent/off never opens chip 1.\n\
--privacy-from FILE is for tests on a lamp only: the privacy section's slide is read from FILE instead (open, closed,\n\
gap for no reading, fault for an unreadable switch), and the journal says so. No product unit passes it.\n\
--touch-from FILE is for tests on a lamp only, with --touch: the head pads are read from FILE instead of their lines\n\
(the touched pads' names, or none), at their own poll; the journal says so. No product unit passes it.\n\
--lines-from FILE is for tests only, with discover-head: the candidates' levels are read from FILE instead of chip 0\n\
(the high lines' numbers, e.g. 97 98, or none), at each sample; stderr says so. No product unit passes it.\n\
LAMP_PRIVACY_CONTROLS=no-slide is the route on a lamp with no slide: it refuses a privacy section, opens no chip 1,\n\
and serve's controls/state says no_privacy_slide at the route's cadence. Replay under it is as off.\n\
Replay never opens a bus or hardware. Times are CLOCK_MONOTONIC microseconds.\n\
Record/output files must not already exist. Acquisition records are capped at 32 MiB.\n\
No controls message grants microphone/camera permission or reports a verified stop.";

#[derive(Default)]
struct Args {
    command: String,
    enabled: bool,
    config: Option<PathBuf>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    record: Option<PathBuf>,
    bus_root: Option<PathBuf>,
    clock: Option<String>,
    seconds: Option<u64>,
    rest_seconds: Option<u64>,
    until: Option<u64>,
    pins: Option<PathBuf>,
    touch: bool,
    profile: Option<String>,
    sink: Option<PathBuf>,
    session: Option<String>,
    plan: lamp_controls::touch_calibrate::Plan,
    replay: Option<PathBuf>,
    /// `serve --privacy-from FILE`: the slide read from a test input.
    privacy_from: Option<PathBuf>,
    /// `serve --touch --touch-from FILE`: the head pads read from a test input.
    touch_from: Option<PathBuf>,
    /// `discover-head --lines-from FILE`: the candidates read from a test input.
    lines_from: Option<PathBuf>,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Args>, String> {
    let mut args = args.into_iter();
    let command = args.next().ok_or("a command is required")?;
    if command == "--help" || command == "-h" {
        return Ok(None);
    }
    if ![
        "serve",
        "replay",
        "clock",
        "probe",
        "discover-head",
        "calibrate-touch",
        "mpr-worker",
    ]
    .contains(&command.as_str())
    {
        return Err(format!("unknown command {command}"));
    }
    let mut out = Args {
        command,
        ..Args::default()
    };
    let mut seen = BTreeSet::new();
    while let Some(option) = args.next() {
        if !seen.insert(option.clone()) {
            return Err(format!("{option} was given twice"));
        }
        let allowed = match out.command.as_str() {
            "serve" => [
                "--config",
                "--pins",
                "--enable",
                "--bus-root",
                "--clock-id",
                "--record",
                "--seconds",
                "--touch",
                "--privacy-from",
                "--touch-from",
            ]
            .contains(&option.as_str()),
            "replay" => [
                "--input",
                "--config",
                "--clock-id",
                "--enable",
                "--output",
                "--until-us",
                "--touch",
                "--pins",
            ]
            .contains(&option.as_str()),
            "probe" => option == "--pins",
            "discover-head" => {
                ["--seconds", "--rest-seconds", "--lines-from"].contains(&option.as_str())
            }
            "calibrate-touch" => [
                "--pins",
                "--output",
                "--settle-seconds",
                "--quiet-seconds",
                "--swipe-seconds",
                "--touch-percent",
                "--release-percent",
                "--record",
                "--replay",
            ]
            .contains(&option.as_str()),
            "mpr-worker" => {
                ["--profile-json", "--sink", "--clock-id", "--session"].contains(&option.as_str())
            }
            _ => option == "--clock-id",
        };
        if !allowed {
            return Err(format!("{} does not accept {option}", out.command));
        }
        if option == "--enable" {
            out.enabled = true;
            continue;
        }
        if option == "--touch" {
            out.touch = true;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{option} needs a value"))?;
        match option.as_str() {
            "--config" => out.config = Some(value.into()),
            "--pins" => out.pins = Some(value.into()),
            "--profile-json" => out.profile = Some(value),
            "--sink" => out.sink = Some(value.into()),
            "--session" => out.session = Some(value),
            "--input" => out.input = Some(value.into()),
            "--output" => out.output = Some(value.into()),
            "--record" => out.record = Some(value.into()),
            "--replay" => out.replay = Some(value.into()),
            "--privacy-from" => out.privacy_from = Some(value.into()),
            "--touch-from" => out.touch_from = Some(value.into()),
            "--lines-from" => out.lines_from = Some(value.into()),
            "--clock-id" => out.clock = Some(value),
            "--bus-root" => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return Err("bus root must be absolute".into());
                }
                out.bus_root = Some(path);
            }
            "--seconds" => {
                out.seconds = Some(
                    value
                        .parse::<u64>()
                        .ok()
                        .filter(|n| (1..=86400).contains(n))
                        .ok_or("seconds must be 1..86400")?,
                )
            }
            "--rest-seconds" => {
                out.rest_seconds = Some(
                    value
                        .parse::<u64>()
                        .ok()
                        .filter(|n| (1..=30).contains(n))
                        .ok_or("rest seconds must be 1..30")?,
                )
            }
            "--settle-seconds" | "--quiet-seconds" | "--swipe-seconds" => {
                let seconds = value
                    .parse::<u64>()
                    .ok()
                    .filter(|n| (1..=600).contains(n))
                    .ok_or_else(|| format!("{option} must be 1..600"))?;
                match option.as_str() {
                    "--settle-seconds" => out.plan.settle = seconds,
                    "--quiet-seconds" => out.plan.quiet = seconds,
                    _ => out.plan.swipe = seconds,
                }
            }
            "--touch-percent" | "--release-percent" => {
                let percent = value
                    .parse::<u8>()
                    .ok()
                    .filter(|n| *n <= 100)
                    .ok_or_else(|| format!("{option} must be 0..100"))?;
                if option == "--touch-percent" {
                    out.plan.touch_percent = percent;
                } else {
                    out.plan.release_percent = percent;
                }
            }
            "--until-us" => {
                out.until = Some(
                    value
                        .parse()
                        .map_err(|_| "until-us must be an unsigned integer")?,
                )
            }
            _ => unreachable!(),
        }
    }
    if out.command == "replay"
        && (out.input.is_none() || out.config.is_none() || out.clock.is_none())
    {
        return Err("replay needs --input, --config and the input's --clock-id".into());
    }
    if out.command == "calibrate-touch" {
        if out.pins.is_none() || out.output.is_none() {
            return Err("calibrate-touch needs --pins and --output".into());
        }
        out.plan.check()?;
        if out.record.is_some() && out.replay.is_some() {
            return Err("calibrate-touch takes --record or --replay, not both".into());
        }
    }
    if out.command == "probe" && out.pins.is_none() {
        return Err("probe needs --pins".into());
    }
    if out.touch && out.pins.is_none() {
        return Err("--touch needs --pins with explicit per-unit wiring".into());
    }
    if out.touch_from.is_some() && !out.touch {
        return Err("--touch-from needs --touch and head pads in --pins".into());
    }
    if out.command == "replay" && out.pins.is_some() && !out.touch && !privacy_selected() {
        return Err("replay accepts --pins only with --touch".into());
    }
    if out.command == "discover-head"
        && out
            .seconds
            .is_none_or(|seconds| !(2..=60).contains(&seconds))
    {
        return Err("discover-head needs --seconds in 2..60".into());
    }
    if out.command == "discover-head"
        && out
            .rest_seconds
            .is_some_and(|rest| rest >= out.seconds.unwrap_or(0))
    {
        return Err("discover-head's --rest-seconds must leave time to touch".into());
    }
    if out.command == "mpr-worker"
        && (out.profile.is_none()
            || out.sink.is_none()
            || out.clock.is_none()
            || out.session.is_none())
    {
        return Err("internal MPR worker needs profile, sink, clock and session".into());
    }
    Ok(Some(out))
}

fn config(args: &Args) -> Result<Config, Box<dyn std::error::Error>> {
    let mut config = if let Some(path) = &args.config {
        let mut bytes = Vec::new();
        File::open(path)?.take(16_385).read_to_end(&mut bytes)?;
        if bytes.len() > 16_384 {
            return Err("controls configuration exceeds 16 KiB".into());
        }
        // A newer build's fields are read past and named (#1680).
        let (config, unread) = lamp_controls::forward::read::<Config>(path, &bytes)?;
        if let Some(line) = unread {
            eprintln!("lamp-controls: {line}");
        }
        config
    } else {
        Config {
            buttons: vec!["primary".into()],
            ..Config::default()
        }
    };
    config.enabled = args.enabled;
    config.check()?;
    Ok(config)
}

/// The board's wiring, from its file.
fn pins(
    path: &std::path::Path,
) -> Result<lamp_controls::pins::PinsConfig, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(16_385).read_to_end(&mut bytes)?;
    if bytes.len() > 16_384 {
        return Err("the pins file exceeds 16 KiB".into());
    }
    // Strict at its top (`PinsConfig`); a pin's unknown fields are named,
    // and serde's reason is told without its message (#1680).
    let (wiring, unread) =
        lamp_controls::forward::read::<lamp_controls::pins::PinsConfig>(path, &bytes)?;
    if let Some(line) = unread {
        eprintln!("lamp-controls: {line}");
    }
    if wiring.privacy.is_some() && !privacy_selected() {
        return Err(unselected_privacy().into());
    }
    Ok(wiring)
}

/// Before the switch existed a privacy section was an unknown field, and the
/// file was refused. Without the switch it still is, so a profile installed
/// without it fails loudly instead of running with no hand input.
const UNSELECTED_PRIVACY: &str =
    "the pins file has a privacy section, which needs LAMP_PRIVACY_CONTROLS=on";

/// A privacy section says a slide is wired: that lamp runs the route with
/// `on`, never `no-slide`, which would leave its slide unread.
const SLIDE_WIRED: &str = "LAMP_PRIVACY_CONTROLS=no-slide, but the pins file has a privacy section: a slide is wired, which is LAMP_PRIVACY_CONTROLS=on";

/// Why a pins file's privacy section is refused while `on` is not selected.
fn unselected_privacy() -> &'static str {
    if no_slide_selected() {
        SLIDE_WIRED
    } else {
        UNSELECTED_PRIVACY
    }
}

/// The reader serve uses: the board's lines if a pins file was given, and
/// otherwise the stand-in that says no input is there.
#[cfg(target_os = "linux")]
fn reader(
    args: &Args,
    config: &Config,
    clock: &str,
) -> Result<Box<dyn lamp_controls::input::PinReader>, Box<dyn std::error::Error>> {
    use lamp_controls::pins::{GpioLines, LinePins};
    let Some(path) = &args.pins else {
        return Ok(Box::new(MissingPins));
    };
    let wiring = pins(path)?;
    wiring.check(config)?;
    if wiring.buttons.is_empty() && wiring.head_pads.is_empty() {
        return Ok(Box::new(lamp_controls::touch_pins::NoGpio));
    }
    let lines = GpioLines::open(&wiring)?;
    let session = format!("pins-{}-{}", std::process::id(), lamp_ipc::monotonic_us());
    Ok(Box::new(LinePins::new(
        lines, wiring, config, clock, session,
    )?))
}

#[cfg(not(target_os = "linux"))]
fn reader(
    args: &Args,
    _: &Config,
    _: &str,
) -> Result<Box<dyn lamp_controls::input::PinReader>, Box<dyn std::error::Error>> {
    if args.pins.is_some() {
        return Err("--pins reads GPIO lines, which this system does not have".into());
    }
    Ok(Box::new(MissingPins))
}

#[cfg(target_os = "linux")]
fn probe(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let wiring = pins(path)?;
    for (kind, pin) in wiring
        .buttons
        .iter()
        .map(|pin| ("button", pin))
        .chain(wiring.head_pads.iter().map(|pin| ("head_pad", pin)))
    {
        let read = lamp_controls::pins::GpioLines::probe(&wiring.chip, pin.line);
        println!(
            "{}",
            match read {
                Ok((pulled_down, pulled_up)) =>
                    serde_json::json!({"kind":kind,"name":pin.name,"line":pin.line,
                    "high_with_pull_down":pulled_down,"high_with_pull_up":pulled_up,
                    "reads":if pulled_down == pulled_up {"driven: something holds this line"} else {"follows the pull: nothing drives it now"}}),
                Err(error) =>
                    serde_json::json!({"kind":kind,"name":pin.name,"line":pin.line,"error":error.to_string()}),
            }
        );
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn probe(_: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    Err("probe reads GPIO lines, which this system does not have".into())
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    if args.command == "discover-head" {
        println!(
            "{}",
            serde_json::to_string(&lamp_controls::head_discovery::run(
                args.seconds.unwrap(),
                args.rest_seconds.unwrap_or(0),
                args.lines_from.as_deref(),
            )?)?
        );
        return Ok(());
    }
    if args.command == "mpr-worker" {
        let profile = args.profile.as_ref().unwrap();
        if profile.len() > 16_384 {
            return Err("MPR121 profile exceeds 16 KiB".into());
        }
        return lamp_controls::strip_worker::run(
            serde_json::from_str(profile)?,
            args.sink.as_ref().unwrap(),
            args.clock.as_deref().unwrap(),
            args.session.as_deref().unwrap(),
        )
        .map_err(Into::into);
    }
    if args.command == "probe" {
        return probe(args.pins.as_ref().expect("checked when parsed"));
    }
    if args.command == "calibrate-touch" {
        return calibrate_touch(&args);
    }
    let clock = runtime::clock_id(args.clock.as_deref())?;
    if args.command == "clock" {
        println!(
            "{}",
            serde_json::json!({"clock_id":clock,"at_us":lamp_ipc::monotonic_us()})
        );
        return Ok(());
    }
    let config = config(&args)?;
    if args.touch {
        return run_touch(args, config, &clock);
    }
    if args.command == "replay" {
        let records = replay::read(File::open(args.input.as_ref().unwrap())?)?;
        let selected = if privacy_selected() {
            match &args.pins {
                Some(path) => {
                    let pins = pins(path)?;
                    pins.check(&config)?;
                    pins.privacy.is_some()
                }
                None => false,
            }
        } else {
            false
        };
        let writer: Box<dyn Write> = if let Some(path) = args.output {
            Box::new(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)?,
            )
        } else {
            Box::new(io::stdout().lock())
        };
        let result =
            replay::run_with_privacy(config, &clock, &records, args.until, writer, selected)?;
        eprintln!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }
    let reader = reader(&args, &config, &clock)?;
    let privacy = if privacy_selected() {
        args.pins
            .as_ref()
            .map(|path| pins(path))
            .transpose()?
            .and_then(|pins| pins.privacy)
            .map_or(PrivacyRoute::None, PrivacyRoute::Slide)
    } else if no_slide_selected() {
        // Read here only so that a privacy section is refused (`pins`).
        if let Some(path) = &args.pins {
            pins(path)?;
        }
        PrivacyRoute::NoSlide
    } else {
        PrivacyRoute::None
    };
    serve(args, config, &clock, reader, privacy)
}

/// What `serve` attaches for the privacy route.
enum PrivacyRoute {
    /// The route is off, or `on` with no privacy section (said at start).
    None,
    /// `on`: the slide named by the pins file's privacy section.
    Slide(lamp_controls::privacy::Pins),
    /// `no-slide`: no slide is wired; each `controls/state` says so.
    NoSlide,
}

impl PrivacyRoute {
    /// The route's cadence and the bus's quick rescan apply.
    fn on(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// One line at start for the privacy route (lamp-4ace, 2026-10-16 10:01:
/// Controls said nothing of it, so nobody could tell whether it was on).
fn say_privacy(privacy: &PrivacyRoute) {
    match privacy {
        PrivacyRoute::Slide(pins) => eprintln!(
            "lamp-controls: privacy route on: {} line {}, LOW is muted",
            pins.chip, pins.line
        ),
        PrivacyRoute::NoSlide => eprintln!(
            "lamp-controls: privacy route on, no slide: the voice grants from its mute alone"
        ),
        PrivacyRoute::None if privacy_selected() => eprintln!(
            "lamp-controls: {}=on but the pins file has no privacy section: no privacy source",
            lamp_controls::privacy::ENV
        ),
        PrivacyRoute::None => {}
    }
}

fn privacy_selected() -> bool {
    lamp_controls::privacy::selected(std::env::var_os(lamp_controls::privacy::ENV).as_deref())
}

fn no_slide_selected() -> bool {
    lamp_controls::privacy::no_slide(std::env::var_os(lamp_controls::privacy::ENV).as_deref())
}

/// `--privacy-from` stands in for a slide only: the route must be on with a
/// privacy section.
fn check_privacy_from(
    from: Option<&std::path::Path>,
    privacy: &PrivacyRoute,
) -> Result<(), String> {
    if from.is_some() && !matches!(privacy, PrivacyRoute::Slide(_)) {
        return Err(format!(
            "--privacy-from needs {}=on and a privacy section in --pins",
            lamp_controls::privacy::ENV
        ));
    }
    Ok(())
}

fn privacy_source(
    pins: &lamp_controls::privacy::Pins,
    clock: &str,
    from: Option<&std::path::Path>,
) -> Result<lamp_controls::privacy::Source, Box<dyn std::error::Error>> {
    let session = format!(
        "privacy-{}-{}",
        std::process::id(),
        lamp_ipc::monotonic_us()
    );
    let reader: Box<dyn lamp_controls::privacy::Reader> = match from {
        // A test on a lamp (#2602): the slide's reading from a file.
        Some(path) => {
            eprintln!(
                "lamp-controls: privacy from a test input: {}, not the slide (--privacy-from, a test flag no product unit passes)",
                path.display()
            );
            Box::new(lamp_controls::privacy::FileReader::new(
                path, clock, &session,
            ))
        }
        None => lamp_controls::privacy::open(pins, clock, &session)?,
    };
    Ok(lamp_controls::privacy::Source::new(reader, clock))
}

fn serve(
    args: Args,
    config: Config,
    clock: &str,
    reader: Box<dyn lamp_controls::input::PinReader>,
    privacy: PrivacyRoute,
) -> Result<(), Box<dyn std::error::Error>> {
    let bus = if let Some(root) = args.bus_root {
        Bus::new(root)?
    } else {
        Bus::from_environment()?
    };
    let bus = if privacy.on() {
        lamp_controls::privacy::selected_bus(bus)
    } else {
        bus
    };
    let mut service = Service::new(&bus, config, reader, clock)?;
    check_privacy_from(args.privacy_from.as_deref(), &privacy)?;
    say_privacy(&privacy);
    match privacy {
        PrivacyRoute::Slide(pins) => {
            service.attach_privacy(privacy_source(&pins, clock, args.privacy_from.as_deref())?)
        }
        PrivacyRoute::NoSlide => service.mark_no_slide()?,
        PrivacyRoute::None => {}
    }
    if let Some(path) = args.record {
        if args.touch {
            service.record_privacy_touch_to(&path)?;
        } else {
            service.record_to(&path)?;
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let signal_ids = [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM]
        .map(|signal| signal_hook::flag::register(signal, stop.clone()))
        .into_iter()
        .collect::<io::Result<Vec<_>>>()?;
    let started = lamp_ipc::monotonic_us();
    let mut hands = HandLog::default();
    while !stop.load(Ordering::Relaxed)
        && args
            .seconds
            .is_none_or(|seconds| lamp_ipc::monotonic_us() - started < seconds * 1_000_000)
    {
        pace::run_step(service.poll_us(), |began| {
            let output = service.step(lamp_ipc::monotonic_us);
            hands.note(began, &output);
        });
    }
    for id in signal_ids {
        signal_hook::low_level::unregister(id);
    }
    let recording = service.finish_recording();
    println!(
        "{}",
        serde_json::json!({"counts":service.counts,"recording":recording,"physical_stop_verified":false})
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn touch_reader(
    wiring: &lamp_controls::touch_pins::TouchPinsConfig,
    clock: &str,
) -> Result<Box<dyn lamp_controls::input::PinReader>, Box<dyn std::error::Error>> {
    let Some((pins, config)) = wiring.gpio_config() else {
        return Ok(Box::new(lamp_controls::touch_pins::NoGpio));
    };
    let lines = match lamp_controls::pins::GpioLines::open(&pins) {
        Ok(lines) => lines,
        Err(error) => {
            return Ok(Box::new(lamp_controls::touch_pins::FailedGpio::new(
                error.to_string(),
            )));
        }
    };
    Ok(Box::new(lamp_controls::pins::LinePins::new(
        lines,
        pins,
        &config,
        clock,
        format!(
            "touch-gpio-{}-{}",
            std::process::id(),
            lamp_ipc::monotonic_us()
        ),
    )?))
}

/// Before Controls takes the lines: which head pad lines a pad drives, read
/// once with each pull, ending on the pad's own (#1798), about 20 ms a line. Pins naming bare lines
/// read no touch and said nothing.
#[cfg(target_os = "linux")]
fn head_lines(
    wiring: &lamp_controls::touch_pins::TouchPinsConfig,
) -> Option<lamp_controls::touch_pins::HeadLines> {
    (!wiring.head_pads.is_empty()).then(|| {
        lamp_controls::touch_pins::HeadLines::probed(&wiring.head_pads, |pad| {
            lamp_controls::pins::GpioLines::probe_ending(&wiring.chip, pad.line, pad.pull)
        })
    })
}

#[cfg(not(target_os = "linux"))]
fn head_lines(
    _: &lamp_controls::touch_pins::TouchPinsConfig,
) -> Option<lamp_controls::touch_pins::HeadLines> {
    None
}

#[cfg(not(target_os = "linux"))]
fn touch_reader(
    _: &lamp_controls::touch_pins::TouchPinsConfig,
    _: &str,
) -> Result<Box<dyn lamp_controls::input::PinReader>, Box<dyn std::error::Error>> {
    Err("touch hardware requires Linux; replay opens no hardware".into())
}

/// `--touch-from FILE` (#2602): the GPIO reader with the head pads' lines read
/// from FILE ([`lamp_controls::pins::FileLines`]); the buttons and reset stay
/// on their lines. Its reader session says `test-input`, so every TouchEvent
/// and the touch state name the source.
fn touch_from_reader(
    wiring: &lamp_controls::touch_pins::TouchPinsConfig,
    clock: &str,
    path: &std::path::Path,
) -> Result<Box<dyn lamp_controls::input::PinReader>, Box<dyn std::error::Error>> {
    let (pins, config) = wiring
        .gpio_config()
        .ok_or("--touch-from needs --touch and head pads in --pins")?;
    let mut others = pins.clone();
    others.head_pads.clear();
    let others = if others.buttons.is_empty() {
        None
    } else {
        match open_lines(&others) {
            Ok(lines) => Some(lines),
            Err(error) => {
                return Ok(Box::new(lamp_controls::touch_pins::FailedGpio::new(
                    error.to_string(),
                )));
            }
        }
    };
    eprintln!(
        "lamp-controls: head pads from a test input: {}, not their lines (--touch-from, a test flag no product unit passes)",
        path.display()
    );
    let lines = lamp_controls::pins::FileLines::new(path, pins.head_pads.clone(), others);
    Ok(Box::new(lamp_controls::pins::LinePins::new(
        lines,
        pins,
        &config,
        clock,
        format!(
            "touch-test-input-{}-{}",
            std::process::id(),
            lamp_ipc::monotonic_us()
        ),
    )?))
}

#[cfg(target_os = "linux")]
fn open_lines(
    pins: &lamp_controls::pins::PinsConfig,
) -> io::Result<Box<dyn lamp_controls::pins::Lines>> {
    Ok(Box::new(lamp_controls::pins::GpioLines::open(pins)?))
}

#[cfg(not(target_os = "linux"))]
fn open_lines(
    _: &lamp_controls::pins::PinsConfig,
) -> io::Result<Box<dyn lamp_controls::pins::Lines>> {
    Err(io::Error::other("touch hardware requires Linux"))
}

fn run_touch(args: Args, config: Config, clock: &str) -> Result<(), Box<dyn std::error::Error>> {
    use lamp_controls::{
        strip_worker::Worker, touch_pins, touch_pins::TouchPinsConfig, touch_runtime,
    };
    let wiring = TouchPinsConfig::load(args.pins.as_ref().expect("validated pins argument"))?;
    // The base strip is off unless switched on; a replay reads what it was
    // recorded with.
    let (wiring, config) = if args.command == "replay" {
        (wiring, config)
    } else {
        let switch = std::env::var(touch_pins::STRIP_ENV).ok();
        let (wiring, config, set_aside) = touch_pins::serving(wiring, config, switch.as_deref());
        if set_aside {
            eprintln!(
                "lamp-controls: the base strip is off: V2 doesn't use it; its pins section is kept, unread; {}=on turns it back on",
                touch_pins::STRIP_ENV
            );
        }
        (wiring, config)
    };
    if args.touch_from.is_some() && wiring.head_pads.is_empty() {
        return Err("--touch-from needs --touch and head pads in --pins".into());
    }
    if wiring.privacy.is_some() && !privacy_selected() {
        return Err(unselected_privacy().into());
    }
    let privacy = privacy_selected().then(|| wiring.privacy.clone()).flatten();
    if let Some(pins) = &privacy
        && wiring.privacy_only(&config)?
    {
        if args.command == "replay" {
            let records =
                lamp_controls::touch_replay::read(File::open(args.input.as_ref().unwrap())?)?;
            let writer: Box<dyn Write> = if let Some(path) = args.output {
                Box::new(BufWriter::with_capacity(
                    8 * 1024,
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(path)?,
                ))
            } else {
                Box::new(io::stdout().lock())
            };
            let summary = lamp_controls::touch_replay::run_privacy_only(
                config, clock, &records, args.until, writer,
            )?;
            eprintln!("{}", serde_json::to_string(&summary)?);
            return Ok(());
        }
        return serve(
            args,
            config,
            clock,
            Box::new(lamp_controls::touch_pins::NoGpio),
            PrivacyRoute::Slide(pins.clone()),
        );
    }
    let touch = wiring.check(&config)?;
    if args.command == "replay" {
        let records = lamp_controls::touch_replay::read(File::open(args.input.as_ref().unwrap())?)?;
        let writer: Box<dyn Write> = if let Some(path) = args.output {
            Box::new(BufWriter::with_capacity(
                8 * 1024,
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)?,
            ))
        } else {
            Box::new(io::stdout().lock())
        };
        let summary = lamp_controls::touch_replay::run_with_privacy(
            touch,
            clock,
            &records,
            args.until,
            writer,
            privacy.is_some(),
        )?;
        eprintln!("{}", serde_json::to_string(&summary)?);
        return Ok(());
    }
    let privacy = match privacy {
        Some(pins) => PrivacyRoute::Slide(pins),
        None if no_slide_selected() => PrivacyRoute::NoSlide,
        None => PrivacyRoute::None,
    };
    let (head_lines, gpio) = match &args.touch_from {
        Some(path) => (None, touch_from_reader(&wiring, clock, path)?),
        None => (head_lines(&wiring), touch_reader(&wiring, clock)?),
    };
    let bus = if let Some(root) = args.bus_root {
        Bus::new(root)?
    } else {
        Bus::from_environment()?
    };
    let bus = if privacy.on() {
        lamp_controls::privacy::selected_bus(bus)
    } else {
        bus
    };
    let mut service =
        touch_runtime::Service::new(&bus, config, wiring.clone(), gpio, None::<Worker>, clock)?;
    // A pet on the head (#3997): off unless LAMP_HEAD_PET=on, and then only
    // with head pads to read.
    let pet = std::env::var(lamp_controls::touch::HEAD_PET_ENV).ok();
    if let Some(hold) = lamp_controls::touch::head_pet(pet.as_deref())
        && !wiring.head_pads.is_empty()
    {
        eprintln!("lamp-controls: head pets on (controls/pet), a kept hand's sleep: {hold:?}");
        service.head_pet(&bus, hold)?;
    }
    if let Some(lines) = head_lines {
        eprintln!("lamp-controls: head pads: {}", lines.said());
        service.head_probed(lines);
    }
    check_privacy_from(args.privacy_from.as_deref(), &privacy)?;
    say_privacy(&privacy);
    match privacy {
        PrivacyRoute::Slide(pins) => service.attach_privacy(
            &bus,
            privacy_source(&pins, clock, args.privacy_from.as_deref())?,
        )?,
        PrivacyRoute::NoSlide => service.mark_no_slide(&bus, clock)?,
        PrivacyRoute::None => {}
    }
    if let Some(path) = args.record {
        service.record_to(&path)?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let signal_ids = [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM]
        .map(|signal| signal_hook::flag::register(signal, stop.clone()))
        .into_iter()
        .collect::<io::Result<Vec<_>>>()?;
    // The owner lock is held before a child can configure the I2C device.
    if let Some(base) = &wiring.base {
        match Worker::start(
            base,
            bus.root(),
            clock,
            &format!(
                "touch-strip-{}-{}",
                std::process::id(),
                lamp_ipc::monotonic_us()
            ),
        ) {
            Ok(worker) => service.strip = Some(worker),
            Err(error) => service.strip_unavailable(lamp_ipc::monotonic_us(), &error.to_string()),
        }
    }
    let started = lamp_ipc::monotonic_us();
    let mut hands = HandLog::default();
    while !stop.load(Ordering::Relaxed)
        && args
            .seconds
            .is_none_or(|seconds| lamp_ipc::monotonic_us() - started < seconds * 1_000_000)
    {
        pace::run_step(service.poll_us(), |began| {
            let output = service.step(lamp_ipc::monotonic_us);
            hands.note_reset(began, &output, service.reset_refused());
        });
    }
    let worker = service.strip.as_mut().map(Worker::shutdown);
    let recording = service.finish_recording();
    for id in signal_ids {
        signal_hook::low_level::unregister(id);
    }
    println!(
        "{}",
        serde_json::json!({"counts":service.counts,"gpio_accepted":service.gpio_accepted,
        "strip_accepted":service.strip_accepted,"recording":recording,"worker_cleanup":worker,
        "physical_stop_verified":false,"capture_permission":"not_owned"})
    );
    Ok(())
}

/// The hardware team's steps for the base strip, with a person at the lamp.
/// The report goes to stdout; the new pins file is written only when every
/// working pad gave thresholds, and never over an existing file.
fn calibrate_touch(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    use lamp_controls::{touch_calibrate, touch_pins::TouchPinsConfig};
    let (mut wiring, unread) =
        TouchPinsConfig::read(args.pins.as_ref().expect("checked when parsed"))?;
    // The new file is written from what this build reads: a newer build's
    // fields are not in it, and its line says so (the reviewer on #2076).
    if let Some(line) = &unread {
        eprintln!("lamp-controls: {line}");
    }
    let dropped = if unread.is_some() {
        "; the fields this build does not read, named above, are not in it"
    } else {
        ""
    };
    let output = args.output.as_ref().expect("checked when parsed");
    if output.exists() {
        return Err(format!("{} already exists", output.display()).into());
    }
    let base = wiring
        .base
        .as_mut()
        .ok_or("the pins file has no base strip")?;
    base.check()?;
    if let Some(path) = &args.record
        && path.exists()
    {
        return Err(format!("{} already exists", path.display()).into());
    }
    let session = match &args.replay {
        Some(path) => {
            let mut bytes = Vec::new();
            File::open(path)?.take(64 << 20).read_to_end(&mut bytes)?;
            serde_json::from_slice::<touch_calibrate::Session>(&bytes)?
        }
        None => touch_calibrate::run(base, args.plan, |step| {
            eprintln!("lamp-controls: calibrate-touch: {step}");
        })?,
    };
    if let Some(path) = &args.record {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?
            .write_all(&serde_json::to_vec(&session)?)?;
        eprintln!(
            "lamp-controls: calibrate-touch: kept the readings in {}",
            path.display()
        );
    }
    let report = touch_calibrate::analyse(&session, &base.pads, &base.profile, args.plan);
    println!("{}", serde_json::to_string(&report)?);
    let Some(profile) = report.apply(&base.profile) else {
        let failed: Vec<String> = report
            .pads
            .iter()
            .filter(|pad| pad.verdict != touch_calibrate::Verdict::Ok)
            .map(|pad| format!("E{} {:?}", pad.electrode, pad.verdict))
            .collect();
        return Err(format!(
            "nothing written; pads without thresholds: {}",
            failed.join(", ")
        )
        .into());
    };
    base.profile = profile;
    let mut bytes = serde_json::to_vec_pretty(&wiring)?;
    bytes.push(b'\n');
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(output)?
        .write_all(&bytes)?;
    eprintln!(
        "lamp-controls: calibrate-touch: wrote {}{dropped}",
        output.display()
    );
    Ok(())
}

fn main() -> ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    match parse(std::env::args().skip(1)) {
        Ok(None) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Some(args)) => match run(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("lamp-controls: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("lamp-controls: {error}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// One journal line whenever a hand changes something: a button pressed or
/// let go, the head touched or let go, a gesture, a fault or its end. So the
/// first real press or pat is kept even when nobody is watching the bus.
/// Written to standard error: standard output carries only the closing summary.
/// Changes only, and at most `HandLog::LINES` lines in any ten seconds, so a
/// line that flaps cannot fill the journal; what is held back is counted.
#[derive(Default)]
struct HandLog {
    last: Option<String>,
    window_us: u64,
    lines: u32,
    held_back: u64,
}

impl HandLog {
    const LINES: u32 = 20;
    const WINDOW_US: u64 = 10_000_000;

    fn note(&mut self, at_us: u64, output: &lamp_controls::engine::Output) {
        let state = &output.state;
        let mut now = state
            .buttons
            .iter()
            .map(|button| {
                let word = if button.pressed {
                    "pressed"
                } else {
                    "released"
                };
                format!("button {} {word}", button.name)
            })
            .collect::<Vec<_>>();
        now.push(
            if state.head_active {
                "head touched"
            } else {
                "head not touched"
            }
            .into(),
        );
        if let Some(fault) = state.fault {
            now.push(format!("fault {fault:?}"));
        }
        let gestures = output
            .gestures
            .iter()
            .map(|gesture| {
                format!(
                    "gesture {:?} on {} ({:?}), {} ms, acted on: {}",
                    gesture.gesture,
                    gesture.name,
                    gesture.intent,
                    gesture.duration_us / 1000,
                    gesture.enabled
                )
            })
            .collect();
        for line in self.lines(at_us, now.join(", "), gestures) {
            eprintln!("lamp-controls: hand: {line}");
        }
    }

    /// The reset button on the touch path: pressed, let go, or held long
    /// enough to ask for a factory reset, so a hold that did nothing can be
    /// told from one Controls never saw (the reviewer on #1167). A line read
    /// pressed through its first second is said once, and nothing more of it.
    fn note_reset(
        &mut self,
        at_us: u64,
        output: &lamp_controls::touch::TouchOutput,
        refused: bool,
    ) {
        use lamp_msgs::TouchSource;
        let Some(reset) = output
            .state
            .sources
            .iter()
            .find(|source| source.source == TouchSource::Reset)
        else {
            return;
        };
        let now = match reset.active {
            _ if refused => {
                "reset read pressed through its first second: no factory reset until Controls starts again"
            }
            Some(true) => "reset pressed",
            Some(false) => "reset released",
            None => "reset not read",
        };
        let asked = output
            .events
            .iter()
            .filter(|event| event.source == TouchSource::Reset)
            .map(|event| {
                format!(
                    "reset held {} ms and let go: a factory reset asked, enabled: {}",
                    event.observed_us.saturating_sub(event.began_us) / 1000,
                    event.enabled
                )
            })
            .collect();
        for line in self.lines(at_us, now.into(), asked) {
            eprintln!("lamp-controls: hand: {line}");
        }
    }

    fn lines(&mut self, at_us: u64, now: String, gestures: Vec<String>) -> Vec<String> {
        let mut said = Vec::new();
        if self.last.as_deref() != Some(now.as_str()) {
            said.push(now.clone());
            self.last = Some(now);
        }
        said.extend(gestures);
        let mut out = Vec::new();
        if said.is_empty() {
            return out;
        }
        if at_us.saturating_sub(self.window_us) >= Self::WINDOW_US {
            if self.held_back > 0 {
                out.push(format!("{} more changes not written", self.held_back));
                self.held_back = 0;
            }
            self.window_us = at_us;
            self.lines = 0;
        }
        for line in said {
            if self.lines < Self::LINES {
                self.lines += 1;
                out.push(line);
            } else {
                self.held_back += 1;
            }
        }
        out
    }
}

#[cfg(test)]
mod hand_log_tests {
    use super::HandLog;

    #[test]
    fn only_changes_are_written() {
        let mut log = HandLog::default();
        let start = 20_000_000;
        assert_eq!(
            log.lines(start, "head not touched".into(), vec![]),
            ["head not touched"]
        );
        assert!(
            log.lines(start + 5_000, "head not touched".into(), vec![])
                .is_empty()
        );
        assert_eq!(
            log.lines(start + 10_000, "head touched".into(), vec![]),
            ["head touched"]
        );
        assert_eq!(
            log.lines(
                start + 15_000,
                "head touched".into(),
                vec!["gesture Pet".into()]
            ),
            ["gesture Pet"]
        );
    }

    #[test]
    fn a_flapping_line_is_held_to_twenty_lines_in_ten_seconds_and_counted() {
        let mut log = HandLog::default();
        let start = 20_000_000;
        let mut written = 0;
        for i in 0..200u64 {
            let now = if i % 2 == 0 {
                "head touched"
            } else {
                "head not touched"
            };
            written += log.lines(start + i * 5_000, now.into(), vec![]).len();
        }
        assert_eq!(written, 20);
        let next = log.lines(start + 10_000_000, "head touched".into(), vec![]);
        assert_eq!(
            next,
            [
                "180 more changes not written".to_string(),
                "head touched".into()
            ]
        );
    }
}
