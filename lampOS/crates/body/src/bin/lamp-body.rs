//! The lamp's small expressive body: one process that owns the servo port
//! and moves the head a few degrees for what the lamp is doing.

use lamp_body::{calibration::UnitId, moves::Asker};
use std::{ffi::OsString, path::PathBuf};

const USAGE: &str = "lamp-body exit-report < LINE_OR_JOURNAL\n\nReads the newest exit report the body printed ({\"port\":...,\"body\":{...}}, e.g. from\n`journalctl -u lamp-body -o short-iso`) on stdin and prints it as plain lines: when (the journal\nprefix), the stop, stood and steps, per joint bad_temperatures, temperature_flaps and bad_reads,\nthe settles, the peak temperatures and goal_writes; a field older bodies lack says so. Exits 0 when\nthe newest was read, 1 when stdin holds none, 3 when the newest cannot be read (cut short: said,\nwith the previous readable one after it, labelled) and 4 when stdin cannot be read. It never\nopens the port.\n\nlamp-body --device ABSOLUTE_PATH --unit-id EXPLICIT_ID --lock-directory PRIVATE_ABSOLUTE_DIRECTORY --mood-socket ABSOLUTE_PATH [--home PITCH,ROLL,YAW] [--reach UP,DOWN,ROLL,YAW] [--speed COUNTS_PER_SECOND] [--servo-speed N] [--servo-acceleration N] [--stand PITCH,ROLL,YAW,SHOULDER,ELBOW --folded-arm SHOULDER,ELBOW [--stand-share PERCENT] [--stand-seconds N] [--awake-seconds N] [--arm-load SHOULDER,ELBOW] [--idle on|off]] [--gaze-socket ABSOLUTE_PATH [--gaze YAW,PITCH] [--gaze-reach YAW,PITCH]] [--stand-roll COUNTS] [--force-limit N] [--torque on|off] [--wake on|off] [--give-way return|stay|off] [--limp on|off] [--overload release] [--let-go-at-stop on|off [--folded-arm SHOULDER,ELBOW]] [--refused-let-go on|off] [--rest-glance on|off] [--pitch-window LOW,HIGH] [--look-at-name on|off] [--moves ABSOLUTE_DIR] [--arbitration on|off] [--caller-caps on|off] [--lease-socket ABSOLUTE_PATH] [--hold-from ABSOLUTE_FILE]\n\nOwns the servo port and moves head pitch, head roll and base yaw a few degrees around\na home pose (the one given, in encoder counts, or else where they are found), for the mood\nit is told on the socket. Without --stand it never touches the shoulder or the elbow, and\nwithout --stand or --torque on it writes torque only off, on all five, as a run ends on a surprise\nor a start is refused for heat or load. With --stand (which needs --home and --folded-arm) it also raises the shoulder and\nthe elbow to that pose when a mood other than rest arrives, folds them down again\n--awake-seconds after the last one, and writes torque once to a shoulder or elbow found\nwith an old overload latched. With --gaze-socket and --gaze it takes where a face is from\nlamp-eyes on that socket ({\"face\":{...}} or {\"face\":null}) and, while the lamp listens\nor speaks, turns the head to it in small corrections: --gaze is the counts of base yaw and of\nhead pitch, signed, that bring a face from the right and from the bottom edge of the picture\nto its middle. --gaze-socket without --gaze (a lamp whose gaze is not measured yet), --gaze-reach given or\nnot, runs with gaze off and says so once. Refuses to start unless the three joints have torque on, a clear\nstatus and the head is inside its start window (a let-go run, --let-go-at-stop on with no --home\nand no --torque on, found outside it with --folded-arm given alone and its shoulder and elbow\neach within 20 counts of those is let go at once, torque off only, no goal written; the folded\ncounts must come from a read the person confirmed was folded, never from this run's own read of\nwhere the arm is now); one found as hot or as loaded as a run stops\nat is refused with torque off on all five, so that nothing pushes on until the next start. With --refused-let-go\non (the default; off is the body as before), a lamp found folded whose start is refused only because\nthe head or the base is out of where it may start is let go instead (torque off on the three, and on\nthe shoulder and the elbow with --stand), says on the bus what it asks of a person (\"My head is tipped\ntoo far...\"), reads the joints ten times a second, and once they have been in range for a second takes\nhold where they are and starts as it would have; a head found tipped up out of range (pitch under\nits range, against its weight) keeps its torque so that it does not drop, and says so; after ten\nminutes nobody has set it straight it stops asking, stays let go and reads twice a second; a lamp\nfound up is refused as before. With --torque on it first takes hold of a\njoint found without torque (the three always, the shoulder and the elbow with --stand):\nits goal is written as where it is, then torque on, and if it then moves more than 12 counts\nit does not start. With --wake on and --home, a joint found outside where it may start\n(the head out of its start window, or a joint farther from home than it may be found) but within\n400 counts of home is not refused: before anything else it is brought home slowly, a small stir,\na pause, then an eased rise at 20 counts a second at most, never away from home, the head\nfirst and the base once the head is up; an arm found off\nits way is not lowered, and the lamp runs folded. By default --give-way return and --limp on
(--give-way off is the body as it was before them, and --limp off keeps giving way without the
slump). With --give-way, a joint that pushes at its goal without moving for\n1.5 s is told to stop (its goal becomes where it is; torque is never written off); with\n--give-way stay, a head or base joint that gave way and is moved more than 15 counts by\na hand for over a second stays where it was put. With --limp on as well, as the old runtime
did, a head or base joint that a hand stops and still pushes a second later makes the lamp
slump (torque off on all five, held again 5 s later where it lies; after an overload, 120 s);
a hand never ends the run, and a head roll or base found beyond 400 counts of home at start is
taken hold of where it lies. With --overload release, any joint that\nreads a load of 800 of 1000 (80% of --force-limit for a joint held to one) for 1.5 s has\ntorque switched off on all five, so the lamp slumps; 120 s later each is held where it is\n(its goal, then torque on) and the head is brought home as --wake does; a shoulder left\nup over an elbow that fell is first lowered folded, and otherwise the arm waits where it\nslumped until the lamp is next asked to stand; a second time within ten\nminutes ends the run with torque off. With --rest-glance on, a lamp at rest takes what\nExpression asks of the head (50 counts of pitch, 25 of roll at most, never the base or the arm)\nand comes back to rest when it runs out. --pitch-window is this lamp's own window for the\nhead's pitch while it is beside the base (by default lamp-4ace's, 1740,2030): never under\nthe head's floor (1670), around --home's pitch, and inside the servo's own limits or it does\nnot start. With --look-at-name on, the lamp's name heard on the bus\n(name/heard) while it does not speak is taken as being spoken to for two seconds: it rises,\nlooks up and turns to a face at once; with nobody in view it glances a little to one side and\nthe other once it stands, and lets it go. At most once in ten seconds. Stops at the first surprise, holding\nwhere it is, then, as the old runtime did, lets go: torque off on all five. Ends on SIGINT or SIGTERM after easing back to where it started, still holding (one asked while a slumped lamp's shoulder is being lowered holds where it is instead, and the next start lowers it); with\n--let-go-at-stop on (the factory's, off unless given) a lamp that got down, its head home and\nits arm folded, is then let go, torque off on all five, so that it can be read at rest.\nThe mood socket takes datagrams of JSON: {\"mood\":\"resting|listening|thinking|speaking\",\"level\":0..255}.\nA lamp standing at its full height also makes a gesture asked for there: {\"gesture\":\"nod\"}\n(nod, headshake, greeting, laugh, shy, stretching); a folded one with --stand stands up for it\nfirst, unless paused, held by a hand or slumped, and the journal says why each was taken or dropped. With --moves, the moves in that folder\n(NAME.json, docs/body.md) are read at start and asked for by name the same way; one this body\ncannot play is refused with why, and a folder that cannot be read leaves the body's own.\nA skill sends its own move whole, {\"move\":{\"name\":...,\"step_ms\":...,\"frames\":[...]}}, held to the same\nbounds and played the same way; one it cannot play is dropped with why.\nWith --arbitration on (off unless given), a gesture or a move may also say who asks,\n\"source\" (free text) and \"priority\" (person, safety, conversation, skill or idle;\nconversation without one). One asked while another plays ends it if asked higher, easing\nout as a dropped gesture does, and is dropped otherwise (\"dropped: busy, conversation\");\na hand on the lamp drops any (\"dropped: held by a hand\"). Off, \"source\" and\n\"priority\" are read and not weighed, so a caller may always say them. Either way, a\nskill's or idle gesture playing ends, eased out, when a conversation begins or a hand holds the head;\nwith LAMP_GESTURE_PAUSE=on, listening only pauses it: it ends when the lamp answers or after 3 s, and\ngoes on where it was if the lamp rests again sooner.\nWith --caller-caps on (off unless given; phase 0 of the lamp programming protocol, #3504), the kernel names\nwho sent each datagram, never the datagram: a gesture or a move is held to the most its systemd unit\nmay ask at, run as that unit's user (lamp-voice person; lamp-cloud and lamp-actions conversation;\nlamp-skills and lamp-control skill; lamp-addons and lamp-expression idle; root's other helpers what they\nask; anyone else idle), its source is that unit's name before what it said, and a cut is said once a\nminute per unit; the gaze socket takes a sight only from lamp-eyes, lamp-skills or root, and drops\nany other, said once a minute.\nWith --lease-socket (no product unit passes it yet; #3504, phase 0), lamp-control may hold the body for a\nwhile so that the lamp's own life stands back: one holder at a time, lamp-control.service as lampos or root,\nnamed by the kernel; lines of JSON, {\"hold\":{\"for_ms\":1..30000}} or {\"let_go\":true}, each answered, and\na change said unasked: {\"lease\":\"held\"}, {\"lease\":\"suspended\",\"why\":\"conversation\"}, {\"lease\":\"lost\",\"why\":\nhand|slumped|private|paused|asleep|lapsed}, {\"lease\":\"free\"} or {\"lease\":\"refused\",\"why\":...}. While held, no shift at rest\nand nothing from Expression; a hold not renewed lapses, and its holder gone ends it.\nWith LAMP_BODY_OWN_RANGES=on, each joint's own stored limits are read at start: a home, folded arm,\nstand or pitch window outside them does not start, and a head or base goal outside them (less 20)\nends the run; a joint storing 0..=4095 keeps its guard as before.\n--hold-from FILE is for tests on a lamp only: while FILE's first word is held, the head's pitch is\ntaken as held by a hand (its goal kept where it is, torque and giving way untouched), and the\njournal says the hand is from a test input; free, another word or no file is no hand. No product unit passes it.\nPrints one JSON report when it ends.\n";

#[derive(Debug)]
struct Arguments {
    device: PathBuf,
    unit_id: UnitId,
    lock_directory: PathBuf,
    mood_socket: PathBuf,
    home: Option<[u16; 3]>,
    reach: Option<[u16; 4]>,
    speed: Option<u16>,
    servo_speed: Option<u16>,
    servo_acceleration: Option<u8>,
    standing: Option<StandingChoice>,
    gaze: Option<GazeChoice>,
    /// `--gaze-socket` given without `--gaze`: a factory `lamp.env` before
    /// its gaze is measured with a face (install.md). The body runs with gaze
    /// off and says so; the path is still checked.
    unmeasured_gaze_socket: Option<PathBuf>,
    stand_roll: Option<u16>,
    force_limit: Option<u16>,
    torque_on: bool,
    wake: bool,
    give_way: Option<GiveWay>,
    limp: bool,
    overload_release: bool,
    let_go_at_stop: bool,
    /// `--folded-arm` alone with `--let-go-at-stop on` (#1682).
    let_go_folded_arm: Option<[u16; 2]>,
    /// `--refused-let-go` (#2602), on unless turned off.
    refused_let_go: bool,
    rest_glance: bool,
    pitch_window: Option<(u16, u16)>,
    look_at_name: bool,
    moves: Option<PathBuf>,
    arbitration: bool,
    /// `--caller-caps on` (#3504, phase 0): who asks is the kernel's word.
    caller_caps: bool,
    /// `--lease-socket` (#3504, phase 0): where lamp-control holds the body.
    lease_socket: Option<PathBuf>,
    /// `--hold-from FILE`: a hand on the head from a test input (#2668),
    /// for tests on a lamp only; no product unit passes it.
    hold_from: Option<PathBuf>,
}

/// Every option this build reads. One that is not among them is refused.
/// One from the lamp's own poses file (`BODY_POSES`, written by
/// `lamp-body-measure` into `/etc/lampOS/body/lamp.env`) is refused too,
/// with a line that names it and says the body stays still: a pose option a
/// newer measure adds may be a bound that lamp's mechanics need, as
/// `--pitch-window` once was, and a body that stands still until it is
/// updated is the safe failure, not one that moves on bounds it knows to be
/// incomplete (the first builder on #2071, 20 Oct 06:28; #1680's one
/// exception, a safety rule).
const OPTIONS: [&str; 36] = [
    "--arbitration",
    "--arm-load",
    "--awake-seconds",
    "--caller-caps",
    "--device",
    "--folded-arm",
    "--force-limit",
    "--gaze",
    "--gaze-reach",
    "--gaze-socket",
    "--give-way",
    "--hold-from",
    "--home",
    "--idle",
    "--lease-socket",
    "--let-go-at-stop",
    "--limp",
    "--lock-directory",
    "--look-at-name",
    "--mood-socket",
    "--moves",
    "--overload",
    "--pitch-window",
    "--reach",
    "--refused-let-go",
    "--rest-glance",
    "--servo-acceleration",
    "--servo-speed",
    "--speed",
    "--stand",
    "--stand-roll",
    "--stand-seconds",
    "--stand-share",
    "--torque",
    "--unit-id",
    "--wake",
];

/// What `--give-way` said; the body's own type is built for Linux only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GiveWay {
    Return,
    Stay,
}

/// What `--gaze-socket`, `--gaze` and `--gaze-reach` said.
#[derive(Clone, Debug, Eq, PartialEq)]
struct GazeChoice {
    socket: PathBuf,
    turn: [i16; 2],
    reach: [u16; 2],
}

/// What `--stand` and its options said. The body checks it against its own
/// caps when it is made.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StandingChoice {
    folded: [u16; 2],
    pose: [u16; 5],
    share: u8,
    seconds: u8,
    awake: u16,
    load: [u16; 2],
    idle: bool,
}

/// Whole numbers separated by commas, exactly as many as asked for.
fn counts_of<const N: usize>(value: &str) -> Option<[u16; N]> {
    let parts: Vec<u16> = value
        .split(',')
        .map(|part| part.parse::<u16>().ok().filter(|_| !part.starts_with('+')))
        .collect::<Option<_>>()?;
    parts.try_into().ok()
}

/// Two whole numbers separated by a comma, each with or without a minus.
fn signed_pair(value: &str) -> Option<[i16; 2]> {
    let (first, second) = value.split_once(',')?;
    let number = |part: &str| part.parse::<i16>().ok().filter(|_| !part.starts_with('+'));
    Some([number(first)?, number(second)?])
}

/// Three encoder positions, each a whole number of counts on one turn.
fn home_of(value: &str) -> Option<[u16; 3]> {
    let mut parts = value.split(',').map(|part| {
        part.parse::<u16>()
            .ok()
            .filter(|count| *count <= 4_095 && !part.starts_with('+'))
    });
    let home = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(home)
}

#[cfg(test)]
fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Arguments>, &'static str> {
    parse_with(args, None)
}

