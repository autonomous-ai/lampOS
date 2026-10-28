//! Bus transport around the injected-clock engine. No hardware is opened.
use crate::{Catalogue, Engine, Input, Output};
use lamp_bus::{Bus, Message, Publisher, Subscriber};
use lamp_msgs::{
    ClockStamp, Concentration, ControlGesture, Diagnostic, ExpressionBody, ExpressionLight,
    ExpressionState, Faces, JointState, Level, Light, NameHeard, PersonActivity, RoomState,
    ScheduleDue, Speech, SuddenSound, TouchEvent, TouchState, VoiceMute, VoicePlayout,
    VoiceProgress, VoiceRemarked, VoiceSound, VoiceState, VoiceTiming,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Every input has a separate bounded drain so noisy text cannot starve a stop.
pub const INPUTS: [(&str, &str); 17] = [
    (VoiceTiming::TOPIC, VoiceTiming::TYPE),
    (VoiceProgress::TOPIC, VoiceProgress::TYPE),
    (PersonActivity::TOPIC, PersonActivity::TYPE),
    (Concentration::TOPIC, Concentration::TYPE),
    (VoicePlayout::TOPIC, VoicePlayout::TYPE),
    (VoiceSound::TOPIC, VoiceSound::TYPE),
    (JointState::TOPIC, JointState::TYPE),
    (Faces::TOPIC, Faces::TYPE),
    (VoiceState::TOPIC, VoiceState::TYPE),
    (RoomState::TOPIC, RoomState::TYPE),
    (Light::TOPICS[0], Light::TYPE),
    (Speech::TOPICS[0], Speech::TYPE),
    (Speech::TOPICS[1], Speech::TYPE),
    (NameHeard::TOPIC, NameHeard::TYPE),
    (SuddenSound::TOPIC, SuddenSound::TYPE),
    (ControlGesture::TOPIC, ControlGesture::TYPE),
    (ScheduleDue::TOPIC, ScheduleDue::TYPE),
];
/// The head's topics (#711), and the voice's mute that says when the head
/// put it to sleep, read only with `serve --head-touch`.
pub const HEAD_INPUTS: [(&str, &str); 3] = [
    (TouchEvent::TOPIC, TouchEvent::TYPE),
    (TouchState::TOPIC, TouchState::TYPE),
    (VoiceMute::TOPIC, VoiceMute::TYPE),
];
/// The voice's remarks about the lamp (#4175), read only with
/// `LAMP_REMARK_GLANCE=on`.
pub const REMARK_INPUTS: [(&str, &str); 1] = [(VoiceRemarked::TOPIC, VoiceRemarked::TYPE)];

/// Whether a `lamp/online` message says the internet cannot be reached: only
/// a fresh, valid one with `online` false. Stale, absent or unknown is not.
pub fn offline_in(message: Option<lamp_bus::Message>) -> bool {
    message
        .filter(|message| !message.expired)
        .and_then(|message| message.parse::<lamp_msgs::Online>().ok())
        .filter(|online| online.check().is_ok())
        .is_some_and(|online| online.online == Some(false))
}

/// Whether the last `voice/privacy` said is the privacy switch closed. It
/// is open only when that lease is valid and says `physical_muted: false`;
/// closed, unknown (the lamp can't listen then either), or a lease that
/// fails its check or doesn't parse, all count as closed, however late, so
/// no sender's trouble reads as open (the reviewer on #1282 and #1301).
/// Only never said counts as open, so a lamp without the route looks up as
/// before. Read at each request from the head and at each step of a look
/// up, never subscribed: the lease is said every 10 ms (#711). Only the
/// Controls privacy route (`LAMP_PRIVACY_CONTROLS=on` or `no-slide`) says it
/// at all. A valid lease of a lamp with no slide (`no_slide`) is open: it
/// has no switch to close, as a lamp without the route.
pub fn privacy_muted_in(message: Option<lamp_bus::Message>) -> bool {
    message.is_some_and(|message| {
        message
            .parse::<lamp_msgs::MicrophonePrivacy>()
            .ok()
            .filter(|lease| lease.check().is_ok())
            .is_none_or(|lease| lease.physical_muted != Some(false) && !lease.no_slide)
    })
}

fn read_privacy_muted(bus: &Bus) -> bool {
    match bus.latest(lamp_msgs::MicrophonePrivacy::TOPIC) {
        Ok(message) => privacy_muted_in(message),
        // A failed read is not the deliberate no-publisher compatibility case.
        Err(_) => true,
    }
}

/// The skill's claim on the night's sleep (#940), if `message` is a valid
/// one from lamp-skills said within [`lamp_msgs::EXPRESSION_SLEEP_CLAIM_FRESH_US`]
/// of `now_us` on this host's monotonic clock. Anything else is no claim.
pub fn sleep_claim_in(
    message: Option<lamp_bus::Message>,
    now_us: u64,
) -> Option<lamp_msgs::ExpressionSleepClaim> {
    use lamp_msgs::{EXPRESSION_SLEEP_CLAIM_FRESH_US, ExpressionSleepClaim};
    let message = message?;
    if message.header.source != ExpressionSleepClaim::SOURCE
        || message.header.at_us > now_us.saturating_add(1_000_000)
        || now_us.saturating_sub(message.header.at_us) >= EXPRESSION_SLEEP_CLAIM_FRESH_US
    {
        return None;
    }
    message
        .parse::<ExpressionSleepClaim>()
        .ok()
        .filter(|claim| claim.check().is_ok())
}

/// The journal's line for the night's sleep as `claim` leaves it.
pub fn sleep_claim_line(claim: Option<&lamp_msgs::ExpressionSleepClaim>) -> String {
    match claim {
        // FALLBACK (#940, for one release): see `Engine::set_sleep_claim`.
        None => "no skill claims the night's sleep: Expression sleeps by its own \
                 schedule, the fallback kept for one release (#940)"
            .into(),
        Some(claim) => format!(
            "{} claims the night's sleep ({}): Expression keeps no schedule of its own",
            claim.skill,
            if claim.on {
                "on"
            } else {
                "off: a person chose that it not sleep at night"
            }
        ),
    }
}

/// How often the live service tells the planner the local time of day.
pub const DAYTIME_EVERY_US: u64 = 60_000_000;
const DRAIN_LIMIT: usize = 32;
pub const REST_REQUESTS_ENVIRONMENT: &str = "LAMP_REST_REQUESTS";

pub fn rest_requests_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// A glance when someone in a busy room said something about the lamp and
/// the voice said nothing (#4175). Off unless set.
pub const REMARK_GLANCE_ENVIRONMENT: &str = "LAMP_REMARK_GLANCE";

pub fn remark_glance_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Preserve the source's monotonic stamp. The header binds the generic skill
/// and publisher incarnation; the engine checks clock, target and lease age.
pub fn rest_from_message(
    message: &Message,
    wall_ms: u64,
) -> io::Result<lamp_msgs::ExpressionRestRequest> {
    use lamp_msgs::ExpressionRestRequest;
    if message.header.topic != ExpressionRestRequest::TOPIC
        || message.header.type_name.as_deref() != Some(ExpressionRestRequest::TYPE)
        || message.header.kind != lamp_bus::Kind::Event
        || message.expired
        || message.header.expired_at(wall_ms)
        || message.header.at_unix_ms > wall_ms.saturating_add(50)
        || message.payload.len() > 4096
    {
        return Err(invalid("invalid or expired rest transport"));
    }
    let request: ExpressionRestRequest = message
        .parse()
        .map_err(|error| invalid(error.to_string()))?;
    request
        .check()
        .map_err(|error| invalid(error.to_string()))?;
    if message.header.source != ExpressionRestRequest::SOURCE
        || message.header.boot != request.skill_session_id
        || message.header.seq != request.stamp.sequence
    {
        return Err(invalid(
            "rest publisher identity or sequence differs from its payload",
        ));
    }
    Ok(request)
}

fn invalid(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.into())
}

