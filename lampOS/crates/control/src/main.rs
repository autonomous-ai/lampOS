//! `lamp-control`: the lamp's local programming door (docs/control.md, #3504).
//!
//! ```text
//! lamp-control serve [--state-dir DIR] [--listen ADDR] [--admin-socket PATH] [--name NAME]
//!                    [--mood-socket PATH] [--say-socket PATH] [--actions-socket PATH]
//!                    [--bus-root DIR] [--boot-id ID]
//!                    [--sight-file PATH|off] [--frames-socket PATH|off] [--remote on|off]
//!                    [--camera-opens-on-demand]
//!                    [--microphone on|off]
//! lamp-control code --grants body,light,speak [--admin-socket PATH]
//! lamp-control grants CLIENT --grants body,light [--admin-socket PATH]
//! lamp-control revoke CLIENT [--admin-socket PATH]
//! lamp-control clients [--admin-socket PATH]
//! ```
//!
//! `serve` runs only with `LAMP_CONTROL=on` in its environment; otherwise it
//! says so and exits 0. It keeps its certificate and paired computers in
//! `--state-dir` (`/var/lib/lampos-control`), listens on `--listen`
//! (`0.0.0.0:7731`) and answers the owner's side on `--admin-socket`
//! (`/run/lampos-control/admin.sock`, 0600). Actions go to the body's
//! `--mood-socket`, the voice's `--say-socket`, the speaker's volume to
//! lamp-actions' `--actions-socket` (`/run/lampos-actions/call.sock`) and the
//! ring's light requests
//! on the bus at `--bus-root` (docs/control.md, "Routing"); a part not given
//! is refused with a why. The `camera` grant's pictures are the eyes' newest
//! frame in `--sight-file` (`/run/lampos-eyes/sight.jpg`, the eyes' own; `off`
//! for none), read only with `--bus-root`, which says when the lamp is
//! private (docs/control.md, "The camera"). The `microphone` grant's sound is
//! the ears' `mic1/audio` on that bus, which they publish only with
//! `LAMP_BUS_AUDIO=on`, and only with `--microphone on` (or
//! `LAMP_CONTROL_MICROPHONE=on`; off unless given) (docs/control.md, "The
//! microphone"). Connections come only from the
//! lamp's own network unless `--remote on` (or `LAMP_CONTROL_REMOTE=on`)
//! lets any address in, a VPN's 100.64/10 included; never to the camera or
//! the microphone, which stay on the lamp's own network (docs/control.md,
//! "The wire").

use std::path::PathBuf;

use lamp_control::{cert, pairing::Clients, parts, server, session};
use lamp_msgs::LightRequest;
use std::sync::{Arc, Mutex};

const ENVIRONMENT: &str = "LAMP_CONTROL";
const STATE_DIR: &str = "/var/lib/lampos-control";
const LISTEN: &str = "0.0.0.0:7731";
const ADMIN: &str = "/run/lampos-control/admin.sock";
/// Where the eyes keep their newest frame (`lamp-eyes --sight-file`).
const SIGHT: &str = "/run/lampos-eyes/sight.jpg";
/// Where the eyes serve every frame to one reader (`lamp-eyes
/// --frames-socket`): the camera's fast stream.
const FRAMES: &str = "/run/lampos-eyes/frames.sock";
/// `--remote on` from the environment, when the flag isn't given.
const REMOTE_ENVIRONMENT: &str = "LAMP_CONTROL_REMOTE";
/// The owner's switch for `microphone.stream`, off unless `on`.
const MICROPHONE_ENVIRONMENT: &str = "LAMP_CONTROL_MICROPHONE";

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&arguments) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("lamp-control: {error}");
            2
        }
    };
    std::process::exit(code);
}

fn option(arguments: &[String], name: &str) -> Option<String> {
    arguments
        .iter()
        .position(|a| a == name)
        .and_then(|at| arguments.get(at + 1))
        .cloned()
}

