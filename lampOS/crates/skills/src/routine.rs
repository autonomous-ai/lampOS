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
    ExpressionRestRequest as Request, ExpressionRestState as Receipt, ExpressionState,
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

pub const ENVIRONMENT: &str = "LAMP_SKILL_ROUTINES";
const MAX_SKILLS: usize = 8;
const RENEW_US: u64 = 1_000_000;
const ADMISSION_US: u64 = 1_000_000;
const MAX_EPISODE_US: u64 = 86_400_000_000;
/// Includes the largest valid custom sleep-settle fallback (10 s).
const WAKE_US: u64 = 12_000_000;

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rest {
    pub op: Op,
    pub during: Option<Window>,
}

impl RestFile {
    pub fn checked(self) -> Result<Rest, String> {
        if (self.op == Op::Rest) != self.during.is_some() {
            return Err("rest requires a during window; wake and cancel have none".into());
        }
        Ok(Rest {
            op: self.op,
            during: self.during.map(Hours::checked).transpose()?,
        })
    }
}

struct Asked {
    skill: String,
    rest: Rest,
    issued_us: u64,
    expires_us: u64,
    answer: mpsc::SyncSender<Result<(), String>>,
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
}

impl Permit {
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
        response: Result<(), String>,
        expires_us: u64,
        now_us: u64,
    ) -> Result<Permit, String> {
        response?;
        let permit = Permit {
            run: self.clone(),
            expires_us,
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
        }
        self.definitions.clear();
        self.enabled = names;
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
            let unchanged = definitions
                .get(name)
                .is_some_and(|new| self.definitions.get(name) == Some(new));
            let resting = source.active.as_ref().is_some_and(|active| {
                active.waiting.is_none()
                    && active.request.op == Op::Rest
                    && now < active.request.valid_until_us
            });
            if !unchanged || !resting {
                cancel(&mut self.publisher, source, now);
            }
        }
        self.enabled = definitions.keys().cloned().collect();
        self.definitions = definitions;
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
        for source in self.sources.values_mut() {
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
                    .remaining_ms(moment.minute, unix_ms % 60_000)
                    .and_then(|remaining| sampled_at_us.checked_add(remaining * 1000)),
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
                cancel(&mut self.publisher, source, now);
                continue;
            }
            active.window_until_us = window_until_us.expect("checked window end");
            if accepted && let Some(asked) = active.waiting.take() {
                // A caller that already left cannot leave an active routine.
                if now >= asked.expires_us || asked.answer.try_send(Ok(())).is_err() {
                    cancel(&mut self.publisher, source, now);
                    continue;
                }
            }
            if terminal {
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
        // Every ask in the channel was issued before it was sent, so the
        // next tick's `now` is later than each deferred one: bounded by the
        // channel, and deferred at most once.
        let mut early = std::mem::take(&mut self.early).into_iter();
        while let Some(ask) = early.next().or_else(|| self.asks.try_recv().ok()) {
            if ask.issued_us > now {
                self.early.push(ask);
                continue;
            }
            let refuse = |why: &str| {
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
                    },
                );
            }
            let source = self.sources.get_mut(&ask.skill).expect("source created");
            if ask.rest.op == Op::Cancel {
                cancel(&mut self.publisher, source, now);
                let _ = ask.answer.try_send(Ok(()));
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
                });
            }
            let Some(active) = &mut source.active else {
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
    }
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
        fn ask(&self, op: Op) -> mpsc::Receiver<Result<(), String>> {
            let (answer, result) = mpsc::sync_channel(1);
            self.client
                .send
                .try_send(Asked {
                    skill: "routine".into(),
                    rest: Rest {
                        op,
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
            assert_eq!(rest.try_recv().unwrap(), Ok(()));
            self.manager.sources["routine"]
                .active
                .as_ref()
                .unwrap()
                .request
                .clone()
        }
    }
    fn next_request(requests: &mut lamp_bus::Subscriber) -> Request {
        requests
            .recv(Duration::from_millis(50))
            .unwrap()
            .unwrap()
            .parse()
            .unwrap()
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
            assert_eq!(rest.try_recv().unwrap(), Ok(()));
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
            let mut f = Fixture::new();
            let mut requests = f.manager.bus.subscribe(Request::TOPIC, "delay").unwrap();
            let mut pending = None;
            if existing.is_some() {
                f.resting();
                next_request(&mut requests);
                if existing == Some(Op::Wake) {
                    pending = Some(f.ask(Op::Wake));
                    f.tick(NIGHT);
                    next_request(&mut requests);
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
                assert_eq!(next_request(&mut requests).op, Op::Cancel);
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
            let mut f = Fixture::new();
            let mut requests = f.manager.bus.subscribe(Request::TOPIC, "delay").unwrap();
            let mut pending = None;
            if existing.is_some() {
                f.resting();
                next_request(&mut requests);
                if existing == Some(Op::Wake) {
                    pending = Some(f.ask(Op::Wake));
                    f.tick(NIGHT);
                    next_request(&mut requests);
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
            let issued = next_request(&mut requests);
            assert_eq!(issued.op, existing.unwrap_or(Op::Rest));
            assert_eq!(issued.valid_until_us, until);
            assert_eq!(issued.valid_until_us - f.now, 200_000);
            if existing.is_none() {
                f.observe(Phase::Resting);
                f.tick_sample(sampled_at_us, wall);
                assert_eq!(pending.take().unwrap().try_recv().unwrap(), Ok(()));
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
            f.now = until;
            f.observe(phase);
            f.tick_sample(sampled_at_us, wall);
            assert!(f.manager.sources["routine"].active.is_none());
            assert_eq!(next_request(&mut requests).op, Op::Cancel);
            if let Some(pending) = pending {
                assert!(pending.try_recv().unwrap().is_err());
            }
        }
    }
    #[test]
    fn a_future_wall_sample_cannot_admit_work_or_extend_the_cached_window() {
        let mut f = Fixture::new();
        let mut requests = f.manager.bus.subscribe(Request::TOPIC, "future").unwrap();
        let rest = f.ask(Op::Rest);
        f.tick_sample(f.now + 1, NIGHT);
        assert!(rest.try_recv().unwrap().is_err());
        assert!(requests.recv(Duration::ZERO).unwrap().is_none());
        let initial = f.resting();
        next_request(&mut requests);
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
        let continued = next_request(&mut requests);
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
        assert_eq!(rest.try_recv().unwrap(), Ok(()));
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
        assert_eq!(rest.try_recv().unwrap(), Ok(()));
        let wake = f.ask(Op::Wake);
        f.now += 1;
        f.tick(NIGHT);
        f.admitted(Phase::Waking);
        assert!(wake.try_recv().is_err());
        f.admitted(Phase::Completed);
        assert_eq!(wake.try_recv().unwrap(), Ok(()));
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
        assert_eq!(wake.try_recv().unwrap(), Ok(()));
        assert!(f.manager.sources["routine"].active.is_none());
    }
    #[test]
    fn completion_cannot_cross_an_owner_window_or_clock_change() {
        for change in 0..3 {
            let mut f = Fixture::new();
            let rest = f.ask(Op::Rest);
            f.tick(NIGHT);
            f.admitted(Phase::Resting);
            assert_eq!(rest.try_recv().unwrap(), Ok(()));
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
            answer.try_send(Ok(())).unwrap();
            // The owner answered in time, but the thread did not receive
            // the queued answer until the original deadline had passed.
            assert!(
                run.received(receive.try_recv().unwrap(), expires_us, expires_us + 1)
                    .is_err()
            );
            let permit = run.received(Ok(()), expires_us, expires_us - 1).unwrap();
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
        let permit = run.received(Ok(()), 200, 100).unwrap();
        assert!(permit.check_at(100).is_ok());
        *client.generation.lock().unwrap() = Arc::new(());
        // Reload after receive invalidates the retained permit, and a
        // completion received after reload cannot create another permit.
        assert!(permit.check_at(101).is_err());
        assert!(run.received(Ok(()), 200, 101).is_err());
        assert!(client.for_run().received(Ok(()), 200, 101).is_ok());
    }
    #[test]
    fn a_terminal_receipt_before_the_last_renewal_still_ends_the_episode() {
        let mut f = Fixture::new();
        let rest = f.ask(Op::Rest);
        f.tick(NIGHT);
        f.admitted(Phase::Resting);
        assert_eq!(rest.try_recv().unwrap(), Ok(()));
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
}
