//! Approved-policy wiring; every service call shares the durable campaign debit.
use crate::{
    Result,
    campaign::{Campaign, Plan},
    files, hash, invalid,
    ledger::{Ledger, Manifest, Mode},
    live_http,
    live_plan::{Policy, Run},
    live_speech,
    model::{Client, Provider},
    model_worker::Worker,
    protocol::{MODEL_MS, Op, Request},
    runner::Runner,
    trial::Trial,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Cursor, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
type Log = Arc<Mutex<Ledger>>;
fn log(log: &Log, kind: &str, data: Value) -> Result<()> {
    log.lock()
        .map_err(|_| invalid("run ledger lock failed"))?
        .append(kind, data)
}
pub fn run_id(fixture: &str, arm: &str, repetition: u32) -> Result<String> {
    Ok(format!(
        "run-{}",
        hash(&serde_json::to_vec(&json!([fixture, arm, repetition]))?)
    ))
}
pub fn speech_id(fixture: &str) -> String {
    format!("speech-{}", hash(fixture.as_bytes()))
}
fn bytes(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > limit {
        return Err(invalid("input must be a bounded regular file"));
    }
    let mut data = Vec::new();
    file.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        return Err(invalid("input grew beyond its bound"));
    }
    Ok(data)
}
fn pin(path: &Path, expected: &str) -> Result<Vec<u8>> {
    let data = bytes(path, 256 * 1024 * 1024)?;
    if hash(&data) != expected {
        return Err(invalid(
            "executable or input hash differs from approved policy",
        ));
    }
    Ok(data)
}
fn save(path: &Path, data: &[u8], mode: u32) -> Result<()> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(mode)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(data)?;
    file.sync_all()?;
    File::open(path.parent().ok_or_else(|| invalid("file needs parent"))?)?.sync_all()?;
    Ok(())
}
struct Source {
    corpus: crate::corpus::Corpus,
    files: BTreeMap<String, Vec<u8>>,
}
fn rows<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<Vec<T>> {
    let mut input = Cursor::new(data);
    let mut rows = Vec::new();
    while let Some(row) = crate::protocol::read_frame(&mut input)? {
        if rows.len() >= 100_000 {
            return Err(invalid("too many registered rows"));
        }
        rows.push(serde_json::from_value(row)?);
    }
    Ok(rows)
}

fn runner_pin(policy: &Policy) -> Result<()> {
    pin(&std::env::current_exe()?, &policy.runner_sha256).map(|_| ())
}
fn private_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::DirBuilder::new().mode(0o700).create(path)?;
        File::open(
            path.parent()
                .ok_or_else(|| invalid("directory needs a parent"))?,
        )?
        .sync_all()?;
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 {
        return Err(invalid("campaign run directory must be private and real"));
    }
    Ok(())
}
pub fn initialize(
    policy: &Policy,
    approval: Plan,
    root: &Path,
    fixture_transport: bool,
) -> Result<Value> {
    policy.check_approval(&approval, fixture_transport)?;
    runner_pin(policy)?;
    let campaign = Campaign::create(root, approval, policy.planned_microusd()?)?;
    private_dir(&root.join("runs"))?;
    Ok(
        json!({"initialized":true,"policy_sha256":policy.hash()?,"planned_microusd":policy.planned_microusd()?,"remaining_microusd":campaign.remaining_microusd(),"scores":null}),
    )
}
fn fixture(policy: &Policy, name: &str, path: &Path) -> Result<Source> {
    let expected = policy
        .fixtures
        .get(name)
        .ok_or_else(|| invalid("fixture is outside approved scope"))?;
    let registration = pin(
        &path.join("registration.json"),
        &expected.registration_sha256,
    )?;
    let value: Value = serde_json::from_slice(&registration)?;
    let profile: crate::corpus::Profile = value
        .get("profile")
        .map(|p| serde_json::from_value(p.clone()))
        .transpose()?
        .unwrap_or_default();
    let mut inputs = BTreeMap::from([("registration.json".into(), registration)]);
    if let crate::corpus::Profile::Aging { scenario_hash, .. } = &profile {
        inputs.insert(
            "scenario.json".into(),
            pin(&path.join("scenario.json"), scenario_hash)?,
        );
    }
    for file in [
        "persona.json",
        "events.jsonl",
        "questions.jsonl",
        "answer-key.jsonl",
    ] {
        let data = bytes(&path.join(file), 32 * 1024 * 1024)?;
        if value["files"][file].as_str() != Some(hash(&data).as_str()) {
            return Err(invalid("consumed fixture bytes differ from registration"));
        }
        inputs.insert(file.into(), data);
    }
    let corpus = crate::corpus::Corpus {
        profile,
        persona: serde_json::from_slice(&inputs["persona.json"])?,
        events: rows(&inputs["events.jsonl"])?,
        questions: rows(&inputs["questions.jsonl"])?,
        answer_key: rows(&inputs["answer-key.jsonl"])?,
    };
    files::validate(&corpus)?;
    let profile = if matches!(corpus.profile, crate::corpus::Profile::Month) {
        "month"
    } else {
        "aging"
    };
    if profile != expected.profile {
        return Err(invalid("fixture profile differs from approved scope"));
    }
    Ok(Source {
        corpus,
        files: inputs,
    })
}
fn save_source(output: &Path, source: &Source) -> Result<()> {
    let dir = output.join("source");
    fs::DirBuilder::new().mode(0o700).create(&dir)?;
    for (name, data) in &source.files {
        save(&dir.join(name), data, 0o400)?;
    }
    Ok(())
}

