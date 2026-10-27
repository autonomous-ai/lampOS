//! The second voice route beside the Live one (lampOS#754).
//!
//! With `LAMP_VOICE_SECOND_CONFIG` set, the service starts the `second`
//! worker as a standby next to Gemini's. Both speak the same provider wire,
//! so a switch is a swap of the worker, its link register and its output
//! credit with the active ones: every path written for `rig.provider` and
//! `cloud` then runs unchanged against the other route. The standby's
//! control goes to its own register; its output is drained, credited and
//! dropped, which is how a late answer of the route switched away from is
//! never heard.
//!
//! Switching:
//! - At rest, to the second route while Gemini's link is not Ready (and has
//!   not ended for good: Gemini's end still ends the service), and back to
//!   Gemini, silently, on the first idle tick its link is Ready again.
//! - At startup, before Gemini's first Ready, and at rest while its link
//!   reconnects (#960), the second route only stands in: a request admitted
//!   on it is held, for up to [`STARTUP_HOLD_US`], and goes to Gemini if its
//!   link is Ready by then; only otherwise (or when Gemini's link has ended)
//!   is the second route given the line. A line already under way when the
//!   link drops is not held: it is replayed to the second route (below).
//! - Mid-turn, when Gemini fails the turn before any of its answer was
//!   heard, or its link changes during the line: the line is replayed to
//!   the second route from [`Line`], a copy of what was sent for it.
//!
//! [`Line`] is held in memory only, never on disk or the bus, bounded at
//! 30 s of 16 kHz mono, and cleared when the line is answered or abandoned
//! and at once when an authority without its owner or with the microphone
//! closed is published (mute, privacy, cancellation). Without a configured
//! route `Rig::second` is `None` and every function here is the old call.
use super::*;
pub(super) use crate::second_provider::worker::RouteFlags;
#[cfg(test)]
use crate::second_provider::worker::{ChatRoute, SearchRoute, SpeechRoute};
use std::ffi::OsString;

/// 30 s of 16 kHz mono microphone audio.
const MAX_LINE_SAMPLES: usize = 30 * 16_000;
/// Standby control and output packets taken per coordinator tick.
const STANDBY_SLICE: usize = 16;
/// The audio a line keeps from before its request was admitted: the
/// activity detector's pre-roll, 300 ms.
const LEAD_IN_US: u64 =
    crate::activity::PRE_ROLL_BLOCKS as u64 * lamp_audio::CAPTURE_SAMPLES as u64 * 1_000_000
        / lamp_audio::CAPTURE_RATE as u64;
/// Room for the detector's own delay before admission and for the replay's
/// first block to reach Gemini's link.
const HOLD_MARGIN_US: u64 = 300_000;
/// How long a request that begins before Gemini's first Ready of this
/// process, or while its link reconnects at rest (#960), waits for it
/// before the second route is given the line (lampOS#779: on lamp-4ace
/// Gemini's first Ready came 1.9 s after the second route's). It is what Gemini's audio age bound leaves after the
/// lead-in and a margin, 1.9 s: a line held longer would replay a block
/// Gemini's worker refuses as stale, which ends the worker (lampOS#892).
pub(super) const STARTUP_HOLD_US: u64 =
    lamp_gemini::MAX_INPUT_AGE.as_micros() as u64 - LEAD_IN_US - HOLD_MARGIN_US;
/// Commands of a replay sent to Gemini per coordinator tick (2 ms): about
/// ten times the pace of speech. Its link takes input into a queue of
/// [`lamp_gemini::INPUT_QUEUE_CAPACITY`] and gives the connection up when
/// that is full; a held line of 1.9 s sent at the ordinary eight per tick
/// overran it 37 ms after Gemini's first Ready on lamp-52e6 (lampOS#892).
/// A write that stalls still fills the queue at this pace; the provider
/// worker then waits for room (`ROOM_WAIT_US`) rather than overrun it.
/// The second route's replay is not paced.
const LIVE_REPLAY_PER_TICK: usize = 2;

/// What was sent to the active Live route for the line in hand.
#[derive(Default)]
pub(super) struct Line {
    request: u64,
    commands: Vec<ProviderInput>,
    samples: usize,
    /// Every command of `request` since its start is in `commands`.
    complete: bool,
}

impl Line {
    fn keep(&mut self, command: &ProviderInput) {
        if matches!(
            command,
            ProviderInput::FaceQuestion { .. } | ProviderInput::FaceConsentDelivery { .. }
        ) {
            return;
        }
        let request = request_of(command);
        if matches!(
            command,
            ProviderInput::Start { .. }
                | ProviderInput::Written { .. }
                | ProviderInput::Reminder { .. }
        ) {
            self.clear();
            self.request = request;
            self.complete = true;
        } else if request != self.request || !self.complete {
            return;
        }
        if let ProviderInput::Audio { samples, .. } = command {
            self.samples += samples.len();
            if self.samples > MAX_LINE_SAMPLES {
                // Too long to replay: kept no more, and not replayed.
                self.clear();
                return;
            }
        }
        self.commands.push(command.clone());
    }

    fn clear(&mut self) {
        *self = Self::default();
    }

    fn covers(&self, request: u64) -> bool {
        self.complete && self.request == request
    }
}

fn request_of(command: &ProviderInput) -> u64 {
    match command {
        ProviderInput::FaceQuestion { request, .. }
        | ProviderInput::FaceConsentDelivery { request, .. }
        | ProviderInput::Start { request, .. }
        | ProviderInput::Audio { request, .. }
        | ProviderInput::End { request, .. }
        | ProviderInput::Called { request, .. }
        | ProviderInput::Paused { request, .. }
        | ProviderInput::CountOnly { request, .. }
        | ProviderInput::StepCall { request, .. }
        | ProviderInput::Onboarding { request, .. }
        | ProviderInput::Written { request, .. }
        | ProviderInput::Reminder { request, .. } => *request,
    }
}

pub(super) struct Route {
    /// The standby: Gemini's worker while the second route speaks, the
    /// second route's otherwise.
    worker: Worker,
    cloud: CloudAvailability,
    flow: OutputReceiver,
    /// `rig.provider` is the second route's worker.
    second_active: bool,
    line: Line,
    /// A line being replayed to the second route, sent before anything
    /// queued after it; or, while a request is held, all of it so far.
    replay: VecDeque<ProviderInput>,
    /// The second route is active only to stand in while Gemini's link is
    /// not Ready yet: before its first Ready, or while it reconnects
    /// (#960). A request admitted on it is held, not given to it.
    stand_in: bool,
    /// The request held while the second route stands in, and since when.
    hold: Option<(u64, u64)>,
}

/// The configuration's path when the variable names one.
pub(super) fn configured(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|path| !path.is_empty()).map(PathBuf::from)
}

/// Which route is the lamp's voice, in the service's environment. Unset,
/// empty or `gemini`: Gemini Live, with the second route standing by when
/// it is configured, as before. `second`: the second route alone, from the
/// first turn (the owner, 19 Oct: Gemini Live costs more; an A/B of the
/// two on lamp-4ace). Anything else is refused at start.
pub(super) const ROUTE_ENV: &str = "LAMP_VOICE_ROUTE";

/// The voice routes a run starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Routes {
    /// Gemini's worker is `rig.provider`; the second route, when it is
    /// configured, stands by in `Rig::second`.
    Live { standby: Option<PathBuf> },
    /// `LAMP_VOICE_ROUTE=second`, with the second route's configuration:
    /// its worker is `rig.provider` from the start, as after a take-over,
    /// and nothing stands by. No Gemini worker is started, so no Gemini
    /// session is opened, and every path written for `rig.provider` runs
    /// against the second route, `Rig::second` being `None`.
    SecondOnly(PathBuf),
}

/// The worker a run starts in `rig.provider`, and the standby's arguments.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Workers {
    /// `provider` (Gemini Live) or `second`.
    pub(super) role: &'static str,
    pub(super) arguments: Vec<String>,
    pub(super) standby: Option<Vec<String>>,
}

impl Routes {
    /// Whether the second route stands by beside Gemini's.
    pub(super) fn standing_by(&self) -> bool {
        matches!(self, Self::Live { standby: Some(_) })
    }

    /// Whether a second route's worker runs at all, standing by or as the
    /// only voice: the one that answers texts (`crate::messages`).
    pub(super) fn has_second(&self) -> bool {
        matches!(self, Self::Live { standby: Some(_) } | Self::SecondOnly(_))
    }

    /// The workers, from the Live worker's arguments (`provider_arguments`)
    /// and the second route's switches; `only_voice` is set here, by route.
    pub(super) fn workers(&self, provider: Vec<String>, flags: RouteFlags) -> io::Result<Workers> {
        Ok(match self {
            Self::Live { standby } => Workers {
                role: "provider",
                standby: standby
                    .as_deref()
                    .map(|config| {
                        let flags = RouteFlags {
                            only_voice: false,
                            ..flags
                        };
                        arguments(&provider, config, &flags)
                    })
                    .transpose()?,
                arguments: provider,
            },
            Self::SecondOnly(config) => Workers {
                role: "second",
                arguments: arguments(
                    &provider,
                    config,
                    &RouteFlags {
                        only_voice: true,
                        ..flags
                    },
                )?,
                standby: None,
            },
        })
    }
}

