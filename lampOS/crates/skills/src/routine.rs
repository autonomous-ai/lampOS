//! Generic skill rest intents. The skill chooses a window; Expression owns
//! admission, physical transitions and cancellation. A receipt is not proof
//! that the body moved. No request is rebound after an owner restart.
use crate::daily::{Moment, Window};
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
    waiting: Option<Asked>,
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
    sources: HashMap<String, Source>,
    enabled: Vec<String>,
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
                sources: HashMap::new(),
                enabled: Vec::new(),
                generation,
            },
            client,
        ))
    }

    /// Reload cancels all old runs, including one whose name still exists.
    /// No admission made under an earlier definition can survive an edit.
    pub fn reload(&mut self, names: Vec<String>, now: u64) {
        *self.generation.lock().expect("routine generation") = Arc::new(());
        for source in self.sources.values_mut() {
            cancel(&mut self.publisher, source, now);
        }
        while let Ok(ask) = self.asks.try_recv() {
            let _ = ask.answer.try_send(Err("skill definitions changed".into()));
        }
        self.enabled = names;
    }

    pub fn tick(&mut self, now: u64, local: Option<&LocalTime>, unix_ms: u64) {
        let moment = local.and_then(|local| Moment::at(local, unix_ms).ok());
        let target = self
            .bus
            .latest(ExpressionState::TOPIC)
            .ok()
            .flatten()
            .and_then(|message| {
                let state: ExpressionState = message.parse().ok()?;
                (state.check().is_ok()
                    && state.enabled
                    && message.header.source == "expression"
                    && state.stamp.clock_id == self.clock_id
                    && state.stamp.at_us <= now
                    && now - state.stamp.at_us < 500_000)
                    .then_some(message.header.boot)
            });
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
            let accepted = (active.request.op == Op::Wake && completed)
                || correlated.is_some_and(|receipt| {
                    active.request.op == Op::Rest
                        && receipt.phase == Phase::Resting
                        && receipt.request.as_ref().is_some_and(|request| {
                            request.op == Op::Rest
                                && request.stamp.sequence >= active.request.stamp.sequence
                        })
                });
            // A terminal tombstone retains its last accepted operation.
            // A newer renewal cannot conceal cancellation/completion.
            let terminal = correlated.is_some_and(|receipt| {
                matches!(
                    receipt.phase,
                    Phase::Completed | Phase::Cancelled | Phase::Refused
                )
            });
            let expired = active
                .waiting
                .as_ref()
                .is_some_and(|ask| now >= ask.expires_us)
                || now.saturating_sub(active.began_us) >= 86_400_000_000
                || moment.is_none_or(|moment| !active.during.contains(moment.minute))
                || target.as_ref() != Some(&active.request.expression_session_id);
            // A completion from an old owner/window is no authority for
            // following sound, even if that receipt itself is still fresh.
            if expired {
                cancel(&mut self.publisher, source, now);
                continue;
            }
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
            if now.saturating_sub(active.request.stamp.at_us) >= RENEW_US {
                // A missing receipt cannot extend an unobserved owner forever.
                if receipt.as_ref().is_none_or(|receipt| {
                    receipt.expression_session_id != active.request.expression_session_id
                }) || send(
                    &self.clock_id,
                    &mut self.publisher,
                    &mut active.request,
                    now,
                )
                .is_err()
                {
                    cancel(&mut self.publisher, source, now);
                }
            }
        }
        while let Ok(ask) = self.asks.try_recv() {
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
            let Some(target) = &target else {
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
                    during: ask.rest.during.expect("checked rest"),
                    began_us: now,
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
            if send(
                &self.clock_id,
                &mut self.publisher,
                &mut active.request,
                now,
            )
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
) -> io::Result<()> {
    request.stamp = ClockStamp {
        clock_id: clock_id.into(),
        at_us: now,
        sequence: publisher.totals().published + 1,
    };
    request.valid_until_us = now.saturating_add(3_000_000);
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
        let _ = send(&clock_id, publisher, &mut active.request, now);
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
            manager.reload(vec!["routine".into()], 10_000_000);
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
            self.heartbeat();
            self.manager
                .tick(self.now, Some(&LocalTime::Offset(0)), wall);
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
        fn admitted(&mut self, phase: Phase) {
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
                    },
                )
                .unwrap();
            self.tick(NIGHT);
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
                },
            )
            .unwrap();
        f.tick(NIGHT);
        assert!(f.manager.sources["routine"].active.is_none());
    }
    #[test]
    fn changed_expression_or_unknown_clock_does_not_rebind_rest() {
        for unknown_clock in [false, true] {
            let mut f = Fixture::new();
            let rest = f.ask(Op::Rest);
            f.tick(NIGHT);
            f.admitted(Phase::Resting);
            assert_eq!(rest.try_recv().unwrap(), Ok(()));
            if !unknown_clock {
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
            f.now += 1;
            f.tick(if unknown_clock { 0 } else { NIGHT });
            assert!(f.manager.sources["routine"].active.is_none());
        }
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
