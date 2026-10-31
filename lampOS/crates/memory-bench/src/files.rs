//! Separate evaluator artifacts from the event-only input sent to an arm.
use crate::{
    Result,
    corpus::{Corpus, Persona, Profile},
    hash, invalid,
    protocol::{MAX_FRAME_BYTES, read_frame},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{BufReader, Write},
    path::Path,
};

pub const ARMS: [&str; 12] = [
    "tiers",
    "tiers_minus_episodes",
    "tiers_minus_life_story",
    "tiers_minus_facts",
    "tiers_minus_owner_model",
    "tiers_minus_how_to_be",
    "tiers_minus_ahead",
    "tiers_minus_feelings",
    "tiers_minus_nightly_sleep",
    "tiers_minus_importance",
    "tiers_minus_fading",
    "tiers_session_cutting_only",
];
pub fn json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() > MAX_FRAME_BYTES as u64 {
        return Err(invalid("JSON input must be a bounded regular file"));
    }
    Ok(serde_json::from_reader(File::open(path)?)?)
}
pub fn lines<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut values = Vec::new();
    while let Some(value) = read_frame(&mut reader)? {
        if values.len() >= 100_000 {
            return Err(invalid("too many input rows"));
        }
        values.push(serde_json::from_value(value)?);
    }
    Ok(values)
}
fn create(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn write_lines<T: Serialize>(path: &Path, values: &[T]) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    for value in values {
        crate::protocol::write_frame(&mut file, value)?;
    }
    file.sync_all()?;
    Ok(())
}
pub fn write_corpus(path: &Path, corpus: &Corpus) -> Result<()> {
    validate(corpus)?;
    fs::create_dir(path)?;
    create(
        &path.join("persona.json"),
        &serde_json::to_vec_pretty(&corpus.persona)?,
    )?;
    write_lines(&path.join("events.jsonl"), &corpus.events)?;
    write_lines(&path.join("answer-key.jsonl"), &corpus.answer_key)?;
    write_lines(&path.join("questions.jsonl"), &corpus.questions)?;
    let hashes = [
        "persona.json",
        "events.jsonl",
        "answer-key.jsonl",
        "questions.jsonl",
    ]
    .into_iter()
    .map(|name| Ok((name, hash(&fs::read(path.join(name))?))))
    .collect::<Result<BTreeMap<_, _>>>()?;
    let mut registration = json!({"version":5,"protocol_version":2,"protocol_minor":1,"status":"draft_pending_human_review","fictional":true,
        "question_count":200,"days":30,"utterances_per_day":50,"timezone":corpus.persona.timezone,
        "seed":corpus.persona.seed,"files":hashes,"scorers":"src/score.rs and tests/guards.rs committed before scored runs",
        "coverage_revision":{"date":"2026-10-31","before_any_score":true,"reason":"Give memory only product-observable activity and a realistic prompt budget; prevent empty or unused tier removals from being treated as evidence of no value.","activity":"Scripted at_desk, head_down and away from visit stage directions; probe posture rotates independently of question content.","topic":null,"objects":[],"memory_text_bytes":4096,"no_memory":"no memory text","full_history":"ceiling; no memory-selection cap, complete prompt must fit pinned model context; transport overflow is unsupported, never truncation","diagnostics":"optional metadata-only tier-usage-v1 on inspection; unknown when absent; never scored","gaps":["fading not exercised by this month","no second identified person","no delivered-recall acknowledgement","no supplied-topic episode split","no supplied object labels"]},
        "topic_segmentation_revision":{"date":"2026-10-31","before_any_score":true,"reason":"The prior engine cut only on sessions and departures; measure sleep-time topic segmentation rather than assuming its benefit.","full_engine":"nightly model proposes record-id ranges within each session; code requires contiguous ranges covering every eligible record exactly once; no added live-reply work","all_ten_removals":"segmentation remains enabled; each removal differs from the full engine only in its named component","no_nightly_sleep_label":"no consolidation of facts, lines and episode summaries; nightly topic-boundary pass still runs","no_nightly_sleep_weekly":"weekly reflection remains enabled","session_cutting_only":"same engine with topic segmentation disabled; session and departure boundaries remain","readiness":"If segmentation is not ready, only the three reference setups may run; hold every Tiers setup for one agreed segmented build.","unchanged":"all persona, event, question and answer-key bytes from registration v4","cost":"six additional two-persona runs; new segmentation overhead unmeasured"},
        "candidate_order":ARMS,"repetitions":3,"candidate_label":"ours (tiers)",
        "facts_removal":"facts not recalled; kept only as evidence",
        "timing_revision":{"date":"2026-10-31","before_any_score":true,"reason":"Nine-minute spacing incorrectly made every utterance a product conversation and biased time-based episode boundaries.","conversations_per_day":8,"exchanges_per_conversation":[2,12],"exchange_gap_seconds":[3,30],"visits_per_day":4,"nightly":"after the final event of each local calendar day","weekly":"after every seventh local calendar day","unchanged":"persona, owner utterance content and IDs, question content and scoring, answer key; eligible source lists include new boundary events"},
        "scripted_replies":{"before_any_score":true,"choices":["Got it.","Okay.","I hear you."],"selection":"deterministic seed/day/line rotation, identical for every setup","reason":"Complete answered exchanges for product journaling without model-dependent history or extra model calls.","limitations":"No new facts or promises. This month does not test the lamp's actual replies or its own promises."},
        "arm_name_revision":{"tiers_minus_facts_people_places":"tiers_minus_facts","tiers_minus_whats_ahead":"tiers_minus_ahead","reason":"Align adapter manifest names; component meanings and order unchanged."},
        "retained_safety_rules":["source_provenance","forgetting_enforcement"],
        "wrong_person_records":["d08-020","d08-021","d08-022"],
        "wrong_person_gate":"zero owner facts or owner-model lines learned from the second resident's default-owner records; missing inspection evidence is unmeasured, never a pass",
        "comparison":"mean loss exceeds full-engine max-minus-min spread for overall or any category, or safety/privacy necessity",
        "recall_ms":200,"snapshot_ms":300,"transcripts":"not_yet_recorded","human_audit":"pending",
        "judge":"required for rubric scores; recorded tests are not live results"});
    if let Profile::Aging { .. } = &corpus.profile {
        registration = json!({"version":2,"profile":corpus.profile,"protocol_version":2,"protocol_minor":1,
            "status":"registered_before_scoring_pending_human_review","fictional":true,"days":120,"question_count":42,
            "timezone":corpus.persona.timezone,"seed":corpus.persona.seed,"files":hashes,
            "candidate_order":["tiers","tiers_minus_fading"],"topic_segmentation":"enabled identically in both setups; use the same agreed segmented build","repetitions":3,"memory_text_bytes":4096,
            "reason":"The unchanged 30-day month cannot exercise the unchanged strictly-over-90-day fading threshold.",
            "schedule":"Thirty-day base plus sixteen additional early two-exchange episodes, then one two-exchange visit each day 31 through 120. Nightly maintenance on all 120 calendar days; weekly after days 7, 14, ... 119.",
            "probes":"Matched gist and high-importance-control questions after day 90 and day 120; recent facts and forgetting after day 120. Probes are isolated.",
            "coverage_gates":["accepted nonempty detail and gist in low-importance episodes below 0.5","high-importance controls at or above 0.5","sleep maintenance after day 90","report whether eligible old detail competes with recent evidence under the fixed byte cap"],
            "comparison":"Full engine first, then no fading; three runs each. Retain fading if removing it lowers overall or any aging category mean by more than full-engine max-minus-min spread, or for safety/privacy necessity. Missing prerequisites mean not exercised; no keep/drop conclusion.",
            "history_access":"ordinary recall is primary; explicit-history access is separately reported because source records survive fading",
            "gaps":["no delivered-recall acknowledgement or protection test","no second identified person"],
            "transcripts":"not_yet_recorded","human_audit":"all 42 questions before any live score","scores":null});
    }
    create(
        &path.join("registration.json"),
        &serde_json::to_vec_pretty(&registration)?,
    )?;
    Ok(())
}
pub fn read_corpus(path: &Path) -> Result<Corpus> {
    let registration: Value = json_file(&path.join("registration.json"))?;
    let profile: Profile = registration
        .get("profile")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?
        .unwrap_or_default();
    if let Profile::Aging { scenario_hash, .. } = &profile
        && hash(&fs::read(path.join("scenario.json"))?) != *scenario_hash
    {
        return Err(invalid("aging scenario hash differs from registration"));
    }
    let persona: Persona = json_file(&path.join("persona.json"))?;
    let events = lines(&path.join("events.jsonl"))?;
    let answer_key = lines(&path.join("answer-key.jsonl"))?;
    let questions = lines(&path.join("questions.jsonl"))?;
    let corpus = Corpus {
        profile,
        persona,
        events,
        answer_key,
        questions,
    };
    validate(&corpus)?;
    for name in [
        "persona.json",
        "events.jsonl",
        "answer-key.jsonl",
        "questions.jsonl",
    ] {
        let actual = hash(&fs::read(path.join(name))?);
        if registration
            .pointer(&format!("/files/{name}"))
            .and_then(Value::as_str)
            != Some(&actual)
        {
            return Err(invalid(format!("registered {name} hash differs")));
        }
    }
    Ok(corpus)
}
pub fn validate(corpus: &Corpus) -> Result<()> {
    corpus.persona.validate()?;
    let (event_count, question_count, day_count) = match corpus.profile {
        Profile::Month => (2220, 200, 30),
        Profile::Aging { .. } => (2856, 42, 120),
    };
    if corpus.events.len() != event_count
        || corpus.questions.len() != question_count
        || corpus.day_end_records()?.len() != day_count
    {
        return Err(invalid("corpus dimensions differ from plan"));
    }
    let mut records = BTreeMap::new();
    let mut last = 0;
    for event in &corpus.events {
        event.validate()?;
        if event.at_unix_ms < last || records.insert(event.record_id.as_str(), event).is_some() {
            return Err(invalid("unordered or duplicate event"));
        }
        last = event.at_unix_ms;
    }
    let mut keyed = BTreeSet::new();
    for evidence in &corpus.answer_key {
        if !records.contains_key(evidence.record_id.as_str()) || !keyed.insert(&evidence.record_id)
        {
            return Err(invalid("answer key references missing or duplicate source"));
        }
    }
    let mut questions = BTreeSet::new();
    for q in &corpus.questions {
        q.validate()?;
        let boundary_index = corpus
            .events
            .iter()
            .position(|e| e.record_id == q.after_record)
            .ok_or_else(|| invalid("question has no scheduled boundary"))?;
        let prefix = &corpus.events[..=boundary_index];
        let expected: BTreeSet<_> = prefix.iter().map(|e| e.record_id.as_str()).collect();
        let eligible: BTreeSet<_> = q.eligible_evidence_ids.iter().map(String::as_str).collect();
        if eligible != expected || eligible.len() != q.eligible_evidence_ids.len() {
            return Err(invalid(
                "eligible evidence differs from the observed history",
            ));
        }
        let boundary = records
            .get(q.after_record.as_str())
            .ok_or_else(|| invalid("question has no scheduled boundary"))?;
        if !questions.insert(&q.id) || q.query.at_unix_ms < boundary.at_unix_ms {
            return Err(invalid("question is before its boundary or duplicated"));
        }
        for source in &q.evidence_ids {
            if !expected.contains(source.as_str()) {
                return Err(invalid("answer key cites future or absent evidence"));
            }
        }
        let mut time = q.query.at_unix_ms;
        for followup in &q.followups {
            if followup.probe_id != q.id || followup.at_unix_ms < time {
                return Err(invalid("invalid multi-turn probe order"));
            }
            time = followup.at_unix_ms;
        }
    }
    Ok(())
}