/// The routes [`ROUTE_ENV`] asks for, with the second route's
/// configuration (`LAMP_VOICE_SECOND_CONFIG`), or why the voice does not
/// start. Both are the service's (`serving`: continuous, with Gemini's
/// configuration): a bounded run never has a second route, and refuses to
/// be asked for one alone rather than run Gemini in its place.
pub(super) fn routes(
    route: Option<OsString>,
    second_config: Option<OsString>,
    serving: bool,
) -> std::result::Result<Routes, String> {
    let second_config = configured(second_config).filter(|_| serving);
    let second_config_env = crate::second_config::SECOND_CONFIG_ENV;
    match route.as_deref().map(std::ffi::OsStr::to_str) {
        None | Some(Some("" | "gemini")) => Ok(Routes::Live {
            standby: second_config,
        }),
        Some(Some("second")) if !serving => Err(format!(
            "{ROUTE_ENV}=second is for the service (`lamp-voice serve`) only: this run does not start"
        )),
        Some(Some("second")) => second_config.map(Routes::SecondOnly).ok_or_else(|| {
            format!(
                "{ROUTE_ENV}=second needs {second_config_env}, the second route's configuration: the voice does not start, and does not fall back to Gemini Live"
            )
        }),
        Some(_) => Err(format!(
            "{ROUTE_ENV} is gemini or second, not {:?}: the voice does not start",
            route.as_deref().unwrap_or_default()
        )),
    }
}

/// [`routes`], from this process's environment.
pub(super) fn routes_from_environment(serving: bool) -> io::Result<Routes> {
    routes(
        var_os(ROUTE_ENV),
        var_os(crate::second_config::SECOND_CONFIG_ENV),
        serving,
    )
    .map_err(|why| io::Error::new(io::ErrorKind::InvalidInput, why))
}

/// The second route's own switches (`LAMP_SECOND_*`), from this process's
/// environment; a wrong value refused.
pub(super) fn route_flags_from_environment() -> io::Result<RouteFlags> {
    RouteFlags::from_values(var_os).map_err(|why| io::Error::new(io::ErrorKind::InvalidInput, why))
}

#[cfg(test)]
thread_local! {
    /// The routes' variables as a test sets them, in place of the
    /// process's own; `None`: the process's.
    pub(super) static TEST_ENVIRONMENT: std::cell::RefCell<Option<Vec<(&'static str, OsString)>>> =
        const { std::cell::RefCell::new(None) };
}

/// One of the routes' variables.
fn var_os(name: &str) -> Option<OsString> {
    #[cfg(test)]
    if let Some(set) = TEST_ENVIRONMENT.with(|set| set.borrow().clone()) {
        return set
            .into_iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value);
    }
    std::env::var_os(name)
}

impl Routes {
    /// `run_start` says which route answers when it is the second alone
    /// (`"voice_route":"second"`), so a session folder of the A/B tells the
    /// two apart (the reviewer on #1686); otherwise it is as before.
    pub(super) fn mark(&self, run_start: &mut Value) {
        if let Self::SecondOnly(_) = self {
            run_start["voice_route"] = json!("second");
        }
    }
}

/// What the journal says at the service's start, for the second route alone.
pub(super) const SECOND_ONLY_LINE: &str = "lamp-voice: the voice route is second only (LAMP_VOICE_ROUTE=second): speech to text, a text model and speech answer every turn; Gemini Live is not used";

/// The `worker second` arguments: the Live worker's, its configuration
/// first, then the second route's, then the route's own flags (`--wait-cue`
/// with `LAMP_SECOND_WAIT_CUE=on`, `--down-line` with
/// `LAMP_SECOND_DOWN_LINE=on`, `--only-voice` with `LAMP_VOICE_ROUTE=second`,
/// `--gemini-chat` and `--gemini-speech` with `LAMP_SECOND_CHAT=gemini` and
/// `LAMP_SECOND_SPEECH=gemini`, `RouteFlags`), then its flags but
/// `--keep-link` and `--wait-for-room`, which are for Gemini's link.
pub(super) fn arguments(
    provider: &[String],
    second_config: &Path,
    route: &RouteFlags,
) -> io::Result<Vec<String>> {
    let (config, flags) = provider
        .split_first()
        .ok_or_else(|| io::Error::other("provider arguments lack a configuration"))?;
    let second_config = second_config
        .to_str()
        .ok_or_else(|| io::Error::other("second route configuration path must be UTF-8"))?;
    let mut arguments = vec![config.clone(), second_config.to_owned()];
    arguments.extend(route.flags().into_iter().map(str::to_owned));
    arguments.extend(
        flags
            .iter()
            .filter(|flag| {
                *flag != crate::provider_worker::KEEP_LINK_FLAG
                    && *flag != crate::provider_worker::WAIT_FOR_ROOM_FLAG
            })
            .cloned(),
    );
    Ok(arguments)
}

impl Route {
    pub(super) fn new(worker: Worker) -> Self {
        Self {
            worker,
            cloud: CloudAvailability::default(),
            flow: OutputReceiver::default(),
            second_active: false,
            line: Line::default(),
            replay: VecDeque::new(),
            stand_in: false,
            hold: None,
        }
    }

    /// A standby already past its startup, for the coordinator's tests.
    #[cfg(test)]
    pub(super) fn standing_by(worker: Worker, link: ProviderLink) -> Self {
        let mut route = Self::new(worker);
        route
            .cloud
            .observe(link, monotonic_us())
            .expect("valid standby link");
        route
    }