/// As [`parse`], `poses` being the lamp's poses file's options as systemd
/// gave them (`BODY_POSES`): an option there this build does not read is
/// still refused, and named.
fn parse_with(
    args: impl IntoIterator<Item = OsString>,
    poses: Option<&str>,
) -> Result<Option<Arguments>, &'static str> {
    let mut args = args.into_iter();
    let (mut device, mut unit_id, mut lock_directory, mut mood_socket) = (None, None, None, None);
    let mut home = None;
    let (mut reach, mut speed, mut servo_speed, mut servo_acceleration) = (None, None, None, None);
    let (mut stand, mut folded_arm, mut arm_load) = (None, None, None);
    let (mut stand_share, mut stand_seconds, mut awake_seconds) = (None, None, None);
    let mut idle = None;
    let (mut gaze_socket, mut gaze_turn, mut gaze_reach) = (None, None, None);
    let mut stand_roll = None;
    let mut force_limit = None;
    let mut torque = None;
    let mut wake = None;
    let mut give_way = None;
    let mut limp = None;
    let mut overload = None;
    let mut let_go_at_stop = None;
    let mut refused_let_go = None;
    let mut rest_glance = None;
    let mut pitch_window = None;
    let mut look_at_name = None;
    let mut moves = None;
    let mut arbitration = None;
    let mut caller_caps = None;
    let mut lease_socket = None;
    let mut hold_from = None;
    while let Some(key) = args.next() {
        if key == "--help" {
            if device.is_some()
                || unit_id.is_some()
                || lock_directory.is_some()
                || mood_socket.is_some()
                || home.is_some()
                || reach.is_some()
                || speed.is_some()
                || servo_speed.is_some()
                || servo_acceleration.is_some()
                || stand.is_some()
                || folded_arm.is_some()
                || arm_load.is_some()
                || stand_share.is_some()
                || stand_seconds.is_some()
                || awake_seconds.is_some()
                || idle.is_some()
                || gaze_socket.is_some()
                || gaze_turn.is_some()
                || gaze_reach.is_some()
                || stand_roll.is_some()
                || force_limit.is_some()
                || torque.is_some()
                || wake.is_some()
                || give_way.is_some()
                || limp.is_some()
                || overload.is_some()
                || let_go_at_stop.is_some()
                || refused_let_go.is_some()
                || rest_glance.is_some()
                || pitch_window.is_some()
                || look_at_name.is_some()
                || moves.is_some()
                || arbitration.is_some()
                || caller_caps.is_some()
                || lease_socket.is_some()
                || hold_from.is_some()
                || args.next().is_some()
            {
                return Err("--help must be used alone");
            }
            return Ok(None);
        }
        let value = args.next().ok_or("every option requires a value")?;
        match key.to_str() {
            Some("--device") if device.is_none() => device = Some(PathBuf::from(value)),
            Some("--lock-directory") if lock_directory.is_none() => {
                lock_directory = Some(PathBuf::from(value));
            }
            Some("--mood-socket") if mood_socket.is_none() => {
                mood_socket = Some(PathBuf::from(value));
            }
            Some("--home") if home.is_none() => {
                home = Some(
                    value
                        .to_str()
                        .and_then(home_of)
                        .ok_or("--home is PITCH,ROLL,YAW in encoder counts")?,
                );
            }
            Some("--unit-id") if unit_id.is_none() => {
                unit_id = Some(
                    UnitId::new(value.to_str().ok_or("unit ID must be UTF-8")?)
                        .map_err(|_| "invalid explicit unit ID")?,
                );
            }
            Some("--reach") if reach.is_none() => {
                reach = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<4>)
                        .ok_or("--reach is UP,DOWN,ROLL,YAW in encoder counts")?,
                );
            }
            Some("--speed") if speed.is_none() => {
                speed = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<1>)
                        .ok_or("--speed is a whole number of counts a second")?[0],
                );
            }
            Some("--servo-speed") if servo_speed.is_none() => {
                servo_speed = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<1>)
                        .ok_or("--servo-speed is a whole number")?[0],
                );
            }
            Some("--servo-acceleration") if servo_acceleration.is_none() => {
                servo_acceleration = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<1>)
                        .and_then(|value| u8::try_from(value[0]).ok())
                        .ok_or("--servo-acceleration is a whole number under 256")?,
                );
            }
            Some("--stand") if stand.is_none() => {
                stand = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<5>)
                        .ok_or("--stand is PITCH,ROLL,YAW,SHOULDER,ELBOW in encoder counts")?,
                );
            }
            Some("--folded-arm") if folded_arm.is_none() => {
                folded_arm = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<2>)
                        .ok_or("--folded-arm is SHOULDER,ELBOW in encoder counts")?,
                );
            }
            Some("--arm-load") if arm_load.is_none() => {
                arm_load = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<2>)
                        .ok_or("--arm-load is SHOULDER,ELBOW")?,
                );
            }
            Some("--stand-share") if stand_share.is_none() => {
                stand_share = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<1>)
                        .and_then(|value| u8::try_from(value[0]).ok())
                        .ok_or("--stand-share is a whole percentage")?,
                );
            }
            Some("--stand-seconds") if stand_seconds.is_none() => {
                stand_seconds = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<1>)
                        .and_then(|value| u8::try_from(value[0]).ok())
                        .ok_or("--stand-seconds is a whole number of seconds")?,
                );
            }
            Some("--awake-seconds") if awake_seconds.is_none() => {
                awake_seconds = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<1>)
                        .ok_or("--awake-seconds is a whole number of seconds")?[0],
                );
            }
            Some("--idle") if idle.is_none() => {
                idle = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--idle is on or off"),
                });
            }
            Some("--force-limit") if force_limit.is_none() => {
                force_limit = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<1>)
                        .map(|value| value[0])
                        .filter(|force| (300..=1_000).contains(force))
                        .ok_or("--force-limit is a whole number from 300 to 1000")?,
                );
            }
            Some("--torque") if torque.is_none() => {
                torque = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--torque is on or off"),
                });
            }
            Some("--wake") if wake.is_none() => {
                wake = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--wake is on or off"),
                });
            }
            Some("--refused-let-go") if refused_let_go.is_none() => {
                refused_let_go = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--refused-let-go is on or off"),
                });
            }
            Some("--rest-glance") if rest_glance.is_none() => {
                rest_glance = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--rest-glance is on or off"),
                });
            }
            Some("--pitch-window") if pitch_window.is_none() => {
                // The head's floor, `Limits::FOLDED.pitch_floor` (built for
                // Linux only).
                let floor = 1_670;
                pitch_window = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<2>)
                        .filter(|[low, high]| floor <= *low && low < high && *high <= 4_095)
                        .map(|[low, high]| (low, high))
                        .ok_or(
                            "--pitch-window is LOW,HIGH in counts, LOW under HIGH, 1670 or over",
                        )?,
                );
            }
            Some("--give-way") if give_way.is_none() => {
                give_way = Some(match value.to_str() {
                    Some("return") => Some(GiveWay::Return),
                    Some("stay") => Some(GiveWay::Stay),
                    Some("off") => None,
                    _ => return Err("--give-way is return, stay or off"),
                });
            }
            Some("--limp") if limp.is_none() => {
                limp = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--limp is on or off"),
                });
            }
            Some("--let-go-at-stop") if let_go_at_stop.is_none() => {
                let_go_at_stop = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--let-go-at-stop is on or off"),
                });
            }
            Some("--overload") if overload.is_none() => {
                overload = Some(match value.to_str() {
                    Some("release") => true,
                    _ => return Err("--overload is release"),
                });
            }
            Some("--moves") if moves.is_none() => moves = Some(PathBuf::from(value)),
            Some("--hold-from") if hold_from.is_none() => hold_from = Some(PathBuf::from(value)),
            Some("--lease-socket") if lease_socket.is_none() => {
                lease_socket = Some(PathBuf::from(value));
            }
            Some("--arbitration") if arbitration.is_none() => {
                arbitration = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--arbitration is on or off"),
                });
            }
            Some("--caller-caps") if caller_caps.is_none() => {
                caller_caps = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--caller-caps is on or off"),
                });
            }
            Some("--look-at-name") if look_at_name.is_none() => {
                look_at_name = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--look-at-name is on or off"),
                });
            }
            Some("--stand-roll") if stand_roll.is_none() => {
                stand_roll = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<1>)
                        .map(|value| value[0])
                        .filter(|counts| *counts <= 200)
                        .ok_or("--stand-roll is a whole number of counts, 200 at most")?,
                );
            }
            Some("--gaze-socket") if gaze_socket.is_none() => {
                gaze_socket = Some(PathBuf::from(value));
            }
            Some("--gaze") if gaze_turn.is_none() => {
                gaze_turn = Some(
                    value
                        .to_str()
                        .and_then(signed_pair)
                        .ok_or("--gaze is YAW,PITCH in signed encoder counts")?,
                );
            }
            Some("--gaze-reach") if gaze_reach.is_none() => {
                gaze_reach = Some(
                    value
                        .to_str()
                        .and_then(counts_of::<2>)
                        .ok_or("--gaze-reach is YAW,PITCH in encoder counts")?,
                );
            }
            _ => {
                let name = key.to_str().unwrap_or_default();
                let from_poses =
                    poses.is_some_and(|poses| poses.split_whitespace().any(|word| word == name));
                if !OPTIONS.contains(&name) && from_poses {
                    eprintln!(
                        "lamp-body: /etc/lampOS/body/lamp.env: {name}: an option this build does not read, written by a newer measure; the body stays still until this lamp is updated"
                    );
                    return Err("an option in the lamp's poses file this build does not read");
                }
                return Err("unknown or duplicate option");
            }
        }
    }
    // Eyes are a socket and what the picture is worth, or nothing. A socket
    // without --gaze is a lamp whose gaze is not measured yet: the product's
    // unit passes the socket and the reach, and lamp-body-measure writes no
    // --gaze (thirteen-P1, lamp-c0a4 on 19 Oct); it runs with gaze off.
    let (gaze, unmeasured_gaze_socket) = match (gaze_socket, gaze_turn) {
        (Some(socket), Some(turn)) => (
            Some(GazeChoice {
                socket,
                turn,
                reach: gaze_reach.unwrap_or([250, 120]),
            }),
            None,
        ),
        (Some(socket), None) => (None, Some(socket)),
        (None, None) if gaze_reach.is_none() => (None, None),
        _ => return Err("--gaze needs --gaze-socket, and --gaze-reach needs them"),
    };
    // A load bound at or over the force limit could never be read, and the
    // guard it is would be off.
    if let (Some(force), Some(load)) = (force_limit, arm_load)
        && load.iter().any(|bound| *bound >= force)
    {
        return Err("--arm-load must be under --force-limit");
    }
    // `--folded-arm` alone is a let-go run's (#1682): with `--let-go-at-stop
    // on`, no `--home` and no `--torque on`, nothing else of standing. Its
    // counts are ones a person confirmed folded, never this run's read.
    let let_go_folded_arm = match (stand, folded_arm) {
        (None, Some(folded))
            if let_go_at_stop == Some(true)
                && home.is_none()
                && torque != Some(true)
                && arm_load.is_none()
                && stand_share.is_none()
                && stand_seconds.is_none()
                && awake_seconds.is_none()
                && idle.is_none() =>
        {
            folded_arm = None;
            Some(folded)
        }
        _ => None,
    };
    // Standing is one choice made whole: a pose, the folded arm and a home.
    let standing = match (stand, folded_arm) {
        (Some(pose), Some(folded)) if home.is_some() => Some(StandingChoice {
            folded,
            pose,
            share: stand_share.unwrap_or(100),
            seconds: stand_seconds.unwrap_or(5),
            awake: awake_seconds.unwrap_or(90),
            // A joint held to a force limit never reads a load above
            // it: the bound that sends the arm back down sits under the
            // limit, or the limit would switch that guard off.
            load: arm_load
                .unwrap_or_else(|| force_limit.map_or([950, 950], |force| [force - 40; 2])),
            idle: idle.unwrap_or(true),
        }),
        (None, None)
            if arm_load.is_none()
                && stand_share.is_none()
                && stand_seconds.is_none()
                && awake_seconds.is_none()
                && idle.is_none() =>
        {
            None
        }
        _ => return Err("--stand needs --home and --folded-arm, and its options need --stand"),
    };
    // A window the home's pitch is outside would push the head off home.
    if let (Some((low, high)), Some(home)) = (pitch_window, home)
        && !(low..=high).contains(&home[0])
    {
        return Err("--pitch-window must contain --home's pitch");
    }
    let result = Arguments {
        device: device.ok_or("--device is required")?,
        unit_id: unit_id.ok_or("--unit-id is required")?,
        lock_directory: lock_directory.ok_or("--lock-directory is required")?,
        mood_socket: mood_socket.ok_or("--mood-socket is required")?,
        home,
        reach,
        speed,
        servo_speed,
        servo_acceleration,
        standing,
        gaze,
        unmeasured_gaze_socket,
        stand_roll,
        force_limit,
        torque_on: torque.unwrap_or(false),
        wake: wake.unwrap_or(true),
        // On by default since #703 (shown by hand on lamp-52e6, 2026-10-15):
        // a hand is given way to, and forced hard it slumps the lamp as V1
        // did. `--give-way off` is the body as it was before.
        give_way: give_way.unwrap_or(Some(GiveWay::Return)),
        limp: limp.unwrap_or(give_way.is_none_or(|way| way.is_some())),
        overload_release: overload.unwrap_or(false),
        let_go_at_stop: let_go_at_stop.unwrap_or(false),
        let_go_folded_arm,
        // On unless turned off (#2602): a lamp refused for a pose a hand
        // can set straight held its torque, so no hand could, and only a
        // power cut let it start. `--refused-let-go off` is the body as it
        // was before.
        refused_let_go: refused_let_go.unwrap_or(true),
        rest_glance: rest_glance.unwrap_or(true),
        pitch_window,
        look_at_name: look_at_name.unwrap_or(true),
        moves,
        arbitration: arbitration.unwrap_or(false),
        caller_caps: caller_caps.unwrap_or(false),
        lease_socket,
        hold_from,
    };
    if result.limp && result.give_way.is_none() {
        return Err("--limp on needs --give-way return or stay");
    }
    if result.stand_roll.is_some() && result.standing.is_none() {
        return Err("--stand-roll needs --stand");
    }
    let gaze_path = result
        .gaze
        .as_ref()
        .map(|gaze| &gaze.socket)
        .or(result.unmeasured_gaze_socket.as_ref());
    for path in [&result.device, &result.lock_directory, &result.mood_socket]
        .into_iter()
        .chain(gaze_path)
        .chain(result.moves.as_ref())
        .chain(result.hold_from.as_ref())
        .chain(result.lease_socket.as_ref())
    {
        if !path.is_absolute()
            || path.as_os_str().len() > 100
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

/// One datagram from the voice service. Anything else is ignored.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn mood_of(datagram: &[u8]) -> Option<(lamp_body::gesture::Mood, u8)> {
    mood_message(datagram).map(|message| (message.mood, message.level))
}

/// A mood datagram: the mood, its level, and with `reading` the place
/// across the picture a skill turns it toward (`toward`, #1679).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MoodMessage {
    mood: lamp_body::gesture::Mood,
    level: u8,
    #[serde(default)]
    toward: Option<f32>,
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn mood_message(datagram: &[u8]) -> Option<MoodMessage> {
    serde_json::from_slice(datagram).ok()
}

/// Where a `reading` datagram turns the lamp, -1 to 1 across the picture:
/// nothing for another mood, none given, or a place off the picture.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn toward_of(datagram: &[u8]) -> Option<f32> {
    let message = mood_message(datagram)?;
    message.toward.filter(|toward| {
        message.mood == lamp_body::gesture::Mood::Reading
            && toward.is_finite()
            && toward.abs() <= 1.0
    })
}

/// Which of the moods heard in one step counts, in the order they came,
/// given whether a skill said `reading` within `MOOD_STALE` before them.
/// Otherwise the newest. The voice tells its own mood every 40 ms
/// (`resting` between turns): while a skill holds the lamp reading, none of
/// that takes the pose from it, or the two would take turns at the head.
/// A `reading` heard in this step counts whatever came after it in the same
/// step, so that the hold is renewed by every one the skill sends; with
/// only the newest kept, a `resting` landing after it dropped the lamp to
/// rest (lamp-52e6, 2026-10-16, 14:30, twice in ten seconds at one a
/// second). Reading ends when the skill stops saying it.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn of_this_step(
    heard: &[(lamp_body::gesture::Mood, u8)],
    reading: bool,
) -> Option<(lamp_body::gesture::Mood, u8)> {
    let said = heard
        .iter()
        .rev()
        .find(|(mood, _)| *mood == lamp_body::gesture::Mood::Reading);
    match (said, heard.last()) {
        (Some(said), _) => Some(*said),
        (None, Some(_)) if reading => None,
        (None, newest) => newest.copied(),
    }
}

/// A gesture asked for by the voice service: `{"gesture":"nod"}` and nothing
/// else in it, naming a clip this body has, its own or a move read from
/// `--moves`.
#[cfg(test)]
fn gesture_of(
    datagram: &[u8],
    moves: &lamp_body::moves::Loaded,
) -> Option<&'static lamp_body::clips::Clip> {
    gesture_asked(datagram, moves).map(|(clip, _)| clip)
}

/// The same, with who asks for it (`source`, `priority`) when it says.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn gesture_asked(
    datagram: &[u8],
    moves: &lamp_body::moves::Loaded,
) -> Option<(&'static lamp_body::clips::Clip, Asker)> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Message<'a> {
        gesture: &'a str,
        source: Option<String>,
        priority: Option<lamp_body::moves::Priority>,
    }
    let message: Message = serde_json::from_slice(datagram).ok()?;
    let clip = lamp_body::clips::named(message.gesture).or_else(|| moves.named(message.gesture))?;
    let asker = Asker {
        source: message.source,
        priority: message.priority,
    };
    Some((clip, asker))
}

/// Of a gesture already read this step and one read after it, the one the
/// body is handed: under `--arbitration on` the higher priority, the newer
/// on a tie, and the other's name and why it is dropped, naming who holds
/// the body; off, the newer, as before, and nothing said.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
type Gesture = (&'static lamp_body::clips::Clip, lamp_body::moves::Priority);
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn kept_of(
    held: Option<Gesture>,
    asked: Gesture,
    arbitration: bool,
) -> (Gesture, Option<(&'static str, &'static str)>) {
    match held {
        Some(old) if arbitration && old.1 > asked.1 => {
            (old, Some((asked.0.name, lamp_body::moves::busy(old.1))))
        }
        Some(old) if arbitration => (asked, Some((old.0.name, lamp_body::moves::busy(asked.1)))),
        _ => (asked, None),
    }
}

/// What `body/gesture` says of the gesture [`kept_of`] drops (#940, the
/// reviewer on #1473: two skills asking within one step): its name and
/// why, whose it was and who keeps the body, as their datagrams named
/// them. `kept_new`: the one read last was kept; `source`, its asker's;
/// `held`, the name and asker of the one read before it, when it named one.
/// Its asker and the one that keeps the body come back as
/// `(source, by)`, for a `Told`.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn intake_dropped(
    kept_new: bool,
    source: Option<&str>,
    held: Option<(&str, &str)>,
) -> (
    Option<lamp_body::moves::Source>,
    Option<lamp_body::moves::Source>,
) {
    use lamp_body::moves::Source;
    let held = held.and_then(|(_, asker)| Source::new(asker));
    let asked = source.and_then(Source::new);
    if kept_new {
        (held, asked)
    } else {
        (asked, held)
    }
}