fn run(arguments: &[String]) -> Result<(), String> {
    let admin =
        PathBuf::from(option(arguments, "--admin-socket").unwrap_or_else(|| ADMIN.to_owned()));
    match arguments.first().map(String::as_str) {
        Some("serve") => serve(arguments, admin),
        Some("code") => {
            let grants: Vec<String> = option(arguments, "--grants")
                .unwrap_or_default()
                .split(',')
                .filter(|grant| !grant.is_empty())
                .map(str::to_owned)
                .collect();
            ask(&admin, &serde_json::json!({"code": {"grants": grants}}))
        }
        Some("grants") => {
            // A keeper's way to change a paired computer's grants at the lamp,
            // as the app's `control.set_grants` does: the whole list, no new code.
            let client = arguments.get(1).ok_or("grants CLIENT --grants a,b")?;
            // The whole list, so it must be given: `--grants ""` is none; a missing flag is a slip.
            let grants: Vec<String> = option(arguments, "--grants")
                .ok_or("grants CLIENT --grants a,b (the whole list; --grants \"\" for none)")?
                .split(',')
                .filter(|grant| !grant.is_empty())
                .map(str::to_owned)
                .collect();
            ask(
                &admin,
                &serde_json::json!({"set_grants": {"client": client, "grants": grants}}),
            )
        }
        Some("revoke") => {
            let client = arguments.get(1).ok_or("revoke CLIENT")?;
            ask(&admin, &serde_json::json!({"revoke": client}))
        }
        Some("clients") => ask(&admin, &serde_json::json!({"clients": true})),
        _ => Err("serve, code, grants, revoke or clients (see docs/control.md)".to_owned()),
    }
}

