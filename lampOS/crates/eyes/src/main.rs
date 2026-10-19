//! The lamp's eyes: one process that owns the camera, finds a face in what
//! it sees and tells the body where it is. It opens the camera only while
//! the voice service keeps saying it may, and sends nothing but the face's
//! place in the picture. With `--sight-file` it also keeps its newest frame,
//! that one only, in a private file for the voice service, and with
//! `--full-look-file` one still the voice service asked for, in another.

use std::{ffi::OsString, path::PathBuf};

const USAGE: &str = "lamp-eyes --device ABSOLUTE_PATH --lock-directory PRIVATE_ABSOLUTE_DIRECTORY --usb VENDOR:PRODUCT@TOPOLOGY --model ABSOLUTE_PATH --permit-socket ABSOLUTE_PATH --body-socket ABSOLUTE_PATH [--sight-file ABSOLUTE_PATH] [--presence-file ABSOLUTE_PATH] [--recogniser ABSOLUTE_PATH --people ABSOLUTE_PATH --who-socket ABSOLUTE_PATH [--known-file ABSOLUTE_PATH] [--visits on|off]] [--leave-still-faces on|off] [--steady-rate on|off] [--look-on-change on|off] [--full-look WIDTHxHEIGHT --full-look-file ABSOLUTE_PATH [--full-look-movement on|off]] [--mouth-model ABSOLUTE_PATH [--mouth-on-heard on|off] [--mouth-runtime ABSOLUTE_PATH]] [--seems ABSOLUTE_PATH] [--photo-dir ABSOLUTE_PATH]\n\nOwns the camera at --device (checked against the USB identity given, hex vendor and\nproduct and the port topology, for example 01da:5875@3-1.1.4) and looks for a face in\nits frames four times a second while there is one and less than twice a second when\nthere is none, with the pinned YuNet model at --model. With --steady-rate on, it looks\ntwice a second while one face has sat still for 3 s (off unless given). With\n--look-on-change on, once no face has been found for 4 s, a frame like the last one the\ndetector ran on is said to hold no face without it, and the detector runs at least every\n3 s (off unless given). Each look\nsends one datagram to --body-socket: {\"face\":{\"x\":..,\"y\":..,\"size\":..,\"turned\":..}} or\n{\"face\":null}. The camera is open only while {\"camera\":true} keeps arriving on\n--permit-socket, at least every 1.5 s; {\"camera\":false} closes it at once. Without\n--sight-file and --full-look-file no frame is written anywhere and none leaves this process. With it the\nnewest frame, and only that one, is kept in that file (mode 0600) while the camera is\nopen and removed when it closes: it is what the voice service shows the model. With\n--presence-file one line, the near face's height as a share of the picture's, is kept\nfresh in that file while there is a face: no picture, only that number. With\n--recogniser (the pinned SFace model), --people and --who-socket, the eyes remember\na face when the voice service asks ({\"remember\":\"NAME\"} on the socket), forget it\n({\"forget\":\"NAME\"}), list the names ({\"list\":true}) and say whose face is at the\nlamp ({\"who\":true}). Only the prints of people who asked are kept, in --people\n(mode 0600); no picture. With --known-file, the name of the one known at the lamp is\nkept fresh in that file while they are there. With --visits on, each remembered person\nhas one familiarity number, rising once a day they are seen and halving every 14 days,\nwith no date or time of a visit kept, and with LAMP_BUS=on the level of the one at the\nlamp is said on humans/familiar, with no name (off unless given). With --leave-still-faces on, a face at a place\nwhere faces have come from for half an hour, every one small (under 0.14 of the height)\nand faint (scored under 0.8), is not a person: it is left out of what the body is told,\nthe bus and recognition (off unless given). With --full-look and --full-look-file the\ncamera runs at that size, the working frame is kept in --sight-file at 320 across as before, and {\"look_full\":ID} on\n--permit-socket asks for one still: taken once the head has settled\n(body/joint_states on the bus, at most 2 s), at most 1024 across and 64 KiB, or with\n{\"look_full\":ID,\"side\":PIXELS} at most that side across and 184 KiB, marked\nwith the ID in a JPEG comment, kept in --full-look-file for 15 s and removed (off unless given).\nWith --full-look-movement on the head settles on movement only: the camera's aim, the arm's\njoints with the head's, and never a joint's distance to its goal (off unless given).\nWith --mouth-model (the pinned Face Mesh V2), the journal says how open the largest\nface's mouth is each look, and with LAMP_EYES_SPEAKING=on that face's speaking reads it\n(off unless given). With --mouth-on-heard on, the mesh runs only while the voice hears a\nline (voice/heard on the bus, from its first words until 2 s after it ends).\nWith --mouth-runtime (the voice's pinned ONNX Runtime library, checked before it is loaded),\nthe mesh runs under ONNX Runtime on one thread instead of tract.\nWith --seems (the pinned FER+ expression model), how the near face seems is read at\nmost every 2 s, as positive, negative or unsure with a confidence, and with LAMP_BUS=on\nsaid on humans/seems, with no name and no picture; the journal says only how many reads\nthere were, how long one took and the longest, once a minute (off unless given; #1671).\nWith --photo-dir (the eyes' own folder, with --recogniser), {\"remember\":\"NAME\",\"photo\":\"FILE\"}\non the who socket takes the one face in that JPEG in the folder and removes the file once\nread (no picture is kept); the face is kept unconfirmed until its person says yes at the\nlamp, and deleted if not (off unless given).\n";

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
    /// How often each remembered person comes by is counted, and the level
    /// of the one at the lamp said (`--visits on`, #1670). Off unless set.
    visits: bool,
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
    /// Face Mesh V2, to measure how open the largest face's mouth is a
    /// look (#720). Nothing without it.
    mouth_model: Option<PathBuf>,
    /// The mesh only while the voice hears a line (#705's heat).
    mouth_on_heard: bool,
    /// ONNX Runtime for the mesh, the voice's pinned library, instead of tract.
    mouth_runtime: Option<PathBuf>,
    /// The eyes' photo folder: a face is remembered from a photo put there
    /// (`{"remember":NAME,"photo":FILE}`), the file removed once read
    /// (#1722, faces from a photo). Nothing without it.
    photo_dir: Option<PathBuf>,
    /// The pinned FER+ model: how the near face seems is read at most every
    /// 2 s and, with LAMP_BUS=on, said on `humans/seems` (#1671). Nothing
    /// without it.
    seems: Option<PathBuf>,
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
    let mut visits = None;
    let (mut mouth_model, mut mouth_on_heard, mut mouth_runtime) = (None, None, None);
    let mut seems = None;
    let mut photo_dir = None;
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
                && visits.is_none()
                && look_on_change.is_none()
                && full_look_movement.is_none()
                && full_look.is_none()
                && full_look_file.is_none()
                && mouth_model.is_none()
                && mouth_on_heard.is_none()
                && mouth_runtime.is_none()
                && seems.is_none()
                && photo_dir.is_none()
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
            Some("--visits") if visits.is_none() => {
                visits = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--visits is on or off"),
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
            Some("--mouth-model") if mouth_model.is_none() => {
                mouth_model = Some(PathBuf::from(value));
            }
            Some("--mouth-runtime") if mouth_runtime.is_none() => {
                mouth_runtime = Some(PathBuf::from(value));
            }
            Some("--seems") if seems.is_none() => seems = Some(PathBuf::from(value)),
            Some("--photo-dir") if photo_dir.is_none() => photo_dir = Some(PathBuf::from(value)),
            Some("--mouth-on-heard") if mouth_on_heard.is_none() => {
                mouth_on_heard = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--mouth-on-heard is on or off"),
                });
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
        visits: visits.unwrap_or(false),
        look_on_change: look_on_change.unwrap_or(false),
        full_look,
        full_look_file,
        full_look_movement: full_look_movement.unwrap_or(false),
        mouth_model,
        mouth_on_heard: mouth_on_heard.unwrap_or(false),
        mouth_runtime,
        seems,
        photo_dir,
    };
    if result.full_look.is_some() != result.full_look_file.is_some()
        || (result.full_look_file.is_some() && result.full_look_file == result.sight_file)
    {
        return Err("--full-look and --full-look-file go together, apart from --sight-file");
    }
    if result.known_file.is_some() && result.who.is_none() {
        return Err("--known-file needs --recogniser, --people and --who-socket");
    }
    if result.visits && result.who.is_none() {
        return Err("--visits needs --recogniser, --people and --who-socket");
    }
    if result.photo_dir.is_some() && result.who.is_none() {
        return Err("--photo-dir needs --recogniser, --people and --who-socket");
    }
    if result.mouth_on_heard && result.mouth_model.is_none() {
        return Err("--mouth-on-heard needs --mouth-model");
    }
    if result.mouth_runtime.is_some() && result.mouth_model.is_none() {
        return Err("--mouth-runtime needs --mouth-model");
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
    .chain(&result.mouth_model)
    .chain(&result.mouth_runtime)
    .chain(&result.seems)
    .chain(&result.photo_dir)
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
        /// How often each remembered person comes by is counted
        /// (`--visits on`, #1670).
        visits: bool,
        /// When the day's visits were last looked at for writing.
        scored_looked: Option<Instant>,
        /// The lamp's zone, for the hour the day's visits are written.
        zone: Option<lamp_local_time::LocalTime>,
        /// With `--photo-dir`: the faces added from photos, unconfirmed
        /// until their person says yes, in their own file (#1968).
        unconfirmed: Option<lamp_eyes::photo::Unconfirmeds>,
        /// The unconfirmed face at the lamp now, and when it was last seen.
        photo_here: Option<(String, Instant)>,
        /// When each unconfirmed face was last seen, monotonic, for the end
        /// of a meeting with no answer.
        photo_seen: Vec<(String, u64)>,
        /// When the faces from photos were last looked at for expiry and
        /// meetings with no answer.
        photos_looked: Option<Instant>,
    }
    /// Counts the remembered person recognised at the lamp, at most once in
    /// sixteen hours, and says how familiar they are (#1670). A count that
    /// cannot be written is said and the level read as it was.
    fn familiar(
        people: &mut People,
        known: Option<&(String, f32, Instant)>,
    ) -> Option<lamp_msgs::FamiliarLevel> {
        let (name, _, _) = known?;
        let now_unix_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        people.seen(name, now_unix_s);
        people.familiar(name, now_unix_s).map(|level| match level {
            lamp_eyes::who::Familiar::New => lamp_msgs::FamiliarLevel::New,
            lamp_eyes::who::Familiar::Regular => lamp_msgs::FamiliarLevel::Regular,
            lamp_eyes::who::Familiar::Close => lamp_msgs::FamiliarLevel::Close,
        })
    }
    // What a remembering answers, said in the journal with no name: the
    // name kept, or why not.
    fn kept_reply(
        kept: std::result::Result<String, who::Refused>,
        visits: bool,
        known: usize,
    ) -> serde_json::Value {
        match kept {
            Ok(name) => {
                eprintln!("lamp-eyes: remembered a face; {known} people known");
                // With visits counted, the voice tells them so in one line
                // (the first builder on #1670, 12:19).
                if visits {
                    serde_json::json!({"remembered": name, "visits": true})
                } else {
                    serde_json::json!({"remembered": name})
                }
            }
            // The face is known by another name: the voice asks the person
            // before it is replaced.
            Err(who::Refused::KnownAs(known)) => {
                eprintln!("lamp-eyes: did not remember a face: it is known by another name");
                serde_json::json!({"error": "known_as", "known_as": known})
            }
            Err(who::Refused::NotAName) => serde_json::json!({"error": "not_a_name"}),
            Err(who::Refused::Store(error)) => {
                eprintln!("lamp-eyes: could not remember a face: {error}");
                serde_json::json!({"error": "full"})
            }
        }
    }
    fn answer(stream: &mut UnixStream, value: serde_json::Value) {
        // A caller that is gone or slow changes nothing here.
        let _ = writeln!(stream, "{value}");
    }

    // The photo folder, the eyes' own, before anything is asked of it.
    if let Some(dir) = &arguments.photo_dir {
        lamp_eyes::photo::ready(dir)?;
    }
    let detector = Detector::load(&arguments.model)?;
    let mut mesh = match (&arguments.mouth_model, &arguments.mouth_runtime) {
        (Some(model), Some(runtime)) => {
            Some(lamp_eyes::mesh::Mesh::load_with_runtime(model, runtime)?)
        }
        (Some(model), None) => Some(lamp_eyes::mesh::Mesh::load(model)?),
        (None, _) => None,
    };
    // With --seems, how the near face seems (#1671): the reads, and how
    // many there were and how long they took, for the journal once a minute.
    struct Seems {
        reader: lamp_eyes::seems::Reader,
        read_at: Option<Instant>,
        reads: u32,
        spent: Duration,
        /// The longest read: the most one look was held back by it.
        longest: Duration,
        told_at: Instant,
    }
    let mut seems = match &arguments.seems {
        Some(model) => Some(Seems {
            reader: lamp_eyes::seems::Reader::load(model)?,
            read_at: None,
            reads: 0,
            spent: Duration::ZERO,
            longest: Duration::ZERO,
            told_at: Instant::now(),
        }),
        None => None,
    };
    // With --mouth-on-heard on, the voice's lines (`voice/heard`), so the
    // mesh runs only while one is heard.
    let mut heard_lines = if arguments.mouth_on_heard {
        Some(lamp_bus::Bus::from_environment()?.subscribe_typed(
            lamp_msgs::Speech::TOPICS[0],
            "eyes_mouth",
            lamp_msgs::Speech::TYPE,
        )?)
    } else {
        None
    };
    let mut heard = lamp_eyes::mesh::Heard::default();
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
                visits: arguments.visits,
                scored_looked: None,
                zone: arguments
                    .visits
                    .then(|| match lamp_local_time::LocalTime::zone("system") {
                        Ok(zone) => Some(zone),
                        Err(error) => {
                            eprintln!(
                                "lamp-eyes: the lamp's zone can't be read ({error}); visits are written at 03:00 UTC"
                            );
                            None
                        }
                    })
                    .flatten(),
                unconfirmed: arguments.photo_dir.as_ref().map(|_| {
                    lamp_eyes::photo::Unconfirmeds::open(
                        lamp_eyes::photo::Unconfirmeds::beside(people_path),
                    )
                }),
                photo_here: None,
                photo_seen: Vec::new(),
                photos_looked: None,
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
    let mut bus_out =
        lamp_eyes::bus_out::BusOut::from_environment(arguments.visits, arguments.seems.is_some());
    if who.is_some()
        && let Some(bus) = bus_out.as_mut()
    {
        bus.activity_by_face();
    }
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
    // With it, the lamp's own light, read from the bus at each glance: a
    // change while the ring's light changes is the lamp's own.
    let own_bus = arguments
        .look_on_change
        .then(|| lamp_bus::Bus::from_environment().ok())
        .flatten();
    let mut own_light = lamp_eyes::change::OwnLight::default();
    // Whether one face has sat still long enough to look half as often.
    let mut steady = lamp_eyes::steady::Steady::default();
    let (mut frames, mut looks, mut with_face, mut unreadable) = (0u32, 0u32, 0u32, 0u32);
    let mut spent = Duration::ZERO;
    // Looks the detector ran on, and its time: with `--look-on-change` the
    // 10 s line's "ms a look" counts the glances it skipped too.
    let (mut detected, mut detecting) = (0u32, Duration::ZERO);
    // With `--look-on-change`, where a look's time goes (the sight file,
    // the decode, the grid, the detector is `detecting`, and the rest), and
    // the looks that found faces but none to follow (#1441 on lamp-4ace:
    // half a look was not the detector, and almost no glance was skipped).
    let mut stages = [Duration::ZERO; 3];
    let mut unfollowed = 0u32;
    let mut reported_at = Instant::now();
    // How the camera is, for the lamp's health.
    let mut camera = lamp_eyes::bus_out::Camera::Closed;
    // How many people's records are set aside (#1505), and when last counted.
    const SET_ASIDE_EVERY: Duration = Duration::from_secs(60);
    let mut aside = 0;
    let mut aside_counted: Option<Instant> = None;
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
            // People's records set aside (#1505), counted once a minute: a
            // reset empties the folder, and a person may clear it by hand.
            if let Some((_, people_path, _)) = &arguments.who
                && aside_counted.is_none_or(|at| at.elapsed() >= SET_ASIDE_EVERY)
            {
                aside = lamp_eyes::who::set_aside_count(people_path);
                aside_counted = Some(Instant::now());
            }
            bus.report(camera, aside, now);
        }
        // Visits counted today reach the file once a UTC day begins, whoever
        // is at the lamp (#1670), looked at once a minute.
        if let Some(who) = who.as_mut()
            && who.visits
            && who
                .scored_looked
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(60))
        {
            who.scored_looked = Some(Instant::now());
            let now_unix_s = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            // At 03:00 in the lamp's own zone, followed as it changes; a
            // zone that can't be read is UTC, said once.
            let offset_s = match who.zone.as_ref() {
                Some(zone) => i64::from(zone.offset_minutes_at(now_unix_s * 1_000)) * 60,
                None => 0,
            };
            if let Err(error) = who.people.score(now_unix_s, offset_s) {
                eprintln!("lamp-eyes: the day's visits were not written: {error}");
            }
        }
        // Faces from photos, once a second: one asked with no answer by the
        // end of the meeting is let go, and one never confirmed in 30 days
        // is deleted (#1968).
        if let Some(who) = who.as_mut()
            && who
                .photos_looked
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
            && let Some(unconfirmed) = who.unconfirmed.as_mut()
        {
            who.photos_looked = Some(Instant::now());
            let now_unix_s = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            match unconfirmed.expire(now_unix_s) {
                Ok(0) => {}
                Ok(gone) => eprintln!(
                    "lamp-eyes: {gone} faces from photos were deleted: never confirmed in 30 days"
                ),
                Err(error) => eprintln!("lamp-eyes: faces from photos not expired: {error}"),
            }
            let unanswered = lamp_eyes::photo::unanswered(
                &unconfirmed.asked_names(),
                &who.photo_seen,
                lamp_ipc::monotonic_us(),
            );
            for name in unanswered {
                if unconfirmed.let_go(&name).unwrap_or(false) {
                    eprintln!(
                        "lamp-eyes: a face from a photo was let go: no answer in its meeting"
                    );
                }
                who.photo_seen.retain(|(seen, _)| seen != &name);
            }
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
                    Some(Ask::Forget(name)) => {
                        // "Forget my face" takes a face from a photo too.
                        if let Some(unconfirmed) = who.unconfirmed.as_mut() {
                            let _ = unconfirmed.let_go(&name);
                        }
                        match who.people.forget(&name) {
                            // Set-aside files it could not search: said, with the
                            // forget waiting for them (#1586).
                            Ok(lamp_eyes::who::Forgot { forgot, unsearched }) if unsearched > 0 => {
                                eprintln!(
                                    "lamp-eyes: asked to forget a face; forgotten: {forgot}; {unsearched} set-aside files could not be searched, and the forget waits for them"
                                );
                                who.known = None;
                                serde_json::json!({"forgot": forgot, "unsearched": unsearched})
                            }
                            Ok(lamp_eyes::who::Forgot { forgot, .. }) => {
                                eprintln!("lamp-eyes: asked to forget a face; forgotten: {forgot}");
                                who.known = None;
                                serde_json::json!({"forgot": forgot})
                            }
                            Err(error) => {
                                eprintln!("lamp-eyes: could not forget a face: {error}");
                                serde_json::json!({"error": "storage"})
                            }
                        }
                    }
                    // A face from a photo (`--photo-dir`): the file read once and
                    // removed, its one face printed and kept unconfirmed until its
                    // person says yes at the lamp (#1968).
                    Some(Ask::RememberPhoto { name, file }) => {
                        match (&arguments.photo_dir, plain_name(&name)) {
                            (None, _) => serde_json::json!({"error": "photos_off"}),
                            (Some(dir), None) => {
                                let _ = lamp_eyes::photo::take(dir, &file);
                                serde_json::json!({"error": "not_a_name"})
                            }
                            (Some(dir), Some(name)) => {
                                eprintln!("lamp-eyes: asked to add a face from a photo");
                                match lamp_eyes::photo::take(dir, &file).and_then(|bytes| {
                                    lamp_eyes::photo::print_of(&bytes, &detector, &who.recogniser)
                                }) {
                                    Err(why) => {
                                        eprintln!(
                                            "lamp-eyes: did not add a face from a photo: {}",
                                            why.code()
                                        );
                                        serde_json::json!({"error": why.code()})
                                    }
                                    // Someone who asked already: theirs.
                                    Ok(print) => match who.people.who(&print) {
                                        Some((known, _)) if known == name => {
                                            serde_json::json!({"remembered": known})
                                        }
                                        Some((known, _)) => {
                                            eprintln!(
                                                "lamp-eyes: did not add a face from a photo: it is known by another name"
                                            );
                                            serde_json::json!({"error": "known_as", "known_as": known})
                                        }
                                        None => {
                                            let now_unix_s = std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .map_or(0, |since| since.as_secs());
                                            match who.unconfirmed.as_mut().map(|unconfirmed| {
                                                unconfirmed.add(&name, print, now_unix_s)
                                            }) {
                                                Some(Ok(())) => {
                                                    eprintln!(
                                                        "lamp-eyes: added a face from a photo, unconfirmed until its person says yes"
                                                    );
                                                    serde_json::json!({"added": name, "unconfirmed": true})
                                                }
                                                _ => serde_json::json!({"error": "full"}),
                                            }
                                        }
                                    },
                                }
                            }
                        }
                    }
                    Some(Ask::PhotoHere) => match &who.unconfirmed {
                        None => serde_json::json!({"error": "photos_off"}),
                        Some(_) => serde_json::json!({"photo_here": who
                            .photo_here
                            .as_ref()
                            .filter(|(_, at)| at.elapsed() < KNOWN_FOR)
                            .map(|(name, _)| name.clone())}),
                    },
                    // Taken only for the face at the lamp now (the reviewer on
                    // #1968, 22:28): never for a face elsewhere.
                    Some(Ask::PhotoAsked(name)) => {
                        let here = lamp_eyes::photo::is_here(
                            who.photo_here
                                .as_ref()
                                .map(|(n, at)| (n.as_str(), at.elapsed())),
                            &name,
                            KNOWN_FOR,
                        );
                        match who.unconfirmed.as_mut() {
                            None => serde_json::json!({"error": "photos_off"}),
                            Some(_) if !here => serde_json::json!({"error": "not_here"}),
                            Some(unconfirmed) => {
                                let asked = unconfirmed.asked(&name).unwrap_or(false);
                                if asked {
                                    // The meeting counts from the asking.
                                    who.photo_seen.retain(|(seen, _)| seen != &name);
                                    who.photo_seen.push((name, lamp_ipc::monotonic_us()));
                                }
                                serde_json::json!({"asked": asked})
                            }
                        }
                    }
                    // A yes only from the face here, and only once it was asked.
                    Some(Ask::PhotoYes(name)) => {
                        let here = lamp_eyes::photo::is_here(
                            who.photo_here
                                .as_ref()
                                .map(|(n, at)| (n.as_str(), at.elapsed())),
                            &name,
                            KNOWN_FOR,
                        );
                        match who.unconfirmed.as_mut() {
                            None => serde_json::json!({"error": "photos_off"}),
                            Some(_) if !here => serde_json::json!({"error": "not_here"}),
                            Some(unconfirmed) if !unconfirmed.was_asked(&name) => {
                                serde_json::json!({"error": "not_asked"})
                            }
                            Some(unconfirmed) => match unconfirmed.confirmed(&name) {
                                Ok(Some(taken)) => {
                                    let now_unix_s = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map_or(0, |since| since.as_secs());
                                    let kept = who.people.remember_one(
                                        taken.name(),
                                        vec![taken.print()],
                                        now_unix_s,
                                        false,
                                    );
                                    if let Err(who::Refused::Store(_)) = &kept {
                                        // Refused by the store: unconfirmed again.
                                        let _ = unconfirmed.put_back(taken);
                                    } else {
                                        eprintln!(
                                            "lamp-eyes: a face from a photo was confirmed by its person"
                                        );
                                        who.photo_here = None;
                                        who.photo_seen.retain(|(seen, _)| seen != &name);
                                    }
                                    kept_reply(kept, who.visits, who.people.names().len())
                                }
                                Ok(None) => serde_json::json!({"error": "not_unconfirmed"}),
                                Err(_) => serde_json::json!({"error": "storage"}),
                            },
                        }
                    }
                    Some(Ask::PhotoNo(name)) => match who.unconfirmed.as_mut() {
                        None => serde_json::json!({"error": "photos_off"}),
                        Some(unconfirmed) => {
                            let gone = unconfirmed.let_go(&name).unwrap_or(false);
                            if gone {
                                eprintln!(
                                    "lamp-eyes: a face from a photo was let go: its person said no"
                                );
                                who.photo_here = None;
                                who.photo_seen.retain(|(seen, _)| seen != &name);
                            }
                            serde_json::json!({"let_go": gone})
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
                    let reply = kept_reply(kept, who.visits, who.people.names().len());
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
            if let Some(bus) = bus_out.as_mut() {
                bus.activity_forget_prints();
            }
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
            if let Some(bus) = bus_out.as_mut() {
                bus.activity_forget_prints();
            }
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
            if let Some(bus) = bus_out.as_mut() {
                bus.activity_forget_prints();
            }
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
            let mut stage_at = looked_at;
            let stage = |at: &mut Instant, into: &mut Duration| {
                let now = Instant::now();
                *into += now.saturating_duration_since(*at);
                *at = now;
            };
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
            stage(&mut stage_at, &mut stages[0]);
            let busy = lamp_eyes::change::busy(
                wanted.is_some(),
                who.as_ref().is_some_and(|who| who.taking.is_some()),
                any_face_at.map(|at| at.elapsed().as_micros() as u64),
            );
            if let Some(change) = change.as_mut() {
                let fresh = |topic: &str| {
                    own_bus
                        .as_ref()
                        .and_then(|bus| bus.latest(topic).ok().flatten())
                        .filter(|message| !message.expired)
                };
                let light = fresh("light/state").and_then(|message| message.parse().ok());
                change.set_lit(own_light.lit(light));
            }
            let decoded = Picture::from_jpeg(&frame[..used]);
            stage(&mut stage_at, &mut stages[1]);
            let found = decoded.and_then(|picture| {
                let detect = lamp_eyes::change::detect(
                    change.as_mut(),
                    &picture,
                    busy,
                    lamp_ipc::monotonic_us(),
                );
                stage(&mut stage_at, &mut stages[2]);
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
                        // How open the largest face's mouth is (#720), how far apart
                        // its eyes, and how long the mesh took, for the journal and,
                        // with LAMP_EYES_SPEAKING=on, for that face's `speaking`.
                        let now_us = lamp_ipc::monotonic_us();
                        if let Some(lines) = heard_lines.as_mut() {
                            while let Ok(Some(message)) = lines.recv(Duration::ZERO) {
                                // When the words were heard, by the voice's monotonic
                                // clock, never later than now: lines that waited while
                                // the camera was closed are as old as they are.
                                if let Ok(line) = message.parse::<lamp_msgs::Speech>() {
                                    heard.words(line.finished, message.header.at_us.min(now_us));
                                }
                            }
                        }
                        let mut opens = vec![None; faces.len()];
                        if let Some(mesh) = mesh.as_mut()
                            && (heard_lines.is_none() || heard.hears(now_us))
                            && let Some((largest, face)) = faces
                                .iter()
                                .enumerate()
                                .max_by(|a, b| a.1.height.total_cmp(&b.1.height))
                        {
                            let began = Instant::now();
                            let open = mesh.mouth_open(&picture, face);
                            let eyes = lamp_eyes::speaking::shape(&face.marks)
                                .map_or(0.0, |shape| shape.1);
                            match open {
                                Ok(Some(open)) => {
                                    opens[largest] = Some(open);
                                    eprintln!(
                                        "lamp-eyes: mouth open {open:.4} eyes {eyes:.0} px mesh {} ms",
                                        began.elapsed().as_millis()
                                    );
                                }
                                Ok(None) => {}
                                Err(error) => eprintln!("lamp-eyes: mouth not read: {error}"),
                            }
                        }
                        if let Some(bus) = bus_out.as_mut() {
                            let now = lamp_ipc::monotonic_us();
                            bus.saw(&faces, &opens, picture.filled, now);
                            // The sitter told from others by face (#1678's
                            // (d)): a print held in memory only.
                            if let Some(who) = who.as_ref()
                                && let Some(index) = bus.activity_wants_print(now)
                            {
                                let print = faces
                                    .get(index)
                                    .and_then(|face| align(&picture, &face.marks))
                                    .and_then(|aligned| who.recogniser.print(&aligned).ok());
                                bus.activity_printed(now, print.as_deref());
                            }
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
                            let near = faces
                                .iter()
                                .zip(&heights)
                                .enumerate()
                                .filter(|(_, (_, height))| **height >= NEAR_ENOUGH)
                                .max_by(|a, b| a.1.1.total_cmp(b.1.1));
                            // That face's passing id, while speaking is
                            // judged: whose mouth the known name goes with.
                            let near_id = near.and_then(|(index, _)| {
                                bus_out.as_ref().and_then(|bus| bus.speaking_id(index))
                            });
                            let print = near
                                .and_then(|(_, (face, _))| align(&picture, &face.marks))
                                .and_then(|aligned| who.recogniser.print(&aligned).ok());
                            if let Some((taking, _)) = who.taking.as_mut() {
                                taking.look(&heights, print);
                            } else if let Some(print) = print {
                                who.known = who
                                    .people
                                    .who(&print)
                                    .map(|(name, likeness)| (name, likeness, Instant::now()));
                                // Nobody who asked: a face from a photo, maybe,
                                // never named, only noted so the voice can ask.
                                if who.known.is_none()
                                    && let Some(name) =
                                        who.unconfirmed.as_ref().and_then(|u| u.here(&print))
                                {
                                    who.photo_seen.retain(|(seen, _)| seen != &name);
                                    who.photo_seen.push((name.clone(), lamp_ipc::monotonic_us()));
                                    who.photo_here = Some((name, Instant::now()));
                                }
                                if who.visits {
                                    let level = familiar(&mut who.people, who.known.as_ref());
                                    if let Some(bus) = bus_out.as_mut() {
                                        bus.familiar(level, lamp_ipc::monotonic_us());
                                    }
                                }
                                if let Some(path) = &arguments.known_file {
                                    lamp_eyes::who::keep_known(
                                        path,
                                        who.known.as_ref().map(|known| known.0.as_str()),
                                        near_id.as_deref(),
                                    );
                                }
                            } else {
                                // Nobody near: nobody known at the lamp.
                                if let Some(path) = &arguments.known_file {
                                    lamp_eyes::who::keep_known(path, None, None);
                                }
                                if who.visits
                                    && let Some(bus) = bus_out.as_mut()
                                {
                                    bus.familiar(None, lamp_ipc::monotonic_us());
                                }
                            }
                        }
                        // How the near face seems, at most every 2 s: a
                        // kind and a confidence on the bus, nothing kept.
                        if let Some(seems) = seems.as_mut() {
                            let tall = picture.filled.1.max(1) as f32;
                            let near = faces
                                .iter()
                                .filter(|face| face.height / tall >= NEAR_ENOUGH)
                                .max_by(|a, b| a.height.total_cmp(&b.height));
                            if let Some(face) = near
                                && seems.read_at.is_none_or(|at| {
                                    at.elapsed() >= lamp_eyes::seems::READ_EVERY
                                })
                                && let Some(aligned) = align(&picture, &face.marks)
                            {
                                let began = Instant::now();
                                seems.read_at = Some(began);
                                match seems.reader.read(&aligned) {
                                    Ok(read) => {
                                        let took = began.elapsed();
                                        seems.reads += 1;
                                        seems.spent += took;
                                        seems.longest = seems.longest.max(took);
                                        if let Some(bus) = bus_out.as_mut() {
                                            bus.seems(read, lamp_ipc::monotonic_us());
                                        }
                                    }
                                    Err(error) => {
                                        eprintln!("lamp-eyes: the face was not read: {error}");
                                    }
                                }
                            }
                            if seems.told_at.elapsed() >= Duration::from_secs(60) {
                                if seems.reads > 0 {
                                    eprintln!(
                                        "lamp-eyes: {} face reads in the last minute, {} ms each, the longest {} ms (that look's delay)",
                                        seems.reads,
                                        seems.spent.as_millis() / u128::from(seems.reads),
                                        seems.longest.as_millis()
                                    );
                                }
                                seems.told_at = Instant::now();
                                seems.reads = 0;
                                seems.spent = Duration::ZERO;
                                seems.longest = Duration::ZERO;
                            }
                        }
                        let sight = choose(&faces, picture.filled);
                        unfollowed += u32::from(!faces.is_empty() && sight.is_none());
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
                change.as_mut().map_or(String::new(), |change| {
                    let each = |spent: Duration| {
                        spent.as_secs_f32() * 1e3 / (looks + unreadable).max(1) as f32
                    };
                    let rest = spent
                        .saturating_sub(stages.iter().sum::<Duration>())
                        .saturating_sub(detecting);
                    format!(
                        ", {} of them unchanged and not detected, {:.0} ms a detected look{}; ms a look: sight file {:.1}, decode {:.1}, grid {:.1}, detector {:.1}, the rest {:.1}{}; {unfollowed} found faces but none to follow",
                        change.skipped(),
                        detecting.as_secs_f32() * 1e3 / detected.max(1) as f32,
                        change.take_why().line(),
                        each(stages[0]),
                        each(stages[1]),
                        each(stages[2]),
                        each(detecting),
                        each(rest),
                        kept.as_mut()
                            .map_or(String::new(), |kept| std::mem::take(&mut kept.cost).line()),
                    )
                }),
                still.as_ref().map_or(String::new(), |still| format!(
                    ", {} still faces left out so far",
                    still.left_out
                ))
            );
            (frames, looks, with_face, unreadable) = (0, 0, 0, 0);
            spent = Duration::ZERO;
            (detected, detecting) = (0, Duration::ZERO);
            (stages, unfollowed) = ([Duration::ZERO; 3], 0);
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
    fn the_mouth_model_is_none_unless_given_and_an_absolute_path() {
        assert_eq!(parse(args(&FULL)).unwrap().unwrap().mouth_model, None);
        let given = [
            &FULL[..],
            &["--mouth-model", "/opt/lampOS/eyes/models/face_mesh_v2.onnx"],
        ]
        .concat();
        assert_eq!(
            parse(args(&given)).unwrap().unwrap().mouth_model,
            Some(PathBuf::from("/opt/lampOS/eyes/models/face_mesh_v2.onnx"))
        );
        assert!(
            parse(args(
                &[&FULL[..], &["--mouth-model", "face_mesh_v2.onnx"]].concat()
            ))
            .is_err()
        );
        let twice = [&given[..], &["--mouth-model", "/tmp/other.onnx"]].concat();
        assert!(parse(args(&twice)).is_err());
        let heard = [&given[..], &["--mouth-on-heard", "on"]].concat();
        assert!(parse(args(&heard)).unwrap().unwrap().mouth_on_heard);
        assert!(!parse(args(&given)).unwrap().unwrap().mouth_on_heard);
        assert!(parse(args(&[&FULL[..], &["--mouth-on-heard", "on"]].concat())).is_err());
        assert!(parse(args(&[&given[..], &["--mouth-on-heard", "yes"]].concat())).is_err());
        let runtime = [
            "--mouth-runtime",
            "/opt/lampOS/voice/current/libonnxruntime.so.1.30.0",
        ];
        assert!(
            parse(args(&[&given[..], &runtime[..]].concat()))
                .unwrap()
                .unwrap()
                .mouth_runtime
                .is_some()
        );
        assert!(parse(args(&[&FULL[..], &runtime[..]].concat())).is_err());
    }

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
        // #1670: visits are counted only when asked for, and only where
        // faces are recognised.
        assert!(!value.visits);
        let counting = [&FULL[..], &["--visits", "on"]].concat();
        match parse(args(&counting)) {
            Ok(Some(counted)) => assert!(counted.visits && counted.who.is_some()),
            Ok(None) => panic!("not help"),
            Err(error) => assert_eq!(
                error,
                "--visits needs --recogniser, --people and --who-socket"
            ),
        }
        assert!(parse(args(&[&FULL[..], &["--visits", "yes"]].concat())).is_err());
        // #1671: no face is read without the model, which is a short
        // absolute path, given once.
        assert!(value.seems.is_none());
        let reading = [
            &FULL[..],
            &["--seems", "/opt/lampOS/eyes/models/emotion-ferplus-8.onnx"],
        ]
        .concat();
        assert!(parse(args(&reading)).unwrap().unwrap().seems.is_some());
        assert!(parse(args(&[&FULL[..], &["--seems", "emotion.onnx"]].concat())).is_err());
        assert!(parse(args(&[&reading[..], &["--seems", "/x.onnx"]].concat())).is_err());
        // Faces from a photo: off unless given, and only with the recogniser.
        assert!(value.photo_dir.is_none());
        let photos = [&FULL[..], &["--photo-dir", "/var/lib/lampos-eyes/photos"]].concat();
        match parse(args(&photos)) {
            Ok(Some(given)) => assert!(given.photo_dir.is_some() && given.who.is_some()),
            Ok(None) => panic!("not help"),
            Err(error) => assert_eq!(
                error,
                "--photo-dir needs --recogniser, --people and --who-socket"
            ),
        }
        assert!(parse(args(&[&FULL[..], &["--photo-dir", "photos"]].concat())).is_err());
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
