//! The second route as a provider worker, on the ordinary provider IPC:
//! the fixture's startup readiness, authority guard and credited output
//! (`crate::fixture_provider`), around [`Engine`] instead of a recording.
//!
//! [`serve`] is the process (`lamp-voice worker second`): the character of
//! the Live route's configuration, told the same time, memory note and
//! conversation thread as a Live session is, with the same tools, and each
//! turn kept in that thread and the memory's journal as the Live route
//! keeps its own.
use super::{Engine, Net, Told, ToolRunner, net::Message, output_request};
use crate::{
    provider_flow::OutputSender,
    provider_link::{LinkContext, ProviderLink, ProviderLinkState},
    provider_worker::{ProviderInput, ProviderOutput},
    transport::{Channel, WorkerChannels},
    wire::{Control, WorkerEvent},
};
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{BootId, BoundaryGuard, MonoTime, Snapshot};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

/// The coordinator renews authority at least this often.
const CONTROL_LEASE_US: u64 = 250_000;
/// The longest a backlog left by a stall may hold the fresh authority back
/// before the controller counts as gone, as the Live worker (#784).
const STALLED_CONTROL_US: u64 = 1_500_000;
/// Control messages taken in one tick before input and output wait.
const CONTROL_SLICE: usize = 16;
/// Input commands taken in one tick, as the Live provider worker does.
const INPUT_SLICE: usize = 8;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// The fixture's authority bookkeeping: who owns the turn, which requests
/// are over, and whether the controller is still there.
struct Gate {
    guard: BoundaryGuard,
    flow: OutputSender,
    last_control_us: u64,
    highest_seen: u64,
    retired_through: u64,
    /// Authority snapshots that lapsed in the queue while this worker was
    /// held up, since the last one installed.
    skipped_expired: u32,
}

enum Drain {
    Empty,
    Deferred,
    Stopped,
}

impl Gate {
    fn new(boot: BootId, now_us: u64) -> Self {
        Self {
            guard: BoundaryGuard::new(boot, MonoTime::from_micros(now_us)),
            flow: OutputSender::default(),
            last_control_us: now_us,
            highest_seen: 0,
            retired_through: 0,
            skipped_expired: 0,
        }
    }

    /// As the fixture's `install_inner`: a newer owner retires every
    /// earlier request, no owner retires all seen, and an old owner coming
    /// back is a fault. The engine drops the retired requests' work.
    fn install(&mut self, now_us: u64, state: Snapshot, engine: &mut Engine) -> io::Result<()> {
        match self.guard.install(MonoTime::from_micros(now_us), state) {
            Ok(()) => {}
            // Queued while this worker was held up (on lamp-4ace by the
            // kernel re-tuning its SD/MMC hosts, #784; it ended the voice
            // there on 2026-10-16, #960): it lapsed in the queue. Fresher
            // ones follow it; the heartbeat in `drain` ends the route if
            // none comes.
            Err(lamp_turn_owner::Error::ExpiredState) => {
                self.skipped_expired = self.skipped_expired.saturating_add(1);
                return Ok(());
            }
            Err(error) => return Err(io::Error::other(error)),
        }
        if self.skipped_expired > 0 {
            eprintln!(
                "lamp-voice second: held up {} ms; {} lapsed authority snapshots skipped, going on",
                now_us.saturating_sub(self.last_control_us) / 1_000,
                self.skipped_expired
            );
            self.skipped_expired = 0;
        }
        self.last_control_us = now_us;
        if let Some(owner) = state.owner() {
            if owner.turn() <= self.retired_through || owner.turn() < self.highest_seen {
                return Err(invalid(
                    "second route authority resurrected a retired owner",
                ));
            }
            self.highest_seen = owner.turn();
            self.retired_through = self.retired_through.max(owner.turn() - 1);
        } else {
            self.retired_through = self.retired_through.max(self.highest_seen);
        }
        engine.retire_through(self.retired_through);
        Ok(())
    }

    /// As the fixture's `fresh`, less its finite-run clock rules: judged
    /// once the control queue is empty, so on the newest authority sent.
    fn fresh(&self, now_us: u64) -> io::Result<()> {
        if let Some(state) = self.guard.state()
            && (now_us >= state.expires_at().as_micros()
                || (state.owner().is_some()
                    && !state.listening_ready(MonoTime::from_micros(now_us))))
        {
            return Err(invalid("second route privacy or input lease expired"));
        }
        Ok(())
    }