    /// Start the standby when the Live service is configured for one.
    pub(super) fn spawn(
        executable: &Path,
        directory: &Path,
        boot: BootId,
        arguments: Option<&[String]>,
        on_wait: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Option<Self>> {
        let Some(arguments) = arguments else {
            return Ok(None);
        };
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        Worker::spawn_with_tick(executable, directory, "second", boot, &arguments, on_wait)
            .map(|worker| Some(Self::new(worker)))
    }

    pub(super) fn worker_mut(&mut self) -> &mut Worker {
        &mut self.worker
    }

    /// During the remaining startup: keep the standby's authority fresh and
    /// take its readiness before its packets age.
    pub(super) fn startup_tick(
        &mut self,
        snapshot: Option<Snapshot>,
        startup: &mut StartupEvents,
    ) -> io::Result<()> {
        if let Some(snapshot) = snapshot {
            self.publish(snapshot)?;
        }
        for _ in 0..STANDBY_SLICE {
            match self.worker.channels.control.receive::<WorkerEvent>()? {
                Some(WorkerEvent::ProviderLink { link }) => {
                    let at = monotonic_us();
                    self.cloud.observe(link, at)?;
                    let mut event = link_trace(link, at);
                    event["route"] = json!("second");
                    startup.push(event)?;
                }
                Some(WorkerEvent::Fault { code }) => return Err(io::Error::other(code)),
                Some(_) => {
                    return Err(io::Error::other(
                        "unexpected second route event before activation",
                    ));
                }
                None => break,
            }
        }
        if self
            .worker
            .channels
            .data
            .receive::<ProviderOutput>()?
            .is_some()
        {
            return Err(io::Error::other("second route output before input"));
        }
        self.flow.replenish(0, &mut self.worker.channels.control)
    }

    /// Authority to the standby. A line is the current owner's alone: it
    /// goes the moment an authority without that owner, or with the
    /// microphone closed, is published.
    pub(super) fn publish(&mut self, snapshot: Snapshot) -> io::Result<()> {
        if snapshot.microphone_permission() != Permission::Allowed
            || snapshot.owner().map(|owner| owner.turn()) != Some(self.line.request)
        {
            self.line.clear();
        }
        self.worker
            .channels
            .control
            .send(Control::Authority { snapshot })
    }

    /// Whether the second route is the one answering now.
    pub(super) fn second_active(&self) -> bool {
        self.second_active
    }

    /// The input in hand can be taken over: by the second route from
    /// Gemini, before any answer was heard, with every sent command kept.
    fn can_take(&self, current: &Reply) -> bool {
        !self.second_active
            && !current.cloud.same_session_only
            && !current.requested_audio_bound()
            && self.cloud.available()
            && !current.had_audio
            && current.pcm.is_empty()
            && current.reminder.is_none()
            && current.cloud.outcome.is_none()
            && (!current.cloud.start_submitted || self.line.covers(current.owner.turn()))
    }

    /// A request held while the second route stands in: nothing of it is
    /// sent until Gemini is Ready or the hold is over.
    fn holds(&mut self, reply: Option<&Reply>) -> bool {
        if self.stand_in
            && self.hold.is_none()
            && let Some(current) = reply.filter(|current| {
                !current.cloud.start_submitted
                    && !current.cloud.same_session_only
                    && !current.requested_audio_bound()
                    && current.cloud.outcome.is_none()
                    && current.reminder.is_none()
            })
        {
            self.hold = Some((current.owner.turn(), monotonic_us()));
        }
        self.hold.is_some_and(|(turn, _)| {
            reply.is_some_and(|current| {
                current.owner.turn() == turn
                    && !current.cloud.same_session_only
                    && !current.requested_audio_bound()
            })
        })
    }

    /// A line goes once it is answered or abandoned, and a replay and a
    /// hold with the request they were for (so a turn admitted in its
    /// place in the same tick is held afresh).
    fn upkeep(&mut self, reply: Option<&Reply>) {
        // A continuation belongs to the session that made its promise. It
        // has no replay copy, including one retained before it was pinned.
        if reply.is_some_and(|current| {
            current.cloud.same_session_only || current.requested_audio_bound()
        }) {
            self.line.clear();
            self.replay.clear();
            self.hold = None;
            return;
        }
        let turn = reply.map(|current| current.owner.turn());
        if self.hold.is_some_and(|(held, _)| {
            reply.is_none_or(|current| {
                current.owner.turn() != held
                    || current.cloud.outcome.is_some()
                    || current.cloud.input_abandoned
            })
        }) {
            self.hold = None;
        }
        if reply.is_none_or(|current| {
            current.had_audio || current.cloud.outcome.is_some() || current.cloud.input_abandoned
        }) || turn != Some(self.line.request)
        {
            self.line.clear();
        }
        if self
            .replay
            .front()
            .is_some_and(|command| Some(request_of(command)) != turn)
        {
            self.replay.clear();
        }
    }

    /// The standby's priority link reports and output: kept and dropped.
    fn drain(&mut self, trace: &mut SessionState) -> Result<()> {
        let mut budget = STANDBY_SLICE;
        service_provider_control(
            &mut self.worker.channels.control,
            &mut self.cloud,
            &mut budget,
            trace,
        )?;
        for _ in 0..STANDBY_SLICE {
            let Some(event) = self.worker.channels.data.receive::<ProviderOutput>()? else {
                break;
            };
            self.flow.received()?;
            if let ProviderOutput::Ended {
                reason,
                fault,
                attempts,
            } = event
            {
                self.cloud.end_output()?;
                record(
                    trace,
                    json!({"kind":"provider_output_ended","at_us":monotonic_us(),
                    "reason":reason,"fault":fault,"attempts":attempts,"standby":true}),
                )?;
            }
        }
        self.flow
            .replenish(0, &mut self.worker.channels.control)
            .map_err(Into::into)
    }
}

/// The second route's worker has exited: the route is dropped and the
/// voice goes on with Gemini alone, instead of ending as for a mandatory
/// worker (#960: on lamp-4ace a dead second worker ended the voice, and the
/// restart came back muted). When it was the active route, Gemini's worker
/// is put back first. True when it held the turn in hand, which the caller
/// ends.
///
/// ponytail: checked once a tick, before anything else of the tick touches
/// a worker; a worker that dies inside the same tick still fails it there.
pub(super) fn dropped_if_gone(
    rig: &mut Rig,
    cloud: &mut CloudAvailability,
    flow: &mut OutputReceiver,
    trace: &mut SessionState,
) -> Result<bool> {
    let Rig {
        provider, second, ..
    } = rig;
    let Some(route) = second.as_mut() else {
        return Ok(false);
    };
    let worker = if route.second_active {
        &mut *provider
    } else {
        &mut route.worker
    };
    let Some(status) = worker.try_exit()? else {
        return Ok(false);
    };
    let active = route.second_active;
    if active {
        swap(provider, cloud, flow, route);
    }
    eprintln!("lamp-voice: the second voice route's worker exited ({status}); going on without it");
    record(
        trace,
        json!({"kind":"voice_route_dropped","at_us":monotonic_us(),"route":"second",
        "status":status.to_string(),"active":active}),
    )?;
    *second = None;
    // The worker's `voice-keys` part went with it, on purpose: lamp-health
    // forgets it rather than count it quiet (#2415's P3).
    crate::second_provider::worker::own_key_dropped_from_environment();
    Ok(active)
}

fn swap(
    provider: &mut Worker,
    cloud: &mut CloudAvailability,
    flow: &mut OutputReceiver,
    route: &mut Route,
) {
    std::mem::swap(provider, &mut route.worker);
    std::mem::swap(cloud, &mut route.cloud);
    std::mem::swap(flow, &mut route.flow);
    route.second_active = !route.second_active;
}

/// Swap to the second route and give it the line in hand: its start (with
/// the second route's session), everything sent and everything queued.
fn take_over(
    provider: &mut Worker,
    cloud: &mut CloudAvailability,
    flow: &mut OutputReceiver,
    route: &mut Route,
    current: &mut Reply,
    queue: &mut VecDeque<ProviderInput>,
) -> io::Result<usize> {
    if current.cloud.same_session_only || current.requested_audio_bound() {
        return Err(io::Error::other(
            "a session-bound request cannot change voice route",
        ));
    }
    swap(provider, cloud, flow, route);
    let turn = current.owner.turn();
    let sent = if route.line.covers(turn) {
        std::mem::take(&mut route.line.commands)
    } else {
        Vec::new()
    };
    route.line.clear();
    // What was sent, then what of a replay was not yet sent, then the queue.
    let mut replay: VecDeque<ProviderInput> = sent.into();
    replay.append(&mut route.replay);
    replay.append(queue);
    route.replay = replay;
    rebind(cloud, current, &mut route.replay)?;
    Ok(route.replay.len())
}

/// The turn in hand and its commands to the active route's session.
pub(super) fn rebind(
    cloud: &CloudAvailability,
    current: &mut Reply,
    commands: &mut VecDeque<ProviderInput>,
) -> io::Result<()> {
    if current.cloud.same_session_only || current.requested_audio_bound() {
        return Err(io::Error::other(
            "a session-bound request cannot be rebound",
        ));
    }
    let session = cloud
        .ready_session()
        .ok_or_else(|| io::Error::other("a route lost its link before a replay"))?;
    for command in commands {
        if let ProviderInput::Start { session: s, .. }
        | ProviderInput::Written { session: s, .. }
        | ProviderInput::FaceQuestion { session: s, .. }
        | ProviderInput::Reminder { session: s, .. } = command
        {
            *s = session;
        }
    }
    current.cloud = CloudRequest::new(session)?;
    Ok(())
}

/// A held request: to Gemini once its link is Ready; to the second route
/// once the hold is over or Gemini's link has ended; let go with its turn.
fn fixed_face_question(
    queue: &VecDeque<ProviderInput>,
    replay: &VecDeque<ProviderInput>,
    turn: u64,
) -> bool {
    queue.iter().chain(replay.iter()).any(|command| {
        matches!(command, ProviderInput::FaceQuestion { request, .. } if *request == turn)
    })
}

fn resolve_hold(
    provider: &mut Worker,
    cloud: &mut CloudAvailability,
    flow: &mut OutputReceiver,
    route: &mut Route,
    reply: &mut Option<Reply>,
    queue: &mut VecDeque<ProviderInput>,
    trace: &mut SessionState,
) -> Result<()> {
    let Some((turn, since)) = route.hold else {
        return Ok(());
    };
    if fixed_face_question(queue, &route.replay, turn) {
        route.hold = None;
        return Ok(());
    }
    let Some(current) = reply.as_mut().filter(|current| {
        current.owner.turn() == turn
            && !current.cloud.same_session_only
            && !current.requested_audio_bound()
            && current.cloud.outcome.is_none()
            && !current.cloud.input_abandoned
    }) else {
        route.hold = None;
        route.replay.clear();
        return Ok(());
    };
    let at = monotonic_us();
    let held_us = at.saturating_sub(since);
    // Past the hold its oldest block may be too old for Gemini, Ready or not.
    let to = if held_us < STARTUP_HOLD_US && route.cloud.available() {
        "live"
    } else if held_us >= STARTUP_HOLD_US || route.cloud.terminal() || route.cloud.output_ended() {
        "second"
    } else {
        return Ok(());
    };
    route.hold = None;
    route.stand_in = false;
    route.replay.extend(queue.drain(..));
    if to == "live" {
        swap(provider, cloud, flow, route);
        rebind(cloud, current, &mut route.replay)?;
    }
    record(
        trace,
        json!({"kind":"voice_route_held","at_us":at,"turn":turn,
        "held_ms":held_us / 1_000,"to":to}),
    )?;
    if to == "second" {
        record(
            trace,
            json!({"kind":"voice_route_switched","at_us":at,"to":"second",
            "why":"live_link_not_ready","turn":turn,"replayed_commands":route.replay.len()}),
        )?;
    }
    Ok(())
}

/// Once per tick, after the first priority link service: the standby's
/// reports, the line's upkeep, a line whose link changed, and the switches
/// at rest.
pub(super) fn service(
    rig: &mut Rig,
    cloud: &mut CloudAvailability,
    flow: &mut OutputReceiver,
    reply: &mut Option<Reply>,
    queue: &mut VecDeque<ProviderInput>,
    draining: bool,
    trace: &mut SessionState,
) -> Result<()> {
    let Rig {
        provider, second, ..
    } = rig;
    let Some(route) = second.as_mut() else {
        return Ok(());
    };
    route.drain(trace)?;
    route.upkeep(reply.as_ref());
    resolve_hold(provider, cloud, flow, route, reply, queue, trace)?;
    if let Some(current) = reply.as_mut() {
        // The link changed during the line, which the gate left for this.
        let changed = current.cloud.movable
            && !current.cloud.input_end_submitted
            && !current.cloud.input_abandoned
            && current.cloud.outcome.is_none()
            && cloud.ready_session() != Some(current.cloud.session);
        if changed && route.can_take(current) {
            let commands = take_over(provider, cloud, flow, route, current, queue)?;
            record(
                trace,
                json!({"kind":"voice_route_switched","at_us":monotonic_us(),"to":"second",
                "why":"live_link_changed","turn":current.owner.turn(),"replayed_commands":commands}),
            )?;
        }
        current.cloud.movable = route.can_take(current);
    }
    let at_rest = queue.is_empty() && route.replay.is_empty() && !draining;
    if !route.second_active
        && at_rest
        && reply.as_ref().is_none_or(|current| {
            current.cloud.outcome.is_some()
                && !current.cloud.same_session_only
                && !current.requested_audio_bound()
        })
        && !cloud.available()
        && !cloud.terminal()
        && !cloud.output_ended()
        && route.cloud.available()
    {
        swap(provider, cloud, flow, route);
        // While Gemini's link is not Ready (before its first Ready, or
        // reconnecting: a GoAway or renewal takes about 2 s on lamp-4ace,
        // #960) the second route only stands in: a request is held for
        // Gemini, and the second route is given it only when the hold is
        // over (`resolve_hold`).
        route.stand_in = true;
    } else if route.second_active
        && at_rest
        && reply.is_none()
        && (route.cloud.available() || route.cloud.terminal() || route.cloud.output_ended())
    {
        swap(provider, cloud, flow, route);
        if !std::mem::take(&mut route.stand_in) {
            record(
                trace,
                json!({"kind":"voice_route_switched","at_us":monotonic_us(),"to":"live",
                "why":if cloud.available() {"live_link_ready"} else {"live_link_ended"},
                "turn":null,"replayed_commands":0}),
            )?;
        }
    }
    Ok(())
}

/// Before an output of the active route is taken: a turn Gemini failed
/// before any of its answer was heard goes to the second route instead.
/// True when the failure was taken over and is not to be observed.
pub(super) fn takes_failed_turn(
    rig: &mut Rig,
    cloud: &mut CloudAvailability,
    flow: &mut OutputReceiver,
    reply: &mut Option<Reply>,
    queue: &mut VecDeque<ProviderInput>,
    event: &ProviderOutput,
    trace: &mut SessionState,
) -> Result<bool> {
    let ProviderOutput::TurnFailed {
        request,
        stage,
        reason,
    } = *event
    else {
        return Ok(false);
    };
    let Rig {
        provider, second, ..
    } = rig;
    let Some(route) = second.as_mut() else {
        return Ok(false);
    };
    let Some(current) = reply
        .as_mut()
        .filter(|current| current.owner.turn() == request && current.cloud.start_submitted)
    else {
        return Ok(false);
    };
    if !route.can_take(current) {
        return Ok(false);
    }
    let commands = take_over(provider, cloud, flow, route, current, queue)?;
    record(
        trace,
        json!({"kind":"voice_route_switched","at_us":monotonic_us(),"to":"second",
        "why":"live_turn_failed","turn":request,"stage":stage,"reason":reason,
        "replayed_commands":commands}),
    )?;
    Ok(true)
}

/// The one send of queued input to the active route. While Gemini speaks,
/// what it is sent is kept as the line; a line being replayed goes first,
/// and what was queued since joins it in order.
pub(super) fn flush(
    rig: &mut Rig,
    cloud: &mut CloudAvailability,
    flow: &mut OutputReceiver,
    reply: &mut Option<Reply>,
    queue: &mut VecDeque<ProviderInput>,
    trace: &mut SessionState,
) -> Result<()> {
    let Rig {
        provider, second, ..
    } = rig;
    let Some(route) = second.as_mut() else {
        return Ok(flush_provider_input(
            cloud,
            reply,
            queue,
            &mut provider.channels.data,
        )?);
    };
    // A replay goes with the request it was for (a cancellation in this
    // tick may have ended it).
    route.upkeep(reply.as_ref());
    // Holding continuations originate on Gemini. A numeric session match
    // on the standby cannot authorize sending their note to that worker.
    if route.second_active
        && reply
            .as_ref()
            .is_some_and(|current| current.cloud.same_session_only)
    {
        return Err(io::Error::other("a session-bound request reached another voice route").into());
    }
    // A turn begun while the second route answers goes to Gemini once its
    // link is Ready again, before anything of it is sent: between turns is
    // not the only way back, as a busy room may never leave one (#960,
    // lamp-4ace 09:03-09:06, every turn on the second route, Gemini up).
    if route.second_active
        && route.cloud.available()
        && route.replay.is_empty()
        && let Some(current) = reply.as_mut().filter(|current| {
            !current.cloud.start_submitted
                && !current.cloud.same_session_only
                && !current.requested_audio_bound()
                && current.cloud.outcome.is_none()
                && current.reminder.is_none()
                // This fixed question was created on this worker; it is not
                // a Gemini continuation and must not migrate before submit.
                && !fixed_face_question(queue, &route.replay, current.owner.turn())
        })
    {
        swap(provider, cloud, flow, route);
        route.stand_in = false;
        rebind(cloud, current, queue)?;
        record(
            trace,
            json!({"kind":"voice_route_switched","at_us":monotonic_us(),"to":"live",
            "why":"live_link_ready","turn":current.owner.turn(),"replayed_commands":0}),
        )?;
    }
    // Completed-question receipts belong to the captured originating worker,
    // even if C retired and Gemini became active again before a later Stop.
    // They are never part of a replay of the next caller's input.
    for _ in 0..8 {
        let Some(command) = queue
            .front()
            .filter(|command| matches!(command, ProviderInput::FaceConsentDelivery { .. }))
        else {
            break;
        };
        let ProviderInput::FaceConsentDelivery { candidate, .. } = command else {
            unreachable!()
        };
        let channel = if candidate.worker == <[u8; 16]>::from(provider.boot) {
            Some(&mut provider.channels.data)
        } else if candidate.worker == <[u8; 16]>::from(route.worker.boot) {
            Some(&mut route.worker.channels.data)
        } else {
            None // That worker exited; its in-memory capability exited with it.
        };
        if let Some(channel) = channel {
            match channel.send(command) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
        queue.pop_front();
    }
    let channel = &mut provider.channels.data;
    let fixed_question = reply
        .as_ref()
        .is_some_and(|current| fixed_face_question(queue, &route.replay, current.owner.turn()));
    if !fixed_question && route.holds(reply.as_ref()) {
        route.replay.extend(queue.drain(..));
        return Ok(());
    }
    let replaying = !route.replay.is_empty();
    let source = if route.replay.is_empty() {
        queue
    } else {
        route.replay.extend(queue.drain(..));
        &mut route.replay
    };
    // A line given back to Gemini after a hold is kept as any other.
    let keep = !route.second_active
        && reply.as_ref().is_none_or(|current| {
            !current.cloud.same_session_only && !current.requested_audio_bound()
        });
    let mut paced = (keep && replaying).then_some(LIVE_REPLAY_PER_TICK);
    let line = &mut route.line;
    Ok(flush_provider_input_with(
        cloud,
        reply,
        source,
        |command| {
            if let Some(left) = paced.as_mut() {
                if *left == 0 {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                *left -= 1;
            }
            channel.send(command)?;
            if keep {
                line.keep(command);
            }
            Ok(())
        },
    )?)
}

#[cfg(test)]
mod tests {
    mod face_consent {
        include!("second/face_consent_tests.rs");
    }

    use super::*;
    use crate::provider_worker::{FailedStage, Fault};

    /// #1673: texts are answered where a second route's worker runs at all:
    /// standing by or as the only voice, and not with Gemini alone.
    #[test]
    fn a_second_route_is_there_standing_by_or_alone_and_not_with_gemini_alone() {
        let config = PathBuf::from("/etc/second.json");
        assert!(
            Routes::Live {
                standby: Some(config.clone())
            }
            .has_second()
        );
        assert!(Routes::SecondOnly(config).has_second());
        assert!(!Routes::Live { standby: None }.has_second());
    }

    fn audio(request: u64, sequence: u64, samples: usize) -> ProviderInput {
        ProviderInput::Audio {
            request,
            privacy_generation: 1,
            sequence,
            read_completed_at_us: 1,
            samples: vec![7; samples],
        }
    }

    fn start(request: u64) -> ProviderInput {
        ProviderInput::Start {
            request,
            session: 1,
            privacy_generation: 1,
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
            heard_of_cut: None,
            soft_lead_us: 0,
        }
    }

    #[test]
    fn a_line_is_bounded_at_thirty_seconds_and_then_not_replayed() {
        let mut line = Line::default();
        line.keep(&start(4));
        for sequence in 1..=3_000 {
            line.keep(&audio(4, sequence, 160));
        }
        assert!(line.covers(4));
        assert_eq!(line.samples, MAX_LINE_SAMPLES);
        assert_eq!(line.commands.len(), 3_001);
        line.keep(&audio(4, 3_001, 160));
        assert!(!line.covers(4));
        assert!(line.commands.is_empty() && line.commands.capacity() == 0);
        // Nothing of that request is kept again; a new line starts afresh.
        line.keep(&audio(4, 3_002, 160));
        assert!(line.commands.is_empty());
        line.keep(&start(5));
        assert!(line.covers(5));
    }

    #[test]
    fn a_line_keeps_only_its_own_request_from_its_start() {
        let mut line = Line::default();
        line.keep(&audio(3, 1, 160));
        assert!(!line.covers(3));
        line.keep(&start(3));
        line.keep(&audio(3, 1, 160));
        line.keep(&audio(2, 9, 160));
        line.keep(&ProviderInput::End {
            request: 3,
            privacy_generation: 1,
            sound: false,
            cut_in: false,
        });
        assert_eq!(line.commands.len(), 3);
    }

    #[test]
    fn the_arguments_are_the_live_workers_with_both_configurations_and_no_kept_link() {
        let provider: Vec<String> = [
            "/etc/provider.json",
            "--thread-file",
            "/t",
            "--keep-link",
            "--wait-for-room",
            "--tools",
        ]
        .map(String::from)
        .to_vec();
        let second = Path::new("/etc/second.json");
        assert_eq!(
            arguments(&provider, second, &RouteFlags::default()).unwrap(),
            [
                "/etc/provider.json",
                "/etc/second.json",
                "--thread-file",
                "/t",
                "--tools"
            ]
        );
        let all = RouteFlags {
            wait_cue: true,
            down_line: true,
            only_voice: true,
            chat: Some(ChatRoute::Gemini),
            speech: Some(SpeechRoute::Gemini),
            search: Some(SearchRoute::DeepSeek),
        };
        let given = arguments(&provider, second, &all).unwrap();
        assert_eq!(
            given[..9],
            [
                "/etc/provider.json",
                "/etc/second.json",
                "--wait-cue",
                "--down-line",
                "--only-voice",
                "--gemini-chat",
                "--gemini-speech",
                "--deepseek-search",
                "--thread-file"
            ]
        );
        // The worker takes back exactly what was given, the rest left as
        // the Live worker's flags.
        let (taken, rest) = RouteFlags::take(&given[2..]);
        assert_eq!(taken, all);
        assert_eq!(rest, ["--thread-file", "/t", "--tools"]);
        // Every chat and search choice goes and comes back as itself.
        for chat in [
            ChatRoute::Gemini,
            ChatRoute::OpenRouter,
            ChatRoute::DeepSeek,
        ] {
            for search in [
                None,
                Some(SearchRoute::DeepSeek),
                Some(SearchRoute::OpenRouter),
                Some(SearchRoute::Exa),
            ] {
                let route = RouteFlags {
                    chat: Some(chat),
                    search,
                    ..RouteFlags::default()
                };
                let given = arguments(&provider, second, &route).unwrap();
                assert_eq!(RouteFlags::take(&given[2..]).0, route);
            }
        }
        let speech_only = RouteFlags {
            speech: Some(SpeechRoute::Gemini),
            ..RouteFlags::default()
        };
        let given = arguments(&provider, second, &speech_only).unwrap();
        assert_eq!(
            given[..3],
            ["/etc/provider.json", "/etc/second.json", "--gemini-speech"]
        );
        assert_eq!(RouteFlags::take(&given[2..]).0, speech_only);
        assert!(configured(None).is_none());
        assert!(configured(Some(OsString::new())).is_none());
        assert_eq!(
            configured(Some("/etc/second.json".into())),
            Some(PathBuf::from("/etc/second.json"))
        );
    }

    /// `LAMP_VOICE_ROUTE` unset, empty or `gemini` is the voice as before:
    /// Gemini's worker answers, the second route standing by only when it
    /// is configured, and only in the service.
    #[test]
    fn unset_or_gemini_is_gemini_live_with_the_second_route_standing_by_as_before() {
        let config = || Some(OsString::from("/etc/second.json"));
        let standby = Routes::Live {
            standby: Some(PathBuf::from("/etc/second.json")),
        };
        let alone = Routes::Live { standby: None };
        for route in [None, Some(OsString::new()), Some("gemini".into())] {
            assert_eq!(routes(route.clone(), config(), true), Ok(standby.clone()));
            assert_eq!(routes(route.clone(), None, true), Ok(alone.clone()));
            assert_eq!(
                routes(route.clone(), Some(OsString::new()), true),
                Ok(alone.clone())
            );
            // A bounded run never had a second route.
            assert_eq!(routes(route, config(), false), Ok(alone.clone()));
        }
        assert!(standby.standing_by());
        assert!(!alone.standing_by());
    }

    /// The reviewer on #1686: `run_start` says `"voice_route":"second"` with
    /// the second route alone, and nothing new otherwise.
    #[test]
    fn run_start_names_the_second_route_alone_and_nothing_else() {
        let plain = json!({"kind": "run_start", "provider_kind": "gemini"});
        let mut alone = plain.clone();
        Routes::SecondOnly(PathBuf::from("/etc/second.json")).mark(&mut alone);
        assert_eq!(alone["voice_route"], "second");
        for routes in [
            Routes::Live { standby: None },
            Routes::Live {
                standby: Some(PathBuf::from("/etc/second.json")),
            },
        ] {
            let mut run_start = plain.clone();
            routes.mark(&mut run_start);
            assert_eq!(run_start, plain);
        }
    }

    /// `second` with its configuration is the second route alone.
    #[test]
    fn second_with_its_configuration_is_the_second_route_alone() {
        assert_eq!(
            routes(Some("second".into()), Some("/etc/second.json".into()), true),
            Ok(Routes::SecondOnly(PathBuf::from("/etc/second.json")))
        );
        assert!(!Routes::SecondOnly(PathBuf::from("/etc/second.json")).standing_by());
    }

    /// Without the second route's configuration, `second` is refused at
    /// start, never Gemini in its place; so is anything but the two names,
    /// and `second` in a bounded run.
    #[test]
    fn second_without_its_configuration_or_an_unknown_route_is_refused() {
        for config in [None, Some(OsString::new())] {
            let why = routes(Some("second".into()), config, true).unwrap_err();
            assert!(
                why.contains("LAMP_VOICE_ROUTE=second needs LAMP_VOICE_SECOND_CONFIG")
                    && why.contains("does not fall back to Gemini Live"),
                "{why}"
            );
        }
        for route in ["Second", "gemini-live", "live", "on", " second"] {
            let why =
                routes(Some(route.into()), Some("/etc/second.json".into()), true).unwrap_err();
            assert_eq!(
                why,
                format!(
                    "LAMP_VOICE_ROUTE is gemini or second, not {route:?}: the voice does not start"
                )
            );
        }
        let why = routes(
            Some("second".into()),
            Some("/etc/second.json".into()),
            false,
        )
        .unwrap_err();
        assert!(why.contains("for the service"), "{why}");
    }

    /// Gemini's worker answers with the second route as its standby, or
    /// the second route's worker answers alone, told it is the only voice,
    /// with nothing standing by.
    #[test]
    fn the_workers_are_gemini_and_its_standby_or_the_second_route_alone() {
        let provider: Vec<String> = ["/etc/provider.json", "--keep-link", "--tools"]
            .map(String::from)
            .to_vec();
        let config = PathBuf::from("/etc/second.json");
        let flags = |wait_cue, down_line| RouteFlags {
            wait_cue,
            down_line,
            speech: Some(SpeechRoute::Gemini),
            ..RouteFlags::default()
        };
        assert_eq!(
            Routes::Live { standby: None }
                .workers(provider.clone(), flags(true, true))
                .unwrap(),
            Workers {
                role: "provider",
                arguments: provider.clone(),
                standby: None,
            }
        );
        // Standing by, the second route is never told it is the only voice,
        // whatever it is given.
        assert_eq!(
            Routes::Live {
                standby: Some(config.clone())
            }
            .workers(
                provider.clone(),
                RouteFlags {
                    only_voice: true,
                    ..flags(false, true)
                }
            )
            .unwrap(),
            Workers {
                role: "provider",
                arguments: provider.clone(),
                standby: Some(arguments(&provider, &config, &flags(false, true)).unwrap()),
            }
        );
        let alone = Routes::SecondOnly(config.clone())
            .workers(provider.clone(), flags(true, false))
            .unwrap();
        assert_eq!(
            alone,
            Workers {
                role: "second",
                arguments: arguments(
                    &provider,
                    &config,
                    &RouteFlags {
                        only_voice: true,
                        ..flags(true, false)
                    }
                )
                .unwrap(),
                standby: None,
            }
        );
        assert_eq!(
            alone.arguments,
            [
                "/etc/provider.json",
                "/etc/second.json",
                "--wait-cue",
                "--only-voice",
                "--gemini-speech",
                "--tools"
            ]
        );
    }

    /// The Gemini switches are on with `gemini` and off unset or empty;
    /// anything else is refused at start. The two older ones keep `on`.
    #[test]
    fn the_second_routes_switches_are_read_and_a_wrong_one_refused() {
        let flags = |pairs: &'static [(&'static str, &'static str)]| {
            RouteFlags::from_values(|name| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| OsString::from(*value))
            })
        };
        assert_eq!(flags(&[]), Ok(RouteFlags::default()));
        assert_eq!(
            flags(&[
                ("LAMP_SECOND_CHAT", "gemini"),
                ("LAMP_SECOND_SPEECH", ""),
                ("LAMP_SECOND_WAIT_CUE", "on"),
                ("LAMP_SECOND_DOWN_LINE", "yes"),
            ]),
            Ok(RouteFlags {
                wait_cue: true,
                chat: Some(ChatRoute::Gemini),
                ..RouteFlags::default()
            })
        );
        // One voice per lamp: each value picks its voice (#1712).
        for (value, voice) in [
            ("gemini", SpeechRoute::Gemini),
            ("elevenlabs", SpeechRoute::ElevenLabs),
            ("openai", SpeechRoute::OpenAi),
        ] {
            let route = RouteFlags::from_values(|name| {
                (name == "LAMP_SECOND_SPEECH").then(|| OsString::from(value))
            })
            .unwrap();
            assert_eq!(route.speech, Some(voice), "{value}");
            let given: Vec<String> = route
                .flags()
                .iter()
                .map(|flag| (*flag).to_owned())
                .collect();
            assert_eq!(
                RouteFlags::take(&given).0,
                route,
                "{value} goes to the worker"
            );
        }
        assert_eq!(
            flags(&[
                ("LAMP_SECOND_CHAT", "openrouter"),
                ("LAMP_SECOND_SEARCH", "openrouter"),
            ]),
            Ok(RouteFlags {
                chat: Some(ChatRoute::OpenRouter),
                search: Some(SearchRoute::OpenRouter),
                ..RouteFlags::default()
            })
        );
        assert_eq!(
            flags(&[
                ("LAMP_SECOND_CHAT", "deepseek"),
                ("LAMP_SECOND_SEARCH", "deepseek"),
            ]),
            Ok(RouteFlags {
                chat: Some(ChatRoute::DeepSeek),
                search: Some(SearchRoute::DeepSeek),
                ..RouteFlags::default()
            })
        );
        assert_eq!(
            flags(&[("LAMP_SECOND_SEARCH", "exa")]).map(|f| f.search),
            Ok(Some(SearchRoute::Exa))
        );
        for (name, value, words) in [
            ("LAMP_SECOND_CHAT", "on", "gemini, openrouter, deepseek"),
            (
                "LAMP_SECOND_CHAT",
                "DeepSeek",
                "gemini, openrouter, deepseek",
            ),
            ("LAMP_SECOND_SPEECH", "Gemini", "gemini, elevenlabs, openai"),
            ("LAMP_SECOND_SPEECH", "polly", "gemini, elevenlabs, openai"),
            ("LAMP_SECOND_SEARCH", "google", "deepseek, openrouter, exa"),
        ] {
            let why =
                RouteFlags::from_values(|asked| (asked == name).then(|| OsString::from(value)))
                    .unwrap_err();
            assert_eq!(
                why,
                format!("{name} is {words} or nothing, not {value:?}: the voice does not start")
            );
        }
        // OpenRouter's web plugin rides on OpenRouter's chat alone.
        let why = flags(&[
            ("LAMP_SECOND_CHAT", "deepseek"),
            ("LAMP_SECOND_SEARCH", "openrouter"),
        ])
        .unwrap_err();
        assert!(why.contains("needs LAMP_SECOND_CHAT=openrouter"), "{why}");
    }

