//! Bounded routine ownership, independent of transport and wall time.
use crate::Error;
use crate::alive::{Context, Life};
use lamp_msgs::{
    EXPRESSION_REST_FRESH_US, ExpressionRestOp as Op, ExpressionRestPhase as Phase,
    ExpressionRestRequest as Request,
};

/// Never evict a tombstone: a full planner refuses new publishers until it
/// restarts with a new target session, which fences every old request.
const MAX_SOURCES: usize = 32;

/// How long the night's sleep is held after its lease lapses (#940): long
/// enough for lamp-skills to restart, so a new session's first rest, or a
/// newer episode, takes it over with the head still down.
pub(crate) const SLEEP_HOLD_US: u64 = 10_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Owner {
    pub session: String,
    pub turn: u64,
    pub request: u64,
}

pub(super) struct Gate {
    pub eligible: bool,
    /// Why it is not eligible: the first shut gate.
    pub why_not: Option<&'static str>,
    /// Why not even the night's sleep may be held: Expression off or
    /// faulted. The built-in sleep stops only with them too.
    pub off: Option<&'static str>,
    /// The skill that claims the night's sleep (#940): its rest is held as
    /// the built-in sleep is, through what does not wake that sleep.
    pub sleep: Option<String>,
    pub owner: Option<Owner>,
    pub context: Context,
}

struct Source {
    skill: String,
    session: String,
    at: u64,
    sequence: u64,
    episode: u64,
    retired: bool,
}

#[derive(Clone)]
struct Active {
    request: Request,
    owner: Owner,
    phase: Phase,
}

pub(super) struct Rest {
    sources: Vec<Source>,
    active: Option<Active>,
    pub request: Option<Request>,
    pub phase: Phase,
    /// Why the last request was refused, kept only while `phase` is `Refused`.
    pub reason: Option<&'static str>,
    /// Why `sync` ended an active rest itself, until the journal takes it.
    pub ended: Option<&'static str>,
}

impl Default for Rest {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            active: None,
            request: None,
            phase: Phase::Idle,
            reason: None,
            ended: None,
        }
    }
}

impl Rest {
    fn end(&mut self, now: u64, phase: Phase, life: &mut Option<Life>) {
        if let Some(active) = self.active.take() {
            if let Some(life) = life {
                life.release_routine(now);
            }
            self.request = Some(active.request);
            self.phase = phase;
            self.reason = None;
        }
    }

    pub fn cancel(&mut self, now: u64, life: &mut Option<Life>) {
        self.end(now, Phase::Cancelled, life);
    }

    /// End the active rest, if any, saying why once, as `sync` does.
    pub fn withdraw(&mut self, now: u64, why: &'static str, life: &mut Option<Life>) {
        if self.active.is_some() {
            self.let_go(now, life);
            self.ended = Some(why);
        }
    }

    /// End the active rest; the night's sleep is kept in place by the
    /// built-in sleep when its conditions hold (#940), with no lift.
    fn let_go(&mut self, now: u64, life: &mut Option<Life>) {
        if life
            .as_mut()
            .is_some_and(|life| life.keep_routine_sleep(now))
        {
            if let Some(active) = self.active.take() {
                self.request = Some(active.request);
                self.phase = Phase::Cancelled;
                self.reason = None;
            }
        } else {
            self.cancel(now, life);
        }
    }

    pub fn sync(&mut self, now: u64, gate: &Gate, life: &mut Option<Life>) {
        let Some(active) = &self.active else {
            return;
        };
        let sleep = active.phase == Phase::Resting && sleeps(gate, &active.request);
        let lapsed = now >= active.request.valid_until_us;
        if sleep && now >= active.request.valid_until_us.saturating_add(SLEEP_HOLD_US) {
            // Nobody took the night's sleep over within the hold (#940):
            // the built-in sleep keeps it if it may, else it is released.
            self.let_go(now, life);
            self.ended = Some("its lease ran out and no session took it over");
            return;
        }
        let why = if lapsed && !sleep {
            Some("its lease ran out")
        } else if sleep {
            // The night's sleep: the voice listening, a privacy mute, the
            // body held or a new turn of the voice do not end it, as they
            // do not end the built-in sleep. A reflex does. Its lease
            // lapsed, it is held down for `SLEEP_HOLD_US`, for a taker.
            gate.off.or(match life.as_ref() {
                None => Some("there is no life planner"),
                Some(life) if !life.routine_owned() => Some(
                    "a stronger reflex took it over: a name, a touch, a reminder or a conversation",
                ),
                Some(_) => None,
            })
        } else if !gate.eligible {
            Some(gate.why_not.unwrap_or("its gate is shut"))
        } else if gate.owner.as_ref() != Some(&active.owner) {
            Some("the voice began another turn")
        } else {
            match life.as_ref() {
                None => Some("there is no life planner"),
                Some(life) if !life.routine_owned() => Some(
                    "a stronger reflex took it over: a name, a touch, a reminder or a conversation",
                ),
                Some(life) if !life.routine_allowed(now, &gate.context) => {
                    Some("rest is no longer allowed: a hand, a lean, a look up or manual sleep")
                }
                Some(_) => None,
            }
        };
        if let Some(why) = why {
            self.cancel(now, life);
            // Said once, by the runtime: a cancel the skill then sends for
            // this episode is a no-op, not a refusal (#1672).
            self.ended = Some(why);
        }
    }

