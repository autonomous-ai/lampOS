//! The opt-in, shadow-only two-judge path. One slow request may be out; a
//! local decision never waits for its completion after its original deadline.
//! Deadline admission uses both worker completion and loop receipt time.
use super::{
    Ask, Context, Diagnostic, Faces, Level, NAME, Note, Policy, Room, SHADOW, Utterance, Waiting,
    Watch, attention, baseline, judge_log, local, millis, unix_ms,
};
use crate::{
    attention::{Answers, Choice, Judgement},
    decide::{Asked, Decider, Failed},
};
use lamp_msgs::Decision;
use serde::Serialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, VecDeque},
    fmt, io,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// These options are used only by `run_with_local`, never by the old entry
/// point. The decision budget is independent of the slow request timeout.
#[derive(Clone, Debug)]
pub struct Options {
    pub weights: local::Weights,
    pub deadline: Duration,
    pub local_deadline: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            weights: local::Weights::seed(),
            deadline: Duration::from_millis(100),
            local_deadline: Duration::from_millis(20),
        }
    }
}

impl Options {
    pub fn validate(&self) -> io::Result<()> {
        self.weights.validate()?;
        if !(Duration::from_millis(1)..=Duration::from_secs(1)).contains(&self.deadline)
            || !(Duration::from_millis(1)..=Duration::from_millis(100))
                .contains(&self.local_deadline)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "judge decision budget must be 1..1000 ms and local budget 1..100 ms",
            ));
        }
        Ok(())
    }
}

/// Numbers and built-in labels only: safe to print without printing speech,
/// a service response, a model-supplied name, a file path or a credential.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Notice {
    pub number: u64,
    pub judge: &'static str,
    pub stage: &'static str,
    pub answers: [Option<f32>; 7],
    pub probability: Option<f32>,
    pub choice: Option<Choice>,
    pub used: bool,
    pub took_us: u64,
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "judge decision={} judge={} stage={} used={} took_us={}",
            self.number, self.judge, self.stage, self.used, self.took_us
        )?;
        for (name, probability) in local::QUESTION_NAMES.iter().zip(self.answers) {
            if let Some(probability) = probability {
                write!(f, " {name}={probability:.6}")?;
            }
        }
        if let Some(probability) = self.probability {
            write!(f, " probability={probability:.6}")?;
        }
        if let Some(choice) = self.choice {
            write!(f, " choice={}", choice.as_str())?;
        }
        Ok(())
    }
}

/// The probability remains the model's estimate even when an operational
/// guard changes the choice. `from_rule` means neither model contributed;
/// the separate guard field is the evidence for any override of thresholds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Combined {
    pub judgement: Judgement,
    pub guard: Option<&'static str>,
    pub by: &'static str,
}

pub fn combine(
    weights: &local::Weights,
    cues: &local::Cues,
    local: Option<Judgement>,
    slow: Option<Judgement>,
) -> Combined {
    let (probability, by) = match (local, slow) {
        (Some(local), Some(slow)) => (
            weights.local_share() * local.probability
                + (1.0 - weights.local_share()) * slow.probability,
            "local+jev",
        ),
        (Some(local), None) => (local.probability, "local"),
        (None, Some(slow)) => (slow.probability, "jev"),
        (None, None) => (0.5, "rule"),
    };
    let called = cues.conversation == Some(true)
        || cues.text_called == Some(true)
        || (cues.facing == Some(true) && cues.face_age_ms.is_some_and(|age| age < 2000))
        || baseline::sure_name(cues.name_score)
        || (cues.room != Some(local::RoomCue::Busy)
            && cues.name_score.is_some_and(|score| score >= 0.9));
    let guard = (slow.is_none() && !called).then_some("no_timely_slow_no_call");
    Combined {
        judgement: Judgement {
            probability,
            choice: if guard.is_some() {
                Choice::LookUp
            } else {
                Choice::at(probability)
            },
            from_rule: local.is_none() && slow.is_none(),
        },
        guard,
        by,
    }
}