    /// Mute, privacy and cancellation all publish an authority without
    /// the line's owner or with the microphone closed: the line goes then.
    #[test]
    fn a_line_goes_at_once_when_its_owner_or_the_microphone_does() {
        let directory = SessionDirectory::create().unwrap();
        let (worker, mut peer) = Worker::sleeping_fixture(&directory.path, "second");
        let mut route = Route::new(worker);
        let (mut owner, current, _) = super::super::tests::active_reply();
        let turn = current.owner.turn();
        route.line.keep(&start(turn));
        route.publish(owner.snapshot(now()).unwrap()).unwrap();
        assert!(route.line.covers(turn));
        owner
            .set_microphone_permission(now(), Permission::Denied)
            .unwrap();
        let closed = owner.snapshot(now()).unwrap();
        route.publish(closed).unwrap();
        assert!(!route.line.covers(turn) && route.line.commands.is_empty());
        assert!(matches!(
            peer.control.receive::<Control>().unwrap(),
            Some(Control::Authority { .. })
        ));
    }

    #[test]
    fn a_line_goes_once_it_is_answered_or_abandoned() {
        let directory = SessionDirectory::create().unwrap();
        let (worker, _peer) = Worker::sleeping_fixture(&directory.path, "second");
        let mut route = Route::new(worker);
        let (_owner, mut current, _) = super::super::tests::active_reply();
        let turn = current.owner.turn();
        current.had_audio = false;
        route.line.keep(&start(turn));
        route.upkeep(Some(&current));
        assert!(route.line.covers(turn));
        // Its first answer audio.
        current.had_audio = true;
        route.upkeep(Some(&current));
        assert!(!route.line.covers(turn));
        current.had_audio = false;
        route.line.keep(&start(turn));
        current.cloud.input_abandoned = true;
        route.upkeep(Some(&current));
        assert!(!route.line.covers(turn));
        route.line.keep(&start(turn));
        route.replay.push_back(start(turn));
        route.upkeep(None);
        assert!(!route.line.covers(turn));
        // A replay cancelled with its turn is not sent to the next owner.
        assert!(route.replay.is_empty());
    }