/// The host boot identity is independent of a publisher's process identity.
/// On non-Linux development hosts the caller must supply a test identity.
pub fn clock_id(given: Option<&str>) -> io::Result<String> {
    let id = match given {
        Some(id) => id.to_owned(),
        None => fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("read boot clock identity (use --clock-id on a test host): {error}"),
                )
            })?
            .trim()
            .to_owned(),
    };
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
    {
        return Err(invalid(
            "clock identity must be 1..128 letters, digits, '_', '-' or '.'",
        ));
    }
    Ok(id)
}

/// The local minute of the day at `unix_ms`, `utc_offset_minutes` east of
/// UTC (420 for the lamps in Vietnam, which keep no summer time).
pub fn minute_of_day(unix_ms: u64, utc_offset_minutes: i32) -> u16 {
    let minutes = (unix_ms / 60_000) as i64 + i64::from(utc_offset_minutes);
    minutes.rem_euclid(1_440) as u16
}

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            elapsed.as_millis().min(u64::MAX as u128) as u64
        })
}

/// Adapt older topics with millisecond wall-clock age, never their private
/// `header.at_us`. New timing messages keep their explicit monotonic stamps.
pub fn input_from_message(
    message: &Message,
    now_us: u64,
    now_unix_ms: u64,
    clock: &str,
    sequence: u64,
) -> io::Result<Input> {
    let topic = message.header.topic.as_str();
    let expected = INPUTS
        .iter()
        .chain(&HEAD_INPUTS)
        .chain(&REMARK_INPUTS)
        .find(|(name, _)| *name == topic)
        .ok_or_else(|| invalid(format!("unsupported expression input {topic}")))?
        .1;
    if message.header.type_name.as_deref() != Some(expected) {
        return Err(invalid(format!(
            "{topic} requires declared type {expected}"
        )));
    }
    if message.expired || message.header.expired_at(now_unix_ms) {
        return Err(invalid(format!("expired {topic}")));
    }
    let persistent_setting = topic == "light/command" || topic == Concentration::TOPIC;
    if !persistent_setting && message.header.at_unix_ms > now_unix_ms.saturating_add(50) {
        return Err(invalid(format!("future wall timestamp on {topic}")));
    }
    let age_ms = now_unix_ms.saturating_sub(message.header.at_unix_ms);
    let limit_ms = match topic {
        JointState::TOPIC => 500,
        Faces::TOPIC => 2_000,
        // Said every second and kept three (#711).
        VoiceMute::TOPIC => 3_000,
        "light/command" => 10_000,
        _ => 2_000,
    };
    if !persistent_setting && age_ms > limit_ms {
        return Err(invalid(format!("stale {topic}: {age_ms} ms")));
    }
    let stamp = ClockStamp {
        clock_id: clock.to_owned(),
        // A persistent setting was loaded now; it is not an observation
        // whose original age establishes authority over a live action.
        at_us: if persistent_setting {
            now_us
        } else {
            now_us.saturating_sub(age_ms.saturating_mul(1_000))
        },
        sequence,
    };
    Ok(match topic {
        VoiceTiming::TOPIC => Input::Voice(message.parse()?),
        VoiceProgress::TOPIC => Input::Progress(message.parse()?),
        PersonActivity::TOPIC => Input::Activity(message.parse()?),
        VoicePlayout::TOPIC => Input::Playout(message.parse()?),
        VoiceSound::TOPIC => Input::Sound(message.parse()?),
        Concentration::TOPIC => Input::Concentration(message.parse()?),
        JointState::TOPIC => Input::Joints {
            stamp,
            value: message.parse()?,
        },
        Faces::TOPIC => Input::Faces {
            stamp,
            value: message.parse()?,
        },
        VoiceState::TOPIC => Input::VoiceState {
            stamp,
            value: message.parse()?,
        },
        RoomState::TOPIC => Input::Room {
            stamp,
            value: message.parse()?,
        },
        "light/command" => Input::Light {
            stamp,
            value: message.parse()?,
        },
        "voice/heard" => Input::Heard {
            stamp,
            value: message.parse()?,
        },
        "voice/said" => Input::Said {
            stamp,
            value: message.parse()?,
        },
        NameHeard::TOPIC => Input::Name {
            stamp,
            value: message.parse()?,
        },
        SuddenSound::TOPIC => Input::Sudden {
            stamp,
            value: message.parse()?,
        },
        ControlGesture::TOPIC => Input::Touch {
            stamp,
            value: message.parse()?,
        },
        ScheduleDue::TOPIC => Input::Due {
            stamp,
            value: message.parse()?,
        },
        TouchEvent::TOPIC => Input::HeadTouch {
            stamp,
            value: message.parse()?,
        },
        TouchState::TOPIC => Input::HeadHand {
            stamp,
            value: message.parse()?,
        },
        VoiceMute::TOPIC => Input::VoiceMute {
            stamp,
            value: message.parse()?,
        },
        VoiceRemarked::TOPIC => Input::Remarked {
            stamp,
            value: message.parse()?,
        },
        _ => unreachable!("INPUTS, HEAD_INPUTS, REMARK_INPUTS and this match list the same topics"),
    })
}

