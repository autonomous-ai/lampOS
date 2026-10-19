use lamp_voice_eval::{
    Result,
    annotations::{self, Annotations},
    canary::{self, CanaryResult},
    evaluate::{self, AttemptScore},
    fake::FakeBackend,
    import::{ImportOptions, import_trace},
    invalid,
    ledger::{create_run_directory, read_rows, unix_ms, write_new_json},
    physical::{
        fake_lamp, fixture_commands,
        mac::{
            FakePlayerFault, FakePreparedRoomPlayer, HumanPrompter, LampMode, ObserverPlayer,
            PhysicalBackend, PhysicalConfig, Player,
        },
        session::{self, RuntimeFlags, SessionOptions, VoiceRoute},
    },
    plan::{LoadedPlan, SERVE_PLAN},
    record::{AttemptRecord, StimulusSource},
    report,
    runner::{SuiteOptions, attempt_order, run_suite},
    stimulus::{AssetIndex, StimulusCatalog},
};
use serde::Deserialize;

mod observe_dry;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
};

const USAGE: &str = "usage:
  lamp-voice-eval observe-dry --store FILE --out NEW_DIR --key-file FILE [--base URL] [--model NAME]
      [--gap-seconds N] [--limit N]
  lamp-voice-eval list | validate | self-test [--plan FILE]
  lamp-voice-eval fake-run --out NEW_DIR [--profile ID] [--scenario ID]... [--repetitions N]
      [--seed N] [--no-shuffle] [--attempt-seed N] [--cache DIR --manifest FILE...] [--plan FILE]
  lamp-voice-eval evaluate RUN_DIR [--annotations FILE] [--plan FILE]
  lamp-voice-eval import-trace --events FILE --scenario ID --out NEW_DIR [--turn STEP=TURN|none]...
      [--room-metadata FILE | --room-receipt FILE --room-input NAME] [--room-independent]
      [--source loudspeaker_synthetic|direct_human] [--plan FILE]
  lamp-voice-eval annotation-template RUN_DIR NEW_FILE
  lamp-voice-eval assets --cache DIR --manifest FILE...
  lamp-voice-eval render-stimuli CACHE NEW_REPORT.json
  lamp-voice-eval physical-run --config FILE --cache DIR --manifest FILE... --out NEW_DIR
      [--scenario ID]... [--repetitions N] [--seed N] [--attempt-seed N] [--execute]
  lamp-voice-eval serve-offline --out NEW_DIR --cache DIR --manifest FILE... [--plan FILE]
      [--scenario ID] [--repetitions N] [--seed N] [--fake-profile ID] [--lamp-root NEW_ABS_DIR]
      [--fake-arg ARG]... [--player-fault N:late:MS|N:prepare|N:slow-prepare:MS]...
      [--recorder-fail-after-ms MS]
  lamp-voice-eval export --since HH:MM:SS --until HH:MM:SS --out NEW_DIR [--sessions DIR]
      [--journal JOURNAL_JSON] [--bus LAMP_BUS_RECORD]... [--date YYYY-MM-DD] [--zone system|ZONE]
      [--wall-offset-us N]
  lamp-voice-eval lamp-session --runtime LAMP_LIVE --work NEW_ABS_DIR --seconds N
      (--fixture-reply WAV --fixture-sha256 SHA | --provider-config FILE | --serve-config FILE)
      [--noise-suppression on|off] [--diagnostics] [--allowance-seconds N]";

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("lamp-voice-eval: {error}");
            std::process::exit(1);
        }
    }
}

struct Args {
    values: Vec<String>,
}
impl Args {
    fn take_flag(&mut self, flag: &str) -> bool {
        match self.values.iter().position(|v| v == flag) {
            Some(index) => {
                self.values.remove(index);
                true
            }
            None => false,
        }
    }
    fn take_all(&mut self, flag: &str) -> Result<Vec<String>> {
        let mut found = Vec::new();
        while let Some(index) = self.values.iter().position(|v| v == flag) {
            if index + 1 >= self.values.len() {
                return Err(invalid(&format!("{flag} needs a value")));
            }
            found.push(self.values.remove(index + 1));
            self.values.remove(index);
        }
        Ok(found)
    }
    fn take(&mut self, flag: &str) -> Result<Option<String>> {
        let mut all = self.take_all(flag)?;
        if all.len() > 1 {
            return Err(invalid(&format!("{flag} given more than once")));
        }
        Ok(all.pop())
    }
    fn positional(&mut self) -> Result<String> {
        if self.values.is_empty() || self.values[0].starts_with("--") {
            return Err(invalid(USAGE));
        }
        Ok(self.values.remove(0))
    }
    fn done(&self) -> Result<()> {
        if self.values.is_empty() {
            Ok(())
        } else {
            Err(invalid(&format!(
                "unexpected arguments {:?}\n{USAGE}",
                self.values
            )))
        }
    }
}