    pub fn after_tick(&mut self, now: u64, life: &mut Option<Life>) {
        if self.active.as_ref().is_some_and(|active| {
            active.phase == Phase::Waking && life.as_ref().is_some_and(|life| !life.routine_owned())
        }) {
            self.end(now, Phase::Completed, life);
        }
    }

    fn refuse(&mut self, request: Request, why: &'static str) -> Result<(), Error> {
        self.request = Some(request);
        self.phase = Phase::Refused;
        self.reason = Some(why);
        Err(Error(why.into()))
    }

    pub fn offer(
        &mut self,
        now: u64,
        request: Request,
        gate: &Gate,
        life: &mut Option<Life>,
        lost_input: bool,
    ) -> Result<(), Error> {
        self.sync(now, gate, life);
        request.check()?;
        if lost_input {
            // The shared publisher may have lost another skill's cancel.
            self.cancel(now, life);
        }
        let index = if let Some(index) = self.sources.iter().position(|source| {
            source.skill == request.skill && source.session == request.skill_session_id
        }) {
            index
        } else {
            if self.sources.len() == MAX_SOURCES {
                return Err(Error(
                    "rest publisher history is full; restart Expression".into(),
                ));
            }
            let known_session = self
                .sources
                .iter()
                .any(|source| source.session == request.skill_session_id);
            let late = !known_session
                && self
                    .sources
                    .iter()
                    .any(|source| request.stamp.at_us <= source.at);
            if !known_session && !late {
                for source in &mut self.sources {
                    source.retired = true;
                }
                // A new session's first rest of the claimed skill takes its
                // night's sleep over below, with no release (#940).
                if !takes_over(gate, &request, self.active.as_ref()) {
                    self.cancel(now, life);
                }
            }
            let retired = late
                || self
                    .sources
                    .iter()
                    .any(|source| source.session == request.skill_session_id && source.retired);
            self.sources.push(Source {
                skill: request.skill.clone(),
                session: request.skill_session_id.clone(),
                at: 0,
                sequence: 0,
                episode: 0,
                retired,
            });
            self.sources.len() - 1
        };
        let source = &mut self.sources[index];
        if source.retired
            || request.stamp.sequence <= source.sequence
            || request.stamp.at_us < source.at
        {
            return Err(Error(
                "retired, duplicate or out-of-order rest publisher".into(),
            ));
        }
        source.at = request.stamp.at_us;
        source.sequence = request.stamp.sequence;
        if request.episode < source.episode {
            return Err(Error("rest episode is older than its tombstone".into()));
        }
        let newer = request.episode > source.episode;
        source.episode = request.episode;
        let current = self
            .active
            .as_ref()
            .filter(|active| {
                active.request.skill == request.skill
                    && active.request.skill_session_id == request.skill_session_id
                    && active.request.episode == request.episode
            })
            .cloned();
        // The night's sleep handed over (#940): a newer episode, or a new
        // session's first rest, of the skill that claims the night takes
        // the active rest as its own, still down: no release, no lift.
        let handover =
            newer && current.is_none() && takes_over(gate, &request, self.active.as_ref());
        let current = current.or_else(|| self.active.clone().filter(|_| handover));
        if !newer && current.is_none() {
            // Expression may have ended the episode first (the voice heard
            // a sound): the skill's cancel that follows asks for what is
            // already so. Nothing is resumed or replayed (#1672).
            // Another skill's active rest keeps its receipt.
            if request.op == Op::Cancel && !lost_input {
                if self.active.is_none() {
                    self.request = Some(request);
                    self.phase = Phase::Cancelled;
                    self.reason = None;
                }
                return Ok(());
            }
            return Err(Error("rest episode was already spent".into()));
        }
        if newer
            && !handover
            && self.active.as_ref().is_some_and(|active| {
                active.request.skill == request.skill
                    && active.request.skill_session_id == request.skill_session_id
            })
        {
            self.cancel(now, life);
        }
        // A missing operation could have cancelled any skill's current rest:
        // all skills share one publisher. Spend this episode and release the
        // active owner before any renewal can hide the missing cancellation.
        if lost_input {
            return self.refuse(request, "rest input lost messages; episode is spent");
        }
        if request.op == Op::Rest
            && request.stamp.at_us <= now
            && now - request.stamp.at_us >= EXPRESSION_REST_FRESH_US
            && now < request.valid_until_us
            && current
                .as_ref()
                .is_some_and(|active| active.phase == Phase::Resting)
        {
            // sync already checked the original lease and current owner. A
            // late renewal cannot extend it, but need not cancel that still
            // valid rest. Keep its exact request and receipt; the ordering
            // mark above still consumes this observation against replay.
            return Err(Error(
                "stale rest renewal ignored; the existing lease is unchanged".into(),
            ));
        }
        if request.stamp.at_us > now
            || now - request.stamp.at_us >= EXPRESSION_REST_FRESH_US
            || now >= request.valid_until_us
        {
            if current.is_some() && !handover {
                self.cancel(now, life);
            }
            return self.refuse(request, "rest observation is future, stale or expired");
        }
        if request.op == Op::Cancel {
            if current.is_some() {
                self.cancel(now, life);
            }
            self.request = Some(request);
            self.phase = Phase::Cancelled;
            self.reason = None;
            return Ok(());
        }
        let sleep = sleeps(gate, &request);
        let renewed = current
            .as_ref()
            .is_some_and(|active| active.phase == Phase::Resting);
        let admitted = if sleep {
            gate.off.is_none() && (gate.owner.is_some() || renewed)
        } else {
            gate.eligible && gate.owner.is_some()
        };
        if !admitted || (self.active.is_some() && current.is_none()) {
            if current.is_some() {
                self.cancel(now, life);
            }
            return self.refuse(
                request,
                "rest owner is unavailable or another routine owns it",
            );
        }
        let allowed = match (current.as_ref().map(|active| active.phase), request.op) {
            (None, Op::Rest) | (Some(Phase::Resting), Op::Rest) => life
                .as_mut()
                .is_some_and(|life| life.request_routine_rest(now, &gate.context, sleep)),
            (Some(Phase::Resting), Op::Wake) => life
                .as_mut()
                .is_some_and(|life| life.wake_routine(now, &gate.context)),
            (Some(Phase::Waking), Op::Wake) => life.as_ref().is_some_and(Life::routine_owned),
            _ => false,
        };
        if !allowed {
            if current.is_some() {
                self.cancel(now, life);
            }
            return self.refuse(
                request,
                "rest transition is unavailable or would replay an episode",
            );
        }
        let phase = if request.op == Op::Rest {
            Phase::Resting
        } else {
            Phase::Waking
        };
        // The night's sleep renewed while the voice is stale keeps the
        // owner it began with: only a generic rest is bound to it.
        let owner = gate
            .owner
            .clone()
            .or_else(|| current.map(|active| active.owner))
            .expect("checked owner");
        self.active = Some(Active {
            request: request.clone(),
            owner,
            phase,
        });
        self.request = Some(request);
        self.phase = phase;
        self.reason = None;
        Ok(())
    }
}