/// Under `--caller-caps on` (phase 0 of the lamp programming protocol,
/// #3504): a gesture's or a move's asker held to the most its caller may
/// ask at (`lamp_msgs::most_for`), whatever its datagram says, and named
/// by its unit before what it said (`lamp-skills/skill:dance`): `root` for
/// root with no unit, `unknown` for a caller the kernel did not name or not
/// as a lamp part. Under `--arbitration off` a source is still not kept.
/// The claim it cut, if it did: who, what it asked and what it is held to.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn held_to(
    asker: Asker,
    caller: Option<&lamp_ipc::caller::Caller>,
    lampos: Option<u32>,
    arbitration: bool,
) -> (
    Asker,
    Option<(
        String,
        lamp_body::moves::Priority,
        lamp_body::moves::Priority,
    )>,
) {
    use lamp_msgs::{MOST_BY_UNIT, RunsAs};
    let most = caller.map_or(Some(lamp_body::moves::Priority::Idle), |caller| {
        lamp_msgs::most_for(caller.unit.as_deref(), caller.uid, lampos)
    });
    let part = |unit: &str| {
        MOST_BY_UNIT.iter().any(|(name, runs_as, _)| {
            *name == unit && *runs_as == RunsAs::Lampos && caller.map(|caller| caller.uid) == lampos
        })
    };
    let who = match caller {
        Some(caller) if caller.uid == 0 => caller.unit.as_deref().unwrap_or("root.service"),
        Some(caller) => caller
            .unit
            .as_deref()
            .filter(|unit| part(unit))
            .unwrap_or("unknown.service"),
        None => "unknown.service",
    };
    let who = who.strip_suffix(".service").unwrap_or(who).to_owned();
    let asked = asker.priority();
    let held = most.map_or(asked, |most| asked.min(most));
    let source = (arbitration || asker.source.is_some()).then(|| match &asker.source {
        Some(said) => format!("{who}/{said}"),
        None => who.clone(),
    });
    let cut = (held < asked).then_some((who, asked, held));
    (
        Asker {
            source,
            priority: Some(held),
        },
        cut,
    )
}

/// Under `--caller-caps on`: whether a sight on the gaze socket is taken.
/// Only the eyes and a skill's run send one, as `lampos`; root may.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn sight_from(caller: Option<&lamp_ipc::caller::Caller>, lampos: Option<u32>) -> bool {
    caller.is_some_and(|caller| {
        caller.uid == 0
            || (Some(caller.uid) == lampos
                && matches!(
                    caller.unit.as_deref(),
                    Some("lamp-eyes.service" | "lamp-skills.service")
                ))
    })
}

/// Once a minute per unit: whether a line about `who` is due at `now`.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
#[derive(Default)]
struct Minutely(std::collections::HashMap<String, std::time::Instant>);

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
impl Minutely {
    fn due(&mut self, who: &str, now: std::time::Instant) -> bool {
        const EVERY: std::time::Duration = std::time::Duration::from_secs(60);
        self.0
            .retain(|_, at| now.saturating_duration_since(*at) < EVERY);
        if self.0.contains_key(who) {
            return false;
        }
        self.0.insert(who.to_owned(), now);
        true
    }
}

/// Whether the body can't be moved by a holder now, for its lease: let go
/// (torque off after a slump), or let go and waiting for a hand to set its
/// pose right (`--refused-let-go`, the reviewer-lead's P3 on #3820).
#[cfg(target_os = "linux")]
fn slumped_now(glimpse: &lamp_body::body::Glimpse) -> bool {
    glimpse.limp || glimpse.waiting.is_some()
}

/// Who may hold the body by `--lease-socket`: `lamp-control.service` run
/// as `lampos`, or root, as the kernel names them (#3504, phase 0).
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn holder_may(caller: Option<&lamp_ipc::caller::Caller>, lampos: Option<u32>) -> bool {
    caller.is_some_and(|caller| {
        caller.uid == 0
            || (Some(caller.uid) == lampos
                && caller.unit.as_deref() == Some("lamp-control.service"))
    })
}

/// Whether a caller the kernel named may hold the body.
// Used by the Linux body and its Linux-only test.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
type Who = Box<dyn Fn(Option<&lamp_ipc::caller::Caller>) -> bool>;

/// `--lease-socket` (#3504, phase 0): the door `lamp-control` holds the
/// body by (`lamp_body::lease`). One holder at a time, let in only if
/// `may` says so of who the kernel names; lines of JSON each way. Its
/// holder gone, the lease ends at once.
// Used by the Linux body and its Linux-only test.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct LeaseDoor {
    listener: std::os::unix::net::UnixListener,
    may: Who,
    holder: Option<(std::os::unix::net::UnixStream, Vec<u8>)>,
    lease: lamp_body::lease::Lease,
}

// Used by the Linux body and its Linux-only test.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl LeaseDoor {
    /// The longest line a holder may send.
    const MOST_LINE: usize = 1024;
    /// How many new callers one step lets in or refuses.
    const MOST_ACCEPTS: usize = 4;
    /// The most one step reads of its holder.
    const MOST_READ: usize = Self::MOST_LINE * 4;

    fn bind(path: &std::path::Path, may: Who) -> std::io::Result<Self> {
        // A socket file left by an earlier run is not a listener: replace it.
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = std::os::unix::net::UnixListener::bind(path)?;
        // Only the body's own user and group may connect; the kernel's
        // word on who connected is still what lets one in.
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o660))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            may,
            holder: None,
            lease: Default::default(),
        })
    }

    /// One step: a new holder let in or refused, its lines answered and a
    /// change said; whether the body is held now. Never waits.
    /// The run is over: a holder still connected is told the lease is
    /// lost and why, before the socket closes (an overload a second time
    /// within ten minutes ends the run, the reviewer-lead's P3 on #3820).
    fn ended(&mut self, why: Option<&'static str>) {
        use std::io::Write;
        // Only a lease still taken, held or suspended: one already lost,
        // let go or lapsed was told so then (Third on #3828).
        if let (Some((stream, _)), Some(why), true) =
            (self.holder.as_mut(), why, self.lease.taken())
        {
            let _ = stream.write_all(lamp_body::lease::Said::lost(why).line().as_bytes());
        }
        self.holder = None;
        self.lease.end();
    }

    fn turn(&mut self, around: lamp_body::lease::Around, now: std::time::Instant) -> bool {
        use lamp_body::lease::Said;
        use std::io::Write;
        for _ in 0..Self::MOST_ACCEPTS {
            let Ok((mut stream, _)) = self.listener.accept() else {
                break;
            };
            let caller = lamp_ipc::caller::of_stream(&stream);
            let why = if !(self.may)(caller.as_ref()) {
                Some("only lamp-control may hold the body")
            } else if self.holder.is_some() {
                Some("the body has a holder")
            } else {
                None
            };
            match why {
                Some(why) => {
                    let _ = stream.write_all(Said::refused(why).line().as_bytes());
                }
                None if stream.set_nonblocking(true).is_ok() => {
                    eprintln!("lamp-body: the lease's door is open to {caller:?}");
                    self.holder = Some((stream, Vec::new()));
                }
                None => {}
            }
        }
        let Some((stream, pending)) = self.holder.as_mut() else {
            return false;
        };
        let mut gone = read_some(stream, pending, Self::MOST_READ, Self::MOST_LINE);
        let mut out = String::new();
        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            out += &self.lease.ask(&line[..end], around, now).line();
        }
        if pending.len() > Self::MOST_LINE {
            out += &Said::refused("a line longer than 1024 bytes").line();
            gone = true;
        }
        if let Some(said) = self.lease.step(around, now) {
            out += &said.line();
        }
        if !out.is_empty() && stream.write_all(out.as_bytes()).is_err() {
            gone = true;
        }
        if gone {
            self.holder = None;
            eprintln!("lamp-body: the lease's holder went: the lamp's own life is back");
            self.lease.end();
        }
        self.lease.holds(now)
    }
}

/// What the holder sent, read into `pending` without waiting: at most
/// `most` bytes, and none more once `pending` has passed `line`, so a holder
/// that never stops writing can't hold the body's step (Fourteen and the
/// reviewer-lead on #3791). Whether it is gone (closed, or an error).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn read_some(
    stream: &mut std::os::unix::net::UnixStream,
    pending: &mut Vec<u8>,
    most: usize,
    line: usize,
) -> bool {
    use std::io::Read;
    let mut buffer = [0u8; 512];
    let mut read = 0;
    while read < most && pending.len() <= line {
        let room = buffer.len().min(most - read);
        match stream.read(&mut buffer[..room]) {
            Ok(0) => return true,
            Ok(length) => {
                pending.extend_from_slice(&buffer[..length]);
                read += length;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return false,
            Err(_) => return true,
        }
    }
    false
}

/// What one datagram on the mood socket asks.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
#[derive(Debug, PartialEq)]
enum Heard {
    /// The body's own clip, a `--moves` file's, or a skill's sent whole.
    Gesture(&'static lamp_body::clips::Clip),
    Mood(lamp_body::gesture::Mood, u8),
    /// Not taken, and why, as the journal says it.
    Dropped(String),
}

/// A gesture by name first, then a move sent whole, then a mood.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
#[cfg(test)]
fn heard(
    datagram: &[u8],
    moves: &lamp_body::moves::Loaded,
    sent: &mut lamp_body::moves::Sent,
) -> Heard {
    heard_asked(datagram, moves, sent, false).0
}

/// The same, and who asked for a gesture or a move. A caller may always
/// say so (`source`, `priority`); only under `--arbitration on` is it
/// weighed, so callers and bodies need not switch together. Off, `source`
/// is ignored and `priority` is kept only so that a skill's or idle
/// gesture still ends when a conversation begins or a hand comes. Any
/// other field is refused either way.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn heard_asked(
    datagram: &[u8],
    moves: &lamp_body::moves::Loaded,
    sent: &mut lamp_body::moves::Sent,
    arbitration: bool,
) -> (Heard, Asker) {
    // `recv` cuts a longer datagram to the buffer, which would then not parse.
    if datagram.len() >= lamp_body::moves::MOST_DATAGRAM {
        return (
            Heard::Dropped("a datagram longer than 16 KiB, not read".into()),
            Asker::default(),
        );
    }
    let weighed = |asker: Asker| {
        if arbitration {
            asker
        } else {
            Asker {
                source: None,
                ..asker
            }
        }
    };
    if let Some((clip, asker)) = gesture_asked(datagram, moves) {
        return (Heard::Gesture(clip), weighed(asker));
    }
    match sent.take_asked(datagram) {
        Some((Ok(clip), asker)) => (Heard::Gesture(clip), weighed(asker)),
        Some((Err(why), _)) => (
            Heard::Dropped(format!("a move this body cannot play: {why}")),
            Asker::default(),
        ),
        None => match mood_of(datagram) {
            Some((mood, level)) => (Heard::Mood(mood, level), Asker::default()),
            None => (
                Heard::Dropped(format!(
                    "not a mood or a gesture this body has: {}",
                    String::from_utf8_lossy(&datagram[..datagram.len().min(200)])
                )),
                Asker::default(),
            ),
        },
    }
}

/// Each journal line about what was asked is said at most once in this
/// long, and then with how many like it were not said.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
const HUSH: std::time::Duration = std::time::Duration::from_secs(5);

/// Lines said lately, by their text, and how many repeats were held back.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
#[derive(Default)]
struct Hush(std::collections::HashMap<String, (std::time::Instant, u32)>);

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
impl Hush {
    /// `line` if it is due at `now`, with how many like it were held back
    /// since it was last said; `None` while it is held back. A line not
    /// heard of for a minute is forgotten, so the map stays small.
    fn due(&mut self, line: String, now: std::time::Instant) -> Option<String> {
        self.0
            .retain(|_, (at, _)| now.saturating_duration_since(*at) < HUSH * 12);
        match self.0.get_mut(&line) {
            Some((at, held)) if now.saturating_duration_since(*at) < HUSH => {
                *held += 1;
                None
            }
            Some((at, held)) => {
                let said = match std::mem::take(held) {
                    0 => line,
                    more => format!("{line} ({more} more like it before this)"),
                };
                *at = now;
                Some(said)
            }
            None => {
                self.0.insert(line.clone(), (now, 0));
                Some(line)
            }
        }
    }
}

/// The moves in `--moves`, read once at start. A refused move and a folder
/// that cannot be read are said, and the body runs with what it has.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn moves_of(folder: Option<&std::path::Path>) -> lamp_body::moves::Loaded {
    let Some(folder) = folder else {
        return lamp_body::moves::Loaded::default();
    };
    match lamp_body::moves::load(folder) {
        Ok(loaded) => {
            for (path, why) in &loaded.refused {
                eprintln!("lamp-body: move {} refused: {why}", path.display());
            }
            let names: Vec<_> = loaded.moves.iter().map(|clip| clip.name).collect();
            eprintln!(
                "lamp-body: {} moves from {}: {}",
                names.len(),
                folder.display(),
                names.join(", ")
            );
            loaded
        }
        Err(error) => {
            eprintln!(
                "lamp-body: the moves in {} cannot be read, so only the body's own: {error}",
                folder.display()
            );
            lamp_body::moves::Loaded::default()
        }
    }
}