fn load(args: &mut Args) -> Result<(StimulusCatalog, LoadedPlan)> {
    let catalog = StimulusCatalog::load()?;
    let plan = match args.take("--plan")? {
        Some(path) => LoadedPlan::parse(&fs::read_to_string(path)?, &catalog)?,
        None => LoadedPlan::default_plan(&catalog)?,
    };
    Ok((catalog, plan))
}

fn suite_options(args: &mut Args, run_id: String) -> Result<SuiteOptions> {
    Ok(SuiteOptions {
        run_id,
        scenarios: args.take_all("--scenario")?,
        repetitions: args.take("--repetitions")?.map_or(Ok(1), |v| v.parse())?,
        seed: args.take("--seed")?.map_or(Ok(1), |v| v.parse())?,
        shuffle: !args.take_flag("--no-shuffle"),
        self_test: Value::Null,
        attempt_seed: args
            .take("--attempt-seed")?
            .map(|v| v.parse())
            .transpose()?,
    })
}

fn run_id(out: &Path) -> String {
    let name = out
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    if clean.is_empty() {
        format!("run-{}", unix_ms())
    } else {
        clean
    }
}

fn assets(args: &mut Args, catalog: &StimulusCatalog) -> Result<Option<AssetIndex>> {
    let cache = args.take("--cache")?;
    let manifests: Vec<PathBuf> = args
        .take_all("--manifest")?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    match (cache, manifests.is_empty()) {
        (None, true) => Ok(None),
        (Some(cache), false) => Ok(Some(AssetIndex::load(
            Path::new(&cache),
            &manifests,
            catalog,
        )?)),
        _ => Err(invalid("--cache and at least one --manifest go together")),
    }
}

