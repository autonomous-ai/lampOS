//! Chronological execution shared by live runs and explicitly unscored checks.
use crate::{
    Result,
    answering::{Answerer, MemoryMode},
    conversation::Conversation,
    corpus::{Corpus, Profile},
    diagnostics::Coverage,
    evaluation, hash, invalid,
    ledger::Mode,
    model_worker::Worker,
    protocol::*,
    runner::{Outcome, Runner},
    score::{Answer, JudgeOutput, Rule},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub struct Trial<'a> {
    pub corpus: &'a Corpus,
    pub observed: &'a [Event],
    pub arm: &'a str,
    pub mode: Mode,
    pub runner: &'a mut Runner,
    pub worker: &'a mut Worker,
}
struct Operations<'a, F> {
    runner: &'a mut Runner,
    worker: &'a mut Worker,
    log: F,
    next: u64,
}
impl<F: FnMut(&str, Value) -> Result<()>> Operations<'_, F> {
    fn run(&mut self, op: Op, data: Value) -> Result<Outcome> {
        let id = format!("operation-{}", self.next);
        self.next += 1;
        let request = Request::new(id, op, data);
        let outcome = self.runner.execute(request.clone(), self.worker)?;
        (self.log)("operation", json!({"request":request,"outcome":outcome}))?;
        Ok(outcome)
    }
    fn drain(&mut self, event: &Event, cadence: &str, recent: &mut Conversation) -> Result<()> {
        let mut cursor = None;
        for _ in 0..100 {
            let out = self.run(
                Op::Sleep,
                json!({"at_unix_ms":event.at_unix_ms,"cadence":cadence}),
            )?;
            if out.data.get("forget_effect").is_some() {
                recent.apply_effect(effect(&out.data)?)?;
            }
            if out.data["pending"] == false {
                return Ok(());
            }
            let next = out.data["next_cursor"]
                .as_str()
                .ok_or_else(|| invalid("sleep omitted cursor"))?
                .to_owned();
            if cursor.as_ref() == Some(&next) {
                return Err(invalid("sleep cursor stopped advancing"));
            }
            cursor = Some(next);
        }
        Err(invalid("sleep exceeded registered bounded drain"))
    }
}
fn effect(data: &Value) -> Result<ForgetEffect> {
    let effect: ForgetEffect = serde_json::from_value(
        data.get("forget_effect")
            .cloned()
            .ok_or_else(|| invalid("mutation omitted forget effect"))?,
    )?;
    effect.validate()?;
    Ok(effect)
}
fn combined(a: ForgetEffect, b: ForgetEffect) -> ForgetEffect {
    ForgetEffect {
        ran: a.ran || b.ran,
        suppress_confirmation: a.suppress_confirmation || b.suppress_confirmation,
    }
}
fn check_observed(corpus: &Corpus, observed: &[Event]) -> Result<()> {
    if corpus.events.len() != observed.len() {
        return Err(invalid(
            "observed stream length differs from registered script",
        ));
    }
    for (source, observed) in corpus.events.iter().zip(observed) {
        observed.validate()?;
        let mut structural = observed.clone();
        structural.heard = source.heard.clone();
        structural.stt_confidence = source.stt_confidence;
        if serde_json::to_value(&structural)? != serde_json::to_value(source)? {
            return Err(invalid("speech cache changed non-speech fixture structure"));
        }
    }
    Ok(())
}
impl Trial<'_> {
    /// Init and close are owned by the outer run, which pins executable and
    /// source identities and settles accounting before publishing completion.
    pub fn run(&mut self, log: impl FnMut(&str, Value) -> Result<()>) -> Result<Value> {
        crate::files::validate(self.corpus)?;
        check_observed(self.corpus, self.observed)?;
        self.walk(log)
    }
    fn walk(&mut self, log: impl FnMut(&str, Value) -> Result<()>) -> Result<Value> {
        let mut ops = Operations {
            runner: self.runner,
            worker: self.worker,
            log,
            next: 0,
        };
        let mut recent = Conversation::default();
        let mut coverage = Coverage::default();
        let mut categories: BTreeMap<String, Vec<Option<f64>>> = BTreeMap::new();
        let mut audit = Vec::new();
        let mut probes = 0;
        let mut turns = 0;
        let mut nights = 0;
        let ends = self.corpus.day_end_records()?;
        let memory_mode = match self.arm {
            "none" => MemoryMode::None,
            "full_history" => MemoryMode::FullHistory,
            _ => MemoryMode::Bounded,
        };
        for event in self.observed {
            let out = ops.run(Op::Ingest, json!({"event":event}))?;
            recent.record(event, effect(&out.data)?)?;
            if ends.contains(&event.record_id) {
                ops.drain(event, "nightly", &mut recent)?;
                nights += 1;
                if nights % 7 == 0 {
                    ops.drain(event, "weekly", &mut recent)?;
                }
            }
            for question in self
                .corpus
                .questions
                .iter()
                .filter(|q| q.after_record == event.record_id)
            {
                let before = ops.run(Op::Inspect, json!({"person_id":self.corpus.persona.id}))?;
                ops.run(
                    Op::ProbeBegin,
                    json!({"probe_id":question.id,"at_unix_ms":question.query.at_unix_ms}),
                )?;
                let mut conversation = recent.clone();
                let mut answers = Vec::<Answer>::new();
                let mut recorded = Vec::new();
                for (index, query) in std::iter::once(&question.query)
                    .chain(&question.followups)
                    .enumerate()
                {
                    let result = Answerer {
                        runner: ops.runner,
                        worker: ops.worker,
                        recent: &mut conversation,
                        timezone: &self.corpus.persona.timezone,
                        mode: memory_mode,
                    }
                    .answer(query, &mut ops.log)?;
                    (ops.log)(
                        "candidate_answer",
                        json!({"question":question.id,"category":question.category,"turn":index,"result":result,"scores":null}),
                    )?;
                    let exchange = Event {
                        record_id: evaluation::probe_source_id(&question.id, index),
                        at_unix_ms: query.at_unix_ms,
                        session_id: question.id.clone(),
                        kind: EventKind::Utterance,
                        person: query.person,
                        person_id: query.person_id.clone(),
                        presence: query.presence,
                        heard: query.heard.clone(),
                        said: result.answer.answer.clone(),
                        stt_confidence: None,
                        cues: query.cues.clone(),
                    };
                    let final_effect = if result.forget_effect.suppress_confirmation {
                        result.forget_effect
                    } else {
                        let ingested = ops.run(Op::Ingest, json!({"event":exchange}))?;
                        combined(result.forget_effect, effect(&ingested.data)?)
                    };
                    conversation.record(&exchange, final_effect)?;
                    recorded.push(!final_effect.suppress_confirmation);
                    answers.push(result.answer);
                    turns += 1;
                }
                let after = ops.run(Op::Inspect, json!({"person_id":self.corpus.persona.id}))?;
                coverage.observe(
                    before.data.get("diagnostics"),
                    after.data.get("diagnostics"),
                )?;
                (ops.log)(
                    "probe_before",
                    json!({"question":question.id,"inspection":before.data,"human_audit":question.human_audit}),
                )?;
                (ops.log)(
                    "probe_after",
                    json!({"question":question.id,"inspection":after.data,"human_audit":question.human_audit}),
                )?;
                ops.run(Op::ProbeEnd, json!({"probe_id":question.id}))?;
                let restored = ops.run(Op::Inspect, json!({"person_id":self.corpus.persona.id}))?;
                for key in ["owner_model", "retained_record_ids"] {
                    if restored.data[key] != before.data[key] {
                        return Err(invalid("probe restoration changed durable memory view"));
                    }
                }
                if self.mode == Mode::Live {
                    let evaluated = evaluation::evaluate_with_history(
                        question,
                        &answers,
                        &recorded,
                        &self.corpus.events,
                        &self.corpus.persona.timezone,
                        ops.worker,
                    )?;
                    if evaluated.score.needs_audit {
                        audit.push(question.id.clone());
                    }
                    categories
                        .entry(question.category.clone())
                        .or_default()
                        .push(evaluated.score.fraction);
                    (ops.log)(
                        "question_evaluation",
                        json!({"question":question.id,"category":question.category,"evaluation":evaluated}),
                    )?;
                } else {
                    // Exercise the judge transport and strict JSON shape without
                    // publishing any synthetic judgment as an accuracy result.
                    if let Some(call) = evaluation::judge_call_with_history(
                        question,
                        &answers,
                        &recorded,
                        &self.corpus.events,
                        &self.corpus.persona.timezone,
                    )? {
                        let reply = ops
                            .worker
                            .call(&call, Instant::now() + Duration::from_millis(MODEL_MS))?;
                        if !reply.tool_calls.is_empty() {
                            return Err(invalid("plumbing judge called tools"));
                        }
                        let judge: JudgeOutput = serde_json::from_str(
                            reply
                                .content
                                .as_deref()
                                .ok_or_else(|| invalid("plumbing judge omitted text"))?,
                        )?;
                        let Rule::Rubric { criteria, .. } = &question.rule else {
                            return Err(invalid("plumbing judge without rubric"));
                        };
                        if judge.awarded.len() != criteria.len()
                            || judge.explanation.trim().is_empty()
                        {
                            return Err(invalid("plumbing judge shape differs from rubric"));
                        }
                        (ops.log)(
                            "judge_plumbing",
                            json!({"question":question.id,"request_hash":call.request_hash()?,"reply_hash":hash(&serde_json::to_vec(&reply)?),"scores":null}),
                        )?;
                    }
                    categories
                        .entry(question.category.clone())
                        .or_default()
                        .push(None);
                    if question.human_audit || matches!(question.rule, Rule::Rubric { .. }) {
                        audit.push(question.id.clone());
                    }
                }
                probes += 1;
            }
        }
        let total = categories.values().map(Vec::len).sum::<usize>();
        let scored = categories.values().flatten().flatten().count();
        let overall = (total > 0 && scored == total)
            .then(|| categories.values().flatten().flatten().sum::<f64>() / total as f64);
        let by_kind:BTreeMap<_,_>=categories.into_iter().map(|(kind,values)|{
            let complete=values.iter().all(Option::is_some);
            let mean=complete.then(||values.iter().flatten().sum::<f64>()/values.len() as f64);
            (kind,json!({"questions":values.len(),"mean":mean,"scored":values.iter().flatten().count()}))
        }).collect();
        Ok(
            json!({"label":if self.mode==Mode::Live {"live; provisional pending human audit"}else{"plumbing check"},"arm":self.arm,"events":self.observed.len(),"questions":probes,"answer_turns":turns,"nights":nights,"weekly_passes":nights/7,"by_kind":by_kind,"overall":overall,"human_audit":audit,"tier_usage":coverage,"fixture_profile":self.corpus.profile,"coverage_gaps":["no second identified person","no delivered-recall acknowledgement",if matches!(self.corpus.profile,Profile::Month){"fading not exercised by this month"}else{"fading requires live metadata audit"},"current-product in-turn forgetting is weaker than the uniform bench restart"],"score_status":if self.mode==Mode::Live{"provisional"}else{"no scores"}}),
        )
    }
}