fn main() -> std::process::ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    #[cfg(target_os = "linux")]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|first| first == "exit-report")
    {
        return exit_report();
    }
    let poses = std::env::var("BODY_POSES").ok();
    let arguments = match parse_with(std::env::args_os().skip(1), poses.as_deref()) {
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
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(error) => {
            eprintln!("lamp-body failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// `lamp-body exit-report`: the last exit report on stdin (a line, or a
/// whole journal) as plain lines on stdout. Exits 0 when the newest was
/// read, 1 when there is none, 3 when the newest could not be read (the
/// previous one follows, labelled) and 4 when stdin cannot be read.
#[cfg(target_os = "linux")]
fn exit_report() -> std::process::ExitCode {
    let mut text = String::new();
    if let Err(error) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text) {
        eprintln!("lamp-body exit-report: stdin cannot be read: {error}");
        return std::process::ExitCode::from(4);
    }
    match lamp_body::exit_report::summary(&text) {
        Some(lamp_body::exit_report::Found::Read(summary)) => {
            print!("{summary}");
            std::process::ExitCode::SUCCESS
        }
        Some(lamp_body::exit_report::Found::Unreadable(summary)) => {
            print!("{summary}");
            std::process::ExitCode::from(3)
        }
        None => {
            eprintln!("lamp-body exit-report: no exit report on stdin");
            std::process::ExitCode::FAILURE
        }
    }
}

/// What each step tells systemd: ready at the first, which comes only once
/// the start checks have passed, and alive at every one (sent at most twice
/// in a watchdog interval, and never without `$NOTIFY_SOCKET`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn step_said(notify: &lamp_keeper::Notify, ready: &std::cell::Cell<bool>) {
    if !ready.replace(true) {
        notify.ready();
    }
    notify.alive();
}

#[cfg(not(target_os = "linux"))]
fn run(arguments: Arguments) -> Result<bool, Box<dyn std::error::Error>> {
    let _ = (arguments.unit_id, arguments.mood_socket, arguments.home);
    let _ = (
        arguments.reach,
        arguments.speed,
        arguments.servo_speed,
        arguments.servo_acceleration,
        arguments.standing,
        arguments.gaze,
        arguments.unmeasured_gaze_socket,
        arguments.stand_roll,
        arguments.force_limit,
        arguments.torque_on,
        arguments.wake,
        arguments.give_way,
        arguments.limp,
        arguments.overload_release,
        arguments.let_go_at_stop,
        arguments.let_go_folded_arm,
        arguments.refused_let_go,
        arguments.rest_glance,
        arguments.pitch_window,
        arguments.look_at_name,
        arguments.moves,
        arguments.arbitration,
        arguments.caller_caps,
    );
    Err("the body requires Linux; no device was opened".into())
}

#[cfg(target_os = "linux")]
fn run(arguments: Arguments) -> Result<bool, Box<dyn std::error::Error>> {
    use lamp_body::{
        body::{Limits, Stop, Tuning},
        linux::{InspectionConfig, run_body},
    };
    use nix::sys::{
        signal::{SigSet, Signal},
        signalfd::{SfdFlags, SignalFd},
    };
    use std::{io::Write, os::unix::net::UnixDatagram};

    let unplug = lamp_body::unplug::Adapter::from_environment()?;
    struct SignalMask(SigSet);
    impl Drop for SignalMask {
        fn drop(&mut self) {
            let _ = self.0.thread_set_mask();
        }
    }
    let config = InspectionConfig::new(
        arguments.device,
        arguments.unit_id,
        arguments.lock_directory,
    )?;
    // A socket file left by an earlier run is not a listener: replace it.
    match std::fs::remove_file(&arguments.mood_socket) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let moods = UnixDatagram::bind(&arguments.mood_socket)?;
    moods.set_nonblocking(true)?;
    // `--caller-caps on` (#3504, phase 0): each datagram comes with who sent
    // it, from the kernel, and `lampos` is the user the parts run as.
    let caller_caps = arguments.caller_caps;
    let lampos = caller_caps
        .then(|| lamp_ipc::caller::uid_named("lampos"))
        .flatten();
    if caller_caps {
        lamp_ipc::caller::pass_credentials(&moods)?;
        eprintln!(
            "lamp-body: callers capped by their unit (--caller-caps on), lampos {}",
            lampos.map_or(
                "unknown: every lampos part asks at idle".into(),
                |uid| format!("uid {uid}")
            )
        );
    }
    let cuts_said = std::cell::RefCell::new(Minutely::default());
    // `--lease-socket` (#3504, phase 0): lamp-control may hold the body,
    // and the lamp's own life stands back while it does. What ends or
    // suspends it is read from the last step (the mood, a hand) and the
    // bus (a pause, a sleep, the person's privacy).
    let mut lease_door = match &arguments.lease_socket {
        Some(path) => {
            let lampos = lamp_ipc::caller::uid_named("lampos");
            eprintln!(
                "lamp-body: the body may be held by lamp-control at {} (lampos {lampos:?})",
                path.display()
            );
            Some(LeaseDoor::bind(
                path,
                Box::new(move |caller| holder_may(caller, lampos)),
            )?)
        }
        None => None,
    };
    let mut lease_mute = lease_door
        .as_ref()
        .and_then(|_| lamp_bus::Bus::from_environment().ok())
        .map(lamp_body::sleep_in::SleepIn::on);
    if lease_door.is_some() && lease_mute.is_none() {
        eprintln!(
            "lamp-body: no bus: the person's privacy mute won't end a lease (a hand, a pause, a sleep and a lapse still do)"
        );
    }
    let lease_mood = std::cell::Cell::new(lamp_body::gesture::Mood::Resting);
    let lease_hand = std::cell::Cell::new(false);
    let lease_slumped = std::cell::Cell::new(false);
    let moves = moves_of(arguments.moves.as_deref());
    // `--hold-from` (#2668): a hand on the head from a test input, for
    // tests on a lamp only; what it says this step, for the journal.
    let mut hold = arguments
        .hold_from
        .as_deref()
        .map(lamp_body::hold_in::HoldIn::from_file);
    if let Some(hold) = &hold {
        eprintln!(
            "lamp-body: hold from a test input: {} (--hold-from, a test flag no product unit passes)",
            hold.path().display()
        );
    }
    let test_held = std::cell::Cell::new(false);
    let old_mask = SigSet::thread_get_mask()?;
    let mut mask = SigSet::empty();
    mask.add(Signal::SIGINT);
    mask.add(Signal::SIGTERM);
    mask.thread_block()?;
    let restore = SignalMask(old_mask);
    let signal_fd = SignalFd::with_flags(&mask, SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK)?;
    let mut cancelled = false;
    // A gesture asked for on the same socket, and at what priority, until
    // the body takes it; and who asked for the last one, for the journal.
    let gesture_asked = std::cell::Cell::new(None);
    let asked_by: std::cell::RefCell<Option<(&'static str, String)>> = Default::default();
    // What was dropped as it was read, two asks in one step, to be told on
    // `body/gesture` with the step's news.
    let intake_told: std::cell::RefCell<Vec<lamp_body::bus_out::ToldNews>> = Default::default();
    let arbitration = arguments.arbitration;
    // Paused by the keeper (`lamp/pause`): it rests and takes nothing new
    // until resumed. Looked at with the moods, once a step.
    let mut pause = lamp_bus::Bus::from_environment()
        .ok()
        .map(lamp_body::pause_in::PauseIn::on);
    let paused = std::cell::Cell::new(false);
    // Put to sleep by a hand on its head (`voice/mute`, #1362): it folds
    // now and takes nothing new until woken. Only with `LAMP_HEAD_TOUCH=on`;
    // looked at with the moods.
    let mut sleep = lamp_body::sleep_in::enabled_from_environment()
        .then(|| lamp_bus::Bus::from_environment().ok())
        .flatten()
        .map(lamp_body::sleep_in::SleepIn::on);
    let asleep = std::cell::Cell::new(false);
    // Open again after that sleep, told once: it stands at once.
    let woken = std::cell::Cell::new(false);
    let mut unknown_said = Hush::default();
    // A skill's move sent whole, played as a gesture (docs/body.md).
    let mut moves_sent = lamp_body::moves::Sent::besides(&moves);
    let mut datagram = vec![0u8; lamp_body::moves::MOST_DATAGRAM];
    // When a skill last said `reading` (`of_this_step`).
    let mut reading_at: Option<std::time::Instant> = None;
    // Where the newest `reading` turns the lamp, if anywhere (#1679).
    let reading_toward: std::cell::Cell<Option<f32>> = std::cell::Cell::new(None);
    let news = || {
        paused.set(pause.as_mut().is_some_and(|pause| pause.paused()));
        // Everything waiting; which of it counts is `of_this_step`'s.
        let mut moods_heard = Vec::new();
        for _ in 0..32 {
            let received = if caller_caps {
                lamp_ipc::caller::recv_with_caller(&moods, &mut datagram)
            } else {
                moods.recv(&mut datagram).map(|length| (length, None))
            };
            match received {
                Ok((length, caller)) => {
                    let sent = &datagram[..length];
                    match heard_asked(sent, &moves, &mut moves_sent, arbitration) {
                        (Heard::Gesture(clip), asker) => {
                            let asker = if caller_caps {
                                let (asker, cut) =
                                    held_to(asker, caller.as_ref(), lampos, arbitration);
                                if let Some((who, asked, held)) = cut
                                    && cuts_said.borrow_mut().due(&who, std::time::Instant::now())
                                {
                                    eprintln!(
                                        "lamp-body: {who} asked at {}, held to {} (--caller-caps; said once a minute)",
                                        format!("{asked:?}").to_lowercase(),
                                        format!("{held:?}").to_lowercase()
                                    );
                                }
                                asker
                            } else {
                                asker
                            };
                            let asked = (clip, asker.priority());
                            let source = asker
                                .source
                                .map(|source| source.chars().take(64).collect::<String>());
                            let held = gesture_asked.get();
                            let (kept, dropped) = kept_of(held, asked, arbitration);
                            gesture_asked.set(Some(kept));
                            if let Some((name, why)) = dropped {
                                // The one read before, with its asker when it
                                // named one and it is still that gesture.
                                let before = asked_by.borrow().clone().filter(|(name, _)| {
                                    held.is_some_and(|(clip, _)| clip.name == *name)
                                });
                                let (whose, by) = intake_dropped(
                                    kept == asked,
                                    source.as_deref(),
                                    before.as_ref().map(|(name, asker)| (*name, asker.as_str())),
                                );
                                intake_told.borrow_mut().push((
                                    (name, lamp_msgs::GestureState::Dropped, Some(why)),
                                    lamp_body::body::Told { source: whose, by },
                                ));
                                let by = match (&source, &*asked_by.borrow()) {
                                    (Some(source), _) if name == clip.name => {
                                        format!(" (asked by {source})")
                                    }
                                    (_, Some((held, source))) if *held == name => {
                                        format!(" (asked by {source})")
                                    }
                                    _ => String::new(),
                                };
                                let line = format!("lamp-body: gesture {name} {why}{by}");
                                if let Some(line) =
                                    unknown_said.due(line, std::time::Instant::now())
                                {
                                    eprintln!("{line}");
                                }
                            }
                            // ponytail: only the last asker kept is known; a
                            // fate said after another ask names no source.
                            if kept == asked {
                                *asked_by.borrow_mut() = source.map(|source| (clip.name, source));
                            }
                        }
                        (Heard::Mood(mood, level), _) => {
                            if mood == lamp_body::gesture::Mood::Reading {
                                reading_toward.set(toward_of(sent));
                            }
                            moods_heard.push((mood, level));
                        }
                        // An unknown gesture, most often: said, as the
                        // body says what it does with every gesture.
                        (Heard::Dropped(why), _) => {
                            let line = format!("lamp-body: dropped from the mood socket, {why}");
                            if let Some(line) = unknown_said.due(line, std::time::Instant::now()) {
                                eprintln!("{line}");
                            }
                        }
                    }
                }
                // Too long for the buffer: refused whole, and the rest read.
                Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                    let line = "lamp-body: dropped from the mood socket, a datagram longer than 16 KiB, not read".to_owned();
                    if let Some(line) = unknown_said.due(line, std::time::Instant::now()) {
                        eprintln!("{line}");
                    }
                }
                Err(_) => break,
            }
        }
        let reading = reading_at.is_some_and(|at| at.elapsed() < lamp_body::body::MOOD_STALE);
        let latest = of_this_step(&moods_heard, reading);
        if latest.is_some_and(|(mood, _)| mood == lamp_body::gesture::Mood::Reading) {
            reading_at = Some(std::time::Instant::now());
        }
        if let Some(sleep) = sleep.as_mut() {
            // The voice speaking to a body taken to be asleep has likely
            // woken: looked at again now, so the lift does not wait.
            if asleep.get()
                && latest.is_some_and(|(mood, _)| mood != lamp_body::gesture::Mood::Resting)
            {
                sleep.look_now();
            }
            asleep.set(sleep.asleep());
            woken.set(woken.get() || sleep.woken());
        }
        if paused.get() || asleep.get() || unplug.as_ref().is_some_and(|unplug| unplug.latched()) {
            return Some((lamp_body::gesture::Mood::Resting, 0));
        }
        latest
    };
    // What is not chosen stays as the folded limits have it.
    let folded = Limits::FOLDED;
    let limits = Limits::folded_with(Tuning {
        reach: arguments.reach.unwrap_or([
            folded.pitch_up as u16,
            folded.pitch_down as u16,
            folded.roll as u16,
            folded.yaw as u16,
        ]),
        speed: arguments.speed.unwrap_or(folded.speed as u16),
        servo_speed: arguments.servo_speed.unwrap_or(folded.servo_speed),
        servo_acceleration: arguments
            .servo_acceleration
            .unwrap_or(folded.servo_acceleration),
    })?;
    // The head's tilt, for a lamp that has stood up.
    let limits = Limits {
        stand_roll: arguments.stand_roll.map_or(folded.stand_roll, f32::from),
        force: arguments.force_limit,
        torque_on: arguments.torque_on,
        wake: arguments.wake,
        give_way: arguments.give_way.map(|choice| match choice {
            GiveWay::Return => lamp_body::body::GiveWay::Return,
            GiveWay::Stay => lamp_body::body::GiveWay::Stay,
        }),
        limp: arguments.limp,
        overload_release: arguments.overload_release,
        let_go_at_stop: arguments.let_go_at_stop,
        let_go_folded_arm: arguments.let_go_folded_arm,
        refused_let_go: arguments.refused_let_go,
        rest_glance: arguments.rest_glance,
        pitch_window: arguments.pitch_window.unwrap_or(folded.pitch_window),
        look_at_name: arguments.look_at_name,
        arbitration: arguments.arbitration,
        gesture_pause: lamp_body::body::gesture_pause_from(
            std::env::var(lamp_body::body::GESTURE_PAUSE)
                .ok()
                .as_deref(),
        ),
        own_ranges: lamp_body::body::own_ranges_from(
            std::env::var(lamp_body::body::OWN_RANGES).ok().as_deref(),
        ),
        ..limits
    };
    // One line to the journal on each change of mood, and twice a second
    // while the goals are changing: what it did, afterwards. At rest the
    // goals never stop changing (it breathes and glances), so there it is
    // a line every thirty seconds: on lamp-4ace on 2026-10-15 the resting
    // body wrote two lines a second, 92% of the journal, and the 47 MB
    // RAM log held only the last 25 minutes.
    let mut said: Option<(lamp_body::gesture::Mood, u64, [u16; 3], [u16; 2])> = None;
    let asked_to_stand = arguments.standing.is_some();
    let mut told_whether_it_stands = false;
    let mut face_said = 0u64;
    let mut gesture_said = lamp_body::bus_out::GestureSaid::default();
    let mut fold_said = lamp_body::bus_out::FoldSaid::default();
    let mut reads_said = lamp_body::bus_out::ReadsSaid::default();
    let mut tipping_said = lamp_body::body::Tipping::default();
    let mut looks_said: Option<&'static str> = None;
    // The place last said refused, so that a refused look is said once.
    let mut refused_said: Option<f32> = None;
    let mut fate_said = Hush::default();
    let mut eased = [0u16; 3];
    let mut handled = ([0u16; 3], [0u16; 5]);
    let mut limp = false;
    // Where the joints are and how the body is, for the rest of the lamp,
    // if the unit says so. Offered and never waited for.
    let mut bus = lamp_body::bus_out::BusOut::from_environment(|why| eprintln!("lamp-body: {why}"));
    let notify = lamp_keeper::Notify::from_environment();
    let ready = std::cell::Cell::new(false);
    // What the latched body's evidence last said, and when it was journalled.
    let mut unplug_said: Option<(&str, std::time::Instant)> = None;
    // Whether the journal last said the test input holds the head.
    let mut hold_said = false;
    // What the body last asked of a person while let go (#2602).
    let mut waiting_said: Option<&str> = None;
    let watch = |glimpse: &lamp_body::body::Glimpse| {
        lease_mood.set(glimpse.mood);
        lease_hand.set(glimpse.hand);
        lease_slumped.set(slumped_now(glimpse));
        step_said(&notify, &ready);
        if glimpse.waiting != waiting_said {
            waiting_said = glimpse.waiting;
            match waiting_said {
                Some(say) => eprintln!(
                    "lamp-body: step {} let go of what its weight does not pull (a head tipped up stays held), waiting to be set straight: {say}",
                    glimpse.step
                ),
                None => eprintln!(
                    "lamp-body: step {} set straight: taken hold where it is, starting",
                    glimpse.step
                ),
            }
        }
        if test_held.get() != hold_said {
            hold_said = test_held.get();
            eprintln!(
                "lamp-body: step {} hold from a test input: {}",
                glimpse.step,
                if hold_said {
                    "the head's pitch held by a hand"
                } else {
                    "the hand lets go"
                }
            );
        }
        if let Some(unplug) = &unplug {
            // A read missed or bad is no evidence either way: nothing is sampled.
            if !glimpse.unplug_gap {
                unplug.sample(glimpse.unplug_safe, glimpse.unplug_unsafe);
            }
            if unplug.latched() {
                let now = std::time::Instant::now();
                let why = match (glimpse.unplug_unsafe, glimpse.unplug_gap) {
                    (Some(at), _) => lamp_body::unplug::UNPLUG_UNSAFE[usize::from(at)],
                    (None, true) => "no evidence: a read missed or bad, or a goal unsent",
                    (None, false) => "folded evidence",
                };
                // An unsafe step is said at once; a change between gaps and
                // evidence at most once a second; the same again every 5 s.
                let due = unplug_said.is_none_or(|(said, at)| {
                    let since = now.duration_since(at);
                    (said != why
                        && (glimpse.unplug_unsafe.is_some()
                            || since >= std::time::Duration::from_secs(1)))
                        || since >= std::time::Duration::from_secs(5)
                });
                if due {
                    eprintln!(
                        "lamp-body: unplug step {}: {why}; head/base {:?} arm {:?} loads {:?}/{:?} temperatures {:?}/{:?} bad reads {:?}",
                        glimpse.step,
                        glimpse.positions,
                        glimpse.arm_positions,
                        glimpse.loads,
                        glimpse.arm_loads,
                        glimpse.temperatures,
                        glimpse.arm_temperatures,
                        glimpse.bad_reads,
                    );
                    unplug_said = Some((why, now));
                }
            }
        }
        if let Some(bus) = bus.as_mut() {
            bus.paused = paused.get();
            bus.offer(glimpse);
        }
        let due = match said {
            None => true,
            Some((mood, _, _, _)) if mood != glimpse.mood => true,
            // While it stands, a line every ten seconds whatever happens:
            // how warm the two joints that carry it are getting.
            Some((_, step, _, _)) if glimpse.stand > 0.0 && glimpse.step - step >= 500 => true,
            Some((_, step, goals, arm)) => {
                let pace = if glimpse.mood == lamp_body::gesture::Mood::Resting {
                    1500
                } else {
                    25
                };
                (goals != glimpse.goals || arm != glimpse.arm_goals) && glimpse.step - step >= pace
            }
        };
        if glimpse.limp != limp {
            limp = glimpse.limp;
            eprintln!(
                "lamp-body: step {} {}; positions {:?} loads {:?} arm positions {:?} loads {:?} warmth {:?} {:?}",
                glimpse.step,
                if limp {
                    format!(
                        "let go under overload or a hand (time {}): torque off on all five for 5 s after a hand or 120 s after overload, or for good if overload trips it twice within ten minutes",
                        glimpse.released
                    )
                } else {
                    "took hold again after letting go, and brings the head home slowly".to_owned()
                },
                glimpse.positions,
                glimpse.loads,
                glimpse.arm_positions,
                glimpse.arm_loads,
                glimpse.temperatures,
                glimpse.arm_temperatures
            );
        }
        if glimpse.eased != eased {
            eased = glimpse.eased;
            eprintln!(
                "lamp-body: step {} met resistance and stopped pushing; so far {:?}; positions {:?} loads {:?}",
                glimpse.step, glimpse.eased, glimpse.positions, glimpse.loads
            );
        }
        if (glimpse.kept_where_put, glimpse.stalls) != handled {
            handled = (glimpse.kept_where_put, glimpse.stalls);
            eprintln!(
                "lamp-body: step {} left where a hand put it {:?}; stopped pushing without moving {:?}; positions {:?} loads {:?} arm positions {:?} loads {:?}",
                glimpse.step,
                glimpse.kept_where_put,
                glimpse.stalls,
                glimpse.positions,
                glimpse.loads,
                glimpse.arm_positions,
                glimpse.arm_loads
            );
        }
        // V1's anti-tip rule acting (src/cog.rs), each time it does.
        let tipping = glimpse.tipping;
        if tipping.cuts != tipping_said.cuts {
            eprintln!(
                "lamp-body: step {} anti-tip: cut a move short that would lean the lamp back {:.1} mm, over its {:.0} mm (V1's rule; {} so far)",
                glimpse.step, tipping.would_mm, tipping.ceiling_mm, tipping.cuts
            );
        }
        if tipping.refused != tipping_said.refused
            && let Some((name, frame, mm)) = tipping.last_refused
        {
            eprintln!(
                "lamp-body: step {} anti-tip: REFUSED gesture {name}: frame {frame} takes the centre of gravity {mm:.1} mm off the base's axis, over V1's 22 mm ({} so far)",
                glimpse.step, tipping.refused
            );
        }
        if tipping.close != tipping_said.close
            && let Some((name, frame, mm)) = tipping.last_close
        {
            eprintln!(
                "lamp-body: step {} anti-tip: gesture {name} plays close to the limit: frame {frame} at {mm:.1} mm of 22",
                glimpse.step
            );
        }
        tipping_said = tipping;
        // Each gesture asked from outside, taken or dropped, and why.
        // Under --arbitration on, with who asked for it when it said. A
        // drop is also said on `body/gesture`, each time (the journal says
        // the same line once in five seconds).
        let source = |name: &str| match &*asked_by.borrow() {
            Some((asked, source)) if *asked == name => format!(" (asked by {source})"),
            _ => String::new(),
        };
        // A hand's end or drop while the test input holds the head is
        // named for it, so a lamp's result can say where the hand came from.
        let from_test = |why: &str| {
            if test_held.get() && why.contains("hand") {
                " (the hand from a test input, --hold-from)"
            } else {
                ""
            }
        };
        for (name, why) in glimpse.gesture_fates {
            if let Some(line) = fate_said.due(
                format!("gesture {name} {why}{}{}", source(name), from_test(why)),
                std::time::Instant::now(),
            ) {
                eprintln!("lamp-body: step {} {line}", glimpse.step);
            }
        }
        let mut gestures: Vec<_> = intake_told.borrow_mut().drain(..).collect();
        gestures.extend(gesture_said.step_told(glimpse));
        // Whether a face in view is looked at, when that changes: a skill's
        // look and the eyes' come on one socket and read the same.
        if (glimpse.face.is_some() || paused.get()) && glimpse.looks_dropped != looks_said {
            looks_said = glimpse.looks_dropped;
            match looks_said {
                Some(why) => eprintln!(
                    "lamp-body: step {} a face is not looked at: {why}",
                    glimpse.step
                ),
                None => eprintln!("lamp-body: step {} a face is looked at", glimpse.step),
            }
        }
        // A search's look kept off the side a face is on (#1738), once per look.
        if glimpse.toward_refused != refused_said {
            refused_said = glimpse.toward_refused;
            if let Some(toward) = refused_said {
                eprintln!(
                    "lamp-body: step {} not turned toward {toward:.2}: a face is seen on that side; the head stays over the desk",
                    glimpse.step
                );
            }
        }
        lamp_body::bus_out::tell_told(&gestures, glimpse.step, bus.as_mut(), arbitration, |line| {
            eprintln!("{line}");
        });
        // A fold's end, and the reads not used so far every five minutes,
        // so a run can be read without stopping it (#705).
        for line in [fold_said.step(glimpse), reads_said.step(glimpse)]
            .into_iter()
            .flatten()
        {
            eprintln!("{line}");
        }
        // What the eyes report and where that has it look, at most every
        // five seconds: for setting the two numbers of --gaze on a lamp.
        // Twice a second filled the journal on a day of faces at desks.
        if let Some(face) = glimpse.face
            && glimpse.step - face_said >= 250
        {
            face_said = glimpse.step;
            eprintln!(
                "lamp-body: step {} {:?} face at {:.2} across {:.2} down, size {:.2}; looking {:?}{}",
                glimpse.step,
                glimpse.mood,
                face[0],
                face[1],
                face[2],
                glimpse.look,
                glimpse
                    .looks_dropped
                    .map_or(String::new(), |why| format!(" (not looked at: {why})"))
            );
        }
        if due {
            said = Some((glimpse.mood, glimpse.step, glimpse.goals, glimpse.arm_goals));
            if asked_to_stand && !told_whether_it_stands {
                told_whether_it_stands = true;
                eprintln!(
                    "lamp-body: {}",
                    if glimpse.stands {
                        "it may stand"
                    } else {
                        "it will not stand this run; the report at its end says why"
                    }
                );
            }
            if glimpse.stands {
                eprintln!(
                    "lamp-body: step {} {:?} goals {:?} positions {:?} loads {:?} stand {:.2} arm goals {:?} positions {:?} loads {:?} warmth {:?}",
                    glimpse.step,
                    glimpse.mood,
                    glimpse.goals,
                    glimpse.positions,
                    glimpse.loads,
                    glimpse.stand,
                    glimpse.arm_goals,
                    glimpse.arm_positions,
                    glimpse.arm_loads,
                    glimpse.arm_temperatures
                );
            } else {
                eprintln!(
                    "lamp-body: step {} {:?} goals {:?} positions {:?} loads {:?}",
                    glimpse.step, glimpse.mood, glimpse.goals, glimpse.positions, glimpse.loads
                );
            }
        }
    };
    if arguments.unmeasured_gaze_socket.is_some() {
        eprintln!(
            "lamp-body: gaze off: --gaze-socket without --gaze, which is measured with a face (install.md); the eyes are not read"
        );
    }
    // The eyes' socket, for a body that looks. A file left by an earlier
    // run is not a listener: replace it.
    let (looking, sights) = match &arguments.gaze {
        Some(choice) => {
            match std::fs::remove_file(&choice.socket) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let sights = UnixDatagram::bind(&choice.socket)?;
            sights.set_nonblocking(true)?;
            if caller_caps {
                lamp_ipc::caller::pass_credentials(&sights)?;
            }
            (
                Some(lamp_body::gaze::Looking::new(choice.turn, choice.reach)?),
                Some(sights),
            )
        }
        None => (None, None),
    };
    let sights_said = std::cell::RefCell::new(Minutely::default());
    let eyes = || {
        // Everything waiting, and only the newest of it counts.
        let sights = sights.as_ref()?;
        let mut latest = None;
        let mut datagram = [0u8; 160];
        for _ in 0..32 {
            let received = if caller_caps {
                lamp_ipc::caller::recv_with_caller(sights, &mut datagram)
            } else {
                sights.recv(&mut datagram).map(|length| (length, None))
            };
            match received {
                Ok((length, caller)) => {
                    if caller_caps && !sight_from(caller.as_ref(), lampos) {
                        let who = caller
                            .as_ref()
                            .and_then(|caller| caller.unit.clone())
                            .unwrap_or_else(|| "unknown".into());
                        if sights_said
                            .borrow_mut()
                            .due(&who, std::time::Instant::now())
                        {
                            eprintln!(
                                "lamp-body: a sight from {who} dropped: only the eyes, a skill or root may send one (--caller-caps; said once a minute)"
                            );
                        }
                        continue;
                    }
                    latest = serde_json::from_slice::<lamp_body::gaze::Seen>(&datagram[..length])
                        .ok()
                        .or(latest);
                }
                Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {}
                Err(_) => break,
            }
        }
        latest
    };
    let standing = arguments
        .standing
        .map(|choice| {
            lamp_body::body::Standing::new(
                choice.folded,
                choice.pose,
                choice.share,
                choice.seconds,
                choice.awake,
                choice.load,
            )
            .map(|standing| {
                // Each run draws its own waits and moves.
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_nanos() as u64);
                standing.idling(choice.idle).seeded(seed)
            })
        })
        .transpose()?;
    // What Expression asks of the head, if the unit says so. Looked at
    // each step and never waited for (docs/expression.md).
    let mut expression = lamp_body::expression_in::ExpressionIn::from_environment(|why| {
        eprintln!("lamp-body: {why}")
    });
    // The lamp's name, heard, if it is to turn to a call.
    let mut name = if arguments.look_at_name {
        match lamp_bus::Bus::from_environment().and_then(|bus| lamp_body::name_in::NameIn::on(&bus))
        {
            Ok(name) => {
                eprintln!("lamp-body: it turns to a call, from name/heard");
                Some(name)
            }
            Err(error) => {
                eprintln!("lamp-body: no call is taken from name/heard: {error}");
                None
            }
        }
    } else {
        None
    };
    let result = run_body(
        &config,
        limits,
        arguments.home,
        standing,
        looking,
        news,
        || {
            let now_us = lamp_ipc::monotonic_us();
            let (gesture, priority) = gesture_asked.take().map_or(
                (None, lamp_body::moves::Priority::Conversation),
                |(clip, priority)| (Some(clip), priority),
            );
            let asked = lamp_body::body::Asked {
                seen: eyes(),
                gesture,
                expression: expression
                    .as_mut()
                    .and_then(|asked| asked.offsets(now_us, |why| eprintln!("lamp-body: {why}"))),
                called: name.as_mut().is_some_and(|name| name.called()),
                still: expression
                    .as_ref()
                    .is_some_and(|asked| asked.holding(now_us)),
                paused: false,
                asleep: false,
                woken: woken.take(),
                // Taken only while the pose is reading (`body.rs`).
                toward: reading_toward.get(),
                unplug: false,
                held_by_test: false,
                held: false,
            };
            // Read all the same, so that nothing kept from before the pause
            // is acted on after it.
            // A gesture is passed on to be dropped and said.
            let asked = if paused.get() || unplug.as_ref().is_some_and(|unplug| unplug.latched()) {
                lamp_body::body::Asked {
                    gesture: asked.gesture,
                    unplug: unplug.as_ref().is_some_and(|unplug| unplug.latched()),
                    ..lamp_body::body::Asked::PAUSED
                }
            } else if asleep.get() {
                // Asleep, the head still takes Expression's sleeping breath.
                lamp_body::body::Asked {
                    gesture: asked.gesture,
                    expression: asked.expression,
                    ..lamp_body::body::Asked::ASLEEP
                }
            } else {
                asked
            };
            // A hand from the test input holds through a pause or a sleep,
            // as a real one would.
            let held = hold
                .as_mut()
                .is_some_and(|hold| hold.held(std::time::Instant::now()));
            test_held.set(held);
            let asked = lamp_body::body::Asked {
                held_by_test: held,
                held: false,
                ..asked
            };
            // The name the kept gesture's asker gave, under arbitration
            // only (`heard_asked` keeps none without it), to tell an asker
            // that lost the body and to whom (#940).
            let source = asked.gesture.and_then(|clip| {
                asked_by
                    .borrow()
                    .as_ref()
                    .filter(|(name, _)| *name == clip.name)
                    .and_then(|(_, source)| lamp_body::moves::Source::new(source))
            });
            // Held by lamp-control: no shift at rest, nothing from
            // Expression, until it lets go, lapses or loses the body.
            let asked = match lease_door.as_mut() {
                Some(door) => {
                    use lamp_body::gesture::Mood;
                    let private = lease_mute.as_mut().is_some_and(|mute| {
                        mute.asleep();
                        mute.private()
                    });
                    let around = lamp_body::lease::Around {
                        conversation: matches!(
                            lease_mood.get(),
                            Mood::Listening | Mood::Thinking | Mood::Speaking
                        ),
                        hand: lease_hand.get() || held,
                        slumped: lease_slumped.get(),
                        private,
                        paused: paused.get(),
                        asleep: asleep.get(),
                    };
                    if door.turn(around, std::time::Instant::now()) {
                        lamp_body::body::Asked {
                            still: true,
                            expression: None,
                            held: true,
                            ..asked
                        }
                    } else {
                        asked
                    }
                }
                None => asked,
            };
            (asked, priority, source)
        },
        watch,
        || unplug.as_ref().is_some_and(|unplug| unplug.latched()),
        || {
            if !cancelled {
                cancelled = signal_fd
                    .read_signal()
                    .map_err(|error| std::io::Error::from_raw_os_error(error as i32))?
                    .is_some();
                if cancelled {
                    // It eases back from here.
                    notify.stopping();
                }
            }
            Ok(cancelled)
        },
    );
    if let Some(unplug) = &unplug {
        unplug.stop();
    }
    // Again for a body that stopped by itself; a second time is harmless.
    notify.stopping();
    // A holder is told why its lease ended with the run, when it was the
    // body that gave out.
    if let Some(door) = lease_door.as_mut() {
        let why = result.body.as_ref().and_then(|body| match body.stop {
            lamp_body::body::Stop::Overload { .. } => Some("slumped"),
            _ => None,
        });
        door.ended(why);
    }
    if let Some(woke) = result.body.as_ref().and_then(|body| body.woke.as_ref()) {
        eprintln!(
            "lamp-body: woke {:?} from {:?} to {:?} in {:.1} s",
            woke.joints, woke.from, woke.to, woke.seconds
        );
    }
    for released in result.body.iter().flat_map(|body| &body.released) {
        eprintln!(
            "lamp-body: let go under overload: {} read {} at {:.1} s",
            released.joint, released.load, released.seconds
        );
    }
    let _ = std::fs::remove_file(&arguments.mood_socket);
    if let Some(choice) = &arguments.gaze {
        let _ = std::fs::remove_file(&choice.socket);
    }
    // The gesture it was making ends with the run: said `over`, on the
    // journal and the bus, before the body's last word.
    let stopped_making = gesture_said.stopped();
    let last_step = result.body.as_ref().map_or(0, |body| body.steps);
    lamp_body::bus_out::tell(&stopped_making, last_step, bus.as_mut(), |line| {
        eprintln!("{line}, as the run stopped");
    });
    // The body's last word on the bus: that it has stopped, and why.
    if let Some(bus) = bus {
        if bus.let_go > 0 || bus.gestures_let_go > 0 {
            eprintln!(
                "lamp-body: {} steps and {} body/gesture messages were not published: the bus thread had no room",
                bus.let_go, bus.gestures_let_go
            );
        }
        match &result.body {
            Some(report) => bus.stopped(&report.stop),
            None => bus.stopped(&Stop::Refused {
                why: "the servo port could not be made ready".into(),
            }),
        }
    }
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, &result)?;
    writeln!(output)?;
    output.flush()?;
    restore.0.thread_set_mask()?;
    Ok(result.port.preparation_error.is_none()
        && result.port.cleanup.errors.is_empty()
        && result.body.is_some_and(|body| body.stop == Stop::Cancelled))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_body::gesture::Mood;

    #[test]
    fn each_step_says_alive_and_the_first_says_ready() {
        use std::os::unix::net::UnixDatagram;
        let directory =
            std::env::temp_dir().join(format!("lamp-body-notify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("notify");
        let ear = UnixDatagram::bind(&path).unwrap();
        ear.set_nonblocking(true).unwrap();
        let notify = lamp_keeper::Notify::new(
            Some(path.to_str().unwrap().to_owned()),
            Some(std::time::Duration::from_secs(10)),
        );
        let ready = std::cell::Cell::new(false);
        for _ in 0..3 {
            step_said(&notify, &ready);
        }
        let mut heard = Vec::new();
        let mut buffer = [0u8; 64];
        while let Ok(length) = ear.recv(&mut buffer) {
            heard.push(String::from_utf8_lossy(&buffer[..length]).into_owned());
        }
        // Three steps within one watchdog interval: alive is sent once.
        assert_eq!(heard, ["READY=1", "WATCHDOG=1"]);
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    /// #1680, the first builder on #2071: a newer `lamp-body-measure` may
    /// write a pose option this build does not read. The body stays strict
    /// on it (it may be a bound the arm needs), and says which option and
    /// why; the poses it knows still read; an unknown word, a bad value or a
    /// known option twice is refused as before.
    #[test]
    fn a_newer_poses_files_option_still_stops_the_body() {
        let poses = "--home 1772,2057,2034 --shoulder-trim 12 --torque on";
        let line: Vec<&str> =
            [&FULL[..], &poses.split_whitespace().collect::<Vec<_>>()[..]].concat();
        assert_eq!(
            parse_with(args(&line), Some(poses)).err(),
            Some("an option in the lamp's poses file this build does not read")
        );
        assert_eq!(
            parse_with(args(&line), None).err(),
            Some("unknown or duplicate option")
        );
        // The poses it knows read as before.
        let known = "--home 1772,2057,2034 --torque on";
        let line: Vec<&str> =
            [&FULL[..], &known.split_whitespace().collect::<Vec<_>>()[..]].concat();
        let read = parse_with(args(&line), Some(known)).unwrap().unwrap();
        assert_eq!(read.home, Some([1772, 2057, 2034]));
        let typo = [&FULL[..], &["--wakee", "on"]].concat();
        assert!(parse_with(args(&typo), Some(known)).is_err());
        let line = [&FULL[..], &["--home", "1,2"]].concat();
        assert!(parse_with(args(&line), Some("--home 1,2")).is_err());
        let twice = "--home 1772,2057,2034 --home 1772,2057,2034";
        let line = [
            &FULL[..],
            &["--home", "1772,2057,2034", "--home", "1772,2057,2034"],
        ]
        .concat();
        assert!(parse_with(args(&line), Some(twice)).is_err());
    }

    /// Every option the usage names is one this build reads, so a known
    /// option is never taken for a newer one.
    #[test]
    fn every_option_the_usage_names_is_known() {
        let synopsis = USAGE
            .split("lamp-body --device")
            .nth(1)
            .and_then(|rest| rest.split("\n\n").next())
            .unwrap();
        let named: Vec<&str> = format!("--device{synopsis}")
            .split(|c: char| !(c.is_ascii_lowercase() || c == '-'))
            .filter(|word| word.starts_with("--") && word.len() > 2)
            .map(|word| {
                OPTIONS
                    .iter()
                    .find(|known| **known == word)
                    .copied()
                    .unwrap_or_else(|| panic!("{word} not in OPTIONS"))
            })
            .collect();
        for option in OPTIONS {
            assert!(named.contains(&option), "{option} not in the usage");
        }
    }

    /// `--hold-from` (#2668) is a test input: read as an absolute file,
    /// off unless given.
    #[test]
    fn hold_from_is_an_absolute_file_and_off_unless_given() {
        let off = parse(args(&FULL)).unwrap().unwrap();
        assert_eq!(off.hold_from, None);
        let line = [&FULL[..], &["--hold-from", "/run/lamp-test/hold"]].concat();
        let on = parse(args(&line)).unwrap().unwrap();
        assert_eq!(on.hold_from, Some(PathBuf::from("/run/lamp-test/hold")));
        for bad in ["hold", "/run/../hold"] {
            let line = [&FULL[..], &["--hold-from", bad]].concat();
            assert!(parse(args(&line)).is_err(), "{bad}");
        }
    }

    /// No file lampOS deploys (units, their drop-ins, env files and the
    /// scripts that write them) passes the test flag `--hold-from`.
    #[test]
    fn nothing_deployed_passes_hold_from() {
        let deploy = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy");
        let (mut folders, mut units) = (vec![deploy], 0);
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
                        !text.contains("--hold-from"),
                        "{} passes --hold-from",
                        path.display()
                    );
                }
            }
        }
        assert!(units > 0, "no unit found under deploy/");
        for unit in ["units/lamp-body.service", "product/lamp-body.service"] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../deploy")
                .join(unit);
            assert!(path.is_file(), "{unit} moved: look for it");
        }
    }

    const FULL: [&str; 8] = [
        "--device",
        "/dev/example",
        "--unit-id",
        "lamp-test",
        "--lock-directory",
        "/home/test/private",
        "--mood-socket",
        "/run/lampos-body/mood.sock",
    ];

    /// #2163 (Fifth on #2228): a seated lamp.env (lamp-body-measure
    /// --seated) is for the generic package's unit, a V1 lamp moved to V2:
    /// from that unit's own ExecStart it starts a body that may not stand
    /// and holds the head and base. The product unit passes the stand's
    /// own options, so it takes only a standing file (the factory's, after
    /// its watched stand) and refuses a seated one: a product lamp is never
    /// seated. Read from the unit files, so neither can drift from this.
    #[test]
    fn a_seated_poses_line_starts_the_generic_unit_and_never_stands() {
        const SEATED: &str = "--home 1772,2052,2038 --pitch-window 1740,2030 --torque on";
        let start = |unit: &str| {
            let exec = unit
                .lines()
                .find_map(|line| line.strip_prefix("ExecStart="))
                .expect("the unit has an ExecStart");
            let words: Vec<String> = exec
                .split_whitespace()
                .skip(1)
                .flat_map(|word| match word {
                    "@BODY_POSES@" | "$BODY_POSES" => {
                        SEATED.split_whitespace().map(str::to_owned).collect()
                    }
                    "@SERVO_DEVICE@" => vec!["/dev/ttyACM0".to_owned()],
                    "@MOTOR_LOCK_DIR@" => vec!["/var/lib/lampos-locks/motor".to_owned()],
                    "@UNIT_ID@" | "%H" => vec!["lamp-a63e".to_owned()],
                    word => vec![word.to_owned()],
                })
                .collect();
            assert!(
                !words.iter().any(|word| word.contains('@')),
                "a placeholder this test does not fill: {words:?}"
            );
            parse(words.iter().map(OsString::from))
        };
        let generic = start(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/units/lamp-body.service"
        )))
        .expect("the generic unit starts a seated body")
        .unwrap();
        assert!(generic.standing.is_none());
        assert!(generic.torque_on);
        let product = start(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/product/lamp-body.service"
        )));
        assert!(product.is_err(), "a product lamp is never seated");
    }

    #[test]
    fn every_path_is_explicit_and_nothing_else_is_accepted() {
        let value = parse(args(&FULL)).unwrap().unwrap();
        assert_eq!(value.unit_id.as_str(), "lamp-test");
        assert_eq!(
            value.mood_socket,
            PathBuf::from("/run/lampos-body/mood.sock")
        );
        assert_eq!(value.home, None);
        let with = |home: &'static str| parse(args(&[&FULL[..], &["--home", home]].concat()));
        assert_eq!(
            with("1930,2043,2015").unwrap().unwrap().home,
            Some([1_930, 2_043, 2_015])
        );
        let tuned = parse(args(
            &[
                &FULL[..],
                &[
                    "--reach",
                    "120,15,60,150",
                    "--speed",
                    "120",
                    "--servo-speed",
                    "400",
                    "--servo-acceleration",
                    "80",
                ],
            ]
            .concat(),
        ))
        .unwrap()
        .unwrap();
        assert_eq!(
            (
                tuned.reach,
                tuned.speed,
                tuned.servo_speed,
                tuned.servo_acceleration
            ),
            (Some([120, 15, 60, 150]), Some(120), Some(400), Some(80))
        );
        assert!(parse(args(&[&FULL[..], &["--reach", "120,15,60"]].concat())).is_err());
        assert!(
            parse(args(
                &[&FULL[..], &["--servo-acceleration", "256"]].concat()
            ))
            .is_err()
        );
        for refused in [
            "1930,2043",
            "1930,2043,2015,1",
            "1930,2043,4096",
            "1930,,2015",
            "+1930,2043,2015",
            "a,b,c",
        ] {
            assert!(with(refused).is_err(), "{refused}");
        }
        assert!(parse(args(&["--help"])).unwrap().is_none());
        for missing in 0..4 {
            let mut some = FULL.to_vec();
            some.drain(missing * 2..missing * 2 + 2);
            assert!(parse(args(&some)).is_err());
        }
        assert!(parse(args(&[&FULL[..], &["--torque", "1"]].concat())).is_err());
        assert!(
            parse(args(
                &[&FULL[..6], &["--mood-socket", "mood.sock"]].concat()
            ))
            .is_err()
        );
        assert!(
            parse(args(
                &[&FULL[..6], &["--mood-socket", "/run/../etc/x"]].concat()
            ))
            .is_err()
        );
    }

    #[test]
    fn standing_is_a_pose_a_folded_arm_and_a_home_or_nothing() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        let stood = with(&[
            "--home",
            "1950,2043,2015",
            "--stand",
            "1772,2045,2015,2657,2602",
            "--folded-arm",
            "2048,2047",
            "--stand-share",
            "30",
            "--stand-seconds",
            "6",
            "--awake-seconds",
            "45",
        ])
        .unwrap()
        .unwrap()
        .standing
        .unwrap();
        assert_eq!(
            stood,
            StandingChoice {
                folded: [2_048, 2_047],
                pose: [1_772, 2_045, 2_015, 2_657, 2_602],
                share: 30,
                seconds: 6,
                awake: 45,
                load: [950, 950],
                // On unless turned off (docs/joining.md).
                idle: true,
            }
        );
        assert!(with(&[]).unwrap().unwrap().standing.is_none());
        let home = ["--home", "1950,2043,2015"];
        let pose = ["--stand", "1772,2045,2015,2657,2602"];
        let arm = ["--folded-arm", "2048,2047"];
        for refused in [
            // No home, no folded arm, an option without a pose, a pose of four.
            [&pose[..], &arm].concat(),
            [&home[..], &pose].concat(),
            [&home[..], &["--stand-share", "30"]].concat(),
            [&home[..], &["--stand", "1772,2045,2015,2657"], &arm].concat(),
            [&home[..], &pose, &arm, &["--stand-share", "300"]].concat(),
        ] {
            assert!(with(&refused).is_err(), "{refused:?}");
        }
    }

    /// The force limit is a number from 300 to 1,000, off unless given;
    /// and with it the arm's load bound comes down under it.
    #[test]
    fn a_force_limit_is_a_number_in_its_range_and_brings_the_load_bound_under_it() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert_eq!(with(&[]).unwrap().unwrap().force_limit, None);
        assert_eq!(
            with(&["--force-limit", "600"])
                .unwrap()
                .unwrap()
                .force_limit,
            Some(600)
        );
        for refused in [
            &["--force-limit", "299"][..],
            &["--force-limit", "1001"],
            &["--force-limit", "+600"],
            &["--force-limit", "six"],
            &["--force-limit", "600", "--force-limit", "700"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
        let stand = [
            "--home",
            "1950,2043,2015",
            "--stand",
            "1772,2045,2015,2657,2602",
            "--folded-arm",
            "2048,2047",
        ];
        let standing = |more: &[&'static str]| {
            with(&[&stand[..], more].concat()).map(|value| value.unwrap().standing.unwrap().load)
        };
        assert_eq!(standing(&[]), Ok([950, 950]));
        assert_eq!(standing(&["--force-limit", "600"]), Ok([560, 560]));
        // A bound that was asked for is kept, if it is under the limit.
        assert_eq!(
            standing(&["--force-limit", "600", "--arm-load", "500,520"]),
            Ok([500, 520])
        );
        assert!(standing(&["--force-limit", "600", "--arm-load", "500,600"]).is_err());
        assert!(standing(&["--arm-load", "900,950", "--force-limit", "600"]).is_err());
    }

    /// Taking hold of a joint found without torque is off unless asked for.
    #[test]
    fn torque_is_on_or_off_and_off_unless_given() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert!(!with(&[]).unwrap().unwrap().torque_on);
        assert!(with(&["--torque", "on"]).unwrap().unwrap().torque_on);
        assert!(!with(&["--torque", "off"]).unwrap().unwrap().torque_on);
        for refused in [
            &["--torque", "1"][..],
            &["--torque", "ON"],
            &["--torque", ""],
            &["--torque"],
            &["--torque", "on", "--torque", "on"],
            &["--torque", "off", "--torque", "on"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// Waking from wherever the lamp is found is off unless asked for.
    #[test]
    fn wake_is_on_or_off_and_on_unless_turned_off() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        // On unless turned off: confirmed on lamp-4ace (docs/joining.md).
        assert!(with(&[]).unwrap().unwrap().wake);
        assert!(with(&["--wake", "on"]).unwrap().unwrap().wake);
        assert!(!with(&["--wake", "off"]).unwrap().unwrap().wake);
        for refused in [
            &["--wake", "1"][..],
            &["--wake", "ON"],
            &["--wake", ""],
            &["--wake"],
            &["--wake", "on", "--wake", "on"],
            &["--wake", "on", "--help"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// A glance from rest is off unless asked for.
    #[test]
    fn rest_glance_is_on_or_off_and_on_unless_turned_off() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert!(with(&[]).unwrap().unwrap().rest_glance);
        assert!(with(&["--rest-glance", "on"]).unwrap().unwrap().rest_glance);
        assert!(
            !with(&["--rest-glance", "off"])
                .unwrap()
                .unwrap()
                .rest_glance
        );
        for refused in [
            &["--rest-glance", "1"][..],
            &["--rest-glance"],
            &["--rest-glance", "on", "--rest-glance", "on"],
            &["--rest-glance", "on", "--help"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// `--refused-let-go` (#2602): on unless turned off.
    #[test]
    fn refused_let_go_is_on_or_off_and_on_unless_turned_off() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert!(with(&[]).unwrap().unwrap().refused_let_go);
        assert!(
            !with(&["--refused-let-go", "off"])
                .unwrap()
                .unwrap()
                .refused_let_go
        );
        for refused in [
            &["--refused-let-go", "1"][..],
            &["--refused-let-go", "on", "--refused-let-go", "on"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// A lamp's own window for the head beside the base, lamp-4ace's
    /// unless given.
    #[test]
    fn pitch_window_is_low_and_high_and_lamp_4ace_s_unless_given() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert_eq!(with(&[]).unwrap().unwrap().pitch_window, None);
        assert_eq!(
            with(&["--pitch-window", "1670,1995"])
                .unwrap()
                .unwrap()
                .pitch_window,
            Some((1_670, 1_995))
        );
        let home = ["--home", "1771,2035,2035"];
        assert!(with(&[&home[..], &["--pitch-window", "1700,1995"]].concat()).is_ok());
        for refused in [
            &["--pitch-window", "1669,1995"][..],
            &["--pitch-window", "1995,1700"],
            &["--pitch-window", "1700,1700"],
            &["--pitch-window", "1700,4096"],
            &["--pitch-window", "1700"],
            &["--pitch-window", "+1700,1995"],
            &["--pitch-window"],
            &["--pitch-window", "1700,1995", "--pitch-window", "1700,1995"],
            &["--pitch-window", "1700,1995", "--help"],
            &["--home", "1771,2035,2035", "--pitch-window", "1800,1995"],
            &["--home", "1771,2035,2035", "--pitch-window", "1700,1770"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// Giving way is return, stay or off, and return unless given (#703).
    #[test]
    fn give_way_is_return_stay_or_off_and_return_unless_given() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert_eq!(with(&[]).unwrap().unwrap().give_way, Some(GiveWay::Return));
        assert_eq!(
            with(&["--give-way", "off"]).unwrap().unwrap().give_way,
            None
        );
        assert_eq!(
            with(&["--give-way", "return"]).unwrap().unwrap().give_way,
            Some(GiveWay::Return)
        );
        assert_eq!(
            with(&["--give-way", "stay"]).unwrap().unwrap().give_way,
            Some(GiveWay::Stay)
        );
        for refused in [
            &["--give-way", "STAY"][..],
            &["--give-way", "on"],
            &["--give-way", "OFF"],
            &["--give-way", ""],
            &["--give-way"],
            &["--give-way", "stay", "--give-way", "return"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// Going limp under a hand is on or off, on unless given while the body
    /// gives way, and off with `--give-way off`, which then is the body as it
    /// was before both (#703).
    #[test]
    fn limp_is_on_or_off_and_on_unless_given_while_giving_way() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert!(with(&[]).unwrap().unwrap().limp);
        let off = with(&["--give-way", "off"]).unwrap().unwrap();
        assert!(!off.limp && off.give_way.is_none());
        assert!(
            with(&["--give-way", "off", "--limp", "on"]).is_err(),
            "limp needs give-way"
        );
        assert!(
            !with(&["--give-way", "off", "--limp", "off"])
                .unwrap()
                .unwrap()
                .limp
        );
        assert!(with(&["--limp", "on"]).unwrap().unwrap().limp);
        assert!(!with(&["--limp", "off"]).unwrap().unwrap().limp);
        for refused in [
            &["--limp", "ON"][..],
            &["--limp", ""],
            &["--limp"],
            &["--limp", "on", "--limp", "off"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// Letting go at a stop is the factory's: on or off, and off unless given.
    #[test]
    fn let_go_at_stop_is_on_or_off_and_off_unless_given() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert!(!with(&[]).unwrap().unwrap().let_go_at_stop);
        assert!(
            with(&["--let-go-at-stop", "on"])
                .unwrap()
                .unwrap()
                .let_go_at_stop
        );
        assert!(
            !with(&["--let-go-at-stop", "off"])
                .unwrap()
                .unwrap()
                .let_go_at_stop
        );
        for refused in [
            &["--let-go-at-stop", "yes"][..],
            &["--let-go-at-stop"],
            &["--let-go-at-stop", "on", "--let-go-at-stop", "on"],
            &["--let-go-at-stop", "on", "--help"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
        // `--folded-arm` alone is a let-go run's only (#1682).
        let arm = ["--let-go-at-stop", "on", "--folded-arm", "2057,2056"];
        assert_eq!(
            with(&arm).unwrap().unwrap().let_go_folded_arm,
            Some([2_057, 2_056])
        );
        for refused in [
            &["--folded-arm", "2057,2056"][..],
            &["--let-go-at-stop", "off", "--folded-arm", "2057,2056"],
            &[&arm[..], &["--home", "1772,2052,2038"]].concat(),
            &[&arm[..], &["--torque", "on"]].concat(),
            &[&arm[..], &["--arm-load", "900,900"]].concat(),
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// Letting go under overload is `release` or not given, and off unless given.
    #[test]
    fn overload_is_release_and_off_unless_given() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert!(!with(&[]).unwrap().unwrap().overload_release);
        assert!(
            with(&["--overload", "release"])
                .unwrap()
                .unwrap()
                .overload_release
        );
        for refused in [
            &["--overload", "on"][..],
            &["--overload", "off"],
            &["--overload", "RELEASE"],
            &["--overload", ""],
            &["--overload"],
            &["--overload", "release", "--overload", "release"],
            &["--overload", "release", "--help"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// A folder of moves is a short absolute path, and none unless given;
    /// one that cannot be read leaves the body's own gestures.
    #[test]
    fn moves_are_a_folder_given_as_a_short_absolute_path() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert_eq!(with(&[]).unwrap().unwrap().moves, None);
        assert_eq!(
            with(&["--moves", "/var/lib/lampos/moves"])
                .unwrap()
                .unwrap()
                .moves,
            Some(PathBuf::from("/var/lib/lampos/moves"))
        );
        for refused in [
            &["--moves", "moves"][..],
            &["--moves", "/var/../moves"],
            &["--moves", "/a", "--moves", "/b"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
        let unread = moves_of(Some(std::path::Path::new("/nonexistent/lamp-moves")));
        assert!(unread.moves.is_empty());
        assert!(moves_of(None).moves.is_empty());
    }

    /// Turning to a call is on or off, and off unless given.
    #[test]
    fn look_at_name_is_on_or_off_and_on_unless_turned_off() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert!(with(&[]).unwrap().unwrap().look_at_name);
        assert!(
            with(&["--look-at-name", "on"])
                .unwrap()
                .unwrap()
                .look_at_name
        );
        assert!(
            !with(&["--look-at-name", "off"])
                .unwrap()
                .unwrap()
                .look_at_name
        );
        for refused in [
            &["--look-at-name", "ON"][..],
            &["--look-at-name", ""],
            &["--look-at-name"],
            &["--look-at-name", "on", "--look-at-name", "on"],
            &["--look-at-name", "on", "--help"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// Caller caps (#3504, phase 0) are on or off, and off unless given.
    #[test]
    fn caller_caps_are_on_or_off_and_off_unless_given() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert!(!with(&[]).unwrap().unwrap().caller_caps);
        assert!(with(&["--caller-caps", "on"]).unwrap().unwrap().caller_caps);
        assert!(
            !with(&["--caller-caps", "off"])
                .unwrap()
                .unwrap()
                .caller_caps
        );
        for refused in [
            &["--caller-caps", "yes"][..],
            &["--caller-caps"],
            &["--caller-caps", "on", "--caller-caps", "on"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// A gesture's asker is held to its unit's most, whatever it says, and
    /// named by its unit; a cut says who, what was asked and what is kept.
    #[test]
    fn a_gesture_is_held_to_what_its_unit_may_ask() {
        use lamp_body::moves::Priority::*;
        use lamp_ipc::caller::Caller;
        let lampos = Some(990);
        let caller = |unit: &str, uid| Caller {
            pid: 7,
            uid,
            unit: Some(unit.to_owned()),
        };
        let asker = |source: Option<&str>, priority| Asker {
            source: source.map(str::to_owned),
            priority,
        };
        let skills = caller("lamp-skills.service", 990);
        // A skill claiming the person is held to skill, its own name kept.
        let (held, cut) = held_to(
            asker(Some("skill:dance"), Some(Person)),
            Some(&skills),
            lampos,
            true,
        );
        assert_eq!(held, asker(Some("lamp-skills/skill:dance"), Some(Skill)));
        assert_eq!(cut, Some(("lamp-skills".into(), Person, Skill)));
        // At or under its own, nothing is cut.
        let (held, cut) = held_to(asker(None, Some(Idle)), Some(&skills), lampos, true);
        assert_eq!(held, asker(Some("lamp-skills"), Some(Idle)));
        assert_eq!(cut, None);
        // No priority said is conversation, as before, then held.
        let (held, cut) = held_to(asker(None, None), Some(&skills), lampos, true);
        assert_eq!(held.priority, Some(Skill));
        assert_eq!(cut, Some(("lamp-skills".into(), Conversation, Skill)));
        // The voice, as root, keeps the person.
        let voice = caller("lamp-voice.service", 0);
        let (held, cut) = held_to(asker(None, Some(Person)), Some(&voice), lampos, true);
        assert_eq!(held, asker(Some("lamp-voice"), Some(Person)));
        assert_eq!(cut, None);
        // The voice's name as lampos, a login user's or no caller: unknown, idle.
        for nobody in [
            Some(caller("lamp-voice.service", 990)),
            Some(Caller {
                pid: 7,
                uid: 1000,
                unit: None,
            }),
            None,
        ] {
            let (held, cut) = held_to(asker(None, Some(Person)), nobody.as_ref(), lampos, true);
            assert_eq!(held, asker(Some("unknown"), Some(Idle)), "{nobody:?}");
            assert_eq!(cut, Some(("unknown".into(), Person, Idle)));
        }
        // Root's other helpers keep what they ask.
        let root = Caller {
            pid: 1,
            uid: 0,
            unit: None,
        };
        let (held, cut) = held_to(asker(None, Some(Safety)), Some(&root), lampos, true);
        assert_eq!(held, asker(Some("root"), Some(Safety)));
        assert_eq!(cut, None);
        // Under --arbitration off no source is kept, and the cap still holds:
        // a skill can't outlast a conversation by claiming one.
        let (held, _) = held_to(
            asker(None, Some(Conversation)),
            Some(&skills),
            lampos,
            false,
        );
        assert_eq!(held, asker(None, Some(Skill)));
    }

    /// Only the eyes, a skill's run or root may say where a face is.
    #[test]
    fn a_sight_is_taken_from_the_eyes_a_skill_or_root_only() {
        use lamp_ipc::caller::Caller;
        let lampos = Some(990);
        let from = |unit: Option<&str>, uid| {
            sight_from(
                Some(&Caller {
                    pid: 7,
                    uid,
                    unit: unit.map(str::to_owned),
                }),
                lampos,
            )
        };
        assert!(from(Some("lamp-eyes.service"), 990));
        assert!(from(Some("lamp-skills.service"), 990));
        assert!(from(None, 0));
        assert!(!from(Some("lamp-eyes.service"), 1000));
        assert!(!from(Some("lamp-addons.service"), 990));
        assert!(!from(None, 990));
        assert!(!sight_from(None, lampos));
        assert!(!sight_from(
            Some(&Caller {
                pid: 7,
                uid: 990,
                unit: Some("lamp-eyes.service".into())
            }),
            None
        ));
    }

    /// A cut is said once a minute per unit.
    #[test]
    fn a_cut_is_said_once_a_minute_per_unit() {
        let mut said = Minutely::default();
        let now = std::time::Instant::now();
        assert!(said.due("lamp-skills", now));
        assert!(!said.due("lamp-skills", now + std::time::Duration::from_secs(59)));
        assert!(said.due("unknown", now + std::time::Duration::from_secs(1)));
        assert!(said.due("lamp-skills", now + std::time::Duration::from_secs(60)));
    }

    /// From a real socket: a process that is not a lamp part claiming the
    /// person on the mood socket is held to idle by what the kernel says.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_claim_on_a_real_socket_is_held_by_who_the_kernel_names() {
        if rustix::process::geteuid().is_root() {
            return; // root keeps what it asks: nothing to show.
        }
        let (one, other) = std::os::unix::net::UnixDatagram::pair().unwrap();
        lamp_ipc::caller::pass_credentials(&other).unwrap();
        one.send(br#"{"gesture":"nod","source":"voice","priority":"person"}"#)
            .unwrap();
        let mut buffer = [0u8; 256];
        let (length, caller) = lamp_ipc::caller::recv_with_caller(&other, &mut buffer).unwrap();
        let moves = lamp_body::moves::Loaded::default();
        let (clip, asker) = gesture_asked(&buffer[..length], &moves).unwrap();
        assert_eq!(clip.name, "nod");
        assert_eq!(asker.priority, Some(lamp_body::moves::Priority::Person));
        // This test runs as nobody's part (no lampos uid, or not its unit).
        let (held, _) = held_to(asker, caller.as_ref(), None, true);
        assert_eq!(held.priority, Some(lamp_body::moves::Priority::Idle));
        assert_eq!(
            held.source
                .as_deref()
                .map(|source| source.ends_with("/voice")),
            Some(true)
        );
    }

    /// `--lease-socket` takes a short absolute path, once.
    #[test]
    fn the_lease_socket_is_an_absolute_path_given_once() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert_eq!(with(&[]).unwrap().unwrap().lease_socket, None);
        assert_eq!(
            with(&["--lease-socket", "/run/lampos-body/lease.sock"])
                .unwrap()
                .unwrap()
                .lease_socket,
            Some(PathBuf::from("/run/lampos-body/lease.sock"))
        );
        for refused in [
            &["--lease-socket", "lease.sock"][..],
            &["--lease-socket", "/run/../etc/x"],
            &["--lease-socket"],
            &["--lease-socket", "/a", "--lease-socket", "/b"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// Only lamp-control, run as lampos, or root may hold the body.
    #[test]
    fn only_lamp_control_or_root_may_hold_the_body() {
        use lamp_ipc::caller::Caller;
        let lampos = Some(990);
        let may = |unit: Option<&str>, uid| {
            holder_may(
                Some(&Caller {
                    pid: 7,
                    uid,
                    unit: unit.map(str::to_owned),
                }),
                lampos,
            )
        };
        assert!(may(Some("lamp-control.service"), 990));
        assert!(may(None, 0));
        assert!(!may(Some("lamp-control.service"), 1000));
        assert!(!may(Some("lamp-skills.service"), 990));
        assert!(!may(None, 990));
        assert!(!holder_may(None, lampos));
        assert!(!holder_may(
            Some(&Caller {
                pid: 7,
                uid: 990,
                unit: Some("lamp-control.service".into())
            }),
            None
        ));
    }

    /// The door over a real socket: a holder let in holds the body and is
    /// answered; a change is said unasked; a second is refused; its holder
    /// gone, the body is its own again; one `may` refuses is told why.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_lease_door_answers_its_one_holder_and_lets_go_when_it_goes() {
        use lamp_body::lease::Around;
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;
        let directory =
            std::env::temp_dir().join(format!("lamp-body-lease-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("lease.sock");
        let mut door = LeaseDoor::bind(&path, Box::new(|caller| caller.is_some())).unwrap();
        let quiet = Around::default();
        let now = std::time::Instant::now();
        assert!(!door.turn(quiet, now), "nobody holds it");
        let mut holder = UnixStream::connect(&path).unwrap();
        let mut heard = BufReader::new(holder.try_clone().unwrap());
        let mut line = String::new();
        holder.write_all(b"{\"hold\":{\"for_ms\":5000}}\n").unwrap();
        assert!(door.turn(quiet, now));
        heard.read_line(&mut line).unwrap();
        assert_eq!(line, "{\"lease\":\"held\"}\n");
        // A conversation suspends it, said unasked.
        let talking = Around {
            conversation: true,
            ..quiet
        };
        assert!(!door.turn(talking, now));
        line.clear();
        heard.read_line(&mut line).unwrap();
        assert_eq!(line, "{\"lease\":\"suspended\",\"why\":\"conversation\"}\n");
        assert!(door.turn(quiet, now));
        line.clear();
        heard.read_line(&mut line).unwrap();
        assert_eq!(line, "{\"lease\":\"held\"}\n");
        // A second holder is refused while there is one.
        let other = UnixStream::connect(&path).unwrap();
        assert!(door.turn(quiet, now));
        let mut refused = String::new();
        BufReader::new(other).read_line(&mut refused).unwrap();
        assert_eq!(
            refused,
            "{\"lease\":\"refused\",\"why\":\"the body has a holder\"}\n"
        );
        // Its holder gone, the body is its own again at once.
        drop(holder);
        drop(heard);
        assert!(!door.turn(quiet, now));
        assert!(door.holder.is_none());
        // One `may` refuses is told why and never holds it.
        let mut shut = LeaseDoor::bind(&directory.join("shut.sock"), Box::new(|_| false)).unwrap();
        let mut stranger = UnixStream::connect(directory.join("shut.sock")).unwrap();
        stranger
            .write_all(b"{\"hold\":{\"for_ms\":5000}}\n")
            .unwrap();
        assert!(!shut.turn(quiet, now));
        let mut refused = String::new();
        BufReader::new(stranger).read_line(&mut refused).unwrap();
        assert_eq!(
            refused,
            "{\"lease\":\"refused\",\"why\":\"only lamp-control may hold the body\"}\n"
        );
        assert!(!shut.turn(quiet, now));
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A holder that never stops writing can't hold the body's step: each
    /// turn reads a bounded amount and returns, and a line past 1024 bytes
    /// ends the lease with `refused` (Fourteen and the reviewer-lead on
    /// #3791). A connect flood is let in a few at a time.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_flooding_holder_is_refused_and_the_step_never_waits_on_it() {
        use lamp_body::lease::Around;
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;
        let directory =
            std::env::temp_dir().join(format!("lamp-body-flood-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("lease.sock");
        let mut door = LeaseDoor::bind(&path, Box::new(|caller| caller.is_some())).unwrap();
        let holder = UnixStream::connect(&path).unwrap();
        let heard = holder.try_clone().unwrap();
        // 1 MiB with no newline, written for as long as the door reads.
        let flood = std::thread::spawn(move || {
            let mut holder = holder;
            let _ = holder.write_all(&vec![b'x'; 1 << 20]);
        });
        let quiet = Around::default();
        let mut turns = 0;
        while door.holder.is_some() || turns == 0 {
            let began = std::time::Instant::now();
            assert!(!door.turn(quiet, began));
            assert!(began.elapsed() < std::time::Duration::from_millis(50));
            turns += 1;
            assert!(turns < 50, "a flood still holds the door");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut refused = String::new();
        BufReader::new(heard).read_line(&mut refused).unwrap();
        assert_eq!(
            refused,
            "{\"lease\":\"refused\",\"why\":\"a line longer than 1024 bytes\"}\n"
        );
        let _ = flood.join();
        // Ten callers at once: a few are taken each step, never all.
        let callers: Vec<_> = (0..10)
            .map(|_| UnixStream::connect(&path).unwrap())
            .collect();
        let mut shut = LeaseDoor::bind(&directory.join("shut.sock"), Box::new(|_| false)).unwrap();
        let strangers: Vec<_> = (0..10)
            .map(|_| UnixStream::connect(directory.join("shut.sock")).unwrap())
            .collect();
        shut.turn(quiet, std::time::Instant::now());
        let answered = strangers
            .iter()
            .filter(|stranger| {
                stranger.set_nonblocking(true).unwrap();
                let mut line = String::new();
                BufReader::new(*stranger).read_line(&mut line).is_ok() && !line.is_empty()
            })
            .count();
        assert_eq!(answered, LeaseDoor::MOST_ACCEPTS);
        drop(callers);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// One step reads at most `most` of what its holder sent, and stops once
    /// a line has passed its limit, however much is waiting.
    #[test]
    fn a_step_reads_a_bounded_amount_of_its_holder() {
        use std::io::Write;
        let (mut holder, door) = std::os::unix::net::UnixStream::pair().unwrap();
        door.set_nonblocking(true).unwrap();
        holder.write_all(&vec![b'x'; 100_000]).unwrap();
        let mut door = door;
        let mut pending = Vec::new();
        assert!(!read_some(&mut door, &mut pending, 4_096, 1_024));
        assert!(pending.len() <= 1_024 + 512, "{}", pending.len());
        // Whole lines are read on to the bound, never past it.
        let (mut writer, mut reader) = std::os::unix::net::UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        writer.write_all(&b"{}\n".repeat(10_000)).unwrap();
        let mut lines = Vec::new();
        assert!(!read_some(&mut reader, &mut lines, 4_096, usize::MAX));
        assert_eq!(lines.len(), 4_096);
        // A holder that closed is gone, once what it sent is read.
        drop(writer);
        let mut rest = Vec::new();
        let mut steps = 0;
        while !read_some(&mut reader, &mut rest, 4_096, usize::MAX) {
            steps += 1;
            assert!(steps < 10);
        }
        drop(holder);
    }

    /// The lease ends `slumped` while the body is let go, and while it is
    /// let go waiting for a hand to set its pose right (#3820's P3).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_let_go_or_waiting_body_is_slumped_for_its_lease() {
        use lamp_body::body::Glimpse;
        assert!(!slumped_now(&Glimpse::default()));
        assert!(slumped_now(&Glimpse {
            limp: true,
            ..Glimpse::default()
        }));
        assert!(slumped_now(&Glimpse {
            waiting: Some("My head is tipped too far..."),
            ..Glimpse::default()
        }));
    }

    /// A run that ends with a holder connected tells it why, when the body
    /// gave out, and lets it go either way.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_run_that_ends_tells_its_holder_why() {
        use lamp_body::lease::Around;
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;
        let directory =
            std::env::temp_dir().join(format!("lamp-body-ended-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("lease.sock");
        let mut door = LeaseDoor::bind(&path, Box::new(|caller| caller.is_some())).unwrap();
        let mut holder = UnixStream::connect(&path).unwrap();
        holder.write_all(b"{\"hold\":{\"for_ms\":5000}}\n").unwrap();
        assert!(door.turn(Around::default(), std::time::Instant::now()));
        let mut heard = BufReader::new(holder.try_clone().unwrap());
        let mut line = String::new();
        heard.read_line(&mut line).unwrap();
        assert_eq!(line, "{\"lease\":\"held\"}\n");
        door.ended(Some("slumped"));
        line.clear();
        heard.read_line(&mut line).unwrap();
        assert_eq!(line, "{\"lease\":\"lost\",\"why\":\"slumped\"}\n");
        assert!(door.holder.is_none());
        // Its socket is closed after.
        line.clear();
        assert_eq!(heard.read_line(&mut line).unwrap(), 0);
        // A lease already lost, or let go, is told nothing more at the end
        // (Third on #3828: no second `lost`).
        for (asked, then) in [
            (
                &b"{\"hold\":{\"for_ms\":5000}}\n"[..],
                Around {
                    hand: true,
                    ..Around::default()
                },
            ),
            (b"{\"let_go\":true}\n", Around::default()),
        ] {
            let mut holder = UnixStream::connect(&path).unwrap();
            holder.write_all(asked).unwrap();
            door.turn(Around::default(), std::time::Instant::now());
            door.turn(then, std::time::Instant::now());
            let mut heard = BufReader::new(holder.try_clone().unwrap());
            let mut told = Vec::new();
            door.ended(Some("slumped"));
            loop {
                let mut line = String::new();
                if heard.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                told.push(line);
            }
            assert!(
                told.iter().filter(|line| line.contains("slumped")).count() == 0,
                "{told:?}"
            );
            assert!(door.holder.is_none());
        }
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Arbitration (#940) is on or off, and off unless given.
    #[test]
    fn arbitration_is_on_or_off_and_off_unless_given() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        assert!(!with(&[]).unwrap().unwrap().arbitration);
        assert!(with(&["--arbitration", "on"]).unwrap().unwrap().arbitration);
        assert!(
            !with(&["--arbitration", "off"])
                .unwrap()
                .unwrap()
                .arbitration
        );
        for refused in [
            &["--arbitration", "ON"][..],
            &["--arbitration"],
            &["--arbitration", "on", "--arbitration", "on"],
            &["--arbitration", "on", "--help"],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn eyes_are_a_socket_and_what_the_picture_is_worth_or_nothing() {
        let with = |more: &[&'static str]| parse(args(&[&FULL[..], more].concat()));
        let socket = ["--gaze-socket", "/run/lampos-body/gaze.sock"];
        let looked = with(&[&socket[..], &["--gaze", "-342,200"]].concat())
            .unwrap()
            .unwrap()
            .gaze
            .unwrap();
        assert_eq!(
            looked,
            GazeChoice {
                socket: PathBuf::from("/run/lampos-body/gaze.sock"),
                turn: [-342, 200],
                reach: [250, 120],
            }
        );
        let reached = with(
            &[
                &socket[..],
                &["--gaze", "342,-200", "--gaze-reach", "300,100"],
            ]
            .concat(),
        )
        .unwrap()
        .unwrap()
        .gaze
        .unwrap();
        assert_eq!((reached.turn, reached.reach), ([342, -200], [300, 100]));
        assert!(with(&[]).unwrap().unwrap().gaze.is_none());
        // A socket without --gaze, the reach given or not, as the product's
        // unit passes them before a lamp's gaze is measured: gaze off.
        for unmeasured in [
            &socket[..],
            &[&socket[..], &["--gaze-reach", "480,120"]].concat(),
        ] {
            let parsed = with(unmeasured).unwrap().unwrap();
            assert!(parsed.gaze.is_none(), "{unmeasured:?}");
            assert_eq!(
                parsed.unmeasured_gaze_socket,
                Some(PathBuf::from("/run/lampos-body/gaze.sock"))
            );
        }
        assert!(with(&[]).unwrap().unwrap().unmeasured_gaze_socket.is_none());
        for refused in [
            &["--gaze-socket", "gaze.sock"][..],
            &["--gaze-socket", "gaze.sock", "--gaze-reach", "480,120"],
            &["--gaze", "342,200"],
            &["--gaze-reach", "300,100"],
            &["--gaze-socket", "gaze.sock", "--gaze", "342,200"],
            &[
                "--gaze-socket",
                "/run/lampos-body/gaze.sock",
                "--gaze",
                "342",
            ],
            &[
                "--gaze-socket",
                "/run/lampos-body/gaze.sock",
                "--gaze",
                "+342,200",
            ],
        ] {
            assert!(with(refused).is_err(), "{refused:?}");
        }
    }

    /// #2188: the product unit's own `ExecStart`, with the poses a freshly
    /// measured lamp has (lamp-body-measure writes no `--gaze`), starts with
    /// gaze off; with this lamp's `--gaze` added once measured, gaze is on.
    /// Read from the unit file, so a unit and this rule cannot drift apart.
    #[test]
    fn the_product_unit_starts_a_fresh_lamp_with_gaze_off_and_on_once_measured() {
        const UNIT: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/product/lamp-body.service"
        ));
        let exec = UNIT
            .lines()
            .find_map(|line| line.strip_prefix("ExecStart="))
            .expect("the product unit has an ExecStart");
        // lamp-a63e's lamp.env as lamp-body-measure wrote it (19 Oct): no --gaze.
        let measured = "--home 1772,2052,2038 --folded-arm 2047,2046 --stand 1772,2052,2038,2657,2602 --pitch-window 1740,2030 --torque on";
        assert!(!measured.contains("--gaze "), "{measured}");
        // systemd splits an unquoted $BODY_POSES on whitespace, and %H is the host.
        let start = |poses: &str| {
            let words: Vec<String> = exec
                .split_whitespace()
                .skip(1)
                .flat_map(|word| match word {
                    "$BODY_POSES" => poses.split_whitespace().map(str::to_owned).collect(),
                    "%H" => vec!["lamp-a63e".to_owned()],
                    word => vec![word.to_owned()],
                })
                .collect();
            parse(words.iter().map(OsString::from))
        };
        let fresh = start(measured)
            .expect("a freshly measured lamp's body starts")
            .unwrap();
        assert!(fresh.gaze.is_none());
        assert_eq!(
            fresh.unmeasured_gaze_socket,
            Some(PathBuf::from("/run/lampos-body/gaze.sock"))
        );
        let looked = start(&format!("{measured} --gaze 755,555"))
            .expect("a lamp with its gaze measured starts")
            .unwrap();
        let gaze = looked.gaze.expect("gaze is on once measured");
        assert_eq!(gaze.turn, [755, 555]);
        assert_eq!(gaze.reach, [480, 120], "the unit's own --gaze-reach");
        assert!(looked.unmeasured_gaze_socket.is_none());
    }

    #[test]
    fn a_gesture_is_asked_for_by_the_name_of_a_clip_and_nothing_else() {
        const WIGGLE: lamp_body::clips::Clip = lamp_body::clips::Clip {
            name: "wiggle",
            step_ms: 100,
            frames: &[[0; 5], [20, 0, 0, 0, 0], [0; 5]],
        };
        let none = lamp_body::moves::Loaded::default();
        let moves = lamp_body::moves::Loaded {
            moves: vec![&WIGGLE],
            refused: Vec::new(),
        };
        let gesture_of = |datagram: &[u8]| gesture_of(datagram, &moves);
        assert_eq!(
            gesture_of(br#"{"gesture":"nod"}"#).map(|clip| clip.name),
            Some("nod")
        );
        assert_eq!(
            gesture_of(br#"{"gesture":"headshake"}"#).map(|clip| clip.name),
            Some("headshake")
        );
        // A move read from --moves, by its name, and not without it.
        assert_eq!(
            gesture_of(br#"{"gesture":"wiggle"}"#).map(|clip| clip.name),
            Some("wiggle")
        );
        assert!(super::gesture_of(br#"{"gesture":"wiggle"}"#, &none).is_none());
        for ignored in [
            &br#"{"gesture":"dance"}"#[..],
            br#"{"gesture":"nod","scale":3}"#,
            br#"{"gesture":7}"#,
            br#"{"mood":"listening","level":3}"#,
            b"nod",
            b"",
        ] {
            assert!(gesture_of(ignored).is_none());
        }
        // And a gesture is not a mood.
        assert_eq!(mood_of(br#"{"gesture":"nod"}"#), None);
    }

    /// Each kind of datagram is read as before a move could be sent: a
    /// gesture by name, a mood, and anything else dropped and said; a move
    /// sent whole is a gesture, or dropped with why.
    #[test]
    fn the_mood_socket_reads_a_gesture_a_move_a_mood_or_says_why_not() {
        let (moves, mut sent) = (
            lamp_body::moves::Loaded::default(),
            lamp_body::moves::Sent::default(),
        );
        let mut heard = |datagram: &str| heard(datagram.as_bytes(), &moves, &mut sent);
        assert_eq!(
            heard(r#"{"gesture":"nod"}"#),
            Heard::Gesture(&lamp_body::clips::NOD)
        );
        assert_eq!(
            heard(r#"{"mood":"listening","level":3}"#),
            Heard::Mood(Mood::Listening, 3)
        );
        let frames: Vec<String> = (0..=30)
            .map(|at| format!("[{},0,0,0,0]", 15 - (at - 15i32).abs()))
            .collect();
        let sway = format!(
            r#"{{"move":{{"name":"sway","step_ms":50,"frames":[{}]}}}}"#,
            frames.join(",")
        );
        match heard(&sway) {
            Heard::Gesture(clip) => assert_eq!((clip.name, clip.frames.len()), ("sway", 31)),
            other => panic!("{other:?}"),
        }
        let Heard::Dropped(why) = heard(&sway.replace("\"step_ms\":50", "\"step_ms\":5")) else {
            panic!("a bad move was taken");
        };
        assert!(
            why.starts_with("a move this body cannot play: move \"sway\""),
            "{why}"
        );
        let Heard::Dropped(why) = heard(r#"{"gesture":"dance"}"#) else {
            panic!("an unknown gesture was taken");
        };
        assert!(why.contains("not a mood or a gesture"), "{why}");
        // A datagram `recv` cut at the buffer is said as too long.
        let long = format!("{sway:<0$}", lamp_body::moves::MOST_DATAGRAM);
        assert_eq!(
            heard(&long),
            Heard::Dropped("a datagram longer than 16 KiB, not read".into())
        );
    }

    /// The reviewer on #1473: two skills asking within one step. The one
    /// `kept_of` drops is told on `body/gesture` too, with whose it was and
    /// who keeps the body, whichever of the two was read first.
    #[test]
    fn a_gesture_dropped_as_it_is_read_is_told_whose_it_was_and_who_keeps_the_body() {
        use lamp_body::moves::{Priority, Source};
        let nod = (&lamp_body::clips::NOD, Priority::Skill);
        let shy = (&lamp_body::clips::SHY, Priority::Skill);
        // The second of equal priority is kept: the first is told.
        let (kept, dropped) = kept_of(Some(nod), shy, true);
        assert_eq!(kept, shy);
        assert_eq!(dropped, Some(("nod", "dropped: busy, skill")));
        let told = intake_dropped(
            true,
            Some("skill:dance"),
            Some(("nod", "skill:stretch-break")),
        );
        assert_eq!(
            told,
            (
                Source::new("skill:stretch-break"),
                Source::new("skill:dance")
            )
        );
        // A lower one read second is dropped: it is told who holds it.
        let talking = (&lamp_body::clips::NOD, Priority::Conversation);
        let (kept, dropped) = kept_of(Some(talking), shy, true);
        assert_eq!(kept, talking);
        assert_eq!(dropped, Some(("shy", "dropped: busy, conversation")));
        let told = intake_dropped(false, Some("skill:dance"), Some(("nod", "voice")));
        assert_eq!(told, (Source::new("skill:dance"), Source::new("voice")));
        // Nobody named: nobody told of.
        assert_eq!(intake_dropped(true, None, None), (None, None));
    }

    /// A gesture or a move may say who asks, and a priority that is not one
    /// of the five refuses it, as does any other field. Only under
    /// --arbitration on is who asks weighed; off it is ignored.
    #[test]
    fn who_asks_is_weighed_only_with_arbitration_and_an_unknown_priority_is_refused() {
        use lamp_body::moves::{Asker, Priority};
        let moves = lamp_body::moves::Loaded::default();
        let mut sent = lamp_body::moves::Sent::default();
        let mut heard =
            |datagram: &str, on: bool| heard_asked(datagram.as_bytes(), &moves, &mut sent, on);
        let skill = Asker {
            source: Some("skill:dance".into()),
            priority: Some(Priority::Skill),
        };
        let asked = r#"{"gesture":"nod","source":"skill:dance","priority":"skill"}"#;
        assert_eq!(
            heard(asked, true),
            (Heard::Gesture(&lamp_body::clips::NOD), skill.clone())
        );
        let (said, asker) = heard(r#"{"gesture":"nod"}"#, true);
        assert_eq!(said, Heard::Gesture(&lamp_body::clips::NOD));
        assert_eq!(asker.priority(), Priority::Conversation);
        let frames: Vec<String> = (0..=30)
            .map(|at| format!("[{},0,0,0,0]", 15 - (at - 15i32).abs()))
            .collect();
        let sway = format!(
            r#"{{"move":{{"name":"sway","step_ms":50,"frames":[{}]}},"source":"skill:dance","priority":"skill"}}"#,
            frames.join(",")
        );
        match heard(&sway, true) {
            (Heard::Gesture(clip), asker) => assert_eq!((clip.name, asker), ("sway", skill)),
            other => panic!("{other:?}"),
        }
        for refused in [
            r#"{"gesture":"nod","priority":"boss"}"#,
            r#"{"gesture":"nod","priority":3}"#,
            r#"{"gesture":"nod","priority":"skill","volume":3}"#,
            &sway.replace(r#""skill"}"#, r#""boss"}"#),
        ] {
            let (said, _) = heard(refused, true);
            assert!(
                matches!(said, Heard::Dropped(ref why) if why.contains("not a mood or a gesture")),
                "{refused}: {said:?}"
            );
        }
        // Off, a stamp is not weighed, even `null`: a stamped nod is taken
        // as an unstamped one, so callers may always stamp; only its
        // priority is kept, for a skill's gesture to give way to a
        // conversation that begins. Any other field is still refused.
        let plain = heard(r#"{"gesture":"nod"}"#, false);
        assert_eq!(
            plain,
            (Heard::Gesture(&lamp_body::clips::NOD), Asker::default())
        );
        let unnamed = Asker {
            source: None,
            priority: Some(Priority::Skill),
        };
        assert_eq!(
            heard(asked, false),
            (Heard::Gesture(&lamp_body::clips::NOD), unnamed.clone())
        );
        for stamped in [
            r#"{"gesture":"nod","source":null}"#,
            r#"{"gesture":"nod","priority":null}"#,
            r#"{"gesture":"nod","source":null,"priority":null}"#,
        ] {
            assert_eq!(heard(stamped, false), plain, "{stamped}");
        }
        let nulls = sway
            .replace(r#""skill:dance""#, "null")
            .replace(r#""skill"}"#, "null}");
        for (stamped, kept) in [(&sway, unnamed), (&nulls, Asker::default())] {
            match heard(stamped, false) {
                (Heard::Gesture(clip), asker) => assert_eq!((clip.name, asker), ("sway", kept)),
                other => panic!("{stamped}: {other:?}"),
            }
        }
        for refused in [
            r#"{"gesture":"nod","volume":3}"#,
            r#"{"gesture":"nod","priority":"skill","volume":3}"#,
        ] {
            let (said, _) = heard(refused, false);
            assert!(
                matches!(said, Heard::Dropped(ref why) if why.contains("not a mood or a gesture")),
                "{refused}: {said:?}"
            );
        }
    }

    /// Two gestures read in one step: under --arbitration on the higher is
    /// handed to the body and the other said dropped, naming who holds it;
    /// on a tie the newer. Off, the newer, as before.
    #[test]
    fn of_two_gestures_in_one_step_the_higher_is_kept_and_the_other_said() {
        use lamp_body::clips::{NOD, STRETCHING};
        use lamp_body::moves::Priority::{Conversation, Person, Skill};
        let (nod, stretch) = ((&NOD, Person), (&STRETCHING, Skill));
        assert_eq!(
            kept_of(Some(nod), stretch, true),
            (nod, Some(("stretching", "dropped: busy, person")))
        );
        assert_eq!(
            kept_of(Some(stretch), nod, true),
            (nod, Some(("stretching", "dropped: busy, person")))
        );
        let talk = (&STRETCHING, Conversation);
        assert_eq!(
            kept_of(Some((&NOD, Conversation)), talk, true),
            (talk, Some(("nod", "dropped: busy, conversation")))
        );
        assert_eq!(kept_of(None, stretch, true), (stretch, None));
        assert_eq!(kept_of(Some(nod), stretch, false), (stretch, None));
    }

    /// A line asked again and again is said once in five seconds, then with
    /// how many were held back; another line is said at once.
    #[test]
    fn a_line_repeated_is_said_once_in_five_seconds_with_a_count() {
        let start = std::time::Instant::now();
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);
        let mut hush = Hush::default();
        let line = || "gesture nod dropped: another gesture plays".to_owned();
        assert_eq!(hush.due(line(), at(0)), Some(line()));
        for step in 1..250 {
            assert_eq!(hush.due(line(), at(step * 20)), None);
        }
        assert_eq!(
            hush.due("gesture nod dropped: paused".into(), at(100))
                .as_deref(),
            Some("gesture nod dropped: paused")
        );
        assert_eq!(
            hush.due(line(), at(5_000)),
            Some(format!("{} (249 more like it before this)", line()))
        );
        assert_eq!(hush.due(line(), at(5_020)), None);
        // Not heard of for a minute: forgotten, and said plainly again.
        assert_eq!(hush.due(line(), at(70_000)), Some(line()));
        assert_eq!(hush.0.len(), 1);
    }

    /// While a skill says `reading`, the voice's moods do not take the
    /// pose from it, even when one comes after it in the same step; once it
    /// stops, they count again.
    #[test]
    fn reading_holds_over_the_voice_until_it_goes_unsaid() {
        let reading = (Mood::Reading, 200);
        let resting = (Mood::Resting, 0);
        // Both in one step, the voice's last: reading, renewed.
        assert_eq!(of_this_step(&[reading, resting], true), Some(reading));
        assert_eq!(of_this_step(&[reading, resting], false), Some(reading));
        assert_eq!(
            of_this_step(&[resting, reading, (Mood::Listening, 90)], true),
            Some(reading)
        );
        // Only the voice, while reading is fresh: nothing new.
        assert_eq!(of_this_step(&[resting], true), None);
        assert_eq!(of_this_step(&[(Mood::Listening, 90)], true), None);
        assert_eq!(of_this_step(&[], true), None);
        // Reading unsaid: the newest of the voice's.
        assert_eq!(
            of_this_step(&[resting, (Mood::Listening, 90)], false),
            Some((Mood::Listening, 90))
        );
        assert_eq!(of_this_step(&[], false), None);
    }

    #[test]
    fn a_mood_is_one_exact_json_object_and_anything_else_is_ignored() {
        assert_eq!(
            mood_of(br#"{"mood":"listening","level":120}"#),
            Some((Mood::Listening, 120))
        );
        assert_eq!(
            mood_of(br#"{"mood":"resting","level":0}"#),
            Some((Mood::Resting, 0))
        );
        assert_eq!(
            mood_of(br#"{"mood":"reading","level":200}"#),
            Some((Mood::Reading, 200))
        );
        for ignored in [
            &br#"{"mood":"dancing","level":1}"#[..],
            br#"{"mood":"speaking","level":256}"#,
            br#"{"mood":"speaking"}"#,
            br#"{"mood":"speaking","level":1,"goal":2100}"#,
            b"listening",
            b"",
        ] {
            assert_eq!(mood_of(ignored), None);
        }
    }

    /// #1679: a reading pose may say where across the picture it turns,
    /// and is still a reading pose; a place off the picture, or one beside
    /// another mood, turns nothing.
    #[test]
    fn a_reading_pose_may_say_where_it_turns() {
        let reading = br#"{"mood":"reading","level":0,"toward":-0.5}"#;
        assert_eq!(mood_of(reading), Some((Mood::Reading, 0)));
        assert_eq!(toward_of(reading), Some(-0.5));
        assert_eq!(toward_of(br#"{"mood":"reading","level":0}"#), None);
        for turns_nothing in [
            &br#"{"mood":"reading","level":0,"toward":1.5}"#[..],
            br#"{"mood":"listening","level":0,"toward":0.5}"#,
            br#"{"mood":"reading","level":0,"toward":"left"}"#,
        ] {
            assert_eq!(toward_of(turns_nothing), None);
        }
        assert_eq!(
            mood_of(br#"{"mood":"listening","level":0,"toward":0.5}"#),
            Some((Mood::Listening, 0))
        );
    }
}
