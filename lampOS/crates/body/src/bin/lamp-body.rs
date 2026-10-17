//! The lamp's small expressive body: one process that owns the servo port
//! and moves the head a few degrees for what the lamp is doing.

use lamp_body::{calibration::UnitId, moves::Asker};
use std::{ffi::OsString, path::PathBuf};

const USAGE: &str = "lamp-body --device ABSOLUTE_PATH --unit-id EXPLICIT_ID --lock-directory PRIVATE_ABSOLUTE_DIRECTORY --mood-socket ABSOLUTE_PATH [--home PITCH,ROLL,YAW] [--reach UP,DOWN,ROLL,YAW] [--speed COUNTS_PER_SECOND] [--servo-speed N] [--servo-acceleration N] [--stand PITCH,ROLL,YAW,SHOULDER,ELBOW --folded-arm SHOULDER,ELBOW [--stand-share PERCENT] [--stand-seconds N] [--awake-seconds N] [--arm-load SHOULDER,ELBOW] [--idle on|off]] [--gaze-socket ABSOLUTE_PATH --gaze YAW,PITCH [--gaze-reach YAW,PITCH]] [--stand-roll COUNTS] [--force-limit N] [--torque on|off] [--wake on|off] [--give-way return|stay|off] [--limp on|off] [--overload release] [--rest-glance on|off] [--pitch-window LOW,HIGH] [--look-at-name on|off] [--moves ABSOLUTE_DIR] [--arbitration on|off]\n\nOwns the servo port and moves head pitch, head roll and base yaw a few degrees around\na home pose (the one given, in encoder counts, or else where they are found), for the mood\nit is told on the socket. Without --stand it never touches the shoulder or the elbow, and\nwithout --stand or --torque on it writes torque only off, on all five, as a run ends on a surprise\nor a start is refused for heat or load. With --stand (which needs --home and --folded-arm) it also raises the shoulder and\nthe elbow to that pose when a mood other than rest arrives, folds them down again\n--awake-seconds after the last one, and writes torque once to a shoulder or elbow found\nwith an old overload latched. With --gaze-socket and --gaze it takes where a face is from\nlamp-eyes on that socket ({\"face\":{...}} or {\"face\":null}) and, while the lamp listens\nor speaks, turns the head to it in small corrections: --gaze is the counts of base yaw and of\nhead pitch, signed, that bring a face from the right and from the bottom edge of the picture\nto its middle. Refuses to start unless the three joints have torque on, a clear\nstatus and the head is inside its start window; one found as hot or as loaded as a run stops\nat is refused with torque off on all five, so that nothing pushes on until the next start. With --torque on it first takes hold of a\njoint found without torque (the three always, the shoulder and the elbow with --stand):\nits goal is written as where it is, then torque on, and if it then moves more than 12 counts\nit does not start. With --wake on and --home, a joint found outside where it may start\n(the head out of its start window, or a joint farther from home than it may be found) but within\n400 counts of home is not refused: before anything else it is brought home slowly, a small stir,\na pause, then an eased rise at 20 counts a second at most, never away from home, the head\nfirst and the base once the head is up; an arm found off\nits way is not lowered, and the lamp runs folded. By default --give-way return and --limp on
(--give-way off is the body as it was before them, and --limp off keeps giving way without the
slump). With --give-way, a joint that pushes at its goal without moving for\n1.5 s is told to stop (its goal becomes where it is; torque is never written off); with\n--give-way stay, a head or base joint that gave way and is moved more than 15 counts by\na hand for over a second stays where it was put. With --limp on as well, as the old runtime
did, a head or base joint that a hand stops and still pushes a second later makes the lamp
slump (torque off on all five, held again 5 s later where it lies; after an overload, 120 s);
a hand never ends the run, and a head roll or base found beyond 400 counts of home at start is
taken hold of where it lies. With --overload release, any joint that\nreads a load of 800 of 1000 (80% of --force-limit for a joint held to one) for 1.5 s has\ntorque switched off on all five, so the lamp slumps; 120 s later each is held where it is\n(its goal, then torque on) and the head is brought home as --wake does; a shoulder left\nup over an elbow that fell is first lowered folded, and otherwise the arm waits where it\nslumped until the lamp is next asked to stand; a second time within ten\nminutes ends the run with torque off. With --rest-glance on, a lamp at rest takes what\nExpression asks of the head (50 counts of pitch, 25 of roll at most, never the base or the arm)\nand comes back to rest when it runs out. --pitch-window is this lamp's own window for the\nhead's pitch while it is beside the base (by default lamp-4ace's, 1740,2030): never under\nthe head's floor (1670), around --home's pitch, and inside the servo's own limits or it does\nnot start. With --look-at-name on, the lamp's name heard on the bus\n(name/heard) while it does not speak is taken as being spoken to for two seconds: it rises,\nlooks up and turns to a face at once; with nobody in view it glances a little to one side and\nthe other once it stands, and lets it go. At most once in ten seconds. Stops at the first surprise, holding\nwhere it is, then, as the old runtime did, lets go: torque off on all five. Ends on SIGINT or SIGTERM after easing back to where it started.\nThe mood socket takes datagrams of JSON: {\"mood\":\"resting|listening|thinking|speaking\",\"level\":0..255}.\nA lamp standing at its full height also makes a gesture asked for there: {\"gesture\":\"nod\"}\n(nod, headshake, greeting, laugh, shy, stretching); a folded one with --stand stands up for it\nfirst, unless paused, held by a hand or slumped, and the journal says why each was taken or dropped. With --moves, the moves in that folder\n(NAME.json, docs/body.md) are read at start and asked for by name the same way; one this body\ncannot play is refused with why, and a folder that cannot be read leaves the body's own.\nA skill sends its own move whole, {\"move\":{\"name\":...,\"step_ms\":...,\"frames\":[...]}}, held to the same\nbounds and played the same way; one it cannot play is dropped with why.\nWith --arbitration on (off unless given), a gesture or a move may also say who asks,\n\"source\" (free text) and \"priority\" (person, safety, conversation, skill or idle;\nconversation without one). One asked while another plays ends it if asked higher, easing\nout as a dropped gesture does, and is dropped otherwise (\"dropped: busy, conversation\");\na hand on the lamp drops any (\"dropped: held by a hand\"). Off, \"source\" and\n\"priority\" are read and ignored, so a caller may always say them.\nPrints one JSON report when it ends.\n";

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
    stand_roll: Option<u16>,
    force_limit: Option<u16>,
    torque_on: bool,
    wake: bool,
    give_way: Option<GiveWay>,
    limp: bool,
    overload_release: bool,
    rest_glance: bool,
    pitch_window: Option<(u16, u16)>,
    look_at_name: bool,
    moves: Option<PathBuf>,
    arbitration: bool,
}

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

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Arguments>, &'static str> {
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
    let mut rest_glance = None;
    let mut pitch_window = None;
    let mut look_at_name = None;
    let mut moves = None;
    let mut arbitration = None;
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
                || rest_glance.is_some()
                || pitch_window.is_some()
                || look_at_name.is_some()
                || moves.is_some()
                || arbitration.is_some()
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
            Some("--overload") if overload.is_none() => {
                overload = Some(match value.to_str() {
                    Some("release") => true,
                    _ => return Err("--overload is release"),
                });
            }
            Some("--moves") if moves.is_none() => moves = Some(PathBuf::from(value)),
            Some("--arbitration") if arbitration.is_none() => {
                arbitration = Some(match value.to_str() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--arbitration is on or off"),
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
            _ => return Err("unknown or duplicate option"),
        }
    }
    // Eyes are a socket and what the picture is worth, or nothing.
    let gaze = match (gaze_socket, gaze_turn) {
        (Some(socket), Some(turn)) => Some(GazeChoice {
            socket,
            turn,
            reach: gaze_reach.unwrap_or([250, 120]),
        }),
        (None, None) if gaze_reach.is_none() => None,
        _ => return Err("--gaze-socket and --gaze go together, and --gaze-reach needs them"),
    };
    // A load bound at or over the force limit could never be read, and the
    // guard it is would be off.
    if let (Some(force), Some(load)) = (force_limit, arm_load)
        && load.iter().any(|bound| *bound >= force)
    {
        return Err("--arm-load must be under --force-limit");
    }
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
        rest_glance: rest_glance.unwrap_or(true),
        pitch_window,
        look_at_name: look_at_name.unwrap_or(true),
        moves,
        arbitration: arbitration.unwrap_or(false),
    };
    if result.limp && result.give_way.is_none() {
        return Err("--limp on needs --give-way return or stay");
    }
    if result.stand_roll.is_some() && result.standing.is_none() {
        return Err("--stand-roll needs --stand");
    }
    let gaze_path = result.gaze.as_ref().map(|gaze| &gaze.socket);
    for path in [&result.device, &result.lock_directory, &result.mood_socket]
        .into_iter()
        .chain(gaze_path)
        .chain(result.moves.as_ref())
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
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Message {
        mood: lamp_body::gesture::Mood,
        level: u8,
    }
    let message: Message = serde_json::from_slice(datagram).ok()?;
    Some((message.mood, message.level))
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
/// weighed, and off it is ignored, so callers and bodies need not switch
/// together. Any other field is refused either way.
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
    let weighed = |asker: Asker| if arbitration { asker } else { Asker::default() };
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
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(error) => {
            eprintln!("lamp-body failed: {error}");
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
        arguments.stand_roll,
        arguments.force_limit,
        arguments.torque_on,
        arguments.wake,
        arguments.give_way,
        arguments.limp,
        arguments.overload_release,
        arguments.rest_glance,
        arguments.pitch_window,
        arguments.look_at_name,
        arguments.moves,
        arguments.arbitration,
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
    let moves = moves_of(arguments.moves.as_deref());
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
    let arbitration = arguments.arbitration;
    // Paused by the keeper (`lamp/pause`): it rests and takes nothing new
    // until resumed. Looked at with the moods, once a step.
    let mut pause = lamp_bus::Bus::from_environment()
        .ok()
        .map(lamp_body::pause_in::PauseIn::on);
    let paused = std::cell::Cell::new(false);
    let mut unknown_said = Hush::default();
    // A skill's move sent whole, played as a gesture (docs/body.md).
    let mut moves_sent = lamp_body::moves::Sent::besides(&moves);
    let mut datagram = vec![0u8; lamp_body::moves::MOST_DATAGRAM];
    // When a skill last said `reading` (`of_this_step`).
    let mut reading_at: Option<std::time::Instant> = None;
    let news = || {
        paused.set(pause.as_mut().is_some_and(|pause| pause.paused()));
        // Everything waiting; which of it counts is `of_this_step`'s.
        let mut moods_heard = Vec::new();
        for _ in 0..32 {
            match moods.recv(&mut datagram) {
                Ok(length) => {
                    let sent = &datagram[..length];
                    match heard_asked(sent, &moves, &mut moves_sent, arbitration) {
                        (Heard::Gesture(clip), asker) => {
                            let asked = (clip, asker.priority());
                            let source = asker
                                .source
                                .map(|source| source.chars().take(64).collect::<String>());
                            let (kept, dropped) = kept_of(gesture_asked.get(), asked, arbitration);
                            gesture_asked.set(Some(kept));
                            if let Some((name, why)) = dropped {
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
                        (Heard::Mood(mood, level), _) => moods_heard.push((mood, level)),
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
                Err(_) => break,
            }
        }
        let reading = reading_at.is_some_and(|at| at.elapsed() < lamp_body::body::MOOD_STALE);
        let latest = of_this_step(&moods_heard, reading);
        if latest.is_some_and(|(mood, _)| mood == lamp_body::gesture::Mood::Reading) {
            reading_at = Some(std::time::Instant::now());
        }
        if paused.get() {
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
        rest_glance: arguments.rest_glance,
        pitch_window: arguments.pitch_window.unwrap_or(folded.pitch_window),
        look_at_name: arguments.look_at_name,
        arbitration: arguments.arbitration,
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
    let mut tipping_said = lamp_body::body::Tipping::default();
    let mut looks_said: Option<&'static str> = None;
    let mut fate_said = Hush::default();
    let mut eased = [0u16; 3];
    let mut handled = ([0u16; 3], [0u16; 5]);
    let mut limp = false;
    // Where the joints are and how the body is, for the rest of the lamp,
    // if the unit says so. Offered and never waited for.
    let mut bus = lamp_body::bus_out::BusOut::from_environment(|why| eprintln!("lamp-body: {why}"));
    let notify = lamp_keeper::Notify::from_environment();
    let ready = std::cell::Cell::new(false);
    let watch = |glimpse: &lamp_body::body::Glimpse| {
        step_said(&notify, &ready);
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
        for (name, why) in glimpse.gesture_fates {
            if let Some(line) = fate_said.due(
                format!("gesture {name} {why}{}", source(name)),
                std::time::Instant::now(),
            ) {
                eprintln!("lamp-body: step {} {line}", glimpse.step);
            }
        }
        let gestures = gesture_said.step(glimpse);
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
        lamp_body::bus_out::tell(&gestures, glimpse.step, bus.as_mut(), |line| {
            eprintln!("{line}");
        });
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
            (
                Some(lamp_body::gaze::Looking::new(choice.turn, choice.reach)?),
                Some(sights),
            )
        }
        None => (None, None),
    };
    let eyes = || {
        // Everything waiting, and only the newest of it counts.
        let sights = sights.as_ref()?;
        let mut latest = None;
        let mut datagram = [0u8; 160];
        for _ in 0..32 {
            match sights.recv(&mut datagram) {
                Ok(length) => {
                    latest = serde_json::from_slice::<lamp_body::gaze::Seen>(&datagram[..length])
                        .ok()
                        .or(latest);
                }
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
            };
            // Read all the same, so that nothing kept from before the pause
            // is acted on after it.
            // A gesture is passed on to be dropped and said.
            let asked = if paused.get() {
                lamp_body::body::Asked {
                    gesture: asked.gesture,
                    ..lamp_body::body::Asked::PAUSED
                }
            } else {
                asked
            };
            (asked, priority)
        },
        watch,
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
    // Again for a body that stopped by itself; a second time is harmless.
    notify.stopping();
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
        for refused in [
            &socket[..],
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
        // Off, a stamp is ignored, even `null`: a stamped nod is taken
        // exactly as an unstamped one, so callers may always stamp. Any
        // other field is still refused.
        let plain = heard(r#"{"gesture":"nod"}"#, false);
        assert_eq!(
            plain,
            (Heard::Gesture(&lamp_body::clips::NOD), Asker::default())
        );
        for stamped in [
            asked,
            r#"{"gesture":"nod","source":null}"#,
            r#"{"gesture":"nod","priority":null}"#,
            r#"{"gesture":"nod","source":null,"priority":null}"#,
        ] {
            assert_eq!(heard(stamped, false), plain, "{stamped}");
        }
        let nulls = sway
            .replace(r#""skill:dance""#, "null")
            .replace(r#""skill"}"#, "null}");
        for stamped in [&sway, &nulls] {
            match heard(stamped, false) {
                (Heard::Gesture(clip), asker) => {
                    assert_eq!((clip.name, asker), ("sway", Asker::default()))
                }
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
}