/// What Mood's last state says at `now_us` (#1670): the lamp's one breath's
/// phase, and how lively the idle layer's moves are. Both `None` for no
/// state, or one from anyone but Mood, not valid, before its stamp or past
/// its 2.5 s ([`lamp_msgs::MoodState::fresh_in`]).
pub fn mood_in(message: Option<&lamp_bus::Message>, now_us: u64) -> (Option<f64>, Option<f32>) {
    let Some(mood) = message.and_then(|message| lamp_msgs::MoodState::fresh_in(message, now_us))
    else {
        return (None, None);
    };
    (
        mood.breath_at(now_us).map(|(phase, _)| phase),
        Some(mood.liveliness()),
    )
}

/// The journal's line when the idle layer takes Mood's liveliness: only from
/// a fresh state, so with Mood off it says nothing (`lamp_msgs::MoodTaken`).
pub fn liveliness_line(
    said: &mut lamp_msgs::MoodTaken,
    liveliness: Option<f32>,
    now_us: u64,
) -> Option<String> {
    let Some(liveliness) = liveliness else {
        said.gone();
        return None;
    };
    said.due(liveliness, now_us)
        .then(|| format!("lamp-expression: Mood's liveliness {liveliness:.2}"))
}

/// One owner, held by the OS even when the process is killed. The file is
/// deliberately not unlinked: another process may already be waiting on it.
pub fn owner_lock(root: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o660)
        .open(root.join("expression.lock"))?;
    file.try_lock().map_err(|error| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("expression owner lock: {error}"),
        )
    })?;
    Ok(file)
}

/// How the planner is, for the lamp's health: a warning only for what is
/// wrong with the planner or what it needs and lacks. What it notes about
/// one turn (an answer that came at short notice, so the look back was
/// shortened) is said in the message and is not a warning: such a note is
/// about a gesture, and a lamp whose gesture was shortened is well.
pub fn standing(counts: &Counts, fault: Option<&str>, state: &ExpressionState) -> (Level, String) {
    let noted = |message: &str| {
        if state.warnings.is_empty() {
            message.to_owned()
        } else {
            format!("{message}; noted: {}", state.warnings.join(", "))
        }
    };
    if counts.input_errors > 0 || counts.output_errors > 0 {
        (
            Level::Warn,
            "bus operations have failed; see error counters".to_owned(),
        )
    } else if counts.records_lost > 0 {
        (
            Level::Warn,
            "output record is incomplete; see records_lost".to_owned(),
        )
    } else if let Some(fault) = fault {
        (Level::Warn, fault.to_owned())
    } else if !state.enabled {
        (Level::Ok, noted("shadow mode; no actuator requests"))
    } else if state.missing.iter().any(|item| !waits_within_a_turn(item)) {
        (
            Level::Warn,
            format!("waiting for {}", state.missing.join(", ")),
        )
    } else if !state.missing.is_empty() {
        (
            Level::Ok,
            noted(&format!(
                "planning; this turn waits for {}",
                state.missing.join(", ")
            )),
        )
    } else {
        (
            Level::Ok,
            noted("planning expression; hardware execution is not confirmed"),
        )
    }
}

/// What a turn waits for in its ordinary course: the first sound of an
/// answer not yet scheduled, the level of sound not yet heard. Neither is
/// something the planner lacks, and neither is a warning for the lamp. On
/// lamp-4ace on 2026-10-14, acting in a talkative office where most turns
/// get no answer, "waiting for scheduled first audio" had the lamp report
/// a warning most of the time.
fn waits_within_a_turn(item: &str) -> bool {
    matches!(
        item,
        "scheduled first audio"
            | "current actual playout level"
            | "fresh current or recent playout level"
    )
}

#[derive(Default)]
pub struct Counts {
    pub accepted: u64,
    pub refused: u64,
    pub input_errors: u64,
    pub output_errors: u64,
    pub dropped: u64,
    pub steps: u64,
    pub longest_step_us: u64,
    pub records_lost: u64,
}