#[derive(Clone, Debug, Serialize)]
struct Answered {
    status: &'static str,
    model: Option<String>,
    answers: BTreeMap<String, f32>,
    judgement: Option<Judgement>,
    error: Option<&'static str>,
    took_us: u64,
}

impl Answered {
    fn absent(status: &'static str) -> Self {
        Self {
            status,
            model: None,
            answers: BTreeMap::new(),
            judgement: None,
            error: None,
            took_us: 0,
        }
    }

    fn of(
        result: Result<Asked, Failed>,
        took: Duration,
        judge: impl FnOnce(&Answers) -> Judgement,
    ) -> Self {
        let mut row = Self::absent("failed");
        row.took_us = micros(took);
        match result {
            Ok(asked) => {
                // No arbitrary service-sized string survives in a pending job.
                row.model = Some(asked.model.chars().take(128).collect());
                match Answers::of(&asked.answers) {
                    Ok(answers) => {
                        row.status = "answered";
                        row.judgement = Some(judge(&answers));
                        row.answers = answers.by_name();
                    }
                    Err(_) => row.error = Some("missing_or_invalid_question"),
                }
            }
            Err(error) => row.error = Some(error.kind()),
        }
        row
    }

    fn notice(&self, number: u64, judge: &'static str, stage: &'static str, used: bool) -> Notice {
        Notice {
            number,
            judge,
            stage,
            used,
            took_us: self.took_us,
            answers: local::QUESTION_NAMES.map(|name| self.answers.get(name).copied()),
            probability: self.judgement.map(|j| j.probability),
            choice: self.judgement.map(|j| j.choice),
        }
    }
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

struct Job {
    number: u64,
    waiting: Waiting,
    local: Answered,
}

/// Kept after the final choice only to audit its eventual slow reply. There
/// is at most one; its bounded transcript cannot be confused with a new job.
struct Flight {
    number: u64,
    context: Context,
    started: Instant,
    cutoff: Instant,
    job: Option<Job>,
    closed: Option<&'static str>,
}

impl Flight {
    fn timely(&self, completed: Instant, observed: Instant) -> bool {
        self.job.is_some() && completed < self.cutoff && observed < self.cutoff
    }
}

#[derive(Default)]
struct Tracker {
    voice: VecDeque<(Instant, bool)>,
    faces: VecDeque<(Instant, Option<bool>)>,
}

fn retain<T>(history: &mut VecDeque<(Instant, T)>, at: Instant, value: T) {
    if history.len() == 32 {
        history.pop_front();
    }
    history.push_back((at, value));
}

impl Tracker {
    /// The newest face before `ended`, while it is `seen_for` old at most.
    fn face(&self, ended: Instant, seen_for: Duration) -> (Option<bool>, Option<u32>) {
        match self.faces.iter().rev().find(|(at, _)| *at <= ended) {
            Some((at, facing)) if ended.duration_since(*at) <= seen_for => {
                (*facing, Some(millis(ended.duration_since(*at))))
            }
            _ => (None, None),
        }
    }

