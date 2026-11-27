//! Adapt one sensor-6 capture or paired-clip replay to the #8850 run schema.
//! See docs/verification/caller-match-8849 for inputs and commands.
use lamp_voice::same_voice::{ASKED_AT, DIFFERENT_BELOW, SAME_FROM};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fs,
    io::{self, BufRead, BufReader},
    path::{Component, Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
struct Manifest {
    version: u8,
    sessions: Vec<Session>,
    trials: Vec<Trial>,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Session {
    id: String,
    split: String,
    clock_id: String,
    consent: bool,
    delete_after_unix_s: u64,
}
#[derive(Deserialize)]
struct Trial {
    id: String,
    session_id: String,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    sensor: String,
    trial_id: String,
    reference_trial_id: Option<String>,
}
#[derive(Deserialize)]
struct Sidecar {
    version: u8,
    manifest_sha256: String,
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    case_id: String,
    trial_id: String,
    reference_trial_id: Option<String>,
    clock_id: String,
    worker_epoch: String,
    request_id: String,
    input: Input,
}
#[derive(Deserialize)]
struct Input {
    audio_relpath: Option<PathBuf>,
    reference_audio_relpath: Option<PathBuf>,
}

// The capture controller records admission wait and behaviour independently.
#[derive(Deserialize)]
struct Audit {
    case_id: String,
    caller_request_id: Option<String>,
    input_end_us: Option<u64>,
    over_playback: Option<bool>,
    reference_over_playback: Option<bool>,
    added_wait_us: u64,
    held_line: bool,
    safety_violation: bool,
}

#[derive(Deserialize, Serialize)]
struct Run {
    version: u8,
    manifest_sha256: String,
    sensor: String,
    build_sha: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    producer_binary_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_patch_sha256: Option<String>,
    model_sha256: String,
    config_sha256: String,
    evidence: String,
    location: String,
    predictions: Vec<Prediction>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Class {
    Same,
    Different,
    Unknown,
}

#[derive(Deserialize, Serialize)]
struct Prediction {
    case_id: String,
    clock_id: String,
    output: Class,
    input_end_us: u64,
    ready_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_admission_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    replay_elapsed_us: Option<u64>,
    added_wait_us: u64,
    held_line: bool,
    safety_violation: bool,
}

#[derive(Deserialize)]
struct Observation {
    clock_id: Option<String>,
    worker_epoch: String,
    caller_request_id: String,
    request_id: String,
    phase: String,
    output: Class,
    input_end_us: Option<u64>,
    line_end_us: u64,
    ready_us: Option<u64>,
    decision_us: u64,
    first_admission_us: Option<u64>,
}

#[derive(Default)]
struct Sample {
    decision: Option<(u64, u64)>,
    result: Option<(Class, u64, u64)>,
    admitted: Option<u64>,
}

struct Bound<'a> {
    entry: &'a Entry,
    audit: &'a Audit,
    session: &'a Session,
    reference: &'a Session,
}

fn index<'a, T>(items: &'a [T], key: impl Fn(&'a T) -> &'a str) -> Result<HashMap<&'a str, &'a T>> {
    let mut indexed = HashMap::new();
    for item in items {
        let id = key(item);
        if id.is_empty() || indexed.insert(id, item).is_some() {
            return Err("empty or duplicate identifier".into());
        }
    }
    Ok(indexed)
}

fn bind<'a>(
    manifest: &'a Manifest,
    sidecar: &'a Sidecar,
    audit: &'a [Audit],
    hash: &str,
) -> Result<Vec<Bound<'a>>> {
    if manifest.version != 1 || sidecar.version != 1 || sidecar.manifest_sha256 != hash {
        return Err("sidecar version or manifest hash mismatch".into());
    }
    let sessions = index(&manifest.sessions, |s| &s.id)?;
    let trials = index(&manifest.trials, |t| &t.id)?;
    let cases = index(&manifest.cases, |c| &c.id)?;
    index(&sidecar.entries, |e| &e.case_id)?;
    let audit = index(audit, |a| &a.case_id)?;
    for id in audit.keys() {
        if !cases.contains_key(id) {
            return Err("unknown observed case".into());
        }
    }
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    let mut split: Option<&str> = None;
    for e in &sidecar.entries {
        if e.worker_epoch.is_empty()
            || e.request_id.is_empty()
            || !seen.insert((&e.worker_epoch, &e.request_id))
        {
            return Err("invalid or repeated worker request".into());
        }
        let case = cases
            .get(e.case_id.as_str())
            .ok_or("unknown sidecar case")?;
        let trial = trials.get(e.trial_id.as_str()).ok_or("unknown trial")?;
        let session = *sessions
            .get(trial.session_id.as_str())
            .ok_or("unknown session")?;
        if case.trial_id != e.trial_id
            || case.reference_trial_id != e.reference_trial_id
            || session.clock_id != e.clock_id
            || e.clock_id.is_empty()
        {
            return Err("trial, reference or clock mismatch".into());
        }
        if case.sensor != "voice_match" {
            continue;
        }
        let id = e
            .reference_trial_id
            .as_deref()
            .ok_or("missing reference trial")?;
        if id == case.trial_id {
            return Err("trial is its own reference".into());
        }
        let reference = trials.get(id).ok_or("unknown opening trial")?;
        let reference = *sessions
            .get(reference.session_id.as_str())
            .ok_or("unknown reference session")?;
        if !matches!(session.split.as_str(), "development" | "holdout")
            || reference.split != session.split
            || split.is_some_and(|s| s != session.split)
        {
            return Err("collection splits differ".into());
        }
        split = Some(session.split.as_str());
        if let Some(audit) = audit.get(e.case_id.as_str()) {
            output.push(Bound {
                entry: e,
                audit,
                session,
                reference,
            });
        }
    }
    Ok(output)
}