pub struct Service {
    engine: Engine,
    clock: String,
    inputs: Vec<Subscriber>,
    body: Publisher,
    light: Publisher,
    state: Publisher,
    diagnostic: Publisher,
    sequence: u64,
    last_report: Option<u64>,
    last_state: Option<u64>,
    local: Option<crate::local_time::LocalTime>,
    last_daytime: Option<u64>,
    /// Asked before each time of day is given: only a clock it says is
    /// set gives one (`--kernel-clock`, #830). None: every clock does.
    clock_set: Option<fn() -> Option<bool>>,
    /// Reads `lamp/online` once a second for the listening face
    /// (`--offline-face`, #830), and when it last did.
    online: Option<(Bus, Option<u64>)>,
    /// Mood's last state, read four times a second, for the lamp's one
    /// breath (#1670), and when it was last looked for.
    mood: Option<(Bus, Option<u64>, Option<lamp_bus::Message>)>,
    /// The liveliness last said in the journal (`lamp_msgs::MoodTaken`).
    mood_said: lamp_msgs::MoodTaken,
    /// With `--head-touch`, where `voice/privacy` is read at each request
    /// from the head: a closed privacy switch is no look up (#711).
    privacy: Option<Bus>,
    /// The clock was last found not set: said once until it is.
    clock_unset: bool,
    /// The keeper's pause (`lamp/pause`), read as the body reads it: paused,
    /// nothing is asked of the body or the light.
    pause: lamp_body::pause_in::PauseIn,
    paused: bool,
    rest: Option<(Subscriber, Publisher)>,
    /// With rest requests, `expression/sleep_claim` read once a second
    /// (#940): when it was last read, and the claim last said in the journal.
    sleep_claim: Option<(
        Bus,
        Option<u64>,
        Option<Option<lamp_msgs::ExpressionSleepClaim>>,
    )>,
    /// The last rest refusal said in the journal, so a renewal refused for
    /// the same reason is not said again.
    rest_refusal_said: Option<String>,
    unplug: Option<crate::unplug::Unplug>,
    pub counts: Counts,
    _lock: File,
}

impl Service {
    pub fn new(bus: &Bus, catalogue: Catalogue, enabled: bool, clock: &str) -> io::Result<Self> {
        Self::new_with_unplug(bus, catalogue, enabled, clock, lamp_body::unplug::enabled())
    }

    fn new_with_unplug(
        bus: &Bus,
        catalogue: Catalogue,
        enabled: bool,
        clock: &str,
        unplug: bool,
    ) -> io::Result<Self> {
        let resting = catalogue.face_levels.resting;
        let resumable = catalogue.resumable_voice;
        // Make the root using the bus's directory permissions before locking.
        let state = bus.publisher_declared(
            ExpressionState::TOPIC,
            &ExpressionState::declaration(),
            "expression",
        )?;
        let lock = owner_lock(bus.root())?;
        let engine = Engine::new(catalogue, enabled, clock)
            .and_then(|engine| engine.with_session_id(state.boot().to_owned()))
            .map_err(|error| invalid(error.to_string()))?;
        Ok(Self {
            engine,
            clock: clock.to_owned(),
            inputs: INPUTS
                .iter()
                // A process selects one complete protocol. The other Voice
                // publications can continue serving legacy consumers without
                // becoming input refusals here or acquiring mixed authority.
                .filter(|(topic, _)| match *topic {
                    VoiceTiming::TOPIC | VoicePlayout::TOPIC => !resumable,
                    VoiceProgress::TOPIC | VoiceSound::TOPIC => resumable,
                    _ => true,
                })
                .map(|(topic, kind)| bus.subscribe_typed(topic, "expression", kind))
                .collect::<io::Result<_>>()?,
            body: bus.publisher_declared(
                ExpressionBody::TOPIC,
                &ExpressionBody::declaration(),
                "expression",
            )?,
            light: bus.publisher_declared(
                ExpressionLight::TOPIC,
                &ExpressionLight::declaration(),
                "expression",
            )?,
            diagnostic: bus.publisher_declared(
                Diagnostic::TOPIC,
                &Diagnostic::declaration(),
                "expression",
            )?,
            state,
            sequence: 0,
            last_report: None,
            last_state: None,
            local: None,
            last_daytime: None,
            clock_set: None,
            online: None,
            mood: None,
            mood_said: lamp_msgs::MoodTaken::default(),
            privacy: None,
            clock_unset: false,
            pause: lamp_body::pause_in::PauseIn::on(bus.clone()),
            paused: false,
            rest: None,
            sleep_claim: None,
            rest_refusal_said: None,
            unplug: unplug
                .then(|| crate::unplug::Unplug::new(bus, resting))
                .transpose()?,
            counts: Counts::default(),
            _lock: lock,
        })
    }

    /// Opt-in intake only; Expression retains its existing actuator writers.
    /// Manual sleep is followed even when the optional head animation is off.
    pub fn with_rest_requests(mut self, bus: &Bus) -> io::Result<Self> {
        use lamp_msgs::{ExpressionRestRequest, ExpressionRestState};
        if self.rest.is_some() {
            return Ok(self);
        }
        self.rest = Some((
            bus.subscribe_typed(
                ExpressionRestRequest::TOPIC,
                "expression",
                ExpressionRestRequest::TYPE,
            )?,
            bus.publisher_declared(
                ExpressionRestState::TOPIC,
                &ExpressionRestState::declaration(),
                "expression",
            )?,
        ));
        if !self
            .inputs
            .iter()
            .any(|input| input.topic() == VoiceMute::TOPIC)
        {
            self.inputs.push(bus.subscribe_typed(
                VoiceMute::TOPIC,
                "expression",
                VoiceMute::TYPE,
            )?);
        }
        self.privacy = Some(bus.clone());
        self.sleep_claim = Some((bus.clone(), None, None));
        self.engine = self.engine.with_rest_requests();
        Ok(self)
    }

    /// Also read the head's requests and state from `--touch` Controls
    /// (#711): a hand kept on the head leans the lamp into it, a hold puts it
    /// to sleep and a tap wakes it. Where the voice says its mute
    /// (`voice/mute`), sleep follows the voice instead. Off unless asked for.
    pub fn with_head_touch(mut self, bus: &Bus) -> io::Result<Self> {
        for (topic, kind) in HEAD_INPUTS {
            self.inputs
                .push(bus.subscribe_typed(topic, "expression", kind)?);
        }
        self.privacy = Some(bus.clone());
        Ok(self)
    }

