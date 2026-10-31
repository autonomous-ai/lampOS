//! Frozen common-answerer policy and arm-blind, multi-turn rubric evaluation.
use crate::{
    Result, hash, invalid,
    model::{Call, Purpose},
    model_worker::Worker,
    protocol::Event,
    score::{self, Answer, JudgeOutput, Question, Rule, Score},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
pub const ANSWER_PROMPT: &str =
    include_str!("../../../fixtures/memory-bench/live-policy/answer-system.txt");
pub const JUDGE_PROMPT: &str =
    include_str!("../../../fixtures/memory-bench/live-policy/judge-system.txt");
pub const OUTPUT_TOKENS: u64 = 512;
pub const TOOL_ROUNDS: usize = 8;
pub fn policy() -> Value {
    json!({"version":1,"before_any_score":true,"answer_prompt_sha256":hash(ANSWER_PROMPT.as_bytes()),"judge_prompt_sha256":hash(JUDGE_PROMPT.as_bytes()),"output_tokens":OUTPUT_TOKENS,"tool_rounds":TOOL_ROUNDS,
        "probe_input":"fixed registered text, including all 224 month turns; no probe STT allowance",
        "month_input":"one verified real-STT cache shared by all setups and repetitions",
        "probe_boundary_reason":"A misheard question changes the memory skill under test. Noisy questions belong to a separate voice test; these scores can be kinder than real spoken use.",
        "judge":"question turns, fixed criteria, eligible scripted source evidence and candidate answers only; no arm, run, provider, spending or diagnostic metadata",
        "multi_turn":"all turns scored together; not just the last answer",
        "judge_sources":"gold and eligible candidate-cited scripted records plus fixed probe questions; generated candidate answers have explicit IDs and can witness timing or dialogue, never establish independent owner facts",
        "probe_citations":"an answer may cite a source from the main prefix or an earlier completed exchange in its own isolated probe, never a future or current exchange; restoration removes probe sources",
        "deterministic_precedence":"exact/date/abstention/forbidden claims and unsupported citations cannot be overruled by the judge",
        "invalid_output":"failed or malformed answer/judge output leaves the trial incomplete; no numerical score, silent repair or unregistered retry",
        "forget_restart":"after an actual forget attempt, drop the current round and all cached memory, take a fresh snapshot and restart from the current question plus a content-free forget-status note; apply uniformly to every setup",
        "current_product_forget_gap":"the current product clears future recent history but retains the current request prefix and previous tool results; the uniform bench restart is stronger and may improve its measured forgetting",
        "human_audit":"rubric scores are provisional until audited; preserve every registered audit and suspected privacy failure"})
}
fn aggregate(question: &Question, answers: &[Answer]) -> Result<Answer> {
    question.validate()?;
    if answers.len() != 1 + question.followups.len() {
        return Err(invalid(
            "candidate turn count differs from registered probe",
        ));
    }
    for answer in answers {
        if answer.answer.len() > 32_768
            || answer.evidence_ids.len() > 128
            || answer
                .evidence_ids
                .iter()
                .any(|id| !crate::protocol::valid_id(id))
        {
            return Err(invalid("candidate answer exceeds bounded format"));
        }
    }
    if answers.len() == 1 {
        return Ok(answers[0].clone());
    }
    if !matches!(question.rule, Rule::Rubric { .. }) {
        return Err(invalid("multi-turn deterministic rule is not registered"));
    }
    Ok(Answer {
        answer: answers
            .iter()
            .map(|a| a.answer.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        evidence_ids: answers
            .iter()
            .flat_map(|a| a.evidence_ids.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        abstain: answers.iter().all(|a| a.abstain),
    })
}
fn prepared(question: &Question, answers: &[Answer]) -> Result<(Question, Answer, Score)> {
    let aggregate = aggregate(question, answers)?;
    let mut eligible: BTreeSet<_> = question.eligible_evidence_ids.iter().cloned().collect();
    if (0..answers.len()).any(|index| eligible.contains(&format!("probe-{}-{index}", question.id)))
    {
        return Err(invalid(
            "probe source namespace collides with scripted history",
        ));
    }
    for (index, answer) in answers.iter().enumerate() {
        if answer.evidence_ids.iter().any(|id| !eligible.contains(id)) {
            return Ok((
                question.clone(),
                aggregate,
                Score {
                    fraction: Some(0.0),
                    needs_judge: false,
                    needs_audit: true,
                    reason: "source cited before it was eligible in this isolated probe".into(),
                },
            ));
        }
        eligible.insert(format!("probe-{}-{index}", question.id));
    }
    let mut scored = question.clone();
    scored.eligible_evidence_ids = eligible.into_iter().collect();
    let preliminary = scored.score(&aggregate)?;
    Ok((scored, aggregate, preliminary))
}
fn scripted_sources(question: &Question, answers: &[Answer]) -> BTreeSet<String> {
    let mut sources: BTreeSet<_> = question.evidence_ids.iter().cloned().collect();
    let eligible: BTreeSet<_> = question.eligible_evidence_ids.iter().collect();
    for answer in answers {
        for id in &answer.evidence_ids {
            if eligible.contains(id) {
                sources.insert(id.clone());
            }
        }
    }
    sources
}
fn supplied_judge_ids(question: &Question, answers: &[Answer]) -> Vec<String> {
    scripted_sources(question, answers)
        .into_iter()
        .chain((0..answers.len()).map(|index| format!("probe-{}-{index}", question.id)))
        .collect()
}
fn local(at: u64, timezone: &str) -> Result<String> {
    let at = i64::try_from(at).map_err(|_| invalid("probe time exceeds calendar"))?;
    let stamp = jiff::Timestamp::from_millisecond(at)
        .map_err(|_| invalid("probe time exceeds calendar"))?;
    let zone =
        jiff::tz::TimeZone::get(timezone).map_err(|_| invalid("invalid evaluation timezone"))?;
    Ok(stamp.to_zoned(zone).to_string())
}
/// No implementation identity is an argument, so it cannot accidentally enter
/// the judge request through the evaluator's run manifest or diagnostics.
pub fn judge_call(
    question: &Question,
    answers: &[Answer],
    source_events: &[Event],
    timezone: &str,
) -> Result<Option<Call>> {
    let (_, _, preliminary) = prepared(question, answers)?;
    if !preliminary.needs_judge {
        return Ok(None);
    }
    let Rule::Rubric { criteria, .. } = &question.rule else {
        return Err(invalid("rubric missing"));
    };
    let eligible: BTreeSet<_> = question.eligible_evidence_ids.iter().collect();
    let mut records = BTreeMap::new();
    for event in source_events {
        event.validate()?;
        if records.insert(&event.record_id, event).is_some() {
            return Err(invalid("duplicate evaluator source"));
        }
    }
    let mut evidence = vec![];
    let sources = scripted_sources(question, answers);
    for id in &sources {
        if !eligible.contains(id) {
            return Err(invalid("rubric source was not eligible at probe start"));
        }
        let event = records
            .get(id)
            .ok_or_else(|| invalid("rubric source missing from frozen script"))?;
        if event.at_unix_ms > question.query.at_unix_ms {
            return Err(invalid("future evidence entered judge"));
        }
        evidence.push(json!({"origin":"scripted_history","source_id":event.record_id,"at_unix_ms":event.at_unix_ms,"local_time":local(event.at_unix_ms,timezone)?,"person":event.person,"person_id":event.person_id,"presence":event.presence,"heard":event.heard,"said":event.said,"cues":event.cues}));
    }
    let turns=std::iter::once(&question.query).chain(&question.followups).zip(answers).enumerate().map(|(index,(query,answer))|Ok(json!({"origin":"candidate_probe_exchange","source_id":format!("probe-{}-{index}",question.id),"at_unix_ms":query.at_unix_ms,"local_time":local(query.at_unix_ms,timezone)?,"speaker":query.person,"person_id":query.person_id,"presence":query.presence,"question":query.heard,"answer":answer}))).collect::<Result<Vec<_>>>()?;
    let packet = json!({"timezone":timezone,"rubric":criteria,"source_evidence":evidence,"candidate_turns":turns});
    let call = Call::Chat {
        purpose: Purpose::Judge,
        messages: vec![
            json!({"role":"system","content":JUDGE_PROMPT}),
            json!({"role":"user","content":serde_json::to_string(&packet)?}),
        ],
        max_output_tokens: OUTPUT_TOKENS,
        response_format: Some(json!({"type":"json_object"})),
        tools: vec![],
        tool_choice: "none".into(),
    };
    call.validate()?;
    Ok(Some(call))
}
#[derive(Serialize)]
pub struct Evaluation {
    pub score: Score,
    pub judge: Option<JudgeOutput>,
    pub judge_request_hash: Option<String>,
}
pub fn evaluate(
    question: &Question,
    answers: &[Answer],
    source_events: &[Event],
    timezone: &str,
    worker: &mut Worker,
) -> Result<Evaluation> {
    let (mut scored_question, aggregate, preliminary) = prepared(question, answers)?;
    let Some(call) = judge_call(question, answers, source_events, timezone)? else {
        return Ok(Evaluation {
            score: preliminary,
            judge: None,
            judge_request_hash: None,
        });
    };
    let reply = worker.call(
        &call,
        Instant::now() + Duration::from_millis(crate::protocol::MODEL_MS),
    )?;
    if !reply.tool_calls.is_empty() {
        return Err(invalid("blind judge requested tools"));
    }
    let text = reply
        .content
        .ok_or_else(|| invalid("blind judge omitted text"))?;
    let judge: JudgeOutput = serde_json::from_str(&text)?;
    if judge.explanation.len() > 16_384 || judge.evidence_ids.len() > 128 {
        return Err(invalid("blind judge exceeded output bounds"));
    }
    scored_question.evidence_ids = supplied_judge_ids(question, answers);
    let score = score::judge_score(&scored_question, &aggregate, &judge)?;
    Ok(Evaluation {
        score,
        judge: Some(judge),
        judge_request_hash: Some(call.request_hash()?),
    })
}