fn manifest(
    policy: &Policy,
    name: &str,
    source: &Source,
    run_id: String,
    arm: &str,
    budget: u64,
    mode: Mode,
) -> Result<Manifest> {
    let f = &policy.fixtures[name];
    Ok(Manifest {
        run_id,
        mode,
        arm: arm.into(),
        implementation_revision: policy.runner_revision.clone(),
        registration_revision: f.registration_revision.clone(),
        registration_hash: f.registration_sha256.clone(),
        fixture_hash: hash(&source.files["events.jsonl"]),
        transcript_hash: None,
        timezone: source.corpus.persona.timezone.clone(),
        model: policy.chat.requested_model.clone(),
        config: json!({"policy_sha256":policy.hash()?,"fixture":name,"runner_revision":policy.runner_revision,"runner_sha256":policy.runner_sha256,"evaluation_sha256":policy.evaluation_sha256,"label":label(policy.preliminary,mode,"live; provisional","plumbing check")}),
        budget_microusd: budget,
        seed: source.corpus.persona.seed,
    })
    .map(|mut m| {
        if policy.preliminary {
            m.config["preliminary"] = json!(true);
        }
        m
    })
}
/// The registered labels stay as they were; a preliminary policy says so in
/// every manifest, result and failure it writes.
fn label(preliminary: bool, mode: Mode, live: &str, recorded: &str) -> String {
    let text = if mode == Mode::Live { live } else { recorded };
    if preliminary {
        format!("{text}; {}", crate::live_plan::PRELIMINARY_LABEL)
    } else {
        text.into()
    }
}
fn worker<P: Provider + Send + 'static>(
    provider: P,
    policy: &Policy,
    budget: u64,
    campaign: Arc<Mutex<Campaign>>,
    ledger: Log,
    run: String,
) -> Result<Worker> {
    Worker::start(
        Client::new(provider, policy.chat_prices.clone(), budget)?
            .with_speech(policy.speech_prices.clone())?
            .with_audit(move |stage, receipt| {
                campaign
                    .lock()
                    .map_err(|_| invalid("campaign lock failed"))?
                    .audit(&run, stage, receipt)?;
                log(
                    &ledger,
                    "model_accounting",
                    json!({"stage":stage,"receipt":receipt}),
                )
            }),
    )
}
fn finish(
    mut worker: Worker,
    ledger: &Log,
    preliminary: bool,
    mode: Mode,
    result: Result<Value>,
) -> Result<Value> {
    let settled = worker.settle(Instant::now() + Duration::from_millis(MODEL_MS));
    let account = worker.account()?;
    let settled = if settled.is_ok() {
        // A barrier drains calls but does not close the worker. Join before a
        // successful terminal record so its campaign lock is already released.
        worker
            .finish(Instant::now() + Duration::from_millis(MODEL_MS))
            .map(|_| ())
    } else {
        // The outstanding call retains the campaign lock and reservation.
        drop(worker);
        settled.map(|_| ())
    };
    let evidence = if mode == Mode::Live {
        account.receipts.iter().all(|r| {
            r.outcome == "expired_before_dispatch"
                || r.provenance.as_ref().is_some_and(|p| p.evidence_complete)
        })
    } else {
        true
    };
    let result = result.and_then(|result| {
        settled?;
        if account.stopped || !evidence {
            Err(invalid("live accounting or provenance incomplete"))
        } else {
            Ok(result)
        }
    });
    match result {
        Ok(mut result) => {
            result["model_calls"] = json!(account.receipts.len());
            result["charged_microusd"] = json!(account.charged_microusd);
            result["incomplete_provider_responses"] = json!(
                account
                    .receipts
                    .iter()
                    .filter(|r| r.provenance.as_ref().is_some_and(|p| !p.response_complete))
                    .count()
            );
            log(ledger, "completed", result.clone())?;
            Ok(result)
        }
        Err(error) => {
            log(
                ledger,
                "failed",
                json!({"error":error.to_string(),"scores":null,"model_calls":account.receipts.len(),"charged_microusd":account.charged_microusd,"label":label(preliminary,mode,"failed live attempt","failed plumbing check")}),
            )?;
            Err(error)
        }
    }
}
pub(crate) fn verified_cache(
    root: &Path,
    policy: &Policy,
    name: &str,
    mode: Mode,
) -> Result<(PathBuf, String)> {
    let path = root.join("runs").join(speech_id(name));
    let verified = crate::ledger::verify(&path)?;
    let manifest: Manifest = files::json_file(&path.join("manifest.json"))?;
    if verified.terminal.as_deref() != Some("completed")
        || manifest.mode != mode
        || manifest.run_id != speech_id(name)
        || manifest.arm != "shared_speech"
        || manifest.registration_hash != policy.fixtures[name].registration_sha256
        || manifest.config["policy_sha256"] != policy.hash()?
        || manifest.config["fixture"] != name
    {
        return Err(invalid(
            "shared speech attempt is not complete for this policy",
        ));
    }
    let entries: Vec<crate::ledger::Entry> = files::lines(&path.join("events.jsonl"))?;
    let hash = entries
        .last()
        .and_then(|e| e.data["manifest_sha256"].as_str())
        .filter(|s| crate::live_plan::digest(s))
        .ok_or_else(|| invalid("speech attempt omitted cache pin"))?
        .to_owned();
    Ok((path.join("cache"), hash))
}
fn prior_attempts(root: &Path, policy: &Policy, run: &Run, mode: Mode) -> Result<()> {
    for arm in &policy.fixtures[&run.fixture].setup_order {
        for repetition in 1..=policy.repetitions {
            if arm == &run.arm && repetition == run.repetition {
                return Ok(());
            }
            let path = root
                .join("runs")
                .join(run_id(&run.fixture, arm, repetition)?);
            // Failed/incomplete attempts remain visible; they never become zeros or
            // vanish, and they cannot be retried under a new accounting identity.
            crate::ledger::verify(&path)?;
            let prior: Manifest = files::json_file(&path.join("manifest.json"))?;
            if prior.mode != mode
                || prior.run_id != run_id(&run.fixture, arm, repetition)?
                || prior.config["fixture"] != run.fixture
                || prior.registration_hash != policy.fixtures[&run.fixture].registration_sha256
                || prior.config["policy_sha256"] != policy.hash()?
                || prior.arm != *arm
                || prior.config["repetition"] != repetition
            {
                return Err(invalid("prior attempt differs from registered order"));
            }
        }
    }
    Err(invalid("run was not in registered order"))
}
pub fn execute(
    policy: &Policy,
    approval: Plan,
    root: &Path,
    run: &Run,
    fixture_transport: bool,
) -> Result<Value> {
    policy.check_approval(&approval, fixture_transport)?;
    runner_pin(policy)?;
    let (fixture_pin, adapter, id) = policy.admitted(run)?;
    let source = fixture(policy, &run.fixture, &run.fixture_directory)?;
    let corpus = &source.corpus;
    let executable_bytes = pin(&run.adapter_executable, &adapter.executable_sha256)?;
    let mode = if fixture_transport {
        Mode::Recorded
    } else {
        Mode::Live
    };
    let campaign = Arc::new(Mutex::new(Campaign::open(
        root,
        approval,
        adapter.budget_microusd,
    )?));
    private_dir(&root.join("runs"))?;
    prior_attempts(root, policy, run, mode)?;
    let (cache, cache_hash) = verified_cache(root, policy, &run.fixture, mode)?;
    let observed = crate::speech_cache::load_from_source(
        &cache,
        &source.files["events.jsonl"],
        &cache_hash,
        !fixture_transport,
    )?;
    let cached = crate::speech_cache::settings(&cache, &cache_hash)?;
    if serde_json::to_value(cached.config)? != serde_json::to_value(&policy.speech)?
        || cached.audio_manifest_hash != fixture_pin.audio_manifest_sha256
    {
        return Err(invalid("shared speech config differs from approved route"));
    }
    let output = root.join("runs").join(&id);
    let mut manifest = manifest(
        policy,
        &run.fixture,
        &source,
        id.clone(),
        &run.arm,
        adapter.budget_microusd,
        mode,
    )?;
    manifest.implementation_revision = adapter.revision.clone();
    manifest.transcript_hash = Some(cache_hash.clone());
    manifest.config["repetition"] = json!(run.repetition);
    manifest.config["adapter_sha256"] = json!(adapter.executable_sha256);
    manifest.config["partial_current_product"] = json!(run.arm == "lampos_today_partial");
    manifest.config["fixture_profile"] = json!(fixture_pin.profile);
    let ledger = Arc::new(Mutex::new(Ledger::create(&output, &manifest)?));
    save_source(&output, &source)?;
    let executable = output.join("adapter");
    save(&executable, &executable_bytes, 0o500)?;
    let executable = fs::canonicalize(executable)?;
    let http = if fixture_transport {
        live_http::Http::loopback_fixture(policy.chat.clone(), &output.join("provider"))
    } else {
        live_http::Http::from_environment(
            policy.chat.clone(),
            "LAMP_BENCH_CHAT_KEY",
            &output.join("provider"),
        )
    };
    let http = match http {
        Ok(http) => http,
        Err(e) => {
            log(
                &ledger,
                "failed",
                json!({"error":e.to_string(),"scores":null,"model_calls":0}),
            )?;
            return Err(e);
        }
    };
    let mut worker = worker(
        http,
        policy,
        adapter.budget_microusd,
        campaign,
        ledger.clone(),
        id.clone(),
    )?;
    let result = (|| {
        let state = output.join("state");
        fs::DirBuilder::new().mode(0o700).create(&state)?;
        let state = fs::canonicalize(state)?;
        let mut runner = Runner::spawn(&executable, &adapter.arguments, &state)?;
        let init=runner.execute(Request::new("init",Op::Init,json!({"run_id":id,"arm":run.arm,"state_dir":state,"owner_id":corpus.persona.id,"timezone":corpus.persona.timezone,"at_unix_ms":corpus.events[0].at_unix_ms,"config":{}})),&mut worker)?;
        if init.data["implementation_revision"] != adapter.revision {
            return Err(invalid(
                "adapter reported a different implementation revision",
            ));
        }
        log(
            &ledger,
            "initialized",
            json!({"outcome":init,"state_directory":"$RUN/state"}),
        )?;
        let result = Trial {
            corpus,
            observed: &observed,
            arm: &run.arm,
            mode,
            runner: &mut runner,
            worker: &mut worker,
        }
        .run(|kind, data| log(&ledger, kind, data))?;
        let mut result = result;
        if policy.preliminary {
            result["label"] = json!(label(
                policy.preliminary,
                mode,
                "live; provisional pending human audit",
                "plumbing check"
            ));
            result["preliminary"] = json!(true);
        }
        let closed = runner.execute(Request::new("close", Op::Close, json!({})), &mut worker)?;
        log(&ledger, "closed", json!({"outcome":closed}))?;
        drop(runner);
        Ok(result)
    })();
    finish(worker, &ledger, policy.preliminary, mode, result)
}
pub fn create_speech(
    policy: &Policy,
    approval: Plan,
    root: &Path,
    name: &str,
    source: &Path,
    audio: &Path,
    fixture_transport: bool,
) -> Result<Value> {
    policy.check_approval(&approval, fixture_transport)?;
    runner_pin(policy)?;
    let source = fixture(policy, name, source)?;
    let fixture_pin = &policy.fixtures[name];
    let audio_manifest = pin(
        &audio.join("audio.jsonl"),
        &fixture_pin.audio_manifest_sha256,
    )?;
    let campaign = Arc::new(Mutex::new(Campaign::open(
        root,
        approval,
        fixture_pin.speech_budget_microusd,
    )?));
    private_dir(&root.join("runs"))?;
    let mode = if fixture_transport {
        Mode::Recorded
    } else {
        Mode::Live
    };
    let id = speech_id(name);
    let output = root.join("runs").join(&id);
    let mut manifest = manifest(
        policy,
        name,
        &source,
        id.clone(),
        "shared_speech",
        fixture_pin.speech_budget_microusd,
        mode,
    )?;
    manifest.model = policy.speech.expected_model_arch.clone();
    manifest.config["purpose"] = json!("shared_speech");
    let ledger = Arc::new(Mutex::new(Ledger::create(&output, &manifest)?));
    save_source(&output, &source)?;
    let artifacts = output.join("provider");
    let speech = if fixture_transport {
        live_speech::Speech::loopback_fixture(policy.speech.clone(), &artifacts)
    } else {
        live_speech::Speech::from_environment(
            policy.speech.clone(),
            "LAMP_BENCH_SPEECH_KEY",
            &artifacts,
        )
    };
    let speech = match speech {
        Ok(s) => s,
        Err(e) => {
            log(
                &ledger,
                "failed",
                json!({"error":e.to_string(),"scores":null,"model_calls":0}),
            )?;
            return Err(e);
        }
    };
    let mut worker = worker(
        speech,
        policy,
        fixture_pin.speech_budget_microusd,
        campaign,
        ledger.clone(),
        id,
    )?;
    let result = crate::speech_cache::create_from_inputs(
        crate::speech_cache::Inputs {
            source_events: &source.files["events.jsonl"],
            audio_manifest: &audio_manifest,
            audio_directory: audio,
        },
        &artifacts,
        &output.join("cache"),
        policy.speech.clone(),
        fixture_transport,
        &mut worker,
    );
    finish(worker, &ledger, policy.preliminary, mode, result)
}
#[cfg(test)]
mod completion_tests {
    use super::*;
    use crate::model::{Call, Prices, Reply};
    use std::sync::atomic::{AtomicBool, Ordering};
    struct SlowDrop(Arc<AtomicBool>);
    impl Drop for SlowDrop {
        fn drop(&mut self) {
            std::thread::sleep(Duration::from_millis(50));
            self.0.store(true, Ordering::SeqCst);
        }
    }
    impl Provider for SlowDrop {
        fn invoke(&mut self, _: &Call, _: Instant) -> Result<Reply> {
            unreachable!()
        }
    }
    #[test]
    fn completed_run_joins_client_and_releases_its_owned_resources() {
        let path = std::env::temp_dir().join(format!("bench-completion-{}", std::process::id()));
        let manifest = Manifest {
            run_id: "completion-test".into(),
            mode: Mode::Recorded,
            arm: "none".into(),
            implementation_revision: "a".repeat(40),
            registration_revision: "b".repeat(40),
            registration_hash: hash(b"registration"),
            fixture_hash: hash(b"fixture"),
            transcript_hash: None,
            timezone: "Asia/Ho_Chi_Minh".into(),
            model: "offline".into(),
            config: json!({}),
            budget_microusd: 100,
            seed: 1,
        };
        let log = Arc::new(Mutex::new(Ledger::create(&path, &manifest).unwrap()));
        let dropped = Arc::new(AtomicBool::new(false));
        let worker = Worker::start(
            Client::new(
                SlowDrop(dropped.clone()),
                Prices {
                    source: "offline".into(),
                    date: "2026-11-01".into(),
                    input_microusd_per_million: 1,
                    output_microusd_per_million: 1,
                },
                100,
            )
            .unwrap(),
        )
        .unwrap();
        let outcome = finish(
            worker,
            &log,
            false,
            Mode::Recorded,
            Ok(json!({"scores":null})),
        );
        let joined = dropped.load(Ordering::SeqCst);
        drop(log);
        std::fs::remove_dir_all(&path).unwrap();
        outcome.unwrap();
        assert!(
            joined,
            "completed run returned before the client released its resources"
        );
    }
}