fn run() -> Result<i32> {
    let mut all: Vec<String> = std::env::args().skip(1).collect();
    if all.is_empty() {
        return Err(invalid(USAGE));
    }
    let command = all.remove(0);
    match command.as_str() {
        "lamp-session" => {
            let options = SessionOptions::parse(&all)?;
            let stdout = std::io::stdout();
            return session::run(&options, std::io::stdin(), &mut stdout.lock());
        }
        "observe-dry" => return observe_dry::run(&all),
        "fake-lamp-voice" => return fake_lamp::run(&all),
        "fake-clock-lamp-voice" => return fixture_commands::runtime(all),
        "fake-clock-session" => return fixture_commands::relay(all),
        "fake-clock-serve-session" => return fixture_commands::serve_relay(all),
        "fake-clock-serve-runtime" => return fixture_commands::serve_runtime(all),
        "fake-clock-room-recorder" => return fixture_commands::recorder(all),
        "fake-clock-slow-transport" => return fixture_commands::slow_transport(all),
        "fake-room-recorder" => return fake_lamp::room_recorder(&all),
        "fake-slow-transport" => return fake_lamp::slow_transport(&all),
        _ => {}
    }
    let mut args = Args { values: all };
    match command.as_str() {
        "export" => {
            let options = lamp_voice_eval::export::ExportOptions {
                since: args.take("--since")?.ok_or_else(|| invalid(USAGE))?,
                until: args.take("--until")?.ok_or_else(|| invalid(USAGE))?,
                date: args.take("--date")?,
                sessions: args.take("--sessions")?.map(PathBuf::from),
                journal: args.take("--journal")?.map(PathBuf::from),
                bus: args
                    .take_all("--bus")?
                    .into_iter()
                    .map(PathBuf::from)
                    .collect(),
                zone: args.take("--zone")?.unwrap_or_else(|| "system".into()),
                wall_offset_us: args
                    .take("--wall-offset-us")?
                    .map(|value| value.parse())
                    .transpose()?,
                out: PathBuf::from(args.take("--out")?.ok_or_else(|| invalid(USAGE))?),
            };
            args.done()?;
            let summary = lamp_voice_eval::export::export(&options)?;
            println!("{summary}");
        }
        "list" | "validate" => {
            let (catalog, plan) = load(&mut args)?;
            args.done()?;
            if command == "validate" {
                println!(
                    "{}",
                    json!({"valid": true, "plan": plan.plan.id, "plan_sha256": plan.sha256,
                    "scenarios": plan.plan.scenarios.len(), "merged_catalog_sha256": catalog.merged_sha256,
                    "desk_catalog_file_sha256": catalog.desk_file_sha256,
                    "new_utterances_needing_render": catalog.extension_utterances})
                );
            } else {
                for scenario in &plan.plan.scenarios {
                    let steps: Vec<String> = scenario
                        .steps
                        .iter()
                        .map(|s| format!("{}:{:?}", s.id, s.expect))
                        .collect();
                    println!(
                        "{:<30} {:<20} {:<12} physical={:<5} {}",
                        scenario.id,
                        format!("{:?}", scenario.cohort),
                        format!("{:?}", scenario.provider),
                        scenario.physical,
                        steps.join(" -> ")
                    );
                }
            }
        }
        "fake-run" => {
            let (catalog, plan) = load(&mut args)?;
            let out = PathBuf::from(
                args.take("--out")?
                    .ok_or_else(|| invalid("--out NEW_DIR is required"))?,
            );
            let profile_id = args
                .take("--profile")?
                .unwrap_or_else(|| "v2-directed-current".into());
            let options = suite_options(&mut args, run_id(&out))?;
            let assets = assets(&mut args, &catalog)?;
            args.done()?;
            let profile = plan.profile(&profile_id)?.clone();
            let self_test = canary::run(&plan, &catalog)?;
            let mut options = options;
            options.self_test = serde_json::to_value(&self_test)?;
            create_run_directory(&out)?;
            let mut backend = FakeBackend::new(
                &profile_id,
                profile,
                plan.plan.fixed_reply.clone(),
                assets.as_ref(),
            );
            let records = run_suite(&plan, &catalog, &mut backend, &options, &out)?;
            let scores: Vec<AttemptScore> = records
                .iter()
                .map(|r| evaluate::score(r, plan.plan.answer_deadline_ms, None))
                .collect();
            let directory = write_evaluation(
                &out,
                &plan,
                &options.run_id,
                &records,
                &scores,
                Vec::new(),
                self_test.clone(),
            )?;
            print_summary(&scores, &directory);
            return Ok(trust_exit(&self_test));
        }
        "self-test" => {
            let (catalog, plan) = load(&mut args)?;
            args.done()?;
            let results = canary::run(&plan, &catalog)?;
            println!("{}", serde_json::to_string_pretty(&results)?);
            return Ok(trust_exit(&results));
        }
        "evaluate" => {
            let run = PathBuf::from(args.positional()?);
            let annotations = args
                .take("--annotations")?
                .map(|path| {
                    fs::read(path)
                        .map_err(Into::into)
                        .and_then(|b| Annotations::parse(&b))
                })
                .transpose()?;
            let (_, plan) = load(&mut args)?;
            args.done()?;
            let (records, problems, run_id) = read_records(&run, &plan)?;
            let scores: Vec<AttemptScore> = records
                .iter()
                .map(|r| evaluate::score(r, plan.plan.answer_deadline_ms, annotations.as_ref()))
                .collect();
            let self_test = canary::run(&plan, &StimulusCatalog::load()?)?;
            let directory = write_evaluation(
                &run,
                &plan,
                &run_id,
                &records,
                &scores,
                problems,
                self_test.clone(),
            )?;
            print_summary(&scores, &directory);
            return Ok(trust_exit(&self_test));
        }
        "import-trace" => {
            let (catalog, plan) = load(&mut args)?;
            let events = PathBuf::from(
                args.take("--events")?
                    .ok_or_else(|| invalid("--events is required"))?,
            );
            let scenario = args
                .take("--scenario")?
                .ok_or_else(|| invalid("--scenario is required"))?;
            let out = PathBuf::from(
                args.take("--out")?
                    .ok_or_else(|| invalid("--out NEW_DIR is required"))?,
            );
            let turn_map = args
                .take_all("--turn")?
                .iter()
                .map(|pair| {
                    let (step, turn) = pair
                        .split_once('=')
                        .ok_or_else(|| invalid("--turn takes STEP=TURN or STEP=none"))?;
                    let turn = match turn {
                        "none" => None,
                        turn => Some(turn.parse::<u64>()?),
                    };
                    Ok((step.to_owned(), turn))
                })
                .collect::<Result<Vec<_>>>()?;
            let room = args.take("--room-metadata")?.map(PathBuf::from);
            let receipt = args.take("--room-receipt")?.map(PathBuf::from);
            let input = args.take("--room-input")?;
            if receipt.is_some() != input.is_some() {
                return Err(invalid(
                    "--room-receipt FILE needs --room-input NAME, and the reverse",
                ));
            }
            let room_independent = args.take_flag("--room-independent");
            let source = match args.take("--source")?.as_deref() {
                None => StimulusSource::Unknown,
                Some("loudspeaker_synthetic") => StimulusSource::LoudspeakerSynthetic,
                Some("direct_human") => StimulusSource::DirectHuman,
                Some(_) => {
                    return Err(invalid("--source is loudspeaker_synthetic or direct_human"));
                }
            };
            args.done()?;
            create_run_directory(&out)?;
            let run_id = run_id(&out);
            let record = import_trace(
                &plan,
                &catalog,
                &ImportOptions {
                    run_id: run_id.clone(),
                    events_path: &events,
                    scenario,
                    turn_map,
                    room_metadata: room.as_deref(),
                    room_receipt: receipt.as_deref().zip(input.as_deref()),
                    room_independent,
                    source,
                },
                &out,
            )?;
            let scores = vec![evaluate::score(&record, plan.plan.answer_deadline_ms, None)];
            let records = vec![record];
            let self_test = canary::run(&plan, &catalog)?;
            let directory = write_evaluation(
                &out,
                &plan,
                &run_id,
                &records,
                &scores,
                Vec::new(),
                self_test.clone(),
            )?;
            print_summary(&scores, &directory);
            return Ok(trust_exit(&self_test));
        }
        "annotation-template" => {
            let run = PathBuf::from(args.positional()?);
            let destination = PathBuf::from(args.positional()?);
            let (_, plan) = load(&mut args)?;
            args.done()?;
            let (records, _, run_id) = read_records(&run, &plan)?;
            let template = annotations::template(&run_id, &records);
            write_new_json(&destination, &serde_json::to_value(&template)?)?;
            println!(
                "{}",
                json!({"attempts": template.attempts.len(), "template": destination})
            );
        }
        "assets" => {
            let catalog = StimulusCatalog::load()?;
            let assets = assets(&mut args, &catalog)?
                .ok_or_else(|| invalid("--cache and --manifest are required"))?;
            args.done()?;
            let resolved: BTreeSet<&str> = assets.scenes().map(|a| a.scene.as_str()).collect();
            let missing: Vec<&str> = catalog
                .catalog
                .scenes
                .iter()
                .map(|s| s.id.as_str())
                .filter(|id| !resolved.contains(id))
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"resolved": resolved, "missing": missing}))?
            );
        }
        "render-stimuli" => {
            // Renders only objects absent from the cache; existing verified speech
            // and mixes are reused by lamp-sound-fixtures's content-addressed keys.
            let cache = PathBuf::from(args.positional()?);
            let manifest = PathBuf::from(args.positional()?);
            args.done()?;
            if manifest.exists() {
                return Err(invalid("use a new report path for every render"));
            }
            let catalog = StimulusCatalog::load()?;
            let cache = lamp_sound_fixtures::Cache::new(cache)?;
            let report = lamp_sound_fixtures::render(
                &catalog.catalog,
                &cache,
                &mut lamp_sound_fixtures::MacSay,
            )?;
            lamp_sound_fixtures::save_report(&manifest, &report)?;
            println!(
                "{}",
                json!({"generated_speech": report.generated_speech, "reused_speech": report.reused_speech,
                "generated_scenes": report.generated_scenes, "reused_scenes": report.reused_scenes, "manifest": manifest})
            );
        }
        "physical-run" => {
            let (catalog, plan) = load(&mut args)?;
            let config_path = PathBuf::from(
                args.take("--config")?
                    .ok_or_else(|| invalid("--config FILE is required"))?,
            );
            let out = PathBuf::from(
                args.take("--out")?
                    .ok_or_else(|| invalid("--out NEW_DIR is required"))?,
            );
            let options = suite_options(&mut args, run_id(&out))?;
            let assets = assets(&mut args, &catalog)?
                .ok_or_else(|| invalid("--cache and --manifest are required"))?;
            let execute = args.take_flag("--execute");
            args.done()?;
            let file: PhysicalFile = serde_json::from_slice(&fs::read(&config_path)?)?;
            let config = file.into_config()?;
            let mut backend = PhysicalBackend::new(config.0, &assets, config.1);
            if !execute {
                use lamp_voice_eval::runner::Backend;
                let order = attempt_order(&plan, &options)?;
                let rows: Vec<Value> = order
                    .iter()
                    .map(|(repetition, id)| {
                        let scenario = plan.scenario(id).expect("validated");
                        json!({"repetition": repetition, "scenario": id,
                            "status": backend.unsupported(scenario).map_or("would run".to_owned(), |r| format!("withheld: {r}"))})
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "dry_run": true,
                        "note": "nothing was started; add --execute after the operator readiness checks",
                        "backend": backend.describe(),
                        "attempts": rows,
                    }))?
                );
                return Ok(0);
            }
            // Lamp time is scarce: never run a physical suite with an evaluator
            // that fails to detect its own injected failures.
            let self_test = canary::run(&plan, &catalog)?;
            if !canary::all_detected(&self_test) {
                eprintln!("{}", serde_json::to_string_pretty(&self_test)?);
                return Err(invalid(
                    "evaluator self-test failed; refusing to start a physical run",
                ));
            }
            let mut options = options;
            options.self_test = serde_json::to_value(&self_test)?;
            create_run_directory(&out)?;
            let records = run_suite(&plan, &catalog, &mut backend, &options, &out)?;
            let scores: Vec<AttemptScore> = records
                .iter()
                .map(|r| evaluate::score(r, plan.plan.answer_deadline_ms, None))
                .collect();
            let directory = write_evaluation(
                &out,
                &plan,
                &options.run_id,
                &records,
                &scores,
                Vec::new(),
                self_test,
            )?;
            print_summary(&scores, &directory);
        }
        "serve-offline" => {
            let catalog = StimulusCatalog::load()?;
            let source = match args.take("--plan")? {
                Some(path) => fs::read_to_string(path)?,
                None => SERVE_PLAN.to_owned(),
            };
            let plan = LoadedPlan::parse(&source, &catalog)?;
            let out = PathBuf::from(
                args.take("--out")?
                    .ok_or_else(|| invalid("--out NEW_DIR is required"))?,
            );
            let mut options = suite_options(&mut args, run_id(&out))?;
            let assets = assets(&mut args, &catalog)?
                .ok_or_else(|| invalid("--cache and --manifest are required"))?;
            let profile = match args.take("--fake-profile")? {
                Some(profile) => profile,
                None => plan
                    .plan
                    .profiles
                    .keys()
                    .next()
                    .cloned()
                    .ok_or_else(|| invalid("the plan has no fake profile"))?,
            };
            // Short by default: Unix socket paths are limited to ~100 bytes.
            let lamp_root = PathBuf::from(
                args.take("--lamp-root")?
                    .unwrap_or_else(|| format!("/tmp/lve-serve-{}", std::process::id())),
            );
            let fake_args = args.take_all("--fake-arg")?;
            let faults = args
                .take_all("--player-fault")?
                .iter()
                .map(|fault| parse_player_fault(fault))
                .collect::<Result<Vec<_>>>()?;
            let recorder_fail = args.take("--recorder-fail-after-ms")?;
            args.done()?;
            let scenario = match (options.scenarios.as_slice(), plan.plan.scenarios.as_slice()) {
                ([one], _) => one.clone(),
                ([], [only]) => only.id.clone(),
                _ => return Err(invalid("serve-offline runs exactly one --scenario")),
            };
            options.scenarios = vec![scenario.clone()];
            if !lamp_root.is_absolute() {
                return Err(invalid("--lamp-root must be absolute"));
            }
            create_run_directory(&out)?;
            fs::DirBuilder::new().mode(0o700).create(&lamp_root)?;
            let plan_copy = lamp_root.join("plan.json");
            fs::write(&plan_copy, &source)?;
            let exe = std::env::current_exe()?.display().to_string();
            let room = lamp_root.join("room.sock");
            let mut lamp_command = vec![
                exe.clone(),
                "lamp-session".into(),
                "--runtime".into(),
                exe.clone(),
            ];
            let mut prefix = vec![
                "fake-lamp-voice".to_owned(),
                "--fake-plan".into(),
                plan_copy.display().to_string(),
                "--fake-scenario".into(),
                scenario,
                "--fake-profile".into(),
                profile,
                "--fake-room".into(),
                room.display().to_string(),
            ];
            prefix.extend(fake_args);
            for arg in prefix {
                lamp_command.extend(["--runtime-prefix-arg".to_owned(), arg]);
            }
            let room_recorder = recorder_fail.map(|ms| {
                vec![
                    exe.clone(),
                    "fake-room-recorder".into(),
                    "--out".into(),
                    "{out}".into(),
                    "--seconds".into(),
                    "{seconds}".into(),
                    "--fail-after-ms".into(),
                    ms,
                ]
            });
            let config = PhysicalConfig {
                lamp_command,
                lamp_work_root: lamp_root.display().to_string(),
                mode: LampMode::Serve {
                    provider_config: "/nonexistent/fake-serve-provider.json".into(),
                },
                noise_suppression: "on".into(),
                diagnostics: false,
                room_recorder,
                room_independent: false,
                allowance_seconds: 10,
                ring_channel_ceiling: None,
                stimulus_source: lamp_voice_eval::record::StimulusSource::LoudspeakerSynthetic,
                runtime_flags: Default::default(),
            };
            let player = FakePreparedRoomPlayer {
                room,
                faults,
                prepared: 0,
            };
            options.self_test = json!({"skipped": "offline fake serve run; no Lamp time is spent"});
            let mut backend = PhysicalBackend::new(config, &assets, Box::new(player));
            let records = run_suite(&plan, &catalog, &mut backend, &options, &out)?;
            let scores: Vec<AttemptScore> = records
                .iter()
                .map(|r| evaluate::score(r, plan.plan.answer_deadline_ms, None))
                .collect();
            let directory = write_evaluation(
                &out,
                &plan,
                &options.run_id,
                &records,
                &scores,
                Vec::new(),
                Vec::new(),
            )?;
            print_summary(&scores, &directory);
            eprintln!("fake serve work directory kept at {}", lamp_root.display());
        }
        _ => return Err(invalid(USAGE)),
    }
    Ok(0)
}

