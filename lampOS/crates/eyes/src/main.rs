//! The lamp's eyes: one process that owns the camera, finds a face in what
//! it sees and tells the body where it is. It opens the camera only while
//! the voice service keeps saying it may, and sends nothing but the face's
//! place in the picture. With `--sight-file` it also keeps its newest frame,
//! that one only, in a private file for the voice service, and with
//! `--full-look-file` one still the voice service asked for, in another.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

const USAGE: &str = "lamp-eyes (--usb VENDOR:PRODUCT@TOPOLOGY [--device ABSOLUTE_PATH] | --usb VENDOR:PRODUCT) --lock-directory PRIVATE_ABSOLUTE_DIRECTORY --model ABSOLUTE_PATH --permit-socket ABSOLUTE_PATH --body-socket ABSOLUTE_PATH [--sight-file ABSOLUTE_PATH] [--presence-file ABSOLUTE_PATH] [--recogniser ABSOLUTE_PATH --people ABSOLUTE_PATH --who-socket ABSOLUTE_PATH [--known-file ABSOLUTE_PATH] [--visits on|off]] [--leave-still-faces on|off] [--steady-rate on|off] [--look-on-change on|off [--change-persist on|off] [--glance-log /var/lib/lampos-eyes/glance.log]] [--full-look WIDTHxHEIGHT --full-look-file ABSOLUTE_PATH [--full-look-movement on|off] [--sight-from-look on|off]] [--mouth-model ABSOLUTE_PATH [--mouth-on-heard on|off] [--mouth-runtime ABSOLUTE_PATH]] [--seems ABSOLUTE_PATH] [--seems-blendshapes ABSOLUTE_PATH] [--seems-log ABSOLUTE_PATH] [--photo-dir ABSOLUTE_PATH] [--mouth-diagnostics on|off] [--camera-helper ABSOLUTE_PATH]\n\nOwns the camera at --device (checked against the USB identity given, hex vendor and\nproduct and the port topology, for example 01da:5875@3-1.1.4; without --device, the one\ncamera with those ids at that port, found at each open, never one on another port; with\nVENDOR:PRODUCT alone, the one camera with those ids on any port; none or two refused\nwith a reason) and looks for a face in\nits frames four times a second while there is one and less than twice a second when\nthere is none, with the pinned YuNet model at --model. With --steady-rate on, it looks\ntwice a second while one face has sat still for 3 s (off unless given). With\n--look-on-change on, once no face has been found for 4 s, a frame like the last one the\ndetector ran on is said to hold no face without it, and the detector runs at least every\n3 s (off unless given). With --change-persist on (with --look-on-change on), a cell
counts as changed only if it changed in this glance and the one compared before it, so a
one-glance flicker is not a change and a person costs at most one glance more (off unless
given). With --glance-log /var/lib/lampos-eyes/glance.log (with --look-on-change on;
that path only, the one place a reset wipes), each compared glance is written to that
file, made new each start (0600, in a folder of the owner's only), as one JSON line of
numbers: the rule's verdict, the cells past the bar and, per cell, its change over its
noise and its change in greys after its row's median change; no grey, pixel or picture,
and it stops after 20 minutes or 50 MB. A test flag: no unit lampOS deploys passes it
(off unless given). Each look\nsends one datagram to --body-socket: {\"face\":{\"x\":..,\"y\":..,\"size\":..,\"turned\":..}} or\n{\"face\":null}. The camera is open only while {\"camera\":true} keeps arriving on\n--permit-socket, at least every 1.5 s; {\"camera\":false} closes it at once;\n{\"close_receipt\":N} after it asks for N on eyes/camera_closed (LAMP_BUS=on) once the\ncamera is shut, with whether its last stop released the device. Without\n--sight-file and --full-look-file no frame is written anywhere and none leaves this process. With it the\nnewest frame, and only that one, is kept in that file (mode 0600) while the camera is\nopen and removed when it closes: it is what the voice service shows the model. With\n--presence-file one line, the near face's height as a share of the picture's, is kept\nfresh in that file while there is a face: no picture, only that number. With\n--recogniser (the pinned SFace model), --people and --who-socket, the eyes remember\na face when the voice service asks ({\"remember\":\"NAME\"} on the socket), forget it\n({\"forget\":\"NAME\"}), list the names ({\"list\":true}) and say whose face is at the\nlamp ({\"who\":true}). Only the prints of people who asked are kept, in --people\n(mode 0600); no picture. With --known-file, the name of the one known at the lamp is\nkept fresh in that file while they are there, and `tellable` beside it holds the names\nof those who said yes to being named in Who was here ({\"visits_yes\":\"NAME\"} on the\nsocket, taken only from the face at the lamp now; {\"visits_no\":\"NAME\"} takes it back). With --visits on, each remembered person\nhas one familiarity number, rising once a day they are seen and halving every 14 days,\nwith no date or time of a visit kept, and with LAMP_BUS=on the level of the one at the\nlamp is said on humans/familiar, with no name (off unless given). With --leave-still-faces on, a face at a place\nwhere faces have come from for half an hour, every one small (under 0.14 of the height)\nand faint (scored under 0.8), is not a person: it is left out of what the body is told,\nthe bus and recognition (off unless given). With --full-look and --full-look-file the\ncamera runs at that size, the working frame is kept in --sight-file at 320 across as before, and {\"look_full\":ID} on\n--permit-socket asks for one still: taken once the head has settled\n(body/joint_states on the bus, at most 2 s), at most 1024 across and 64 KiB, or with\n{\"look_full\":ID,\"side\":PIXELS} at most that side across and 184 KiB, marked\nwith the ID in a JPEG comment, kept in --full-look-file for 15 s and removed (off unless given).\nWith --full-look-movement on the head settles on movement only: the camera's aim, the arm's\njoints with the head's, and never a joint's distance to its goal (off unless given).\nWith --sight-from-look on (with --full-look and --sight-file), the sight file's 320-across\nframe is encoded from the look's own decoded picture instead of the full frame being decoded\nand shrunk again (off unless given).\nWith --mouth-model (the pinned Face Mesh V2), the largest face's mouth is read each look\n(with --mouth-diagnostics on, the journal says how open, each look), and with LAMP_EYES_SPEAKING=on that face's speaking reads it\n(off unless given); with LAMP_EYES_SPEAKING_ALL_FACES=on, up to three faces' mouths, largest first,\nwhile the look's meshing stays within 200 ms (off unless set). With --mouth-on-heard on, the mesh runs only while the voice hears a\nline (voice/heard on the bus, from its first words until 2 s after it ends).\nWith --mouth-runtime (the voice's pinned ONNX Runtime library, checked before it is loaded),\nthe mesh runs under ONNX Runtime on one thread instead of tract.\nWith --seems (the pinned FER+ expression model), how the near face seems is read at\nmost every 2 s, as positive, negative or unsure with a confidence, and with LAMP_BUS=on\nsaid on humans/seems, with no name and no picture, and only while the lamp's yes (for everyone it sees) is kept\nin /var/lib/lampOS/consent/face-expressions.json ({\"face_expressions\":\"yes\"}; none, no\nread: the eyes still look); the journal says only how many reads\nthere were, how long one took and the longest, once a minute (off unless given; #1671).\nWith --seems-log (with --seems or --seems-blendshapes), each read is also written to that\nfile (0600) as its time, kind, confidence and reader, nothing of whose face, up to 20,000\nlines, for the measure; lamp-seems-measure counts it against a labelled session and\nremoves both (off unless given; #1671).\nWith --seems-blendshapes (the pinned Blendshape V2 model, with --mouth-model), the same is\nread from the mesh's points: the smiles, frowns and sneers of the near face, said and\njournaled the same way and with the same yes, each read naming its reader, so both may run\nside by side (off unless given; #1671). This read then runs on its own thread, from the\npoints of the look's own mesh of that face (or of that thread's mesh of its crop, for another\nface), and no look waits on it. With LAMP_SEEMS_BROW_LOW=on, it also names a face negative with\nthe brows drawn down at or over 0.80 and the smiles under 0.3 (off unless set; #2132).\nWith LAMP_REMEMBER_SPEAKER=on (with --recogniser, LAMP_BUS and LAMP_EYES_SPEAKING),\nlive enrollment requires the consent-capable internal remember_speaker_consented operation\nof the current Voice and an existing print\nof the face seen speaking that line; plain live remember is refused (off unless set).\nWith LAMP_FACE_FULL_CROP=on (with --recogniser), faces are remembered and recognised from the\nframe at its own size, and the one known is held through a miss for 4 s after their last\nmatch, unless an unknown face is near; the camera closed lets them go at once (off unless set; #3126).\nWith LAMP_EYES_MOUTH_FULL_LOOK=on (with --full-look and --mouth-model), the mouths are cut\nfrom the frame at its own size and their eye distance counted in its pixels (off unless set; #1711).\nWith LAMP_EYES_YAWN=on (with --mouth-model, --seems-blendshapes and LAMP_ACTIVITY), the largest\nface's yawns are counted into the sitting, as yawns on room/activity: its mouth held wide for\n1.5 s with no smile and the brows not raised, and only while the face-expressions yes is kept; a\ncount only (off unless set; #3709).\nWith --photo-dir (the eyes' own folder, with --recogniser), {\"remember\":\"NAME\",\"photo\":\"FILE\"}\non the who socket takes the one face in that JPEG in the folder and removes the file once\nread (no picture is kept); the face is kept unconfirmed until its person says yes at the\nlamp, and deleted if not (off unless given).\nWith LAMP_TURN_TO_ANSWERED=on (with LAMP_BUS=on and LAMP_EYES_SPEAKING=on), as the voice\nbegins an answer (voice/state on the bus), the one face seen speaking just before is the\none sent to --body-socket until the answer ends; while it is out of view nothing is sent (off unless given; #1810).\nWith --mouth-diagnostics on (off by default), one numeric journal record per look explains\nthe largest detection's speaking decision. Requires LAMP_BUS=on and LAMP_EYES_SPEAKING=on;\nnever enables inference and records no IDs, landmarks, images, sound or words.\nWith --camera-helper (the camera's helper's socket, #2892), after three opens in a row that did\nnot open, while seeing is permitted, the helper is asked, on a thread of its own, to re-authorize\nthe camera's USB device, then at most once a minute while they go on failing, and its answer\nis said in the journal (off unless given).\n\nlamp-eyes reauthorize-camera --usb VENDOR:PRODUCT[@TOPOLOGY] is that helper, run as root by its\nsocket for each connection: it checks the device at that port is that camera, or with no port\nfinds the one USB device with those ids on any port (none or two: refused), writes its\nauthorized 0 then 1, as an unplug and a plug would, at most once a minute, answers one line,\nand reads nothing from whoever connected.\n";

#[derive(Clone, Debug, Eq, PartialEq)]
struct Usb {
    vendor: u16,
    product: u16,
    /// The port, or empty for any (#2190). Without `--device` the one camera
    /// with the ids, at this port when it is named (#3504), is found at each
    /// open.
    topology: String,
}

#[derive(Debug, Eq, PartialEq)]
struct Arguments {
    /// The camera's node; none when it is found at each open by `--usb`,
    /// at its port when it names one (#3504).
    device: Option<PathBuf>,
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
    /// With it, a change counts only where it held two glances (persist-2).
    change_persist: bool,
    glance_log: Option<PathBuf>,
    /// The camera's size for a full look (#836), when one may be asked for.
    full_look: Option<(u32, u32)>,
    /// Where a full look's still is kept, apart from the working frame.
    full_look_file: Option<PathBuf>,
    /// A full look's head settles on movement only (#842).
    full_look_movement: bool,
    /// With `--full-look`, the sight file's working frame from the look's
    /// own decoded picture (`--sight-from-look on`).
    sight_from_look: bool,
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
    /// The pinned Blendshape V2 model, the product's reader for the same
    /// `humans/seems`, from the mesh's points (#1671, the lead's 20 Oct
    /// 08:16). Needs `--mouth-model`; beside `--seems` for the measure.
    seems_blendshapes: Option<PathBuf>,
    /// One line a face read, its time, kind, confidence and reader, for the
    /// measure (#1671, step 0). Needs a reader; removed with the measure.
    seems_log: Option<PathBuf>,
    /// Numeric explanations of the speaking judge, never another inference.
    mouth_diagnostics: bool,
    /// The camera's helper's socket (#2892): asked to re-authorize the
    /// camera's USB device after opens that did not open. Nothing without it.
    camera_helper: Option<PathBuf>,
}

/// `VENDOR:PRODUCT@TOPOLOGY`, the two numbers as four hex digits each, or
/// `VENDOR:PRODUCT` alone for the one camera with them on any port (#2190).
fn usb_of(value: &str) -> Option<Usb> {
    let (numbers, topology) = value.split_once('@').unwrap_or((value, ""));
    let (vendor, product) = numbers.split_once(':')?;
    let hex = |text: &str| {
        (text.len() == 4 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| u16::from_str_radix(text, 16).ok())
            .flatten()
            .filter(|number| *number != 0)
    };
    let plain = (topology.is_empty() && !value.contains('@'))
        || topology.len() <= 64
            && !topology.is_empty()
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

/// Resets the camera's USB device at `port`, said in the journal, as an
/// unplug would (#2126); `reset_usb` checks the ids there first.
#[cfg(target_os = "linux")]
fn reset_camera(usb: &Usb, port: &str, reopen: &mut lamp_eyes::reopen::Reopen) {
    let identity = lamp_camera::linux::UsbIdentity {
        vendor: usb.vendor,
        product: usb.product,
        topology: port.to_owned(),
        serial: None,
        interface_number: 0,
        capture_index: 0,
    };
    match lamp_camera::linux::reset_usb(&identity) {
        Ok(node) => {
            reopen.reset();
            eprintln!(
                "lamp-eyes: camera's USB device {port} reset ({}), as an unplug would; tried again in {} s",
                node.display(),
                lamp_eyes::reopen::RETRY_STUCK.as_secs()
            );
        }
        Err(error) => eprintln!("lamp-eyes: camera's USB device {port} not reset: {error:?}"),
    }
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
    let mut sight_from_look = None;
    let (mut visits, mut glance_log, mut change_persist) = (None, None, None);
    let (mut mouth_model, mut mouth_on_heard, mut mouth_runtime) = (None, None, None);
    let (mut seems, mut seems_blendshapes, mut seems_log) = (None, None, None);
    let mut photo_dir = None;
    let mut mouth_diagnostics = None;
    let mut camera_helper = None;
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
                && glance_log.is_none()
                && change_persist.is_none()
                && full_look_movement.is_none()
                && sight_from_look.is_none()
                && full_look.is_none()
                && full_look_file.is_none()
                && mouth_model.is_none()
                && mouth_on_heard.is_none()
                && mouth_runtime.is_none()
                && seems.is_none()
                && seems_blendshapes.is_none()
                && seems_log.is_none()
                && photo_dir.is_none()
                && mouth_diagnostics.is_none()
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
            Some("--change-persist") if change_persist.is_none() => {
                change_persist = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--change-persist is on or off"),
                });
            }
            Some("--sight-from-look") if sight_from_look.is_none() => {
                sight_from_look = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--sight-from-look is on or off"),
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
            Some("--mouth-diagnostics") if mouth_diagnostics.is_none() => {
                mouth_diagnostics = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--mouth-diagnostics is on or off"),
                });
            }
            Some("--mouth-model") if mouth_model.is_none() => {
                mouth_model = Some(PathBuf::from(value));
            }
            Some("--mouth-runtime") if mouth_runtime.is_none() => {
                mouth_runtime = Some(PathBuf::from(value));
            }
            Some("--seems") if seems.is_none() => seems = Some(PathBuf::from(value)),
            Some("--seems-blendshapes") if seems_blendshapes.is_none() => {
                seems_blendshapes = Some(PathBuf::from(value));
            }
            Some("--seems-log") if seems_log.is_none() => seems_log = Some(PathBuf::from(value)),
            Some("--glance-log") if glance_log.is_none() => {
                glance_log = Some(PathBuf::from(value));
            }
            Some("--photo-dir") if photo_dir.is_none() => photo_dir = Some(PathBuf::from(value)),
            Some("--camera-helper") if camera_helper.is_none() => {
                camera_helper = Some(PathBuf::from(value));
            }
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
                        "--usb is VENDOR:PRODUCT@TOPOLOGY, for example 01da:5875@3-1.1.4, or VENDOR:PRODUCT for any port",
                    )?);
            }
            _ => return Err("unknown or duplicate option"),
        }
    }
    let result = Arguments {
        device,
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
        change_persist: change_persist.unwrap_or(false),
        glance_log,
        full_look,
        full_look_file,
        full_look_movement: full_look_movement.unwrap_or(false),
        sight_from_look: sight_from_look.unwrap_or(false),
        mouth_model,
        mouth_on_heard: mouth_on_heard.unwrap_or(false),
        mouth_runtime,
        seems,
        seems_blendshapes,
        seems_log,
        photo_dir,
        mouth_diagnostics: mouth_diagnostics.unwrap_or(false),
        camera_helper,
    };
    // Without --device the camera is found by --usb, at its port when it
    // names one (#3504): a second camera that enumerated first as video0 is
    // not taken in its place.
    if result.device.is_some() && result.usb.topology.is_empty() {
        return Err(
            "--usb VENDOR:PRODUCT finds the camera on any port; leave out --device, or name the port",
        );
    }
    if result.full_look.is_some() != result.full_look_file.is_some()
        || (result.full_look_file.is_some() && result.full_look_file == result.sight_file)
    {
        return Err("--full-look and --full-look-file go together, apart from --sight-file");
    }
    if result.sight_from_look && (result.full_look.is_none() || result.sight_file.is_none()) {
        return Err("--sight-from-look needs --full-look and --sight-file");
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
    if result.seems_log.is_some() && result.seems.is_none() && result.seems_blendshapes.is_none() {
        return Err("--seems-log needs --seems or --seems-blendshapes");
    }
    if result
        .seems_log
        .as_deref()
        .is_some_and(|path| path != std::path::Path::new(lamp_eyes::seems_log::PATH))
    {
        return Err("--seems-log is /var/lib/lampos-eyes/seems.log, the one place a reset wipes");
    }
    if result.change_persist && !result.look_on_change {
        return Err("--change-persist needs --look-on-change on");
    }
    if result.glance_log.is_some() && !result.look_on_change {
        return Err("--glance-log needs --look-on-change on");
    }
    if result
        .glance_log
        .as_deref()
        .is_some_and(|path| path != std::path::Path::new(lamp_eyes::glance_log::PATH))
    {
        return Err("--glance-log is /var/lib/lampos-eyes/glance.log, the one place a reset wipes");
    }
    if result.seems_blendshapes.is_some() && result.mouth_model.is_none() {
        return Err("--seems-blendshapes needs --mouth-model");
    }
    for path in [
        &result.lock_directory,
        &result.model,
        &result.permit_socket,
        &result.body_socket,
    ]
    .into_iter()
    .chain(&result.device)
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
    .chain(&result.seems_blendshapes)
    .chain(&result.seems_log)
    .chain(&result.glance_log)
    .chain(&result.photo_dir)
    .chain(&result.camera_helper)
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

fn diagnostic_requirements(enabled: bool, bus: bool, speaking: bool) -> Result<(), &'static str> {
    if enabled && (!bus || !speaking) {
        return Err("--mouth-diagnostics needs LAMP_BUS=on and LAMP_EYES_SPEAKING=on");
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    // The camera's helper (#2892): root's, started by its socket, the one
    // thing it does.
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new(REAUTHORIZE)) {
        return reauthorize_camera(std::env::args_os().skip(2));
    }
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
    if let Err(error) = diagnostic_requirements(
        arguments.mouth_diagnostics,
        std::env::var("LAMP_BUS").ok().as_deref() == Some("on"),
        std::env::var("LAMP_EYES_SPEAKING").ok().as_deref() == Some("on"),
    ) {
        eprintln!("{error}");
        return std::process::ExitCode::from(2);
    }
    match run(arguments) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-eyes failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// The helper's command: `lamp-eyes reauthorize-camera --usb
/// VENDOR:PRODUCT[@TOPOLOGY]` (#2892; any port without one, #4074).
const REAUTHORIZE: &str = "reauthorize-camera";
/// Where the helper keeps when it last re-authorized: its socket's folder,
/// root's.
const REAUTHORIZED_AT: &str = "/run/lampos-eyes-camera/last";

/// The camera's helper: one answer line on its standard output (the
/// connection), what it did said once in the journal, never anything read
/// from whoever connected.
fn reauthorize_camera(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> std::process::ExitCode {
    let usb = match (args.next(), args.next(), args.next()) {
        (Some(key), Some(value), None) if key == "--usb" => value.to_str().and_then(usb_of),
        _ => None,
    };
    let Some(usb) = usb else {
        eprintln!("lamp-eyes {REAUTHORIZE}: --usb VENDOR:PRODUCT[@TOPOLOGY]");
        return std::process::ExitCode::from(2);
    };
    let now_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (devices, stamp) = (
        Path::new("/sys/bus/usb/devices"),
        Path::new(REAUTHORIZED_AT),
    );
    let outcome = if usb.topology.is_empty() {
        lamp_eyes::camera_helper::reauthorize_any(devices, usb.vendor, usb.product, stamp, now_s)
    } else {
        lamp_eyes::camera_helper::reauthorize_at(
            devices,
            &usb.topology,
            usb.vendor,
            usb.product,
            stamp,
            now_s,
        )
    };
    match outcome {
        Ok(outcome) => {
            eprintln!(
                "lamp-eyes {REAUTHORIZE}: camera {:04x}:{:04x} at {}: {}",
                usb.vendor,
                usb.product,
                if usb.topology.is_empty() {
                    "any port"
                } else {
                    &usb.topology
                },
                outcome.line()
            );
            println!("{}", outcome.line());
            if outcome == lamp_eyes::camera_helper::Outcome::Reauthorized {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("lamp-eyes {REAUTHORIZE}: not re-authorized: {error}");
            println!(r#"{{"reauthorized":false,"why":"error"}}"#);
            std::process::ExitCode::FAILURE
        }
    }
}

/// After an open that did not open, the `failed`th in a row: the camera's
/// helper asked, if there is one and it's time (`asked`, the last ask, in
/// seconds of the monotonic clock), on a thread of its own so the loop
/// never waits for it; its answer said once (#2892). Linux only, as its
/// callers are.
#[cfg(target_os = "linux")]
fn ask_helper(helper: Option<&Path>, failed: u32, asked: &mut Option<u64>) {
    let Some(helper) = helper else {
        return;
    };
    let now_s = lamp_ipc::monotonic_us() / 1_000_000;
    if !lamp_eyes::camera_helper::ask_now(failed, *asked, now_s) {
        return;
    }
    *asked = Some(now_s);
    let helper = helper.to_path_buf();
    std::thread::spawn(move || match lamp_eyes::camera_helper::ask(&helper) {
        Ok(answer) => eprintln!(
            "lamp-eyes: camera did not open {failed} times in a row: its helper asked to re-authorize its USB device: {answer}"
        ),
        Err(error) => eprintln!(
            "lamp-eyes: camera did not open {failed} times in a row: its helper could not be asked: {error}"
        ),
    });
}

#[cfg(not(target_os = "linux"))]
fn run(arguments: Arguments) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = arguments;
    Err("the eyes require Linux; no camera was opened".into())
}

#[cfg(target_os = "linux")]
fn run(arguments: Arguments) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // An explicitly requested diagnostic must have a usable judge and bus
    // before loading models or opening sockets/camera. Off keeps the old order.
    let diagnostic_bus = if arguments.mouth_diagnostics {
        let mut bus = lamp_eyes::bus_out::BusOut::from_environment(
            arguments.visits,
            arguments.seems.is_some() || arguments.seems_blendshapes.is_some(),
        )
        .ok_or("--mouth-diagnostics needs LAMP_BUS=on and a usable bus")?;
        bus.enable_mouth_diagnostics()?;
        Some(bus)
    } else {
        None
    };
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
        who::{self, Ask, NEAR_ENOUGH, People, Recogniser, Taking, align, live_ask_of, plain_name},
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
    /// A camera that failed while open is tried again after this.
    const RETRY: Duration = lamp_eyes::reopen::RETRY;
    const REPORT_EVERY: Duration = Duration::from_secs(10);
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
        /// The unconfirmed face at the lamp now, its passing id under the
        /// faces publisher's boot while speaking is judged (`boot/f3`), and
        /// when it was last seen.
        photo_here: Option<(String, Option<String>, Instant)>,
        /// `LAMP_PHOTO_SPEAKER=on` (#2856): a yes is kept only with that id.
        photo_speaker: bool,
        remember_speaker: bool,
        speaker_anchors: lamp_eyes::remember_speaker::Anchors,
        /// With it on, a yes that passed, waiting for the look forced after
        /// it (`photo::PendingYes`).
        photo_yes: Option<lamp_eyes::photo::PendingYes<UnixStream>>,
        /// When each unconfirmed face was last seen, monotonic, for the end
        /// of a meeting with no answer.
        photo_seen: Vec<(String, u64)>,
        /// When the faces from photos were last looked at for expiry and
        /// meetings with no answer.
        photos_looked: Option<Instant>,
        /// `LAMP_FACE_FULL_CROP=on`: faces cut from the frame at its own
        /// size, and the known person held through a short miss.
        held: lamp_eyes::who::Held,
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
    /// A yes or no to being named in Who was here (#1997), saved, and what
    /// the voice is answered; the journal says which, never the name.
    fn visits_reply(people: &mut People, name: &str, yes: bool) -> serde_json::Value {
        let said = if yes { "yes" } else { "no" };
        match people.tell_visits(name, yes) {
            Ok(true) => {
                eprintln!(
                    "lamp-eyes: someone the lamp knows said {said} to being named in Who was here"
                );
                serde_json::json!({ "visits": said })
            }
            Ok(false) => serde_json::json!({"error": "not_known"}),
            Err(_) => serde_json::json!({"error": "storage"}),
        }
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
            // Near someone known by a name one letter off: the voice asks
            // whether they are that person (#2176 row 4).
            Err(who::Refused::NearTo(near)) => {
                eprintln!(
                    "lamp-eyes: did not remember a face: it is near someone known by a near name"
                );
                serde_json::json!({"error": "near_to", "near_to": near})
            }
            // A name the lamp knows, said by a face that isn't near theirs.
            Err(who::Refused::NotLikeThem) => {
                eprintln!(
                    "lamp-eyes: did not remember a face: its name is someone's whose face it isn't"
                );
                serde_json::json!({"error": "not_like_them"})
            }
            Err(who::Refused::NotAName) => serde_json::json!({"error": "not_a_name"}),
            Err(who::Refused::Store(error)) => {
                eprintln!("lamp-eyes: could not remember a face: {error}");
                serde_json::json!({"error": "full"})
            }
        }
    }
    /// A face from a photo, confirmed by its person: kept as an ordinary
    /// print, or why not.
    fn keep_photo_yes(who: &mut Who, name: &str) -> serde_json::Value {
        let Some(unconfirmed) = who.unconfirmed.as_mut() else {
            return serde_json::json!({"error": "photos_off"});
        };
        match unconfirmed.confirmed(name) {
            Ok(Some(taken)) => {
                let now_unix_s = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_secs());
                // A face its person confirmed by name: no
                // near-name question (`apart`).
                let kept =
                    who.people
                        .remember_one(taken.name(), taken.prints(), now_unix_s, false, true);
                if let Err(who::Refused::Store(_)) = &kept {
                    // Refused by the store: unconfirmed again.
                    let _ = unconfirmed.put_back(taken);
                } else {
                    eprintln!("lamp-eyes: a face from a photo was confirmed by its person");
                    who.photo_here = None;
                    who.photo_seen.retain(|(seen, _)| seen != name);
                }
                kept_reply(kept, who.visits, who.people.names().len())
            }
            Ok(None) => serde_json::json!({"error": "not_unconfirmed"}),
            Err(_) => serde_json::json!({"error": "storage"}),
        }
    }
    /// A waiting photo yes ended: kept, or answered why not.
    fn photo_yes_ended(
        who: &mut Who,
        ended: lamp_eyes::photo::YesEnded<UnixStream>,
        known_file: Option<&std::path::Path>,
    ) {
        // The commit point (Fourth's 17:01 on #2856): `fenced` checks the
        // voice's connection right before `confirmed()`. A close observed
        // there refuses and nothing is kept; a close after it cannot undo
        // the save. A close is not an atomic revocation across the two
        // processes: one that lands after this fence is too late.
        let (mut stream, reply) = match ended.fenced() {
            lamp_eyes::photo::YesEnded::Keep(name, stream) => (stream, keep_photo_yes(who, &name)),
            lamp_eyes::photo::YesEnded::Refuse(stream, why) => {
                eprintln!("lamp-eyes: a yes to a face from a photo was not kept: {why}");
                (stream, serde_json::json!({"error": why}))
            }
        };
        answer(&mut stream, reply);
        if let Some(known) = known_file {
            lamp_eyes::who::keep_tellable(
                &lamp_eyes::who::tellable_of(known),
                &who.people.tellable(),
            );
        }
    }
    fn answer(stream: &mut UnixStream, value: serde_json::Value) {
        // A caller that is gone or slow changes nothing here.
        let _ = writeln!(stream, "{value}");
    }

    // The photo folder (`/run/lampos-photos` on a lamp, made at boot by the
    // product's tmpfiles, docs/cloud.md), before anything is asked of it.
    // One that can't be made or is open to others stops only photos, never
    // the camera, presence or recognition: each photo asked checks it again
    // (Reviewer-lead on #2273).
    if let Some(dir) = &arguments.photo_dir
        && let Err(error) = lamp_eyes::photo::ready(dir)
    {
        eprintln!("lamp-eyes: photos are off until the photo folder is ready: {error}");
    }
    let detector = Detector::load(&arguments.model)?;
    let load_mesh = || -> Result<_, Box<dyn std::error::Error + Send + Sync>> {
        Ok(match (&arguments.mouth_model, &arguments.mouth_runtime) {
            (Some(model), Some(runtime)) => {
                Some(lamp_eyes::mesh::Mesh::load_with_runtime(model, runtime)?)
            }
            (Some(model), None) => Some(lamp_eyes::mesh::Mesh::load(model)?),
            (None, _) => None,
        })
    };
    // The mouth's pass runs on the look, one mesh a look, with or without
    // --seems-blendshapes: on lamp-52e6 the seems thread meshing every look's
    // crop instead cost 60% of the looks (97 a minute against 234) and gave
    // 39 of them the lips (#3158). A seems read due on the face it meshed
    // takes the same points.
    let mut mesh = load_mesh()?;
    // With --seems and/or --seems-blendshapes, how the near face seems
    // (#1671), each reader on its own: the reads, and how many there were
    // and how long they took, for the journal once a minute. With both, the
    // two run side by side for the measure (the first builder, 09:51).
    /// One read's outcome, how long it took and when its face was seen.
    type SeemsRead = (Result<lamp_msgs::HumanSeems, String>, Duration, u64);
    /// One face offered to the seems worker: the look's mesh points or its
    /// crop, the eye line, and when it was seen.
    type SeemsOffer = (lamp_eyes::blendshapes::Input, (f32, f32), u64);
    enum Reader {
        FerPlus(lamp_eyes::seems::Reader),
        /// On its own thread: the look offers the face's mesh points, or
        /// its crop for the thread's own mesh when the look did not mesh
        /// it, newest only, and takes the reads that are done.
        Blendshapes {
            latest: lamp_eyes::latest::Latest<SeemsOffer>,
            done: std::sync::mpsc::Receiver<SeemsRead>,
        },
    }
    struct Seems {
        reader: Reader,
        read_at: Option<Instant>,
        reads: u32,
        spent: Duration,
        /// The longest read: the most one look was held back by it.
        longest: Duration,
        told_at: Instant,
    }
    let mut readers = Vec::new();
    if let Some(model) = &arguments.seems {
        readers.push(Reader::FerPlus(lamp_eyes::seems::Reader::load(model)?));
    }
    if let Some(model) = &arguments.seems_blendshapes {
        let reader = lamp_eyes::blendshapes::Reader::load(model)?.with_brow_low(
            std::env::var(lamp_eyes::blendshapes::BROW_LOW_ENVIRONMENT)
                .ok()
                .as_deref()
                == Some("on"),
        );
        let mut mesh = load_mesh()?.ok_or("--seems-blendshapes needs --mouth-model")?;
        let (latest, done) = lamp_eyes::latest::worker(
            "eyes-seems",
            move |(input, eye_line, at_us): SeemsOffer| -> SeemsRead {
                let began = Instant::now();
                let landmarks = match input {
                    lamp_eyes::blendshapes::Input::Landmarks(landmarks) => Ok(landmarks),
                    lamp_eyes::blendshapes::Input::Crop(crop) => {
                        mesh.landmarks_of(crop).map_err(|error| error.to_string())
                    }
                };
                let read = landmarks.and_then(|landmarks| {
                    let points = lamp_eyes::blendshapes::points(&landmarks, eye_line)
                        .ok_or("no face to read")?;
                    reader.read(&points).map_err(|error| error.to_string())
                });
                (read, began.elapsed(), at_us)
            },
        )?;
        readers.push(Reader::Blendshapes { latest, done });
    }
    let mut seems: Vec<Seems> = readers
        .into_iter()
        .map(|reader| Seems {
            reader,
            read_at: None,
            reads: 0,
            spent: Duration::ZERO,
            longest: Duration::ZERO,
            told_at: Instant::now(),
        })
        .collect();
    // Whether the lamp's yes to face reads, for everyone it sees, was there
    // at the last ask, for the journal's one line each time it changes.
    let mut face_yes_told: Option<bool> = None;
    // With --seems-log, each read's time, kind, confidence and reader, for
    // the measure (#1671, step 0): nothing of whose face.
    let mut seems_log = match &arguments.seems_log {
        Some(path) => Some(lamp_eyes::seems_log::Log::open(path)?),
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
    // With LAMP_EYES_YAWN=on (#3709): the largest face's yawns, from the
    // look's own mesh and, while its mouth is open wide, a Blendshape V2 read
    // of its points on a thread of its own, so the look never waits (the
    // reviewer-lead on #3842, as the seems read's #2872); and only while the
    // face-expressions yes is kept (asked at most every 2 s).
    /// One look's largest face for the yawn thread: the grant its frame was
    /// captured under (`yawn::Grant`), the frame's time, and its
    /// `mouth_open` with, while open wide, its Blendshape V2 points.
    type YawnOffer = (lamp_eyes::yawn::Grant, u64, Option<(f32, Option<Vec<f32>>)>);
    let mut yawning = match (&arguments.seems_blendshapes, mesh.is_some()) {
        (Some(model), true)
            if std::env::var(lamp_eyes::yawn::ENVIRONMENT).as_deref() == Ok("on") =>
        {
            let reader = lamp_eyes::blendshapes::Reader::load(model)?;
            let mut yawns = lamp_eyes::yawn::Generations::default();
            let (offer, yawned) = lamp_eyes::latest::worker(
                "eyes-yawn",
                move |(grant, frame_us, face): YawnOffer| -> Option<(lamp_eyes::yawn::Grant, u64)> {
                    let mouth = face.map(|(open, points)| {
                        let scores = points
                            .and_then(|points| reader.scores(&points).ok())
                            .and_then(|scores| lamp_eyes::blendshapes::yawn_scores(&scores));
                        lamp_eyes::yawn::Mouth {
                            open,
                            smile: scores.map(|(smile, _)| smile),
                            brow_up: scores.map(|(_, up)| up),
                        }
                    });
                    yawns.look(grant, mouth, frame_us)
                },
            )?;
            Some((offer, yawned, None::<(Instant, bool)>, 0u64))
        }
        _ => {
            if std::env::var(lamp_eyes::yawn::ENVIRONMENT).as_deref() == Ok("on") {
                eprintln!(
                    "lamp-eyes: {} is on without --mouth-model and --seems-blendshapes: no yawn is counted",
                    lamp_eyes::yawn::ENVIRONMENT
                );
            }
            None
        }
    };
    let mut who = match &arguments.who {
        Some((model, people_path, socket)) => {
            let recogniser = Recogniser::load(model)?;
            let people = People::open(people_path.clone())?;
            // Who was here's names (#1997): those who said yes, as they are now.
            if let Some(known) = &arguments.known_file {
                lamp_eyes::who::keep_tellable(
                    &lamp_eyes::who::tellable_of(known),
                    &people.tellable(),
                );
            }
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
            if lamp_eyes::who::full_crop_on() {
                eprintln!(
                    "lamp-eyes: {}=on: faces cut from the frame at its own size, the one known held {} s through a miss",
                    lamp_eyes::who::FULL_CROP_ENVIRONMENT,
                    lamp_eyes::who::KNOWN_HOLD.as_secs()
                );
            }
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
                photo_speaker: lamp_eyes::photo::speaker_from_environment(),
                remember_speaker: lamp_eyes::remember_speaker::from_environment(),
                speaker_anchors: lamp_eyes::remember_speaker::Anchors::default(),
                photo_yes: None,
                photo_seen: Vec::new(),
                photos_looked: None,
                held: lamp_eyes::who::Held::new(lamp_eyes::who::full_crop_on()),
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
    // Opens in a row that did not open, and the USB resets of a stuck
    // camera (#2126); apart from `failures`, which the full size's turns
    // also set.
    let mut reopen = lamp_eyes::reopen::Reopen::default();
    let mut helper_asked = None;
    // Without `--device`: why no one camera was found, said once while it
    // stays so, and the port it was last found on.
    let (mut not_found, mut found_on): (Option<String>, Option<String>) = (None, None);
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
    let mut bus_out = if arguments.mouth_diagnostics {
        diagnostic_bus
    } else {
        lamp_eyes::bus_out::BusOut::from_environment(
            arguments.visits,
            arguments.seems.is_some() || arguments.seems_blendshapes.is_some(),
        )
    };
    if who.is_some()
        && let Some(bus) = bus_out.as_mut()
    {
        bus.activity_by_face();
    }
    // With LAMP_TURN_TO_ANSWERED=on, the face seen asking is the one sent
    // to the body while the voice answers (#1810), read off the bus.
    let mut answered = lamp_eyes::answered::from_environment();
    if answered.is_some() && bus_out.is_none() {
        eprintln!(
            "lamp-eyes: {} is on without the bus (LAMP_BUS=on): the lamp does not turn to the one it answers",
            lamp_eyes::answered::ENVIRONMENT
        );
        answered = None;
    }
    if answered.is_some()
        && std::env::var(lamp_eyes::speaking::ENVIRONMENT)
            .ok()
            .as_deref()
            != Some("on")
    {
        eprintln!(
            "lamp-eyes: {} is on without {}=on: no face reads speaking, so the lamp turns to nobody it answers",
            lamp_eyes::answered::ENVIRONMENT,
            lamp_eyes::speaking::ENVIRONMENT
        );
    }
    if std::env::var(lamp_eyes::speaking::OUTLIER_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
        && std::env::var(lamp_eyes::speaking::ENVIRONMENT)
            .ok()
            .as_deref()
            != Some("on")
    {
        eprintln!(
            "lamp-eyes: {} is on without {}=on: no mouth is read, so nothing is dropped",
            lamp_eyes::speaking::OUTLIER_ENVIRONMENT,
            lamp_eyes::speaking::ENVIRONMENT
        );
    }
    // With LAMP_EYES_SPEAKING_ALL_FACES=on, more than the largest face's
    // mouth is meshed each look (the first builder's 08:49 on Queue #3126).
    let all_faces_meshed = std::env::var(lamp_eyes::mesh::ALL_FACES_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if all_faces_meshed && mesh.is_none() {
        eprintln!(
            "lamp-eyes: {} is on without --mouth-model: no mouth is meshed",
            lamp_eyes::mesh::ALL_FACES_ENVIRONMENT
        );
    }
    let mouth_full_look = std::env::var(lamp_eyes::mesh::FULL_LOOK_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    let voice_bus = answered
        .as_ref()
        .and_then(|_| lamp_bus::Bus::from_environment().ok());
    // Faces that never move, left out when asked.
    let mut still = arguments.leave_still.then(lamp_eyes::still::Still::default);
    let mut epoch = 0u64;
    let mut port: Option<LinuxPort> = None;
    let permit = lamp_eyes::permit::Permit::listen(permits)?;
    // The receipt the voice asked for a close, said once the camera is shut.
    let mut receipt = lamp_eyes::permit::CloseReceipt::default();
    let mut retry_at = Instant::now();
    let mut frame = vec![0u8; MAX_FRAME_BYTES];
    // Bytes of a frame that has not been looked at yet.
    let mut latest: Option<usize> = None;
    // When that frame was captured, monotonic µs: a photo yes is looked at
    // only on a frame from after it (#2856).
    let mut latest_us = 0u64;
    let mut looked_at = Instant::now();
    let mut face_at: Option<Instant> = None;
    // When any face was last found, even one too small to follow: with
    // `--look-on-change`, a frame like the last needs no detector only
    // once nobody has been found for a while.
    let mut any_face_at: Option<Instant> = None;
    let mut change = arguments.look_on_change.then(|| {
        let mut change = lamp_eyes::change::Change::default();
        if arguments.change_persist {
            change.persist_2();
        }
        change
    });
    // With it, the lamp's own light, read from the bus at each glance: a
    // change while the ring's light changes is the lamp's own.
    let own_bus = arguments
        .look_on_change
        .then(|| lamp_bus::Bus::from_environment().ok())
        .flatten();
    let mut own_light = lamp_eyes::change::OwnLight::default();
    // With --glance-log, each compared glance's change counts and cell mask,
    // no picture, for replaying the change bar's rules offline.
    let mut glance_log = match &arguments.glance_log {
        Some(path) => {
            if let Some(change) = change.as_mut() {
                change.keep_glances();
            }
            Some(lamp_eyes::glance_log::Log::open(path)?)
        }
        None => None,
    };
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
            bus.report(camera, aside, reopen.resets(), now);
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
            if who.remember_speaker {
                who.speaker_anchors
                    .refresh(lamp_ipc::monotonic_us(), port.as_ref().and(permit.held()));
            }
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
                let ask = match live_ask_of(line.trim_ascii(), who.remember_speaker) {
                    Ok(ask) => ask,
                    Err(error) => {
                        answer(&mut stream, serde_json::json!({"error": error}));
                        continue;
                    }
                };
                let reply = match Some(ask) {
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
                    // Only lamp-cloud's ask, for the owner's page that sent
                    // them (`face.list`, #2192): with --photo-dir, the faces
                    // from photos that wait for a yes too.
                    Some(Ask::ListWaiting) => serde_json::json!({
                        "people": who.people.names(),
                        "waiting": who
                            .unconfirmed
                            .as_ref()
                            .map(|unconfirmed| unconfirmed.names())
                            .unwrap_or_default(),
                    }),
                    Some(Ask::Forget(name)) => {
                        if who.remember_speaker {
                            who.speaker_anchors.clear();
                            if who
                                .taking
                                .as_ref()
                                .is_some_and(|(taking, _)| taking.speaker.is_some())
                                && let Some((_, mut caller)) = who.taking.take()
                            {
                                answer(&mut caller, serde_json::json!({"error": "forgotten"}));
                            }
                        }
                        // "Forget my face" takes a face from a photo too, and
                        // one let go is a face forgotten.
                        // A let-go its store couldn't write is an error, not
                        // "not known": after a restart that face would be back
                        // (the reviewer on #2199). The people's forget runs
                        // either way.
                        let let_go = match who.unconfirmed.as_mut() {
                            Some(unconfirmed) => {
                                unconfirmed.let_go(&name).map_err(|error| error.to_string())
                            }
                            None => Ok(false),
                        };
                        // Their memory id, said with the answer so the app's
                        // remove forgets their memory, kept under it since a
                        // rename (#2176): only with LAMP_MEMORY_STABLE_ID=on.
                        let id = who.people.id_of(&name);
                        let forgot = who.people.forget(&name).map_err(|error| error.to_string());
                        // Whether memory's by-name id for the name is nobody
                        // else's now: a person renamed from it keeps it, and
                        // the app's remove then leaves it (#2330).
                        let name_id_free = who.people.name_id_free(&name);
                        match let_go.and_then(|let_go| {
                            forgot.map(|forgot| lamp_eyes::who::Forgot {
                                forgot: forgot.forgot || let_go,
                                ..forgot
                            })
                        }) {
                            // Set-aside files it could not search: said, with the
                            // forget waiting for them (#1586).
                            Ok(lamp_eyes::who::Forgot { forgot, unsearched }) if unsearched > 0 => {
                                eprintln!(
                                    "lamp-eyes: asked to forget a face; forgotten: {forgot}; {unsearched} set-aside files could not be searched, and the forget waits for them"
                                );
                                who.known = None;
                                let mut answer = serde_json::json!({"forgot": forgot, "unsearched": unsearched, "name_id_free": name_id_free});
                                if let Some(id) = id {
                                    answer["id"] = id.into();
                                }
                                answer
                            }
                            Ok(lamp_eyes::who::Forgot { forgot, .. }) => {
                                eprintln!("lamp-eyes: asked to forget a face; forgotten: {forgot}");
                                who.known = None;
                                let mut answer = serde_json::json!({"forgot": forgot, "name_id_free": name_id_free});
                                if let Some(id) = id {
                                    answer["id"] = id.into();
                                }
                                answer
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
                            (Some(dir), Some(_)) if lamp_eyes::photo::ready(dir).is_err() => {
                                eprintln!(
                                    "lamp-eyes: did not add a face from a photo: the photo folder isn't ready"
                                );
                                serde_json::json!({"error": "photos_off"})
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
                    // With whether that face was asked about: one added again
                    // under the same name is a new face, not asked yet, so the
                    // voice asks it once more (Mobile on #1968, lamp-0c89).
                    Some(Ask::PhotoHere) => match &who.unconfirmed {
                        None => serde_json::json!({"error": "photos_off"}),
                        Some(unconfirmed) => lamp_eyes::photo::here_answer(
                            unconfirmed,
                            who.photo_here
                                .as_ref()
                                .map(|(name, _, at)| (name.as_str(), at.elapsed())),
                            who.photo_here
                                .as_ref()
                                .and_then(|(_, face, _)| face.as_deref()),
                            who.photo_speaker,
                            KNOWN_FOR,
                        ),
                    },
                    // Taken only for the face at the lamp now (the reviewer on
                    // #1968, 22:28): never for a face elsewhere.
                    Some(Ask::PhotoAsked(name)) => {
                        let here = lamp_eyes::photo::is_here(
                            who.photo_here
                                .as_ref()
                                .map(|(n, _, at)| (n.as_str(), at.elapsed())),
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
                    // With the switch off, a yes naming a face, even as
                    // `null`, is not understood, as before it (#2856).
                    Some(Ask::PhotoYes { face: Some(_), .. }) if !who.photo_speaker => {
                        serde_json::json!({"error": "not_understood"})
                    }
                    // A yes only from the face here, and only once it was
                    // asked; with `LAMP_PHOTO_SPEAKER=on`, only with the id
                    // of the face that matched the photo (#2856).
                    Some(Ask::PhotoYes { name, face }) => {
                        let here = lamp_eyes::photo::is_here(
                            who.photo_here
                                .as_ref()
                                .map(|(n, _, at)| (n.as_str(), at.elapsed())),
                            &name,
                            KNOWN_FOR,
                        );
                        match who.unconfirmed.as_mut() {
                            None => serde_json::json!({"error": "photos_off"}),
                            Some(_) if !here => serde_json::json!({"error": "not_here"}),
                            Some(_)
                                if who.photo_speaker
                                    && !lamp_eyes::photo::is_that_face(
                                        who.photo_here
                                            .as_ref()
                                            .and_then(|(_, here, _)| here.as_deref()),
                                        face.as_ref().and_then(Option::as_deref),
                                    ) =>
                            {
                                serde_json::json!({"error": "not_that_face"})
                            }
                            Some(unconfirmed) if !unconfirmed.was_asked(&name) => {
                                serde_json::json!({"error": "not_asked"})
                            }
                            // With `LAMP_PHOTO_SPEAKER=on`, kept only on a look
                            // forced after it (`photo::yes_after_look`), so a
                            // face that took the photo face's passing id can't
                            // keep it (reviewer-three's P2 on #2856).
                            Some(_) if who.photo_speaker && who.photo_yes.is_some() => {
                                serde_json::json!({"error": "busy"})
                            }
                            // Pinned to this record of the name and this hold
                            // of the camera's permission (#2856).
                            Some(unconfirmed) if who.photo_speaker => {
                                match (unconfirmed.record_of(&name), permit.held()) {
                                    (None, _) => serde_json::json!({"error": "not_unconfirmed"}),
                                    (_, None) => serde_json::json!({"error": "eyes_shut"}),
                                    (Some(record), Some(hold)) => {
                                        who.photo_yes = Some(lamp_eyes::photo::PendingYes::new(
                                            name,
                                            face.flatten(),
                                            stream,
                                            record,
                                            hold,
                                            lamp_ipc::monotonic_us(),
                                        ));
                                        who.looked_for = None;
                                        continue;
                                    }
                                }
                            }
                            Some(_) => keep_photo_yes(who, &name),
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
                    Some(Ask::RememberSpeaker(_)) if who.taking.is_some() => {
                        serde_json::json!({"error": "busy"})
                    }
                    Some(Ask::RememberSpeaker(_)) if port.is_none() => {
                        serde_json::json!({"error": "eyes_shut"})
                    }
                    Some(Ask::RememberSpeaker(request)) => {
                        let now = lamp_ipc::monotonic_us();
                        match who.speaker_anchors.capture(&request, now, permit.held()) {
                            Err(error) => serde_json::json!({"error": error}),
                            Ok(taking) => {
                                who.taking = Some((taking, stream));
                                continue;
                            }
                        }
                    }
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
                    Some(Ask::RememberOne {
                        name,
                        replace,
                        apart,
                    }) => match plain_name(&name) {
                        Some(name) => {
                            eprintln!(
                                "lamp-eyes: asked to remember a face, one name for it{}",
                                if replace { ", in place of another" } else { "" }
                            );
                            let mut taking = Taking::new(name, lamp_ipc::monotonic_us());
                            taking.one_face = Some(replace);
                            taking.apart = apart;
                            who.taking = Some((taking, stream));
                            continue;
                        }
                        None => serde_json::json!({"error": "not_a_name"}),
                    },
                    // Who was here's names (#1997): a yes only from the known
                    // face at the lamp now, so nothing elsewhere can say yes for
                    // someone; a no at any time, since it only narrows.
                    Some(Ask::VisitsYes(_) | Ask::VisitsNo(_))
                        if arguments.known_file.is_none() =>
                    {
                        serde_json::json!({"error": "tellable_off"})
                    }
                    Some(Ask::VisitsYes(name))
                        if !lamp_eyes::photo::is_here(
                            who.known
                                .as_ref()
                                .map(|known| (known.0.as_str(), known.2.elapsed())),
                            &name,
                            KNOWN_FOR,
                        ) =>
                    {
                        serde_json::json!({"error": "not_here"})
                    }
                    Some(Ask::VisitsYes(name)) => visits_reply(&mut who.people, &name, true),
                    Some(Ask::VisitsNo(name)) => visits_reply(&mut who.people, &name, false),
                    None => serde_json::json!({"error": "not_understood"}),
                };
                answer(&mut stream, reply);
                // After a yes, a no, a forget or a rename: the names as they are.
                if let Some(known) = &arguments.known_file {
                    lamp_eyes::who::keep_tellable(
                        &lamp_eyes::who::tellable_of(known),
                        &who.people.tellable(),
                    );
                }
            }
            // Remembering ends with enough prints, or without them in time.
            if let Some((mut taking, mut stream)) = who.taking.take() {
                let refused = taking.speaker.as_ref().and_then(|guard| {
                    guard
                        .checked(
                            lamp_ipc::monotonic_us(),
                            port.as_ref().and(permit.held()),
                            lamp_eyes::photo::Caller::closed(&stream),
                        )
                        .err()
                });
                if let Some(error) = refused {
                    answer(&mut stream, serde_json::json!({"error": error}));
                } else if let Some(prints) = taking.taken() {
                    let now_unix_s = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |since| since.as_secs());
                    // The final fence is separate from the per-loop check:
                    // a closed caller or changed permission cannot write a
                    // completed capture while the loop was otherwise busy.
                    if let Some(error) = taking.speaker.as_ref().and_then(|guard| {
                        guard
                            .checked(
                                lamp_ipc::monotonic_us(),
                                port.as_ref().and(permit.held()),
                                lamp_eyes::photo::Caller::closed(&stream),
                            )
                            .err()
                    }) {
                        answer(&mut stream, serde_json::json!({"error": error}));
                        continue;
                    }
                    let kept = match taking.one_face {
                        None => who
                            .people
                            .remember(&taking.name, prints, now_unix_s)
                            .map_err(|error| who::Refused::Store(error.to_string())),
                        Some(replace) => who.people.remember_one(
                            &taking.name,
                            prints,
                            now_unix_s,
                            replace,
                            taking.apart,
                        ),
                    };
                    lamp_eyes::who::after_remembering(&mut who.looked_for, kept.is_ok());
                    let reply = kept_reply(kept, who.visits, who.people.names().len());
                    answer(&mut stream, reply);
                    if let Some(known) = &arguments.known_file {
                        lamp_eyes::who::keep_tellable(
                            &lamp_eyes::who::tellable_of(known),
                            &who.people.tellable(),
                        );
                    }
                } else if let Some(why) = taking.given_up(lamp_ipc::monotonic_us()) {
                    eprintln!("lamp-eyes: did not remember a face: {}", why.code());
                    answer(&mut stream, serde_json::json!({"error": why.code()}));
                } else if port.is_none() {
                    answer(&mut stream, serde_json::json!({"error": "eyes_shut"}));
                } else {
                    who.taking = Some((taking, stream));
                }
            }
            // A photo yes whose forced look did not come in time, or whose
            // camera was shut meanwhile, before the closed camera's turn
            // below: not kept, for good (#2856).
            if let Some(pending) = who.photo_yes.take() {
                match pending.checked(lamp_ipc::monotonic_us(), permit.held()) {
                    Ok(ended) => photo_yes_ended(who, ended, arguments.known_file.as_deref()),
                    Err(pending) => who.photo_yes = Some(pending),
                }
            }
        }
        for datagram in permit.heard() {
            if let Some(close) = lamp_eyes::permit::receipt_asked(&datagram) {
                receipt.asked(close);
                continue;
            }
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
                receipt.stopped(open.stop(budget(STOP)));
                eprintln!("lamp-eyes: camera closed: not permitted");
                tell(Seen { face: None });
            }
            // With `LAMP_FACE_FULL_CROP=on`, the camera closed lets the one
            // known go at once: nothing is held while it cannot see.
            if let Some(who) = who.as_mut().filter(|who| who.held.on())
                && who.known.take().is_some()
            {
                who.held.let_go();
                if let Some(path) = &arguments.known_file {
                    lamp_eyes::who::keep_known(path, None, None, None);
                }
            }
            camera = lamp_eyes::bus_out::Camera::Closed;
            // After the stop has returned, never before (#1802).
            if let Some(closed) = receipt.due(port.is_none()) {
                eprintln!(
                    "lamp-eyes: camera closed for the voice's asked close; released: {}",
                    closed.released
                );
                if let Some(bus) = bus_out.as_mut() {
                    bus.camera_closed(closed, lamp_ipc::monotonic_us());
                }
            }
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
            // The node and port: as given, or the one camera with the ids,
            // at --usb's port when it names one (#3504) or else on any port,
            // found now, since a reset or a replug may move it (#2190).
            let (device, usb_port) = match &arguments.device {
                Some(device) => (device.clone(), arguments.usb.topology.clone()),
                None => match lamp_camera::linux::find_camera(
                    arguments.usb.vendor,
                    arguments.usb.product,
                    &arguments.usb.topology,
                ) {
                    Ok(Ok(found)) => found,
                    other => {
                        let why = match &other {
                            Ok(Err(lamp_camera::linux::NotFound::None))
                                if arguments.usb.topology.is_empty() =>
                            {
                                "no camera with those ids".to_owned()
                            }
                            Ok(Err(lamp_camera::linux::NotFound::None)) => format!(
                                "no camera with those ids at port {}",
                                arguments.usb.topology
                            ),
                            Ok(Err(lamp_camera::linux::NotFound::Several(ports)))
                                if !arguments.usb.topology.is_empty() =>
                            {
                                format!(
                                    "{} capture nodes with those ids at port {}",
                                    ports.len(),
                                    arguments.usb.topology
                                )
                            }
                            Ok(Err(lamp_camera::linux::NotFound::Several(ports))) => format!(
                                "{} cameras with those ids, on {}: name one's port in --usb",
                                ports.len(),
                                ports.join(", ")
                            ),
                            Err(error) => format!("the cameras could not be listed: {error:?}"),
                            Ok(Ok(_)) => unreachable!(),
                        };
                        if not_found.as_deref() != Some(why.as_str()) {
                            eprintln!(
                                "lamp-eyes: camera {:04x}:{:04x} not opened: {why}",
                                arguments.usb.vendor, arguments.usb.product
                            );
                            not_found = Some(why);
                        }
                        camera = lamp_eyes::bus_out::Camera::NotOpening;
                        retry_at = Instant::now() + RETRY;
                        // Gone from its port's capture nodes but found
                        // there before (a probe that failed after a reset):
                        // a failed open at that port, so a stuck camera
                        // without a node is reset too (the reviewer's P3 on
                        // #2215). `reset_usb` checks the ids there.
                        if let (Ok(Err(lamp_camera::linux::NotFound::None)), Some(at)) =
                            (&other, found_on.as_deref())
                        {
                            let failed = reopen.failed(permit.holds());
                            if failed.reset {
                                reset_camera(&arguments.usb, at, &mut reopen);
                            }
                            if permit.holds() {
                                ask_helper(
                                    arguments.camera_helper.as_deref(),
                                    failed.failed,
                                    &mut helper_asked,
                                );
                            }
                            retry_at = Instant::now() + failed.retry_after;
                        }
                        continue;
                    }
                },
            };
            if not_found.take().is_some() || found_on.as_deref() != Some(usb_port.as_str()) {
                if arguments.device.is_none() {
                    eprintln!(
                        "lamp-eyes: camera found at {} on port {usb_port}",
                        device.display()
                    );
                }
                found_on = Some(usb_port.clone());
            }
            epoch += 1;
            let config = LinuxConfig::new(
                device,
                arguments.lock_directory.clone(),
                source,
                UsbIdentity {
                    vendor: arguments.usb.vendor,
                    product: arguments.usb.product,
                    topology: usb_port.clone(),
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
                        receipt.stopped(opening.stop(budget(STOP)));
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
                    if let Some(failed) = reopen.opened() {
                        eprintln!(
                            "lamp-eyes: camera open again after {failed} opens in a row that did not"
                        );
                    }
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
                    receipt.stopped(opening.stop(budget(STOP)));
                    camera = lamp_eyes::bus_out::Camera::NotOpening;
                    // The full size first, then the two in turn.
                    failures = failures.saturating_add(1);
                    let failed = reopen.failed(permit.holds());
                    if failed.stuck {
                        eprintln!(
                            "lamp-eyes: camera did not open {} times in a row, the last {error:?}: the device seems stuck (minutes, or until a reboot); tried again every {} s now",
                            failed.failed,
                            lamp_eyes::reopen::RETRY_STUCK.as_secs()
                        );
                    }
                    if failed.reset {
                        // Closed above; nothing of ours holds the device.
                        reset_camera(&arguments.usb, &usb_port, &mut reopen);
                    }
                    // Only while seeing is permitted, as the reset (the
                    // reviewer's P3 on #2129).
                    if permit.holds() {
                        ask_helper(
                            arguments.camera_helper.as_deref(),
                            failed.failed,
                            &mut helper_asked,
                        );
                    }
                    retry_at = Instant::now() + failed.retry_after;
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
                        latest_us = lamp_eyes::photo::frame_us(got.timestamp);
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
                receipt.stopped(open.stop(budget(STOP)));
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
            receipt.stopped(open.stop(budget(STOP)));
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
            let frame_us = latest_us;
            let frame_hold = permit.held();
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
            // With `--sight-from-look` it is made from the look's own
            // picture, once decoded, below.
            if let Some(kept) = kept.as_mut() {
                if arguments.full_look.is_none() {
                    let _ = kept.keep(&frame[..used]);
                } else if !arguments.sight_from_look
                    && let Ok(working) = lamp_eyes::full_look::working(&frame[..used])
                {
                    let _ = kept.keep(&working);
                }
            }
            stage(&mut stage_at, &mut stages[0]);
            let busy = lamp_eyes::change::busy(
                wanted.is_some(),
                who.as_ref()
                    .is_some_and(|who| who.taking.is_some() || who.photo_yes.is_some()),
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
            // With `LAMP_FACE_FULL_CROP=on`, the decoded frame is kept for
            // this look, for the near face's print (`who::align_full`), and
            // with `LAMP_EYES_MOUTH_FULL_LOOK=on`, `--full-look` and
            // `--mouth-model`, for the mouths' mesh
            // (`mesh::crop_full`): a seated face's eyes are about 20 of the
            // picture's pixels apart, too few for the mesh to be judged on.
            // The picture is shrunk from that same decode, so keeping it
            // costs no second decode.
            let full_crop = who.as_ref().is_some_and(|who| who.held.on());
            let full_mouth = mouth_full_look && arguments.full_look.is_some() && mesh.is_some();
            let (decoded, full_frame) = if full_crop || full_mouth {
                match Picture::from_jpeg_full(&frame[..used]) {
                    Ok((picture, full)) => (Ok(picture), Some(full)),
                    Err(error) => (Err(error), None),
                }
            } else {
                (Picture::from_jpeg(&frame[..used]), None)
            };
            stage(&mut stage_at, &mut stages[1]);
            if arguments.sight_from_look
                && let (Some(kept), Ok(picture)) = (kept.as_mut(), &decoded)
                && let Ok(working) = lamp_eyes::full_look::working_from_picture(picture)
            {
                let _ = kept.keep(&working);
            }
            stage(&mut stage_at, &mut stages[0]);
            // Set when this look sends nothing (an answer's held face is gone).
            let mut quiet = false;
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
                        if let (Some(log), Some(glance)) = (
                            glance_log.as_mut(),
                            change.as_mut().and_then(|change| change.take_glance()),
                        ) && let Some(said) = log.write(&glance, faces.len())
                        {
                            eprintln!("lamp-eyes: {said}");
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
                        let mouth_wanted = heard_lines.is_none() || heard.hears(now_us);
                        // The largest face, and with LAMP_EYES_SPEAKING_ALL_FACES=on
                        // up to two more within the look's mesh budget.
                        let heights: Vec<f32> = faces.iter().map(|face| face.height).collect();
                        // Each meshed face's points this look, for a
                        // Blendshape V2 read due on it below.
                        let mut meshed: Vec<(usize, Vec<f32>)> = Vec::new();
                        // The frame at its own size the mouths are cut
                        // from, with `--full-look`, and how many of its
                        // pixels a detector pixel is.
                        let mouth_frame = full_frame.as_ref().filter(|_| full_mouth);
                        let mesh_scale = mouth_frame.map_or(1.0, |frame| 1.0 / frame.scale);
                        if let Some(mesh) = mesh.as_mut()
                            && mouth_wanted
                        {
                            let began = Instant::now();
                            let mut last = Duration::ZERO;
                            for (nth, index) in lamp_eyes::mesh::meshed_order(&heights, all_faces_meshed)
                                .into_iter()
                                .enumerate()
                            {
                                if nth > 0 && !lamp_eyes::mesh::more_faces(began.elapsed(), last) {
                                    break;
                                }
                                let face = &faces[index];
                                let one = Instant::now();
                                let open = lamp_eyes::mesh::crop_of(&picture, mouth_frame, face)
                                    .map(|input| mesh.landmarks_of(input))
                                    .transpose()
                                    .map(|landmarks| {
                                        landmarks.and_then(|landmarks| {
                                            let open = lamp_eyes::mesh::mouth_open(&landmarks);
                                            meshed.push((index, landmarks));
                                            open
                                        })
                                    });
                                last = one.elapsed();
                                let eyes = lamp_eyes::speaking::shape(&face.marks)
                                    .map_or(0.0, |shape| shape.1);
                                match open {
                                    Ok(Some(open)) => {
                                        opens[index] = Some(open);
                                        // Up to four a second a face: only when
                                        // asked for, never in the product's journal.
                                        if arguments.mouth_diagnostics {
                                            eprintln!(
                                                "lamp-eyes: mouth open {open:.4} eyes {eyes:.0} px ({:.0} px meshed) mesh {} ms",
                                                eyes * mesh_scale,
                                                last.as_millis()
                                            );
                                        }
                                    }
                                    Ok(None) => {}
                                    Err(error) => eprintln!("lamp-eyes: mouth not read: {error}"),
                                }
                            }
                        }
                        // The largest face's mouth, offered for a yawn with
                        // its points while open wide, only while the yes is
                        // kept; without it the sitting's count is forgotten.
                        let mut yawns_yes = None;
                        let mut yawned = 0;
                        if let Some((offer, done, asked, yes_epoch)) = yawning.as_mut() {
                            let yes = match *asked {
                                Some((at, yes)) if at.elapsed() < Duration::from_secs(2) => yes,
                                _ => {
                                    let yes = lamp_eyes::seems::face_expressions_yes(
                                        std::path::Path::new(lamp_eyes::seems::FACE_EXPRESSIONS_FILE),
                                    );
                                    *asked = Some((Instant::now(), yes));
                                    yes
                                }
                            };
                            yawns_yes = Some(yes);
                            // Without the yes, nothing begun before it counts after.
                            if !yes {
                                *yes_epoch += 1;
                            }
                            if yes && let Some(hold) = frame_hold {
                                let largest = heights
                                    .iter()
                                    .enumerate()
                                    .max_by(|a, b| a.1.total_cmp(b.1))
                                    .map(|(index, _)| index);
                                let face = meshed
                                    .iter()
                                    .find(|(index, _)| Some(*index) == largest)
                                    .and_then(|(index, landmarks)| {
                                        let open = lamp_eyes::mesh::mouth_open(landmarks)?;
                                        let points = lamp_eyes::yawn::wants_scores(open)
                                            .then(|| {
                                                lamp_eyes::mesh::eye_line(&faces[*index]).and_then(
                                                    |eye_line| {
                                                        lamp_eyes::blendshapes::points(landmarks, eye_line)
                                                    },
                                                )
                                            })
                                            .flatten();
                                        Some((open, points))
                                    });
                                let grant = lamp_eyes::yawn::Grant { hold, yes: *yes_epoch };
                                // A camera whose timestamps aren't the host's clock
                                // gives 0: the look's own time stands in for it.
                                let captured_us = if frame_us > 0 { frame_us } else { lamp_ipc::monotonic_us() };
                                offer.offer((grant, captured_us, face));
                            }
                            // Only under the hold and the yes the look has now, from a
                            // frame not too old: one finished across a close is dropped.
                            let now = permit.held().map(|hold| lamp_eyes::yawn::Grant { hold, yes: *yes_epoch });
                            yawned = lamp_eyes::yawn::current_counts(done.try_iter(), now, lamp_ipc::monotonic_us());
                        }
                        if let Some(bus) = bus_out.as_mut() {
                            let now = lamp_ipc::monotonic_us();
                            bus.set_mesh_scale(mesh_scale);
                            bus.saw(&faces, &opens, picture.filled, now);
                            if let Some(yes) = yawns_yes {
                                bus.activity_yawns_allowed(yes);
                            }
                            for _ in 0..yawned {
                                bus.activity_yawned(now);
                            }
                            if let Some(answered) = answered.as_mut() {
                                answered.looked(now, bus.talking());
                            }
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
                            && lamp_eyes::who::recognise_due(
                                who.looked_for,
                                who.taking.is_some(),
                                Instant::now(),
                            )
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
                                .and_then(|(_, (face, _))| match full_frame.as_ref().filter(|_| full_crop) {
                                    Some(full) => lamp_eyes::who::align_full(full, &face.marks),
                                    None => align(&picture, &face.marks),
                                })
                                .and_then(|aligned| who.recogniser.print(&aligned).ok());
                            // Each check with a face in view, at most every
                            // two seconds: the best likeness and the largest
                            // face's height, never a name.
                            if who.taking.is_none()
                                && let Some(tallest) = heights.iter().copied().reduce(f32::max)
                            {
                                let best = print
                                    .as_ref()
                                    .and_then(|print| who.people.best_likeness(print))
                                    .map_or_else(|| "none".to_owned(), |best| format!("{best:.3}"));
                                eprintln!(
                                    "lamp-eyes: recognition best {best} height {tallest:.3} (near at {NEAR_ENOUGH}, same at {})",
                                    lamp_eyes::who::SAME_PERSON
                                );
                            }
                            let near_face = near_id
                                .as_ref()
                                .zip(bus_out.as_ref())
                                .map(|(id, bus)| format!("{}/{id}", bus.faces_boot()));
                            // Reuse this recognition print only: no extra
                            // inference or disk write for enrollment evidence.
                            // The passing id alone does not mean it spoke.
                            if who.remember_speaker
                                && heights.iter().filter(|h| **h >= NEAR_ENOUGH).count() == 1
                                && let (Some(face), Some(print), Some(bus)) =
                                    (near_face.as_deref(), print.as_deref(), bus_out.as_ref())
                                && bus.talking().iter().any(|id| near_id.as_deref() == Some(format!("f{id}").as_str()))
                            {
                                who.speaker_anchors.observe(face, print, frame_us,
                                    lamp_ipc::monotonic_us(), frame_hold, permit.held());
                            }
                            // The face from a photo the near print matched.
                            let mut matched = None;
                            if let Some((taking, _)) = who.taking.as_mut() {
                                taking.look_seen(&heights, print, near_face.as_deref(),
                                    frame_us, lamp_ipc::monotonic_us(), permit.held());
                            } else if let Some(print) = print {
                                let now = Instant::now();
                                let found = who
                                    .people
                                    .who(&print)
                                    .map(|(name, likeness)| (name, likeness, now));
                                // Nobody who asked: a face from a photo, maybe,
                                // never named, only noted so the voice can ask.
                                if found.is_none() {
                                    matched = who
                                        .unconfirmed
                                        .as_ref()
                                        .and_then(|unconfirmed| unconfirmed.here(&print));
                                }
                                // With `LAMP_FACE_FULL_CROP=on`, a miss within
                                // the hold keeps the one known, as they were,
                                // only while the face still looks like them
                                // (`who::Held::keeps_print`): a stranger's
                                // face lets them go at once (#3285).
                                let held = found.is_none()
                                    && matched.is_none()
                                    && who.known.as_ref().is_some_and(|known| {
                                        who.held.keeps_print(
                                            now,
                                            who.people.likeness_to(&known.0, &print),
                                            near_id.as_deref(),
                                        )
                                    });
                                if held {
                                    if let Some(known) = who.known.as_mut() {
                                        known.2 = now;
                                    }
                                } else {
                                    if found.is_some() {
                                        who.held.matched(now, near_id.clone());
                                    } else {
                                        who.held.let_go();
                                    }
                                    who.known = found;
                                }
                                // A print not the photo's: no longer here
                                // (`photo::print_is_photos`, #2856).
                                if who.photo_speaker
                                    && who.photo_here.as_ref().is_some_and(|(here, ..)| {
                                        !lamp_eyes::photo::print_is_photos(here, matched.as_deref())
                                    })
                                {
                                    who.photo_here = None;
                                }
                                if let Some(name) = matched.clone()
                                    && let Some(unconfirmed) = who.unconfirmed.as_mut()
                                {
                                    // As the lamp sees them, kept beside the
                                    // photo's on their yes (#2854).
                                    unconfirmed.saw(&name, print.clone());
                                    who.photo_seen.retain(|(seen, _)| seen != &name);
                                    who.photo_seen.push((name.clone(), lamp_ipc::monotonic_us()));
                                    who.photo_here = Some((name, near_face.clone(), Instant::now()));
                                }
                                if who.visits && !held {
                                    let level = familiar(&mut who.people, who.known.as_ref());
                                    if let Some(bus) = bus_out.as_mut() {
                                        bus.familiar(level, lamp_ipc::monotonic_us());
                                    }
                                }
                                if let Some(path) = arguments.known_file.as_ref().filter(|_| !held) {
                                    let name = who.known.as_ref().map(|known| known.0.as_str());
                                    let id = name.and_then(|name| who.people.id_of(name));
                                    lamp_eyes::who::keep_known(
                                        path,
                                        name,
                                        id.as_deref(),
                                        near_id.as_deref(),
                                    );
                                }
                            } else if who.known.is_some()
                                && who.held.keeps_unprinted(
                                    Instant::now(),
                                    near.is_some(),
                                    near_id.as_deref(),
                                )
                            {
                                // No print, within the hold
                                // (`LAMP_FACE_FULL_CROP=on`): the one known
                                // stays, as they were, only if nobody is near
                                // or the near face has their passing id
                                // (`who::Held::keeps_unprinted`, #3285).
                                if let Some(known) = who.known.as_mut() {
                                    known.2 = Instant::now();
                                }
                            } else {
                                // Nobody near: nobody known at the lamp.
                                if who.held.on() {
                                    who.held.let_go();
                                    who.known = None;
                                }
                                if let Some(path) = &arguments.known_file {
                                    lamp_eyes::who::keep_known(path, None, None, None);
                                }
                                if who.visits
                                    && let Some(bus) = bus_out.as_mut()
                                {
                                    bus.familiar(None, lamp_ipc::monotonic_us());
                                }
                            }
                            // The look forced by a photo yes, on a frame
                            // captured after it: its deadline, the camera's
                            // hold and the name's record checked here, as it
                            // is kept (`photo::PendingYes::looked`, #2856).
                            if let Some(pending) = who.photo_yes.take() {
                                let look = lamp_eyes::photo::YesLook {
                                    frame_us,
                                    now_us: lamp_ipc::monotonic_us(),
                                    hold: permit.held(),
                                    record: who.unconfirmed.as_ref().and_then(|unconfirmed| {
                                        unconfirmed.record_of(pending.name())
                                    }),
                                    matched: matched.as_deref(),
                                    face: near_face.as_deref(),
                                };
                                match pending.looked(&look) {
                                    Ok(ended) => photo_yes_ended(
                                        who,
                                        ended,
                                        arguments.known_file.as_deref(),
                                    ),
                                    // A frame from before it: the next look.
                                    Err(pending) => {
                                        who.photo_yes = Some(pending);
                                        who.looked_for = None;
                                    }
                                }
                            }
                        }
                        // How the near face seems, at most every 2 s and
                        // only with the lamp's kept yes, for everyone it sees
                        // (privacy-switch.md):
                        // a kind and a confidence on the bus, nothing kept.
                        // One reader a look, so two readers' times never
                        // add up in one look's delay (the reviewer on
                        // #2132): the other's turn comes on the next.
                        // Blendshape V2 adds none: its read is offered to
                        // its worker and taken here once done.
                        let mut read_this_look = false;
                        // The face the seems worker reads for how it seems.
                        let mut seems_face = None;
                        for seems in seems.iter_mut() {
                            // A far face, its eyes too close together to
                            // read, gets no read rather than `unsure`.
                            let near = lamp_eyes::seems::near_face(
                                &faces,
                                picture.filled.1.max(1) as f32,
                            );
                            let due = !read_this_look
                                && near.is_some()
                                && match lamp_eyes::seems::due(
                                    seems.read_at,
                                    std::path::Path::new(lamp_eyes::seems::FACE_EXPRESSIONS_FILE),
                                ) {
                                    lamp_eyes::seems::Due::NotYet => false,
                                    lamp_eyes::seems::Due::NoYes => {
                                        // Asked again in 2 s, not every look.
                                        seems.read_at = Some(Instant::now());
                                        if face_yes_told != Some(false) {
                                            eprintln!(
                                                "lamp-eyes: no yes to face reads in {}: no face is read",
                                                lamp_eyes::seems::FACE_EXPRESSIONS_FILE
                                            );
                                            face_yes_told = Some(false);
                                        }
                                        false
                                    }
                                    lamp_eyes::seems::Due::Now => {
                                        if face_yes_told != Some(true) {
                                            eprintln!(
                                                "lamp-eyes: a yes to face reads in {}: the near face is read",
                                                lamp_eyes::seems::FACE_EXPRESSIONS_FILE
                                            );
                                            face_yes_told = Some(true);
                                        }
                                        true
                                    }
                                };
                            // FER+ reads the face cut out upright, once its
                            // read is due; a face that can't be is not read,
                            // and the next look tries again, as before
                            // Blendshape V2 (the reviewer on #2132).
                            let aligned = match (&seems.reader, near) {
                                (Reader::FerPlus(_), Some((_, face))) if due => {
                                    align(&picture, &face.marks)
                                }
                                _ => None,
                            };
                            let mut reads: Vec<SeemsRead> = Vec::new();
                            if let Some((index, face)) = near
                                && due
                                && (aligned.is_some()
                                    || !matches!(seems.reader, Reader::FerPlus(_)))
                            {
                                let began = Instant::now();
                                seems.read_at = Some(began);
                                read_this_look = true;
                                let at_us = lamp_ipc::monotonic_us();
                                match &seems.reader {
                                    Reader::FerPlus(reader) => {
                                        let read = aligned
                                            .map(|aligned| reader.read(&aligned))
                                            .unwrap_or_else(|| Err("no face to read".into()))
                                            .map_err(|error| error.to_string());
                                        reads.push((read, began.elapsed(), at_us));
                                    }
                                    // Offered below.
                                    Reader::Blendshapes { .. } => {
                                        seems_face = Some((index, face));
                                    }
                                }
                            }
                            if let Reader::Blendshapes { done, .. } = &seems.reader {
                                reads.extend(done.try_iter());
                            }
                            for (read, took, at_us) in reads {
                                match read {
                                    Ok(read) => {
                                        seems.reads += 1;
                                        seems.spent += took;
                                        seems.longest = seems.longest.max(took);
                                        if let Some(log) = seems_log.as_mut() {
                                            let unix_s = std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .map_or(0, |since| since.as_secs());
                                            if let Some(said) = log.write(&read, unix_s) {
                                                eprintln!("lamp-eyes: {said}");
                                            }
                                        }
                                        // As of when the face was seen, so a
                                        // read done off the look lasts no
                                        // longer for it (`HumanSeems::LASTS`).
                                        if let Some(bus) = bus_out.as_mut() {
                                            bus.seems(read, at_us);
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
                                        "lamp-eyes: {} face reads by {} in the last minute, {} ms each, the longest {} ms ({})",
                                        seems.reads,
                                        match seems.reader {
                                            Reader::FerPlus(_) => "FER+",
                                            Reader::Blendshapes { .. } => "Blendshape V2",
                                        },
                                        seems.spent.as_millis() / u128::from(seems.reads),
                                        seems.longest.as_millis(),
                                        match seems.reader {
                                            Reader::FerPlus(_) => "that look's delay",
                                            Reader::Blendshapes { .. } => "off the look, on its own thread",
                                        },
                                    );
                                }
                                seems.told_at = Instant::now();
                                seems.reads = 0;
                                seems.spent = Duration::ZERO;
                                seems.longest = Duration::ZERO;
                            }
                        }
                        // A Blendshape V2 read due: the points this look's
                        // mouth pass meshed for that face, or else its crop,
                        // offered to the seems worker; an older one not yet
                        // taken is dropped for it.
                        if let Some(latest) = seems.iter().find_map(|seems| match &seems.reader {
                            Reader::Blendshapes { latest, .. } => Some(latest),
                            Reader::FerPlus(_) => None,
                        }) && let Some((index, face)) = seems_face
                        {
                            let points = meshed
                                .iter()
                                .position(|(at, _)| *at == index)
                                .map(|at| meshed.swap_remove(at));
                            match lamp_eyes::blendshapes::input(points, index, || {
                                lamp_eyes::mesh::crop_of(&picture, mouth_frame, face)
                            })
                            .zip(lamp_eyes::mesh::eye_line(face))
                            {
                                Some((input, eye_line)) => {
                                    latest.offer((input, eye_line, lamp_ipc::monotonic_us()));
                                }
                                None => {
                                    eprintln!("lamp-eyes: the face was not read: no face to read");
                                }
                            }
                        }
                        let mut sight = choose(&faces, picture.filled);
                        // While the voice answers, the face seen asking, if
                        // one was picked as the answer began and is in view.
                        if let Some(answered) = answered.as_mut() {
                            let now_us = lamp_ipc::monotonic_us();
                            let state = voice_bus
                                .as_ref()
                                .and_then(|bus| bus.latest(lamp_msgs::VoiceState::TOPIC).ok().flatten())
                                .filter(|message| {
                                    now_us.saturating_sub(message.header.at_us) < lamp_eyes::answered::VOICE_FRESH_US
                                })
                                .and_then(|message| message.parse::<lamp_msgs::VoiceState>().ok())
                                .map(|state| state.state);
                            if let Some(id) = answered.voice(state, now_us) {
                                eprintln!("lamp-eyes: the answer turns to f{id}, the face seen asking");
                            }
                            let index = answered
                                .held()
                                .and_then(|id| bus_out.as_ref()?.index_of(id));
                            match answered.sight(&faces, index, picture.filled, sight) {
                                Some(answer) => sight = answer,
                                // The asker's passing id is gone: nothing
                                // is sent, so the body keeps its last aim.
                                None => quiet = true,
                            }
                        }
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
                    if !quiet {
                        tell(Seen { face });
                    }
                }
                Err(_) => {
                    unreadable += 1;
                    if let Some((taking, _)) = who.as_mut().and_then(|who| who.taking.as_mut())
                        && taking.speaker.is_some()
                    {
                        taking.look_seen(
                            &[],
                            None,
                            None,
                            frame_us,
                            lamp_ipc::monotonic_us(),
                            permit.held(),
                        );
                    }
                }
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
    fn mouth_diagnostics_is_explicit_default_off_and_accepts_only_on_or_off() {
        assert!(!parse(args(&FULL)).unwrap().unwrap().mouth_diagnostics);
        for bus in [false, true] {
            for speaking in [false, true] {
                assert!(diagnostic_requirements(false, bus, speaking).is_ok());
                assert_eq!(
                    diagnostic_requirements(true, bus, speaking).is_ok(),
                    bus && speaking
                );
            }
        }
        for (word, enabled) in [("on", true), ("off", false)] {
            let given = [&FULL[..], &["--mouth-diagnostics", word]].concat();
            assert_eq!(
                parse(args(&given)).unwrap().unwrap().mouth_diagnostics,
                enabled
            );
        }
        for extra in [
            &["--mouth-diagnostics", "yes"][..],
            &["--mouth-diagnostics"][..],
            &["--mouth-diagnostics", "on", "--mouth-diagnostics", "off"][..],
            &["--mouth-diagnostics", "on", "--help"][..],
        ] {
            assert!(parse(args(&[&FULL[..], extra].concat())).is_err());
        }
    }

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
        // The product's reader (#1671, the lead's 08:16): off unless given,
        // only with the mesh it reads from; beside FER+ for the measure.
        assert!(value.seems_blendshapes.is_none());
        let blendshapes = [
            "--seems-blendshapes",
            "/opt/lampOS/eyes/models/face_blendshapes.onnx",
        ];
        let mesh = ["--mouth-model", "/opt/lampOS/eyes/models/face_mesh_v2.onnx"];
        let meshed = [&FULL[..], &mesh, &blendshapes].concat();
        assert!(
            parse(args(&meshed))
                .unwrap()
                .unwrap()
                .seems_blendshapes
                .is_some()
        );
        assert_eq!(
            parse(args(&[&FULL[..], &blendshapes].concat())).err(),
            Some("--seems-blendshapes needs --mouth-model")
        );
        let both = parse(args(&[&reading[..], &mesh, &blendshapes].concat()))
            .unwrap()
            .unwrap();
        assert!(both.seems.is_some() && both.seems_blendshapes.is_some());
        let relative = [
            &FULL[..],
            &mesh,
            &["--seems-blendshapes", "face_blendshapes.onnx"],
        ];
        assert!(parse(args(&relative.concat())).is_err());
        assert!(parse(args(&[&meshed[..], &blendshapes].concat())).is_err());
        // #1671's measure log: off unless given, only beside a reader, a short
        // absolute path given once.
        assert!(value.seems_log.is_none());
        let log = ["--seems-log", "/var/lib/lampos-eyes/seems.log"];
        assert!(
            parse(args(&[&reading[..], &log].concat()))
                .unwrap()
                .unwrap()
                .seems_log
                .is_some()
        );
        assert!(
            parse(args(&[&meshed[..], &log].concat()))
                .unwrap()
                .unwrap()
                .seems_log
                .is_some()
        );
        assert_eq!(
            parse(args(&[&FULL[..], &log].concat())).err(),
            Some("--seems-log needs --seems or --seems-blendshapes")
        );
        assert!(
            parse(args(
                &[&reading[..], &["--seems-log", "seems.log"]].concat()
            ))
            .is_err()
        );
        assert!(parse(args(&[&reading[..], &log, &log].concat())).is_err());
        // Anywhere else, a reset wouldn't wipe it: refused.
        assert_eq!(
            parse(args(
                &[
                    &reading[..],
                    &["--seems-log", "/var/lib/lampos-eyes/other.log"]
                ]
                .concat()
            ))
            .err(),
            Some("--seems-log is /var/lib/lampos-eyes/seems.log, the one place a reset wipes")
        );
        assert_eq!(log[1], lamp_eyes::seems_log::PATH);
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
        // The camera's helper (#2892): off unless given, an absolute path, once.
        assert!(value.camera_helper.is_none());
        let helper = [
            &FULL[..],
            &["--camera-helper", "/run/lampos-eyes-camera/camera.sock"],
        ]
        .concat();
        assert_eq!(
            parse(args(&helper))
                .unwrap()
                .unwrap()
                .camera_helper
                .as_deref(),
            Some(Path::new("/run/lampos-eyes-camera/camera.sock"))
        );
        assert!(
            parse(args(
                &[&FULL[..], &["--camera-helper", "camera.sock"]].concat()
            ))
            .is_err()
        );
        let twice = [&helper[..], &["--camera-helper", "/run/x.sock"]].concat();
        assert!(parse(args(&twice)).is_err());
        // Every glance is detected unless looking on change is asked for.
        assert!(!value.look_on_change);
        let on = [&FULL[..], &["--look-on-change", "on"]].concat();
        assert!(parse(args(&on)).unwrap().unwrap().look_on_change);
        let off = [&FULL[..], &["--look-on-change", "off"]].concat();
        assert!(!parse(args(&off)).unwrap().unwrap().look_on_change);
        assert!(parse(args(&[&FULL[..], &["--look-on-change", "yes"]].concat())).is_err());
        let twice = [&on[..], &["--look-on-change", "on"]].concat();
        assert!(parse(args(&twice)).is_err());
        // Persist-2 only with looking on change, off unless given.
        assert!(!value.change_persist);
        let persist = [&on[..], &["--change-persist", "on"]].concat();
        assert!(parse(args(&persist)).unwrap().unwrap().change_persist);
        let persist_off = [&on[..], &["--change-persist", "off"]].concat();
        assert!(!parse(args(&persist_off)).unwrap().unwrap().change_persist);
        assert!(parse(args(&[&on[..], &["--change-persist", "yes"]].concat())).is_err());
        assert!(parse(args(&[&persist[..], &["--change-persist", "on"]].concat())).is_err());
        assert_eq!(
            parse(args(&[&FULL[..], &["--change-persist", "on"]].concat())).err(),
            Some("--change-persist needs --look-on-change on")
        );
        // The glance log only with looking on change, at the one path a
        // reset wipes.
        assert_eq!(value.glance_log, None);
        let logged = [
            &on[..],
            &["--glance-log", "/var/lib/lampos-eyes/glance.log"],
        ]
        .concat();
        assert_eq!(
            parse(args(&logged)).unwrap().unwrap().glance_log,
            Some(PathBuf::from("/var/lib/lampos-eyes/glance.log"))
        );
        assert_eq!(
            parse(args(
                &[
                    &FULL[..],
                    &["--glance-log", "/var/lib/lampos-eyes/glance.log"]
                ]
                .concat()
            ))
            .err(),
            Some("--glance-log needs --look-on-change on")
        );
        assert!(parse(args(&[&on[..], &["--glance-log", "glance.log"]].concat())).is_err());
        assert_eq!(
            parse(args(
                &[&on[..], &["--glance-log", "/var/lib/lampos-eyes/other.log"]].concat()
            ))
            .err(),
            Some("--glance-log is /var/lib/lampos-eyes/glance.log, the one place a reset wipes")
        );
        assert_eq!(logged[logged.len() - 1], lamp_eyes::glance_log::PATH);
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
        // Each but `--device`, which a named port may go without (#3504).
        for missing in 1..6 {
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
            "1da:5875@3-1",
            "0000:5875@3-1",
            "01da:5875@",
            "01da:5875@3-1;x",
            "zzzz:5875@3-1",
        ] {
            assert_eq!(usb_of(refused), None, "{refused}");
        }
    }

    /// #2190: `--usb VENDOR:PRODUCT` alone takes the one camera with those
    /// ids on any port, found at each open, so no `--device` goes with it;
    /// with a port, `--device` may be given, or left out to find the camera
    /// at that port only (#3504).
    #[test]
    fn the_camera_by_its_ids_alone_is_on_any_port_and_needs_no_device() {
        assert_eq!(
            usb_of("01da:5875"),
            Some(Usb {
                vendor: 0x01da,
                product: 0x5875,
                topology: String::new()
            })
        );
        let any = [&FULL[2..4], &["--usb", "01da:5875"], &FULL[6..]].concat();
        let value = parse(args(&any)).unwrap().unwrap();
        assert_eq!((value.device, value.usb.topology.as_str()), (None, ""));
        // A device beside ids alone is refused: the port it is on would be
        // chosen by the device and not checked.
        let both = [&FULL[..4], &["--usb", "01da:5875"], &FULL[6..]].concat();
        assert!(parse(args(&both)).is_err());
        // With a port and no device: found at that port (#3504).
        let at_port = parse(args(&FULL[2..])).unwrap().unwrap();
        assert_eq!(
            (at_port.device, at_port.usb.topology.as_str()),
            (None, "3-1.1.4")
        );
        assert_eq!(
            parse(args(&FULL)).unwrap().unwrap().device,
            Some(PathBuf::from("/dev/video0"))
        );
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
        // The working frame from the look's own picture: off unless given,
        // and only with a full look and a sight file.
        assert!(!with.sight_from_look);
        let from_look = [&full[..], &["--sight-from-look", "on"]].concat();
        assert!(parse(args(&from_look)).unwrap().unwrap().sight_from_look);
        let bad = [&full[..], &["--sight-from-look", "yes"]].concat();
        assert!(parse(args(&bad)).is_err());
        let alone = [
            &FULL[..],
            &[
                "--sight-file",
                "/run/lampos-eyes/sight.jpg",
                "--sight-from-look",
                "on",
            ],
        ]
        .concat();
        assert_eq!(
            parse(args(&alone)).err(),
            Some("--sight-from-look needs --full-look and --sight-file")
        );
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

    /// No file lampOS deploys (units, their drop-ins, env files and the
    /// scripts that write them) passes the test flag `--glance-log`.
    #[test]
    fn nothing_deployed_passes_glance_log() {
        let deploy = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy");
        let (mut folders, mut units) = (vec![deploy.clone()], 0);
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    folders.push(path);
                    continue;
                }
                units += usize::from(path.extension().is_some_and(|ext| ext == "service"));
                // Text only: a binary file says nothing a unit passes.
                if let Ok(text) = std::fs::read_to_string(&path) {
                    assert!(
                        !text.contains("--glance-log"),
                        "{} passes --glance-log",
                        path.display()
                    );
                }
            }
        }
        assert!(units > 0, "no unit found under deploy/");
        for unit in ["units/lamp-eyes.service", "product/lamp-eyes.service"] {
            assert!(deploy.join(unit).is_file(), "{unit} moved: look for it");
        }
    }
}