    /// Input is taken only for the admitted owner, under the current
    /// microphone generation. Input for a retired request is dropped.
    fn admits(&self, now_us: u64, command: &ProviderInput) -> io::Result<bool> {
        let (request, privacy) = match command {
            ProviderInput::Start {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::Audio {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::End {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::Called {
                request,
                privacy_generation,
            }
            | ProviderInput::Written {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::Reminder {
                request,
                privacy_generation,
                ..
            } => (*request, *privacy_generation),
        };
        let state = self
            .guard
            .state()
            .ok_or_else(|| invalid("second route has no authority"))?;
        if request == 0
            || !state.listening_ready(MonoTime::from_micros(now_us))
            || privacy != state.microphone_generation()
        {
            return Err(invalid(
                "second route input lost privacy or retention authority",
            ));
        }
        if request <= self.retired_through {
            return Ok(false);
        }
        if state.owner().map(|owner| owner.turn()) != Some(request) {
            return Err(invalid("second route input has no matching admitted owner"));
        }
        Ok(true)
    }

    /// Output of a request goes only to its owner while input is retained.
    fn may_send(&self, now_us: u64, event: &ProviderOutput) -> io::Result<()> {
        let Some(request) = output_request(event) else {
            return Ok(());
        };
        let state = self
            .guard
            .state()
            .ok_or_else(|| invalid("second route output has no authority"))?;
        if !state.listening_ready(MonoTime::from_micros(now_us))
            || state.owner().map(|owner| owner.turn()) != Some(request)
        {
            return Err(invalid("second route output lost original owner"));
        }
        Ok(())
    }

    fn drain(&mut self, control: &mut Channel, engine: &mut Engine) -> io::Result<Drain> {
        let mut drained = Drain::Deferred;
        for _ in 0..CONTROL_SLICE {
            match control.receive()? {
                Some(Control::Stop) => return Ok(Drain::Stopped),
                Some(Control::Authority { snapshot }) => {
                    self.install(monotonic_us(), snapshot, engine)?
                }
                Some(Control::ProviderOutputCapacity { through }) => self.flow.grant(through)?,
                Some(Control::StartCapture | Control::ConnectReference { .. }) => {
                    return Err(invalid("invalid second route provider control"));
                }
                None => {
                    drained = Drain::Empty;
                    break;
                }
            }
        }
        // While a backlog left by a stall is still being read, the fresh
        // authority may be further down it: the heartbeat is judged once
        // the queue is empty, or after the longest stall a gap may be.
        let silent = monotonic_us().saturating_sub(self.last_control_us);
        if silent > STALLED_CONTROL_US
            || (matches!(drained, Drain::Empty) && silent >= CONTROL_LEASE_US)
        {
            return Err(invalid("second route controller heartbeat expired"));
        }
        Ok(drained)
    }
}

/// Where the coordinator reads whether a slow first round makes a sound
/// (sixth-P3, [`super::CUE_AFTER`]). `on` only.
pub const WAIT_CUE_ENVIRONMENT: &str = "LAMP_SECOND_WAIT_CUE";
/// The worker's flag for it, right after the second route's configuration.
pub const WAIT_CUE_FLAG: &str = "--wait-cue";

/// Whether the service's environment says `LAMP_SECOND_WAIT_CUE=on`.
pub fn wait_cue_from_environment() -> bool {
    std::env::var(WAIT_CUE_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Take a leading `--wait-cue` off the worker's flags.
pub fn take_wait_cue(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == WAIT_CUE_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Where the coordinator reads whether a turn the chat never began says
/// [`super::DOWN_LINE`]. `on` only.
pub const DOWN_LINE_ENVIRONMENT: &str = "LAMP_SECOND_DOWN_LINE";
/// The worker's flag for it, after `--wait-cue` if that is given.
pub const DOWN_LINE_FLAG: &str = "--down-line";

/// Whether the service's environment says `LAMP_SECOND_DOWN_LINE=on`.
pub fn down_line_from_environment() -> bool {
    std::env::var(DOWN_LINE_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Take a leading `--down-line` off the worker's flags.
pub fn take_down_line(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == DOWN_LINE_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Whether this route checks short sounds: as the Live route does, only
/// with `--sounds-checked` and `--attention` together.
pub fn checks_sounds(files: &crate::provider_worker::Files) -> bool {
    files.attention && files.sounds_checked
}

/// The `worker second` flags: the Live provider worker's, as
/// `coordinator::provider_arguments` passes them, parsed by the same code,
/// but for `--keep-link`: this route has no link to keep.
pub fn parse_flags(dir: &Path, flags: &[String]) -> io::Result<crate::provider_worker::Parsed> {
    let parsed = crate::provider_worker::parse_flags(dir, flags)?;
    if parsed.2.keep_link {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the second voice route has no link to keep",
        ));
    }
    Ok(parsed)
}

/// The second route's worker process until Stop. `files` are the Live
/// provider worker's flags; this route uses the thread's file and the
/// tools (the memory among them), and has no use yet for the rest.
pub fn serve(
    channels: WorkerChannels,
    boot: BootId,
    provider_config: &Path,
    second_config: &Path,
    files: crate::provider_worker::Files,
    wait_cue: bool,
    down_line: bool,
) -> io::Result<()> {
    let character = crate::config::ProviderConfig::load(provider_config)?.instruction(false)?;
    let config = crate::second_config::load_from_path(second_config)?;
    let checked = checks_sounds(&files);
    let tools = files.tools;
    if let Some(socket) = &tools.memory {
        // As the Live route starts it: each known person's memory apart.
        crate::remember::start(
            socket.clone(),
            tools.who.as_deref().map(crate::known_faces::known_file_of),
        );
    }
    let declared = tools.clone();
    let net = Net::new(config, Arc::new(move || crate::tools::declared(&declared)))
        .map_err(io::Error::other)?;
    let thread = files.thread;
    let file = thread.clone();
    let clock = tools.clock;
    // Each turn is one person's, as a Live session is (#764): whose it is,
    // counted from the eyes' known file as the Live worker counts it, opens
    // the thread for them and asks for their kept things. A Days note is
    // frozen for a session; here a session is the run of turns for one
    // person, so it is asked again only when the person changes.
    let socket = tools.memory.clone();
    let who = tools
        .who
        .as_deref()
        .map(crate::known_faces::known_file_of)
        .filter(|_| socket.is_some());
    let days = crate::remember::days_from_environment();
    // Told after the character with `LAMP_TASKS_ASKED=on`, as the Live route is (#854).
    let tasks = tools.tasks.clone();
    // What a Live session is told of the moment, with `--session-fact` and
    // `--room-facts`: the room as Gemini's worker publishes it.
    let session_fact = files.session_fact;
    let room_bus = files
        .bus_root
        .as_deref()
        .filter(|_| files.room_facts)
        .and_then(|root| lamp_bus::Bus::new(root).ok());
    let sessions = std::sync::Mutex::new(Sessions::default());
    let system = move || {
        let now = Instant::now();
        let Ok(mut sessions) = sessions.lock() else {
            return Told::default();
        };
        let (changed, person) = sessions.open(who.as_deref(), now);
        let memory = match &socket {
            Some(socket) if days => sessions.frozen(changed, |session| {
                crate::remember::told_for_session(socket, session, person.as_deref())
            }),
            Some(socket) => crate::remember::told_for_session(socket, 0, person.as_deref()),
            None => crate::remember::told(),
        };
        let going_on = crate::thread::current(file.as_deref(), |thread| thread.going_on(now))
            .unwrap_or_default();
        let room = room_bus.as_ref().map(crate::bus_out::room_now);
        system_message(
            &character,
            crate::agent_tasks::noted(crate::local_time::told_here(clock), tasks.as_ref())
                .as_deref(),
            &(memory + &facts(session_fact && !going_on, room)),
            file.as_deref(),
        )
    };
    let runner: ToolRunner = Arc::new(move |request, heard, name, args| {
        let settings = tools.clone();
        Box::pin(async move {
            let call = lamp_gemini::ToolCall {
                id: String::new(),
                name,
                args,
            };
            tokio::task::spawn_blocking(move || {
                crate::tools::answer(&settings, &call, request, heard)
            })
            .await
            .unwrap_or_else(|_| crate::tools::unavailable())
        })
    });
    let mut engine = Engine::new(Arc::new(net), system, runner)
        .checking_sounds(checked)
        .waiting_with_a_sound(wait_cue)
        .saying_when_down(down_line);
    // A look reads the eyes' look file with the Live route's bounds: a full
    // look's still may be older and larger (#836, #941).
    if let Some(path) = files.look.clone() {
        let (within, most) = if files.look_full {
            (crate::sight::FULL_FRESH, lamp_gemini::MAX_LOOK_IMAGE_BYTES)
        } else {
            (Duration::from_secs(1), lamp_gemini::MAX_IMAGE_BYTES)
        };
        engine = engine.looking(Arc::new(move || {
            crate::sight::fresh_up_to(&path, SystemTime::now(), within, most).map(|(jpeg, _)| jpeg)
        }));
    }
    if files.attention {
        let heeding = Mutex::new(Heeding::new(
            files.attention_busy,
            files.name_in_passing_ignored,
            files.bus_root.as_deref(),
            thread.clone(),
        ));
        if files.attention_near
            && let (Ok(mut heeding), Some(root)) = (heeding.lock(), files.bus_root.as_deref())
        {
            heeding.hear_levels(root);
        }
        engine = engine.heeding(Arc::new(move |request, said, heard, from| {
            heeding
                .lock()
                .ok()
                .and_then(|mut heeding| heeding.heed(request, said, heard, from))
        }));
    }
    if checked {
        let file = thread.clone();
        engine = engine.answering_the_lamp(Arc::new(move |request| {
            crate::thread::current(file.as_deref(), |thread| {
                lamp_just_asked(thread, request, Instant::now())
            })
            .unwrap_or_default()
        }));
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run(channels, boot, engine, thread))
}

/// Whether a checked sound heard for `request` may be the person's answer
/// to the lamp: a conversation is going on and the lamp's own last line
/// before it was a question, as the Live route's rule reads the same thread
/// (`crate::attention`, 17 October).
pub(crate) fn lamp_just_asked(thread: &crate::thread::Thread, request: u64, now: Instant) -> bool {
    thread.going_on(now) && thread.lamp_asked(request)
}

/// What a Live session is told of the moment beside what it hears, told
/// here in each turn's system message, since each chat starts with nothing:
/// the session fact ([`crate::session_fact::FACT`]) when `session` (no
/// conversation going on), and how the room is when it is known
/// ([`crate::room_told::BUSY`], [`crate::room_told::QUIET`]).
pub(crate) fn facts(session: bool, room: Option<crate::room_told::Room>) -> String {
    let room = match room {
        Some(crate::room_told::Room::Busy) => Some(crate::room_told::BUSY),
        Some(crate::room_told::Room::Quiet) => Some(crate::room_told::QUIET),
        _ => None,
    };
    session
        .then_some(crate::session_fact::FACT)
        .into_iter()
        .chain(room)
        .map(|fact| format!("\n\n{fact}"))
        .collect()
}

/// The Live route's rule for whom it answers (`crate::attention`, given
/// with `--attention`), run on this route's spoken turns with what this
/// route can give it: how the room is as Gemini's worker publishes it
/// (`room/state`), the lamp's name as the part that listens for it hears it
/// (`name/heard`), whether the eyes see someone turned to the lamp, and the
/// thread (a conversation going on, the lamp's last line a question). On
/// lamp-4ace on 2026-10-16 this route answered two remarks in a busy room
/// that Gemini's session left alone, nobody facing and no name (#1014).
///
/// With `--attention-near`, also how far the room microphone rose over its
/// quiet during the line (`room/sound`), sampled as the Live worker samples
/// it. Not given: a tap on the head and whether it was said from close
/// (`--addressed`), so a line in a busy room that only those would let
/// through on the Live route is held here. Each turn is judged as a session's first: this
/// route keeps no session that has heard enough to be trusted.
pub(crate) struct Heeding {
    attention: crate::attention::Attention,
    strict: bool,
    /// The bus, and the name's hearings on it since the last turn.
    bus: Option<(lamp_bus::Bus, lamp_bus::Subscriber)>,
    thread: Option<PathBuf>,
    /// The room microphone's rise over its quiet, by when it was sampled,
    /// with `--attention-near`: the last [`LEVELS_KEPT_US`].
    levels: Option<Levels>,
    /// The request the rule judged last, the session it is judged in, which
    /// a request going on from it keeps, and the words it was judged with,
    /// which are stripped from the front of that request's.
    judged: Option<(u64, u64, String)>,
}

/// The room microphone's rise over its quiet, dB, by when it was sampled.
type Levels = Arc<Mutex<std::collections::VecDeque<(u64, f32)>>>;

/// What the bus and the thread say as a spoken turn is judged.
pub(crate) struct Signs {
    pub room: crate::room_told::Room,
    pub going_on: bool,
    pub lamp_asked: bool,
    pub turned: Option<bool>,
    /// The name's hearings: when each began, by this process's clock, and
    /// its score.
    pub names: Vec<(u64, f32)>,
    /// The room microphone rose [`crate::attention::NEAR_RISE_DB`] or more
    /// over its quiet during the line.
    pub near: bool,
    /// The request this one goes on from, the same utterance after the lamp
    /// ended it early (`ProviderInput::Start::continues`,
    /// `LAMP_CUT_KEEPS_NAME=on`): the one begun just before, same session.
    pub continues_from: Option<u64>,
}

/// Hearings of the name read in one turn's judgement; more wait for the next.
const NAMES_READ: usize = 64;
/// The room microphone's level is sampled this often, as the Live worker does.
const LEVEL_EVERY: Duration = Duration::from_millis(100);
/// A level older than this is let go: longer than a line replayed to this
/// route takes to be heard.
const LEVELS_KEPT_US: u64 = 30_000_000;

impl Heeding {
    pub(crate) fn new(
        strict: bool,
        name_in_passing_ignored: bool,
        bus_root: Option<&Path>,
        thread: Option<PathBuf>,
    ) -> Self {
        let mut attention = crate::attention::Attention::default();
        attention.ignore_name_in_passing(name_in_passing_ignored);
        let bus = bus_root.and_then(|root| {
            let bus = lamp_bus::Bus::new(root).ok()?;
            let names = bus
                .subscribe_typed(
                    lamp_msgs::NameHeard::TOPIC,
                    "second",
                    lamp_msgs::NameHeard::TYPE,
                )
                .inspect_err(|why| {
                    eprintln!(
                        "lamp-voice second: the lamp's name cannot be taken from the bus, so whom it answers goes without it: {why}"
                    );
                })
                .ok()?;
            Some((bus, names))
        });
        Self {
            attention,
            strict,
            bus,
            thread,
            levels: None,
            judged: None,
        }
    }

    /// Sample the room microphone's rise over its quiet from now on, on a
    /// thread of its own: the turn's task cannot, since the line is judged
    /// only after it has been heard.
    fn hear_levels(&mut self, root: &Path) {
        let Ok(bus) = lamp_bus::Bus::new(root) else {
            return;
        };
        let levels: Levels = Arc::default();
        // Weak: the thread ends once this `Heeding` is gone (Second on #1028).
        let kept = Arc::downgrade(&levels);
        let started = std::thread::Builder::new()
            .name("second-levels".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(LEVEL_EVERY);
                    let now_us = monotonic_us();
                    let rise = bus
                        .latest(lamp_msgs::RoomSound::TOPIC)
                        .ok()
                        .flatten()
                        .filter(|message| {
                            unix_ms(SystemTime::now()).saturating_sub(message.header.at_unix_ms)
                                < 1_000
                        })
                        .and_then(|message| message.parse::<lamp_msgs::RoomSound>().ok())
                        .filter(|level| !level.own_voice)
                        .and_then(|level| Some(level.level_db - level.floor_db?));
                    let Some(levels) = kept.upgrade() else { return };
                    let Ok(mut levels) = levels.lock() else {
                        return;
                    };
                    if let Some(rise) = rise {
                        levels.push_back((now_us, rise));
                    }
                    while levels
                        .front()
                        .is_some_and(|(at, _)| now_us.saturating_sub(*at) > LEVELS_KEPT_US)
                    {
                        levels.pop_front();
                    }
                }
            });
        if started.is_ok() {
            self.levels = Some(levels);
        }
    }

    fn heed(
        &mut self,
        request: u64,
        said: (u64, u64),
        heard: &str,
        continues_from: Option<u64>,
    ) -> Option<&'static str> {
        let signs = Signs {
            continues_from,
            ..self.signs(request, said)
        };
        self.judge(request, said, heard, signs)
    }

    /// What the bus and the thread say now of a line said in `began..=ended`.
    fn signs(&mut self, request: u64, (began, ended): (u64, u64)) -> Signs {
        let (going_on, lamp_asked) = crate::thread::current(self.thread.as_deref(), |thread| {
            (thread.going_on(Instant::now()), thread.lamp_asked(request))
        })
        .unwrap_or_default();
        let mut signs = Signs {
            room: crate::room_told::Room::Unknown,
            going_on,
            lamp_asked,
            turned: None,
            names: Vec::new(),
            near: self
                .levels
                .as_ref()
                .and_then(|levels| levels.lock().ok())
                .is_some_and(|levels| {
                    levels.iter().any(|(at, rise)| {
                        (began..=ended).contains(at) && *rise >= crate::attention::NEAR_RISE_DB
                    })
                }),
            continues_from: None,
        };
        let Some((bus, names)) = &mut self.bus else {
            return signs;
        };
        signs.room = crate::bus_out::room_now(bus);
        signs.turned = crate::bus_out::someone_turned_to_lamp(bus);
        let now_us = monotonic_us();
        let now_ms = unix_ms(SystemTime::now());
        for _ in 0..NAMES_READ {
            let Ok(Some(message)) = names.recv(Duration::ZERO) else {
                break;
            };
            if let Ok(name) = message.parse::<lamp_msgs::NameHeard>() {
                // Its publisher's monotonic clock is its own: dated by the
                // wall clock both share.
                let ago_ms =
                    now_ms.saturating_sub(message.header.at_unix_ms) + u64::from(name.ago_ms);
                signs
                    .names
                    .push((now_us.saturating_sub(ago_ms * 1_000), name.score));
            }
        }
        signs
    }

    /// The rule's verdict on a spoken turn: why it is not answered, or
    /// nothing.
    pub(crate) fn judge(
        &mut self,
        request: u64,
        (began_us, ended_us): (u64, u64),
        heard: &str,
        signs: Signs,
    ) -> Option<&'static str> {
        let attention = &mut self.attention;
        // Each turn is its own session here. One going on from the request
        // begun just before it, when the rule judged that one last, keeps
        // what the rule knew of it (its name, the words so far, whether it
        // was close), as on the Live route. Anything else, a request never
        // judged among them, is judged fresh (the reviewer on #1226).
        let (session, heard) = match self
            .judged
            .as_ref()
            .filter(|(last, _, _)| signs.continues_from == Some(*last))
        {
            Some((_, session, before)) => {
                attention.continued_at(request, *session, began_us);
                // The engine put the words going on from in front of these
                // (#1168); the rule keeps them already, so it is given only
                // this request's own (the reviewer on #1226).
                // Its leading space kept: the rule's text goes on from the
                // first half's words (Fourth on #1262: "HeyLamp, …").
                let own = heard.strip_prefix(before.trim()).unwrap_or(heard);
                // ponytail: a chain whose first link was never judged keeps
                // that link's words in `before`, the strip misses, and the
                // rule reads the joined words as before this (Fourth on #1262).
                (*session, own)
            }
            None => {
                attention.began_at(request, request, began_us);
                (request, heard)
            }
        };
        self.judged = Some((request, session, heard.to_owned()));
        attention.ended(request, ended_us);
        for (at_us, score) in signs.names {
            attention.name_heard_scored(at_us, score);
        }
        attention.heard(heard);
        attention.lamp_asked(signs.lamp_asked);
        attention.set_near(signs.near);
        let turned = signs.turned.filter(|_| self.strict);
        match attention.first_output_busy(request, signs.room, signs.going_on, self.strict, turned)
        {
            Some(crate::attention::Verdict::Hold(why)) => Some(why),
            // Said, as each hold is by the route (#829, 17:21).
            Some(crate::attention::Verdict::Play(why)) => {
                eprintln!("lamp-voice second: request {request}: its answer is played: {why}");
                None
            }
            None => None,
        }
    }
}

fn unix_ms(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis().try_into().unwrap_or(u64::MAX))
}

/// Whose the second route's turns are, and the Days note of that person's
/// run of turns.
#[derive(Default)]
struct Sessions {
    presence: crate::known_faces::Presence,
    /// Runs of turns opened so far: the Days session number.
    opened: u64,
    note: Option<String>,
}
impl Sessions {
    /// Open the thread for whoever is at the lamp now ([`Thread::opening`]):
    /// whether that is someone other than the last turn's, and whom.
    ///
    /// [`Thread::opening`]: crate::thread::Thread::opening
    fn open(&mut self, who: Option<&Path>, now: Instant) -> (bool, Option<String>) {
        let known = who
            .and_then(crate::known_faces::known_now)
            .map(|name| crate::remember::person_id(&name));
        let whose = who.and(self.presence.seen(known.clone(), now).map(str::to_owned));
        let Ok(mut thread) = crate::thread::THREAD.lock() else {
            return (false, None);
        };
        if who.is_some() {
            thread.at_lamp(whose);
            thread.seen_now(known);
        }
        let changed = thread.opening(now);
        (changed, thread.opened_for().map(str::to_owned))
    }

    /// The Days note for this run of turns, asked anew when the person changed.
    fn frozen(&mut self, changed: bool, ask: impl FnOnce(u64) -> String) -> String {
        if changed || self.note.is_none() {
            self.opened += 1;
            self.note = Some(ask(self.opened));
        }
        self.note.clone().unwrap_or_default()
    }
}

/// The system message for a turn begun now, as a Live session opened now
/// is told its instruction (`crate::thread::Recalling`): the character,
/// the time and place, the memory note, and the thread in the room the
/// rest leave it, taken first from `file` if the Live route wrote it since.
///
/// With it, the person's last exchanges as messages: up to
/// [`HISTORY_EXCHANGES`] of the last [`HISTORY_WITHIN`]. A Live session keeps
/// its own, and "and how many live there?" needs the one before. Taken
/// from the thread under the same lock as the recall, after the thread's
/// forgetting and reload, so a forgotten exchange is never given back.
pub(crate) fn system_message(
    character: &str,
    time: Option<&str>,
    memory: &str,
    file: Option<&Path>,
) -> Told {
    let told = format!("{character}{}{memory}", time.unwrap_or_default());
    let room = lamp_gemini::MAX_INSTRUCTION_BYTES.saturating_sub(told.len());
    let now = Instant::now();
    let (thread, recent) = crate::thread::current(file, |thread| {
        (
            thread.recall(now, room),
            thread.recent(now, HISTORY_WITHIN, HISTORY_EXCHANGES),
        )
    })
    .unwrap_or_default();
    Told {
        text: told + &thread.unwrap_or_default(),
        history: recent.into_iter().flat_map(messages).collect(),
    }
}

/// One exchange as chat messages. A request the lamp was given in writing
/// goes as a system note, never as words a person said; nothing is sent
/// for a person who said nothing.
fn messages(exchange: crate::thread::Recent) -> Vec<Message> {
    let mut messages = Vec::with_capacity(3);
    if let Some(written) = exchange.written {
        messages.push(Message::system(format!(
            "(A note you were given in writing, not words anyone said: {written})"
        )));
    }
    if !exchange.person.is_empty() {
        messages.push(Message::user(exchange.person));
    }
    messages.push(Message::assistant(exchange.lamp));
    messages
}

/// Exchanges given to the chat as messages, and for how long since each.
pub const HISTORY_EXCHANGES: usize = 6;
pub const HISTORY_WITHIN: Duration = Duration::from_secs(300);

/// Keep what a turn's output says in the thread, as the Live route keeps
/// its transcripts: the person's words, the lamp's (not the announcement),
/// and at the turn's end the exchange to the memory's journal and the
/// thread to its file.
pub(crate) fn keep(event: &ProviderOutput, file: Option<&Path>) {
    let now = Instant::now();
    match event {
        ProviderOutput::Transcript {
            request: None,
            text,
            finished,
        } => {
            if let Ok(mut thread) = crate::thread::THREAD.lock() {
                thread.person(text, *finished, now);
            }
        }
        ProviderOutput::Transcript {
            request: Some(request),
            text,
            ..
        } => {
            // Sentences come trimmed, those after the first with a space
            // before; the thread joins its pieces as given.
            if let Ok(mut thread) = crate::thread::THREAD.lock() {
                thread.lamp(*request, &format!("{} ", text.trim_start()), now);
            }
        }
        ProviderOutput::TurnComplete { request, .. } => {
            crate::remember::finished(*request);
            save(file);
        }
        ProviderOutput::TurnFailed { .. } => save(file),
        _ => {}
    }
}

/// The thread to its file if it changed; one not written loses the thread
/// at the next restart and nothing else.
fn save(file: Option<&Path>) {
    let Some(path) = file else { return };
    let text = crate::thread::THREAD
        .lock()
        .ok()
        .and_then(|mut thread| thread.save(Instant::now(), SystemTime::now()));
    if let Some(text) = text
        && crate::thread::write_file(path, &text).is_ok()
        && let Ok(mut thread) = crate::thread::THREAD.lock()
    {
        thread.wrote(path);
    }
}

/// Run the engine as a provider worker until Stop. Readiness is the
/// fixture's: `Ready`, then one `ProviderLink` Ready on the control plane.
/// ponytail: the services are not probed first; the link says Ready and a
/// service that cannot be reached fails that turn. A probe at startup if a
/// dead key should show as an unavailable link.
pub async fn run(
    mut channels: WorkerChannels,
    boot: BootId,
    mut engine: Engine,
    thread: Option<PathBuf>,
) -> io::Result<()> {
    let started_us = monotonic_us();
    channels.control.send(WorkerEvent::Ready)?;
    let ready = ProviderLink {
        revision: 1,
        session: 1,
        observed_at_us: started_us,
        source_clock: None,
        state: ProviderLinkState::Ready {
            setup_us: monotonic_us() - started_us,
            attempts: 1,
            context: LinkContext::Initial,
            outage_started_at_us: None,
            after: None,
        },
    };
    ready.validate(monotonic_us())?;
    channels
        .control
        .send(WorkerEvent::ProviderLink { link: ready })?;
    let mut gate = Gate::new(boot, monotonic_us());
    let mut ticker = tokio::time::interval(Duration::from_millis(2));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // At most one input read ahead of its authority, and one output waiting
    // for credit or a full socket.
    let mut retained: Option<ProviderInput> = None;
    let mut pending: Option<ProviderOutput> = None;
    'tick: loop {
        // The turn tasks run while this waits.
        ticker.tick().await;
        match gate.drain(&mut channels.control, &mut engine)? {
            Drain::Stopped => return Ok(()),
            Drain::Deferred => continue,
            Drain::Empty => {}
        }
        gate.fresh(monotonic_us())?;
        // As many inputs a tick as the Live worker takes: a line replayed
        // to this route arrives faster than it was spoken, and a packet
        // left waiting past its age bound is refused.
        for _ in 0..INPUT_SLICE {
            if retained.is_none() {
                retained = channels.data.receive()?;
            }
            // The parent sends authority before its dependent input, possibly
            // after the first empty read: drain again before judging the input.
            match gate.drain(&mut channels.control, &mut engine)? {
                Drain::Stopped => return Ok(()),
                Drain::Deferred => continue 'tick,
                Drain::Empty => {}
            }
            let Some(command) = retained.take() else {
                break;
            };
            if gate.admits(monotonic_us(), &command)? {
                // A written request is kept for the thread as the note it
                // is, with the answer to it (`Thread::written`).
                if let ProviderInput::Written { request, text, .. } = &command
                    && let Ok(mut thread) = crate::thread::THREAD.lock()
                {
                    thread.written(*request, text);
                }
                engine.submit(command);
            }
        }
        // Output kept waiting for a request since retired is dropped.
        if pending
            .as_ref()
            .and_then(output_request)
            .is_some_and(|request| request <= gate.retired_through)
        {
            pending = None;
        }
        if pending.is_none() {
            pending = engine.try_output();
            if let Some(event) = &pending {
                keep(event, thread.as_deref());
            }
        }
        if let Some(event) = pending.take() {
            gate.may_send(monotonic_us(), &event)?;
            if !gate.flow.try_send(|| channels.data.send(&event))? {
                pending = Some(event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// What the engine is told for a checked sound (17 October): the lamp's
    /// own last line a question, in a conversation going on. A line that
    /// asks nothing, or the same question past the conversation, is not.
    #[test]
    fn the_lamp_just_asked_only_with_its_question_in_a_conversation() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let asked = |lamp: &str, later: Duration| {
            let mut thread = crate::thread::Thread::new();
            thread.person("Can you get the office build checked?", true, now);
            thread.lamp(1, lamp, now);
            super::lamp_just_asked(&thread, 2, now + later)
        };
        let question = "I can hand that to Codex. Shall I start it?";
        assert!(asked(question, Duration::ZERO));
        assert!(asked(question, Duration::from_secs(20)));
        assert!(!asked(question, Duration::from_secs(120)));
        assert!(!asked(
            "I can hand that to Codex whenever you like.",
            Duration::ZERO
        ));
        // Its own answer is not what it asked before.
        let mut thread = crate::thread::Thread::new();
        thread.lamp(2, question, now);
        assert!(!super::lamp_just_asked(&thread, 2, now));
    }

    /// Each fact as a Live session is told it; the session fact only with
    /// no conversation going on, the room only when known.
    #[test]
    fn the_facts_are_the_live_routes_own() {
        use crate::room_told::Room;
        assert_eq!(super::facts(false, None), "");
        assert_eq!(super::facts(false, Some(Room::Unknown)), "");
        assert_eq!(
            super::facts(true, Some(Room::Busy)),
            format!(
                "\n\n{}\n\n{}",
                crate::session_fact::FACT,
                crate::room_told::BUSY
            )
        );
        assert_eq!(
            super::facts(false, Some(Room::Quiet)),
            format!("\n\n{}", crate::room_told::QUIET)
        );
    }

    /// A request going on from the one begun just before it, the same
    /// utterance after the lamp ended it early (`continues`), keeps what the
    /// rule knew of it when the rule judged that one: "Hey Lamp, what" then
    /// "time is it?" in a busy room is answered. One going on from a request
    /// the rule never judged (replaced before its words came back) is judged
    /// fresh, never as going on from an older question (the reviewer on
    /// #1226: "budget" ten minutes after "weather").
    #[test]
    fn a_request_going_on_keeps_only_the_one_just_before_it() {
        use crate::room_told::Room;
        let busy = |continues_from: Option<u64>| Signs {
            room: Room::Busy,
            going_on: false,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from,
        };
        let said = (10_000_000, 11_000_000);
        let rest = (11_400_000, 12_000_000);
        let mut heeding = Heeding::new(true, true, None, None);
        assert_eq!(heeding.judge(300, said, "Hey Lamp, what", busy(None)), None);
        assert_eq!(
            heeding.judge(301, rest, "time is it?", busy(Some(300))),
            None
        );
        // Not going on: the same words alone are office talk.
        let mut fresh = Heeding::new(true, true, None, None);
        assert_eq!(fresh.judge(300, said, "Hey Lamp, what", busy(None)), None);
        assert!(fresh.judge(301, rest, "time is it?", busy(None)).is_some());
        // The reviewer's probe: 302 goes on from 301, which was never judged
        // (replaced before its words came back), ten minutes after 300.
        let mut probe = Heeding::new(true, true, None, None);
        assert_eq!(
            probe.judge(300, said, "Hey Lamp, what's the weather", busy(None)),
            None
        );
        let later = (610_000_000, 611_000_000);
        assert!(
            probe
                .judge(
                    302,
                    later,
                    "do you think the budget is fine",
                    busy(Some(301))
                )
                .is_some()
        );
    }

    /// Fourth's probe on #1262: "Hey" held, then the request going on from
    /// it with the engine's joined words "Hey Lamp, turn the light on." is a
    /// call, so it plays; the stripped words keep their space.
    #[test]
    fn a_carried_over_call_keeps_the_space_between_its_halves() {
        use crate::room_told::Room;
        let busy = |continues_from: Option<u64>| Signs {
            room: Room::Busy,
            going_on: false,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from,
        };
        let said = (10_000_000, 10_400_000);
        let rest = (10_800_000, 12_000_000);
        let mut heeding = Heeding::new(true, true, None, None);
        assert!(heeding.judge(300, said, "Hey", busy(None)).is_some());
        assert_eq!(
            heeding.judge(301, rest, "Hey Lamp, turn the light on.", busy(Some(300))),
            None
        );
    }

    /// The two remarks on lamp-4ace on 2026-10-16 that this route answered
    /// and Gemini's session left alone (#1014): a busy room, no name, nobody
    /// facing. Held, 0 of 2; a named question in the same room, and a
    /// follow-up in a conversation with the lamp, are still answered.
    #[test]
    fn the_rule_holds_office_talk_and_lets_a_call_or_a_follow_up_through() {
        use crate::room_told::Room;
        let busy = |names: Vec<(u64, f32)>, going_on: bool| Signs {
            room: Room::Busy,
            going_on,
            lamp_asked: false,
            turned: Some(false),
            names,
            near: false,
            continues_from: None,
        };
        let said = (10_000_000, 11_500_000);
        let mut heeding = Heeding::new(true, true, None, None);
        for (request, remark) in [(202, "Did you see what he wants us to look"), (203, "Me.")] {
            assert!(
                heeding
                    .judge(request, said, remark, busy(Vec::new(), false))
                    .is_some(),
                "{remark}"
            );
        }
        // Named in the text, or heard plainly in the sound.
        assert_eq!(
            heeding.judge(
                204,
                said,
                "Hey Lamp, what time is it?",
                busy(Vec::new(), false)
            ),
            None
        );
        assert_eq!(
            heeding.judge(
                205,
                said,
                "What time is it?",
                busy(vec![(9_800_000, 1.0)], false)
            ),
            None
        );
        // A hearing from before the line's window is not its call.
        let later = (30_000_000, 31_500_000);
        assert!(
            heeding
                .judge(
                    206,
                    later,
                    "What time is it?",
                    busy(vec![(25_000_000, 1.0)], false)
                )
                .is_some()
        );
        assert_eq!(
            heeding.judge(
                207,
                said,
                "And what about tomorrow?",
                busy(Vec::new(), true)
            ),
            None
        );
        // Someone turned to the lamp, or a room not known to be busy.
        let mut turned = busy(Vec::new(), false);
        turned.turned = Some(true);
        assert_eq!(heeding.judge(208, said, "Me.", turned), None);
        let mut quiet = busy(Vec::new(), false);
        quiet.room = Room::Quiet;
        assert_eq!(heeding.judge(209, said, "Me.", quiet), None);
        // busy-near: a name heard at 0.95 or more counts only with the room
        // microphone 18 dB over its quiet during the line.
        let (soon, sooner) = ((50_000_000, 51_500_000), (70_000_000, 71_500_000));
        assert!(
            heeding
                .judge(
                    210,
                    soon,
                    "What time is it?",
                    busy(vec![(50_200_000, 0.96)], false)
                )
                .is_some()
        );
        let mut near = busy(vec![(70_200_000, 0.96)], false);
        near.near = true;
        assert_eq!(heeding.judge(211, sooner, "What time is it?", near), None);
    }

    use super::*;
    use crate::second_provider::{Listener, Services, net};
    use futures_util::{
        future::BoxFuture,
        stream::{self, BoxStream},
    };
    use lamp_turn_owner::{AdmissionState, AdmittedInput, CaptureState, Controller, Permission};
    use std::sync::Arc;

    /// Services that never answer: only authority is under test.
    struct Silent;
    impl Services for Silent {
        fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
            Box::pin(std::future::pending())
        }
        fn chat(&self, _: Vec<net::Message>) -> BoxStream<'static, net::Result<net::ChatEvent>> {
            Box::pin(stream::pending())
        }
        fn speak(&self, _: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
            Box::pin(stream::pending())
        }
    }

    /// #960, lamp-4ace 08:47:27: the worker was held up (#784's SD/MMC
    /// stalls) while finishing a turn, and the authority queued meanwhile
    /// had lapsed when it was read. That ended the route and the voice.
    /// Lapsed snapshots are skipped and the next fresh one is taken.
    #[tokio::test]
    async fn authority_that_lapsed_in_the_queue_during_a_stall_is_skipped() {
        let boot = BootId::new([7; 16]).unwrap();
        let start_us = 10_000;
        let at = |us| MonoTime::from_micros(us);
        let mut owner = Controller::new(boot, at(start_us));
        owner
            .set_microphone_permission(at(start_us), Permission::Allowed)
            .unwrap();
        let renew = |owner: &mut Controller, now_us: u64| {
            let until = at(now_us + 100_000);
            owner
                .set_capture(at(now_us), CaptureState::RetainingUntil(until))
                .unwrap();
            owner
                .set_admission(at(now_us), AdmissionState::OpenUntil(until))
                .unwrap();
            owner.snapshot(at(now_us)).unwrap()
        };
        let mut engine = Engine::new(
            Arc::new(Silent),
            Told::default,
            Arc::new(|_, _, _, _| Box::pin(async { serde_json::Value::Null })),
        );
        let mut gate = Gate::new(boot, start_us);
        let first = renew(&mut owner, start_us);
        gate.install(start_us, first, &mut engine).unwrap();
        owner.admit(at(start_us), AdmittedInput::NewTurn).unwrap();
        // Sent every few ms while the worker was held up for 300 ms.
        let queued: Vec<_> = (1..=60)
            .map(|step| renew(&mut owner, start_us + step * 5_000))
            .collect();
        let read_us = start_us + 300_000;
        for snapshot in queued {
            gate.install(read_us, snapshot, &mut engine).unwrap();
        }
        assert_eq!(gate.last_control_us, read_us);
        assert_eq!(gate.skipped_expired, 0);
        gate.fresh(read_us).unwrap();
    }

    #[tokio::test]
    async fn a_new_owner_retires_the_old_turn_and_its_late_input() {
        let now_us = 10_000;
        let at = MonoTime::from_micros(now_us);
        let boot = BootId::new([7; 16]).unwrap();
        let mut owner = Controller::new(boot, at);
        owner
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(now_us + 100_000);
        owner
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
        let mut engine = Engine::new(
            Arc::new(Silent),
            Told::default,
            Arc::new(|_, _, _, _| Box::pin(async { serde_json::Value::Null })),
        );
        let mut gate = Gate::new(boot, now_us);
        let first = owner.admit(at, AdmittedInput::NewTurn).unwrap();
        let state = owner.snapshot(at).unwrap();
        gate.install(now_us, state, &mut engine).unwrap();
        let start = |request| ProviderInput::Start {
            request,
            session: 1,
            privacy_generation: state.microphone_generation(),
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
        };
        assert!(gate.admits(now_us, &start(first.turn())).unwrap());
        engine.submit(start(first.turn()));
        assert!(gate.may_send(now_us, &engine.try_output().unwrap()).is_ok());
        assert!(engine.turn.is_some());

        let second = owner.admit(at, AdmittedInput::Interruption).unwrap();
        gate.install(now_us, owner.snapshot(at).unwrap(), &mut engine)
            .unwrap();
        assert!(engine.turn.is_none());
        assert!(!gate.admits(now_us, &start(first.turn())).unwrap());
        assert!(gate.admits(now_us, &start(second.turn())).unwrap());
        // Output of the old owner would be refused at the boundary.
        let late = ProviderOutput::GenerationComplete {
            request: first.turn(),
        };
        assert!(gate.may_send(now_us, &late).is_err());
    }

    fn strings(flags: &[&str]) -> Vec<String> {
        flags.iter().map(|flag| (*flag).to_owned()).collect()
    }

    #[test]
    fn a_written_request_goes_to_the_chat_as_a_note_and_silence_as_nothing() {
        let written = messages(crate::thread::Recent {
            written: Some("Say ORCHID.".into()),
            person: String::new(),
            lamp: "ORCHID.".into(),
        });
        assert_eq!(
            written,
            [
                Message::system(
                    "(A note you were given in writing, not words anyone said: Say ORCHID.)"
                ),
                Message::assistant("ORCHID."),
            ]
        );
        let spoken = messages(crate::thread::Recent {
            written: None,
            person: "Hi.".into(),
            lamp: "Hello.".into(),
        });
        assert_eq!(spoken, [Message::user("Hi."), Message::assistant("Hello.")]);
    }

    #[test]
    fn the_second_worker_takes_the_live_workers_flags_but_keep_link() {
        let dir = Path::new("/run/lamp-voice");
        let flags = strings(&[
            "--provider-continuation",
            "kept-start",
            "--thread-file",
            "/run/lamp-voice/thread.json",
            "--session-fact",
            "--room-facts",
            "--air-facts",
            "--tools",
            "--actions-dir",
            "/run/lamp-exec",
            "--memory-socket",
            "/run/lamp-memory/memory.sock",
            "--clock",
            "--attention",
            "--attention-busy",
            "--name-signal",
            "--name-note",
            "--bus-root",
            "/run/lampos-bus",
        ]);
        let (_, _, files, visual) = parse_flags(dir, &flags).unwrap();
        assert_eq!(
            files.thread.as_deref(),
            Some(Path::new("/run/lamp-voice/thread.json"))
        );
        assert!(files.tools.decline && files.tools.clock && files.attention);
        assert_eq!(
            files.tools.memory.as_deref(),
            Some(Path::new("/run/lamp-memory/memory.sock"))
        );
        assert!(visual.is_none());
        assert!(parse_flags(dir, &[]).is_ok());
        for refused in [
            strings(&["--keep-link"]),
            strings(&["--tools", "--unknown"]),
            strings(&["--clock", "--tools"]),
        ] {
            assert!(parse_flags(dir, &refused).is_err(), "{refused:?}");
        }
    }

    /// A finished turn goes to the thread, its file and the journal, and
    /// the next turn's system message carries the character, the memory's
    /// note and that thread. In a child process: the thread and the
    /// memory are this process's globals.
    #[test]
    fn a_turn_is_kept_and_told_to_the_next() {
        use std::io::{BufRead, BufReader, Write};
        const CHILD: &str = "LAMP_SECOND_KEEP_TEST_CHILD";
        if std::env::var(CHILD).ok().as_deref() != Some("1") {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "second_provider::worker::tests::a_turn_is_kept_and_told_to_the_next",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env_remove(crate::remember::DAYS_ENVIRONMENT)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let dir = std::env::temp_dir().join(format!("lv-second-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let socket = dir.join("memory.sock");
        let file = dir.join("thread.json");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        // The memory: the kept list, then the journal's first append.
        let memory = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                let answer = match request["op"].as_str() {
                    Some("kept") => {
                        serde_json::json!({"ok":true,"entries":[{"text":"coffee black"}]})
                    }
                    _ => serde_json::json!({"ok":true}),
                };
                writeln!(stream, "{answer}").unwrap();
                if request["op"] == "append" {
                    return request;
                }
            }
            unreachable!()
        });
        crate::remember::start(socket.clone(), None);
        let request = 7;
        for event in [
            ProviderOutput::Transcript {
                request: None,
                text: "What should I drink?".into(),
                finished: true,
            },
            ProviderOutput::Transcript {
                request: Some(request),
                text: "Try some tea.".into(),
                finished: false,
            },
            ProviderOutput::Transcript {
                request: Some(request),
                text: "It is calm.".into(),
                finished: false,
            },
            ProviderOutput::TurnComplete {
                request,
                idle: true,
            },
        ] {
            keep(&event, Some(&file));
        }
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(
            written.contains("What should I drink?")
                && written.contains("Try some tea. It is calm.")
        );
        let journalled = memory.join().unwrap();
        assert_eq!(journalled["heard"], "What should I drink?");
        assert_eq!(journalled["said"], "Try some tea. It is calm.");

        let told = system_message(
            "You are a lamp.",
            Some(" It is noon."),
            &crate::remember::told(),
            Some(&file),
        );
        assert!(
            told.text
                .starts_with("You are a lamp. It is noon.\n\nYour memory\n")
        );
        assert!(told.text.contains("- coffee black\n"));
        assert!(
            told.text
                .contains("Person: What should I drink?\nYou: Try some tea. It is calm.\n")
        );
        assert_eq!(
            told.history,
            [
                Message::user("What should I drink?"),
                Message::assistant("Try some tea. It is calm.")
            ]
        );
        // Forgotten, then another person's turn, then back: nothing of it
        // is told or given again (first builder's reviewer on #778).
        crate::thread::THREAD.lock().unwrap().forget();
        for whose in [Some("sam"), None] {
            let mut thread = crate::thread::THREAD.lock().unwrap();
            thread.at_lamp(whose.map(str::to_owned));
            thread.opening(Instant::now());
        }
        let told = system_message("You are a lamp.", None, "", Some(&file));
        assert!(told.history.is_empty(), "{:?}", told.history);
        assert!(!told.text.contains("Try some tea"), "{}", told.text);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The reviewer on #1100: this route reads the eyes' file only as a
    /// turn opens, so it never sees the 30 s a person is unseen between
    /// turns. B's last turn at 30 s, then A's turns an hour later: they are
    /// A's at once, never B's thread and kept things.
    #[test]
    fn a_known_person_long_after_another_has_their_own_turns_at_once() {
        use std::time::{Duration, Instant};
        let dir =
            std::env::temp_dir().join(format!("lamp-voice-second-sessions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("known");
        // The process's one thread, set aside for the test and put back.
        let kept = std::mem::take(
            &mut *crate::thread::THREAD
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let start = Instant::now();
        let mut sessions = Sessions::default();
        std::fs::write(&file, "B\n").unwrap();
        let (_, person) = sessions.open(Some(&file), start + Duration::from_secs(30));
        assert_eq!(person, Some(crate::remember::person_id("B")));
        std::fs::write(&file, "A\n").unwrap();
        let mut opened = Vec::new();
        for at in [3_600, 3_605] {
            opened.push(
                sessions
                    .open(Some(&file), start + Duration::from_secs(at))
                    .1,
            );
        }
        *crate::thread::THREAD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = kept;
        std::fs::remove_dir_all(&dir).unwrap();
        let a = Some(crate::remember::person_id("A"));
        assert_eq!(opened, [a.clone(), a]);
    }
}