    fn cues(&self, context: &Context, utterance: &Utterance, seen_for: Duration) -> local::Cues {
        let known_voice = self
            .voice
            .iter()
            .rev()
            .find(|(at, _)| *at <= utterance.began)
            .is_some_and(|(_, known)| *known);
        let (facing, face_age_ms) = self.face(utterance.ended, seen_for);
        local::Cues {
            name_score: context
                .name_heard
                .filter(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            conversation: known_voice.then(|| context.in_conversation()),
            room: match context.room {
                Room::Quiet => Some(local::RoomCue::Quiet),
                Room::Busy => Some(local::RoomCue::Busy),
                Room::Unknown => None,
            },
            facing,
            face_age_ms,
            since_lamp_spoke_ms: context
                .seconds_since_lamp_spoke
                .filter(|s| s.is_finite() && *s >= 0.0)
                .map(|s| (f64::from(s) * 1000.0).min(f64::from(u32::MAX)) as u32),
            word_count: (!utterance.cut).then(|| {
                context
                    .heard
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| !w.is_empty())
                    .count() as u32
            }),
            // ASR receipt timestamps are not the duration of the sound.
            duration_ms: None,
            text_called: Some(baseline::called(&context.heard)),
            ..local::Cues::default()
        }
    }
}

pub(super) struct Judges {
    local: local::Local,
    deadline: Duration,
    local_deadline: Duration,
    tracker: Tracker,
    flight: Option<Flight>,
    run: String,
    number: u64,
    local_failures: u64,
    slow_failures: u64,
    late: u64,
    busy: u64,
    local_failed: bool,
    last_failed: bool,
    comparisons: baseline::Counts,
}

impl Judges {
    pub(super) fn new(options: Options) -> Self {
        static RUNS: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let run = format!(
            "{nanos}-{}-{}",
            std::process::id(),
            RUNS.fetch_add(1, Ordering::Relaxed)
        );
        Self {
            local: local::Local::new(options.weights),
            deadline: options.deadline,
            local_deadline: options.local_deadline,
            tracker: Tracker::default(),
            flight: None,
            run,
            number: 0,
            local_failures: 0,
            slow_failures: 0,
            late: 0,
            busy: 0,
            local_failed: false,
            last_failed: false,
            comparisons: baseline::Counts::default(),
        }
    }

    pub(super) fn voice_seen(&mut self, at: Instant) {
        retain(&mut self.tracker.voice, at, true);
    }
    pub(super) fn voice_gone(&mut self, at: Instant) {
        retain(&mut self.tracker.voice, at, false);
    }
    pub(super) fn eyes_gone(&mut self, at: Instant) {
        retain(&mut self.tracker.faces, at, None);
    }

    pub(super) fn faces(&mut self, faces: &Faces, at: Instant) {
        let mut facing = None;
        let mut unknown = false;
        for face in &faces.faces {
            if face.check().is_err() {
                unknown = true;
                continue;
            }
            if face.ymax - face.ymin < super::NEAR_HEIGHT {
                continue;
            }
            match face.facing {
                Some(true) => {
                    facing = Some(true);
                    break;
                }
                Some(false) => facing = Some(false),
                None => unknown = true,
            }
        }
        if unknown && facing != Some(true) {
            facing = None;
        }
        retain(&mut self.tracker.faces, at, facing);
    }

    pub(super) fn cues(
        &self,
        context: &Context,
        utterance: &Utterance,
        seen_for: Duration,
    ) -> local::Cues {
        self.tracker.cues(context, utterance, seen_for)
    }

    pub(super) fn wait(&self, now: Instant) -> Duration {
        match self.flight.as_ref().filter(|flight| flight.job.is_some()) {
            Some(flight) => {
                super::TICK_WHILE_ASKING.min(flight.cutoff.saturating_duration_since(now))
            }
            None => super::TICK,
        }
    }

    pub(super) fn record_model(&self, policy: &mut Policy<'_>) {
        let at = unix_ms();
        self.log(
            policy,
            at,
            judge_log::Kind::Models,
            &json!({
                "schema": 1, "event": "model", "at_unix_ms": at, "run": self.run,
                "shadow": SHADOW, "weights": self.local.weights(),
                "questions": local::QUESTION_NAMES, "slow_requested": policy.judge_name,
                "decision_budget_us": micros(self.deadline),
                "local_budget_us": micros(self.local_deadline),
                "baseline_source": baseline::SOURCE,
            }),
        );
    }

    fn log<T: Serialize>(
        &self,
        policy: &mut Policy<'_>,
        at: u64,
        kind: judge_log::Kind,
        value: &T,
    ) -> bool {
        policy
            .judge_log
            .as_mut()
            .is_some_and(|writer| writer.write(kind, at, value))
    }

