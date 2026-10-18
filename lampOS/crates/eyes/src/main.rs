//! The lamp's eyes: one process that owns the camera, finds a face in what
//! it sees and tells the body where it is. It opens the camera only while
//! the voice service keeps saying it may, and sends nothing but the face's
//! place in the picture. With `--sight-file` it also keeps its newest frame,
//! that one only, in a private file for the voice service, and with
//! `--full-look-file` one still the voice service asked for, in another.

use std::{ffi::OsString, path::PathBuf};

const USAGE: &str = "lamp-eyes --device ABSOLUTE_PATH --lock-directory PRIVATE_ABSOLUTE_DIRECTORY --usb VENDOR:PRODUCT@TOPOLOGY --model ABSOLUTE_PATH --permit-socket ABSOLUTE_PATH --body-socket ABSOLUTE_PATH [--sight-file ABSOLUTE_PATH] [--presence-file ABSOLUTE_PATH] [--recogniser ABSOLUTE_PATH --people ABSOLUTE_PATH --who-socket ABSOLUTE_PATH [--known-file ABSOLUTE_PATH]] [--leave-still-faces on|off] [--steady-rate on|off] [--look-on-change on|off] [--full-look WIDTHxHEIGHT --full-look-file ABSOLUTE_PATH [--full-look-movement on|off]]\n\nOwns the camera at --device (checked against the USB identity given, hex vendor and\nproduct and the port topology, for example 01da:5875@3-1.1.4) and looks for a face in\nits frames four times a second while there is one and less than twice a second when\nthere is none, with the pinned YuNet model at --model. With --steady-rate on, it looks\ntwice a second while one face has sat still for 3 s (off unless given). With\n--look-on-change on, once no face has been found for 4 s, a frame like the last one the\ndetector ran on is said to hold no face without it, and the detector runs at least every\n3 s (off unless given). Each look\nsends one datagram to --body-socket: {\"face\":{\"x\":..,\"y\":..,\"size\":..,\"turned\":..}} or\n{\"face\":null}. The camera is open only while {\"camera\":true} keeps arriving on\n--permit-socket, at least every 1.5 s; {\"camera\":false} closes it at once. Without\n--sight-file and --full-look-file no frame is written anywhere and none leaves this process. With it the\nnewest frame, and only that one, is kept in that file (mode 0600) while the camera is\nopen and removed when it closes: it is what the voice service shows the model. With\n--presence-file one line, the near face's height as a share of the picture's, is kept\nfresh in that file while there is a face: no picture, only that number. With\n--recogniser (the pinned SFace model), --people and --who-socket, the eyes remember\na face when the voice service asks ({\"remember\":\"NAME\"} on the socket), forget it\n({\"forget\":\"NAME\"}), list the names ({\"list\":true}) and say whose face is at the\nlamp ({\"who\":true}). Only the prints of people who asked are kept, in --people\n(mode 0600); no picture. With --known-file, the name of the one known at the lamp is\nkept fresh in that file while they are there. With --leave-still-faces on, a face at a place\nwhere faces have come from for half an hour, every one small (under 0.14 of the height)\nand faint (scored under 0.8), is not a person: it is left out of what the body is told,\nthe bus and recognition (off unless given). With --full-look and --full-look-file the\ncamera runs at that size, the working frame is kept in --sight-file at 320 across as before, and {\"look_full\":ID} on\n--permit-socket asks for one still: taken once the head has settled\n(body/joint_states on the bus, at most 2 s), at most 1024 across and 64 KiB, or with\n{\"look_full\":ID,\"side\":PIXELS} at most that side across and 184 KiB, marked\nwith the ID in a JPEG comment, kept in --full-look-file for 15 s and removed (off unless given).\nWith --full-look-movement on the head settles on movement only: the camera's aim, the arm's\njoints with the head's, and never a joint's distance to its goal (off unless given).\n";

#[derive(Clone, Debug, Eq, PartialEq)]
struct Usb {
    vendor: u16,
    product: u16,
    topology: String,
}

#[derive(Debug, Eq, PartialEq)]
struct Arguments {
    device: PathBuf,
    lock_directory: PathBuf,
    usb: Usb,
    model: PathBuf,
    permit_socket: PathBuf,
    body_socket: PathBuf,
    sight_file: Option<PathBuf>,
    presence_file: Option<PathBuf>,
    /// The recogniser, the people's file and the request socket: all three
    /// or none.
    who: Option<(PathBuf, PathBuf, PathBuf)>,
    /// The name of whoever known is at the lamp, kept fresh while they are.
    known_file: Option<PathBuf>,
    /// Leave out a face that never moves (#763).
    leave_still: bool,
    /// Look half as often while one face sits still (#705).
    steady_rate: bool,
    /// With nobody in view, run no detector on a frame like the last (#705).
    look_on_change: bool,
    /// The camera's size for a full look (#836), when one may be asked for.
    full_look: Option<(u32, u32)>,
    /// Where a full look's still is kept, apart from the working frame.
    full_look_file: Option<PathBuf>,
    /// A full look's head settles on movement only (#842).
    full_look_movement: bool,
}

/// `VENDOR:PRODUCT@TOPOLOGY`, the two numbers as four hex digits each.
fn usb_of(value: &str) -> Option<Usb> {
    let (numbers, topology) = value.split_once('@')?;
    let (vendor, product) = numbers.split_once(':')?;
    let hex = |text: &str| {
        (text.len() == 4 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| u16::from_str_radix(text, 16).ok())
            .flatten()
            .filter(|number| *number != 0)
    };
    let plain = !topology.is_empty()
        && topology.len() <= 64
        && topology
            .bytes()
            .all(|byte| byte.is_ascii_digit() || b"-.".contains(&byte));
    plain.then_some(())?;
    Some(Usb {
        vendor: hex(vendor)?,
        product: hex(product)?,
        topology: topology.into(),
    })
}

/// The camera's working size, the only one without `--full-look`.
const WORKING: (u32, u32) = (320, 240);

/// The size the camera is opened at: the working size without a full look,
/// and with one after it last failed to open at the full size.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn opened_at(full_look: Option<(u32, u32)>, failures: u32) -> (u32, u32) {
    full_look
        .filter(|_| lamp_eyes::full_look::full_next(failures))
        .unwrap_or(WORKING)
}

