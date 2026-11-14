//! `lamp-seen serve`: the "no frames" gate on the bus and, given a frame
//! socket, the camera's frame client behind the ring's mark (#6543). Given
//! the account too, activity's bursts go to the perception service (phase
//! 2); else each copy is dropped. The sitting's timers go out on
//! `humans/doing`, and each new sighting on `humans/doing_seen`. The opt-in
//! is said back once when it turns on and once when it turns off, and a
//! forget turns it off; the kept yes is only read, never written. With
//! `LAMP_SEEN_POSTURE=on` and the account too, one frame every 30 s goes to
//! the pose model and `humans/posture` is said while someone sits (phase 3).
//! Off unless `LAMP_SEEN_ACTIVITY=on`: off, it idles until stopped and
//! touches nothing (no bus, no file, no socket).
//!
//! `lamp-seen frame-link`, run by the product unit before `serve`, keeps
//! the frame link's two boots for the eyes and this part
//! ([`lamp_seen::link`]): made once with the switch on, removed with it off.
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_camera::frames::{FrameClient, Role};
use lamp_msgs::{
    Diagnostic, DoingSeen, ExpressionState, Faces, HumanDoing, HumanFamiliar, HumanPosture, Level,
    MemoryForgot, MicrophonePrivacy, RoomActivity, RoomPresence, StoodUp,
};
use lamp_seen::{
    BUS_NAME, ENVIRONMENT, HEALTH_NAME,
    activity::{self, Activity},
    burst::{Outcome, Pace},
    endpoint::Endpoint,
    frames::{self, Sink, Taker},
    gate, link,
    mark::{BusRing, Marked},
    opt_in,
    opt_in::Reader as _,
    outlet::Outlet,
    pause::{self, Pause},
    pose::{self, Pose},
    said_back::{self, SaidBack, SayService},
    sitting::Sitting,
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

const USAGE: &str = "usage: lamp-seen serve [--bus-root ABSOLUTE_PATH] [--opt-in-folder ABSOLUTE_PATH] [--state-folder ABSOLUTE_PATH] [--say-socket ABSOLUTE_PATH] [--frames-socket ABSOLUTE_PATH --own-boot HEX32 --controller-boot HEX32 | --frame-link-file ABSOLUTE_PATH] [--account ABSOLUTE_PATH --key ABSOLUTE_PATH [--public-key-file ABSOLUTE_PATH]] [--run-for-ms N]\n       lamp-seen frame-link [--file ABSOLUTE_PATH] [--socket ABSOLUTE_PATH]\n\nOff unless LAMP_SEEN_ACTIVITY=on: off, it idles and touches nothing. On, it reads the bus\n(voice/privacy, room/presence, humans/faces/tracked, humans/familiar, expression/state,\nmemory/forgot, room/activity, room/stood_up) and the owner's opt-in (seen-activity.json in\n--opt-in-folder, default /var/lib/lampOS/consent, read again every 2 s and never written),\nsays on diagnostics whether a frame may be asked for, and says the sitting's timers on humans/doing every 30 s\nwhile one remembered face sits at the desk. With --frames-socket it is the camera frame\nlink's perception consumer (or, with --frame-link-file, the link and boots that file keeps):\na frame asked for only while the gate is open and once the ring\nsaid it drew its mark (light/mark, light/mark_drawn). With --account (setup's\nbackend.json) and --key (the account key) too, one burst of 8 frames a minute goes to the\nperception service's activity socket, encrypted, on a socket closed after each burst, and\nwhat was seen goes on humans/doing and humans/doing_seen (#6543 phase 2); without them each\ncopy is dropped at once. No frame is asked for before the on line for the kept yes has\nbegun. The opt-in is said back once each way, on and off (a forget included; an on line not\nbegun is withdrawn), through the voice's say service\n(--say-socket, LAMP_SAY_SOCKET or /run/lampos-say/say.sock), and what was said back is\nkept in --state-folder (default /var/lib/lampos-seen). memory/forgot turns the opt-in off\nuntil the voice removes the kept yes or a later yes is given. With LAMP_SEEN_POSTURE=on and the account too, one frame every 30 s goes, encrypted,\nto the pose model on a socket closed after it, and humans/posture is said every 30 s while\nsomeone sits (#6543 phase 3).\n\nframe-link: with LAMP_SEEN_ACTIVITY=on, keeps in --file (/run/lampos-seen/frame-link.env)\ntwo new boots from /dev/urandom, made once, as LAMP_EYES_FRAME_LINK=--frame-link SOCKET\n--frame-link-controller HEX32 --frame-link-reader HEX32 and LAMP_EYES_VISITS=--visits on\n(the stranger check's humans/familiar) for lamp-eyes.service, the socket\n--socket (/run/lampos-eyes-frames/camera.frames); off, removes the file.";

/// One step of the loop.
const TICK: Duration = Duration::from_millis(100);
/// Messages read from one topic in one step at most.
const DRAIN: usize = 64;
/// How often `diagnostics` is said.
const HEALTH_EVERY: Duration = Duration::from_secs(1);

struct Config {
    bus_root: PathBuf,
    opt_in_folder: PathBuf,
    state_folder: PathBuf,
    say_socket: PathBuf,
    frames: Option<(PathBuf, BootId, BootId)>,
    /// The frame link's file ([`link::FILE`]), read at start for the
    /// socket and the boots instead of `frames`.
    frame_link_file: Option<PathBuf>,
    /// Setup's `backend.json`, the account key, and the server's public
    /// key if given ahead.
    account: Option<(PathBuf, PathBuf, Option<PathBuf>)>,
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
    link::parse_boot(&value).ok_or(format!("{flag} is not a boot"))
}

/// `lamp-seen frame-link`'s file and socket.
#[derive(Debug, PartialEq, Eq)]
struct FrameLink {
    file: PathBuf,
    socket: PathBuf,
}

fn parse_frame_link(mut args: impl Iterator<Item = String>) -> Result<FrameLink, String> {
    let mut file = PathBuf::from(link::FILE);
    let mut socket = PathBuf::from(link::SOCKET);
    let mut seen = std::collections::BTreeSet::new();
    while let Some(flag) = args.next() {
        if !seen.insert(flag.clone()) {
            return Err(format!("{flag} given twice"));
        }
        match flag.as_str() {
            "--file" => file = absolute(args.next(), &flag)?,
            "--socket" => socket = absolute(args.next(), &flag)?,
            _ => return Err(USAGE.into()),
        }
    }
    Ok(FrameLink { file, socket })
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Config, String> {
    if args.next().as_deref() != Some("serve") {
        return Err(USAGE.into());
    }
    let mut bus_root = PathBuf::from(lamp_bus::DEFAULT_ROOT);
    let mut opt_in_folder = PathBuf::from(lamp_msgs::consent::FOLDER);
    let mut state_folder = PathBuf::from(opt_in::STATE_FOLDER);
    let mut frame_link_file = None;
    let mut say_socket = std::env::var_os("LAMP_SAY_SOCKET")
        .map_or_else(|| PathBuf::from(SayService::DEFAULT_SOCKET), PathBuf::from);
    let (mut socket, mut own, mut controller, mut run_for) = (None, None, None, None);
    let (mut account, mut key, mut public_key) = (None, None, None);
    let mut seen = std::collections::BTreeSet::new();
    while let Some(flag) = args.next() {
        if !seen.insert(flag.clone()) {
            return Err(format!("{flag} given twice"));
        }
        match flag.as_str() {
            "--bus-root" => bus_root = absolute(args.next(), &flag)?,
            "--opt-in-folder" => opt_in_folder = absolute(args.next(), &flag)?,
            "--state-folder" => state_folder = absolute(args.next(), &flag)?,
            "--frame-link-file" => frame_link_file = Some(absolute(args.next(), &flag)?),
            "--say-socket" => say_socket = absolute(args.next(), &flag)?,
            "--frames-socket" => socket = Some(absolute(args.next(), &flag)?),
            "--own-boot" => own = Some(boot(args.next(), &flag)?),
            "--controller-boot" => controller = Some(boot(args.next(), &flag)?),
            "--account" => account = Some(absolute(args.next(), &flag)?),
            "--key" => key = Some(absolute(args.next(), &flag)?),
            "--public-key-file" => public_key = Some(absolute(args.next(), &flag)?),
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
    if frames.is_some() && frame_link_file.is_some() {
        return Err("--frames-socket and --frame-link-file are two ways to one link".into());
    }
    let account = match (account, key, public_key) {
        (None, None, None) => None,
        (Some(_), Some(_), _) if frames.is_none() && frame_link_file.is_none() => {
            return Err(
                "--account needs --frames-socket or --frame-link-file: nothing to send".into(),
            );
        }
        (Some(account), Some(key), public_key) => Some((account, key, public_key)),
        _ => return Err("--account and --key go together".into()),
    };
    Ok(Config {
        bus_root,
        opt_in_folder,
        state_folder,
        say_socket,
        frames,
        frame_link_file,
        account,
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
    activity: Subscriber,
    stood_up: Subscriber,
    /// `room/activity`'s sitting length and when it was read.
    sitting_s: Option<(u32, u64)>,
}

/// What came in one step besides the gate's readings.
#[derive(Default)]
struct Came {
    forgot: bool,
    stood_up: bool,
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
            activity: typed(RoomActivity::TOPIC, RoomActivity::TYPE)?,
            stood_up: typed(StoodUp::TOPIC, StoodUp::TYPE)?,
            sitting_s: None,
        })
    }

    /// When this sitting began, as `room/activity` counts it (a gap under
    /// two minutes forgiven), if it said so lately; else `now_us`.
    fn arrived_us(&self, now_us: u64) -> u64 {
        let fresh = RoomActivity::LASTS.as_micros() as u64;
        match self.sitting_s {
            Some((sitting_s, at)) if now_us.saturating_sub(at) <= fresh => {
                now_us.saturating_sub(u64::from(sitting_s).saturating_mul(1_000_000))
            }
            _ => now_us,
        }
    }

    /// Read what came into `readings`.
    fn read(&mut self, readings: &mut Readings) -> Came {
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
        if let Some(activity) = newest::<RoomActivity>(&mut self.activity) {
            self.sitting_s = Some((activity.sitting_s, now));
        }
        Came {
            forgot: forgot(&mut self.forgot),
            stood_up: newest::<StoodUp>(&mut self.stood_up).is_some(),
        }
    }
}

/// The boot's id, the clock the ring's marks are on; empty, and so no mark
/// and no frame, where it can't be read.
fn boot_clock_id() -> String {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|id| id.trim().to_owned())
        .unwrap_or_default()
}

/// "on" or "off", for the journal.
fn said_word(said: opt_in::Said) -> &'static str {
    match said {
        opt_in::Said::On { .. } => "on",
        opt_in::Said::Off => "off",
    }
}

type Frames = (Taker<FrameClient>, Marked<BusRing, Outlet>);

fn health(
    publisher: &mut Publisher,
    gate: Result<(), gate::Closed>,
    pause: pause::Counts,
    frames: Option<&Frames>,
) {
    let activity = frames.and_then(|(_, marked)| marked.inner().activity.as_deref());
    let posture = frames.and_then(|(_, marked)| marked.inner().pose.as_deref());
    let warn = activity.is_some_and(|activity| activity.retreat().warn())
        || posture.is_some_and(|pose| pose.warn());
    let mut diagnostic = Diagnostic::new(
        if warn { Level::Warn } else { Level::Ok },
        HEALTH_NAME,
        if warn {
            "the perception service is busy or unreachable: its frames are held back"
        } else {
            ""
        },
    )
    .with(
        "gate",
        gate.map_or_else(|closed| closed.as_str(), |()| "open"),
    )
    .with("paused", pause.paused)
    .with("misread_on_track", pause.misread_on_track);
    diagnostic = match activity {
        None => diagnostic.with("sends", "none"),
        Some(activity) => {
            let counts = activity.counts();
            // Posture runs only beside activity: `activity+pose` with it.
            let sends = if posture.is_some() {
                "activity+pose"
            } else {
                "activity"
            };
            diagnostic
                .with("sends", sends)
                .with("bursts", counts.bursts)
                .with("answered", counts.answered)
                .with("busy", counts.busy)
                .with("failed", counts.failed)
                .with("aborted", counts.aborted)
                .with("frames_sent", counts.frames)
                .with("frames_wiped", activity.wiped())
                .with("wrong_frame", counts.wrong_frame)
                .with(
                    "last_failure",
                    activity
                        .last_failure()
                        .map_or("none", |failure| failure.as_str()),
                )
        }
    };
    if let Some(pose) = posture {
        let counts = pose.counts();
        let now = monotonic_us();
        diagnostic = diagnostic
            .with("pose", pose.state(now))
            .with("pose_sent", counts.sent)
            .with("pose_answered", counts.answered)
            .with("pose_no_body", counts.no_body)
            .with("pose_busy", counts.busy)
            .with("pose_failed", counts.failed)
            .with("pose_cancelled", counts.cancelled)
            .with("pose_samples", pose.samples())
            .with("pose_wrong_frame", counts.wrong_frame)
            .with(
                "pose_last_failure",
                pose.last_failure()
                    .map_or("none", |failure| failure.as_str()),
            );
    }
    diagnostic = match frames {
        None => diagnostic.with("frames", "no frame socket"),
        Some((taker, marked)) => {
            let counts = taker.counts();
            let marks = marked.counts();
            diagnostic
                .with("requested", counts.requested)
                .with("handed", counts.handed)
                .with("grant_changed", counts.grant_changed)
                .with("lost", counts.lost)
                .with("marks_asked", marks.asked)
                .with("marks_not_drawn", marks.not_drawn)
                .with("unmarked_dropped", marks.refused)
                .with("blinks_missed", marks.missed)
        }
    };
    let _ = publisher.publish_json(monotonic_us(), &diagnostic);
}

fn serve(config: Config, stop: &AtomicBool) -> io::Result<()> {
    let bus = Bus::new(&config.bus_root)?;
    let mut inputs = Inputs::subscribe(&bus)?;
    let mut diagnostics =
        bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), BUS_NAME)?;
    let mut doing_out =
        bus.publisher_declared(HumanDoing::TOPIC, &HumanDoing::declaration(), BUS_NAME)?;
    let mut seen_out =
        bus.publisher_declared(DoingSeen::TOPIC, &DoingSeen::declaration(), BUS_NAME)?;
    let opt_in_file = opt_in::File::in_folder(&config.opt_in_folder);
    let record = opt_in::Record::in_folder(&config.state_folder);
    let mut kept = lamp_msgs::consent::Kept::default();
    let mut read_at: Option<Instant> = None;
    let mut latch = opt_in::OptIn::default();
    let mut readings = Readings::default();
    let mut mouth = SayService::new(config.say_socket.clone());
    // Activity's bursts, given the account, and posture's samples with its
    // switch too; else the copies are dropped.
    let mut outlet = Outlet::default();
    if let Some((account, key, public_key)) = config.account.as_ref() {
        let endpoint = Endpoint::from_files(account, key).and_then(|endpoint| match public_key {
            Some(pem) => endpoint.with_public_key_file(pem),
            None => Ok(endpoint),
        });
        match endpoint {
            Ok(endpoint) => {
                eprintln!("lamp-seen: activity's bursts go to {}", endpoint.socket_url);
                if switched_on(std::env::var(pose::ENVIRONMENT).ok().as_deref()) {
                    match pose::client::Client::for_endpoint(&endpoint) {
                        Ok(client) => {
                            eprintln!(
                                "lamp-seen: posture's frames go to {}",
                                client.address().pose_url()
                            );
                            outlet.pose = Some(Box::new(Pose::new(client)));
                        }
                        Err(error) => eprintln!("lamp-seen: no posture sent: {error}"),
                    }
                }
                outlet.activity = Some(Box::new(Activity::new(endpoint, Pace::default())));
            }
            Err(error) => eprintln!("lamp-seen: no activity or posture sent: {error}"),
        }
    }
    // With posture sending, the on line owed names it, and posture sends
    // nothing until that line has begun for the kept yes.
    let mut said_back = SaidBack::with_posture(outlet.pose.is_some());
    let mut posture_out = match outlet.pose {
        Some(_) => Some(bus.publisher_declared(
            HumanPosture::TOPIC,
            &HumanPosture::declaration(),
            BUS_NAME,
        )?),
        None => None,
    };
    let every_us = if outlet.activity.is_some() {
        activity::SPACING_US
    } else {
        frames::EVERY_US
    };
    // The product's link: the socket and boots `lamp-seen frame-link` kept
    // for the eyes and this part.
    let from_file = match config.frame_link_file.as_deref().map(link::read) {
        None => None,
        Some(Ok(Some(link))) => {
            eprintln!("lamp-seen: the frame link is {}", link.socket.display());
            Some((link.socket, link.reader, link.controller))
        }
        Some(Ok(None)) => {
            eprintln!("lamp-seen: no frame link kept: no frame is asked for");
            None
        }
        Some(Err(error)) => {
            eprintln!(
                "lamp-seen: the frame link could not be read ({error}): no frame is asked for"
            );
            None
        }
    };
    // No frame without the ring's mark: no mark asked for, no frames.
    let mut frames: Option<Frames> = match config.frames.clone().or(from_file).as_ref() {
        None => None,
        Some((socket, own, controller)) => Some((
            Taker::with_every(
                FrameClient::new(
                    socket,
                    Role::Perception,
                    *own,
                    *controller,
                    lamp_camera::MAX_FRAME_BYTES,
                ),
                every_us,
            ),
            Marked::new(BusRing::on(&bus, boot_clock_id())?, outlet),
        )),
    };
    let mut sitting: Option<Sitting> = None;
    let mut pause = Pause::default();
    let mut doing_said: Option<Instant> = None;
    let started = Instant::now();
    let mut said_at: Option<Instant> = None;
    let mut last_gate = None;
    while !stop.load(Ordering::Relaxed) && config.run_for.is_none_or(|run| started.elapsed() < run)
    {
        let came = inputs.read(&mut readings);
        if came.forgot {
            latch.forgot(unix_s());
            eprintln!(
                "lamp-seen: a forget came: the opt-in is off until its kept yes is removed or given again"
            );
        }
        // The kept yes, read again every 2 s; never written here.
        if read_at.is_none_or(|at| at.elapsed() >= opt_in::READ_EVERY) {
            kept = opt_in_file.read();
            if opt_in_file.gone() {
                latch.gone();
            }
            read_at = Some(Instant::now());
        }
        let opted_in = latch.on(&kept);
        match said_back.step(opted_in, &kept, &record, &mut mouth, monotonic_us()) {
            said_back::Step::Asked(said) => {
                eprintln!("lamp-seen: saying the opt-in back: {}", said_word(said))
            }
            said_back::Step::Said(said, Ok(())) => {
                eprintln!("lamp-seen: the opt-in was said back: {}", said_word(said))
            }
            said_back::Step::Said(said, Err(error)) => eprintln!(
                "lamp-seen: the opt-in was said back ({}), but that could not be kept: {error}",
                said_word(said)
            ),
            said_back::Step::NotSaid(said, why) => eprintln!(
                "lamp-seen: the opt-in was not said back ({}: {why}); asked again later",
                said_word(said)
            ),
            said_back::Step::Withdrawn(said) => eprintln!(
                "lamp-seen: the opt-in's {} line, not begun, is no longer owed: withdrawn",
                said_word(said)
            ),
            said_back::Step::Idle | said_back::Step::Waiting => {}
        }
        let now = monotonic_us();
        // No frame before the lamp has begun saying it will look.
        let said_on = said_back.said_on(&kept, &record);
        let now_inputs = readings.inputs(opted_in, said_on, now);
        let gate = gate::may_request(&now_inputs);
        // A stranger reading on the same tracked face pauses: the gate stays
        // closed, and the window and timers are kept unsaid, at most 5 s.
        let was_paused = pause.paused();
        let before = pause.counts();
        let verdict = pause.step(now, &now_inputs, &readings.on_track(now));
        let verdict = pause::after_stood_up(&mut pause, verdict, came.stood_up);
        if pause.counts().paused != before.paused {
            let counts = pause.counts();
            eprintln!(
                "lamp-seen: gate paused on a misread (paused {}, misread_on_track {})",
                counts.paused, counts.misread_on_track
            );
        } else if was_paused && !verdict.paused {
            eprintln!(
                "lamp-seen: the pause ended: {}",
                if verdict.wipe { "wiped" } else { "kept" }
            );
        }
        if verdict.wipe {
            if sitting.take().is_some() {
                doing_said = None;
            }
            if let Some((_, marked)) = frames.as_mut()
                && let Some(pose) = marked.inner_mut().pose.as_mut()
            {
                pose.forget();
            }
        }
        if last_gate != Some(gate) {
            eprintln!(
                "lamp-seen: gate {}",
                gate.map_or_else(|closed| closed.as_str(), |()| "open")
            );
            last_gate = Some(gate);
        }
        // The sitting: begun when one remembered face is at the desk
        // (whatever the opt-in, its said-back line, the privacy or sleep
        // say), ended on room/stood_up and when nobody or a stranger is
        // there. Frames still go only under `gate`, said-back included.
        let desk = gate::may_request(&gate::Inputs {
            opted_in: true,
            said_on: true,
            privacy_open: Some(true),
            asleep: Some(false),
            ..now_inputs
        });
        if pause::ends_sitting(verdict, came.stood_up, desk) && sitting.take().is_some() {
            doing_said = None;
        }
        if desk.is_ok() && sitting.is_none() {
            sitting = Some(Sitting::new(inputs.arrived_us(now)));
        }
        if let Some((taker, marked)) = frames.as_mut() {
            // Posture's own gate: closed `not_said_yet` until a line naming
            // posture was said for this yes, whatever activity's says.
            let posture_gate = pose::gate(gate, said_back.said_posture(&kept, &record));
            if verdict.paused
                && let Some(pose) = marked.inner_mut().pose.as_mut()
            {
                pose.hold(now);
            } else if let (Some(pose), Some(posture_out)) =
                (marked.inner_mut().pose.as_mut(), posture_out.as_mut())
                && let Some(posture) = pose.step(now, posture_gate, came.stood_up)
                && let Err(error) = posture_out.publish_json(now, &posture)
            {
                eprintln!("lamp-seen: humans/posture could not be said: {error}");
            }
            if let Some(activity) = marked.inner_mut().activity.as_mut()
                && let Some(finished) = activity.poll(now)
            {
                match finished.outcome {
                    Outcome::Answered => {}
                    Outcome::Busy => eprintln!("lamp-seen: the server is busy: the burst dropped"),
                    Outcome::Failed(failure) => {
                        eprintln!("lamp-seen: a burst failed ({})", failure.as_str())
                    }
                }
                // Paused: the burst under way is aborted below, and what it
                // saw is dropped, never said.
                if let Some(sitting) = sitting.as_mut().filter(|_| !verdict.paused) {
                    for newly in sitting.saw(&finished.seen, now) {
                        if newly.check().is_ok() {
                            let _ = seen_out.publish_json(now, &newly);
                        }
                    }
                    if !finished.seen.is_empty() {
                        // Said at once, not at the next 30 s.
                        doing_said = None;
                    }
                }
            }
            taker.step(gate, now, marked);
        }
        if let Some(sitting) = sitting.as_ref().filter(|_| !verdict.paused)
            && doing_said.is_none_or(|at| at.elapsed() >= HumanDoing::EVERY)
        {
            let doing = sitting.doing(now);
            if doing.check().is_ok() {
                let _ = doing_out.publish_json(now, &doing);
            }
            doing_said = Some(Instant::now());
        }
        if said_at.is_none_or(|at| at.elapsed() >= HEALTH_EVERY) {
            health(&mut diagnostics, gate, pause.counts(), frames.as_ref());
            said_at = Some(Instant::now());
        }
        std::thread::sleep(TICK);
    }
    if let Some((taker, marked)) = frames.as_mut() {
        taker.drop_everything();
        marked.stopped();
    }
    let _ = diagnostics.publish_json(monotonic_us(), &Diagnostic::stopping(HEALTH_NAME));
    Ok(())
}