/// `N:late:MS` (the Nth prepared start copies MS after its grant) or
/// `N:prepare` (the Nth preparation fails).
fn parse_player_fault(text: &str) -> Result<(usize, FakePlayerFault)> {
    let parts: Vec<&str> = text.split(':').collect();
    let number: usize = parts
        .first()
        .ok_or_else(|| invalid("empty --player-fault"))?
        .parse()?;
    let fault = match parts.as_slice() {
        [_, "late", ms] => FakePlayerFault::LateFirstCopy {
            delay_ms: ms.parse()?,
        },
        [_, "prepare"] => FakePlayerFault::PrepareFails,
        [_, "slow-prepare", ms] => FakePlayerFault::SlowPrepare {
            delay_ms: ms.parse()?,
        },
        _ => {
            return Err(invalid(
                "--player-fault is N:late:MS, N:prepare or N:slow-prepare:MS",
            ));
        }
    };
    if number == 0 {
        return Err(invalid("--player-fault counts preparations from 1"));
    }
    Ok((number, fault))
}

/// Reviewable physical-run configuration. Contains no credentials: the Lamp
/// command reuses the operator's existing authorized SSH setup.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PhysicalFile {
    lamp_command: Vec<String>,
    lamp_work_root: String,
    fixture_reply: Option<String>,
    fixture_sha256: Option<String>,
    provider_config: Option<String>,
    /// One `lamp-voice serve` runtime per attempt (Gemini provider config).
    #[serde(default)]
    serve_config: Option<String>,
    #[serde(default = "default_noise")]
    noise_suppression: String,
    #[serde(default)]
    diagnostics: bool,
    room_recorder: Option<Vec<String>>,
    #[serde(default)]
    room_independent: bool,
    /// Required for loudspeaker replay; unused for direct human speech.
    #[serde(default)]
    output_device: Option<String>,
    /// "loudspeaker_synthetic" (default) or "direct_human".
    #[serde(default = "default_source")]
    stimulus_source: StimulusSource,
    /// Label (not a name) for the person speaking in direct-human runs.
    #[serde(default)]
    human_speaker: Option<String>,
    #[serde(default = "default_allowance")]
    allowance_seconds: u16,
    #[serde(default)]
    ring_channel_ceiling: Option<u16>,
    /// lamp-voice runtime policy, forwarded unchanged when set (`directed` and
    /// serve only): for example the installed pilot's `v1-baseline` or `duplex`.
    #[serde(default)]
    microphone: Option<String>,
    #[serde(default)]
    turn_taking: Option<String>,
    #[serde(default)]
    reference_clock: Option<String>,
    #[serde(default)]
    reference_recovery: Option<String>,
    #[serde(default)]
    playout_start_ms: Option<u16>,
    #[serde(default)]
    provider_continuation: Option<String>,
    /// Opt-in end-of-turn experiment (`smart-turn`); absent is the default.
    #[serde(default)]
    endpoint: Option<String>,
    /// The voice route of a `directed` or `serve` run: "second" (the
    /// default, the owner's 19 Oct 19:3x rule) or "live", only for a
    /// comparison the first builder names on #1712.
    #[serde(default)]
    voice_route: Option<String>,
    /// `LAMP_*` settings for the runtime's environment, copied from the
    /// lamp's voice unit: the second route needs `LAMP_VOICE_SECOND_CONFIG`
    /// and takes its `LAMP_SECOND_` switches from here.
    #[serde(default)]
    runtime_env: std::collections::BTreeMap<String, String>,
}
fn default_source() -> StimulusSource {
    StimulusSource::LoudspeakerSynthetic
}
fn default_noise() -> String {
    "on".into()
}
fn default_allowance() -> u16 {
    40
}
impl PhysicalFile {
    fn runtime_flags(&self) -> Result<RuntimeFlags> {
        let mut flags = RuntimeFlags::default();
        let playout = self.playout_start_ms.map(|ms| ms.to_string());
        for (flag, value) in [
            ("--microphone", &self.microphone),
            ("--turn-taking", &self.turn_taking),
            ("--reference-clock", &self.reference_clock),
            ("--reference-recovery", &self.reference_recovery),
            ("--playout-start-ms", &playout),
            ("--provider-continuation", &self.provider_continuation),
            ("--endpoint", &self.endpoint),
        ] {
            if let Some(value) = value {
                flags.take(flag, value)?;
            }
        }
        if !flags.is_empty() {
            if self.fixture_reply.is_some() {
                return Err(invalid(
                    "runtime policy fields need provider_config or serve_config",
                ));
            }
            flags.validate(
                &self.noise_suppression,
                self.ring_channel_ceiling,
                self.diagnostics && self.serve_config.is_none(),
            )?;
        }
        if self.fixture_reply.is_some() {
            if self.voice_route.is_some() || !self.runtime_env.is_empty() {
                return Err(invalid(
                    "voice_route and runtime_env need provider_config or serve_config",
                ));
            }
            return Ok(flags);
        }
        // Named in every directed or serve run, so its record says which.
        flags.take(
            VoiceRoute::FLAG,
            self.voice_route
                .as_deref()
                .unwrap_or(VoiceRoute::Second.as_str()),
        )?;
        for (name, value) in &self.runtime_env {
            flags.take("--runtime-env", &format!("{name}={value}"))?;
        }
        flags.validate_route()?;
        Ok(flags)
    }
    fn into_config(self) -> Result<(PhysicalConfig, Box<dyn Player>)> {
        let runtime_flags = self.runtime_flags()?;
        if self.lamp_command.is_empty() || !self.lamp_work_root.starts_with('/') {
            return Err(invalid(
                "lamp_command must be nonempty and lamp_work_root absolute",
            ));
        }
        let player: Box<dyn Player> = match self.stimulus_source {
            StimulusSource::LoudspeakerSynthetic => {
                let output_device = self
                    .output_device
                    .ok_or_else(|| invalid("loudspeaker replay needs output_device"))?;
                lamp_observer::playback::validate_output_name(&output_device)?;
                Box::new(ObserverPlayer { output_device })
            }
            StimulusSource::DirectHuman => Box::new(HumanPrompter {
                speaker_label: self.human_speaker.unwrap_or_else(|| "person-1".into()),
            }),
            _ => {
                return Err(invalid(
                    "stimulus_source must be loudspeaker_synthetic or direct_human",
                ));
            }
        };
        let mode = match (
            self.fixture_reply,
            self.fixture_sha256,
            self.provider_config,
            self.serve_config,
        ) {
            (Some(reply), Some(sha256), None, None) => LampMode::Fixture { reply, sha256 },
            (None, None, Some(provider_config), None) => LampMode::Directed { provider_config },
            (None, None, None, Some(provider_config)) => {
                // Serve starts every stimulus by a prepared grant.
                if self.stimulus_source != StimulusSource::LoudspeakerSynthetic {
                    return Err(invalid(
                        "serve_config needs loudspeaker replay through the prepared player",
                    ));
                }
                if self.diagnostics {
                    return Err(invalid("serve_config cannot request raw audio diagnostics"));
                }
                LampMode::Serve { provider_config }
            }
            _ => {
                return Err(invalid(
                    "give fixture_reply + fixture_sha256, provider_config or serve_config",
                ));
            }
        };
        Ok((
            PhysicalConfig {
                lamp_command: self.lamp_command,
                lamp_work_root: self.lamp_work_root,
                mode,
                noise_suppression: self.noise_suppression,
                diagnostics: self.diagnostics,
                room_recorder: self.room_recorder,
                room_independent: self.room_independent,
                allowance_seconds: self.allowance_seconds,
                ring_channel_ceiling: self.ring_channel_ceiling,
                stimulus_source: self.stimulus_source,
                runtime_flags,
            },
            player,
        ))
    }
}