    fn end(request: u64) -> ProviderInput {
        ProviderInput::End {
            request,
            privacy_generation: 1,
            sound: false,
            cut_in: false,
        }
    }

    fn link(revision: u64, ready: bool) -> ProviderLink {
        ProviderLink {
            revision,
            session: 1,
            observed_at_us: monotonic_us(),
            source_clock: None,
            state: if ready {
                ProviderLinkState::Ready {
                    setup_us: 1,
                    attempts: 1,
                    context: crate::provider_link::LinkContext::Initial,
                    outage_started_at_us: None,
                    after: None,
                }
            } else {
                ProviderLinkState::Unavailable {
                    reason: crate::provider_worker::Fault::Transport,
                }
            },
        }
    }

    /// Start + `blocks` Audio + End of `request`.
    fn whole_line(request: u64, blocks: u64) -> VecDeque<ProviderInput> {
        let mut line = VecDeque::from([start(request)]);
        line.extend((1..=blocks).map(|sequence| audio(request, sequence, 160)));
        line.push_back(end(request));
        line
    }

    /// The voice at startup, before Gemini's first Ready: the second route
    /// is active and only stands in, Gemini's worker is the standby.
    struct Bench {
        rig: Rig,
        /// The active register: the second route's, Ready.
        cloud: CloudAvailability,
        flow: OutputReceiver,
        trace: SessionState,
        gemini: WorkerChannels,
        second: WorkerChannels,
        _rest: Vec<WorkerChannels>,
        _directory: SessionDirectory,
    }