fn ask(admin: &PathBuf, asked: &serde_json::Value) -> Result<(), String> {
    use std::io::{BufRead as _, Write as _};
    let mut stream = std::os::unix::net::UnixStream::connect(admin)
        .map_err(|e| format!("{}: {e}", admin.display()))?;
    writeln!(stream, "{asked}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    std::io::BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    print!("{line}");
    Ok(())
}

fn serve(arguments: &[String], admin: PathBuf) -> Result<(), String> {
    if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
        eprintln!("lamp-control: off ({ENVIRONMENT} is not on): nothing listens");
        return Ok(());
    }
    let state =
        PathBuf::from(option(arguments, "--state-dir").unwrap_or_else(|| STATE_DIR.to_owned()));
    let listen = option(arguments, "--listen").unwrap_or_else(|| LISTEN.to_owned());
    let host = option(arguments, "--name")
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|h| h.trim().to_owned())
        })
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "lamp".to_owned());
    std::fs::create_dir_all(&state).map_err(|e| format!("{}: {e}", state.display()))?;
    let identity = cert::load_or_make(&state, &host).map_err(|e| format!("certificate: {e}"))?;
    let clients_path = state.join("clients.json");
    let clients =
        Clients::load(&clients_path).map_err(|e| format!("{}: {e}", clients_path.display()))?;
    eprintln!(
        "lamp-control: on, {listen}, {} paired, certificate pin {}",
        clients.clients.len(),
        identity.pin
    );
    // The quiet hours (#3917): the lamp's zone, `--zone`, `system` by
    // default. With a window set, one that can't be read keeps every
    // computer's say and sound refused as quiet, never a silent UTC; with
    // the quiet hours off (the default) the zone is never consulted.
    let zone = option(arguments, "--zone").unwrap_or_else(|| "system".to_owned());
    let local_time = match lamp_local_time::LocalTime::zone(&zone) {
        Ok(local_time) => Some(Arc::new(local_time)),
        Err(error) => {
            eprintln!(
                "lamp-control: {error}; with quiet hours set, say and sound are refused as quiet"
            );
            None
        }
    };
    // This lamp's own window (`--quiet-hours`, else LAMP_CONTROL_QUIET),
    // off unless set (the owner's "no limits for developers", 30 Oct). A
    // bad value stops it here, before anything is served.
    let window = session::quiet_window_of(
        option(arguments, "--quiet-hours").as_deref(),
        std::env::var("LAMP_CONTROL_QUIET").ok().as_deref(),
    )?;
    match window {
        Some(window) => eprintln!("lamp-control: quiet hours {window} local"),
        None => eprintln!("lamp-control: quiet hours off, this lamp's setting"),
    }
    // The rates are off unless `--rates default` (or
    // LAMP_CONTROL_RATES=default) turns them back on: every rate lifted but
    // the refused actions' guard and the phrase changes (the person's
    // decision, 30 Oct 02:1x: "remove the rate").
    let rates_off = session::rates_off_of(
        option(arguments, "--rates").as_deref(),
        std::env::var("LAMP_CONTROL_RATES").ok().as_deref(),
    )?;
    if rates_off {
        eprintln!(
            "lamp-control: rates off: every rate lifted but refused actions' and phrase changes"
        );
    } else {
        eprintln!("lamp-control: rates on, this lamp's setting");
    }
    let mut door = session::Door::new(
        &host,
        &host,
        identity.pin_bytes(),
        clients,
        Some(clients_path),
    )
    .with_quiet_hours(session::Quiet::new(local_time).with_window(window));
    if rates_off {
        door = door.with_rates_off();
    }
    // Skill push: on with the manage helper's socket, the packages kept in
    // the state folder for it.
    let manage = option(arguments, "--manage-socket").map(PathBuf::from);
    if manage.is_some() {
        door = door.with_push_folder(state.clone());
    }
    let body = option(arguments, "--mood-socket").map(PathBuf::from);
    let say = option(arguments, "--say-socket").map(PathBuf::from);
    // A computer's own clips: kept in the state folder's `sounds`, which
    // the voice plays from (its LAMP_SAY_CLIPS), with the voice only.
    let clips = state.join(lamp_control::sounds::SOUNDS_FOLDER);
    if say.is_some() {
        door = door.with_sound_folder(clips.clone());
    }
    // A computer's own notes: always kept, in the state folder's `notes`.
    door = door.with_notes_folder(state.join(lamp_control::notes::NOTES_FOLDER));
    let actions = option(arguments, "--actions-socket").map(PathBuf::from);
    let bus = match option(arguments, "--bus-root") {
        Some(root) => Some(lamp_bus::Bus::new(root).map_err(|e| format!("bus: {e}"))?),
        None => None,
    };
    let watch = Arc::new(Mutex::new(lamp_control::watch::Watch::new()));
    let light: Option<Box<dyn parts::LightOut>> = match &bus {
        Some(bus) => {
            let out = bus
                .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "control")
                .map_err(|e| format!("light/request: {e}"))?;
            Some(Box::new(out))
        }
        None => None,
    };
    let clock_id = option(arguments, "--boot-id")
        .or_else(|| {
            std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .ok()
                .map(|id| id.trim().to_owned())
        })
        .unwrap_or_default();
    let watching = watch.clone();
    // The camera only where the bus can say the lamp is private.
    let sight = match option(arguments, "--sight-file").as_deref() {
        Some("off") => None,
        Some(path) => Some(PathBuf::from(path)),
        None => Some(PathBuf::from(SIGHT)),
    }
    .filter(|_| bus.is_some());
    // The camera and the microphone are never seen or heard without the
    // ring's mark (`light/mark`): no mark on the bus, neither.
    let ring_mark = match &bus {
        Some(bus) => match bus.publisher_declared(
            lamp_msgs::RingMark::TOPIC,
            &lamp_msgs::RingMark::declaration(),
            "control",
        ) {
            Ok(publisher) => Some(Arc::new(Mutex::new(publisher))),
            Err(error) => {
                eprintln!(
                    "lamp-control: no camera or microphone, since the ring's mark can't be asked: {error}"
                );
                None
            }
        },
        None => None,
    };
    let marked = |kind: lamp_msgs::MarkKind| {
        ring_mark.clone().map(|publisher| {
            lamp_control::camera::mark_on(clock_id.clone(), kind, move |mark| {
                if let Ok(mut publisher) = publisher.lock()
                    && let Err(error) = publisher.publish_json(lamp_ipc::monotonic_us(), mark)
                {
                    eprintln!("lamp-control: the ring's mark was not asked: {error}");
                }
            })
        })
    };
    // The ring's word that it drew each computer's mark
    // (`light/mark_drawn`): no word, no picture and no sound (Thirteen's P2
    // on #4521).
    let drawn = match &bus {
        Some(bus) if ring_mark.is_some() => match drawn_on(bus, clock_id.clone()) {
            Ok(drawn) => Some(drawn),
            Err(error) => {
                eprintln!(
                    "lamp-control: no camera or microphone, since the ring's word on its mark can't be read: {error}"
                );
                None
            }
        },
        _ => None,
    };
    let sight = sight.zip(marked(lamp_msgs::MarkKind::Camera).zip(drawn.clone()));
    // The microphone: the ears' `mic1/audio`, subscribed only while a
    // stream is on; shared only where they publish it (`LAMP_BUS_AUDIO=on`).
    let hearing = bus
        .clone()
        .zip(marked(lamp_msgs::MarkKind::Microphone).zip(drawn))
        .map(|(bus, marking)| (lamp_control::microphone::on_bus(bus), marking));
    // The fast stream from the eyes' frames socket, when they serve one; its
    // frames go out once the server below has a way to send them.
    let frames = match option(arguments, "--frames-socket").as_deref() {
        Some("off") => None,
        Some(path) => Some(PathBuf::from(path)),
        None => Some(PathBuf::from(FRAMES)),
    };
    let sink: lamp_control::camera::Sink = Arc::default();
    let sink_for_lamp = sink.clone();
    // The body's own lease, with `--lease-socket` only (lamp-body
    // --lease-socket): a computer's `look` on it, and the follower set by a
    // `look` at `hand`, told to the body with each hold.
    let lease_socket = option(arguments, "--lease-socket").map(PathBuf::from);
    let (looks, looked) = tokio::sync::mpsc::unbounded_channel();
    let follow = lamp_control::body_lease::Follower::default();
    let on_lease = lease_socket.is_some().then(|| (looks, follow.clone()));
    let sink_for_ears = sink.clone();
    // The microphone stream is off unless the owner switches it on: no unit
    // sets it (docs/control.md, "The microphone").
    let microphone = match option(arguments, "--microphone")
        .or_else(|| std::env::var(MICROPHONE_ENVIRONMENT).ok())
        .as_deref()
    {
        None | Some("off") => false,
        Some("on") => true,
        Some(_) => return Err("--microphone is on or off".to_owned()),
    };
    eprintln!(
        "lamp-control: {}",
        if microphone {
            "the microphone stream is on (--microphone on), where the ears share their sound"
        } else {
            "the microphone stream is off (--microphone)"
        }
    );
    // The body's word on each motion (`body/gesture`), for a gesture's,
    // move's and feeling's move's `done` (#4209); without it they end at
    // their answer, as hello's limits then say.
    let gestures = match (&bus, &body) {
        (Some(bus), Some(_)) => match bus.subscribe(lamp_msgs::BodyGesture::TOPIC, "control") {
            Ok(subscriber) => Some(subscriber),
            Err(error) => {
                eprintln!(
                    "lamp-control: no motion done, since body/gesture can't be read: {error}"
                );
                None
            }
        },
        _ => None,
    };
    let shared = server::Shared::new_with(door, identity.pin.clone(), move |events| {
        let lamp = parts::Lamp::new(body, light, say, clock_id, events)
            .watching(watching)
            .with_clips(clips);
        let mut lamp = match hearing {
            Some((listen, marking)) => lamp.hearing(listen, marking, sink_for_ears, microphone),
            None => lamp,
        };
        if let Some(gestures) = gestures {
            lamp = lamp.telling_motions(gestures);
        }
        if let Some(actions) = actions {
            lamp = lamp.volume_through(actions);
        }
        let on_demand = arguments.iter().any(|a| a == "--camera-opens-on-demand");
        let lamp = match sight {
            Some((sight, mark)) => match frames {
                Some(frames) => lamp
                    .seeing(sight, mark)
                    .camera_on_demand(on_demand)
                    .fast_frames(frames, sink_for_lamp),
                None => lamp.seeing(sight, mark).camera_on_demand(on_demand),
            },
            None => lamp,
        };
        Box::new(match on_lease {
            Some((looks, follow)) => lamp.looking(looks).following(follow),
            None => lamp,
        })
    });
    if let Some(manage) = manage {
        shared.set_manage_socket(manage);
    }
    let _ = sink.set(shared.binary());
    let remote = match option(arguments, "--remote")
        .or_else(|| std::env::var(REMOTE_ENVIRONMENT).ok())
        .as_deref()
    {
        None | Some("off") => false,
        Some("on") => true,
        Some(_) => return Err("--remote is on or off".to_owned()),
    };
    shared.set_remote(remote);
    eprintln!(
        "lamp-control: {}",
        if remote {
            "computers beyond the lamp's own network may connect (--remote on); never to the camera or the microphone"
        } else {
            "only computers on the lamp's own network may connect"
        }
    );
    let tls = server::acceptor(&identity).map_err(|e| format!("tls: {e}"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&listen)
            .await
            .map_err(|e| format!("{listen}: {e}"))?;
        let _ = std::fs::remove_file(&admin);
        // The owner's side: its folder 0700 before the socket is bound, so
        // nobody else can reach it even before its own 0600 (the
        // Reviewer-lead's P3 on #3778).
        if let Some(parent) = admin.parent() {
            use std::os::unix::fs::DirBuilderExt as _;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(|e| format!("{}: {e}", parent.display()))?;
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let owner = tokio::net::UnixListener::bind(&admin)
            .map_err(|e| format!("{}: {e}", admin.display()))?;
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&admin, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        tokio::spawn(server::admin(owner, shared.clone()));
        if let Some(bus) = &bus {
            server::watch_bus(bus, watch, shared.clone())
                .map_err(|e| format!("bus topics: {e}"))?;
        }
        // The body's own lease, so the lamp's own life stands back while a
        // computer holds `body` (lamp-body --lease-socket).
        if let Some(path) = lease_socket {
            tokio::spawn(lamp_control::body_lease::hold(
                path,
                shared.clone(),
                follow,
                shared.events(),
                looked,
            ));
        }
        tokio::select! {
            () = server::serve(listener, tls, shared) => {}
            _ = tokio::signal::ctrl_c() => {}
        }
        Ok::<(), String>(())
    })
}