/// `WIDTHxHEIGHT`, each even, from 320 by 240 to 3264 by 2448: the camera's
/// bounds (`lamp_camera::MAX_WIDTH`, `MAX_HEIGHT`), so that a size it would
/// refuse is refused here, at start, and not at every open.
fn size_of(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once('x')?;
    let (width, height) = (width.parse::<u32>().ok()?, height.parse::<u32>().ok()?);
    ((320..=3_264).contains(&width)
        && (240..=2_448).contains(&height)
        && width % 2 == 0
        && height % 2 == 0)
        .then_some((width, height))
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Arguments>, &'static str> {
    let mut args = args.into_iter();
    let (mut device, mut lock_directory, mut usb) = (None, None, None);
    let (mut model, mut permit_socket, mut body_socket) = (None, None, None);
    let (mut sight_file, mut presence_file) = (None, None);
    let (mut recogniser, mut people, mut who_socket, mut known_file) = (None, None, None, None);
    let (mut leave_still, mut full_look, mut full_look_file) = (None, None, None);
    let (mut steady_rate, mut full_look_movement, mut look_on_change) = (None, None, None);
    while let Some(key) = args.next() {
        if key == "--help" {
            let alone = device.is_none()
                && lock_directory.is_none()
                && usb.is_none()
                && model.is_none()
                && permit_socket.is_none()
                && body_socket.is_none()
                && sight_file.is_none()
                && presence_file.is_none()
                && recogniser.is_none()
                && people.is_none()
                && who_socket.is_none()
                && known_file.is_none()
                && leave_still.is_none()
                && steady_rate.is_none()
                && look_on_change.is_none()
                && full_look_movement.is_none()
                && full_look.is_none()
                && full_look_file.is_none()
                && args.next().is_none();
            return if alone {
                Ok(None)
            } else {
                Err("--help must be used alone")
            };
        }
        let value = args.next().ok_or("every option requires a value")?;
        match key.to_str() {
            Some("--device") if device.is_none() => device = Some(PathBuf::from(value)),
            Some("--lock-directory") if lock_directory.is_none() => {
                lock_directory = Some(PathBuf::from(value));
            }
            Some("--model") if model.is_none() => model = Some(PathBuf::from(value)),
            Some("--permit-socket") if permit_socket.is_none() => {
                permit_socket = Some(PathBuf::from(value));
            }
            Some("--body-socket") if body_socket.is_none() => {
                body_socket = Some(PathBuf::from(value));
            }
            Some("--sight-file") if sight_file.is_none() => {
                sight_file = Some(PathBuf::from(value));
            }
            Some("--presence-file") if presence_file.is_none() => {
                presence_file = Some(PathBuf::from(value));
            }
            Some("--recogniser") if recogniser.is_none() => recogniser = Some(PathBuf::from(value)),
            Some("--people") if people.is_none() => people = Some(PathBuf::from(value)),
            Some("--who-socket") if who_socket.is_none() => who_socket = Some(PathBuf::from(value)),
            Some("--known-file") if known_file.is_none() => known_file = Some(PathBuf::from(value)),
            Some("--leave-still-faces") if leave_still.is_none() => {
                leave_still = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--leave-still-faces is on or off"),
                });
            }
            Some("--steady-rate") if steady_rate.is_none() => {
                steady_rate = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--steady-rate is on or off"),
                });
            }
            Some("--look-on-change") if look_on_change.is_none() => {
                look_on_change = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--look-on-change is on or off"),
                });
            }
            Some("--full-look-movement") if full_look_movement.is_none() => {
                full_look_movement = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--full-look-movement is on or off"),
                });
            }
            Some("--full-look-file") if full_look_file.is_none() => {
                full_look_file = Some(PathBuf::from(value));
            }
            Some("--full-look") if full_look.is_none() => {
                full_look = Some(value.to_str().and_then(size_of).ok_or(
                    "--full-look is WIDTHxHEIGHT, for example 1280x960, from 320x240 to 3264x2448",
                )?);
            }
            Some("--usb") if usb.is_none() => {
                usb =
                    Some(value.to_str().and_then(usb_of).ok_or(
                        "--usb is VENDOR:PRODUCT@TOPOLOGY, for example 01da:5875@3-1.1.4",
                    )?);
            }
            _ => return Err("unknown or duplicate option"),
        }
    }
    let result = Arguments {
        device: device.ok_or("--device is required")?,
        lock_directory: lock_directory.ok_or("--lock-directory is required")?,
        usb: usb.ok_or("--usb is required")?,
        model: model.ok_or("--model is required")?,
        permit_socket: permit_socket.ok_or("--permit-socket is required")?,
        body_socket: body_socket.ok_or("--body-socket is required")?,
        sight_file,
        presence_file,
        who: match (recogniser, people, who_socket) {
            (Some(model), Some(people), Some(socket)) => Some((model, people, socket)),
            (None, None, None) => None,
            _ => return Err("--recogniser, --people and --who-socket go together"),
        },
        known_file,
        leave_still: leave_still.unwrap_or(false),
        steady_rate: steady_rate.unwrap_or(false),
        look_on_change: look_on_change.unwrap_or(false),
        full_look,
        full_look_file,
        full_look_movement: full_look_movement.unwrap_or(false),
    };
    if result.full_look.is_some() != result.full_look_file.is_some()
        || (result.full_look_file.is_some() && result.full_look_file == result.sight_file)
    {
        return Err("--full-look and --full-look-file go together, apart from --sight-file");
    }
    if result.known_file.is_some() && result.who.is_none() {
        return Err("--known-file needs --recogniser, --people and --who-socket");
    }
    for path in [
        &result.device,
        &result.lock_directory,
        &result.model,
        &result.permit_socket,
        &result.body_socket,
    ]
    .into_iter()
    .chain(&result.sight_file)
    .chain(&result.full_look_file)
    .chain(&result.presence_file)
    .chain(
        result
            .who
            .iter()
            .flat_map(|(model, people, socket)| [model, people, socket]),
    )
    .chain(&result.known_file)
    {
        if !path.is_absolute()
            || path.as_os_str().len() > 200
            || path.to_str().is_none()
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err("paths must be short absolute UTF-8 paths without parent traversal");
        }
    }
    Ok(Some(result))
}

fn main() -> std::process::ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
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
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-eyes failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn run(arguments: Arguments) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = arguments;
    Err("the eyes require Linux; no camera was opened".into())
}

