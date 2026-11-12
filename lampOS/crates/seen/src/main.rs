//! `lamp-seen serve`: the "no frames" gate on the bus and, given a frame
//! socket, the camera's frame client; each copy dropped (#6543 phase 1, no
//! server call). Off unless `LAMP_SEEN_ACTIVITY=on`: off, it idles until
//! stopped and touches nothing (no bus, no file, no socket).
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_camera::frames::{FrameClient, Role};
use lamp_msgs::{
    Diagnostic, ExpressionState, Faces, HumanFamiliar, Level, MemoryForgot, MicrophonePrivacy,
    RoomPresence,
};
use lamp_seen::{
    BUS_NAME, ENVIRONMENT, HEALTH_NAME,
    frames::{Discard, Taker},
    gate, opt_in,
    opt_in::Reader as _,
    switched_on,
    watch::Readings,
};
use lamp_turn_owner::BootId;
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const USAGE: &str = "usage: lamp-seen serve [--bus-root ABSOLUTE_PATH] [--opt-in-folder ABSOLUTE_PATH] [--frames-socket ABSOLUTE_PATH --own-boot HEX32 --controller-boot HEX32] [--run-for-ms N]\n\nOff unless LAMP_SEEN_ACTIVITY=on: off, it idles and touches nothing. On, it reads the bus\n(voice/privacy, room/presence, humans/faces/tracked, humans/familiar, expression/state,\nmemory/forgot) and the owner's opt-in, and says on diagnostics whether a frame may be asked\nfor. With --frames-socket it is the camera frame link's perception consumer: one frame\nasked for at a time, only while the gate is open, copied and dropped at once. No frame\nleaves the lamp (#6543 phase 1).";

/// One step of the loop.
const TICK: Duration = Duration::from_millis(100);
/// Messages read from one topic in one step at most.
const DRAIN: usize = 64;
/// How often `diagnostics` is said.
const HEALTH_EVERY: Duration = Duration::from_secs(1);

struct Config {
    bus_root: PathBuf,
    opt_in_folder: PathBuf,
    frames: Option<(PathBuf, BootId, BootId)>,
    run_for: Option<Duration>,
}

fn absolute(value: Option<String>, flag: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value.ok_or(format!("{flag} needs an absolute path"))?);
    if !path.is_absolute() {
        return Err(format!("{flag} needs an absolute path"));
    }
    Ok(path)
}

fn boot(value: Option<String>, flag: &str) -> Result<BootId, String> {
    let value = value.ok_or(format!("{flag} needs 32 hex digits"))?;
    if value.len() != 32 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{flag} needs 32 hex digits"));
    }
    let mut bytes = [0u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| format!("{flag} needs 32 hex digits"))?;
    }
    BootId::new(bytes).map_err(|_| format!("{flag} is not a boot"))
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Config, String> {
    if args.next().as_deref() != Some("serve") {
        return Err(USAGE.into());
    }
    let mut bus_root = PathBuf::from(lamp_bus::DEFAULT_ROOT);
    let mut opt_in_folder = PathBuf::from(lamp_msgs::consent::FOLDER);
    let (mut socket, mut own, mut controller, mut run_for) = (None, None, None, None);
    let mut seen = std::collections::BTreeSet::new();
    while let Some(flag) = args.next() {
        if !seen.insert(flag.clone()) {
            return Err(format!("{flag} given twice"));
        }
        match flag.as_str() {
            "--bus-root" => bus_root = absolute(args.next(), &flag)?,
            "--opt-in-folder" => opt_in_folder = absolute(args.next(), &flag)?,
            "--frames-socket" => socket = Some(absolute(args.next(), &flag)?),
            "--own-boot" => own = Some(boot(args.next(), &flag)?),
            "--controller-boot" => controller = Some(boot(args.next(), &flag)?),
            "--run-for-ms" => {
                let ms: u64 = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .filter(|ms| (1..=86_400_000).contains(ms))
                    .ok_or("--run-for-ms needs 1..86400000")?;
                run_for = Some(Duration::from_millis(ms));
            }
            _ => return Err(USAGE.into()),
        }
    }
    let frames = match (socket, own, controller) {
        (None, None, None) => None,
        (Some(socket), Some(own), Some(controller)) => Some((socket, own, controller)),
        _ => return Err("--frames-socket needs --own-boot and --controller-boot".into()),
    };
    Ok(Config {
        bus_root,
        opt_in_folder,
        frames,
        run_for,
    })
}