/// Every attempt row of a run, including planned attempts that never finished.
fn read_records(
    run: &Path,
    plan: &LoadedPlan,
) -> Result<(Vec<AttemptRecord>, Vec<String>, String)> {
    let (rows, truncated) = read_rows(run)?;
    let mut problems = Vec::new();
    if truncated {
        problems.push("the final ledger line was truncated".to_owned());
    }
    let start = rows
        .first()
        .filter(|r| r["row"] == "run_start")
        .ok_or_else(|| invalid("ledger has no run_start"))?;
    if start["plan_sha256"] != plan.sha256 {
        return Err(invalid(
            "this run used a different plan; pass the same --plan",
        ));
    }
    let run_id = start["run_id"].as_str().unwrap_or_default().to_owned();
    let mut records = Vec::new();
    let mut planned = Vec::new();
    for row in &rows {
        match row["row"].as_str() {
            Some("attempt_planned") => {
                planned.push(row["attempt_id"].as_str().unwrap_or_default().to_owned())
            }
            Some("attempt_finished") => records.push(serde_json::from_value::<AttemptRecord>(
                row["record"].clone(),
            )?),
            _ => {}
        }
    }
    for id in planned {
        if !records.iter().any(|r| r.attempt_id == id) {
            problems.push(format!(
                "attempt {id} was planned but never finished (runner crash or kill)"
            ));
        }
    }
    if rows.last().is_none_or(|r| r["row"] != "run_end") {
        problems.push("the run has no run_end row".into());
    }
    Ok((records, problems, run_id))
}