    impl Bench {
        fn standing_in() -> Self {
            let directory = SessionDirectory::create().unwrap();
            let (second_worker, second) = Worker::sleeping_fixture(&directory.path, "second");
            let (gemini_worker, gemini) = Worker::sleeping_fixture(&directory.path, "provider");
            let (capture, capture_peer) = Worker::sleeping_fixture(&directory.path, "capture");
            let (speaker, speaker_peer) = Worker::sleeping_fixture(&directory.path, "speaker");
            let (privacy, privacy_peer) = Worker::sleeping_fixture(&directory.path, "privacy");
            let mut route = Route::new(gemini_worker);
            route.second_active = true;
            route.stand_in = true;
            let mut cloud = CloudAvailability::default();
            cloud.observe(link(1, true), monotonic_us()).unwrap();
            Self {
                rig: Rig {
                    expression_out: None,
                    second: Some(route),
                    microphone: crate::options::Microphone::Mic2,
                    end_silence_blocks: None,
                    early_end_blocks: None,
                    early_finalize: false,
                    open_name: false,
                    call_takes_next: false,
                    unfinished_wait: false,
                    faint_level: None,
                    speaker,
                    capture,
                    provider: second_worker,
                    privacy,
                    privacy_controls: false,
                    capture_held: false,
                    privacy_receipts: false,
                    ring: None.into(),
                    camera: None,
                    motor: None,
                    ring_channel_ceiling: None,
                    diagnostic_directory: None,
                    noise_suppression: NoiseSuppression::On,
                    turn_taking: TurnTaking::VadOnly,
                    playout_start: PlayoutStart::default(),
                    fixture_at_ms: None,
                    endpoint: None,
                    call_wait_blocks: None,
                    cut_keeps_name: false,
                    asker_first: crate::asker_first::Mode::Off,
                    follows_on: false,
                    paused: Box::new(|| false),
                },
                cloud,
                flow: OutputReceiver::default(),
                trace: SessionState::default(),
                gemini,
                second,
                _rest: vec![capture_peer, speaker_peer, privacy_peer],
                _directory: directory,
            }
        }