fn monotonic_us() -> u64 {
    let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let seconds = u64::try_from(now.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(now.tv_nsec).unwrap_or(0);
    seconds
        .saturating_mul(1_000_000)
        .saturating_add(nanos / 1_000)
}

fn unix_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The newest message of `subscriber` that parses as `T`, reading at most
/// [`DRAIN`].
fn newest<T: serde::de::DeserializeOwned>(subscriber: &mut Subscriber) -> Option<T> {
    let mut newest = None;
    for _ in 0..DRAIN {
        match subscriber.recv(Duration::ZERO) {
            Ok(Some(message)) => {
                if let Ok(value) = message.parse::<T>() {
                    newest = Some(value);
                }
            }
            Ok(None) | Err(_) => break,
        }
    }
    let _ = subscriber.notices();
    newest
}

/// Whether any `memory/forgot` came, reading at most [`DRAIN`].
fn forgot(subscriber: &mut Subscriber) -> bool {
    let mut any = false;
    for _ in 0..DRAIN {
        match subscriber.recv(Duration::ZERO) {
            Ok(Some(message)) => {
                any |= message
                    .parse::<MemoryForgot>()
                    .is_ok_and(|forgot| forgot.check().is_ok());
            }
            Ok(None) | Err(_) => break,
        }
    }
    let _ = subscriber.notices();
    any
}

struct Inputs {
    privacy: Subscriber,
    presence: Subscriber,
    faces: Subscriber,
    familiar: Subscriber,
    expression: Subscriber,
    forgot: Subscriber,
}

impl Inputs {
    fn subscribe(bus: &Bus) -> io::Result<Self> {
        let typed = |topic, type_name| bus.subscribe_typed(topic, BUS_NAME, type_name);
        Ok(Self {
            privacy: typed(MicrophonePrivacy::TOPIC, MicrophonePrivacy::TYPE)?,
            presence: typed(RoomPresence::TOPIC, RoomPresence::TYPE)?,
            faces: typed(Faces::TOPIC, Faces::TYPE)?,
            familiar: typed(HumanFamiliar::TOPIC, HumanFamiliar::TYPE)?,
            expression: typed(ExpressionState::TOPIC, ExpressionState::TYPE)?,
            forgot: typed(MemoryForgot::TOPIC, MemoryForgot::TYPE)?,
        })
    }

    /// Read what came into `readings`; true when a forget came.
    fn read(&mut self, readings: &mut Readings) -> bool {
        let now = monotonic_us();
        if let Some(lease) = newest::<MicrophonePrivacy>(&mut self.privacy) {
            readings.privacy(&lease, now);
        }
        if let Some(presence) = newest::<RoomPresence>(&mut self.presence) {
            readings.presence(&presence, now);
        }
        if let Some(faces) = newest::<Faces>(&mut self.faces) {
            readings.faces(&faces, now);
        }
        if let Some(familiar) = newest::<HumanFamiliar>(&mut self.familiar) {
            readings.familiar(&familiar, now);
        }
        if let Some(state) = newest::<ExpressionState>(&mut self.expression) {
            readings.expression(&state, now);
        }
        forgot(&mut self.forgot)
    }
}

fn health(
    publisher: &mut Publisher,
    gate: Result<(), gate::Closed>,
    taker: Option<&Taker<FrameClient>>,
) {
    let mut diagnostic = Diagnostic::new(Level::Ok, HEALTH_NAME, "")
        .with(
            "gate",
            gate.map_or_else(|closed| closed.as_str(), |()| "open"),
        )
        .with("sends", "none (phase 1)");
    diagnostic = match taker {
        None => diagnostic.with("frames", "no frame socket"),
        Some(taker) => {
            let counts = taker.counts();
            diagnostic
                .with("requested", counts.requested)
                .with("dropped_unsent", counts.handed)
                .with("grant_changed", counts.grant_changed)
                .with("lost", counts.lost)
        }
    };
    let _ = publisher.publish_json(monotonic_us(), &diagnostic);
}

fn serve(config: Config, stop: &AtomicBool) -> io::Result<()> {
    let bus = Bus::new(&config.bus_root)?;
    let mut inputs = Inputs::subscribe(&bus)?;
    let mut diagnostics =
        bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), BUS_NAME)?;
    let opt_in_file = opt_in::File::in_folder(&config.opt_in_folder);
    let mut latch = opt_in::OptIn::default();
    let mut readings = Readings::default();
    let mut taker = config.frames.as_ref().map(|(socket, own, controller)| {
        Taker::new(FrameClient::new(
            socket,
            Role::Perception,
            *own,
            *controller,
            lamp_camera::MAX_FRAME_BYTES,
        ))
    });
    let started = Instant::now();
    let mut said_at: Option<Instant> = None;
    let mut last_gate = None;
    while !stop.load(Ordering::Relaxed) && config.run_for.is_none_or(|run| started.elapsed() < run)
    {
        if inputs.read(&mut readings) {
            latch.forgot(unix_s());
            eprintln!("lamp-seen: a forget came: the opt-in is off until it is given again");
        }
        let opted_in = latch.on(&opt_in_file.read());
        let now = monotonic_us();
        let gate = gate::may_request(&readings.inputs(opted_in, now));
        if last_gate != Some(gate) {
            eprintln!(
                "lamp-seen: gate {}",
                gate.map_or_else(|closed| closed.as_str(), |()| "open")
            );
            last_gate = Some(gate);
        }
        if let Some(taker) = taker.as_mut() {
            taker.step(gate, now, &mut Discard);
        }
        if said_at.is_none_or(|at| at.elapsed() >= HEALTH_EVERY) {
            health(&mut diagnostics, gate, taker.as_ref());
            said_at = Some(Instant::now());
        }
        std::thread::sleep(TICK);
    }
    if let Some(taker) = taker.as_mut() {
        taker.drop_everything();
    }
    let _ = diagnostics.publish_json(monotonic_us(), &Diagnostic::stopping(HEALTH_NAME));
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse(std::env::args().skip(1))?;
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop.clone())?;
    if !switched_on(std::env::var(ENVIRONMENT).ok().as_deref()) {
        // Off: idle until stopped, with no bus, file or socket touched.
        println!(
            "{{\"enabled\":false,\"message\":\"lamp-seen is off; {ENVIRONMENT}=on turns it on.\"}}"
        );
        let started = Instant::now();
        while !stop.load(Ordering::Relaxed)
            && config.run_for.is_none_or(|run| started.elapsed() < run)
        {
            std::thread::sleep(TICK);
        }
        return Ok(());
    }
    serve(config, &stop)?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("lamp-seen: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> impl Iterator<Item = String> {
        text.split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn the_command_line() {
        let config = parse(args("serve")).unwrap();
        assert_eq!(config.bus_root, PathBuf::from(lamp_bus::DEFAULT_ROOT));
        assert_eq!(
            config.opt_in_folder,
            PathBuf::from(lamp_msgs::consent::FOLDER)
        );
        assert!(config.frames.is_none());
        let hex = "0123456789abcdef0123456789abcdef";
        let config = parse(args(&format!(
            "serve --frames-socket /run/x/camera.frames --own-boot {hex} --controller-boot {hex} --run-for-ms 5"
        )))
        .unwrap();
        assert!(config.frames.is_some());
        assert_eq!(config.run_for, Some(Duration::from_millis(5)));
        for bad in [
            "",
            "run",
            "serve --frames-socket /run/x/camera.frames",
            "serve --bus-root relative",
            "serve --own-boot 00 --controller-boot 00 --frames-socket /x",
            "serve --run-for-ms 0",
            "serve --bus-root /a --bus-root /b",
            "serve --other",
        ] {
            assert!(parse(args(bad)).is_err(), "{bad}");
        }
        assert!(
            parse(args(&format!(
                "serve --frames-socket /x --own-boot {} --controller-boot {hex}",
                "0".repeat(32)
            )))
            .is_err(),
            "a zero boot"
        );
    }
}