fn write_evaluation(
    run: &Path,
    plan: &LoadedPlan,
    run_id: &str,
    records: &[AttemptRecord],
    scores: &[AttemptScore],
    problems: Vec<String>,
    self_test: Vec<CanaryResult>,
) -> Result<PathBuf> {
    let parent = run.join("evaluations");
    if !parent.exists() {
        create_run_directory(&parent)?;
    }
    let directory = parent.join(unix_ms().to_string());
    create_run_directory(&directory)?;
    let report = report::build(
        run_id,
        &plan.plan.id,
        &plan.sha256,
        scores,
        problems,
        self_test,
    );
    write_new_json(
        &directory.join("evaluation.json"),
        &serde_json::to_value(&report)?,
    )?;
    let attempts = directory.join("attempts");
    create_run_directory(&attempts)?;
    for (record, score) in records.iter().zip(scores) {
        let mut file = lamp_voice_eval::ledger::new_private_file(
            &attempts.join(format!("{}.md", record.attempt_id)),
        )?;
        std::io::Write::write_all(
            &mut file,
            report::attempt_markdown(record, score).as_bytes(),
        )?;
    }
    let markdown = report::markdown(&report, &plan.plan.targets);
    let mut file = lamp_voice_eval::ledger::new_private_file(&directory.join("report.md"))?;
    std::io::Write::write_all(&mut file, markdown.as_bytes())?;
    Ok(directory)
}