    fn answer_log(
        &self,
        policy: &mut Policy<'_>,
        flight: &Flight,
        row: &Answered,
        times: (Instant, Instant),
        used: bool,
        phase: &'static str,
    ) {
        let (observed, completed) = times;
        let at = unix_ms();
        self.log(
            policy,
            at,
            judge_log::Kind::Judges,
            &json!({
                "schema": 1, "event": "slow", "at_unix_ms": at, "run": self.run,
                "number": flight.number, "answer": row, "phase": phase, "used": used,
                "after_final": flight.job.is_none(), "closed": flight.closed,
                "completed_us": micros(completed.saturating_duration_since(flight.started)),
                "observed_us": micros(observed.saturating_duration_since(flight.started)),
                "decision_budget_us": micros(self.deadline),
            }),
        );
        (policy.say)(Note::Judge(row.notice(flight.number, "slow", phase, used)));
    }

    pub(super) fn tick(&mut self, policy: &mut Policy<'_>, _now: Instant) {
        match policy.asker.answers.try_recv() {
            Ok((result, took, completed)) => {
                let observed = Instant::now();
                if let Some(mut flight) = self.flight.take() {
                    let timely = flight.timely(completed, observed);
                    let row =
                        Answered::of(result, took, |a| attention::combine(a, &flight.context));
                    self.last_failed = row.judgement.is_none();
                    self.slow_failures += u64::from(self.last_failed);
                    self.late += u64::from(!timely);
                    self.answer_log(
                        policy,
                        &flight,
                        &row,
                        (observed, completed),
                        timely && row.judgement.is_some(),
                        if timely { "timely" } else { "late_or_closed" },
                    );
                    if let Some(job) = flight.job.take() {
                        if timely {
                            self.finalize(policy, job, &row, "slow_returned");
                        } else {
                            self.finalize(policy, job, &Answered::absent("late"), "deadline");
                        }
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                if !std::mem::replace(&mut policy.asker_gone, true) {
                    (policy.say)(Note::SlowGone);
                }
                if let Some(mut flight) = self.flight.take() {
                    let now = Instant::now();
                    let row = Answered::of(
                        Err(Policy::no_asker()),
                        now.saturating_duration_since(flight.started),
                        |a| attention::combine(a, &flight.context),
                    );
                    self.slow_failures += 1;
                    self.last_failed = true;
                    self.answer_log(policy, &flight, &row, (now, now), false, "worker_gone");
                    if let Some(job) = flight.job.take() {
                        self.finalize(policy, job, &row, "worker_gone");
                    }
                }
            }
        }
        if self
            .flight
            .as_ref()
            .is_some_and(|f| f.job.is_some() && Instant::now() >= f.cutoff)
        {
            self.close_pending(policy, "deadline");
        }
        while let Some(waiting) = policy.waiting.pop_front() {
            // Preserve utterance order: an older pending choice must not be
            // published after the newer local-only choice overtakes it.
            self.close_pending(policy, "superseded");
            self.number += 1;
            let number = self.number;
            let cues = waiting.local.as_ref().cloned().unwrap_or_default();
            let began = Instant::now();
            let result = serde_json::to_string(&cues)
                .map_err(|_| Failed::Unreadable("local cues"))
                .and_then(|state| {
                    self.local
                        .ask(&state, &attention::questions(), self.local_deadline)
                });
            let took = began.elapsed();
            let result = if took > self.local_deadline {
                Err(Failed::TimedOut)
            } else {
                result
            };
            let local = Answered::of(result, took, |answers| self.local.weights().judge(answers));
            self.local_failed = local.judgement.is_none();
            self.local_failures += u64::from(self.local_failed);
            let at = unix_ms();
            self.log(
                policy,
                at,
                judge_log::Kind::Judges,
                &json!({
                    "schema": 1, "event": "local", "at_unix_ms": at, "run": self.run,
                    "number": number, "cues": cues, "answer": local,
                    "eligible_to_answer_us": micros(waiting.eligible.elapsed()),
                }),
            );
            (policy.say)(Note::Judge(local.notice(
                number,
                "local",
                "immediate",
                false,
            )));
            let cutoff = waiting.eligible + self.deadline;
            let job = Job {
                number,
                waiting,
                local,
            };
            if self.flight.is_some() || policy.asker_gone || Instant::now() >= cutoff {
                let reason = if self.flight.is_some() {
                    self.busy += 1;
                    "remote_busy"
                } else if policy.asker_gone {
                    "worker_gone"
                } else {
                    "deadline_before_remote"
                };
                self.finalize(policy, job, &Answered::absent(reason), reason);
                continue;
            }
            // The original byte-pinned Jev state and seven questions are
            // unchanged. Local cues are never substituted for that prompt.
            let state = attention::state_text(&job.waiting.context);
            match policy.asker.states.try_send(Ask::About(state)) {
                Ok(()) => {
                    self.flight = Some(Flight {
                        number,
                        context: job.waiting.context.clone(),
                        started: job.waiting.eligible,
                        cutoff,
                        job: Some(job),
                        closed: None,
                    })
                }
                Err(error) => {
                    let reason = match error {
                        mpsc::TrySendError::Full(_) => {
                            self.busy += 1;
                            "remote_busy"
                        }
                        mpsc::TrySendError::Disconnected(_) => {
                            if !std::mem::replace(&mut policy.asker_gone, true) {
                                (policy.say)(Note::SlowGone);
                            }
                            "worker_gone"
                        }
                    };
                    self.finalize(policy, job, &Answered::absent(reason), reason);
                }
            }
        }
        if policy.waiting.is_empty()
            && let Some(dropped) = policy.dropping.right()
        {
            (policy.say)(Note::CaughtUp { dropped });
        }
    }

    fn close_pending(&mut self, policy: &mut Policy<'_>, why: &'static str) {
        if let Some(mut flight) = self.flight.take() {
            if let Some(job) = flight.job.take() {
                self.finalize(policy, job, &Answered::absent("pending"), why);
                flight.closed = Some(why);
            }
            self.flight = Some(flight);
        }
    }

    fn finalize(
        &mut self,
        policy: &mut Policy<'_>,
        job: Job,
        slow: &Answered,
        reason: &'static str,
    ) {
        let Job {
            number,
            waiting,
            local,
        } = job;
        let cues = waiting.local.as_ref().cloned().unwrap_or_default();
        let result = combine(self.local.weights(), &cues, local.judgement, slow.judgement);
        let judgement = result.judgement;
        let mut answers = BTreeMap::new();
        for name in local::QUESTION_NAMES {
            let answer = match (local.answers.get(name), slow.answers.get(name)) {
                (Some(local), Some(slow)) => {
                    self.local.weights().local_share() * local
                        + (1.0 - self.local.weights().local_share()) * slow
                }
                (Some(answer), None) | (None, Some(answer)) => *answer,
                (None, None) => continue,
            };
            answers.insert(name.to_owned(), answer);
        }
        if let Some((watch, previous)) = policy.watch.take() {
            policy.close(&watch, previous, true);
        }
        policy.totals.decided += 1;
        policy.totals.by_rule += u64::from(judgement.from_rule);
        let took = waiting.eligible.elapsed();
        let took_ms = millis(took);
        let waited_ms = millis(
            waiting
                .eligible
                .saturating_duration_since(waiting.utterance.ended),
        );
        policy.publish(&Decision {
            about: attention::ABOUT.to_owned(),
            choice: judgement.choice.as_str().to_owned(),
            probability: judgement.probability,
            answers,
            by: result.by.to_owned(),
            took_ms,
            shadow: SHADOW,
            request: waiting.utterance.request,
            task: None,
            arrival: None,
            ask: None,
        });
        let at = unix_ms();
        policy.watch = Some((
            Watch::new(
                at,
                judgement.choice,
                Instant::now(),
                waiting.utterance.sent,
                &policy.begun,
            ),
            number,
        ));
        let snapshot = baseline::Snapshot {
            kind: baseline::SnapshotKind::SameSnapshot,
            room: match waiting.context.room {
                Room::Quiet => baseline::Room::Quiet,
                Room::Busy => baseline::Room::Busy,
                Room::Unknown => baseline::Room::Unknown,
            },
            own_turn: false,
            going_on: waiting.context.in_conversation(),
            named_sure: baseline::sure_name(cues.name_score),
            text: waiting.context.heard.clone(),
            facing: cues.facing,
        };
        self.comparisons
            .note(Some(&snapshot), judgement.choice, None);
        if !self.log(policy, at, judge_log::Kind::Judges, &json!({
            "schema": 1, "event": "choice", "at_unix_ms": at, "run": self.run,
            "number": number, "shadow": SHADOW, "request": waiting.utterance.request,
            "context": waiting.context, "cues": cues, "seen": waiting.seen,
                "local": local, "slow": slow, "result": result, "reason": reason,
                "local_used": local.judgement.is_some(), "slow_used": slow.judgement.is_some(),
            "decision_budget_us": micros(self.deadline), "eligible_to_choice_us": micros(took),
            "last_piece_to_eligible_us": micros(waiting.eligible.saturating_duration_since(waiting.utterance.ended)),
            "baseline": snapshot, "baseline_permission": baseline::judge(&snapshot),
            "pieces": waiting.utterance.pieces, "cut": waiting.utterance.cut,
            "association": "policy_receipt_times_not_capture_or_speaker_identity",
            "bus_answers": "share_blend_for_inspection_not_choice_input",
        })) { policy.totals.unlogged += 1; }
        (policy.say)(Note::Judge(Notice {
            number,
            judge: "policy",
            stage: reason,
            answers: [None; 7],
            probability: Some(judgement.probability),
            choice: Some(judgement.choice),
            used: true,
            took_us: micros(took),
        }));
        (policy.say)(Note::Decided {
            number,
            choice: judgement.choice,
            probability: judgement.probability,
            by_rule: judgement.from_rule,
            took_ms,
            waited_ms,
            on: None,
            pieces: waiting.utterance.pieces,
            bytes: waiting.utterance.text.len() as u32,
        });
    }

    pub(super) fn report(&self, policy: &Policy<'_>) -> Diagnostic {
        let logs = policy
            .judge_log
            .as_ref()
            .map(|writer| writer.stats())
            .unwrap_or_default();
        let (level, message) = if logs.failed > 0 || logs.dropped > 0 || !logs.alive {
            (
                Level::Warn,
                "judge log incomplete; shadow decisions continue",
            )
        } else if self.local_failed {
            (
                Level::Warn,
                "local judge failed; timely slow result or look-up stand-in continues",
            )
        } else if policy.asker_gone || self.last_failed {
            (Level::Warn, "slow judge unavailable; local judge continues")
        } else {
            (Level::Ok, "shadow: two judges, one final choice")
        };
        Diagnostic::new(level, NAME, message)
            .with("shadow", SHADOW)
            .with("decided", policy.totals.decided)
            .with("local_failures", self.local_failures)
            .with("slow_failures", self.slow_failures)
            .with("late_or_closed", self.late)
            .with("remote_busy", self.busy)
            .with("log_queued", logs.queued)
            .with("log_written", logs.written)
            .with("log_failed", logs.failed)
            .with("log_dropped", logs.dropped)
            .with("log_pending", logs.pending)
    }

    pub(super) fn finish(&mut self, policy: &mut Policy<'_>) {
        self.close_pending(policy, "shutdown");
        let at = unix_ms();
        self.log(
            policy,
            at,
            judge_log::Kind::Judges,
            &json!({
                "schema": 1, "event": "shutdown", "at_unix_ms": at, "run": self.run,
                "decided": policy.totals.decided, "comparisons": self.comparisons,
                "slow_unobserved": self.flight.as_ref().map(|f| f.number),
                "open_outcome": policy.watch.as_ref().map(|(_, number)| number),
                "local_failures": self.local_failures, "slow_failures": self.slow_failures,
                "late_or_closed": self.late, "remote_busy": self.busy,
            }),
        );
    }
}

#[cfg(test)]
mod tests;