/// `lamp-seen frame-link`: the link kept with the switch on, removed off.
fn frame_link(args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let FrameLink { file, socket } = parse_frame_link(args)?;
    let on = switched_on(std::env::var(ENVIRONMENT).ok().as_deref());
    match link::prepare(&file, &socket, on, link::new_boot)? {
        link::Prepared::Kept(_) => {
            eprintln!("lamp-seen: the frame link kept in {}", file.display())
        }
        link::Prepared::Made(_) => {
            eprintln!("lamp-seen: a new frame link kept in {}", file.display())
        }
        link::Prepared::Off => {}
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().map(String::as_str) == Some("frame-link") {
        args.next();
        return frame_link(args);
    }
    let config = parse(args)?;
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
        assert_eq!(config.state_folder, PathBuf::from("/var/lib/lampos-seen"));
        assert!(config.frames.is_none());
        let config = parse(args("serve --state-folder /tmp/seen-state")).unwrap();
        assert_eq!(config.state_folder, PathBuf::from("/tmp/seen-state"));
        assert!(config.frame_link_file.is_none());
        let config = parse(args("serve --say-socket /run/x/say.sock")).unwrap();
        assert_eq!(config.say_socket, PathBuf::from("/run/x/say.sock"));
        let hex = "0123456789abcdef0123456789abcdef";
        let config = parse(args(&format!(
            "serve --frames-socket /run/x/camera.frames --own-boot {hex} --controller-boot {hex} --run-for-ms 5"
        )))
        .unwrap();
        assert!(config.frames.is_some());
        assert!(config.account.is_none());
        assert_eq!(config.run_for, Some(Duration::from_millis(5)));
        let config = parse(args(&format!(
            "serve --frames-socket /run/x/camera.frames --own-boot {hex} --controller-boot {hex} --account /run/c/account --key /run/c/key"
        )))
        .unwrap();
        assert_eq!(
            config.account,
            Some((
                PathBuf::from("/run/c/account"),
                PathBuf::from("/run/c/key"),
                None
            ))
        );
        // The product's line: the link from its file, the account's two
        // files by path, never their contents.
        let config = parse(args(
            "serve --bus-root /run/lampos-bus --say-socket /run/lampos-say/say.sock --opt-in-folder /var/lib/lampOS/consent --state-folder /var/lib/lampos-seen --frame-link-file /run/lampos-seen/frame-link.env --account /run/credentials/lamp-seen.service/account --key /run/credentials/lamp-seen.service/key",
        ))
        .unwrap();
        assert_eq!(
            config.frame_link_file,
            Some(PathBuf::from("/run/lampos-seen/frame-link.env"))
        );
        assert_eq!(config.state_folder, PathBuf::from("/var/lib/lampos-seen"));
        assert!(config.frames.is_none());
        assert!(config.account.is_some());
        for bad in [
            "serve --account /a --key /k".to_owned(),
            format!(
                "serve --frames-socket /x --own-boot {hex} --controller-boot {hex} --frame-link-file /f"
            ),
            "serve --frame-link-file relative".to_owned(),
            format!(
                "serve --frames-socket /x --own-boot {hex} --controller-boot {hex} --account /a"
            ),
            format!(
                "serve --frames-socket /x --own-boot {hex} --controller-boot {hex} --account /a --key k"
            ),
            format!(
                "serve --frames-socket /x --own-boot {hex} --controller-boot {hex} --public-key-file /p"
            ),
        ] {
            assert!(parse(args(&bad)).is_err(), "{bad}");
        }
        for bad in [
            "",
            "run",
            "serve --frames-socket /run/x/camera.frames",
            "serve --bus-root relative",
            "serve --say-socket relative",
            "serve --state-folder relative",
            "serve --state-folder",
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

    #[test]
    fn the_frame_link_command_line() {
        assert_eq!(
            parse_frame_link(args("")).unwrap(),
            FrameLink {
                file: PathBuf::from("/run/lampos-seen/frame-link.env"),
                socket: PathBuf::from("/run/lampos-eyes-frames/camera.frames"),
            }
        );
        assert_eq!(
            parse_frame_link(args("--file /a/f.env --socket /b/s.frames")).unwrap(),
            FrameLink {
                file: PathBuf::from("/a/f.env"),
                socket: PathBuf::from("/b/s.frames"),
            }
        );
        for bad in [
            "--file relative",
            "--file /a --file /b",
            "--other",
            "--socket",
        ] {
            assert!(parse_frame_link(args(bad)).is_err(), "{bad}");
        }
    }
}