    /// Also read the voice's remarks about the lamp (`voice/remarked`,
    /// #4175), for the glance at a remark the catalogue was given. Off unless
    /// `LAMP_REMARK_GLANCE=on`.
    pub fn with_remark_glance(mut self, bus: &Bus) -> io::Result<Self> {
        for (topic, kind) in REMARK_INPUTS {
            self.inputs
                .push(bus.subscribe_typed(topic, "expression", kind)?);
        }
        Ok(self)
    }

    /// Tell the planner the local time of day, from the wall clock this many
    /// minutes east of UTC, at once and every minute: sleep comes only at
    /// night (docs/alive.md).
    pub fn with_local_time(self, utc_offset_minutes: i32) -> Self {
        self.with_local(crate::local_time::LocalTime::Offset(utc_offset_minutes))
    }

    /// As [`Self::with_local_time`], with a zone whose offset is taken at each
    /// moment (#820), or a fixed offset.
    pub fn with_local(mut self, local: crate::local_time::LocalTime) -> Self {
        self.local = Some(local);
        self
    }

    /// Breathes the head with the lamp's one breath where Mood keeps it,
    /// and makes the idle layer's moves livelier or quieter as Mood says
    /// (#1670, `mood/state`): the same phase the ring's resting breath takes
    /// ([`lamp_msgs::MoodState::breath_in`]). With no fresh mood, the plain
    /// breath on the boot clock, as before.
    pub fn with_mood_breath(mut self, bus: &Bus) -> Self {
        self.mood = Some((bus.clone(), None, None));
        self
    }

    /// Draws listening as not ready while `lamp/online` says the internet
    /// cannot be reached (#830). A value gone stale, or none, is online.
    pub fn with_offline_face(mut self, bus: &Bus) -> Self {
        self.online = Some((bus.clone(), None));
        self
    }

    /// Gives the time of day only while `set` says the clock is set. With
    /// none given the lamp knows no night, so it does not fall asleep or
    /// wake by a wrong clock (#830, `docs/offline.md`).
    pub fn with_clock_check(mut self, set: fn() -> Option<bool>) -> Self {
        self.clock_set = Some(set);
        self
    }