/// Whether `request` is from the skill that claims the night's sleep.
fn sleeps(gate: &Gate, request: &Request) -> bool {
    gate.sleep.as_deref() == Some(request.skill.as_str())
}

/// Whether `request` may take `active` over without a release (#940): a
/// rest of the skill that claims the night, while that skill's rest is the
/// night's sleep, still down.
fn takes_over(gate: &Gate, request: &Request, active: Option<&Active>) -> bool {
    request.op == Op::Rest
        && sleeps(gate, request)
        && active.is_some_and(|active| {
            active.phase == Phase::Resting && active.request.skill == request.skill
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Catalogue, alive::Scheduling};
    use lamp_msgs::{ClockStamp, Voice};

    #[test]
    fn a_rest_transport_gap_spends_the_active_episode_before_a_renewal() {
        for age in [0, EXPRESSION_REST_FRESH_US] {
            let catalogue = Catalogue::from_json(include_str!("../alive-behaviours.json")).unwrap();
            let mut life = Life::new(catalogue.alive.unwrap(), 2_000_000, 1_000_000);
            life.set_scheduling(Scheduling::Skills, 1_000_000);
            let mut life = Some(life);
            let gate = Gate {
                eligible: true,
                why_not: None,
                off: None,
                sleep: None,
                owner: Some(Owner {
                    session: "voice".into(),
                    turn: 1,
                    request: 1,
                }),
                context: Context {
                    voice: Some(Voice::Resting),
                    free: true,
                },
            };
            let mut rest = Rest::default();
            let mut request = Request {
                stamp: ClockStamp {
                    clock_id: "host".into(),
                    at_us: 1_000_000,
                    sequence: 1,
                },
                skill: "routine".into(),
                skill_session_id: "source".into(),
                expression_session_id: "target".into(),
                episode: 1,
                op: Op::Rest,
                valid_until_us: 4_000_000,
            };
            rest.offer(1_000_000, request.clone(), &gate, &mut life, false)
                .unwrap();
            request.stamp.sequence = 3;
            assert!(
                rest.offer(1_000_000 + age, request.clone(), &gate, &mut life, true)
                    .is_err()
            );
            assert_eq!(rest.phase, Phase::Cancelled);
            assert_eq!(rest.reason, None);
            assert!(!life.as_ref().unwrap().routine_owned());
            request.stamp.sequence = 4;
            assert!(
                rest.offer(1_000_000 + age, request.clone(), &gate, &mut life, false)
                    .is_err()
            );
            assert_eq!(rest.phase, Phase::Cancelled);
            // A new episode offered with lost input is refused, and says so.
            request.episode = 2;
            request.stamp.sequence = 5;
            assert!(
                rest.offer(1_000_000 + age, request, &gate, &mut life, true)
                    .is_err()
            );
            assert_eq!(rest.phase, Phase::Refused);
            assert_eq!(
                rest.reason,
                Some("rest input lost messages; episode is spent")
            );
        }
    }

    fn resting_gate() -> Gate {
        Gate {
            eligible: true,
            why_not: None,
            off: None,
            sleep: None,
            owner: Some(Owner {
                session: "voice".into(),
                turn: 1,
                request: 1,
            }),
            context: Context {
                voice: Some(Voice::Resting),
                free: true,
            },
        }
    }

    /// Row 7 on lamp-06c1 (#1672): a sound ended the rest, and the skill's
    /// cancel of that spent episode was refused as "already spent".
    #[test]
    fn a_cancel_of_an_episode_a_sound_already_ended_is_a_no_op() {
        let catalogue = Catalogue::from_json(include_str!("../alive-behaviours.json")).unwrap();
        let mut life = Life::new(catalogue.alive.unwrap(), 2_000_000, 1_000_000);
        life.set_scheduling(Scheduling::Skills, 1_000_000);
        let mut life = Some(life);
        let mut rest = Rest::default();
        let mut request = Request {
            stamp: ClockStamp {
                clock_id: "host".into(),
                at_us: 1_000_000,
                sequence: 1,
            },
            skill: "routine".into(),
            skill_session_id: "source".into(),
            expression_session_id: "target".into(),
            episode: 1,
            op: Op::Rest,
            valid_until_us: 4_000_000,
        };
        rest.offer(
            1_000_000,
            request.clone(),
            &resting_gate(),
            &mut life,
            false,
        )
        .unwrap();
        assert_eq!(rest.phase, Phase::Resting);
        // The voice hears a sound: Expression ends the rest and says why.
        let mut listening = resting_gate();
        listening.eligible = false;
        listening.why_not = Some("the voice is no longer resting: it is listening");
        listening.context.voice = Some(Voice::Listening);
        rest.sync(1_100_000, &listening, &mut life);
        assert_eq!(rest.phase, Phase::Cancelled);
        assert_eq!(
            rest.ended.take(),
            Some("the voice is no longer resting: it is listening")
        );
        assert!(!life.as_ref().unwrap().routine_owned());
        // The skill's cancel of that episode asks for what is already so.
        request.op = Op::Cancel;
        request.stamp.at_us = 1_200_000;
        request.stamp.sequence = 2;
        request.valid_until_us = 4_200_000;
        assert!(
            rest.offer(1_200_000, request.clone(), &listening, &mut life, false)
                .is_ok()
        );
        assert_eq!(rest.phase, Phase::Cancelled);
        assert_eq!(rest.reason, None);
        assert_eq!(rest.ended, None, "said once, not again by the cancel");
        // It resumes nothing: a rest of the spent episode is still refused.
        request.op = Op::Rest;
        request.stamp.sequence = 3;
        assert!(
            rest.offer(1_200_000, request, &resting_gate(), &mut life, false)
                .is_err()
        );
        assert!(!life.as_ref().unwrap().routine_owned());
    }
}