/// When the ring last drew each computer's mark, as it says on
/// `light/mark_drawn` (`lamp_msgs::RingMarkDrawn`, its asker's `control:`
/// source), read on a thread of its own: the camera's word that a picture
/// may go (`lamp_control::camera::Drawn`). Another boot's is none.
fn drawn_on(bus: &lamp_bus::Bus, clock_id: String) -> std::io::Result<lamp_control::camera::Drawn> {
    use lamp_msgs::RingMarkDrawn;
    let mut said = bus.subscribe_typed(RingMarkDrawn::TOPIC, "control", RingMarkDrawn::TYPE)?;
    let seen: Arc<Mutex<std::collections::HashMap<String, std::time::Instant>>> = Arc::default();
    let into = seen.clone();
    std::thread::spawn(move || {
        loop {
            match said.recv(std::time::Duration::from_millis(200)) {
                Ok(Some(message)) => {
                    // The ring's word only (Thirteen's P3 on #4521).
                    if message.header.source == RingMarkDrawn::SOURCE
                        && let Ok(drawn) = message.parse::<RingMarkDrawn>()
                        && drawn.clock_id == clock_id
                        && let Ok(mut seen) = into.lock()
                    {
                        seen.insert(drawn.source, std::time::Instant::now());
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("lamp-control: the ring's word on its mark is lost: {error}");
                    return;
                }
            }
        }
    });
    Ok(Arc::new(move |client: &str| {
        seen.lock()
            .ok()
            .and_then(|seen| seen.get(&format!("control:{client}")).copied())
    }))
}