    /// The clock is injected here too: tests can use real sockets without
    /// sleeping through any behavior. Each valid topic has a bounded budget.
    pub fn step(&mut self, now: impl Fn() -> u64, wall_ms: u64) -> Output {
        let began = now();
        if self.rest.is_some() {
            self.engine.set_rest_paused(began, self.pause.paused());
            if let Some(bus) = &self.privacy {
                self.engine.set_privacy_muted(read_privacy_muted(bus));
            }
        }
        if let Some((bus, looked, said)) = self.sleep_claim.as_mut()
            && looked.is_none_or(|at| began.saturating_sub(at) >= 1_000_000)
        {
            *looked = Some(began);
            let claim = sleep_claim_in(
                bus.latest(lamp_msgs::ExpressionSleepClaim::TOPIC)
                    .ok()
                    .flatten(),
                began,
            );
            if said.as_ref() != Some(&claim) {
                eprintln!("lamp-expression: {}", sleep_claim_line(claim.as_ref()));
                *said = Some(claim.clone());
            }
            self.engine.set_sleep_claim(began, claim.as_ref());
        }
        if let Some((bus, looked)) = self.online.as_mut()
            && looked.is_none_or(|at| began.saturating_sub(at) >= 1_000_000)
        {
            *looked = Some(began);
            let offline = offline_in(bus.latest(lamp_msgs::Online::TOPIC).ok().flatten());
            self.engine.set_offline(offline);
        }
        if let Some((bus, looked, state)) = self.mood.as_mut() {
            if looked.is_none_or(|at| began.saturating_sub(at) >= 250_000) {
                *looked = Some(began);
                // A bus that cannot be read is no mood.
                *state = bus.latest(lamp_msgs::MoodState::TOPIC).ok().flatten();
            }
            let (phase, liveliness) = mood_in(state.as_ref(), began);
            if let Some(line) = liveliness_line(&mut self.mood_said, liveliness, began) {
                eprintln!("{line}");
            }
            self.engine.set_mood_breath(phase);
            self.engine.set_mood_liveliness(liveliness);
        }
        // The zone or offset given, and a minute since the last (#820).
        let due = self.local.is_some()
            && self
                .last_daytime
                .is_none_or(|last| began.saturating_sub(last) >= DAYTIME_EVERY_US);
        if due {
            self.last_daytime = Some(began);
            let set = self.clock_set.is_none_or(|set| set() == Some(true));
            if set == self.clock_unset {
                self.clock_unset = !set;
                if !set {
                    // The last time of day given is not to be carried on
                    // for its two minutes of freshness either.
                    self.engine.forget_daytime();
                }
                eprintln!(
                    "lamp-expression: {}",
                    if set {
                        "the clock is set: the time of day is known again"
                    } else {
                        "the clock is not set: no time of day, so no night"
                    }
                );
            }
        }
        if due
            && !self.clock_unset
            && let Some(local) = &self.local
        {
            self.sequence = self.sequence.saturating_add(1);
            let daytime = Input::DayTime {
                stamp: ClockStamp {
                    clock_id: self.clock.clone(),
                    at_us: began,
                    sequence: self.sequence,
                },
                minute: local.minute_of_day(wall_ms),
            };
            match self.engine.ingest(began, daytime) {
                Ok(()) => self.counts.accepted += 1,
                Err(why) => {
                    self.counts.refused += 1;
                    eprintln!("lamp-expression: refused the time of day: {why}");
                }
            }
        }
        for input in &mut self.inputs {
            for _ in 0..DRAIN_LIMIT {
                let message = match input.recv(Duration::ZERO) {
                    Ok(Some(message)) => message,
                    Ok(None) => break,
                    Err(_) => {
                        self.counts.input_errors += 1;
                        break;
                    }
                };
                self.sequence = self.sequence.saturating_add(1);
                // A request from the head is weighed with the privacy
                // switch as it is now (#711).
                if message.header.topic == TouchEvent::TOPIC
                    && let Some(bus) = &self.privacy
                {
                    self.engine.set_privacy_muted(read_privacy_muted(bus));
                }
                let received = now();
                // The wall sample belongs to `began`, not this later receive.
                // Slow draining must age observations rather than renew them.
                let observed_wall_ms =
                    wall_ms.saturating_add(received.saturating_sub(began) / 1_000);
                let parsed = input_from_message(
                    &message,
                    received,
                    observed_wall_ms,
                    &self.clock,
                    self.sequence,
                );
                let accepted = parsed.and_then(|value| {
                    self.engine
                        .ingest(received, value)
                        .map_err(|error| invalid(error.to_string()))
                });
                match accepted {
                    Ok(()) => self.counts.accepted += 1,
                    Err(why) => {
                        self.counts.refused += 1;
                        // Said for the first few and then ever more
                        // seldom: which input, and why, is how a fault
                        // in whoever publishes it is found on a lamp.
                        let refused = self.counts.refused;
                        if refused <= 5 || refused.is_power_of_two() {
                            eprintln!(
                                "lamp-expression: refused an input ({refused} so far): {why}"
                            );
                        }
                    }
                }
            }
        }
        if let Some((requests, _)) = &mut self.rest {
            for _ in 0..DRAIN_LIMIT {
                let message = match requests.recv(Duration::ZERO) {
                    Ok(Some(message)) => message,
                    Ok(None) => break,
                    Err(_) => {
                        self.counts.input_errors += 1;
                        break;
                    }
                };
                let received = now();
                self.engine.set_rest_paused(received, self.pause.paused());
                if let Some(bus) = &self.privacy {
                    self.engine.set_privacy_muted(read_privacy_muted(bus));
                }
                let observed_wall = wall_ms.saturating_add(received.saturating_sub(began) / 1_000);
                match rest_from_message(&message, observed_wall).and_then(|request| {
                    self.engine
                        .ingest_rest_with_gap(received, request, message.gap > 0)
                        .map_err(|error| invalid(error.to_string()))
                }) {
                    Ok(()) => self.counts.accepted += 1,
                    Err(why) => {
                        self.counts.refused += 1;
                        // A skill renews every second: say a reason once, and
                        // again only when it changes, as row 7 on lamp-06c1
                        // had a refusal and no cause.
                        let why = why.to_string();
                        if self.rest_refusal_said.as_deref() != Some(why.as_str()) {
                            eprintln!(
                                "lamp-expression: refused a rest ({} so far): {why}",
                                self.counts.refused
                            );
                            self.rest_refusal_said = Some(why);
                        }
                    }
                }
            }
        }
        // Read privacy again at the final planning boundary.
        if (self.engine.looking_up() || self.rest.is_some())
            && let Some(bus) = &self.privacy
        {
            self.engine.set_privacy_muted(read_privacy_muted(bus));
        }
        let at_us = now();
        // Paused by the keeper: no new request to the body or the light;
        // those already sent run out with their leases.
        let paused = self.pause.paused();
        self.engine.set_rest_paused(at_us, paused);
        let mut output = self.engine.tick(at_us);
        if paused != self.paused {
            self.paused = paused;
            eprintln!(
                "lamp-expression: {}",
                if paused {
                    "paused by the keeper: nothing is asked of the body or the light"
                } else {
                    "resumed by the keeper"
                }
            );
        }
        if paused {
            output.light = None;
            output.body = None;
        }
        let unplug_request = self.unplug.as_mut().and_then(|unplug| {
            unplug.apply(
                &mut output,
                at_us,
                wall_ms.saturating_add(at_us.saturating_sub(began) / 1_000),
            )
        });
        // Publish face first. Consumers still enforce lease/time/ownership;
        // datagram order between topics is not a physical timing guarantee.
        let published = output
            .light
            .as_ref()
            .map(|light| self.light.publish_json(at_us, light));
        if published.as_ref().is_none_or(Result::is_ok)
            && let (Some(unplug), Some(request)) = (&mut self.unplug, unplug_request)
        {
            account(&mut self.counts, unplug.acknowledge(at_us, &request));
        }
        if let Some(published) = published {
            account(&mut self.counts, published);
        }
        if let Some(body) = &output.body {
            account(&mut self.counts, self.body.publish_json(at_us, body));
        }
        // Publish the current actuator requests before journal I/O. A slow
        // journal must not hold an already-stamped command behind a notice.
        for notice in self.engine.take_sleep_name_notices() {
            eprintln!("lamp-expression: {notice}");
        }
        if let Some(why) = self.engine.take_rest_ended() {
            eprintln!("lamp-expression: ended a skill's rest: {why}");
        }
        if self
            .last_state
            .is_none_or(|last| at_us.saturating_sub(last) >= 200_000)
        {
            if let Some((_, receipt)) = &mut self.rest
                && let Some(rest) = self.engine.rest_state(at_us)
            {
                account(&mut self.counts, receipt.publish_json(at_us, &rest));
            }
            account(
                &mut self.counts,
                self.state.publish_json(at_us, &output.state),
            );
            self.last_state = Some(at_us);
        }
        if self
            .last_report
            .is_none_or(|last| at_us.saturating_sub(last) >= 1_000_000)
        {
            let report = self.report(&output);
            account(
                &mut self.counts,
                self.diagnostic.publish_json(at_us, &report),
            );
            for publisher in [&mut self.body, &mut self.light] {
                if publisher.alive().is_err() {
                    self.counts.output_errors += 1;
                }
            }
            self.last_report = Some(at_us);
        }
        self.counts.steps += 1;
        self.counts.longest_step_us = self.counts.longest_step_us.max(now().saturating_sub(began));
        output
    }

