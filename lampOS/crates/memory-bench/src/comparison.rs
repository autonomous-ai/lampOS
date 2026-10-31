//! Complete registered comparison, including every failed or absent repetition.
use crate::{
    Result,
    campaign::{Campaign, Plan},
    diagnostics::Coverage,
    files, hash, invalid,
    ledger::{self, Entry, Manifest, Mode},
    live_plan::Policy,
    live_run,
    model::Provenance,
    score::{Question, Score},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
fn label(arm: &str) -> &str {
    match arm {
        "none" => "no memory",
        "full_history" => "full history",
        "lampos_today_partial" => "partial current product",
        "tiers" => "ours (tiers)",
        "tiers_minus_episodes" => "without episodes",
        "tiers_minus_life_story" => "without life story (chapters and events)",
        "tiers_minus_owner_model" => "without owner model",
        "tiers_minus_how_to_be" => "without how-to-be",
        "tiers_minus_ahead" => "without what is ahead",
        "tiers_minus_feelings" => "without feelings",
        "tiers_minus_importance" => "without importance scoring",
        "tiers_minus_fading" => "without fading",
        "tiers_minus_facts" => "facts not recalled; kept only as evidence",
        "tiers_minus_nightly_sleep" => {
            "no consolidation of facts, lines and episode summaries; nightly topic-boundary pass still runs"
        }
        "tiers_session_cutting_only" => "session cutting only",
        _ => arm,
    }
}
fn fraction(v: &Value) -> Result<f64> {
    v.as_f64()
        .filter(|n| n.is_finite() && (0.0..=1.0).contains(n))
        .ok_or_else(|| invalid("invalid reported score fraction"))
}
fn summaries(
    questions: &[Question],
    entries: &[Entry],
    terminal: &Value,
    mode: Mode,
) -> Result<BTreeMap<String, f64>> {
    let expected: BTreeMap<_, _> = questions.iter().map(|q| (q.id.as_str(), q)).collect();
    let mut seen = BTreeSet::new();
    let mut kinds: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for q in questions {
        *counts.entry(q.category.clone()).or_default() += 1;
    }
    for row in entries.iter().filter(|e| e.kind == "question_evaluation") {
        if mode != Mode::Live {
            return Err(invalid("recorded attempt contains numerical evaluations"));
        }
        let id = row.data["question"]
            .as_str()
            .ok_or_else(|| invalid("evaluation lacks question"))?;
        let q = expected
            .get(id)
            .filter(|_| seen.insert(id))
            .ok_or_else(|| invalid("duplicate or unregistered evaluation"))?;
        if row.data["category"] != q.category {
            return Err(invalid("evaluation category differs from registration"));
        }
        let score: Score = serde_json::from_value(row.data["evaluation"]["score"].clone())?;
        if score.needs_judge {
            return Err(invalid("completed result still needs a judge"));
        }
        let n = fraction(&json!(score.fraction))?;
        kinds.entry(q.category.clone()).or_default().push(n);
    }
    if terminal["questions"] != questions.len()
        || terminal["by_kind"].as_object().map(|v| v.len()) != Some(counts.len())
    {
        return Err(invalid("terminal question coverage differs"));
    }
    let mut means = BTreeMap::new();
    let mut total = 0.0;
    for (kind, count) in counts {
        let row = &terminal["by_kind"][&kind];
        if row["questions"] != count {
            return Err(invalid("terminal category count differs"));
        }
        if mode == Mode::Recorded {
            if !row["mean"].is_null() || row["scored"] != 0 {
                return Err(invalid("recorded summary has a score"));
            }
            continue;
        }
        let values = kinds
            .get(&kind)
            .filter(|v| v.len() == count)
            .ok_or_else(|| invalid("incomplete evaluated question kind"))?;
        let sum = values.iter().sum::<f64>();
        let mean = sum / count as f64;
        if row["scored"] != count || (fraction(&row["mean"])? - mean).abs() > 1e-12 {
            return Err(invalid("terminal category mean differs from evaluations"));
        }
        total += sum;
        means.insert(kind, mean);
    }
    if mode == Mode::Live {
        let overall = total / questions.len() as f64;
        if (fraction(&terminal["overall"])? - overall).abs() > 1e-12 {
            return Err(invalid("terminal overall differs from evaluations"));
        }
        means.insert("overall".into(), overall);
    } else if !terminal["overall"].is_null() {
        return Err(invalid("recorded overall must be null"));
    }
    Ok(means)
}
fn evidence(path: &Path, entries: &[Entry], policy: &Policy) -> Result<usize> {
    let mut ids = BTreeSet::new();
    for row in entries
        .iter()
        .filter(|e| e.kind == "model_accounting" && e.data["stage"] == "model_completed")
    {
        let r = &row.data["receipt"];
        let number = r["call_number"]
            .as_u64()
            .filter(|n| *n > 0 && ids.insert(*n))
            .ok_or_else(|| invalid("invalid completed model identity"))?;
        if r["outcome"] == "expired_before_dispatch" && r["charged_microusd"] == 0 {
            continue;
        }
        let p: Provenance = serde_json::from_value(r["provenance"].clone())?;
        if !p.evidence_complete
            || p.transport != "deepseek_http_sse"
            || p.requested_model != policy.chat.requested_model
            || p.artifact.len() > 32
            || p.artifact.is_empty()
            || !p.artifact.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(invalid(
                "model evidence is incomplete or from another route",
            ));
        }
        let dir = path.join("provider").join(&p.artifact);
        private_directory(&path.join("provider"))?;
        private_directory(&dir)?;
        let stored: Value = files::json_file(&dir.join("provenance.json"))?;
        if stored != serde_json::to_value(&p)?
            || hash(&read(&dir.join("request.json"), 4 << 20)?) != p.request_sha256
            || hash(&read(&dir.join("response.sse"), 4 << 20)?) != p.response_sha256
        {
            return Err(invalid(format!(
                "saved provider evidence differs for call {number}"
            )));
        }
    }
    Ok(ids.len())
}
fn private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 {
        return Err(invalid(
            "report directory must be private and not a symlink",
        ));
    }
    Ok(())
}
fn read(path: &Path, max: u64) -> Result<Vec<u8>> {
    use std::{io::Read, os::unix::fs::OpenOptionsExt};
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    if !f.metadata()?.is_file() || f.metadata()?.len() > max {
        return Err(invalid("report input exceeds file bound"));
    }
    let mut b = Vec::new();
    f.take(max + 1).read_to_end(&mut b)?;
    if b.len() as u64 > max {
        return Err(invalid("report input grew beyond bound"));
    }
    Ok(b)
}
struct Attempt {
    row: Value,
    means: Option<BTreeMap<String, f64>>,
    coverage: Option<Coverage>,
}
fn attempt(
    root: &Path,
    policy: &Policy,
    slot: (&str, &str, u32),
    mode: Mode,
    campaign: &Campaign,
    cache: Option<&str>,
) -> Result<Attempt> {
    let (fixture, arm, repetition) = slot;
    let id = live_run::run_id(fixture, arm, repetition)?;
    let path = root.join("runs").join(&id);
    let accounting = campaign.run_accounting(&id);
    if !path.try_exists()? {
        return Ok(Attempt {
            row: json!({"repetition":repetition,"status":if accounting.calls==0{"not started"}else{"missing ledger"},"scores":null,"accounting":accounting}),
            means: None,
            coverage: None,
        });
    }
    let verified = ledger::verify(&path)?;
    let m: Manifest = files::json_file(&path.join("manifest.json"))?;
    let f = &policy.fixtures[fixture];
    let a = &policy.adapters[arm];
    if m.run_id != id
        || m.mode != mode
        || m.arm != arm
        || m.implementation_revision != a.revision
        || m.registration_revision != f.registration_revision
        || m.registration_hash != f.registration_sha256
        || m.model != policy.chat.requested_model
        || m.budget_microusd != a.budget_microusd
        || m.config["fixture"] != fixture
        || m.config["repetition"] != repetition
        || m.config["policy_sha256"] != policy.hash()?
        || m.config["runner_sha256"] != policy.runner_sha256
        || m.config["runner_revision"] != policy.runner_revision
        || m.config["evaluation_sha256"] != policy.evaluation_sha256
        || m.config["adapter_sha256"] != a.executable_sha256
    {
        return Err(invalid(
            "run manifest differs from registered slot or policy",
        ));
    }
    let status = verified.terminal.as_deref().unwrap_or("incomplete");
    let mut row = json!({"repetition":repetition,"status":status,"ledger_hash":verified.final_hash,"ledger":path,"accounting":accounting,"scores":null});
    let entries: Vec<Entry> = files::lines(&path.join("events.jsonl"))?;
    if status != "completed" {
        row["failure"] = entries
            .last()
            .filter(|e| e.kind == "failed")
            .map(|e| e.data.clone())
            .unwrap_or(Value::Null);
        return Ok(Attempt {
            row,
            means: None,
            coverage: None,
        });
    }
    if m.transcript_hash.as_deref() != cache || cache.is_none() {
        return Err(invalid(
            "completed run lacks the shared verified speech cache",
        ));
    }
    private_directory(&path.join("source"))?;
    if hash(&read(&path.join("adapter"), 256 << 20)?) != a.executable_sha256 {
        return Err(invalid(
            "preserved adapter differs from approved executable",
        ));
    }
    let corpus = files::read_corpus(&path.join("source"))?;
    if hash(&read(&path.join("source/registration.json"), 1 << 20)?) != f.registration_sha256
        || hash(&read(&path.join("source/events.jsonl"), 32 << 20)?) != m.fixture_hash
        || m.seed != corpus.persona.seed
        || m.timezone != corpus.persona.timezone
    {
        return Err(invalid("preserved source differs from run pins"));
    }
    let result = &entries
        .last()
        .ok_or_else(|| invalid("empty completed ledger"))?
        .data;
    if result["events"] != corpus.events.len()
        || result["answer_turns"]
            != corpus
                .questions
                .iter()
                .map(|q| 1 + q.followups.len())
                .sum::<usize>()
        || result["nights"] != corpus.day_end_records()?.len()
        || result["model_calls"] != accounting.calls
        || result["charged_microusd"] != accounting.charged_microusd
        || accounting.settled_calls != accounting.calls
        || evidence(&path, &entries, policy)? as u64 != accounting.calls
    {
        return Err(invalid(
            "completed coverage, accounting or provider evidence differs",
        ));
    }
    let means = summaries(&corpus.questions, &entries, result, mode)?;
    let coverage: Coverage = serde_json::from_value(result["tier_usage"].clone())?;
    if coverage.probes != corpus.questions.len() as u64 {
        return Err(invalid("diagnostic coverage differs from probes"));
    }
    row["scores"] = if mode == Mode::Live {
        json!(means)
    } else {
        Value::Null
    };
    row["tier_usage"] = json!(coverage);
    row["human_audit"] = result["human_audit"].clone();
    row["coverage_gaps"] = result["coverage_gaps"].clone();
    Ok(Attempt {
        row,
        means: (mode == Mode::Live).then_some(means),
        coverage: Some(coverage),
    })
}
fn aggregate(attempts: &[Attempt]) -> Option<BTreeMap<String, Value>> {
    if attempts.len() != 3 {
        return None;
    }
    let data = attempts
        .iter()
        .map(|a| a.means.as_ref())
        .collect::<Option<Vec<_>>>()?;
    let first = data.first()?;
    if data.iter().any(|d| d.keys().ne(first.keys())) {
        return None;
    }
    Some(first.keys().map(|kind|{let values:Vec<_>=data.iter().map(|d|d[kind]).collect();let min=values.iter().copied().fold(f64::INFINITY,f64::min);let max=values.iter().copied().fold(f64::NEG_INFINITY,f64::max);(kind.clone(),json!({"mean":values.iter().sum::<f64>()/3.0,"min":min,"max":max,"spread":max-min}))}).collect())
}
fn exercise(arm: &str, profile: &str, full: &[Attempt]) -> String {
    if arm == "tiers_minus_fading" {
        return if profile == "month" {
            "not exercised by this month"
        } else {
            "pending live aging metadata audit: old low-importance detail and no recent recall"
        }
        .into();
    }
    let tier = arm.strip_prefix("tiers_minus_").filter(|t| {
        [
            "episodes",
            "life_story",
            "facts",
            "owner_model",
            "how_to_be",
            "ahead",
            "feelings",
        ]
        .contains(t)
    });
    let Some(tier) = tier else {
        return "mechanism coverage requires metadata audit".into();
    };
    let states: Vec<_> = full
        .iter()
        .map(|a| {
            a.coverage
                .as_ref()
                .map(|c| c.exercise(tier))
                .unwrap_or("unknown")
        })
        .collect();
    if states.contains(&"not exercised") {
        "not exercised".into()
    } else if states.len() != 3 || states.contains(&"unknown") {
        "unknown".into()
    } else {
        "population and returned text observed; not quality evidence".into()
    }
}
fn speech_ready(
    root: &Path,
    policy: &Policy,
    name: &str,
    mode: Mode,
    campaign: &Campaign,
) -> Result<String> {
    let (cache, pin) = live_run::verified_cache(root, policy, name, mode)?;
    let path = root.join("runs").join(live_run::speech_id(name));
    let f = &policy.fixtures[name];
    private_directory(&path.join("source"))?;
    files::read_corpus(&path.join("source"))?;
    if hash(&read(&path.join("source/registration.json"), 1 << 20)?) != f.registration_sha256 {
        return Err(invalid("shared speech source differs from registration"));
    }
    let original = read(&path.join("source/events.jsonl"), 32 << 20)?;
    let manifest: Manifest = files::json_file(&path.join("manifest.json"))?;
    if manifest.fixture_hash != hash(&original)
        || manifest.registration_revision != f.registration_revision
        || manifest.budget_microusd != f.speech_budget_microusd
        || manifest.model != policy.speech.expected_model_arch
        || manifest.config["runner_sha256"] != policy.runner_sha256
        || manifest.config["evaluation_sha256"] != policy.evaluation_sha256
    {
        return Err(invalid("shared speech manifest differs from policy"));
    }
    crate::speech_cache::load_from_source(&cache, &original, &pin, mode == Mode::Live)?;
    let settings = crate::speech_cache::settings(&cache, &pin)?;
    if settings.audio_manifest_hash != f.audio_manifest_sha256
        || serde_json::to_value(settings.config)? != serde_json::to_value(&policy.speech)?
    {
        return Err(invalid("speech policy differs"));
    }
    let entries: Vec<Entry> = files::lines(&path.join("events.jsonl"))?;
    let result = &entries
        .last()
        .ok_or_else(|| invalid("empty speech ledger"))?
        .data;
    let account = campaign.run_accounting(&live_run::speech_id(name));
    if account.calls != account.settled_calls
        || result["model_calls"] != account.calls
        || result["charged_microusd"] != account.charged_microusd
    {
        return Err(invalid("shared speech settlement differs from campaign"));
    }
    Ok(pin)
}
pub fn report(
    policy: &Policy,
    approval: Plan,
    root: &Path,
    fixture_transport: bool,
) -> Result<Value> {
    policy.check_approval(&approval, fixture_transport)?;
    let campaign = Campaign::inspect(root, approval)?;
    let mode = if fixture_transport {
        Mode::Recorded
    } else {
        Mode::Live
    };
    private_directory(&root.join("runs"))?;
    let mut admitted = BTreeSet::new();
    for (name, f) in &policy.fixtures {
        admitted.insert(live_run::speech_id(name));
        for arm in &f.setup_order {
            for r in 1..=3 {
                admitted.insert(live_run::run_id(name, arm, r)?);
            }
        }
    }
    for entry in fs::read_dir(root.join("runs"))? {
        let name = entry?
            .file_name()
            .into_string()
            .map_err(|_| invalid("invalid run directory name"))?;
        if !admitted.contains(&name) {
            return Err(invalid("campaign contains an unregistered attempt"));
        }
    }
    let mut fixtures = Vec::new();
    for (name, f) in &policy.fixtures {
        let speech_path = root.join("runs").join(live_run::speech_id(name));
        let (speech, cache) = if speech_path.try_exists()? {
            match speech_ready(root, policy, name, mode, &campaign) {
                Ok(pin) => (
                    json!({"status":"completed","cache_sha256":pin,"accounting":campaign.run_accounting(&live_run::speech_id(name))}),
                    Some(pin),
                ),
                Err(e) => (
                    json!({"status":"unavailable","reason":e.to_string(),"accounting":campaign.run_accounting(&live_run::speech_id(name))}),
                    None,
                ),
            }
        } else {
            (
                json!({"status":"not started","accounting":campaign.run_accounting(&live_run::speech_id(name))}),
                None,
            )
        };
        let mut all: BTreeMap<String, Vec<Attempt>> = BTreeMap::new();
        let mut setups = Vec::new();
        let mut aggregates = BTreeMap::new();
        for arm in &f.setup_order {
            let attempts:Vec<_>=(1..=3).map(|r|attempt(root,policy,(name,arm,r),mode,&campaign,cache.as_deref()).unwrap_or_else(|e|Attempt{row:json!({"repetition":r,"status":"invalid","reason":e.to_string(),"scores":null,"accounting":campaign.run_accounting(&live_run::run_id(name,arm,r).unwrap_or_default())}),means:None,coverage:None})).collect();
            let mean = aggregate(&attempts);
            setups.push(json!({"arm":arm,"label":label(arm),"revision":policy.adapters[arm].revision,"executable_sha256":policy.adapters[arm].executable_sha256,"runs":attempts.iter().map(|a|&a.row).collect::<Vec<_>>(),"three_run_summary":mean}));
            aggregates.insert(arm.clone(), mean);
            all.insert(arm.clone(), attempts);
        }
        let mut comparisons = Vec::new();
        let full = aggregates.get("tiers").and_then(Option::as_ref);
        for arm in f
            .setup_order
            .iter()
            .filter(|a| a.starts_with("tiers_") && a.as_str() != "tiers")
        {
            let coverage = exercise(arm, &f.profile, &all["tiers"]);
            let removed = aggregates.get(arm).and_then(Option::as_ref);
            let differences=full.zip(removed).map(|(full,removed)|full.iter().filter_map(|(kind,row)|removed.get(kind).map(|r|{let loss=row["mean"].as_f64().unwrap()-r["mean"].as_f64().unwrap();(kind.clone(),json!({"mean_loss":loss,"full_engine_spread":row["spread"],"exceeds_spread":loss>row["spread"].as_f64().unwrap()}))})).collect::<BTreeMap<_,_>>());
            comparisons.push(json!({"arm":arm,"label":label(arm),"exercise":coverage,"differences":differences,"retention_decision":"pending human safety/privacy and coverage audit; no automatic removal"}));
        }
        fixtures.push(json!({"fixture":name,"profile":f.profile,"registration_sha256":f.registration_sha256,"speech":speech,"setups":setups,"comparisons":comparisons,"gaps":["partial current product is not a complete product baseline","no second identified person in the development fixture","no delivered-recall acknowledgement","fixed-text probes; misheard questions require a separate voice test","current-product in-turn forgetting is weaker than the uniform bench restart","scripted companion acknowledgements do not test real lamp replies or promises",if f.profile=="month"{"fading not exercised by this month"}else{"fading requires live prerequisite audit"}]}));
    }
    Ok(
        json!({"label":if mode==Mode::Recorded{"plumbing check; no scores"}else{"live; provisional pending human audit"},"policy_sha256":policy.hash()?,"charged_microusd":campaign.charged_microusd(),"remaining_microusd":campaign.remaining_microusd(),"spending_stopped":campaign.stopped()||campaign.remaining_microusd()==0,"fixtures":fixtures,"optional_outside_systems":"excluded; not priced or approved","selection_rule":"all registered setups and all three attempts remain; missing or failed attempts never become zero and prevent a three-run mean"}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn attempt_with(value: Option<f64>) -> Attempt {
        Attempt {
            row: json!({}),
            means: value.map(|v| BTreeMap::from([("overall".into(), v), ("exact".into(), v)])),
            coverage: None,
        }
    }
    #[test]
    fn all_three_live_attempts_are_required_and_spread_is_max_minus_min() {
        assert!(
            aggregate(&[
                attempt_with(Some(0.9)),
                attempt_with(Some(0.8)),
                attempt_with(None)
            ])
            .is_none()
        );
        assert!(aggregate(&[attempt_with(Some(0.9)), attempt_with(Some(0.8))]).is_none());
        assert!(aggregate(&[attempt_with(None), attempt_with(None), attempt_with(None)]).is_none());
        let values = aggregate(&[
            attempt_with(Some(0.9)),
            attempt_with(Some(0.8)),
            attempt_with(Some(0.7)),
        ])
        .unwrap();
        assert!((values["overall"]["mean"].as_f64().unwrap() - 0.8).abs() < 1e-12);
        assert!((values["exact"]["spread"].as_f64().unwrap() - 0.2).abs() < 1e-12);
    }
    fn questions() -> Vec<Question> {
        let corpus = files::read_corpus(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/memory-bench/development"),
        )
        .unwrap();
        let mut a = corpus.questions[0].clone();
        a.id = "first".into();
        a.category = "exact".into();
        let mut b = a.clone();
        b.id = "second".into();
        vec![a, b]
    }
    fn evaluation(id: &str, value: f64) -> Entry {
        Entry {
            sequence: 0,
            previous_hash: String::new(),
            kind: "question_evaluation".into(),
            data: json!({"question":id,"category":"exact","evaluation":{"score":{"fraction":value,"needs_judge":false,"needs_audit":true,"reason":"unit fixture"}}}),
            hash: String::new(),
        }
    }
    #[test]
    fn terminal_means_are_recomputed_and_recorded_evaluations_are_refused() {
        let q = questions();
        let rows = vec![evaluation("first", 1.0), evaluation("second", 0.0)];
        let good = json!({"questions":2,"overall":0.5,"by_kind":{"exact":{"questions":2,"scored":2,"mean":0.5}}});
        assert_eq!(
            summaries(&q, &rows, &good, Mode::Live).unwrap()["overall"],
            0.5
        );
        let mut wrong = good.clone();
        wrong["overall"] = json!(1.0);
        assert!(summaries(&q, &rows, &wrong, Mode::Live).is_err());
        let duplicate = vec![evaluation("first", 1.0), evaluation("first", 0.0)];
        assert!(summaries(&q, &duplicate, &good, Mode::Live).is_err());
        assert!(summaries(&q, &rows, &good, Mode::Recorded).is_err());
        let recorded = json!({"questions":2,"overall":null,"by_kind":{"exact":{"questions":2,"scored":0,"mean":null}}});
        assert!(
            summaries(&q, &[], &recorded, Mode::Recorded)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn empty_tiers_and_fading_cannot_be_called_valueless() {
        let mut full = vec![attempt_with(None), attempt_with(None), attempt_with(None)];
        for a in &mut full {
            a.coverage=Some(serde_json::from_value(json!({"probes":200,"missing_diagnostics":false,"tiers":{"facts":{"items":0,"text_bytes":0,"recall_calls":0,"snapshot_calls":0}}})).unwrap());
        }
        assert_eq!(
            exercise("tiers_minus_facts", "month", &full),
            "not exercised"
        );
        assert_eq!(
            exercise("tiers_minus_fading", "month", &full),
            "not exercised by this month"
        );
        assert!(exercise("tiers_minus_fading", "aging", &full).contains("pending live aging"));
        assert_eq!(
            label("tiers_minus_facts"),
            "facts not recalled; kept only as evidence"
        );
    }
}