#[cfg(target_os = "linux")]
fn run(arguments: Arguments) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use lamp_camera::{
        CaptureConfig, CaptureId, DRIVER_TEARDOWN_BUDGET_US, FrameInterval, MAX_FRAME_BYTES,
        OPEN_BUDGET_US, OperationBudget, PortIo, READ_BUDGET_US, SourceId,
        linux::{LinuxConfig, LinuxPort, UsbIdentity},
    };
    use lamp_eyes::{
        faces::Detector,
        full_look::Head,
        kept::{Kept, Presence},
        picture::Picture,
        sight::{Seen, choose},
        who::{self, Ask, NEAR_ENOUGH, People, Recogniser, Taking, align, ask_of, plain_name},
    };
    use lamp_turn_owner::{BootId, MonoTime};
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::{
        num::NonZeroU64,
        os::unix::net::UnixDatagram,
        time::{Duration, Instant},
    };

    /// A permission older than this is none.
    /// The newest frame is looked at this often at most while there is a
    /// face to follow, and for a few seconds after the last one.
    const LOOK_EVERY: Duration = Duration::from_millis(250);
    /// And this often with nobody there: often enough to notice someone
    /// sitting down within a second, at a quarter of the work. Looking six
    /// times a second all day kept lamp-4ace's processor 8 to 10 degrees
    /// warmer (63 to 64 degrees C with the eyes stopped, 70 to 75 with them),
    /// above the 70 at which its kernel begins to hold it back.
    const GLANCE_EVERY: Duration = Duration::from_millis(600);
    const FOLLOW_FOR: Duration = Duration::from_secs(4);
    /// And this often while one face has sat still (`steady.rs`, #705).
    const STEADY_EVERY: Duration = Duration::from_millis(lamp_eyes::steady::STEADY_EVERY_MS);
    /// Time for STREAMOFF and the buffers' release to return when the camera
    /// is closed, so it is left stopped and freed before its file is closed.
    const STOP: u64 = DRIVER_TEARDOWN_BUDGET_US;
    /// A camera that did not open, or failed, is tried again after this.
    const RETRY: Duration = Duration::from_secs(5);
    const REPORT_EVERY: Duration = Duration::from_secs(10);
    /// Whose the near face is, asked at most this often: a print takes a
    /// tenth of a second or more on the lamp.
    const RECOGNISE_EVERY: Duration = Duration::from_secs(2);
    /// Who was at the lamp is said for this long after the last look.
    const KNOWN_FOR: Duration = Duration::from_secs(3);

    /// Remembering faces, for the people who asked.
    struct Who {
        recogniser: Recogniser,
        people: People,
        listener: UnixListener,
        known: Option<(String, f32, Instant)>,
        looked_for: Option<Instant>,
        taking: Option<(Taking, UnixStream)>,
    }
    /// The name of the one known at the lamp, replaced whole, or no file.
    fn keep_known(path: &std::path::Path, name: Option<&str>) {
        use std::os::unix::fs::OpenOptionsExt;
        let Some(name) = name else {
            let _ = std::fs::remove_file(path);
            return;
        };
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".new");
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)
            .and_then(|mut file| writeln!(file, "{name}"));
        if written.is_ok() {
            let _ = std::fs::rename(&temporary, path);
        }
    }
    fn answer(stream: &mut UnixStream, value: serde_json::Value) {
        // A caller that is gone or slow changes nothing here.
        let _ = writeln!(stream, "{value}");
    }

    let detector = Detector::load(&arguments.model)?;
    let mut who = match &arguments.who {
        Some((model, people_path, socket)) => {
            let recogniser = Recogniser::load(model)?;
            let people = People::open(people_path.clone())?;
            match std::fs::remove_file(socket) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let listener = UnixListener::bind(socket)?;
            listener.set_nonblocking(true)?;
            eprintln!(
                "lamp-eyes: remembers a face when asked; {} people known",
                people.names().len()
            );
            Some(Who {
                recogniser,
                people,
                listener,
                known: None,
                looked_for: None,
                taking: None,
            })
        }
        None => None,
    };
    // A socket file left by an earlier run is not a listener: replace it.
    match std::fs::remove_file(&arguments.permit_socket) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let permits = UnixDatagram::bind(&arguments.permit_socket)?;
    let out = UnixDatagram::unbound()?;
    out.set_nonblocking(true)?;
    let tell = |seen: Seen| {
        if let Ok(text) = serde_json::to_vec(&seen) {
            // A body that is absent or slow changes nothing here.
            let _ = out.send_to(&text, &arguments.body_socket);
        }
    };

    let source = SourceId::new("lamp-eyes").map_err(|error| format!("{error:?}"))?;
    let mut boot = [0u8; 16];
    boot[..4].copy_from_slice(&std::process::id().to_le_bytes());
    boot[4..12].copy_from_slice(&lamp_ipc::monotonic_us().to_le_bytes());
    boot[15] = 1;
    let worker = BootId::new(boot).map_err(|error| format!("{error:?}"))?;
    let budget = |micros: u64| {
        let now = MonoTime::from_micros(lamp_ipc::monotonic_us());
        OperationBudget {
            started_at: now,
            deadline: now.checked_add(micros).unwrap_or(now),
        }
    };

    let mut kept = arguments.sight_file.clone().map(Kept::new);
    // A full look's still, in its own file.
    // Up to the larger bound: an ask may name a side of its own (#941).
    let mut still_kept = arguments
        .full_look_file
        .clone()
        .map(|path| Kept::up_to(path, lamp_eyes::full_look::LARGE_MOST_BYTES));
    // A full look (#836): the one still asked for, when it was written, and
    // where the body says how its joints are. A camera that did not open at
    // the full size is opened at the working size the next time.
    let mut wanted: Option<lamp_eyes::full_look::Wanted> = None;
    // The ask that is wanted: its id, written with its still, and its size.
    let mut wanted_ask = lamp_eyes::full_look::Ask { id: 0, side: None };
    let mut still_at: Option<Instant> = None;
    // Opens in a row that failed, and when to try the full size again
    // after falling back to the working size.
    let mut failures = 0u32;
    let mut upgrade = lamp_eyes::full_look::Upgrade::default();
    let joints_bus = arguments
        .full_look
        .and_then(|_| lamp_bus::Bus::from_environment().ok());
    let mut joints_read_at: Option<Instant> = None;
    let mut head = Head::Absent;
    // Every frame the camera has given, and the bytes of the newest.
    let (mut frames_all, mut newest) = (0u64, 0usize);
    let mut presence = arguments.presence_file.clone().map(Presence::new);
    // The faces in view, for whoever listens on the lamp's bus.
    let mut bus_out = lamp_eyes::bus_out::BusOut::from_environment();
    // Faces that never move, left out when asked.
    let mut still = arguments.leave_still.then(lamp_eyes::still::Still::default);
    let mut epoch = 0u64;
    let mut port: Option<LinuxPort> = None;
    let permit = lamp_eyes::permit::Permit::listen(permits)?;
    let mut retry_at = Instant::now();
    let mut frame = vec![0u8; MAX_FRAME_BYTES];
    // Bytes of a frame that has not been looked at yet.
    let mut latest: Option<usize> = None;
    let mut looked_at = Instant::now();
    let mut face_at: Option<Instant> = None;
    // When any face was last found, even one too small to follow: with
    // `--look-on-change`, a frame like the last needs no detector only
    // once nobody has been found for a while.
    let mut any_face_at: Option<Instant> = None;
    let mut change = arguments
        .look_on_change
        .then(lamp_eyes::change::Change::default);
    // Whether one face has sat still long enough to look half as often.
    let mut steady = lamp_eyes::steady::Steady::default();
    let (mut frames, mut looks, mut with_face, mut unreadable) = (0u32, 0u32, 0u32, 0u32);
    let mut spent = Duration::ZERO;
    // Looks the detector ran on, and its time: with `--look-on-change` the
    // 10 s line's "ms a look" counts the glances it skipped too.
    let (mut detected, mut detecting) = (0u32, Duration::ZERO);
    let mut reported_at = Instant::now();
    // How the camera is, for the lamp's health.
    let mut camera = lamp_eyes::bus_out::Camera::Closed;
    eprintln!("lamp-eyes: ready; the camera stays closed until it is permitted");
    loop {
        // A still is there for the one request it was taken for, and no
        // longer: first on every turn, before anything that skips the rest.
        if still_at
            .is_some_and(|at| at.elapsed() >= Duration::from_micros(lamp_eyes::full_look::KEPT_US))
        {
            still_at = None;
            if let Some(kept) = still_kept.as_mut() {
                kept.forget();
            }
        }
        // On every turn, whatever the camera is doing: that the eyes are
        // there, and how they are.
        if let Some(bus) = bus_out.as_mut() {
            let now = lamp_ipc::monotonic_us();
            bus.tick(now);
            bus.report(camera, now);
        }
        // What the voice service asks: one line. Remembering is answered
        // once the prints are taken.
        if let Some(who) = who.as_mut() {
            for _ in 0..4 {
                let Ok((mut stream, _)) = who.listener.accept() else {
                    break;
                };
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
                // One line, up to its newline or the caller's end.
                let mut line = Vec::new();
                let _ = std::io::BufRead::read_until(
                    &mut std::io::BufReader::new((&mut stream).take(1024)),
                    b'\n',
                    &mut line,
                );
                let reply = match ask_of(line.trim_ascii()) {
                    Some(Ask::Who) => match who
                        .known
                        .as_ref()
                        .filter(|known| known.2.elapsed() < KNOWN_FOR)
                    {
                        Some((name, likeness, _)) => {
                            serde_json::json!({"who": name, "likeness": likeness})
                        }
                        None => serde_json::json!({"who": null}),
                    },
                    Some(Ask::List) => serde_json::json!({"people": who.people.names()}),
                    Some(Ask::Forget(name)) => match who.people.forget(&name) {
                        Ok(forgot) => {
                            eprintln!("lamp-eyes: asked to forget a face; forgotten: {forgot}");
                            who.known = None;
                            serde_json::json!({"forgot": forgot})
                        }
                        Err(error) => {
                            eprintln!("lamp-eyes: could not forget a face: {error}");
                            serde_json::json!({"error": "storage"})
                        }
                    },
                    Some(Ask::Remember(_) | Ask::RememberOne { .. }) if who.taking.is_some() => {
                        serde_json::json!({"error": "busy"})
                    }
                    Some(Ask::Remember(_) | Ask::RememberOne { .. }) if port.is_none() => {
                        serde_json::json!({"error": "eyes_shut"})
                    }
                    Some(Ask::Remember(name)) => match plain_name(&name) {
                        Some(name) => {
                            eprintln!("lamp-eyes: asked to remember a face");
                            who.taking =
                                Some((Taking::new(name, lamp_ipc::monotonic_us()), stream));
                            continue;
                        }
                        None => serde_json::json!({"error": "not_a_name"}),
                    },
                    Some(Ask::RememberOne { name, replace }) => match plain_name(&name) {
                        Some(name) => {
                            eprintln!(
                                "lamp-eyes: asked to remember a face, one name for it{}",
                                if replace { ", in place of another" } else { "" }
                            );
                            let mut taking = Taking::new(name, lamp_ipc::monotonic_us());
                            taking.one_face = Some(replace);
                            who.taking = Some((taking, stream));
                            continue;
                        }
                        None => serde_json::json!({"error": "not_a_name"}),
                    },
                    None => serde_json::json!({"error": "not_understood"}),
                };
                answer(&mut stream, reply);
            }
            // Remembering ends with enough prints, or without them in time.
            if let Some((mut taking, mut stream)) = who.taking.take() {
                if let Some(prints) = taking.taken() {
                    let now_unix_s = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |since| since.as_secs());
                    let kept = match taking.one_face {
                        None => who
                            .people
                            .remember(&taking.name, prints, now_unix_s)
                            .map_err(|error| who::Refused::Store(error.to_string())),
                        Some(replace) => {
                            who.people
                                .remember_one(&taking.name, prints, now_unix_s, replace)
                        }
                    };
                    let reply = match kept {
                        Ok(name) => {
                            eprintln!(
                                "lamp-eyes: remembered a face; {} people known",
                                who.people.names().len()
                            );
                            serde_json::json!({"remembered": name})
                        }
                        // The face is known by another name: the voice asks
                        // the person before it is replaced.
                        Err(who::Refused::KnownAs(known)) => {
                            eprintln!(
                                "lamp-eyes: did not remember a face: it is known by another name"
                            );
                            serde_json::json!({"error": "known_as", "known_as": known})
                        }
                        Err(who::Refused::NotAName) => {
                            serde_json::json!({"error": "not_a_name"})
                        }
                        Err(who::Refused::Store(error)) => {
                            eprintln!("lamp-eyes: could not remember a face: {error}");
                            serde_json::json!({"error": "full"})
                        }
                    };
                    answer(&mut stream, reply);
                } else if let Some(why) = taking.given_up(lamp_ipc::monotonic_us()) {
                    eprintln!("lamp-eyes: did not remember a face: {}", why.code());
                    answer(&mut stream, serde_json::json!({"error": why.code()}));
                } else if port.is_none() {
                    answer(&mut stream, serde_json::json!({"error": "eyes_shut"}));
                } else {
                    who.taking = Some((taking, stream));
                }
            }
        }
        for datagram in permit.heard() {
            if arguments.full_look.is_some()
                && let Some(ask) = lamp_eyes::full_look::asked(&datagram)
            {
                if port.is_some() {
                    wanted = Some(lamp_eyes::full_look::Wanted::new(lamp_ipc::monotonic_us()));
                    wanted_ask = ask;
                    (head, joints_read_at) = (Head::Absent, None);
                } else {
                    eprintln!("lamp-eyes: a full look asked for with the camera shut");
                }
            }
        }
        if !permit.holds() {
            if let Some(mut open) = port.take() {
                let _ = open.stop(budget(STOP));
                eprintln!("lamp-eyes: camera closed: not permitted");
                tell(Seen { face: None });
            }
            camera = lamp_eyes::bus_out::Camera::Closed;
            lamp_eyes::kept::forget_all(&mut kept, &mut still_kept);
            still_at = None;
            wanted = None;
            if let Some(presence) = presence.as_mut() {
                presence.gone();
            }
            latest = None;
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        if port.is_none() {
            if Instant::now() < retry_at {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            epoch += 1;
            let config = LinuxConfig::new(
                arguments.device.clone(),
                arguments.lock_directory.clone(),
                source,
                UsbIdentity {
                    vendor: arguments.usb.vendor,
                    product: arguments.usb.product,
                    topology: arguments.usb.topology.clone(),
                    serial: None,
                    interface_number: 0,
                    capture_index: 0,
                },
            )
            .map_err(|error| format!("camera configuration: {error:?}"))?;
            let mut opening = LinuxPort::new(config);
            let (width, height) = opened_at(arguments.full_look, failures);
            let started = opening.start(
                CaptureConfig {
                    source,
                    width,
                    height,
                    interval: Some(
                        FrameInterval::new(1, 10).map_err(|error| format!("{error:?}"))?,
                    ),
                    max_frame_bytes: MAX_FRAME_BYTES,
                    buffers: 2,
                },
                CaptureId {
                    worker,
                    epoch: NonZeroU64::new(epoch).ok_or("no epoch")?,
                },
                // A camera closed a moment ago is slow to start (#911). The
                // permission is heard all the while, on a thread of its own,
                // and checked again as soon as the start returns.
                budget(OPEN_BUDGET_US),
            );
            let took_ms = opening.diagnostics().start.map_or(0, |timing| {
                timing
                    .completed_at
                    .as_micros()
                    .saturating_sub(timing.started_at.as_micros())
                    / 1_000
            });
            match started {
                Ok(mode) => {
                    // The open may outlast the permission, or it may be taken
                    // back meanwhile: then no frame is read and it closes now.
                    let Some(opening) = permit.opened(opening, |mut opening| {
                        let _ = opening.stop(budget(STOP));
                    }) else {
                        eprintln!(
                            "lamp-eyes: camera closed: not permitted after opening in {took_ms} ms"
                        );
                        continue;
                    };
                    eprintln!(
                        "lamp-eyes: camera open, {} by {}, in {took_ms} ms",
                        mode.width, mode.height
                    );
                    failures = 0;
                    if let Some(full) = arguments.full_look {
                        upgrade.opened((mode.width, mode.height) == full, lamp_ipc::monotonic_us());
                    }
                    port = Some(opening);
                    camera = lamp_eyes::bus_out::Camera::Open;
                    reported_at = Instant::now();
                }
                Err(error) => {
                    eprintln!(
                        "lamp-eyes: camera did not open: {error:?} {:?} after {took_ms} ms",
                        opening.diagnostics().fault
                    );
                    let _ = opening.stop(budget(STOP));
                    camera = lamp_eyes::bus_out::Camera::NotOpening;
                    // The full size first, then the two in turn.
                    failures = failures.saturating_add(1);
                    retry_at = Instant::now() + RETRY;
                    continue;
                }
            }
        }
        // Everything the camera has: only the newest frame is of interest.
        let mut failed = None;
        if let Some(open) = port.as_mut() {
            loop {
                match open.try_frame(&mut frame, budget(READ_BUDGET_US)) {
                    Ok(Some(got)) => {
                        frames += 1;
                        frames_all += 1;
                        newest = got.bytes_used.min(frame.len());
                        latest = Some(newest);
                    }
                    Ok(None) => break,
                    Err(error) => {
                        failed = Some(error);
                        break;
                    }
                }
            }
        }
        if let Some(error) = failed {
            eprintln!("lamp-eyes: camera failed: {error:?}; closing it");
            camera = lamp_eyes::bus_out::Camera::Failed;
            if let Some(mut open) = port.take() {
                let _ = open.stop(budget(STOP));
            }
            tell(Seen { face: None });
            lamp_eyes::kept::forget_all(&mut kept, &mut still_kept);
            still_at = None;
            if let Some(presence) = presence.as_mut() {
                presence.gone();
            }
            latest = None;
            wanted = None;
            retry_at = Instant::now() + RETRY;
            continue;
        }
        if let Some(asked) = wanted.as_mut() {
            let now_us = lamp_ipc::monotonic_us();
            // What the body says of the head, read twenty times a second
            // while the still waits for it.
            if let Some(bus) = joints_bus.as_ref()
                && joints_read_at.is_none_or(|at| at.elapsed() >= Duration::from_millis(50))
            {
                joints_read_at = Some(Instant::now());
                head = bus
                    .latest(lamp_msgs::JointState::TOPIC)
                    .ok()
                    .flatten()
                    .filter(|message| !message.expired)
                    .and_then(|message| {
                        let joints = message.parse::<lamp_msgs::JointState>().ok()?;
                        Some(lamp_eyes::full_look::head_of(
                            &joints,
                            message.header.at_unix_ms,
                            lamp_eyes::full_look::unix_ms(),
                            arguments.full_look_movement,
                        ))
                    })
                    .unwrap_or(Head::Absent);
            }
            asked.body(head, now_us, frames_all);
            if asked.take(frames_all) && newest > 0 {
                let made = Instant::now();
                let waited_ms = asked.waited_us().unwrap_or(0) / 1_000;
                match lamp_eyes::full_look::fit_for(&frame[..newest], wanted_ask) {
                    Ok((still, size)) => {
                        // The ask's id goes with the picture, so that a
                        // late still is never taken for a later ask.
                        let still = lamp_eyes::full_look::tagged(&still, wanted_ask.id);
                        let written = still_kept.as_mut().map(|kept| kept.keep(&still));
                        eprintln!(
                            "lamp-eyes: a full look: {} by {}, {} bytes, the head waited for {waited_ms} ms, made in {} ms{}",
                            size.0,
                            size.1,
                            still.len(),
                            made.elapsed().as_millis(),
                            match written {
                                Some(Ok(())) => String::new(),
                                _ => ", not written".to_owned(),
                            }
                        );
                        still_at = Some(Instant::now());
                    }
                    Err(error) => eprintln!("lamp-eyes: no full look: {error}"),
                }
                wanted = None;
            } else if asked.given_up(now_us) {
                eprintln!("lamp-eyes: no full look: no frame in time");
                wanted = None;
            }
        }
        // Streaming at the working size for want of the full one: try the
        // full size again once nobody is in view and no look is asked for.
        if arguments.full_look.is_some()
            && upgrade.due(
                lamp_ipc::monotonic_us(),
                face_at.is_some_and(|at| at.elapsed() < FOLLOW_FOR),
                wanted.is_some(),
            )
            && let Some(mut open) = port.take()
        {
            let tries = upgrade.tried();
            eprintln!(
                "lamp-eyes: closing the camera to try the full size again (try {tries}); at the working size if it does not open"
            );
            // Stopped with the STOP budget, as every close is (#921): the
            // budget is checked around the driver's blocking calls, so a
            // stuck ioctl or a failed STREAMOFF can still hold the buffers
            // (docs/camera-runtime.md). The reopen at the full size comes
            // next, through the same `permit.opened` as any open.
            let _ = open.stop(budget(STOP));
            camera = lamp_eyes::bus_out::Camera::Closed;
            lamp_eyes::kept::forget_all(&mut kept, &mut still_kept);
            still_at = None;
            wanted = None;
            latest = None;
            // One try at the full size, then the two in turn.
            failures = lamp_eyes::full_look::FULL_FIRST - 1;
            retry_at = Instant::now();
            continue;
        }
        // A full look or remember under way wants every look it can have.
        let halved = arguments.steady_rate
            && steady.halved(
                wanted.is_some() || who.as_ref().is_some_and(|who| who.taking.is_some()),
                lamp_ipc::monotonic_us(),
            );
        let every = if halved {
            STEADY_EVERY
        } else if face_at.is_some_and(|at| at.elapsed() < FOLLOW_FOR) {
            LOOK_EVERY
        } else {
            GLANCE_EVERY
        };
        if let Some(used) = latest
            && looked_at.elapsed() >= every
        {
            latest = None;
            looked_at = Instant::now();
            // What the lamp sees now, for the voice service; a frame that is
            // not whole leaves the one before it.
            // With a full look the frame is larger: it is kept shrunk to
            // 320 across, and a still asked for goes to its own file.
            if let Some(kept) = kept.as_mut() {
                if arguments.full_look.is_none() {
                    let _ = kept.keep(&frame[..used]);
                } else if let Ok(working) = lamp_eyes::full_look::working(&frame[..used]) {
                    let _ = kept.keep(&working);
                }
            }
            let empty_room = lamp_eyes::change::empty_room(
                wanted.is_some(),
                who.as_ref().is_some_and(|who| who.taking.is_some()),
                any_face_at.map(|at| at.elapsed().as_micros() as u64),
            );
            let found = Picture::from_jpeg(&frame[..used]).and_then(|picture| {
                let detect = lamp_eyes::change::detect(
                    change.as_mut(),
                    &picture,
                    empty_room,
                    lamp_ipc::monotonic_us(),
                );
                // A frame like the last one the detector ran on, with
                // nobody in it then: nobody in it now either.
                let faces = if detect {
                    let began = Instant::now();
                    let faces = detector.find(&picture);
                    (detected, detecting) = (detected + 1, detecting + began.elapsed());
                    faces
                } else {
                    Ok(Vec::new())
                };
                faces
                    .map(|mut faces| {
                        if let Some(still) = still.as_mut() {
                            let now_us = lamp_ipc::monotonic_us();
                            faces.retain(|face| {
                                !still.leave_out(
                                    lamp_eyes::bus_out::edges(face, picture.filled),
                                    face.score,
                                    now_us,
                                )
                            });
                        }
                        if !faces.is_empty() {
                            any_face_at = Some(Instant::now());
                        }
                        if let Some(bus) = bus_out.as_mut() {
                            bus.saw(&faces, picture.filled, lamp_ipc::monotonic_us());
                        }
                        // Whose the near face is, when it is time to ask or
                        // someone is being remembered.
                        if let Some(who) = who.as_mut()
                            && (who.taking.is_some()
                                || who
                                    .looked_for
                                    .is_none_or(|at| at.elapsed() >= RECOGNISE_EVERY))
                        {
                            who.looked_for = Some(Instant::now());
                            let tall = picture.filled.1.max(1) as f32;
                            let heights: Vec<f32> =
                                faces.iter().map(|face| face.height / tall).collect();
                            let print = faces
                                .iter()
                                .zip(&heights)
                                .filter(|(_, height)| **height >= NEAR_ENOUGH)
                                .max_by(|a, b| a.1.total_cmp(b.1))
                                .and_then(|(face, _)| align(&picture, &face.marks))
                                .and_then(|aligned| who.recogniser.print(&aligned).ok());
                            if let Some((taking, _)) = who.taking.as_mut() {
                                taking.look(&heights, print);
                            } else if let Some(print) = print {
                                who.known = who
                                    .people
                                    .who(&print)
                                    .map(|(name, likeness)| (name, likeness, Instant::now()));
                                if let Some(path) = &arguments.known_file {
                                    keep_known(
                                        path,
                                        who.known.as_ref().map(|known| known.0.as_str()),
                                    );
                                }
                            } else if let Some(path) = &arguments.known_file {
                                // Nobody near: nobody known at the lamp.
                                keep_known(path, None);
                            }
                        }
                        let sight = choose(&faces, picture.filled);
                        let tall = picture.filled.1.max(1) as f32;
                        steady.look(
                            faces
                                .iter()
                                .filter(|face| face.height / tall >= lamp_eyes::sight::LEAST_SIZE)
                                .count(),
                            sight,
                            who.is_some(),
                            who.as_ref()
                                .and_then(|who| who.known.as_ref())
                                .filter(|known| known.2.elapsed() < KNOWN_FOR)
                                .map(|known| known.0.as_str()),
                            lamp_ipc::monotonic_us(),
                        );
                        sight
                    })
                    .map_err(|error| error.to_string())
            });
            spent += looked_at.elapsed();
            match found {
                Ok(face) => {
                    looks += 1;
                    with_face += u32::from(face.is_some());
                    if let Some(face) = face.as_ref() {
                        face_at = Some(Instant::now());
                        if let Some(presence) = presence.as_mut() {
                            let _ = presence.there(face.size);
                        }
                    }
                    tell(Seen { face });
                }
                Err(_) => unreadable += 1,
            }
        }
        if reported_at.elapsed() >= REPORT_EVERY {
            eprintln!(
                "lamp-eyes: {frames} frames, looked at {looks}, a face in {with_face}, {unreadable} unreadable, {:.0} ms a look{}{}{}",
                spent.as_secs_f32() * 1e3 / (looks + unreadable).max(1) as f32,
                if halved { ", steady" } else { "" },
                change.as_mut().map_or(String::new(), |change| format!(
                    ", {} of them unchanged and not detected, {:.0} ms a detected look",
                    change.take_skipped(),
                    detecting.as_secs_f32() * 1e3 / detected.max(1) as f32
                )),
                still.as_ref().map_or(String::new(), |still| format!(
                    ", {} still faces left out so far",
                    still.left_out
                ))
            );
            (frames, looks, with_face, unreadable) = (0, 0, 0, 0);
            spent = Duration::ZERO;
            (detected, detecting) = (0, Duration::ZERO);
            reported_at = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    const FULL: [&str; 12] = [
        "--device",
        "/dev/video0",
        "--lock-directory",
        "/home/test/private-camera-locks",
        "--usb",
        "01da:5875@3-1.1.4",
        "--model",
        "/opt/lampOS/eyes/face_detection_yunet_2023mar.onnx",
        "--permit-socket",
        "/run/lampos-eyes/permit.sock",
        "--body-socket",
        "/run/lampos-body/gaze.sock",
    ];

    #[test]
    fn faces_are_remembered_only_with_all_three_paths_and_the_known_file_needs_them() {
        let who = [
            "--recogniser",
            "/opt/lampOS/eyes/face_recognition_sface_2021dec.onnx",
            "--people",
            "/var/lib/lampos-eyes/people.json",
            "--who-socket",
            "/run/lampos-eyes/who.sock",
        ];
        assert_eq!(parse(args(&FULL)).unwrap().unwrap().who, None);
        let value = parse(args(&[&FULL[..], &who[..]].concat()))
            .unwrap()
            .unwrap();
        assert_eq!(
            value.who,
            Some((
                PathBuf::from(who[1]),
                PathBuf::from(who[3]),
                PathBuf::from(who[5])
            ))
        );
        assert_eq!(value.known_file, None);
        let known = ["--known-file", "/run/lampos-eyes/known"];
        assert_eq!(
            parse(args(&[&FULL[..], &who[..], &known[..]].concat()))
                .unwrap()
                .unwrap()
                .known_file,
            Some(PathBuf::from(known[1]))
        );
        assert!(parse(args(&[&FULL[..], &known[..]].concat())).is_err());
        for missing in 0..3 {
            let mut some = who.to_vec();
            some.drain(missing * 2..missing * 2 + 2);
            assert!(parse(args(&[&FULL[..], &some[..]].concat())).is_err());
        }
        assert!(
            parse(args(
                &[&FULL[..], &who[..4], &["--who-socket", "who.sock"]].concat()
            ))
            .is_err()
        );
    }

    #[test]
    fn every_path_and_the_camera_are_named_and_nothing_else_is_accepted() {
        let value = parse(args(&FULL)).unwrap().unwrap();
        assert_eq!(
            value.usb,
            Usb {
                vendor: 0x01da,
                product: 0x5875,
                topology: "3-1.1.4".into()
            }
        );
        assert_eq!(
            value.body_socket,
            PathBuf::from("/run/lampos-body/gaze.sock")
        );
        assert!(parse(args(&["--help"])).unwrap().is_none());
        // A face that never moves is kept unless asked otherwise.
        assert!(!value.leave_still);
        let leaving = [&FULL[..], &["--leave-still-faces", "on"]].concat();
        assert!(parse(args(&leaving)).unwrap().unwrap().leave_still);
        let keeping = [&FULL[..], &["--leave-still-faces", "off"]].concat();
        assert!(!parse(args(&keeping)).unwrap().unwrap().leave_still);
        assert!(parse(args(&[&FULL[..], &["--leave-still-faces", "yes"]].concat())).is_err());
        // Four looks a second stay unless the steady rate is asked for.
        assert!(!value.steady_rate);
        let halving = [&FULL[..], &["--steady-rate", "on"]].concat();
        assert!(parse(args(&halving)).unwrap().unwrap().steady_rate);
        let keeping = [&FULL[..], &["--steady-rate", "off"]].concat();
        assert!(!parse(args(&keeping)).unwrap().unwrap().steady_rate);
        assert!(parse(args(&[&FULL[..], &["--steady-rate", "yes"]].concat())).is_err());
        // Every glance is detected unless looking on change is asked for.
        assert!(!value.look_on_change);
        let on = [&FULL[..], &["--look-on-change", "on"]].concat();
        assert!(parse(args(&on)).unwrap().unwrap().look_on_change);
        let off = [&FULL[..], &["--look-on-change", "off"]].concat();
        assert!(!parse(args(&off)).unwrap().unwrap().look_on_change);
        assert!(parse(args(&[&FULL[..], &["--look-on-change", "yes"]].concat())).is_err());
        let twice = [&on[..], &["--look-on-change", "on"]].concat();
        assert!(parse(args(&twice)).is_err());
        // The newest frame is kept for the voice service only when asked.
        assert_eq!(value.sight_file, None);
        let seeing = [&FULL[..], &["--sight-file", "/run/lampos-eyes/sight.jpg"]].concat();
        assert_eq!(
            parse(args(&seeing)).unwrap().unwrap().sight_file,
            Some(PathBuf::from("/run/lampos-eyes/sight.jpg"))
        );
        assert!(parse(args(&[&FULL[..], &["--sight-file", "sight.jpg"]].concat())).is_err());
        assert!(
            parse(args(
                &[&seeing[..], &["--sight-file", "/run/x.jpg"]].concat()
            ))
            .is_err()
        );
        // And the one line that says someone is there.
        assert_eq!(value.presence_file, None);
        let present = [&FULL[..], &["--presence-file", "/run/lampos-eyes/present"]].concat();
        assert_eq!(
            parse(args(&present)).unwrap().unwrap().presence_file,
            Some(PathBuf::from("/run/lampos-eyes/present"))
        );
        assert!(parse(args(&[&FULL[..], &["--presence-file", "present"]].concat())).is_err());
        for missing in 0..6 {
            let mut some = FULL.to_vec();
            some.drain(missing * 2..missing * 2 + 2);
            assert!(parse(args(&some)).is_err());
        }
        assert!(parse(args(&[&FULL[..], &["--save-frames", "/tmp"]].concat())).is_err());
        assert!(
            parse(args(
                &[&FULL[..10], &["--body-socket", "gaze.sock"]].concat()
            ))
            .is_err()
        );
        assert!(
            parse(args(
                &[&FULL[..10], &["--body-socket", "/run/../etc/x"]].concat()
            ))
            .is_err()
        );
        for refused in [
            "01da:5875",
            "1da:5875@3-1",
            "0000:5875@3-1",
            "01da:5875@",
            "01da:5875@3-1;x",
            "zzzz:5875@3-1",
        ] {
            assert_eq!(usb_of(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_full_look_is_off_unless_given_a_size_and_its_own_file() {
        // Without it the camera is opened at 320 by 240 as before.
        let without = parse(args(&FULL)).unwrap().unwrap();
        assert_eq!((without.full_look, without.full_look_file), (None, None));
        let sight = [
            "--sight-file",
            "/run/lampos-eyes/sight.jpg",
            "--full-look-file",
            "/run/lampos-eyes/full-look.jpg",
        ];
        let full = [&FULL[..], &sight[..], &["--full-look", "1280x960"]].concat();
        let with = parse(args(&full)).unwrap().unwrap();
        assert_eq!(with.full_look, Some((1_280, 960)));
        // Its head settles on movement only when asked (#842).
        assert!(!with.full_look_movement);
        let moving = [&full[..], &["--full-look-movement", "on"]].concat();
        assert!(parse(args(&moving)).unwrap().unwrap().full_look_movement);
        let bad = [&full[..], &["--full-look-movement", "yes"]].concat();
        assert!(parse(args(&bad)).is_err());
        // The still has its own file: the working frame stays in the
        // sight file for whoever reads it at any time (#873).
        assert_eq!(
            with.full_look_file,
            Some(PathBuf::from("/run/lampos-eyes/full-look.jpg"))
        );
        assert_eq!(
            with.sight_file,
            Some(PathBuf::from("/run/lampos-eyes/sight.jpg"))
        );
        // One without the other, or the still in the sight file: refused.
        assert!(parse(args(&[&FULL[..], &["--full-look", "1280x960"]].concat())).is_err());
        assert!(parse(args(&[&FULL[..], &sight[..]].concat())).is_err());
        let same = [
            "--sight-file",
            "/run/lampos-eyes/sight.jpg",
            "--full-look-file",
            "/run/lampos-eyes/sight.jpg",
            "--full-look",
            "1280x960",
        ];
        assert!(parse(args(&[&FULL[..], &same[..]].concat())).is_err());
        // Without a sight file the full look is still taken; no working
        // frame is kept, as without the full look.
        let alone = [
            "--full-look-file",
            "/run/lampos-eyes/full-look.jpg",
            "--full-look",
            "1280x960",
        ];
        assert!(parse(args(&[&FULL[..], &alone[..]].concat())).is_ok());
        for refused in [
            "1280",
            "1280x",
            "x960",
            "160x120",
            "8192x6144",
            "1281x960",
            "3266x2448",
            "3264x2450",
            "big",
        ] {
            assert!(
                parse(args(
                    &[&FULL[..], &sight[..], &["--full-look", refused]].concat()
                ))
                .is_err(),
                "{refused}"
            );
        }
        assert!(parse(args(&[&full[..], &["--full-look", "640x480"]].concat())).is_err());
        // Its ask is not a permission, and says nothing to the camera.
        assert_eq!(lamp_eyes::permit::of(br#"{"look_full":7}"#), None);
    }

    /// The largest full look is the largest size the camera opens (#836: at
    /// 1280x960 against a bound of 720 it never opened on lamp-52e6).
    #[cfg(target_os = "linux")]
    #[test]
    fn the_largest_full_look_is_the_camera_s_largest_size() {
        let (width, height) = (lamp_camera::MAX_WIDTH, lamp_camera::MAX_HEIGHT);
        assert_eq!(size_of(&format!("{width}x{height}")), Some((width, height)));
        assert_eq!(size_of(&format!("{}x{height}", width + 2)), None);
        assert_eq!(size_of(&format!("{width}x{}", height + 2)), None);
    }

    #[test]
    fn without_a_full_look_the_camera_opens_as_before() {
        let without = parse(args(&FULL)).unwrap().unwrap().full_look;
        // Whatever the last open did.
        assert_eq!(opened_at(without, 0), (320, 240));
        assert_eq!(opened_at(without, 7), (320, 240));
        let with = Some((1_280, 960));
        assert_eq!(opened_at(with, 0), (1_280, 960));
        // After failing at the full size, the working size once.
        assert_eq!(
            opened_at(with, lamp_eyes::full_look::FULL_FIRST),
            (320, 240)
        );
    }
}