    fn report(&self, output: &Output) -> Diagnostic {
        let (level, message) = standing(&self.counts, self.engine.fault(), &output.state);
        let mut report = Diagnostic::new(level, "expression", &message);
        for (key, value) in [
            ("accepted", self.counts.accepted),
            ("refused", self.counts.refused),
            ("input_errors", self.counts.input_errors),
            ("output_errors", self.counts.output_errors),
            ("dropped", self.counts.dropped),
            ("steps", self.counts.steps),
            ("longest_step_us", self.counts.longest_step_us),
            ("records_lost", self.counts.records_lost),
        ] {
            report.values.push(lamp_msgs::KeyValue {
                key: key.into(),
                value: value.to_string(),
            });
        }
        report
    }
}

fn account(counts: &mut Counts, result: io::Result<lamp_bus::Published>) {
    match result {
        Ok(sent) => counts.dropped += sent.dropped as u64,
        Err(_) => counts.output_errors += 1,
    }
}

/// Optional bounded output record. A slow/full disk never holds the engine.
/// On shutdown the detached writer is not awaited; pending records may be lost.
pub struct Recorder {
    sender: SyncSender<crate::replay::TickRecord>,
    pub lost: Arc<AtomicU64>,
}

impl Recorder {
    pub fn new(path: &Path, max_bytes: u64) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        let (sender, receiver) = sync_channel::<crate::replay::TickRecord>(16);
        let lost = Arc::new(AtomicU64::new(0));
        let worker_lost = Arc::clone(&lost);
        thread::Builder::new()
            .name("expression_record".into())
            .spawn(move || {
                let mut written = 0_u64;
                let mut failed = false;
                for record in receiver {
                    if failed {
                        worker_lost.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    let result = (|| -> io::Result<()> {
                        let mut bytes = serde_json::to_vec(&record)?;
                        bytes.push(b'\n');
                        if written.saturating_add(bytes.len() as u64) > max_bytes {
                            return Err(io::Error::other(
                                "expression record reached its size limit",
                            ));
                        }
                        file.write_all(&bytes)?;
                        written += bytes.len() as u64;
                        Ok(())
                    })();
                    if result.is_err() {
                        failed = true;
                        worker_lost.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })?;
        Ok(Self { sender, lost })
    }

    pub fn record(&self, at_us: u64, output: Output) {
        match self
            .sender
            .try_send(crate::replay::TickRecord { at_us, output })
        {
            Ok(()) => {}
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.lost.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod unplug_tests {
    use super::*;
    use lamp_bus::Kind;
    use lamp_msgs::{ExpressionFace, Voice, VoiceEvent, VoiceTiming};

    struct Fixture {
        root: std::path::PathBuf,
        bus: Bus,
        service: Service,
    }
    impl Fixture {
        fn new(enabled: bool, unplug: bool) -> Self {
            Self::with(enabled, unplug, Catalogue::builtin(), false)
        }
        /// `light_before`: the light was set before Expression started, as
        /// on a lamp whose light was last changed long ago.
        fn with(enabled: bool, unplug: bool, catalogue: Catalogue, light_before: bool) -> Self {
            Self::with_initial_voice(enabled, unplug, catalogue, light_before, true)
        }
        fn with_initial_voice(
            enabled: bool,
            unplug: bool,
            catalogue: Catalogue,
            light_before: bool,
            initial_voice: bool,
        ) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::path::Path::new("/tmp").join(format!(
                "ex-u-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let bus = Bus::new(&root).unwrap();
            let light = || {
                bus.publisher_declared(Light::TOPICS[0], &Light::declaration(), "test")
                    .unwrap()
                    .publish_json(1, &Light::new(true, "warm", 2).unwrap())
                    .unwrap();
            };
            if light_before {
                light();
            }
            let service =
                Service::new_with_unplug(&bus, catalogue, enabled, "test", unplug).unwrap();
            if !light_before {
                light();
            }
            if initial_voice {
                let mut voice = bus
                    .publisher_declared(VoiceTiming::TOPIC, &VoiceTiming::declaration(), "voice")
                    .unwrap();
                Self::publish_speaking(&mut voice);
            }
            Self { root, bus, service }
        }
        fn publish_speaking(voice: &mut Publisher) {
            voice
                .publish_json(
                    1,
                    &VoiceTiming {
                        stamp: ClockStamp {
                            clock_id: "test".into(),
                            at_us: 1_000_000,
                            sequence: 1,
                        },
                        session_id: "voice".into(),
                        turn: 1,
                        request: 1,
                        state: Voice::Speaking,
                        accepting_input: false,
                        event: VoiceEvent::State,
                        first_audio_us: None,
                    },
                )
                .unwrap();
        }
        fn say(&self, topic: &str, value: serde_json::Value) {
            let source = if topic == lamp_body::unplug::STATE_TOPIC {
                "body"
            } else {
                "test"
            };
            let mut publisher = self.bus.publisher(topic, Kind::Latest, source).unwrap();
            publisher.expires_after(Some(Duration::from_millis(500)));
            publisher.publish_json(1_000_000, &value).unwrap();
        }
        fn request(&self, phase: &str, request: &str) {
            self.say(
                lamp_body::unplug::REQUEST_TOPIC,
                serde_json::json!({"request":"r1"}),
            );
            self.say(
                lamp_body::unplug::STATE_TOPIC,
                serde_json::json!({"request":request,"phase":phase,"why":""}),
            );
        }
        fn ack(&self) -> bool {
            self.bus
                .latest("expression/unplug_state")
                .unwrap()
                .is_some()
        }
        fn step(&mut self, wall: u64) -> Output {
            self.service.step(|| 1_000_000, wall)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn ready_presents_sleep_even_while_voice_speaks_then_acknowledges() {
        // This case's single speaking observation follows all Ready setup.
        let mut fixture =
            Fixture::with_initial_voice(true, true, Catalogue::builtin(), false, false);
        let mut voice = fixture
            .bus
            .publisher_declared(VoiceTiming::TOPIC, &VoiceTiming::declaration(), "voice")
            .unwrap();
        fixture.request("ready", "r1");
        Fixture::publish_speaking(&mut voice);
        let output = fixture.step(unix_ms());
        assert_eq!(output.light.unwrap().face, ExpressionFace::Resting);
        assert!(output.state.behaviours.contains(&"asleep".into()));
        assert!(output.body.is_none());
        let ack = fixture
            .bus
            .latest("expression/unplug_state")
            .unwrap()
            .unwrap();
        assert_eq!(ack.header.expires_ms, Some(500));
        assert_eq!(
            ack.parse::<serde_json::Value>().unwrap(),
            serde_json::json!({"request":"r1","asleep":true})
        );
    }

    /// lamp-52e6 on 2026-10-20 (#1699 try 3): the body said Ready and
    /// Expression never acknowledged asleep. Here everything comes as the
    /// lamp's services send it: its catalogue, declared publishers, live
    /// clocks, a light set before Expression started, and a voice whose
    /// last turn was cancelled or that says nothing fresh.
    #[test]
    fn body_ready_from_the_real_publishers_is_acknowledged_asleep() {
        for voice in [
            Some(VoiceEvent::State),
            Some(VoiceEvent::Cancelled),
            Some(VoiceEvent::Finished),
            None,
        ] {
            let catalogue = Catalogue::from_json(include_str!(
                "../../../deploy/lamp-4ace/expression-alive-lamp-4ace.json"
            ))
            .unwrap();
            let mut fixture = Fixture::with(true, true, catalogue, true);
            let bus = fixture.bus.clone();
            let now = lamp_ipc::monotonic_us();
            if let Some(event) = voice {
                bus.publisher_declared(
                    lamp_msgs::VoiceProgress::TOPIC,
                    &lamp_msgs::VoiceProgress::declaration(),
                    "voice",
                )
                .unwrap()
                .publish_json(
                    now,
                    &lamp_msgs::VoiceProgress {
                        timing: VoiceTiming {
                            stamp: ClockStamp {
                                clock_id: "test".into(),
                                at_us: now,
                                sequence: 2,
                            },
                            session_id: "voice".into(),
                            turn: 1,
                            request: 1,
                            state: Voice::Resting,
                            accepting_input: false,
                            event,
                            first_audio_us: None,
                        },
                        audio_generation: 1,
                        held: false,
                    },
                )
                .unwrap();
            }
            bus.publisher_declared(
                lamp_body::unplug::Request::TOPIC,
                &lamp_body::unplug::Request::declaration(),
                "skills",
            )
            .unwrap()
            .publish_json(now, &serde_json::json!({"request": "boot-1"}))
            .unwrap();
            bus.publisher_declared(
                lamp_body::unplug::State::TOPIC,
                &lamp_body::unplug::State::declaration(),
                "body",
            )
            .unwrap()
            .publish_json(
                lamp_ipc::monotonic_us(),
                &serde_json::json!({"request": "boot-1", "phase": "ready", "why": ""}),
            )
            .unwrap();
            let output = fixture.service.step(lamp_ipc::monotonic_us, unix_ms());
            assert!(fixture.ack(), "{voice:?} {:?}", output.state);
            assert!(output.body.is_none());
            assert!(output.state.behaviours.contains(&"asleep".into()));
            if let Some(light) = output.light {
                assert_eq!(light.face, ExpressionFace::Resting);
            }
        }
    }

    #[test]
    fn switch_off_shadow_failed_mismatch_and_stale_never_acknowledge() {
        for (enabled, unplug, phase, request, age) in [
            (true, false, "ready", "r1", 0),
            (false, true, "ready", "r1", 0),
            (true, true, "failed", "r1", 0),
            (true, true, "folding", "r1", 0),
            (true, true, "ready", "old", 0),
            (true, true, "ready", "r1", 501),
        ] {
            let mut fixture = Fixture::new(enabled, unplug);
            fixture.request(phase, request);
            let output = fixture.step(unix_ms() + age);
            assert!(!fixture.ack(), "{enabled} {unplug} {phase} {request} {age}");
            assert!(!output.state.behaviours.contains(&"ready to unplug".into()));
            if enabled && !unplug {
                assert_eq!(output.light.unwrap().face, ExpressionFace::Speaking);
            }
        }
    }

    #[test]
    fn a_matching_skills_cancel_hands_the_body_back() {
        let mut fixture = Fixture::new(true, true);
        fixture.request("ready", "r1");
        let latched = |fixture: &Fixture| fixture.service.unplug.as_ref().unwrap().latched;
        fixture.step(unix_ms());
        assert!(latched(&fixture));
        let bus = fixture.bus.clone();
        let cancel = |source: &str, request: &str| {
            bus.publisher(lamp_body::unplug::CANCEL_TOPIC, Kind::Latest, source)
                .unwrap()
                .publish_json(1_000_000, &serde_json::json!({ "request": request }))
                .unwrap();
        };
        cancel("expression", "r1");
        cancel("skills", "r0");
        fixture.step(unix_ms());
        assert!(
            latched(&fixture),
            "foreign or other-request cancels keep the latch"
        );
        cancel("skills", "r1");
        let output = fixture.step(unix_ms());
        assert!(!latched(&fixture), "the body is handed back");
        assert!(!output.state.behaviours.contains(&"asleep".into()));
    }

    #[test]
    fn closed_privacy_and_failed_light_publication_never_acknowledge() {
        let mut fixture = Fixture::new(true, true);
        fixture.request("ready", "r1");
        fixture.say(
            lamp_msgs::MicrophonePrivacy::TOPIC,
            serde_json::json!({"physical_muted":true}),
        );
        assert!(
            !fixture
                .step(unix_ms())
                .state
                .behaviours
                .contains(&"ready to unplug".into())
        );
        assert!(!fixture.ack());

        let mut fixture = Fixture::new(true, true);
        fixture.request("ready", "r1");
        // A directory at the kept-file destination makes the atomic rename fail.
        fs::create_dir(fixture.root.join("expression/light/latest")).unwrap();
        fixture.step(unix_ms());
        assert!(fixture.service.counts.output_errors > 0);
        assert!(!fixture.ack());
    }
}