impl Bound<'_> {
    fn prediction(&self, output: Class, input_end_us: u64) -> Prediction {
        Prediction {
            case_id: self.entry.case_id.clone(),
            clock_id: self.entry.clock_id.clone(),
            output,
            input_end_us,
            ready_us: None,
            first_admission_us: None,
            replay_elapsed_us: None,
            added_wait_us: self.audit.added_wait_us,
            held_line: self.audit.held_line,
            safety_violation: self.audit.safety_violation,
        }
    }
}

fn live(bound: &[Bound<'_>], journal: impl BufRead) -> Result<Vec<Prediction>> {
    let by_request: HashMap<_, _> = bound
        .iter()
        .enumerate()
        .map(|(i, b)| {
            (
                (b.entry.worker_epoch.as_str(), b.entry.request_id.as_str()),
                i,
            )
        })
        .collect();
    let mut samples: Vec<_> = bound.iter().map(|_| Sample::default()).collect();
    for line in journal.lines() {
        let line = line?;
        let Some((_, data)) = line.split_once("lamp-voice caller-match: ") else {
            continue;
        };
        let r: Observation = serde_json::from_str(data)?;
        let Some(&i) = by_request.get(&(r.worker_epoch.as_str(), r.request_id.as_str())) else {
            continue;
        };
        let b = &bound[i];
        if r.clock_id.as_deref() != Some(b.entry.clock_id.as_str())
            || b.audit.caller_request_id.as_deref() != Some(r.caller_request_id.as_str())
        {
            return Err("capture clock or observed opening request mismatch".into());
        }
        let s = &mut samples[i];
        let times = (r.line_end_us, r.decision_us);
        match r.phase.as_str() {
            "admission" => {
                if s.decision.replace(times).is_some() {
                    return Err("duplicate admission in capture".into());
                }
            }
            "admitted" | "available" | "renewal" => match s.decision {
                None => continue,
                Some(t) if t != times => return Err("line or decision time changed".into()),
                _ => {}
            },
            _ => return Err("unknown caller-match phase".into()),
        }
        if let Some(a) = r.first_admission_us {
            if a < r.decision_us || s.admitted.is_some_and(|v| v != a) {
                return Err("first admission time changed".into());
            }
            s.admitted = Some(a);
        }
        if let (Some(input), Some(ready)) = (r.input_end_us, r.ready_us) {
            let value = (r.output, input, ready);
            if ready < input || s.result.is_some_and(|v| v != value) {
                return Err("invalid or conflicting prediction".into());
            }
            s.result = Some(value);
        }
    }
    Ok(bound
        .iter()
        .zip(samples)
        .filter_map(|(b, s)| {
            let (class, input, ready) = s.result?;
            let mut p = b.prediction(class, input);
            p.ready_us = Some(ready);
            p.first_admission_us = s.admitted;
            Some(p)
        })
        .collect())
}

// A path is resolved under the consented session, including symlink resolution.
fn clip_path(root: &Path, session: &Session, rel: &Path) -> Result<PathBuf> {
    if !rel.components().all(|c| matches!(c, Component::Normal(_)))
        || !matches!(
            Path::new(&session.id)
                .components()
                .collect::<Vec<_>>()
                .as_slice(),
            [Component::Normal(_)]
        )
    {
        return Err("invalid relative clip or session".into());
    }
    let folder = root.join(&session.id).canonicalize()?;
    let clip = root.join(rel).canonicalize()?;
    if folder != root.join(&session.id) || !clip.starts_with(&folder) || !clip.is_file() {
        return Err("clip outside its consented session".into());
    }
    Ok(clip)
}

fn replay(
    bound: &[Bound<'_>],
    root: &Path,
    model: &Path,
    expected_hash: &str,
) -> Result<Vec<Prediction>> {
    if format!("{:x}", Sha256::digest(fs::read(model)?)) != expected_hash {
        return Err("model digest mismatch".into());
    }
    let voice = lamp_same_voice::Voice::load(model)?;
    let root = root.canonicalize()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let mut predictions = Vec::new();
    for b in bound {
        if [b.session, b.reference]
            .iter()
            .any(|s| !s.consent || now >= s.delete_after_unix_s)
        {
            return Err("clip consent is missing or expired".into());
        }
        let Some(input_end) = b.audit.input_end_us else {
            continue;
        };
        let (Some(candidate), Some(reference)) = (
            b.entry.input.audio_relpath.as_deref(),
            b.entry.input.reference_audio_relpath.as_deref(),
        ) else {
            continue;
        };
        if b.audit.over_playback != Some(false) || b.audit.reference_over_playback != Some(false) {
            predictions.push(b.prediction(Class::Unknown, input_end));
            continue;
        }
        let candidate = lamp_same_voice::read_wav(&clip_path(&root, b.session, candidate)?)?;
        let reference = lamp_same_voice::read_wav(&clip_path(&root, b.reference, reference)?)?;
        let before = Instant::now();
        let print = |clip: &[f32]| -> Result<Option<Vec<f32>>> {
            // Files are captured worker input with its pre-roll, as in the live model.
            if clip.len() < ASKED_AT {
                return Ok(None);
            }
            Ok(Some(voice.embedding(&clip[..ASKED_AT])?))
        };
        let score = match (print(&reference)?, print(&candidate)?) {
            (Some(a), Some(b)) => lamp_same_voice::cosine(&a, &b),
            _ => None,
        };
        let output = match score {
            Some(s) if s >= SAME_FROM => Class::Same,
            Some(s) if s < DIFFERENT_BELOW => Class::Different,
            _ => Class::Unknown,
        };
        let elapsed = u64::try_from(before.elapsed().as_micros())?;
        let mut p = b.prediction(output, input_end);
        p.replay_elapsed_us = Some(elapsed);
        predictions.push(p);
    }
    Ok(predictions)
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [
        mode,
        manifest_path,
        sidecar_path,
        run_path,
        audit_path,
        files @ ..,
    ] = args.as_slice()
    else {
        return Err("usage: caller_match_predictions MODE MANIFEST SIDECAR EMPTY_RUN AUDIT (JOURNAL | SESSION_ROOT MODEL)".into());
    };
    let bytes = fs::read(manifest_path)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    let sidecar: Sidecar = serde_json::from_slice(&fs::read(sidecar_path)?)?;
    let mut run: Run = serde_json::from_slice(&fs::read(run_path)?)?;
    let audit: Vec<Audit> = serde_json::from_slice(&fs::read(audit_path)?)?;
    let digest =
        |value: &str, digits| value.len() == digits && value.bytes().all(|b| b.is_ascii_hexdigit());
    if run.version != 1
        || run.manifest_sha256 != hash
        || run.sensor != "voice_match"
        || run.location != "local"
        || !digest(&run.build_sha, 40)
        || !digest(&run.model_sha256, 64)
        || !digest(&run.config_sha256, 64)
        || run
            .producer_binary_sha256
            .as_ref()
            .is_some_and(|v| !digest(v, 64))
        || run
            .source_patch_sha256
            .as_ref()
            .is_some_and(|v| !digest(v, 64) || run.producer_binary_sha256.is_none())
        || !run.predictions.is_empty()
    {
        return Err("incompatible run header".into());
    }
    let bound = bind(&manifest, &sidecar, &audit, &hash)?;
    run.predictions = match (mode.to_str(), files, run.evidence.as_str()) {
        (Some("live"), [journal], "lamp_live" | "synthetic") => {
            live(&bound, BufReader::new(fs::File::open(journal)?))?
        }
        (Some("replay"), [root, model], "replay") => {
            replay(&bound, Path::new(root), Path::new(model), &run.model_sha256)?
        }
        _ => return Err("arguments or evidence kind do not match mode".into()),
    };
    serde_json::to_writer_pretty(io::stdout().lock(), &run)?;
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn fixture() -> (Manifest, Sidecar, Vec<Audit>) {
        let manifest = json!({"version":1,
            "sessions":[
                {"id":"s1","split":"holdout","clock_id":"one","consent":true,"delete_after_unix_s":9000},
                {"id":"s2","split":"holdout","clock_id":"two","consent":true,"delete_after_unix_s":9000}],
            "trials":[{"id":"c","session_id":"s1"},{"id":"v","session_id":"s2"}],
            "cases":[{"id":"pair","sensor":"voice_match","trial_id":"v","reference_trial_id":"c"}]});
        let sidecar = json!({"version":1,"manifest_sha256":"hash","entries":[
            {"case_id":"pair","trial_id":"v","reference_trial_id":"c","clock_id":"two",
             "worker_epoch":"run","request_id":"2","input":{}}]});
        let audit = json!([{"case_id":"pair","caller_request_id":"1","added_wait_us":30,"held_line":false,"safety_violation":false}]);
        (
            serde_json::from_value(manifest).unwrap(),
            serde_json::from_value(sidecar).unwrap(),
            serde_json::from_value(audit).unwrap(),
        )
    }

    #[test]
    fn sidecar_is_bound_to_manifest_trials_split_clock_and_worker() {
        let (mut manifest, mut sidecar, audit) = fixture();
        assert_eq!(bind(&manifest, &sidecar, &audit, "hash").unwrap().len(), 1);
        assert!(bind(&manifest, &sidecar, &audit, "other").is_err());
        manifest.sessions[0].split = "development".into();
        assert!(bind(&manifest, &sidecar, &audit, "hash").is_err());
        manifest.sessions[0].split = "holdout".into();
        for field in ["clock_id", "reference_trial_id", "case_id", "worker_epoch"] {
            let mut entry = json!({"case_id":"pair","trial_id":"v","reference_trial_id":"c","clock_id":"two","worker_epoch":"run","request_id":"2","input":{}});
            entry[field] = Value::from("");
            sidecar.entries[0] = serde_json::from_value(entry).unwrap();
            assert!(
                bind(&manifest, &sidecar, &audit, "hash").is_err(),
                "{field}"
            );
        }
        let (_, mut sidecar, _) = fixture();
        let (_, mut duplicate, _) = fixture();
        sidecar.entries.append(&mut duplicate.entries);
        assert!(bind(&manifest, &sidecar, &audit, "hash").is_err());
        sidecar.entries[1].case_id = "second".into();
        let (mut duplicate, _, _) = fixture();
        duplicate.cases[0].id = "second".into();
        manifest.cases.append(&mut duplicate.cases);
        assert!(bind(&manifest, &sidecar, &audit, "hash").is_err());
    }

    fn row(phase: &str, ready: Option<u64>, admitted: Option<u64>) -> String {
        let row = json!({"clock_id":"two","worker_epoch":"run","caller_request_id":"1","request_id":"2",
            "phase":phase,"line_end_us":4_000_000,"decision_us":4_049_000,"input_end_us":4_010_000,
            "ready_us":ready,"first_admission_us":admitted,"output":if ready.is_some() {"same"} else {"unknown"}});
        format!("lamp-voice caller-match: {row}\n")
    }

    #[test]
    fn late_after_admission_is_measured_even_inside_the_acoustic_budget() {
        let (m, s, a) = fixture();
        let bound = bind(&m, &s, &a, "hash").unwrap();
        let start = row("admission", None, None);
        let admitted = row("admitted", None, Some(4_050_000));
        let late = row("available", Some(4_080_000), Some(4_050_000));
        let all = format!("{start}{admitted}{late}");
        let predictions = live(&bound, all.as_bytes()).unwrap();
        let [p] = predictions.as_slice() else {
            panic!("expected one observed prediction");
        };
        assert_eq!(p.added_wait_us, 30);
        assert_eq!(
            (p.output, p.input_end_us, p.ready_us, p.first_admission_us),
            (Class::Same, 4_010_000, Some(4_080_000), Some(4_050_000))
        );
        assert!(live(&bound, late.as_bytes()).unwrap().is_empty());
        assert!(live(&bound, start.as_bytes()).unwrap().is_empty());
        assert!(live(&bound, format!("{start}{start}").as_bytes()).is_err());
        assert!(
            live(
                &bound,
                all.replace("\"caller_request_id\":\"1\"", "\"caller_request_id\":\"8\"")
                    .as_bytes()
            )
            .is_err()
        );
    }
}
