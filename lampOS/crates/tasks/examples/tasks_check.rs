//! Standalone, account-free qualification. This executable impersonates the
//! scripted CLI in child processes; it never invokes the installed Harness.
//! Speech/confirmation are authored attestations, never physical playback.
#[path = "../tests/fixtures/harness_cli.rs"]
mod scripted;

use lamp_tasks::{
    harness::{Cli, CliConfig, EngineKind, Target},
    runtime::{Config, SystemClock, request, serve_with_adapter},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs, io,
    os::unix::{fs::DirBuilderExt, net::UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MACHINE: &str = "0123456789abcdef0123456789abcdef";
fn require(value: bool, message: &str) -> io::Result<()> {
    if value {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}
fn accepted(value: Value) -> io::Result<Value> {
    require(
        value["ok"] == true,
        &format!("local fixture request refused: {value}"),
    )?;
    Ok(value)
}
fn private_dir(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)
}
fn plan(home: &Path, value: &Value) -> io::Result<()> {
    fs::write(home.join("plan.json"), serde_json::to_vec(value)?)
}
fn normal_plan() -> Value {
    json!({"machines":{"default":true},"new":{"default":true},"show":{"default":true}})
}

struct Running {
    config: Config,
    home: PathBuf,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<io::Result<()>>>,
}
impl Running {
    fn new(root: &Path) -> io::Result<Self> {
        let home = root.join("home");
        private_dir(&home)?;
        plan(&home, &normal_plan())?;
        let config = Config {
            enabled: true,
            dir: root.join("data"),
            socket: root.join("r/p"),
            voice_socket: root.join("r/v"),
            bus_root: root.join("b"),
            targets: BTreeMap::from([(
                "coding".into(),
                Target {
                    machine_id: MACHINE.into(),
                    project: "qualification".into(),
                    engine: EngineKind::Codex,
                },
            )]),
            run_for: None,
            watch: false,
            progress: false,
            push_wait: false,
            close_told: false,
        };
        let mut running = Self {
            config,
            home,
            stop: Arc::new(AtomicBool::new(false)),
            handle: None,
        };
        running.start()?;
        Ok(running)
    }
    fn start(&mut self) -> io::Result<()> {
        self.stop.store(false, Ordering::Release);
        let config = self.config.clone();
        let stop = self.stop.clone();
        let cli_stop = stop.clone();
        let executable = std::env::current_exe()?;
        let home = self.home.clone();
        let clock = SystemClock::new(Some("tasks-check-authored-boot"))?;
        self.handle = Some(thread::spawn(move || {
            serve_with_adapter(config, clock, stop, move |session| {
                let mut config = CliConfig::new(executable);
                config.environment = BTreeMap::from([
                    ("HOME".into(), home.into_os_string()),
                    ("PATH".into(), "/usr/bin:/bin".into()),
                    ("LANG".into(), "C".into()),
                ]);
                config.cancel = Some(cli_stop);
                config.launch_timeout = Duration::from_secs(2);
                config.read_timeout = Duration::from_secs(2);
                Cli::new(config, session.into())
            })
        }));
        let start = Instant::now();
        loop {
            if self.call(false, json!({"op":"status"})).is_ok() {
                return Ok(());
            }
            if self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
                return self.stop().and(Err(io::Error::other(
                    "fixture service exited during startup",
                )));
            }
            if start.elapsed() > Duration::from_secs(5) {
                self.stop()?;
                return Err(io::Error::other("fixture startup deadline"));
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn stop(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            handle
                .join()
                .map_err(|_| io::Error::other("fixture service panicked"))??;
        }
        Ok(())
    }
    fn call(&self, trusted: bool, value: Value) -> io::Result<Value> {
        accepted(request(
            if trusted {
                &self.config.voice_socket
            } else {
                &self.config.socket
            },
            &value,
            Duration::from_millis(1500),
        )?)
    }
    fn status(&self) -> io::Result<Value> {
        self.call(false, json!({"op":"status"}))
    }
    fn guard(&self) -> io::Result<Value> {
        let s = self.status()?;
        Ok(
            json!({"store_id":s["store_id"],"revision":s["revision"],"request_id":format!("check-{}",s["revision"])}),
        )
    }
    fn prepare(&self) -> io::Result<Value> {
        self.call(false,json!({"op":"prepare","guard":self.guard()?,"target":"coding","text":"Qualification fixture: inspect the authored test."}))
    }
    fn confirm_request(&self, prepared: &Value) -> io::Result<Value> {
        Ok(
            json!({"op":"confirm","guard":self.guard()?,"id":prepared["id"],"prepared_digest":prepared["prepared_digest"],"session_id":self.status()?["summary"]["session_id"]}),
        )
    }
    fn wait(&self, id: &Value, predicate: impl Fn(&Value) -> bool) -> io::Result<Value> {
        let start = Instant::now();
        loop {
            let task = self.call(false, json!({"op":"get_task","id":id}))?;
            if predicate(&task) {
                return Ok(task);
            }
            require(
                start.elapsed() < Duration::from_secs(5),
                "fixture task transition deadline",
            )?;
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn calls(&self) -> io::Result<Vec<Value>> {
        match fs::read_to_string(self.home.join("calls.jsonl")) {
            Ok(text) => text
                .lines()
                .map(|line| serde_json::from_str(line).map_err(io::Error::other))
                .collect(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn elapsed(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
fn cycle(root: &Path) -> io::Result<Value> {
    let start = Instant::now();
    let mut running = Running::new(root)?;
    let startup = elapsed(start);
    let status = running.status()?;
    let store_id = status["store_id"].clone();
    let guard = running.guard()?;
    let start = Instant::now();
    let prepared=running.call(false,json!({"op":"prepare","guard":guard,"target":"coding","text":"Qualification fixture: inspect the authored test."}))?;
    let prepare = elapsed(start);
    require(running.calls()?.is_empty(), "prepare invoked CLI")?;
    let confirm = running.confirm_request(&prepared)?;
    let public = request(
        &running.config.socket,
        &confirm,
        Duration::from_millis(1500),
    )?;
    require(
        public["error"] == "untrusted",
        "public route confirmed a task",
    )?;
    let start = Instant::now();
    running.call(true, confirm.clone())?;
    let confirm_receipt = elapsed(start);
    running.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "accepted"
    })?;
    let confirm_to_created = elapsed(start);
    let observe = json!({"op":"observe","guard":running.guard()?,"id":prepared["id"]});
    let start = Instant::now();
    running.call(false, observe)?;
    let task = running.wait(&prepared["id"], |v| {
        v["observations"].as_array().is_some_and(|v| !v.is_empty())
    })?;
    let observe_to_stored = elapsed(start);
    let observation = task["observations"][0]["id"].clone();
    let result = running.call(
        false,
        json!({"op":"get_observation","id":prepared["id"],"observation_id":observation}),
    )?;
    require(
        result["data"]["answer"] == "Observed answer"
            && result["completion"] == "unverified"
            && result["speech"].is_null(),
        "observation became completion or speech",
    )?;
    let session = running.status()?["summary"]["session_id"].clone();
    let claim = json!({"op":"claim_speech","guard":running.guard()?,"id":prepared["id"],"observation_id":observation,"text":"The fixture supplied an observed answer; completion is unverified.","session_id":session});
    let start = Instant::now();
    let granted = running.call(true, claim.clone())?;
    let claim_time = elapsed(start);
    let duplicate = request(
        &running.config.voice_socket,
        &claim,
        Duration::from_millis(1500),
    )?;
    require(
        duplicate["error"] == "claim_uncertain",
        "duplicate claim granted playback again",
    )?;
    let ack = json!({"op":"ack_speech","guard":running.guard()?,"id":prepared["id"],"observation_id":observation,"claim_id":granted["claim_id"],"text_digest":granted["text_digest"],"session_id":session,"played":true});
    let start = Instant::now();
    running.call(true, ack)?;
    let ack_time = elapsed(start);
    let silent = UnixStream::connect(&running.config.socket)?;
    let start = Instant::now();
    running.status()?;
    let status_beside_silent = elapsed(start);
    drop(silent);
    running.stop()?;
    let start = Instant::now();
    running.start()?;
    let restart = elapsed(start);
    require(
        running.status()?["store_id"] == store_id,
        "restart changed store identity",
    )?;
    let old = request(
        &running.config.voice_socket,
        &confirm,
        Duration::from_millis(1500),
    )?;
    require(
        old["error"] == "stale_session",
        "old confirmation survived restart",
    )?;
    let result = running.call(
        false,
        json!({"op":"get_observation","id":prepared["id"],"observation_id":observation}),
    )?;
    require(
        result["speech"]["state"] == "spoken" && result["completion"] == "unverified",
        "restart lost speech evidence or invented completion",
    )?;
    running.call(
        false,
        json!({"op":"observe","guard":running.guard()?,"id":prepared["id"]}),
    )?;
    running.wait(&prepared["id"], |v| v["observations"][0]["seen_count"] == 2)?;
    let calls = running.calls()?;
    require(
        calls.iter().filter(|c| c["args"][0] == "new").count() == 1,
        "launch was repeated",
    )?;
    require(
        calls.iter().filter(|c| c["args"][0] == "show").count() == 2,
        "unexpected observation count",
    )?;
    require(
        calls.iter().all(|c| {
            c["environment"]["AI_AGENT"] == "lampos"
                && c["environment"]["HARNESS_AGENT_SESSION"] == store_id
        }),
        "caller identity changed across restart",
    )?;
    running.stop()?;
    Ok(
        json!({"startup_ms":startup,"prepare_receipt_ms":prepare,"confirm_receipt_ms":confirm_receipt,"confirm_to_scripted_created_ms":confirm_to_created,"observe_to_stored_ms":observe_to_stored,"speech_claim_ms":claim_time,"authored_playout_ack_ms":ack_time,"status_beside_silent_ms":status_beside_silent,"restart_ms":restart,"new_calls":1,"show_calls":2,"all_assertions_passed":true}),
    )
}

fn unresolved(root: &Path, pending: bool) -> io::Result<Value> {
    let mut running = Running::new(root)?;
    let prepared = running.prepare()?;
    let response = if pending {
        json!({"ok":true,"pending":true,"machineId":MACHINE,"creationId":"22222222-2222-4222-8222-222222222222","name":prepared["label"]}).to_string()
    } else {
        "{".into()
    };
    plan(
        &running.home,
        &json!({"machines":{"default":true},"new":{"stdout":response},"show":{"default":true}}),
    )?;
    running.call(true, running.confirm_request(&prepared)?)?;
    let state = if pending { "pending" } else { "uncertain" };
    running.wait(&prepared["id"], |v| v["task"]["dispatch"]["state"] == state)?;
    running.stop()?;
    running.start()?;
    running.wait(&prepared["id"], |v| v["task"]["dispatch"]["state"] == state)?;
    let refused = request(
        &running.config.socket,
        &json!({"op":"observe","guard":running.guard()?,"id":prepared["id"]}),
        Duration::from_millis(1500),
    )?;
    require(
        refused["error"] == "unresolved",
        "unresolved task acquired a guessed agent binding",
    )?;
    require(
        running
            .calls()?
            .iter()
            .filter(|c| c["args"][0] == "new")
            .count()
            == 1,
        "unresolved task was sent again",
    )?;
    running.stop()?;
    Ok(
        json!({"state":state,"new_calls":1,"restarted_without_resend":true,"observation_refused":true}),
    )
}

fn statistics(values: &[Value], field: &str) -> Value {
    let all: Vec<_> = values
        .iter()
        .map(|v| v[field].as_f64().expect("numeric measurement"))
        .collect();
    let mut warm = all[1..].to_vec();
    warm.sort_by(f64::total_cmp);
    let median = if warm.is_empty() {
        None
    } else {
        let mid = warm.len() / 2;
        Some(if warm.len().is_multiple_of(2) {
            (warm[mid - 1] + warm[mid]) / 2.0
        } else {
            warm[mid]
        })
    };
    json!({"count":all.len(),"first_ms":all[0],"warm_count":warm.len(),"warm_median_ms":median,"warm_max_ms":warm.last(),"all_ms":all})
}

fn qualify() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut parent = None;
    let mut samples = 8usize;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(arg) = args.next() {
        require(seen.insert(arg.clone()), "duplicate option")?;
        match arg.as_str() {
            "--scratch-parent" => {
                parent = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| io::Error::other("missing scratch parent"))?,
                ))
            }
            "--samples" => {
                samples = args
                    .next()
                    .ok_or_else(|| io::Error::other("missing samples"))?
                    .parse()
                    .map_err(io::Error::other)?
            }
            _ => {
                return Err(io::Error::other(
                    "usage: tasks_check --scratch-parent ABS [--samples 8]",
                ));
            }
        }
    }
    require((1..=100).contains(&samples), "samples must be 1..100")?;
    let parent = parent.ok_or_else(|| io::Error::other("--scratch-parent is required"))?;
    require(parent.is_absolute(), "scratch parent must be absolute")?;
    let parent = parent.canonicalize()?;
    let mut random = [0u8; 6];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let root = parent.join(format!("tasks-check-{suffix}"));
    require(
        root.join("c100/r/p").as_os_str().len() <= lamp_bus::MOST_SOCKET_PATH_BYTES,
        "scratch parent is too long for Unix sockets",
    )?;
    private_dir(&root)?;
    let start = Instant::now();
    let result = (|| -> io::Result<Value> {
        let mut cycles = Vec::new();
        for index in 0..samples {
            let dir = root.join(format!("c{index}"));
            private_dir(&dir)?;
            cycles.push(cycle(&dir)?);
        }
        let lost = root.join("malformed");
        private_dir(&lost)?;
        let lost = unresolved(&lost, false)?;
        let pending = root.join("pending");
        private_dir(&pending)?;
        let pending = unresolved(&pending, true)?;
        let mut stats = serde_json::Map::new();
        for field in [
            "startup_ms",
            "prepare_receipt_ms",
            "confirm_receipt_ms",
            "confirm_to_scripted_created_ms",
            "observe_to_stored_ms",
            "speech_claim_ms",
            "authored_playout_ack_ms",
            "status_beside_silent_ms",
            "restart_ms",
        ] {
            stats.insert(field.into(), statistics(&cycles, field));
        }
        Ok(
            json!({"all_assertions_passed":true,"unit":"milliseconds","samples":samples,"cycles":cycles,"statistics":stats,"malformed_reply":lost,"pending":pending,"elapsed_ms":elapsed(start),"platform":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"boundaries":"real local SQLite, sockets, process spawn and scripted JSON; fresh private store per cycle; confirmation, boot identity and played receipts authored; no account, remote execution, physical reboot or sound","scratch":root}),
        )
    })();
    match result {
        Ok(mut value) => {
            fs::remove_dir_all(&root)?;
            value["scratch_removed"] = json!(!root.exists());
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        Err(error) => Err(io::Error::other(format!(
            "{error}; private fixture evidence retained at {}",
            root.display()
        ))),
    }
}

fn main() -> io::Result<()> {
    if std::env::args()
        .nth(1)
        .is_some_and(|v| matches!(v.as_str(), "machines" | "new" | "show" | "__hold"))
    {
        scripted::main()
    } else {
        qualify()
    }
}