/// One JSON line. Exit status 0 only means the run and its evaluation were
/// written; it never certifies room audio or acoustic acceptance. Room
/// recordings are counted from the recorder's terminal report.
fn print_summary(scores: &[AttemptScore], directory: &Path) {
    let mut counts = std::collections::BTreeMap::new();
    let mut rooms = std::collections::BTreeMap::new();
    for score in scores {
        *counts
            .entry(format!("{:?}", score.outcome).to_lowercase())
            .or_insert(0) += 1;
        let room = match score.room_recording {
            evaluate::RoomRecording::NotApplicable => continue,
            evaluate::RoomRecording::Absent => "absent",
            evaluate::RoomRecording::Invalid { .. } => "invalid",
            evaluate::RoomRecording::Verified { .. } => "verified",
        };
        *rooms.entry(room).or_insert(0) += 1;
    }
    let acoustic = scores
        .iter()
        .flat_map(|s| &s.acoustic)
        .filter(|a| a.measurement().is_some())
        .count();
    println!(
        "{}",
        json!({"attempts": scores.len(), "outcomes": counts, "room_recordings": rooms,
            "acoustic_measurements": acoustic, "report": directory.join("report.md")})
    );
}

/// Exit code 2 when the evaluator failed its self-test: the report exists but
/// is marked untrusted.
fn trust_exit(self_test: &[CanaryResult]) -> i32 {
    if canary::all_detected(self_test) {
        0
    } else {
        eprintln!("lamp-voice-eval: evaluator self-test failed; the report is marked UNTRUSTED");
        2
    }
}
