//! Generic skill rest intents. The skill chooses a window; Expression owns
//! admission, physical transitions and cancellation. A receipt is not proof
//! that the body moved. No request is rebound after an owner restart.
use crate::{
    Skill,
    daily::{Moment, Window},
};
use lamp_bus::{Bus, Publisher};
use lamp_local_time::LocalTime;
use lamp_msgs::{
    ClockStamp, ExpressionRestOp as Op, ExpressionRestPhase as Phase,
    ExpressionRestRequest as Request, ExpressionRestState as Receipt, ExpressionSleepClaim,
    ExpressionState, Faces, MicrophonePrivacy, Voice, VoiceMute, VoiceState,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

pub const ENVIRONMENT: &str = "LAMP_SKILL_ROUTINES";
/// Where, in `--state-dir`, the night each routine rested is kept across a
/// restart (#1672): one line a skill, its name and the local day the night
/// began (`bedtime-morning 20453`), nothing about a person.
pub const RESTED_FILE: &str = "rested";
const MAX_SKILLS: usize = 8;
const RENEW_US: u64 = 1_000_000;
const ADMISSION_US: u64 = 1_000_000;
const MAX_EPISODE_US: u64 = 86_400_000_000;
/// Includes the largest valid custom sleep-settle fallback (10 s).
const WAKE_US: u64 = 12_000_000;
/// The most re-settles a rest may declare a night (#1672: three).
pub const MOST_RESETTLES: u8 = 3;
/// A voice state older than this is not known to be resting: the voice
/// says its state once a second.
const VOICE_FRESH_US: u64 = 3_000_000;
/// Why a rest cannot be had while no fresh receipt comes from Expression:
/// it is down, faulted, on another boot's clock, or its rest requests are
/// off (`LAMP_REST_REQUESTS`).
pub const NOT_CONNECTED: &str = "Expression's rest receipts are not fresh";
/// A rest ask Expression neither admitted nor refused in time: its loop is
/// held up, or it drops this publisher's requests (a retired session).
pub const NOT_ANSWERED: &str = "Expression did not answer the rest in time";

/// Refusals of a rest ask that will meet the next ask too (#940, the first
/// builder's P2 on #2744), so the rest path is down until an ask is
/// admitted: Expression's clock and lamp-skills' disagree or its loop lags
/// (Expression's `rest observation is future, stale or expired`), no trusted
/// local clock, no fresh Expression, no answer in time, or a bound spent.
/// Every other refusal passes with the moment: the voice in a turn, a hand
/// or a lean (`rest owner is unavailable ...`, `rest transition is
/// unavailable ...`), lost input, a window not yet current, a rest already
/// active.
const REPEATS: [&str; 6] = [
    "rest observation is future, stale or expired",
    "rest transitions require a trusted local clock",
    "Expression is not fresh and enabled",
    "rest source history is full",
    "rest episodes exhausted",
    NOT_ANSWERED,
];

/// Whether a rest refused for `why` will be refused again ([`REPEATS`]).
pub fn refusal_repeats(why: &str) -> bool {
    REPEATS.contains(&why)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hours {
    from: String,
    to: String,
}

impl Hours {
    fn checked(self) -> Result<Window, String> {
        Window::parse(&self.from, &self.to).map_err(|error| error.to_string())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsFile {
    during: Hours,
    light_channel_max: Option<u16>,
    volume_raw_max: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    pub during: Window,
    pub light_channel_max: Option<u16>,
    pub volume_raw_max: Option<u8>,
}

impl LimitsFile {
    pub fn checked(self) -> Result<Limits, String> {
        if self.light_channel_max.is_none() && self.volume_raw_max.is_none()
            || self
                .light_channel_max
                .is_some_and(|value| !(12..=120).contains(&value))
            || self
                .volume_raw_max
                .is_some_and(|value| !(1..=100).contains(&value))
        {
            return Err(
                "limits require a light channel maximum 12..120 or raw volume maximum 1..100"
                    .into(),
            );
        }
        Ok(Limits {
            during: self.during.checked()?,
            light_channel_max: self.light_channel_max,
            volume_raw_max: self.volume_raw_max,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestFile {
    op: Op,
    during: Option<Hours>,
    resettle: Option<ResettleFile>,
}

/// `resettle = { most = 3, after = "30s", during = { from, to } }`: a rest
/// that a sound ended (the voice no longer resting) settles again, with no
/// run, no wake clip and no daily claim, once the voice has rested `after`
/// again inside `during`, at most `most` times (#1672).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResettleFile {
    most: u8,
    after: String,
    during: Hours,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resettle {
    pub most: u8,
    pub after_us: u64,
    pub during: Window,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rest {
    pub op: Op,
    pub during: Option<Window>,
    pub resettle: Option<Resettle>,
}

impl RestFile {
    pub fn checked(self) -> Result<Rest, String> {
        if (self.op == Op::Rest) != self.during.is_some() {
            return Err("rest requires a during window; wake and cancel have none".into());
        }
        if self.op != Op::Rest && self.resettle.is_some() {
            return Err("resettle goes with op = \"rest\"".into());
        }
        let resettle = self
            .resettle
            .map(|file| {
                let after_us = crate::duration_us(&file.after)?;
                if !(1..=MOST_RESETTLES).contains(&file.most)
                    || !(5_000_000..=600_000_000).contains(&after_us)
                {
                    return Err(format!(
                        "resettle is most 1 to {MOST_RESETTLES}, after 5s to 10m"
                    ));
                }
                Ok(Resettle {
                    most: file.most,
                    after_us,
                    during: file.during.checked()?,
                })
            })
            .transpose()?;
        Ok(Rest {
            op: self.op,
            during: self.during.map(Hours::checked).transpose()?,
            resettle,
        })
    }
}

/// How a request was answered. `Owner`: Expression took it (a rest, or
/// the wake of the rest this skill owns, its stretch Expression's wake
/// clip). `Already`: a wake with no rest left to own, the morning after a
/// night it rested whose rest a sound ended early; the lamp is awake, so
/// the skill makes its own stretch before its sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Woke {
    Owner,
    Already,
}

struct Asked {
    skill: String,
    rest: Rest,
    issued_us: u64,
    expires_us: u64,
    answer: mpsc::SyncSender<Result<Woke, String>>,
    generation: Arc<()>,
}

#[derive(Clone)]
pub struct Client {
    send: mpsc::SyncSender<Asked>,
    generation: Arc<Mutex<Arc<()>>>,
}

/// Captured before a run thread is spawned. Scheduling that old thread
/// after a reload cannot mint authority under a new definition.
#[derive(Clone)]
pub struct RunClient {
    client: Client,
    generation: Arc<()>,
}

/// Permission to begin accompanying presentation within the original
/// transition deadline and definition generation.
pub struct Permit {
    run: RunClient,
    expires_us: u64,
    already: bool,
}

impl Permit {
    /// A wake answered `Woke::Already`: nobody stretched the lamp, so the
    /// run does before its sound.
    pub fn already(&self) -> bool {
        self.already
    }

    /// Original same-host monotonic deadline. Receiving a confirmation
    /// never extends it; an output owner can enforce it after transport.
    pub fn expires_us(&self) -> u64 {
        self.expires_us
    }

    pub fn check(&self) -> Result<(), String> {
        self.check_at(lamp_ipc::monotonic_us())
    }

    fn check_at(&self, now_us: u64) -> Result<(), String> {
        if now_us >= self.expires_us {
            return Err("rest transition confirmation expired".into());
        }
        if !self.run.current() {
            return Err("skill definitions changed".into());
        }
        Ok(())
    }
}

impl Client {
    pub fn for_run(&self) -> RunClient {
        RunClient {
            client: self.clone(),
            generation: self.generation.lock().expect("routine generation").clone(),
        }
    }
}

impl RunClient {
    pub fn current(&self) -> bool {
        self.client
            .generation
            .lock()
            .is_ok_and(|current| Arc::ptr_eq(&current, &self.generation))
    }
    /// Bounded admission wait, on the run's thread, never on capture or the
    /// main Skills loop. Both sides check the original deadline, and the
    /// returned permit must be checked immediately before presentation.
    pub fn request(&self, skill: &str, rest: Rest) -> Result<Permit, String> {
        if !self.current() {
            return Err("skill definitions changed".into());
        }
        let (answer, receive) = mpsc::sync_channel(1);
        let issued_us = lamp_ipc::monotonic_us();
        let wait_us = if rest.op == Op::Wake {
            WAKE_US
        } else {
            ADMISSION_US
        };
        let expires_us = issued_us.saturating_add(wait_us);
        self.client
            .send
            .try_send(Asked {
                skill: skill.into(),
                rest,
                issued_us,
                expires_us,
                answer,
                generation: self.generation.clone(),
            })
            .map_err(|_| "rest request queue is unavailable".to_owned())?;
        let response = receive
            .recv_timeout(Duration::from_micros(wait_us))
            .map_err(|_| "rest transition was not confirmed in time".to_owned())?;
        self.received(response, expires_us, lamp_ipc::monotonic_us())
    }

    fn received(
        &self,
        response: Result<Woke, String>,
        expires_us: u64,
        now_us: u64,
    ) -> Result<Permit, String> {
        let woke = response?;
        let permit = Permit {
            run: self.clone(),
            expires_us,
            already: woke == Woke::Already,
        };
        permit.check_at(now_us)?;
        Ok(permit)
    }
}

struct Active {
    request: Request,
    during: Window,
    began_us: u64,
    /// End of the last trusted local window on this host's monotonic clock.
    /// Only an already admitted rest may retain it while trust is absent.
    /// This does not predict future zone changes or daylight-saving shifts.
    window_until_us: u64,
    waiting: Option<Asked>,
    /// Expression said `resting` for this episode at least once.
    admitted: bool,
    resettle: Option<Resettle>,
    /// Re-settles already made this night, this episode included.
    resettled: u8,
}

/// A rest a night sound ended, waiting for the voice to rest again.
struct Stirred {
    resettle: Resettle,
    /// The rest's own renewal window.
    during: Window,
    resettled: u8,
    quiet_since_us: Option<u64>,
}

/// What `humans/faces/tracked` says of the desk now, read as bedtime's
/// `present = false` trigger reads it: a valid list, empty, still within its
/// lifespan. Missing, malformed or stale faces are not an empty desk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Desk {
    Empty,
    Face,
    Unknown,
}

impl Active {
    fn deadline_us(&self) -> u64 {
        self.window_until_us
            .min(self.began_us.saturating_add(MAX_EPISODE_US))
    }

    fn send(&mut self, clock_id: &str, publisher: &mut Publisher, now: u64) -> io::Result<()> {
        let not_after_us = self.deadline_us();
        send(clock_id, publisher, &mut self.request, now, not_after_us)
    }
}

struct Source {
    last_episode: u64,
    active: Option<Active>,
    stirred: Option<Stirred>,
    /// The local day the last night this skill rested began (its rest
    /// window's start day), set when Expression first took a rest. The
    /// next day's wake with no rest left to own answers `Woke::Already`
    /// once. With [`Manager::keep_rested_in`] it is read back at a
    /// start from [`RESTED_FILE`], as `night` is.
    rested_day: Option<i64>,
    /// The same night, kept through that wake: what a `yields_to` asks of
    /// the morning after ([`Manager::rested_day`]), and what is kept in
    /// [`RESTED_FILE`]. It ends when the next night's rest begins, or with
    /// a change to the skill.
    night: Option<i64>,
}

pub struct Manager {
    bus: Bus,
    publisher: Publisher,
    clock_id: String,
    asks: mpsc::Receiver<Asked>,
    /// Asks issued after the current tick's `now` was sampled. A request
    /// stamped before its own ask would let the permit outlive the wake
    /// Expression was sent, so they wait for the next tick's sample.
    early: Vec<Asked>,
    sources: HashMap<String, Source>,
    enabled: Vec<String>,
    definitions: HashMap<String, Skill>,
    generation: Arc<Mutex<Arc<()>>>,
    /// Where the nights are kept ([`RESTED_FILE`]), and what it last held.
    rested_file: Option<(PathBuf, BTreeMap<String, i64>)>,
    /// A fresh receipt from Expression at the last tick.
    connected: bool,
    /// Each skill's last rest ask, if it was refused for a reason that
    /// repeats ([`refusal_repeats`]). An admitted ask, or one refused for a
    /// passing reason, clears it.
    refused: HashMap<String, String>,
}

impl Manager {
    pub fn new(bus: Bus, clock_id: String) -> io::Result<(Self, Client)> {
        let (send, asks) = mpsc::sync_channel(MAX_SKILLS);
        let publisher =
            bus.publisher_declared(Request::TOPIC, &Request::declaration(), Request::SOURCE)?;
        let generation = Arc::new(Mutex::new(Arc::new(())));
        let client = Client {
            send,
            generation: generation.clone(),
        };
        Ok((
            Self {
                bus,
                publisher,
                clock_id,
                asks,
                early: Vec::new(),
                sources: HashMap::new(),
                enabled: Vec::new(),
                definitions: HashMap::new(),
                generation,
                rested_file: None,
                connected: false,
                refused: HashMap::new(),
            },
            client,
        ))
    }

    /// Explicitly replace the allowed names without retaining any ownership.
    /// Callers with complete definitions should use `reload_skills` instead.
    pub fn reload(&mut self, names: Vec<String>, now: u64) {
        self.invalidate_runs();
        for source in self.sources.values_mut() {
            cancel(&mut self.publisher, source, now);
            source.stirred = None;
            source.rested_day = None;
            source.night = None;
        }
        self.definitions.clear();
        self.enabled = names;
        self.keep_rested();
    }

    /// An unrelated edit does not end an admitted rest. Pending runs and
    /// presentation permits still lose their old definition generation.
    pub fn reload_skills(&mut self, skills: &[Skill], now: u64) {
        self.invalidate_runs();
        let definitions: HashMap<_, _> = skills
            .iter()
            .filter(|skill| skill.enabled && skill.is_routine())
            .map(|skill| (skill.name.clone(), skill.clone()))
            .collect();
        for (name, source) in &mut self.sources {
            // A night read back at the start has no definition before the
            // first load: still loaded, it is the same skill's.
            let unchanged = definitions
                .get(name)
                .is_some_and(|new| self.definitions.get(name).is_none_or(|old| old == new));
            let resting = source.active.as_ref().is_some_and(|active| {
                active.waiting.is_none()
                    && active.request.op == Op::Rest
                    && now < active.request.valid_until_us
            });
            if !unchanged || !resting {
                cancel(&mut self.publisher, source, now);
            }
            if !unchanged {
                source.stirred = None;
                source.rested_day = None;
                source.night = None;
            }
        }
        self.enabled = definitions.keys().cloned().collect();
        self.definitions = definitions;
        self.keep_rested();
    }

    /// Why `skill` cannot rest now, if it cannot (#940, the first
    /// builder's P2 on #2744): Expression is not connected
    /// ([`NOT_CONNECTED`]), or its last rest ask was refused for a reason
    /// that repeats. Night sleep's claim is not said while this is so.
    pub fn cannot_rest(&self, skill: &str) -> Option<String> {
        if !self.connected {
            return Some(NOT_CONNECTED.into());
        }
        self.refused.get(skill).cloned()
    }

    /// The local day the last night a routine rested began (its rest
    /// window's start day), the newest of its skills': what a `yields_to`
    /// trigger asks of the night (#1672). Kept through the morning's wake,
    /// until the next night's rest begins, and across a restart with
    /// [`Manager::keep_rested_in`].
    pub fn rested_day(&self) -> Option<i64> {
        self.sources
            .values()
            .filter_map(|source| source.night)
            .max()
    }

    /// The nights rested are kept in `dir` (`--state-dir`), in
    /// [`RESTED_FILE`], and read from it now, before the first load: a
    /// restart between a rested night and its morning still knows the lamp
    /// rested, for the morning's wake and for a `yields_to` (#1672). A line
    /// that is not a skill's name and a day is passed over. Without it, a
    /// restart forgets the night.
    pub fn keep_rested_in(&mut self, dir: &Path) -> Result<Vec<(String, i64)>, String> {
        let file = dir.join(RESTED_FILE);
        let text = match fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(format!("{}: {error}", file.display())),
        };
        let mut kept = BTreeMap::new();
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let (Some(name), Some(day), None) = (words.next(), words.next(), words.next()) else {
                continue;
            };
            let (Ok(day), true) = (day.parse::<i64>(), crate::plain_name(name)) else {
                continue;
            };
            if kept.len() < MAX_SKILLS {
                kept.insert(name.to_owned(), day);
            }
        }
        for (name, day) in &kept {
            let source = self.sources.entry(name.clone()).or_insert(Source {
                last_episode: 0,
                active: None,
                stirred: None,
                rested_day: None,
                night: None,
            });
            source.rested_day = Some(*day);
            source.night = Some(*day);
        }
        let read = kept
            .iter()
            .map(|(name, day)| (name.clone(), *day))
            .collect();
        self.rested_file = Some((file, kept));
        Ok(read)
    }

    /// The nights, written whole where they are kept when they changed: a
    /// file beside it, synced, then moved over it, and the folder synced,
    /// as `off` is. A failure is said, and tried again at the next change.
    fn keep_rested(&mut self) {
        let Some((file, kept)) = &mut self.rested_file else {
            return;
        };
        let nights: BTreeMap<String, i64> = self
            .sources
            .iter()
            .filter_map(|(name, source)| Some((name.clone(), source.night?)))
            .collect();
        if nights == *kept {
            return;
        }
        let lines: String = nights
            .iter()
            .map(|(name, day)| format!("{name} {day}\n"))
            .collect();
        let new = file.with_extension("new");
        let written = (|| -> io::Result<()> {
            let mut out = fs::File::create(&new)?;
            io::Write::write_all(&mut out, lines.as_bytes())?;
            out.sync_all()?;
            fs::rename(&new, &*file)?;
            if let Some(folder) = file.parent() {
                fs::File::open(folder)?.sync_all()?;
            }
            Ok(())
        })();
        match written {
            Ok(()) => *kept = nights,
            Err(error) => eprintln!(
                "lamp-skills: the night rested not kept in {}: {error}",
                file.display()
            ),
        }
    }

    fn invalidate_runs(&mut self) {
        *self.generation.lock().expect("routine generation") = Arc::new(());
        for ask in self.early.drain(..) {
            let _ = ask.answer.try_send(Err("skill definitions changed".into()));
        }
        while let Ok(ask) = self.asks.try_recv() {
            let _ = ask.answer.try_send(Err("skill definitions changed".into()));
        }
    }

    /// Both timestamps use this host's monotonic clock. Capture
    /// `sampled_at_us` no later than the wall sample; time spent obtaining or
    /// delivering that sample must not extend its remaining local window.
    pub fn tick(&mut self, now: u64, sampled_at_us: u64, local: Option<&LocalTime>, unix_ms: u64) {
        let moment = local
            .filter(|_| sampled_at_us <= now)
            .and_then(|local| Moment::at(local, unix_ms).ok());
        let observed = self
            .bus
            .latest(ExpressionState::TOPIC)
            .ok()
            .flatten()
            .and_then(|message| {
                let state: ExpressionState = message.parse().ok()?;
                (state.check().is_ok() && message.header.source == "expression")
                    .then_some((message.header.boot, state))
            });
        let target = observed
            .as_ref()
            .filter(|(_, state)| {
                state.enabled
                    && state.stamp.clock_id == self.clock_id
                    && state.stamp.at_us <= now
                    && now - state.stamp.at_us < 500_000
            })
            .map(|(owner, _)| owner);
        let receipt = self
            .bus
            .latest(Receipt::TOPIC)
            .ok()
            .flatten()
            .and_then(|message| {
                let receipt: Receipt = message.parse().ok()?;
                (receipt.check().is_ok()
                    && receipt.enabled
                    && message.header.source == "expression"
                    && receipt.stamp.clock_id == self.clock_id
                    && receipt.stamp.at_us <= now
                    && now - receipt.stamp.at_us < 500_000)
                    .then_some(receipt)
            });
        self.connected = receipt.is_some();
        let voice = self
            .bus
            .latest(VoiceState::TOPIC)
            .ok()
            .flatten()
            .filter(|message| {
                message.header.at_us <= now && now - message.header.at_us < VOICE_FRESH_US
            })
            .and_then(|message| message.parse::<VoiceState>().ok())
            .map(|state| state.state);
        let desk = self
            .bus
            .latest(Faces::TOPIC)
            .ok()
            .flatten()
            .filter(|message| {
                let lifespan_us = message
                    .header
                    .expires_ms
                    .unwrap_or(crate::FACE_LIFESPAN_MS)
                    .saturating_mul(1_000);
                message.header.at_us <= now && now - message.header.at_us < lifespan_us
            })
            .and_then(|message| message.parse::<Faces>().ok())
            .filter(|faces| faces.check().is_ok())
            .map_or(Desk::Unknown, |faces| {
                if faces.faces.is_empty() {
                    Desk::Empty
                } else {
                    Desk::Face
                }
            });
        for (name, source) in &mut self.sources {
            let Some(active) = &mut source.active else {
                continue;
            };
            let correlated = receipt.as_ref().filter(|receipt| {
                receipt.request.as_ref().is_some_and(|request| {
                    request.skill == active.request.skill
                        && request.skill_session_id == active.request.skill_session_id
                        && request.expression_session_id == active.request.expression_session_id
                        && request.episode == active.request.episode
                })
            });
            let completed = correlated.is_some_and(|receipt| receipt.phase == Phase::Completed);
            let accepted = target == Some(&active.request.expression_session_id)
                && ((active.request.op == Op::Wake && completed)
                    || correlated.is_some_and(|receipt| {
                        active.request.op == Op::Rest
                            && receipt.phase == Phase::Resting
                            && receipt.request.as_ref().is_some_and(|request| {
                                request.op == Op::Rest
                                    && request.stamp.sequence >= active.request.stamp.sequence
                            })
                    }));
            // A terminal tombstone retains its last accepted operation.
            // A newer renewal cannot conceal cancellation/completion.
            let terminal = correlated.is_some_and(|receipt| {
                matches!(
                    receipt.phase,
                    Phase::Completed | Phase::Cancelled | Phase::Refused
                )
            });
            let window_until_us = match moment {
                Some(moment) => active
                    .during
                    .rest_remaining_ms(moment.minute, unix_ms % 60_000)
                    .and_then(|(remaining, carried)| {
                        // Carried into the next night: that night's episode
                        // cap counts from here, as a new rest's would.
                        if carried {
                            active.began_us = now;
                        }
                        sampled_at_us.checked_add(remaining * 1000)
                    }),
                None if active.waiting.is_none() && active.request.op == Op::Rest => {
                    Some(active.window_until_us)
                }
                None => None,
            };
            let expired = active
                .waiting
                .as_ref()
                .is_some_and(|ask| now >= ask.expires_us)
                || now >= active.request.valid_until_us
                || now < active.request.stamp.at_us
                || now.saturating_sub(active.began_us) >= MAX_EPISODE_US
                // Check the old deadline before accepting any clock
                // correction: elapsed windows cannot be reopened.
                || now >= active.window_until_us
                || window_until_us.is_none_or(|until| now >= until)
                || observed.as_ref().is_some_and(|(owner, state)| {
                    owner != &active.request.expression_session_id
                        || !state.enabled
                        || state.stamp.clock_id != self.clock_id
                        || state.stamp.at_us > now
                });
            // A completion from an old owner/window is no authority for
            // following sound, even if that receipt itself is still fresh.
            if expired {
                if active
                    .waiting
                    .as_ref()
                    .is_some_and(|ask| now >= ask.expires_us)
                {
                    self.refused.insert(name.clone(), NOT_ANSWERED.into());
                }
                cancel(&mut self.publisher, source, now);
                continue;
            }
            active.window_until_us = window_until_us.expect("checked window end");
            if accepted && active.request.op == Op::Rest && !active.admitted {
                // The night it rested, by the window's start day: 02:00
                // belongs to the night that began the evening before.
                if let Some(day) = moment
                    .and_then(|moment| active.during.occurrence_day(moment.day, moment.minute))
                {
                    source.rested_day = Some(day);
                    source.night = Some(day);
                }
            }
            if accepted {
                active.admitted = true;
                self.refused.remove(name);
            }
            if accepted && let Some(asked) = active.waiting.take() {
                // A caller that already left cannot leave an active routine.
                if now >= asked.expires_us || asked.answer.try_send(Ok(Woke::Owner)).is_err() {
                    cancel(&mut self.publisher, source, now);
                    continue;
                }
            }
            if let Some(why) = correlated
                .filter(|receipt| receipt.phase == Phase::Refused)
                .map(|receipt| receipt.reason.as_deref().unwrap_or("refused"))
            {
                if refusal_repeats(why) {
                    self.refused.insert(name.clone(), why.into());
                } else {
                    self.refused.remove(name);
                }
            }
            if terminal {
                let cancelled = correlated.is_some_and(|receipt| receipt.phase == Phase::Cancelled);
                if cancelled && active.admitted && active.request.op == Op::Rest {
                    source.stirred = stirred(name, active, voice);
                } else if active.resettled > 0 && !active.admitted {
                    eprintln!(
                        "lamp-skills: skill:{name}: Expression did not take its settling again; it stays awake"
                    );
                }
                cancel(&mut self.publisher, source, now);
                continue;
            }
            // A late sample may use the remainder of the issued lease, but
            // never extend it without fresh evidence for this exact episode.
            // A trusted correction can shorten the next lease immediately,
            // but cannot rewrite an already issued lease without evidence.
            let renew = (now.saturating_sub(active.request.stamp.at_us) >= RENEW_US
                || active.request.valid_until_us > active.deadline_us())
                && target == Some(&active.request.expression_session_id)
                && correlated
                    .is_some_and(|receipt| matches!(receipt.phase, Phase::Resting | Phase::Waking));
            if renew
                && active
                    .send(&self.clock_id, &mut self.publisher, now)
                    .is_err()
            {
                cancel(&mut self.publisher, source, now);
            }
        }
        // A rest a night sound ended settles again once the voice has
        // rested, inside its night window and with the desk empty: the fold
        // and the dim, no run, no wake clip and no daily claim, so bedtime's
        // trigger stays spent. A turn or a face means someone is up.
        for (name, source) in &mut self.sources {
            if source.active.is_some() {
                continue;
            }
            let Some(stir) = &mut source.stirred else {
                continue;
            };
            let Some(moment) = moment else {
                // Pending transitions still require trusted time.
                stir.quiet_since_us = None;
                continue;
            };
            if !stir.resettle.during.contains(moment.minute) || !stir.during.contains(moment.minute)
            {
                eprintln!(
                    "lamp-skills: skill:{name}: the night window ended; it does not settle again"
                );
                source.stirred = None;
                continue;
            }
            if matches!(voice, Some(Voice::Thinking | Voice::Speaking)) {
                eprintln!(
                    "lamp-skills: skill:{name}: the voice took a turn; someone is up, so the night's rest ends"
                );
                source.stirred = None;
                continue;
            }
            if voice != Some(Voice::Resting) {
                stir.quiet_since_us = None;
                continue;
            }
            let since = *stir.quiet_since_us.get_or_insert(now);
            let Some(target) = target.filter(|_| now - since >= stir.resettle.after_us) else {
                continue;
            };
            // Bedtime's own test again, at this moment: a face in view is
            // someone at the desk, and an unknown desk is not an empty one.
            match desk {
                Desk::Empty => {}
                Desk::Face => {
                    eprintln!(
                        "lamp-skills: skill:{name}: a face is in view; someone is up, so the night's rest ends"
                    );
                    source.stirred = None;
                    continue;
                }
                Desk::Unknown => continue,
            }
            let Some(episode) = source.last_episode.checked_add(1) else {
                source.stirred = None;
                continue;
            };
            let Some(window_until_us) = stir
                .during
                .remaining_ms(moment.minute, unix_ms % 60_000)
                .and_then(|remaining| sampled_at_us.checked_add(remaining * 1000))
                .filter(|until| now < *until)
            else {
                source.stirred = None;
                continue;
            };
            let resettled = stir.resettled + 1;
            let mut active = Active {
                request: Request {
                    stamp: ClockStamp {
                        clock_id: self.clock_id.clone(),
                        at_us: now,
                        sequence: 1,
                    },
                    skill: name.clone(),
                    skill_session_id: self.publisher.boot().into(),
                    expression_session_id: target.clone(),
                    episode,
                    op: Op::Rest,
                    valid_until_us: now.saturating_add(3_000_000),
                },
                during: stir.during,
                began_us: now,
                window_until_us,
                waiting: None,
                admitted: false,
                resettle: Some(stir.resettle.clone()),
                resettled,
            };
            eprintln!(
                "lamp-skills: skill:{name}: woke at a night sound; the voice has rested {} s again and no face is in view, so it settles again ({resettled} of {})",
                stir.resettle.after_us / 1_000_000,
                stir.resettle.most
            );
            source.last_episode = episode;
            source.stirred = None;
            if active
                .send(&self.clock_id, &mut self.publisher, now)
                .is_ok()
            {
                source.active = Some(active);
            }
        }
        // Every ask in the channel was issued before it was sent, so the
        // next tick's `now` is later than each deferred one: bounded by the
        // channel, and deferred at most once.
        let mut early = std::mem::take(&mut self.early).into_iter();
        while let Some(ask) = early.next().or_else(|| self.asks.try_recv().ok()) {
            if ask.issued_us > now {
                self.early.push(ask);
                continue;
            }
            let refused = &mut self.refused;
            let mut refuse = |why: &str| {
                if refusal_repeats(why) {
                    refused.insert(ask.skill.clone(), why.into());
                } else {
                    refused.remove(&ask.skill);
                }
                let _ = ask.answer.try_send(Err(why.into()));
            };
            let current = self
                .generation
                .lock()
                .is_ok_and(|generation| Arc::ptr_eq(&generation, &ask.generation));
            if !current
                || now >= ask.expires_us
                || now.saturating_sub(ask.issued_us) >= ADMISSION_US
                || !self.enabled.contains(&ask.skill)
            {
                refuse("skill is no longer enabled or its request expired");
                continue;
            }
            if (ask.rest.op == Op::Rest) != ask.rest.during.is_some() {
                refuse("rest requires a window and other operations have none");
                continue;
            }
            if ask.rest.op != Op::Cancel && moment.is_none() {
                refuse("rest transitions require a trusted local clock");
                continue;
            }
            let Some(target) = target else {
                refuse("Expression is not fresh and enabled");
                continue;
            };
            if ask.rest.op == Op::Rest
                && moment.is_none_or(|moment| {
                    !ask.rest
                        .during
                        .expect("checked rest")
                        .contains(moment.minute)
                })
            {
                refuse("rest window is not current");
                continue;
            }
            if !self.sources.contains_key(&ask.skill) {
                if self.sources.len() >= MAX_SKILLS {
                    refuse("rest source history is full");
                    continue;
                }
                self.sources.insert(
                    ask.skill.clone(),
                    Source {
                        last_episode: 0,
                        active: None,
                        stirred: None,
                        rested_day: None,
                        night: None,
                    },
                );
            }
            let source = self.sources.get_mut(&ask.skill).expect("source created");
            // The skill's own operation ends any waiting re-settle.
            source.stirred = None;
            if ask.rest.op == Op::Cancel {
                cancel(&mut self.publisher, source, now);
                let _ = ask.answer.try_send(Ok(Woke::Owner));
                continue;
            }
            if ask.rest.op == Op::Rest {
                if source.active.is_some() {
                    refuse("a rest episode is already active");
                    continue;
                }
                let Some(episode) = source.last_episode.checked_add(1) else {
                    refuse("rest episodes exhausted");
                    continue;
                };
                let during = ask.rest.during.expect("checked rest");
                // The next night's rest begins: last night's is over.
                let tonight = moment
                    .as_ref()
                    .and_then(|moment| during.occurrence_day(moment.day, moment.minute));
                if source.night.is_some() && source.night != tonight {
                    source.night = None;
                }
                let Some(window_until_us) = moment
                    .and_then(|moment| during.remaining_ms(moment.minute, unix_ms % 60_000))
                    .and_then(|remaining| sampled_at_us.checked_add(remaining * 1000))
                    .filter(|until| now < *until)
                else {
                    refuse("rest window has no current deadline");
                    continue;
                };
                source.last_episode = episode;
                source.active = Some(Active {
                    request: Request {
                        stamp: ClockStamp {
                            clock_id: self.clock_id.clone(),
                            at_us: now,
                            sequence: 1,
                        },
                        skill: ask.skill.clone(),
                        skill_session_id: self.publisher.boot().into(),
                        expression_session_id: target.clone(),
                        episode,
                        op: Op::Rest,
                        valid_until_us: now.saturating_add(3_000_000),
                    },
                    during,
                    began_us: now,
                    window_until_us,
                    waiting: None,
                    admitted: false,
                    resettle: ask.rest.resettle.clone(),
                    resettled: 0,
                });
            }
            let Some(active) = &mut source.active else {
                // The morning after a night it rested, its rest ended early
                // by a sound (or the window): the lamp is already awake.
                // Once, and never for a lamp put to sleep by hand or kept
                // private, which a stretch would move.
                let morning_after = moment.is_some_and(|moment| {
                    source.rested_day.and_then(|day| day.checked_add(1)) == Some(moment.day)
                });
                if ask.rest.op == Op::Wake && morning_after {
                    if let Some(why) = kept_asleep(&self.bus) {
                        refuse(&format!("{why}, so it is not woken"));
                        continue;
                    }
                    source.rested_day = None;
                    let _ = ask.answer.try_send(Ok(Woke::Already));
                    continue;
                }
                refuse("this skill owns no rest to wake");
                continue;
            };
            if active.waiting.is_some() || active.request.expression_session_id != *target {
                refuse("rest admission is pending or its owner changed");
                continue;
            }
            active.request.op = ask.rest.op;
            if active
                .send(&self.clock_id, &mut self.publisher, now)
                .is_err()
            {
                refuse("rest request was not sent");
                cancel(&mut self.publisher, source, now);
                continue;
            }
            active.waiting = Some(ask);
        }
        self.keep_rested();
    }
}

/// The core skill that holds when the lamp sleeps at night (#940).
pub const NIGHT_SLEEP: &str = "night-sleep";

/// What lamp-skills says on `expression/sleep_claim` (#940): while the
/// folder `dir/night-sleep` loads as a skill granted the night's sleep, a
/// claim naming it, on while it is enabled and not `stopped` (turned off by a
/// person, or snoozed). Turned off, it still claims the night: a person
/// chose a lamp that does not sleep, and Expression must not sleep by
/// itself. With no such folder, or one that does not load, no claim, and
/// Expression sleeps by its own schedule (its fallback, for one release).
///
/// On only while the skill can rest (the first builder's P2 on #2744):
/// with `cannot_rest` set ([`Manager::cannot_rest`]), no claim either, so
/// the last one goes stale and Expression's own sleep takes the night,
/// rather than a lamp nobody sleeps.
pub fn sleep_claim(
    dir: &std::path::Path,
    stopped: bool,
    cannot_rest: Option<&str>,
) -> Option<ExpressionSleepClaim> {
    let skill = crate::load(&dir.join(NIGHT_SLEEP)).ok()?;
    let on = skill.enabled && !stopped;
    let granted = skill.permissions.grants("sleep") && !(on && cannot_rest.is_some());
    granted.then_some(ExpressionSleepClaim {
        on,
        skill: skill.name,
    })
}

/// Why the lamp must not be moved: put to sleep by hand (the voice's mute,
/// reason `sleep`), or kept private (the voice's own mute, or the privacy
/// switch closed on `voice/privacy`), as Expression reads them
/// (`crates/expression/src/alive.rs`, `runtime.rs`'s `privacy_muted_in`).
/// The newest word on each, however old: a muted voice that went quiet is
/// still muted, and a privacy word that is not a valid open lease counts as
/// closed. Never said is awake: a lamp without either route.
pub fn kept_asleep(bus: &Bus) -> Option<&'static str> {
    let latest = |topic| bus.latest(topic).ok().flatten();
    if let Some(mute) = latest(VoiceMute::TOPIC)
        .and_then(|message| message.parse::<VoiceMute>().ok())
        .filter(|mute| mute.check().is_ok() && mute.muted)
    {
        return Some(if mute.asleep() {
            "the lamp was put to sleep by hand"
        } else {
            "the lamp's microphones are muted for privacy"
        });
    }
    latest(MicrophonePrivacy::TOPIC)
        .is_some_and(|message| {
            message
                .parse::<MicrophonePrivacy>()
                .ok()
                .filter(|lease| lease.check().is_ok())
                .is_none_or(|lease| lease.physical_muted != Some(false) && !lease.no_slide)
        })
        .then_some("the privacy switch is closed")
}

/// After Expression ended an admitted rest: whether it may settle again,
/// said in the journal either way. Only a night sound (the voice listening,
/// not yet a turn) stirs it; a turn, a hand, privacy, a pause or a reflex
/// ends the night's rest. A listening that becomes a turn ends it later.
fn stirred(name: &str, active: &Active, voice: Option<Voice>) -> Option<Stirred> {
    let resettle = active.resettle.clone()?;
    match voice {
        Some(Voice::Listening) => {}
        Some(Voice::Thinking | Voice::Speaking) => {
            eprintln!(
                "lamp-skills: skill:{name}: Expression ended its rest for a turn; someone is up, so it does not settle again"
            );
            return None;
        }
        Some(Voice::Resting) | None => {
            eprintln!(
                "lamp-skills: skill:{name}: Expression ended its rest, not for a sound; it does not settle again"
            );
            return None;
        }
    }
    if active.resettled >= resettle.most {
        eprintln!(
            "lamp-skills: skill:{name}: woke at a night sound; it has settled again {} times tonight and stays awake",
            resettle.most
        );
        return None;
    }
    eprintln!(
        "lamp-skills: skill:{name}: woke at a night sound; it settles again once the voice has rested {} s with no face in view",
        resettle.after_us / 1_000_000
    );
    Some(Stirred {
        during: active.during,
        resettled: active.resettled,
        quiet_since_us: None,
        resettle,
    })
}

fn send(
    clock_id: &str,
    publisher: &mut Publisher,
    request: &mut Request,
    now: u64,
    not_after_us: u64,
) -> io::Result<()> {
    request.stamp = ClockStamp {
        clock_id: clock_id.into(),
        at_us: now,
        sequence: publisher.totals().published + 1,
    };
    request.valid_until_us = now.saturating_add(3_000_000).min(not_after_us);
    request.check().map_err(io::Error::other)?;
    publisher.publish_json(now, request).map(drop)
}

fn cancel(publisher: &mut Publisher, source: &mut Source, now: u64) {
    if let Some(mut active) = source.active.take() {
        if let Some(ask) = active.waiting.take() {
            let _ = ask
                .answer
                .try_send(Err("rest was cancelled or refused".into()));
        }
        active.request.op = Op::Cancel;
        let clock_id = active.request.stamp.clock_id.clone();
        // Withdrawal remains independently deliverable after the original
        // rest window or episode bound has elapsed.
        let _ = send(&clock_id, publisher, &mut active.request, now, u64::MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const CLOCK: &str = "routine-host";
    const NIGHT: u64 = 1_767_225_600_000 + 23 * 3_600_000;
    struct Fixture {
        _root: tempfile::TempDir,
        manager: Manager,
        client: Client,
        state: Publisher,
        receipt: Publisher,
        now: u64,
    }
    impl Fixture {
        fn clock_policy() -> Self {
            let mut fixture = Self::new();
            // These cases step policy time independently of the host. Keep
            // their queued transport alive through a bounded host stall;
            // the paired control below still exercises stock expiry.
            fixture
                .manager
                .publisher
                .expires_after(Some(Duration::from_secs(5)));
            fixture
        }
        fn new() -> Self {
            let root = tempfile::Builder::new()
                .prefix("rest-")
                .tempdir_in("/tmp")
                .unwrap();
            let bus = Bus::new(root.path().join("b")).unwrap();
            let state = bus
                .publisher_declared(
                    ExpressionState::TOPIC,
                    &ExpressionState::declaration(),
                    "expression",
                )
                .unwrap();
            let receipt = bus
                .publisher_declared(Receipt::TOPIC, &Receipt::declaration(), "expression")
                .unwrap();
            let (mut manager, client) = Manager::new(bus, CLOCK.into()).unwrap();
            let folder = root.path().join("routine");
            std::fs::create_dir(&folder).unwrap();
            std::fs::write(
                folder.join("SKILL.md"),
                "---\nname: routine\ndescription: Check owned rest.\n---\n",
            )
            .unwrap();
            std::fs::write(folder.join("skill.toml"),
                "[permissions]\nbody = true\ntopics = [\"voice/state\", \"clock/local\"]\n[trigger]\ntopic = \"clock/local\"\nfield = \"known\"\nequals = true\n[ask]\npolicy = \"nudge\"\n[[do]]\nrest = { op = \"rest\", during = { from = \"23:00\", to = \"10:00\" } }\n").unwrap();
            manager.reload_skills(&[crate::load(&folder).unwrap()], 10_000_000);
            Self {
                _root: root,
                manager,
                client,
                state,
                receipt,
                now: 10_000_000,
            }
        }
        fn heartbeat(&mut self) {
            let stamp = ClockStamp {
                clock_id: CLOCK.into(),
                at_us: self.now,
                sequence: self.state.totals().published + 1,
            };
            self.state
                .publish_json(
                    self.now,
                    &ExpressionState {
                        stamp,
                        enabled: true,
                        turn: None,
                        request: None,
                        behaviours: vec![],
                        missing: vec![],
                        warnings: vec![],
                        body: false,
                        light: false,
                        asleep: None,
                    },
                )
                .unwrap();
        }
        fn tick(&mut self, wall: u64) {
            self.tick_sample(self.now, wall);
        }
        fn tick_sample(&mut self, sampled_at_us: u64, wall: u64) {
            self.heartbeat();
            self.manager
                .tick(self.now, sampled_at_us, Some(&LocalTime::Offset(0)), wall);
        }
        /// lamp-skills restarted: a fresh manager on the same bus, the
        /// nights read back from `dir` before the first load.
        fn restart(&mut self, dir: &Path) {
            let (mut manager, client) =
                Manager::new(self.manager.bus.clone(), CLOCK.into()).unwrap();
            manager.keep_rested_in(dir).unwrap();
            let routine = crate::load(&self._root.path().join("routine")).unwrap();
            manager.reload_skills(&[routine], self.now);
            self.manager = manager;
            self.client = client;
        }
        fn ask(&self, op: Op) -> mpsc::Receiver<Result<Woke, String>> {
            let (answer, result) = mpsc::sync_channel(1);
            self.client
                .send
                .try_send(Asked {
                    skill: "routine".into(),
                    rest: Rest {
                        op,
                        resettle: None,
                        during: (op == Op::Rest).then(|| Window::parse("23:00", "10:00").unwrap()),
                    },
                    issued_us: self.now,
                    expires_us: self.now
                        + if op == Op::Wake {
                            WAKE_US
                        } else {
                            ADMISSION_US
                        },
                    answer,
                    generation: self.client.generation.lock().unwrap().clone(),
                })
                .unwrap();
            result
        }
        fn observe(&mut self, phase: Phase) {
            let request = self.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request
                .clone();
            self.receipt
                .publish_json(
                    self.now,
                    &Receipt {
                        stamp: ClockStamp {
                            clock_id: CLOCK.into(),
                            at_us: self.now,
                            sequence: self.receipt.totals().published + 1,
                        },
                        expression_session_id: self.state.boot().into(),
                        enabled: true,
                        request: Some(request),
                        phase,
                        reason: None,
                    },
                )
                .unwrap();
        }
        fn admitted(&mut self, phase: Phase) {
            self.observe(phase);
            self.tick(NIGHT);
        }
        fn resting(&mut self) -> Request {
            let rest = self.ask(Op::Rest);
            self.tick(NIGHT);
            self.admitted(Phase::Resting);
            assert_eq!(rest.try_recv().unwrap(), Ok(Woke::Owner));
            self.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request
                .clone()
        }
    }
    fn next_request(
        requests: &mut lamp_bus::Subscriber,
        fixture: &Fixture,
        phase: Option<Op>,
        expected: Op,
        stage: &str,
        previous: Option<&Request>,
    ) -> Request {
        let began = std::time::Instant::now();
        let expired_before = requests.expired();
        let received = requests.recv(Duration::from_millis(50));
        // Format only on failure; diagnostics must not delay the next phase.
        let context = || {
            format!(
                "stage={stage} phase={phase:?} expected={expected:?} waited={:?} now_us={} publisher={:?} expired={expired_before}->{} malformed={} wrong_type={} active={:?} previous={previous:?}",
                began.elapsed(),
                fixture.now,
                fixture.manager.publisher.totals(),
                requests.expired(),
                requests.malformed(),
                requests.wrong_type(),
                fixture
                    .manager
                    .sources
                    .get("routine")
                    .and_then(|source| source.active.as_ref())
                    .map(|active| &active.request),
            )
        };
        let message = received
            .unwrap_or_else(|error| panic!("request receive failed: {error}; {}", context()))
            .unwrap_or_else(|| panic!("no request received; {}", context()));
        message.parse().unwrap_or_else(|error| {
            panic!(
                "request parse failed: {error}; header={:?}; {}",
                message.header,
                context()
            )
        })
    }
    #[test]
    fn a_host_stall_expires_the_request_header_without_changing_the_manager_deadline() {
        use std::{
            os::fd::AsFd,
            time::{Instant, SystemTime, UNIX_EPOCH},
        };

        #[derive(Debug)]
        struct Probe {
            lifetime_ms: u64,
            declared_ms: Option<u64>,
            header: lamp_bus::Header,
            queued: Request,
            expected: Request,
            active_after: Option<Request>,
            now_us: [u64; 2],
            window_until_us: [u64; 2],
            totals: [lamp_bus::Totals; 3],
            subscribers: usize,
            expired: [u64; 2],
            malformed: u64,
            wrong_type: u64,
            slept: Duration,
            elapsed: Duration,
            wall_ms: [u64; 3],
            received: Result<Option<(lamp_bus::Header, Request)>, String>,
        }
        let wall_ms = || -> u64 {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis()
                .try_into()
                .unwrap()
        };
        let mut probes = Vec::with_capacity(2);
        for extended in [false, true] {
            let mut f = if extended {
                Fixture::clock_policy()
            } else {
                Fixture::new()
            };
            let mut requests = f.manager.bus.subscribe(Request::TOPIC, "stall").unwrap();
            let before = f.manager.publisher.totals();
            let now_us = f.now;
            let _pending = f.ask(Op::Rest);
            let began = Instant::now();
            let wall_before = wall_ms();
            f.tick(NIGHT);
            let active = f.manager.sources["routine"].active.as_ref().unwrap();
            let expected = active.request.clone();
            let window_until_us = active.window_until_us;
            let published = f.manager.publisher.totals();

            // Peek this subscriber's actual queued frame without consuming
            // it or introducing a second receiver that could race the stall.
            let mut bytes = vec![0; lamp_bus::MOST_BYTES + 1];
            let (_, length) = rustix::net::recv(
                requests.as_fd(),
                &mut bytes[..],
                rustix::net::RecvFlags::PEEK | rustix::net::RecvFlags::DONTWAIT,
            )
            .unwrap();
            let end = bytes[..length]
                .iter()
                .position(|byte| *byte == b'\n')
                .unwrap();
            let header = serde_json::from_slice(&bytes[..end]).unwrap();
            let queued = serde_json::from_slice(&bytes[end + 1..length]).unwrap();
            let expired_before = requests.expired();
            // No business-clock advance or Manager tick occurs during this
            // host stall. Both arms leave the same pending payload queued.
            let stall = Instant::now();
            std::thread::sleep(Duration::from_millis(700));
            let slept = stall.elapsed();
            let wall_read = wall_ms();
            let received = requests.recv(Duration::from_millis(50));
            let wall_after = wall_ms();
            let elapsed = began.elapsed();
            let received = received
                .map_err(|error| error.to_string())
                .and_then(|message| {
                    message
                        .map(|message| {
                            let request = message.parse().map_err(|error| error.to_string())?;
                            Ok((message.header, request))
                        })
                        .transpose()
                });
            let active_after = f.manager.sources["routine"].active.as_ref().unwrap();
            probes.push(Probe {
                lifetime_ms: if extended { 5_000 } else { 500 },
                declared_ms: f.manager.publisher.declaration().unwrap().lifespan_ms,
                header,
                queued,
                expected,
                active_after: Some(active_after.request.clone()),
                now_us: [now_us, f.now],
                window_until_us: [window_until_us, active_after.window_until_us],
                totals: [before, published, f.manager.publisher.totals()],
                subscribers: f.manager.publisher.subscribers().len(),
                expired: [expired_before, requests.expired()],
                malformed: requests.malformed(),
                wrong_type: requests.wrong_type(),
                slept,
                elapsed,
                wall_ms: [wall_before, wall_read, wall_after],
                received,
            });
        }
        // Preserve both observations before any diagnostic I/O or outcome
        // assertion. This proves a controlled mechanism, not the cause of an
        // earlier missing request; a clock step/overrun invalidates the pair.
        eprintln!("routine request transport probes: {probes:#?}");
        for probe in &probes {
            assert!(
                probe.slept >= Duration::from_millis(700)
                    && probe.elapsed < Duration::from_secs(5)
                    && probe.wall_ms[0] <= probe.header.at_unix_ms
                    && probe.header.at_unix_ms <= probe.wall_ms[1]
                    && probe.wall_ms[1] <= probe.wall_ms[2],
                "probe clock step or host overrun; no historical cause established: {probe:?}"
            );
            assert_eq!(probe.declared_ms, Some(500));
            assert_eq!(probe.header.expires_ms, Some(probe.lifetime_ms));
            assert_eq!(probe.header.kind, lamp_bus::Kind::Event);
            assert_eq!(probe.header.topic, Request::TOPIC);
            assert_eq!(probe.header.source, Request::SOURCE);
            assert_eq!(probe.header.seq, probe.expected.stamp.sequence);
            assert_eq!(probe.header.at_us, probe.expected.stamp.at_us);
            assert_eq!(probe.queued, probe.expected);
            probe.expected.check().unwrap();
            assert_eq!(probe.active_after.as_ref(), Some(&probe.expected));
            assert_eq!(probe.now_us[0], probe.now_us[1]);
            assert_eq!(probe.window_until_us[0], probe.window_until_us[1]);
            assert_eq!(probe.expected.valid_until_us, probe.now_us[0] + 3_000_000);
            let [before, published, after] = probe.totals;
            assert_eq!(published.published - before.published, 1);
            assert_eq!(published.delivered - before.delivered, 1);
            assert_eq!(published.dropped - before.dropped, 0);
            assert_eq!(after, published);
            assert_eq!(probe.subscribers, 1);
            assert_eq!(probe.expired[0], 0);
            assert_eq!(probe.malformed, 0);
            assert_eq!(probe.wrong_type, 0);
            let expired = probe.lifetime_ms == 500;
            for wall in &probe.wall_ms[1..] {
                assert_eq!(
                    probe.header.expired_at(*wall),
                    expired,
                    "probe left its controlled header-age window; clock step/overrun: {probe:?}"
                );
            }
            let received = probe.received.as_ref().expect("probe receive/parse failed");
            if expired {
                assert!(received.is_none());
                assert_eq!(probe.expired[1], 1);
            } else {
                let (header, request) = received.as_ref().expect("bounded header should survive");
                assert_eq!(header, &probe.header);
                assert_eq!(request, &probe.expected);
                assert_eq!(probe.expired[1], 0);
            }
        }
        // Fresh fixtures have distinct session IDs. Their exact wire payloads
        // match their own Managers above; these logical fields match the pair.
        let ordinary = &probes[0].expected;
        let bounded = &probes[1].expected;
        assert_eq!(ordinary.stamp, bounded.stamp);
        assert_eq!(ordinary.skill, bounded.skill);
        assert_eq!(ordinary.episode, bounded.episode);
        assert_eq!(ordinary.op, bounded.op);
        assert_eq!(ordinary.valid_until_us, bounded.valid_until_us);
    }
    #[test]
    fn an_ask_issued_after_the_tick_sample_waits_so_its_request_is_not_stamped_earlier() {
        let mut f = Fixture::new();
        let initial = f.resting();
        f.now += 1_000;
        let (answer, wake) = mpsc::sync_channel(1);
        // The run thread sampled its clock after the main loop sampled `now`.
        let issued_us = f.now + 35;
        f.client
            .send
            .try_send(Asked {
                skill: "routine".into(),
                rest: Rest {
                    op: Op::Wake,
                    resettle: None,
                    during: None,
                },
                issued_us,
                expires_us: issued_us + WAKE_US,
                answer,
                generation: f.client.generation.lock().unwrap().clone(),
            })
            .unwrap();
        let published = f.manager.publisher.totals().published;
        f.tick(NIGHT);
        assert_eq!(f.manager.publisher.totals().published, published);
        assert!(wake.try_recv().is_err(), "deferred, not refused");
        f.now += 1_000;
        f.tick(NIGHT);
        let active = f.manager.sources["routine"].active.as_ref().unwrap();
        assert_eq!(active.request.op, Op::Wake);
        assert_eq!(active.request.episode, initial.episode);
        // The permit's deadline never outlives the wake Expression was sent.
        assert!(issued_us + WAKE_US <= active.request.stamp.at_us + WAKE_US);
        assert!(active.waiting.is_some());
    }
    #[test]
    fn a_late_sample_keeps_only_the_existing_lease_and_recovers_the_same_episode() {
        let mut f = Fixture::new();
        let initial = f.resting();
        let published = f.manager.publisher.totals().published;
        f.now += 600_000;
        // Both latest samples are late, but the issued lease remains valid.
        f.manager
            .tick(f.now, f.now, Some(&LocalTime::Offset(0)), NIGHT);
        assert_eq!(f.manager.publisher.totals().published, published);
        assert_eq!(
            f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request,
            initial
        );
        f.now += 600_000;
        f.tick(NIGHT); // A fresh state alone cannot renew an unseen episode.
        assert_eq!(f.manager.publisher.totals().published, published);
        assert_eq!(
            f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request,
            initial
        );
        f.admitted(Phase::Resting);
        let recovered = &f.manager.sources["routine"]
            .active
            .as_ref()
            .unwrap()
            .request;
        assert_eq!(recovered.episode, initial.episode);
        assert_eq!(
            recovered.expression_session_id,
            initial.expression_session_id
        );
        assert!(recovered.stamp.sequence > initial.stamp.sequence);
        assert_eq!(recovered.valid_until_us, f.now + 3_000_000);
    }
    #[test]
    fn missing_receipts_cannot_extend_an_issued_rest_lease() {
        let mut f = Fixture::new();
        let initial = f.resting();
        let published = f.manager.publisher.totals().published;
        for delta in [1_000_000, 2_000_000] {
            f.now = initial.stamp.at_us + delta;
            f.tick(NIGHT);
            assert_eq!(f.manager.publisher.totals().published, published);
            assert!(f.manager.sources["routine"].active.is_some());
        }
        f.now = initial.valid_until_us;
        f.tick(NIGHT);
        assert!(f.manager.sources["routine"].active.is_none());
        assert_eq!(f.manager.publisher.totals().published, published + 1);
    }
    #[test]
    fn clock_loss_keeps_admitted_rest_only_until_the_last_trusted_window_end() {
        for restored_at_end in [false, true] {
            let mut f = Fixture::new();
            let initial = f.resting();
            // A later trusted sample, not admission time, bounds the hold.
            // 09:59:57.749 has exactly 2.251 seconds left in this window.
            let wall = NIGHT + 11 * 3_600_000 - 2_251;
            f.now += 1_000;
            f.observe(Phase::Resting);
            f.tick(wall);
            let until = f.now + 2_251_000;
            for delta in [1_000_000, 2_000_000, 2_250_999] {
                f.now = until - 2_251_000 + delta;
                f.observe(Phase::Resting);
                f.tick(0); // The live loop passes zero when clock trust is absent.
                let active = f.manager.sources["routine"].active.as_ref().unwrap();
                assert!(active.waiting.is_none());
                assert_eq!(active.request.op, Op::Rest);
                assert_eq!(active.request.episode, initial.episode);
                assert_eq!(
                    active.request.expression_session_id,
                    initial.expression_session_id
                );
                assert_eq!(active.window_until_us, until);
                assert!(active.request.stamp.sequence > initial.stamp.sequence);
            }
            let published = f.manager.publisher.totals().published;
            f.now = until;
            f.observe(Phase::Resting);
            // Even a restored clock corrected back into the window cannot
            // reopen an episode whose previous monotonic bound has elapsed.
            f.tick(if restored_at_end { wall } else { 0 });
            assert!(f.manager.sources["routine"].active.is_none());
            assert_eq!(f.manager.publisher.totals().published, published + 1);
            f.now += 1;
            f.tick(wall);
            assert!(f.manager.sources["routine"].active.is_none());
            assert_eq!(f.manager.publisher.totals().published, published + 1);
        }
    }
    #[test]
    fn published_rest_and_wake_leases_end_at_the_window_if_skills_stalls() {
        for wake in [false, true] {
            let mut f = Fixture::new();
            let mut requests = f.manager.bus.subscribe(Request::TOPIC, "expiry").unwrap();
            let wall = NIGHT + 11 * 3_600_000 - 2_251;
            let until = f.now + 2_251_000;
            let rest = f.ask(Op::Rest);
            f.tick(wall);
            let initial: Request = requests
                .recv(Duration::from_millis(50))
                .unwrap()
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(initial.valid_until_us, until);
            f.observe(Phase::Resting);
            f.tick(wall);
            assert_eq!(rest.try_recv().unwrap(), Ok(Woke::Owner));
            let pending_wake = if wake {
                f.now += 500_000;
                let pending = f.ask(Op::Wake);
                f.tick(wall + 500);
                Some(pending)
            } else {
                f.now += RENEW_US;
                f.observe(Phase::Resting);
                f.tick(0);
                None
            };
            let issued: Request = requests
                .recv(Duration::from_millis(50))
                .unwrap()
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(issued.op, if wake { Op::Wake } else { Op::Rest });
            assert_eq!(issued.episode, initial.episode);
            assert_eq!(issued.valid_until_us, until);
            assert!(issued.valid_until_us > issued.stamp.at_us);
            issued.check().unwrap();
            // No Manager tick is needed for the published lease to expire
            // at Expression: its own monotonic check uses this exact bound.
            f.now = until;
            assert_eq!(issued.valid_until_us, f.now);
            assert!(requests.recv(Duration::ZERO).unwrap().is_none());
            // When Skills runs again, withdrawal is still freshly valid.
            f.tick(0);
            let cancelled: Request = requests
                .recv(Duration::from_millis(50))
                .unwrap()
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(cancelled.op, Op::Cancel);
            assert_eq!(cancelled.valid_until_us, f.now + 3_000_000);
            cancelled.check().unwrap();
            if let Some(pending) = pending_wake {
                assert!(pending.try_recv().unwrap().is_err());
            }
        }
    }
    #[test]
    fn a_trusted_correction_shortens_the_published_lease_only_with_fresh_evidence() {
        for fresh in [false, true] {
            let mut f = Fixture::new();
            let mut requests = f.manager.bus.subscribe(Request::TOPIC, "expiry").unwrap();
            let initial = f.resting();
            requests.recv(Duration::from_millis(50)).unwrap().unwrap();
            f.now += 600_000; // Before renewal, after the previous receipt aged out.
            let wall = NIGHT + 11 * 3_600_000 - 1_251;
            let until = f.now + 1_251_000;
            assert!(until < initial.valid_until_us);
            if fresh {
                f.observe(Phase::Resting);
            }
            f.tick(wall);
            if !fresh {
                assert!(requests.recv(Duration::ZERO).unwrap().is_none());
                assert_eq!(
                    f.manager.sources["routine"]
                        .active
                        .as_ref()
                        .unwrap()
                        .request,
                    initial
                );
                // A correction does not retroactively change the receiver's
                // old lease. Fresh matching evidence permits a shorter one.
                f.observe(Phase::Resting);
                f.tick(wall);
            }
            let shortened: Request = requests
                .recv(Duration::from_millis(50))
                .unwrap()
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(shortened.op, Op::Rest);
            assert_eq!(shortened.episode, initial.episode);
            assert_eq!(shortened.stamp.at_us, f.now);
            assert!(shortened.stamp.at_us - initial.stamp.at_us < RENEW_US);
            assert_eq!(shortened.valid_until_us, until);
            shortened.check().unwrap();
        }
    }
    #[test]
    fn a_delayed_wall_sample_cannot_renew_or_admit_after_its_window_ended() {
        for existing in [None, Some(Op::Rest), Some(Op::Wake)] {
            let mut f = Fixture::clock_policy();
            let mut requests = f.manager.bus.subscribe(Request::TOPIC, "delay").unwrap();
            let mut pending = None;
            if existing.is_some() {
                f.resting();
                next_request(
                    &mut requests,
                    &f,
                    existing,
                    Op::Rest,
                    "expired/bootstrap",
                    None,
                );
                if existing == Some(Op::Wake) {
                    pending = Some(f.ask(Op::Wake));
                    f.tick(NIGHT);
                    next_request(&mut requests, &f, existing, Op::Wake, "expired/wake", None);
                }
            }
            let sampled_at_us = f.now;
            let published = f.manager.publisher.totals().published;
            f.now += 300_000;
            if existing.is_some() {
                f.observe(if existing == Some(Op::Wake) {
                    Phase::Completed
                } else {
                    Phase::Resting
                });
            } else {
                pending = Some(f.ask(Op::Rest));
            }
            // The sample was 100 ms before the end, but reaches this tick
            // 300 ms later. Fresh Expression evidence cannot recover it.
            let previous = f
                .manager
                .sources
                .get("routine")
                .and_then(|source| source.active.as_ref().map(|active| active.request.clone()));
            f.tick_sample(sampled_at_us, NIGHT + 11 * 3_600_000 - 100);
            assert!(
                f.manager
                    .sources
                    .values()
                    .all(|source| source.active.is_none())
            );
            if let Some(pending) = pending {
                assert!(pending.try_recv().unwrap().is_err());
            }
            if existing.is_some() {
                assert_eq!(
                    next_request(
                        &mut requests,
                        &f,
                        existing,
                        Op::Cancel,
                        "expired/cancel",
                        previous.as_ref(),
                    )
                    .op,
                    Op::Cancel
                );
                assert_eq!(f.manager.publisher.totals().published, published + 1);
            } else {
                assert_eq!(f.manager.publisher.totals().published, published);
            }
            assert!(requests.recv(Duration::ZERO).unwrap().is_none());
        }
    }
    #[test]
    fn a_delayed_sample_inside_the_window_keeps_its_original_deadline() {
        for existing in [None, Some(Op::Rest), Some(Op::Wake)] {
            let mut f = Fixture::clock_policy();
            let mut requests = f.manager.bus.subscribe(Request::TOPIC, "delay").unwrap();
            let mut pending = None;
            if existing.is_some() {
                f.resting();
                next_request(
                    &mut requests,
                    &f,
                    existing,
                    Op::Rest,
                    "inside/bootstrap",
                    None,
                );
                if existing == Some(Op::Wake) {
                    pending = Some(f.ask(Op::Wake));
                    f.tick(NIGHT);
                    next_request(&mut requests, &f, existing, Op::Wake, "inside/wake", None);
                }
            }
            let sampled_at_us = f.now;
            let wall = NIGHT + 11 * 3_600_000 - 500;
            let until = sampled_at_us + 500_000;
            f.now += 300_000;
            let phase = if existing == Some(Op::Wake) {
                Phase::Waking
            } else {
                Phase::Resting
            };
            if existing.is_some() {
                f.observe(phase);
            } else {
                pending = Some(f.ask(Op::Rest));
            }
            f.tick_sample(sampled_at_us, wall);
            let issued = next_request(
                &mut requests,
                &f,
                existing,
                existing.unwrap_or(Op::Rest),
                "inside/shortened",
                None,
            );
            assert_eq!(issued.op, existing.unwrap_or(Op::Rest));
            assert_eq!(issued.valid_until_us, until);
            assert_eq!(issued.valid_until_us - f.now, 200_000);
            if existing.is_none() {
                f.observe(Phase::Resting);
                f.tick_sample(sampled_at_us, wall);
                assert_eq!(pending.take().unwrap().try_recv().unwrap(), Ok(Woke::Owner));
            }
            // Delivering that same trusted sample later cannot rejuvenate
            // its cached deadline, including during a pending wake.
            f.now += 100_000;
            f.observe(phase);
            f.tick_sample(sampled_at_us, wall);
            let active = f.manager.sources["routine"].active.as_ref().unwrap();
            assert_eq!(active.window_until_us, until);
            assert_eq!(active.request.valid_until_us, until);
            assert!(requests.recv(Duration::ZERO).unwrap().is_none());
            let previous = active.request.clone();
            f.now = until;
            f.observe(phase);
            f.tick_sample(sampled_at_us, wall);
            assert!(f.manager.sources["routine"].active.is_none());
            assert_eq!(
                next_request(
                    &mut requests,
                    &f,
                    existing,
                    Op::Cancel,
                    "inside/cancel",
                    Some(&previous),
                )
                .op,
                Op::Cancel
            );
            if let Some(pending) = pending {
                assert!(pending.try_recv().unwrap().is_err());
            }
        }
    }
    #[test]
    fn a_future_wall_sample_cannot_admit_work_or_extend_the_cached_window() {
        let mut f = Fixture::clock_policy();
        let mut requests = f.manager.bus.subscribe(Request::TOPIC, "future").unwrap();
        let rest = f.ask(Op::Rest);
        f.tick_sample(f.now + 1, NIGHT);
        assert!(rest.try_recv().unwrap().is_err());
        assert!(requests.recv(Duration::ZERO).unwrap().is_none());
        let initial = f.resting();
        next_request(
            &mut requests,
            &f,
            Some(Op::Rest),
            Op::Rest,
            "future/bootstrap",
            None,
        );
        let until = f.manager.sources["routine"]
            .active
            .as_ref()
            .unwrap()
            .window_until_us;
        f.now += RENEW_US;
        f.observe(Phase::Resting);
        let wake = f.ask(Op::Wake);
        f.tick_sample(f.now + 1, NIGHT);
        assert!(wake.try_recv().unwrap().is_err());
        let continued = next_request(
            &mut requests,
            &f,
            Some(Op::Rest),
            Op::Rest,
            "future/continued",
            None,
        );
        assert_eq!(continued.op, Op::Rest);
        assert_eq!(continued.episode, initial.episode);
        assert_eq!(
            f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .window_until_us,
            until
        );
        assert!(continued.valid_until_us <= until);
        assert!(requests.recv(Duration::ZERO).unwrap().is_none());
    }
    /// main takes the clock before `tick` reads Expression's latest state,
    /// so a state published in between is stamped after `now`: newer than
    /// the pass's clock, not from the future. It must not end a held rest
    /// (`routine_service`'s reload test failed so on ARM64, 31 Oct).
    #[test]
    fn a_state_stamped_after_the_ticks_clock_keeps_the_held_rest() {
        let mut f = Fixture::new();
        let held = f.resting();
        f.now += 1;
        f.heartbeat();
        f.now -= 1;
        f.manager
            .tick(f.now, f.now, Some(&LocalTime::Offset(0)), NIGHT);
        let active = f.manager.sources["routine"].active.as_ref();
        assert_eq!(
            active.map(|active| (active.request.op, active.request.episode)),
            Some((Op::Rest, held.episode)),
            "a state 1 µs ahead of the pass's clock ended the rest"
        );
    }
    /// Night sleep's 23:00 to 22:59 (#940): a rest still held at 22:58 is
    /// carried through the gap minute into the next night, its episode cap
    /// counted from the handover, with no cancel.
    #[test]
    fn the_longest_window_carries_a_held_rest_into_the_next_night() {
        let mut f = Fixture::new();
        f.now = MAX_EPISODE_US + 10_000_000;
        let initial = f.resting();
        let active = f
            .manager
            .sources
            .get_mut("routine")
            .unwrap()
            .active
            .as_mut()
            .unwrap();
        active.during = Window::parse("23:00", "22:59").unwrap();
        // Taken a day ago: one second short of its episode cap.
        active.began_us = f.now + 1_000_000 - MAX_EPISODE_US;
        let day = 24 * 3_600_000;
        f.now += 500_000;
        f.observe(Phase::Resting);
        f.tick(NIGHT + day - 90_000); // 22:58:30
        let active = f.manager.sources["routine"].active.as_ref().unwrap();
        assert_eq!(active.began_us, f.now);
        assert_eq!(
            active.window_until_us,
            f.now + (90_000 + 1439 * 60_000) * 1000
        );
        f.now += 1_000_000;
        f.observe(Phase::Resting);
        f.tick(NIGHT + day - 30_000); // 22:59:30, the gap minute
        let active = f.manager.sources["routine"].active.as_ref().unwrap();
        assert_eq!(active.request.op, Op::Rest);
        assert_eq!(active.request.episode, initial.episode);
        assert_eq!(active.request.valid_until_us, f.now + 3_000_000);
    }
    #[test]
    fn publication_stops_at_the_episode_cap_but_cancel_remains_deliverable() {
        let mut f = Fixture::new();
        let mut requests = f.manager.bus.subscribe(Request::TOPIC, "expiry").unwrap();
        f.now = MAX_EPISODE_US + 10_000_000;
        f.resting();
        requests.recv(Duration::from_millis(50)).unwrap().unwrap();
        // A still-leased episode snapshot with 1.251 seconds left in its
        // overall lifetime; the local window remains much longer.
        let active = f
            .manager
            .sources
            .get_mut("routine")
            .unwrap()
            .active
            .as_mut()
            .unwrap();
        active.began_us = f.now - MAX_EPISODE_US + 1_251_000;
        active.send(CLOCK, &mut f.manager.publisher, f.now).unwrap();
        let issued: Request = requests
            .recv(Duration::from_millis(50))
            .unwrap()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(issued.valid_until_us, f.now + 1_251_000);
        f.now = issued.valid_until_us;
        let published = f.manager.publisher.totals().published;
        assert!(active.send(CLOCK, &mut f.manager.publisher, f.now).is_err());
        assert_eq!(f.manager.publisher.totals().published, published);
        assert!(requests.recv(Duration::ZERO).unwrap().is_none());
        let source = f.manager.sources.get_mut("routine").unwrap();
        cancel(&mut f.manager.publisher, source, f.now);
        let cancelled: Request = requests
            .recv(Duration::from_millis(50))
            .unwrap()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(cancelled.op, Op::Cancel);
        assert_eq!(cancelled.valid_until_us, f.now + 3_000_000);
        cancelled.check().unwrap();
    }
    #[test]
    fn clock_loss_does_not_renew_without_fresh_matching_expression_evidence() {
        let mut f = Fixture::new();
        let initial = f.resting();
        let published = f.manager.publisher.totals().published;
        f.now += RENEW_US;
        f.tick(0); // Fresh owner, stale receipt.
        assert_eq!(f.manager.publisher.totals().published, published);
        f.now += RENEW_US;
        f.observe(Phase::Resting); // Fresh receipt, stale owner.
        f.manager.tick(f.now, f.now, Some(&LocalTime::Offset(0)), 0);
        assert_eq!(f.manager.publisher.totals().published, published);
        assert_eq!(
            f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request,
            initial
        );
        f.now = initial.valid_until_us;
        f.observe(Phase::Resting);
        f.tick(0); // Fresh evidence at expiry is too late to extend the lease.
        assert!(f.manager.sources["routine"].active.is_none());
        assert_eq!(f.manager.publisher.totals().published, published + 1);
        f.now += 1;
        f.tick(NIGHT);
        assert!(f.manager.sources["routine"].active.is_none());
        assert_eq!(f.manager.publisher.totals().published, published + 1);
    }
    #[test]
    fn clock_loss_cancels_pending_admission_and_wake_even_with_a_fresh_ack() {
        for op in [Op::Rest, Op::Wake] {
            let mut f = Fixture::new();
            if op == Op::Wake {
                f.resting();
            }
            let pending = f.ask(op);
            f.tick(NIGHT);
            f.now += 1;
            f.observe(if op == Op::Rest {
                Phase::Resting
            } else {
                Phase::Completed
            });
            f.tick(0);
            assert!(pending.try_recv().unwrap().is_err());
            assert!(f.manager.sources["routine"].active.is_none());
        }
    }
    #[test]
    fn an_untrusted_clock_cannot_start_rest_or_wake_an_admitted_rest() {
        let mut f = Fixture::new();
        let pending = f.ask(Op::Rest);
        f.tick(0);
        assert!(pending.try_recv().unwrap().is_err());
        assert!(f.manager.sources.is_empty());
        assert_eq!(f.manager.publisher.totals().published, 0);
        let initial = f.resting();
        let published = f.manager.publisher.totals().published;
        let wake = f.ask(Op::Wake);
        f.now += 1;
        f.tick(0);
        assert!(wake.try_recv().unwrap().is_err());
        assert_eq!(
            f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request,
            initial
        );
        assert_eq!(f.manager.publisher.totals().published, published);
    }
    #[test]
    fn a_trusted_outside_window_sample_cancels_rest_immediately() {
        let mut f = Fixture::new();
        f.resting();
        f.now += 1;
        assert!(
            f.now
                < f.manager.sources["routine"]
                    .active
                    .as_ref()
                    .unwrap()
                    .window_until_us
        );
        f.observe(Phase::Resting);
        f.tick(NIGHT + 11 * 3_600_000);
        assert!(f.manager.sources["routine"].active.is_none());
        let published = f.manager.publisher.totals().published;
        f.tick(NIGHT);
        assert!(f.manager.sources["routine"].active.is_none());
        assert_eq!(f.manager.publisher.totals().published, published);
    }
    #[test]
    fn unrelated_reload_retains_admitted_rest_but_invalidates_old_run_authority() {
        let mut f = Fixture::new();
        let initial = f.resting();
        let old_run = f.client.for_run();
        let owner = f.manager.definitions["routine"].clone();
        let mut other = owner.clone();
        other.name = "another-routine".into();
        f.manager
            .reload_skills(&[owner.clone(), other.clone()], f.now + 1);
        assert!(!old_run.current());
        assert_eq!(
            f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request,
            initial
        );
        other.description = "An unrelated definition changed.".into();
        f.manager.reload_skills(&[owner, other], f.now + 2);
        assert_eq!(
            f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request,
            initial
        );
    }
    #[test]
    fn an_edited_disabled_or_removed_owner_cannot_keep_its_rest() {
        for change in 0..3 {
            let mut f = Fixture::new();
            f.resting();
            let mut owner = f.manager.definitions["routine"].clone();
            let definitions = match change {
                0 => {
                    owner.description = "Changed owner.".into();
                    vec![owner]
                }
                1 => {
                    owner.enabled = false;
                    vec![owner]
                }
                _ => vec![],
            };
            f.manager.reload_skills(&definitions, f.now + 1);
            assert!(
                f.manager.sources["routine"].active.is_none(),
                "change {change}"
            );
        }
    }
    #[test]
    fn an_unchanged_definition_cannot_retain_pending_admission_or_wake() {
        for wake in [false, true] {
            let mut f = Fixture::new();
            if wake {
                f.resting();
            }
            let pending = f.ask(if wake { Op::Wake } else { Op::Rest });
            f.tick(NIGHT);
            let owner = f.manager.definitions["routine"].clone();
            f.manager.reload_skills(&[owner], f.now + 1);
            assert!(pending.try_recv().unwrap().is_err());
            assert!(f.manager.sources["routine"].active.is_none());
        }
    }
    /// What night sleep's claim reads (the first builder's P2 on #2744): no
    /// fresh receipt, or a last ask refused for a reason that repeats or not
    /// answered, is a skill that cannot rest. An admission, or a refusal
    /// that passes with the moment, clears it.
    #[test]
    fn a_skill_cannot_rest_with_no_receipt_or_a_refusal_that_repeats() {
        let mut f = Fixture::new();
        let say = |f: &mut Fixture, phase: Phase, reason: Option<&str>| {
            let request = f
                .manager
                .sources
                .get("routine")
                .and_then(|source| source.active.as_ref().map(|active| active.request.clone()));
            let receipt = Receipt {
                stamp: ClockStamp {
                    clock_id: CLOCK.into(),
                    at_us: f.now,
                    sequence: f.receipt.totals().published + 1,
                },
                expression_session_id: f.state.boot().into(),
                enabled: true,
                request: request.filter(|_| phase != Phase::Idle),
                phase,
                reason: reason.map(str::to_owned),
            };
            f.receipt.publish_json(f.now, &receipt).unwrap();
            f.tick(NIGHT);
        };
        f.tick(NIGHT);
        assert_eq!(
            f.manager.cannot_rest("routine").as_deref(),
            Some(NOT_CONNECTED)
        );
        say(&mut f, Phase::Idle, None);
        assert_eq!(f.manager.cannot_rest("routine"), None);
        let stale = "rest observation is future, stale or expired";
        let passing = "rest owner is unavailable or another routine owns it";
        for (reason, cannot) in [(stale, Some(stale)), (passing, None), (stale, Some(stale))] {
            let _rest = f.ask(Op::Rest);
            f.tick(NIGHT);
            say(&mut f, Phase::Refused, Some(reason));
            assert_eq!(f.manager.cannot_rest("routine").as_deref(), cannot);
        }
        let _rest = f.ask(Op::Rest);
        f.tick(NIGHT);
        f.now += ADMISSION_US;
        say(&mut f, Phase::Idle, None);
        assert_eq!(
            f.manager.cannot_rest("routine").as_deref(),
            Some(NOT_ANSWERED)
        );
        f.resting();
        assert_eq!(f.manager.cannot_rest("routine"), None);
    }

    #[test]
    fn wake_uses_the_admitted_episode_and_cancellation_stops_renewal() {
        let mut f = Fixture::new();
        let mut subscriber = f.manager.bus.subscribe(Request::TOPIC, "test").unwrap();
        let rest = f.ask(Op::Rest);
        f.tick(NIGHT);
        assert!(rest.try_recv().is_err(), "send is not admission");
        let message = subscriber.recv(Duration::from_millis(50)).unwrap().unwrap();
        let request: Request = message.parse().unwrap();
        assert_eq!(request.stamp.sequence, message.header.seq);
        assert_eq!(request.skill_session_id, message.header.boot);
        assert_eq!(request.expression_session_id, f.state.boot());
        f.admitted(Phase::Resting);
        assert_eq!(rest.try_recv().unwrap(), Ok(Woke::Owner));
        let wake = f.ask(Op::Wake);
        f.now += 1;
        f.tick(NIGHT);
        assert_eq!(
            f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request
                .episode,
            request.episode
        );
        f.admitted(Phase::Waking);
        assert!(
            wake.try_recv().is_err(),
            "hum waits until the planned wake ends"
        );
        f.admitted(Phase::Cancelled);
        assert!(wake.try_recv().unwrap().is_err());
        assert!(f.manager.sources["routine"].active.is_none());
        let published = f.manager.publisher.totals().published;
        f.now += 2_000_000;
        f.tick(NIGHT);
        assert_eq!(f.manager.publisher.totals().published, published);
        let wake = f.ask(Op::Wake);
        f.tick(NIGHT);
        assert!(
            wake.try_recv().unwrap().is_err(),
            "no manual or cancelled rest is woken"
        );
    }
    #[test]
    fn only_a_completed_wake_allows_its_following_sound() {
        let mut f = Fixture::new();
        let rest = f.ask(Op::Rest);
        f.tick(NIGHT);
        f.admitted(Phase::Resting);
        assert_eq!(rest.try_recv().unwrap(), Ok(Woke::Owner));
        let wake = f.ask(Op::Wake);
        f.now += 1;
        f.tick(NIGHT);
        f.admitted(Phase::Waking);
        assert!(wake.try_recv().is_err());
        f.admitted(Phase::Completed);
        assert_eq!(wake.try_recv().unwrap(), Ok(Woke::Owner));
        assert!(f.manager.sources["routine"].active.is_none());
    }
    #[test]
    fn a_foreign_completion_neither_confirms_nor_renews_a_pending_wake() {
        let mut f = Fixture::new();
        f.resting();
        let wake = f.ask(Op::Wake);
        f.tick(NIGHT);
        f.admitted(Phase::Waking);
        let original = f.manager.sources["routine"]
            .active
            .as_ref()
            .unwrap()
            .request
            .clone();
        let mut foreign = original.clone();
        foreign.skill_session_id = "another-skills-session".into();
        f.now += RENEW_US;
        f.receipt
            .publish_json(
                f.now,
                &Receipt {
                    stamp: ClockStamp {
                        clock_id: CLOCK.into(),
                        at_us: f.now,
                        sequence: f.receipt.totals().published + 1,
                    },
                    expression_session_id: f.state.boot().into(),
                    enabled: true,
                    request: Some(foreign),
                    phase: Phase::Completed,
                    reason: None,
                },
            )
            .unwrap();
        f.tick(NIGHT);
        assert!(
            wake.try_recv().is_err(),
            "a foreign completion cannot authorize presentation"
        );
        assert_eq!(
            f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request,
            original
        );
        f.admitted(Phase::Waking);
        assert!(
            f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request
                .stamp
                .sequence
                > original.stamp.sequence
        );
        f.admitted(Phase::Completed);
        assert_eq!(wake.try_recv().unwrap(), Ok(Woke::Owner));
        assert!(f.manager.sources["routine"].active.is_none());
    }
    #[test]
    fn completion_cannot_cross_an_owner_window_or_clock_change() {
        for change in 0..3 {
            let mut f = Fixture::new();
            let rest = f.ask(Op::Rest);
            f.tick(NIGHT);
            f.admitted(Phase::Resting);
            assert_eq!(rest.try_recv().unwrap(), Ok(Woke::Owner));
            let wake = f.ask(Op::Wake);
            f.now += 1;
            f.tick(NIGHT);
            f.admitted(Phase::Waking);
            let request = f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request
                .clone();
            f.receipt
                .publish_json(
                    f.now,
                    &Receipt {
                        stamp: ClockStamp {
                            clock_id: CLOCK.into(),
                            at_us: f.now,
                            sequence: f.receipt.totals().published + 1,
                        },
                        expression_session_id: f.state.boot().into(),
                        enabled: true,
                        request: Some(request),
                        phase: Phase::Completed,
                        reason: None,
                    },
                )
                .unwrap();
            if change == 0 {
                f.state = f
                    .manager
                    .bus
                    .publisher_declared(
                        ExpressionState::TOPIC,
                        &ExpressionState::declaration(),
                        "expression",
                    )
                    .unwrap();
            }
            let wall = match change {
                0 => NIGHT,
                1 => 0,
                _ => NIGHT + 11 * 3_600_000,
            };
            f.tick(wall);
            assert!(wake.try_recv().unwrap().is_err(), "change {change}");
            assert!(f.manager.sources["routine"].active.is_none());
        }
    }
    #[test]
    fn a_thread_captured_before_reload_cannot_start_old_work() {
        let mut f = Fixture::new();
        let old = f.client.for_run();
        f.manager.reload(vec!["routine".into()], f.now);
        assert!(!old.current());
        assert!(
            old.request(
                "routine",
                Rest {
                    op: Op::Rest,
                    resettle: None,
                    during: Some(Window::parse("23:00", "10:00").unwrap())
                }
            )
            .is_err()
        );
        assert!(f.manager.asks.try_recv().is_err());
    }
    #[test]
    fn a_queued_completion_cannot_extend_the_original_deadline() {
        let (send, _asks) = mpsc::sync_channel(1);
        let run = Client {
            send,
            generation: Arc::new(Mutex::new(Arc::new(()))),
        }
        .for_run();
        for wait_us in [ADMISSION_US, WAKE_US] {
            let issued_us = 10_000_000;
            let expires_us = issued_us + wait_us;
            let (answer, receive) = mpsc::sync_channel(1);
            answer.try_send(Ok(Woke::Owner)).unwrap();
            // The owner answered in time, but the thread did not receive
            // the queued answer until the original deadline had passed.
            assert!(
                run.received(receive.try_recv().unwrap(), expires_us, expires_us + 1)
                    .is_err()
            );
            let permit = run
                .received(Ok(Woke::Owner), expires_us, expires_us - 1)
                .unwrap();
            assert_eq!(permit.expires_us(), expires_us);
            assert!(permit.check_at(expires_us - 1).is_ok());
            // A pause after receive cannot extend the presentation window.
            assert!(permit.check_at(expires_us).is_err());
        }
    }
    #[test]
    fn a_completion_keeps_its_captured_definition_generation() {
        let (send, _asks) = mpsc::sync_channel(1);
        let client = Client {
            send,
            generation: Arc::new(Mutex::new(Arc::new(()))),
        };
        let run = client.for_run();
        let permit = run.received(Ok(Woke::Owner), 200, 100).unwrap();
        assert!(permit.check_at(100).is_ok());
        *client.generation.lock().unwrap() = Arc::new(());
        // Reload after receive invalidates the retained permit, and a
        // completion received after reload cannot create another permit.
        assert!(permit.check_at(101).is_err());
        assert!(run.received(Ok(Woke::Owner), 200, 101).is_err());
        assert!(client.for_run().received(Ok(Woke::Owner), 200, 101).is_ok());
    }
    #[test]
    fn a_terminal_receipt_before_the_last_renewal_still_ends_the_episode() {
        let mut f = Fixture::new();
        let rest = f.ask(Op::Rest);
        f.tick(NIGHT);
        f.admitted(Phase::Resting);
        assert_eq!(rest.try_recv().unwrap(), Ok(Woke::Owner));
        let request = f.manager.sources["routine"]
            .active
            .as_ref()
            .unwrap()
            .request
            .clone();
        f.now += RENEW_US;
        f.admitted(Phase::Resting); // Tick sends a newer renewal.
        assert!(
            f.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request
                .stamp
                .sequence
                > request.stamp.sequence
        );
        f.receipt
            .publish_json(
                f.now,
                &Receipt {
                    stamp: ClockStamp {
                        clock_id: CLOCK.into(),
                        at_us: f.now,
                        sequence: f.receipt.totals().published + 1,
                    },
                    expression_session_id: f.state.boot().into(),
                    enabled: true,
                    request: Some(request),
                    phase: Phase::Cancelled,
                    reason: None,
                },
            )
            .unwrap();
        f.tick(NIGHT);
        assert!(f.manager.sources["routine"].active.is_none());
    }
    #[test]
    fn changed_expression_does_not_rebind_rest_during_clock_loss() {
        let mut f = Fixture::new();
        f.resting();
        f.state = f
            .manager
            .bus
            .publisher_declared(
                ExpressionState::TOPIC,
                &ExpressionState::declaration(),
                "expression",
            )
            .unwrap();
        f.now += 1;
        f.tick(0);
        assert!(f.manager.sources["routine"].active.is_none());
    }
    #[test]
    fn reload_cancels_pending_admission_and_a_late_ack_cannot_restore_it() {
        let mut f = Fixture::new();
        let rest = f.ask(Op::Rest);
        f.tick(NIGHT);
        f.manager.reload(vec!["routine".into()], f.now + 1);
        assert!(rest.try_recv().unwrap().is_err());
        assert!(f.manager.sources["routine"].active.is_none());
        let late = f.ask(Op::Rest);
        f.now += ADMISSION_US;
        f.tick(NIGHT);
        assert!(late.try_recv().unwrap().is_err());
        assert!(f.manager.sources["routine"].active.is_none());
    }

    fn resettling(f: &mut Fixture) -> mpsc::Receiver<Result<Woke, String>> {
        let (answer, result) = mpsc::sync_channel(1);
        f.client
            .send
            .try_send(Asked {
                skill: "routine".into(),
                rest: Rest {
                    op: Op::Rest,
                    during: Some(Window::parse("23:00", "10:00").unwrap()),
                    resettle: Some(Resettle {
                        most: 3,
                        after_us: 30_000_000,
                        during: Window::parse("23:00", "06:00").unwrap(),
                    }),
                },
                issued_us: f.now,
                expires_us: f.now + ADMISSION_US,
                answer,
                generation: f.client.generation.lock().unwrap().clone(),
            })
            .unwrap();
        result
    }

    fn eyes(f: &Fixture) -> Publisher {
        f.manager
            .bus
            .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
            .unwrap()
    }

    /// The eyes' look now: no one, or one face at the desk.
    fn look(f: &Fixture, eyes: &mut Publisher, someone: bool) {
        let faces = if someone {
            serde_json::json!({ "faces": [{ "id": "f1", "xmin": 0.4, "ymin": 0.3,
                "xmax": 0.6, "ymax": 0.6, "confidence": 0.9 }] })
        } else {
            serde_json::json!({ "faces": [] })
        };
        eyes.publish_json(f.now, &faces).unwrap();
    }

    /// Bedtime's rest, admitted, then ended by Expression while the voice
    /// listens: a night sound or the start of a turn.
    fn woken(f: &mut Fixture, voice: &mut Publisher) {
        voice
            .publish_json(
                f.now,
                &VoiceState {
                    state: Voice::Resting,
                },
            )
            .unwrap();
        let rest = resettling(f);
        f.tick(NIGHT);
        f.admitted(Phase::Resting);
        assert_eq!(rest.try_recv().unwrap(), Ok(Woke::Owner));
        f.now += 100_000;
        voice
            .publish_json(
                f.now,
                &VoiceState {
                    state: Voice::Listening,
                },
            )
            .unwrap();
        f.admitted(Phase::Cancelled);
        assert_eq!(episode(f), None);
        assert!(f.manager.sources["routine"].stirred.is_some());
    }

    fn voice_of(f: &Fixture) -> Publisher {
        f.manager
            .bus
            .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
            .unwrap()
    }

    /// `seconds` of the voice in `state`, one tick a second, the eyes
    /// looking each second.
    fn hold(
        f: &mut Fixture,
        voice: &mut Publisher,
        eyes: &mut Publisher,
        state: Voice,
        someone: bool,
        seconds: u64,
    ) {
        for _ in 0..seconds {
            f.now += 1_000_000;
            voice.publish_json(f.now, &VoiceState { state }).unwrap();
            look(f, eyes, someone);
            f.tick(NIGHT);
        }
    }

    fn episode(f: &Fixture) -> Option<u64> {
        f.manager.sources["routine"]
            .active
            .as_ref()
            .map(|active| active.request.episode)
    }

    /// #1672: a lamp gone to bed that hears a sound stirs and settles again,
    /// as a pet does, at most three times a night, and only in the window.
    #[test]
    fn a_rest_a_sound_ended_settles_again_at_most_three_times_a_night() {
        let mut f = Fixture::new();
        let mut voice = f
            .manager
            .bus
            .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
            .unwrap();
        let mut say = |f: &mut Fixture, state: Voice| {
            voice.publish_json(f.now, &VoiceState { state }).unwrap();
        };
        let mut eyes = eyes(&f);
        say(&mut f, Voice::Resting);
        let rest = resettling(&mut f);
        f.tick(NIGHT);
        f.admitted(Phase::Resting);
        assert_eq!(rest.try_recv().unwrap(), Ok(Woke::Owner));
        assert_eq!(episode(&f), Some(1));
        for night_sound in 1..=4_u64 {
            // A sound: the voice listens, and Expression ends the rest.
            f.now += 100_000;
            say(&mut f, Voice::Listening);
            f.admitted(Phase::Cancelled);
            assert_eq!(episode(&f), None);
            assert_eq!(
                f.manager.sources["routine"].stirred.is_some(),
                night_sound <= 3,
                "sound {night_sound}"
            );
            // Quiet again, nobody in view: not before the voice has rested
            // 30 s.
            for second in 0..=30 {
                f.now += 1_000_000;
                say(&mut f, Voice::Resting);
                look(&f, &mut eyes, false);
                f.tick(NIGHT);
                if second < 30 || night_sound > 3 {
                    assert_eq!(episode(&f), None, "sound {night_sound}, {second} s");
                }
            }
            if night_sound <= 3 {
                // A new episode, no run and no daily claim behind it.
                assert_eq!(episode(&f), Some(1 + night_sound));
                assert_eq!(
                    f.manager.sources["routine"]
                        .active
                        .as_ref()
                        .unwrap()
                        .resettled,
                    night_sound as u8
                );
                f.admitted(Phase::Resting);
            }
        }
        assert!(f.manager.sources["routine"].stirred.is_none());
    }

    #[test]
    fn leaving_the_night_window_ends_the_resettling() {
        let mut f = Fixture::new();
        let mut voice = f
            .manager
            .bus
            .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
            .unwrap();
        voice
            .publish_json(
                f.now,
                &VoiceState {
                    state: Voice::Resting,
                },
            )
            .unwrap();
        let rest = resettling(&mut f);
        f.tick(NIGHT);
        f.admitted(Phase::Resting);
        assert_eq!(rest.try_recv().unwrap(), Ok(Woke::Owner));
        f.now += 100_000;
        voice
            .publish_json(
                f.now,
                &VoiceState {
                    state: Voice::Listening,
                },
            )
            .unwrap();
        f.admitted(Phase::Cancelled);
        assert!(f.manager.sources["routine"].stirred.is_some());
        // 06:00: the re-settle window is over before the voice rests again.
        let morning = NIGHT + 7 * 3_600_000;
        for _ in 0..=31 {
            f.now += 1_000_000;
            voice
                .publish_json(
                    f.now,
                    &VoiceState {
                        state: Voice::Resting,
                    },
                )
                .unwrap();
            f.tick(morning);
        }
        assert!(f.manager.sources["routine"].stirred.is_none());
        assert_eq!(episode(&f), None);
    }

    #[test]
    fn a_rest_ended_for_anything_but_a_sound_does_not_settle_again() {
        let mut f = Fixture::new();
        let mut voice = f
            .manager
            .bus
            .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
            .unwrap();
        voice
            .publish_json(
                f.now,
                &VoiceState {
                    state: Voice::Resting,
                },
            )
            .unwrap();
        let rest = resettling(&mut f);
        f.tick(NIGHT);
        f.admitted(Phase::Resting);
        assert_eq!(rest.try_recv().unwrap(), Ok(Woke::Owner));
        // A hand or the privacy switch: the voice still rests.
        f.now += 100_000;
        voice
            .publish_json(
                f.now,
                &VoiceState {
                    state: Voice::Resting,
                },
            )
            .unwrap();
        f.admitted(Phase::Cancelled);
        assert!(f.manager.sources["routine"].stirred.is_none());
    }

    /// The reviewer-lead's P2 on #2512: a name call arrives as the voice
    /// listening, then becomes a turn. Someone is up: the night's rest ends,
    /// and 30 s of quiet after the answer does not fold the lamp beside them.
    #[test]
    fn a_turn_ends_the_nights_rest_and_quiet_after_it_does_not_settle_again() {
        let mut f = Fixture::new();
        let mut voice = voice_of(&f);
        let mut eyes = eyes(&f);
        woken(&mut f, &mut voice);
        hold(&mut f, &mut voice, &mut eyes, Voice::Thinking, false, 2);
        assert!(f.manager.sources["routine"].stirred.is_none());
        hold(&mut f, &mut voice, &mut eyes, Voice::Speaking, false, 3);
        hold(&mut f, &mut voice, &mut eyes, Voice::Resting, false, 40);
        assert_eq!(f.manager.sources["routine"].last_episode, 1);
        assert!(f.manager.sources["routine"].stirred.is_none());
    }

    /// A rest Expression ended mid-turn is not a night sound either.
    #[test]
    fn a_rest_ended_in_a_turn_does_not_settle_again() {
        let mut f = Fixture::new();
        let mut voice = voice_of(&f);
        voice
            .publish_json(
                f.now,
                &VoiceState {
                    state: Voice::Resting,
                },
            )
            .unwrap();
        let rest = resettling(&mut f);
        f.tick(NIGHT);
        f.admitted(Phase::Resting);
        assert_eq!(rest.try_recv().unwrap(), Ok(Woke::Owner));
        f.now += 100_000;
        voice
            .publish_json(
                f.now,
                &VoiceState {
                    state: Voice::Speaking,
                },
            )
            .unwrap();
        f.admitted(Phase::Cancelled);
        assert!(f.manager.sources["routine"].stirred.is_none());
    }

    #[test]
    fn a_sound_with_no_turn_and_no_face_settles_again_once() {
        let mut f = Fixture::new();
        let mut voice = voice_of(&f);
        let mut eyes = eyes(&f);
        woken(&mut f, &mut voice);
        hold(&mut f, &mut voice, &mut eyes, Voice::Resting, false, 29);
        assert_eq!(episode(&f), None);
        hold(&mut f, &mut voice, &mut eyes, Voice::Resting, false, 2);
        assert_eq!(episode(&f), Some(2));
        assert!(f.manager.sources["routine"].stirred.is_none());
        // Once: more quiet starts no further episode.
        hold(&mut f, &mut voice, &mut eyes, Voice::Resting, false, 40);
        assert_eq!(f.manager.sources["routine"].last_episode, 2);
    }

    /// Someone sat back down after the sound: the lamp stays with them.
    #[test]
    fn a_sound_with_a_face_in_view_does_not_settle_again() {
        let mut f = Fixture::new();
        let mut voice = voice_of(&f);
        let mut eyes = eyes(&f);
        woken(&mut f, &mut voice);
        hold(&mut f, &mut voice, &mut eyes, Voice::Resting, true, 40);
        assert_eq!(f.manager.sources["routine"].last_episode, 1);
        assert!(f.manager.sources["routine"].stirred.is_none());
        // Gone again later: the night's rest has ended, nothing re-settles.
        hold(&mut f, &mut voice, &mut eyes, Voice::Resting, false, 40);
        assert_eq!(f.manager.sources["routine"].last_episode, 1);
    }

    /// Missing or stale faces are not an empty desk: it waits, it does not
    /// settle on no news.
    #[test]
    fn stale_or_missing_faces_do_not_settle_again() {
        let mut f = Fixture::new();
        let mut voice = voice_of(&f);
        let mut eyes = eyes(&f);
        woken(&mut f, &mut voice);
        for _ in 0..40 {
            f.now += 1_000_000;
            voice
                .publish_json(
                    f.now,
                    &VoiceState {
                        state: Voice::Resting,
                    },
                )
                .unwrap();
            f.tick(NIGHT);
        }
        assert_eq!(episode(&f), None);
        look(&f, &mut eyes, false);
        f.now += 3_000_000;
        voice
            .publish_json(
                f.now,
                &VoiceState {
                    state: Voice::Resting,
                },
            )
            .unwrap();
        f.tick(NIGHT);
        assert_eq!(episode(&f), None);
        assert!(f.manager.sources["routine"].stirred.is_some());
        // A fresh empty look: now it settles.
        hold(&mut f, &mut voice, &mut eyes, Voice::Resting, false, 1);
        assert_eq!(episode(&f), Some(2));
    }

    /// 05:30 and 07:10 of the morning after `NIGHT` (23:00).
    const STIR: u64 = NIGHT + 6 * 3_600_000 + 30 * 60_000;
    const MORNING: u64 = NIGHT + 8 * 3_600_000 + 10 * 60_000;

    /// The rest taken at 23:00, then ended at 05:30: its lease lapses, as a
    /// rest a sound ended past its resettle window does.
    fn rested_then_stirred(f: &mut Fixture) {
        f.resting();
        f.now += 6 * 3_600_000_000 + 30 * 60_000_000;
        f.tick(STIR);
        assert!(f.manager.sources["routine"].active.is_none());
    }

    fn wake_at(f: &mut Fixture, wall: u64) -> Result<Woke, String> {
        let wake = f.ask(Op::Wake);
        f.tick(wall);
        wake.try_recv().unwrap()
    }

    #[test]
    fn a_rest_a_stir_ended_at_0530_is_greeted_at_0710_with_a_stretch() {
        let mut f = Fixture::new();
        assert_eq!(f.manager.rested_day(), None);
        rested_then_stirred(&mut f);
        // The night it rested, for a `yields_to` (#1672): the day it began.
        let rested = f.manager.sources["routine"].rested_day;
        assert!(rested.is_some());
        assert_eq!(f.manager.rested_day(), rested);
        let mut requests = f.manager.bus.subscribe(Request::TOPIC, "morning").unwrap();
        f.now += 3_600_000_000 + 40 * 60_000_000;
        assert_eq!(wake_at(&mut f, MORNING), Ok(Woke::Already));
        // Nothing is asked of Expression: the lamp is already up, and the
        // run makes its own stretch.
        assert!(requests.recv(Duration::ZERO).unwrap().is_none());
        assert!(f.manager.sources["routine"].active.is_none());
        // Once a morning: a second wake has no night left to answer for.
        f.now += 60_000_000;
        let again = wake_at(&mut f, MORNING + 60_000);
        assert!(again.unwrap_err().contains("owns no rest"));
    }

    /// #2757: the night rested is kept in `--state-dir`'s `rested`, so a
    /// restart before the morning still has its wake answered, one after it
    /// still tells a `yields_to` the lamp rested, and the next night's rest
    /// clears it.
    #[test]
    fn the_night_rested_is_kept_across_a_restart_until_the_next_night() {
        let mut f = Fixture::new();
        let dir = f._root.path().to_path_buf();
        assert_eq!(f.manager.keep_rested_in(&dir), Ok(vec![]));
        rested_then_stirred(&mut f);
        let rested = f.manager.rested_day().expect("rested");
        let kept = || std::fs::read_to_string(dir.join(RESTED_FILE)).unwrap();
        assert_eq!(kept(), format!("routine {rested}\n"));
        // Restarted before the morning: its wake is the lamp's, as before.
        f.restart(&dir);
        assert_eq!(f.manager.rested_day(), Some(rested));
        f.now += 3_600_000_000 + 40 * 60_000_000;
        assert_eq!(wake_at(&mut f, MORNING), Ok(Woke::Already));
        // After the morning's wake, and after another restart: still the
        // night it rested, for welcome-back's `yields_to`.
        assert_eq!(f.manager.rested_day(), Some(rested));
        f.restart(&dir);
        assert_eq!(f.manager.rested_day(), Some(rested));
        assert_eq!(kept(), format!("routine {rested}\n"));
        // The next night's rest begins: last night's is over.
        f.now += 60_000_000;
        let _rest = f.ask(Op::Rest);
        f.tick(NIGHT + 86_400_000);
        assert_eq!(f.manager.rested_day(), None);
        assert_eq!(kept(), "");
        // A line that is not a skill and a day is passed over.
        std::fs::write(dir.join(RESTED_FILE), "Routine 1\nroutine x\nroutine 5 6\n").unwrap();
        f.restart(&dir);
        assert_eq!(f.manager.rested_day(), None);
    }

    #[test]
    fn no_rest_tonight_no_greeting() {
        // Never rested.
        let mut f = Fixture::new();
        let never = wake_at(&mut f, MORNING);
        assert!(never.unwrap_err().contains("owns no rest"));
        // Rested the night before last: not this morning's night.
        let mut f = Fixture::new();
        rested_then_stirred(&mut f);
        f.now += 25 * 3_600_000_000;
        let late = wake_at(&mut f, MORNING + 86_400_000);
        assert!(late.unwrap_err().contains("owns no rest"));
    }

    #[test]
    fn a_lamp_asleep_by_hand_or_private_is_not_stretched_the_morning_after() {
        let closed = serde_json::json!({ "physical_muted": true });
        for kept in ["by hand", "muted", "switch"] {
            let mut f = Fixture::new();
            rested_then_stirred(&mut f);
            let mut mute = f
                .manager
                .bus
                .publisher_declared(VoiceMute::TOPIC, &VoiceMute::declaration(), "voice")
                .unwrap();
            let mut privacy = f
                .manager
                .bus
                .publisher_declared(
                    MicrophonePrivacy::TOPIC,
                    &MicrophonePrivacy::declaration(),
                    "voice",
                )
                .unwrap();
            f.now += 3_600_000_000 + 40 * 60_000_000;
            match kept {
                "by hand" => mute.publish_json(
                    f.now,
                    &VoiceMute {
                        muted: true,
                        reason: Some(lamp_msgs::MuteReason::Sleep),
                        muted_by: None,
                    },
                ),
                "muted" => mute.publish_json(
                    f.now,
                    &VoiceMute {
                        muted: true,
                        reason: None,
                        muted_by: None,
                    },
                ),
                _ => privacy.publish_json(f.now, &closed),
            }
            .unwrap();
            let why = wake_at(&mut f, MORNING).unwrap_err();
            assert!(
                why.contains("sleep by hand") || why.contains("privacy"),
                "{kept}: {why}"
            );
            assert!(
                f.manager.sources["routine"].active.is_none(),
                "{kept}: nothing is sent"
            );
            // Woken by hand later that morning, it may still greet.
            if kept != "switch" {
                mute.publish_json(
                    f.now,
                    &VoiceMute {
                        muted: false,
                        reason: None,
                        muted_by: None,
                    },
                )
                .unwrap();
                f.now += 60_000_000;
                assert_eq!(wake_at(&mut f, MORNING + 60_000), Ok(Woke::Already));
            }
        }
    }
}
