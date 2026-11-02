//! Pre-registered deterministic scorers and a strict, separately audited judge.
use crate::{Result, invalid};
use jiff::{civil::Date, tz::TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub answer: String,
    pub evidence_ids: Vec<String>,
    pub abstain: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    Exact {
        answers: Vec<String>,
        forbidden: Vec<String>,
    },
    Date {
        expected: String,
        tolerance_days: u32,
    },
    Abstain {
        forms: Vec<String>,
        forbidden: Vec<String>,
    },
    Rubric {
        criteria: Vec<String>,
        forbidden: Vec<String>,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub id: String,
    pub category: String,
    pub after_record: String,
    pub query: crate::protocol::Query,
    #[serde(default)]
    pub followups: Vec<crate::protocol::Query>,
    pub rule: Rule,
    pub evidence_ids: Vec<String>,
    pub eligible_evidence_ids: Vec<String>,
    pub human_audit: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Score {
    /// None means unjudged, not zero accuracy.
    pub fraction: Option<f64>,
    pub needs_judge: bool,
    pub needs_audit: bool,
    pub reason: String,
}
impl Score {
    fn decided(correct: bool, audit: bool, reason: &str) -> Self {
        Self {
            fraction: Some(if correct { 1.0 } else { 0.0 }),
            needs_judge: false,
            needs_audit: audit,
            reason: reason.into(),
        }
    }
}
pub fn normalize(text: &str) -> String {
    text.trim()
        .trim_end_matches(['.', '!', '?'])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn forbidden(answer: &str, values: &[String]) -> bool {
    let words = |s: &str| {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let answer = format!(" {} ", words(answer));
    values.iter().any(|s| {
        let value = words(s);
        !value.is_empty() && answer.contains(&format!(" {value} "))
    })
}
fn day(text: &str) -> Result<i64> {
    let date: Date = text
        .parse()
        .map_err(|_| invalid("date answer must be YYYY-MM-DD"))?;
    let zoned = date
        .at(0, 0, 0, 0)
        .to_zoned(TimeZone::UTC)
        .map_err(|_| invalid("date outside range"))?;
    Ok(zoned.timestamp().as_millisecond() / 86_400_000)
}
impl Question {
    pub fn validate(&self) -> Result<()> {
        self.query.validate()?;
        let mut at = self.query.at_unix_ms;
        for followup in &self.followups {
            followup.validate()?;
            if followup.probe_id != self.id
                || followup.at_unix_ms <= at
                || followup.heard.trim().is_empty()
            {
                return Err(invalid(
                    "follow-up is out of order or belongs to another probe",
                ));
            }
            at = followup.at_unix_ms;
        }
        if !crate::protocol::valid_id(&self.id)
            || self.query.probe_id != self.id
            || self.query.heard.trim().is_empty()
            || self.category.is_empty()
            || !crate::protocol::valid_id(&self.after_record)
        {
            return Err(invalid("invalid question identity or schedule"));
        }
        match &self.rule {
            Rule::Exact { answers, .. }
                if answers.is_empty() || answers.iter().any(|a| normalize(a).is_empty()) =>
            {
                Err(invalid("exact scorer needs nonempty aliases"))
            }
            Rule::Abstain { forms, .. }
                if forms.is_empty() || forms.iter().any(|a| normalize(a).is_empty()) =>
            {
                Err(invalid("abstention scorer needs registered forms"))
            }
            Rule::Date {
                expected,
                tolerance_days,
            } => {
                day(expected)?;
                if *tolerance_days > 1 {
                    return Err(invalid("date tolerance exceeds registered one day"));
                }
                Ok(())
            }
            Rule::Rubric { criteria, .. }
                if criteria.is_empty() || criteria.iter().any(|c| c.trim().is_empty()) =>
            {
                Err(invalid("rubric has no criteria"))
            }
            _ => Ok(()),
        }
    }
    pub fn score(&self, answer: &Answer) -> Result<Score> {
        self.validate()?;
        let known: BTreeSet<_> = self.eligible_evidence_ids.iter().collect();
        if answer.evidence_ids.iter().any(|id| !known.contains(id)) {
            return Ok(Score::decided(false, true, "unsupported evidence citation"));
        }
        match &self.rule {
            Rule::Exact {
                answers,
                forbidden: banned,
            } => Ok(Score::decided(
                !answer.abstain
                    && !forbidden(&answer.answer, banned)
                    && answers
                        .iter()
                        .any(|a| normalize(a) == normalize(&answer.answer)),
                self.human_audit,
                "whole-answer exact match",
            )),
            Rule::Date {
                expected,
                tolerance_days,
            } => {
                let correct = !answer.abstain
                    && day(answer.answer.trim()).is_ok_and(|actual| {
                        day(expected).is_ok_and(|expected| {
                            actual.abs_diff(expected) <= u64::from(*tolerance_days)
                        })
                    });
                Ok(Score::decided(
                    correct,
                    self.human_audit,
                    "registered calendar-day tolerance",
                ))
            }
            Rule::Abstain {
                forms,
                forbidden: banned,
            } => Ok(Score::decided(
                answer.abstain
                    && answer.evidence_ids.is_empty()
                    && !forbidden(&answer.answer, banned)
                    && forms
                        .iter()
                        .any(|s| normalize(s) == normalize(&answer.answer)),
                self.human_audit,
                "registered abstention without an invented answer",
            )),
            Rule::Rubric {
                forbidden: banned, ..
            } => {
                if forbidden(&answer.answer, banned) {
                    return Ok(Score::decided(
                        false,
                        true,
                        "forbidden claim before rubric judge",
                    ));
                }
                Ok(Score {
                    fraction: None,
                    needs_judge: true,
                    needs_audit: true,
                    reason: "awaiting blinded rubric judge and human audit".into(),
                })
            }
        }
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JudgeOutput {
    pub awarded: Vec<bool>,
    pub explanation: String,
    pub evidence_ids: Vec<String>,
}
pub fn judge_score(question: &Question, answer: &Answer, judge: &JudgeOutput) -> Result<Score> {
    let preliminary = question.score(answer)?;
    if !preliminary.needs_judge {
        return Ok(preliminary);
    }
    let Rule::Rubric { criteria, .. } = &question.rule else {
        return Err(invalid("not a rubric question"));
    };
    if judge.awarded.len() != criteria.len()
        || judge.explanation.trim().is_empty()
        || judge
            .evidence_ids
            .iter()
            .any(|id| !question.evidence_ids.contains(id))
    {
        return Err(invalid("judge response does not match frozen rubric"));
    }
    Ok(Score {
        fraction: Some(judge.awarded.iter().filter(|&&v| v).count() as f64 / criteria.len() as f64),
        needs_judge: false,
        needs_audit: true,
        reason: "provisional rubric score; human audit pending".into(),
    })
}