        fn route(&mut self) -> &mut Route {
            self.rig.second.as_mut().unwrap()
        }

        fn gemini_link(&mut self, revision: u64, ready: bool) {
            self.gemini
                .control
                .send(WorkerEvent::ProviderLink {
                    link: link(revision, ready),
                })
                .unwrap();
        }

        fn service(&mut self, reply: &mut Option<Reply>, queue: &mut VecDeque<ProviderInput>) {
            service(
                &mut self.rig,
                &mut self.cloud,
                &mut self.flow,
                reply,
                queue,
                false,
                &mut self.trace,
            )
            .unwrap();
        }

        fn flush(&mut self, reply: &mut Option<Reply>, queue: &mut VecDeque<ProviderInput>) {
            flush(
                &mut self.rig,
                &mut self.cloud,
                &mut self.flow,
                reply,
                queue,
                &mut self.trace,
            )
            .unwrap();
        }
    }

    /// A request not yet sent anywhere, of the active reply's turn.
    fn unsent_reply() -> (Controller, Reply) {
        let (owner, mut current, _) = super::super::tests::active_reply();
        current.had_audio = false;
        current.pcm.clear();
        current.cloud = CloudRequest::new(1).unwrap();
        (owner, current)
    }

    /// (request, Audio sequence or 0 for Start, u64::MAX for End) of all
    /// a peer was sent.
    fn received(peer: &mut WorkerChannels) -> Vec<(u64, u64)> {
        let mut seen = Vec::new();
        while let Some(command) = peer.data.receive::<ProviderInput>().unwrap() {
            seen.push(match command {
                ProviderInput::Start { request, .. } => (request, 0),
                ProviderInput::Audio {
                    request, sequence, ..
                } => (request, sequence),
                ProviderInput::End { request, .. } => (request, u64::MAX),
                other => panic!("unexpected {other:?}"),
            });
        }
        seen
    }

    fn sequences(request: u64, blocks: u64) -> Vec<(u64, u64)> {
        let mut all = vec![(request, 0)];
        all.extend((1..=blocks).map(|sequence| (request, sequence)));
        all.push((request, u64::MAX));
        all
    }

    fn written(request: u64) -> ProviderInput {
        ProviderInput::Written {
            request,
            session: 1,
            privacy_generation: 1,
            text: crate::holding_answer::NOTE.into(),
            look: false,
            search: false,
            quoted: false,
        }
    }

