//! Separately registered aging extension; never changes the month fixture.
use crate::{
    Result,
    corpus::{Corpus, Evidence, Profile},
    files, hash, invalid,
    protocol::*,
    score::{Question, Rule},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Episode {
    pub id: String,
    pub day: u32,
    pub role: String,
    pub topic_word: String,
    pub heard: Vec<String>,
    pub question: String,
    pub criterion: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recent {
    pub id: String,
    pub day: u32,
    pub label: String,
    pub value: String,
    pub heard: String,
    pub question: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub version: u32,
    pub fictional: bool,
    pub owner: String,
    pub end_day: u32,
    pub memory_text_bytes: usize,
    pub candidate_order: Vec<String>,
    pub repetitions: u32,
    pub checkpoints: Vec<u32>,
    pub episodes: Vec<Episode>,
    pub recent: Vec<Recent>,
    pub daily_continuation: Vec<String>,
    pub reason: String,
    pub gates: Vec<String>,
}
impl Scenario {
    fn validate(&self) -> Result<()> {
        if self.version != 1
            || !self.fictional
            || self.end_day != 120
            || self.memory_text_bytes != 4096
            || self.repetitions != 3
            || self.candidate_order != ["tiers", "tiers_minus_fading"]
            || self.checkpoints != [90, 120]
            || self.episodes.len() != 16
            || self.recent.len() != 8
            || self.daily_continuation.len() != 2
        {
            return Err(invalid(
                "aging dimensions differ from the registered design",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut days = BTreeSet::new();
        for e in &self.episodes {
            if !valid_id(&e.id)
                || !ids.insert(&e.id)
                || !days.insert(e.day)
                || !(1..=16).contains(&e.day)
                || e.heard.len() != 2
                || e.heard.iter().any(|t| t.is_empty() || t.len() > 16384)
                || e.question.is_empty()
                || e.criterion.is_empty()
                || e.role
                    != if e.day <= 8 {
                        "low_importance"
                    } else {
                        "high_importance_control"
                    }
            {
                return Err(invalid("invalid aging episode or control"));
            }
        }
        days.clear();
        for r in &self.recent {
            if !valid_id(&r.id)
                || !ids.insert(&r.id)
                || !days.insert(r.day)
                || !(110..=117).contains(&r.day)
                || r.value.is_empty()
                || r.heard.is_empty()
                || r.question.is_empty()
            {
                return Err(invalid("invalid aging recent fact"));
            }
        }
        Ok(())
    }
}
fn visit(
    c: &mut Corpus,
    day: u32,
    hour: i8,
    minute: i8,
    id: &str,
    lines: &[String],
    fact: Option<&str>,
) -> Result<Vec<String>> {
    let at = c.persona.at(day, hour, minute)?;
    let mut sources = Vec::new();
    for (suffix, kind, offset, line) in [
        ("arrive", EventKind::Arrive, 0, None),
        ("start", EventKind::SessionStart, 300_000, None),
        ("one", EventKind::Utterance, 303_000, Some(0)),
        ("two", EventKind::Utterance, 318_000, Some(1)),
        ("end", EventKind::SessionEnd, 363_000, None),
        ("leave", EventKind::Leave, 364_000, None),
    ] {
        let record_id = format!("{id}-{suffix}");
        let utterance = line.is_some();
        if utterance {
            sources.push(record_id.clone());
            c.answer_key.push(Evidence {
                record_id: record_id.clone(),
                fact_id: fact.map(str::to_owned),
                trait_id: None,
                true_speaker: c.persona.id.clone(),
            });
        }
        c.events.push(Event {
            record_id,
            at_unix_ms: at + offset,
            session_id: id.into(),
            kind,
            person: Person::Owner,
            person_id: Some(c.persona.id.clone()),
            presence: if kind == EventKind::Leave {
                Presence::Unknown
            } else {
                Presence::OwnerAlone
            },
            heard: line.map(|i| lines[i].clone()).unwrap_or_default(),
            said: if utterance {
                ["Got it.", "Okay.", "I hear you."][(day as usize + line.unwrap_or(0)) % 3].into()
            } else {
                String::new()
            },
            stt_confidence: None,
            cues: Cues {
                activity: Some(
                    if kind == EventKind::Leave {
                        "away"
                    } else if line == Some(1) {
                        "head_down"
                    } else {
                        "at_desk"
                    }
                    .into(),
                ),
                ..Cues::default()
            },
        });
    }
    Ok(sources)
}
fn probe(
    c: &Corpus,
    id: String,
    day: u32,
    category: &str,
    heard: String,
    rule: Rule,
    evidence_ids: Vec<String>,
) -> Result<Question> {
    let after_record = format!("age-day-{day:03}-leave");
    let index = c
        .events
        .iter()
        .position(|e| e.record_id == after_record)
        .ok_or_else(|| invalid("aging probe boundary absent"))?;
    Ok(Question {
        id: id.clone(),
        category: category.into(),
        after_record,
        query: Query {
            probe_id: id,
            at_unix_ms: c.persona.at(day, 18, 0)?,
            person: Person::Owner,
            person_id: Some(c.persona.id.clone()),
            presence: Presence::OwnerAlone,
            heard,
            cues: Cues {
                activity: Some("at_desk".into()),
                ..Cues::default()
            },
            max_context_bytes: 4096,
        },
        followups: Vec::new(),
        rule,
        evidence_ids,
        eligible_evidence_ids: c.events[..=index]
            .iter()
            .map(|e| e.record_id.clone())
            .collect(),
        human_audit: true,
    })
}
pub fn generate(
    mut base: Corpus,
    scenario: &Scenario,
    base_hash: String,
    scenario_hash: String,
) -> Result<Corpus> {
    scenario.validate()?;
    if !matches!(base.profile, Profile::Month) {
        return Err(invalid("aging needs an unchanged month base"));
    }
    files::validate(&base)?;
    base.profile = Profile::Aging {
        base_registration_hash: base_hash,
        scenario_hash,
    };
    base.questions.clear();
    let mut episode_sources = Vec::new();
    for e in &scenario.episodes {
        episode_sources.push(visit(
            &mut base,
            e.day,
            17,
            30,
            &e.id,
            &e.heard,
            Some(&e.id),
        )?);
    }
    let mut recent_sources = std::collections::BTreeMap::new();
    for day in 31..=120 {
        let mut lines = scenario.daily_continuation.clone();
        let recent = scenario.recent.iter().find(|r| r.day == day);
        if let Some(r) = recent {
            lines[0] = r.heard.clone();
        }
        let sources = visit(
            &mut base,
            day,
            8,
            55,
            &format!("age-day-{day:03}"),
            &lines,
            recent.map(|r| r.id.as_str()),
        )?;
        if let Some(r) = recent {
            recent_sources.insert(r.id.clone(), vec![sources[0].clone()]);
        }
    }
    base.events.sort_by_key(|e| e.at_unix_ms);
    for day in &scenario.checkpoints {
        for (i, e) in scenario.episodes.iter().enumerate() {
            let q = probe(
                &base,
                format!("{}-d{day}", e.id),
                *day,
                if e.role == "low_importance" {
                    "aging_gist"
                } else {
                    "aging_control"
                },
                e.question.clone(),
                Rule::Rubric {
                    criteria: vec![e.criterion.clone()],
                    forbidden: Vec::new(),
                },
                episode_sources[i].clone(),
            )?;
            base.questions.push(q);
        }
    }
    for r in &scenario.recent {
        let q = probe(
            &base,
            format!("{}-d120", r.id),
            120,
            "aging_recent",
            r.question.clone(),
            Rule::Exact {
                answers: vec![r.value.clone()],
                forbidden: Vec::new(),
            },
            recent_sources[&r.id].clone(),
        )?;
        base.questions.push(q);
    }
    for f in base.persona.facts.iter().filter(|f| f.forget_day.is_some()) {
        let q = probe(
            &base,
            format!("aging-forget-{}", f.id),
            120,
            "aging_forgetting",
            format!("What is my {}?", f.label),
            Rule::Abstain {
                forms: vec![
                    "I don't know".into(),
                    "I don't remember".into(),
                    "I can't share that".into(),
                ],
                forbidden: vec![f.value.clone()],
            },
            Vec::new(),
        )?;
        base.questions.push(q);
    }
    files::validate(&base)?;
    Ok(base)
}
pub fn write(month: &Path, scenario_path: &Path, destination: &Path) -> Result<Corpus> {
    let base = files::read_corpus(month)?;
    let raw = fs::read(scenario_path)?;
    let scenario: Scenario = files::json_file(scenario_path)?;
    let corpus = generate(
        base,
        &scenario,
        hash(&fs::read(month.join("registration.json"))?),
        hash(&raw),
    )?;
    files::write_corpus(destination, &corpus)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination.join("scenario.json"))?;
    use std::io::Write;
    file.write_all(&raw)?;
    file.sync_all()?;
    files::read_corpus(destination)?;
    Ok(corpus)
}
