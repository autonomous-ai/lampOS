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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Owner {
    pub session: String,
    pub turn: u64,
    pub request: u64,
}

pub(super) struct Gate {
    pub eligible: bool,
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
}

impl Default for Rest {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            active: None,
            request: None,
            phase: Phase::Idle,
            reason: None,
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

    pub fn sync(&mut self, now: u64, gate: &Gate, life: &mut Option<Life>) {
        if self.active.as_ref().is_some_and(|active| {
            now >= active.request.valid_until_us
                || !gate.eligible
                || gate.owner.as_ref() != Some(&active.owner)
                || life.as_ref().is_none_or(|life| {
                    !life.routine_owned() || !life.routine_allowed(now, &gate.context)
                })
        }) {
            self.cancel(now, life);
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
                self.cancel(now, life);
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
        if !newer && current.is_none() {
            return Err(Error("rest episode was already spent".into()));
        }
        if newer
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
            if current.is_some() {
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
        if !gate.eligible || gate.owner.is_none() || (self.active.is_some() && current.is_none()) {
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
                .is_some_and(|life| life.request_routine_rest(now, &gate.context)),
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
        self.active = Some(Active {
            request: request.clone(),
            owner: gate.owner.clone().expect("checked owner"),
            phase,
        });
        self.request = Some(request);
        self.phase = phase;
        self.reason = None;
        Ok(())
    }
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
}