    #[test]
    fn a_session_bound_written_turn_fails_on_its_original_route_without_replay() {
        for pinned in [false, true] {
            let mut bench = Bench::standing_in();
            bench.gemini_link(1, true);
            bench.service(&mut None, &mut VecDeque::new());
            assert!(!bench.route().second_active);
            let original_worker = bench.rig.provider.boot;
            let (_owner, mut current) = unsent_reply();
            let turn = current.owner.turn();
            current.cloud.same_session_only = pinned;
            current.playback = None;
            current.input_sequence = 0;
            let mut reply = Some(current);
            let mut queue = VecDeque::from([written(turn)]);
            bench.service(&mut reply, &mut queue);
            bench.flush(&mut reply, &mut queue);
            assert!(matches!(
                bench.gemini.data.receive::<ProviderInput>().unwrap(),
                Some(ProviderInput::Written { request, session: 1, text, .. })
                    if request == turn && text == crate::holding_answer::NOTE
            ));
            assert!(queue.is_empty());
            reply.as_mut().unwrap().cloud.confirm_start(1).unwrap();
            bench.cloud.observe(link(2, false), monotonic_us()).unwrap();
            let failed = ProviderOutput::TurnFailed {
                request: turn,
                stage: FailedStage::Responding,
                reason: Fault::Transport,
            };
            let moved = takes_failed_turn(
                &mut bench.rig,
                &mut bench.cloud,
                &mut bench.flow,
                &mut reply,
                &mut queue,
                &failed,
                &mut bench.trace,
            )
            .unwrap();
            assert_eq!(moved, !pinned);
            if pinned {
                observe_provider_output(&mut bench.cloud, &mut reply, &failed).unwrap();
                bench.service(&mut reply, &mut queue);
                bench.flush(&mut reply, &mut queue);
                assert_eq!(bench.rig.provider.boot, original_worker);
                assert!(!reply.as_ref().unwrap().cloud.movable);
                assert!(matches!(
                    reply.as_ref().unwrap().cloud.outcome,
                    Some(RequestOutcome::Failed {
                        stage: FailedStage::Responding,
                        ..
                    })
                ));
                assert!(
                    bench
                        .second
                        .data
                        .receive::<ProviderInput>()
                        .unwrap()
                        .is_none()
                );
                assert!(bench.route().replay.is_empty());
                assert!(!bench.route().line.covers(turn));
                // Once this reply retires, normal idle fallback is available.
                reply = None;
                bench.service(&mut reply, &mut queue);
                assert!(bench.route().second_active);
            } else {
                bench.flush(&mut reply, &mut queue);
                assert!(matches!(
                    bench.second.data.receive::<ProviderInput>().unwrap(),
                    Some(ProviderInput::Written { request, .. }) if request == turn
                ));
            }
            assert!(
                bench
                    .gemini
                    .data
                    .receive::<ProviderInput>()
                    .unwrap()
                    .is_none()
            );
            assert!(
                bench
                    .second
                    .data
                    .receive::<ProviderInput>()
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn a_session_bound_note_cannot_use_a_stand_ins_matching_session_number() {
        for live_ready in [false, true] {
            let mut bench = Bench::standing_in();
            if live_ready {
                bench
                    .route()
                    .cloud
                    .observe(link(1, true), monotonic_us())
                    .unwrap();
            }
            let original_worker = bench.rig.provider.boot;
            let (_owner, mut current) = unsent_reply();
            current.cloud.same_session_only = true;
            let turn = current.owner.turn();
            assert!(!bench.route().holds(Some(&current)));
            let mut reply = Some(current);
            let mut queue = VecDeque::from([written(turn)]);
            assert!(
                flush(
                    &mut bench.rig,
                    &mut bench.cloud,
                    &mut bench.flow,
                    &mut reply,
                    &mut queue,
                    &mut bench.trace,
                )
                .is_err()
            );
            assert_eq!(bench.rig.provider.boot, original_worker);
            assert_eq!(queue.len(), 1);
            assert!(!reply.as_ref().unwrap().cloud.start_submitted);
            assert!(bench.route().hold.is_none());
            assert!(bench.route().replay.is_empty());
            assert!(
                bench
                    .gemini
                    .data
                    .receive::<ProviderInput>()
                    .unwrap()
                    .is_none()
            );
            assert!(
                bench
                    .second
                    .data
                    .receive::<ProviderInput>()
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn a_session_bound_request_refuses_rebinding_before_commands_or_evidence_change() {
        let mut cloud = CloudAvailability::default();
        let mut ready = link(1, true);
        ready.session = 2;
        ready.state = ProviderLinkState::Ready {
            setup_us: 1,
            attempts: 1,
            context: crate::provider_link::LinkContext::Fresh,
            outage_started_at_us: Some(ready.observed_at_us),
            after: Some(Fault::Transport),
        };
        cloud.observe(ready, monotonic_us()).unwrap();
        let (_owner, mut current) = unsent_reply();
        current.cloud.same_session_only = true;
        let mut queue = VecDeque::from([written(current.owner.turn())]);
        assert!(rebind(&cloud, &mut current, &mut queue).is_err());
        assert!(current.cloud.same_session_only);
        assert_eq!(current.cloud.session, 1);
        assert!(!current.cloud.start_submitted);
        assert!(matches!(
            queue.front(),
            Some(ProviderInput::Written { session: 1, .. })
        ));
    }

    /// Held, given to Gemini at its first Ready, and Gemini's link lost
    /// after the first bounded flush: the second route is given what was
    /// sent, the rest of the replay and the queue, in order (PR #885 P2).
    #[test]
    fn a_link_lost_mid_replay_gives_the_second_route_every_block_and_the_end() {
        let mut bench = Bench::standing_in();
        let (_owner, current) = unsent_reply();
        let turn = current.owner.turn();
        let mut reply = Some(current);
        let mut queue = whole_line(turn, 20);
        bench.flush(&mut reply, &mut queue);
        assert_eq!(bench.route().replay.len(), 22);
        bench.gemini_link(1, true);
        bench.service(&mut reply, &mut queue);
        assert!(!bench.route().second_active);
        // The first paced flush: Start + 1 Audio to Gemini.
        bench.flush(&mut reply, &mut queue);
        assert_eq!(received(&mut bench.gemini).len(), LIVE_REPLAY_PER_TICK);
        bench.gemini_link(2, false);
        let mut budget = STANDBY_SLICE;
        service_provider_control(
            &mut bench.rig.provider.channels.control,
            &mut bench.cloud,
            &mut budget,
            &mut bench.trace,
        )
        .unwrap();
        bench.service(&mut reply, &mut queue);
        assert!(bench.route().second_active);
        while !bench.route().replay.is_empty() {
            bench.flush(&mut reply, &mut queue);
        }
        assert_eq!(received(&mut bench.second), sequences(turn, 20));
    }

    /// A held turn cancelled and replaced in the same tick, before
    /// `service` ran: the replacement is held, and Gemini's Ready gives it
    /// all to Gemini (PR #885 P2).
    #[test]
    fn a_turn_admitted_in_place_of_a_held_one_is_held_too() {
        let mut bench = Bench::standing_in();
        let (_owner, current) = unsent_reply();
        let turn = current.owner.turn();
        // The cancelled turn's hold and replay, not yet retired.
        let stale = turn + 1;
        bench.route().hold = Some((stale, monotonic_us()));
        bench.route().replay = whole_line(stale, 2);
        let mut reply = Some(current);
        let mut queue = whole_line(turn, 2);
        bench.flush(&mut reply, &mut queue);
        assert!(received(&mut bench.second).is_empty());
        assert_eq!(bench.route().hold.map(|(held, _)| held), Some(turn));
        bench.gemini_link(1, true);
        bench.service(&mut reply, &mut queue);
        while !bench.route().replay.is_empty() {
            bench.flush(&mut reply, &mut queue);
        }
        assert_eq!(received(&mut bench.gemini), sequences(turn, 2));
        assert!(received(&mut bench.second).is_empty());
    }

    /// Gemini Ready in the last 20 ms of the hold: the line, its first
    /// block captured the lead-in and most of the margin before the hold
    /// began, reaches Gemini whole, every block younger than Gemini's
    /// audio age bound when it is sent (lampOS#892).
    #[test]
    fn a_line_given_to_gemini_at_the_end_of_the_hold_is_never_too_old() {
        let _clock =
            crate::scenario_clock::Scope::enter_at(Duration::from_secs(10), 600_000_000).unwrap();
        let mut bench = Bench::standing_in();
        let (_owner, current) = unsent_reply();
        let turn = current.owner.turn();
        let mut reply = Some(current);
        let now = monotonic_us();
        let since = now - (STARTUP_HOLD_US - 20_000);
        let first = since - LEAD_IN_US - (HOLD_MARGIN_US - 100_000);
        let blocks = (now - first) / 10_000;
        let mut queue = VecDeque::from([start(turn)]);
        queue.extend((1..=blocks).map(|sequence| ProviderInput::Audio {
            request: turn,
            privacy_generation: 1,
            sequence,
            read_completed_at_us: first + (sequence - 1) * 10_000,
            samples: vec![7; 160],
        }));
        bench.flush(&mut reply, &mut queue);
        bench.route().hold = Some((turn, since));
        bench.gemini_link(1, true);
        bench.service(&mut reply, &mut queue);
        assert!(
            !bench.route().second_active,
            "the line went to the second route"
        );
        let bound = lamp_gemini::MAX_INPUT_AGE.as_micros() as u64;
        let mut sent = 0;
        while !bench.route().replay.is_empty() {
            bench.flush(&mut reply, &mut queue);
            while let Some(command) = bench.gemini.data.receive::<ProviderInput>().unwrap() {
                if let ProviderInput::Audio {
                    read_completed_at_us,
                    ..
                } = command
                {
                    let age = monotonic_us() - read_completed_at_us;
                    assert!(age < bound, "a block {age} us old was sent to Gemini");
                }
                sent += 1;
            }
        }
        assert_eq!(sent, blocks + 1);
        assert!(received(&mut bench.second).is_empty());
    }

    /// Past the hold the line goes to the second route even when Gemini's
    /// Ready is there in the same tick.
    #[test]
    fn a_ready_after_the_hold_leaves_the_line_to_the_second_route() {
        let _clock =
            crate::scenario_clock::Scope::enter_at(Duration::from_secs(10), 600_000_000).unwrap();
        let mut bench = Bench::standing_in();
        let (_owner, current) = unsent_reply();
        let turn = current.owner.turn();
        let mut reply = Some(current);
        let mut queue = whole_line(turn, 2);
        bench.flush(&mut reply, &mut queue);
        bench.route().hold = Some((turn, monotonic_us() - STARTUP_HOLD_US));
        bench.gemini_link(1, true);
        bench.service(&mut reply, &mut queue);
        assert!(bench.route().second_active);
        while !bench.route().replay.is_empty() {
            bench.flush(&mut reply, &mut queue);
        }
        assert_eq!(received(&mut bench.second), sequences(turn, 2));
        assert!(received(&mut bench.gemini).is_empty());
    }

    /// lamp-52e6, PR #885 run 2: a line held 1.9 s went to Gemini at its
    /// first Ready, and its link gave the connection up 37 ms later as
    /// `Backpressure` (reported `local`), so the second route answered.
    /// Here Gemini's side takes three commands a tick, as a link writing
    /// slower than the replay; what it was sent and has not taken must
    /// stay within its input queue, and the whole line arrives in order.
    #[test]
    fn a_line_held_two_seconds_goes_to_gemini_within_its_input_queue() {
        let mut bench = Bench::standing_in();
        let (_owner, current) = unsent_reply();
        let turn = current.owner.turn();
        let mut reply = Some(current);
        // 2 s of 10 ms blocks, then its End, all held.
        let mut queue = whole_line(turn, 200);
        bench.flush(&mut reply, &mut queue);
        bench.gemini_link(1, true);
        bench.service(&mut reply, &mut queue);
        assert!(!bench.route().second_active);
        let mut taken = Vec::new();
        let mut waiting = VecDeque::new();
        for _ in 0..1_000 {
            bench.flush(&mut reply, &mut queue);
            waiting.extend(received(&mut bench.gemini));
            assert!(
                waiting.len() <= lamp_gemini::INPUT_QUEUE_CAPACITY,
                "{} commands waiting on Gemini's link",
                waiting.len()
            );
            taken.extend(waiting.drain(..waiting.len().min(3)));
            if bench.route().replay.is_empty() && waiting.is_empty() {
                break;
            }
        }
        assert_eq!(taken, sequences(turn, 200));
        assert!(received(&mut bench.second).is_empty());
    }

    /// Gemini's Ready and its loss in one drain while the second route
    /// stands in. Until #960 the next request went to the second route at
    /// once (PR #885 P2); a link that was Ready is now taken to be
    /// reconnecting, and the request is held for it, bounded as a startup
    /// hold, rather than given to a slower route.
    #[test]
    fn a_ready_lost_in_the_same_drain_holds_the_next_request_for_the_reconnect() {
        let _clock =
            crate::scenario_clock::Scope::enter_at(Duration::from_secs(10), 600_000_000).unwrap();
        let mut bench = Bench::standing_in();
        bench.gemini_link(1, true);
        bench.gemini_link(2, false);
        bench.service(&mut None, &mut VecDeque::new());
        let (_owner, current) = unsent_reply();
        let turn = current.owner.turn();
        let mut reply = Some(current);
        let mut queue = whole_line(turn, 2);
        bench.flush(&mut reply, &mut queue);
        assert_eq!(bench.route().hold.map(|(held, _)| held), Some(turn));
        assert!(received(&mut bench.second).is_empty());
        // Past the hold, with Gemini still away, the second route has it.
        bench.route().hold = Some((turn, monotonic_us() - STARTUP_HOLD_US));
        bench.service(&mut reply, &mut queue);
        bench.flush(&mut reply, &mut queue);
        assert!(bench.route().hold.is_none());
        assert_eq!(received(&mut bench.second), sequences(turn, 2));
    }
}
