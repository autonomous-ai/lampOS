//! Scripted local executable fixtures. No installed Harness, daemon, account,
//! remote agent, credentials or provider is invoked by these tests.
use lamp_tasks::harness::{
    AccountMachine, CLOSE_BUDGET, CLOSE_READ_TIMEOUT, Cli, CliConfig, Closed, Ended, EngineKind,
    EngineRow, JoinedOn, Launch, LaunchOutcome, Listed, MOST_ABOUT_BYTES, MOST_LISTED, MOST_WAITED,
    PS_STDOUT_LIMIT, SessionMachines, SignIn, Target, WATCH_LINE_LIMIT, WatchEnd, WatchEvent,
    WatchLine, iso_unix_ms, parse_auth_here, parse_auth_status, parse_engines, parse_machines,
    parse_ps, parse_wait, parse_watch_line, this_computer,
};
use lamp_tasks::runtime::{
    HarnessSwitches, NEW_FOLDER_FILE, SESSION_INDEX_FILE, targets_from_machines,
};
use lamp_tasks::session_index;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const MACHINE: &str = "0123456789abcdef0123456789abcdef";
const AGENT: &str = "11111111-1111-4111-8111-111111111111";
const CREATION: &str = "22222222-2222-4222-8222-222222222222";
/// This computer's own id, as Harness keeps it while no account is signed
/// in (openharness `computerId()`, a UUID): never an account machine's.
const COMPUTER: &str = "9d14f36b-e73a-4acb-9922-0bbb39cbc22e";

/// One test with processes at a time (#1569 row 18). On macOS a child that
/// another test thread spawns while this one makes a child's pipes can take
/// them before they are close-on-exec, and keep them until it exits: the
/// harness then finds them open at its cleanup (`cleanup_incomplete`,
/// `pipes_open`). Beside a thread spawning `sleep 1` in a loop, 5 of 10 runs
/// of this file failed so; without it, none. Linux makes pipes close-on-exec
/// at once, and CI runs each test in a process of its own.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

thread_local! {
    /// The Rigs this thread holds, and the turn while it holds any.
    static TURN: std::cell::RefCell<(usize, Option<std::sync::MutexGuard<'static, ()>>)> =
        const { std::cell::RefCell::new((0, None)) };
}

/// This test's turn ([`ONE_AT_A_TIME`]), held by each of its Rigs: a test
/// may make several, and the turn ends with the last.
struct Turn;
impl Turn {
    fn take() -> Self {
        TURN.with(|turn| {
            let mut turn = turn.borrow_mut();
            if turn.0 == 0 {
                // A test that failed while holding it leaves it poisoned; the
                // next one still runs alone.
                turn.1 = Some(
                    ONE_AT_A_TIME
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                );
            }
            turn.0 += 1;
        });
        Self
    }
}
impl Drop for Turn {
    fn drop(&mut self) {
        TURN.with(|turn| {
            let mut turn = turn.borrow_mut();
            turn.0 -= 1;
            if turn.0 == 0 {
                turn.1 = None;
            }
        });
    }
}

struct Rig {
    root: tempfile::TempDir,
    config: CliConfig,
    /// The last field, so the turn ends after the rest of the Rig is gone.
    _turn: Turn,
}
impl Rig {
    fn new() -> Self {
        let turn = Turn::take();
        let root = tempfile::Builder::new()
            .prefix("tasks-harness-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let config = CliConfig {
            executable: fixture(),
            // The production read bound. The fixture is a fresh process
            // per call, and on a loaded host its first start alone has
            // taken over a second (the fourth builder's run of 2026-10-15,
            // #698). Each timeout case sets its own short limit.
            launch_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(10),
            environment: BTreeMap::from([
                ("HOME".into(), root.path().as_os_str().into()),
                ("PATH".into(), OsString::from("/usr/bin:/bin")),
                ("LANG".into(), OsString::from("C")),
            ]),
            cancel: None,
            as_person: false,
            folder_missing: false,
            engines_ready: false,
            route: false,
            reply_named: false,
            resume_closed: false,
            restart_daemon: false,
            absolute_folder: false,
            new_folder: None,
            new_folder_only: None,
            session_index: None,
            journal: None,
            calls: None,
            session_machines: None,
            health_after_start: false,
            openclaw_gateway: false,
            user_runtime_root: root.path().join("run-user"),
        };
        let rig = Self {
            root,
            config,
            _turn: turn,
        };
        rig.plan(
            json!({"machines":{"default":true},"new":{"default":true},"show":{"default":true},"reply":{"default":true}}),
        );
        rig
    }
    /// Blocks until the held descendant has exited, then proves it was
    /// killed rather than left to finish its five-second hold.
    fn assert_descendant_killed(&self) {
        let lock = fs::File::open(self.root.path().join("descendant.lock")).unwrap();
        lock.lock().unwrap();
        assert!(!self.root.path().join("descendant.finished").exists());
    }
    fn plan(&self, plan: Value) {
        use std::io::Write;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(self.root.path().join("plan.json"))
            .unwrap()
            .write_all(serde_json::to_string(&plan).unwrap().as_bytes())
            .unwrap();
    }
    fn new_step(&self, step: Value) {
        self.plan(json!({"machines":{"default":true},"new":step,"show":{"default":true},"reply":{"default":true}}));
    }
    fn show_step(&self, step: Value) {
        self.plan(json!({"show":step}));
    }
    fn cli(&self) -> Cli {
        Cli::new(self.config.clone(), "stable-session_123".into()).unwrap()
    }
    fn calls(&self) -> Vec<Value> {
        match fs::read_to_string(self.root.path().join("calls.jsonl")) {
            Ok(text) => text
                .split_inclusive('\n')
                .filter(|line| line.ends_with('\n'))
                .map(|line| serde_json::from_str(line).unwrap())
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("fixture call log: {error}"),
        }
    }
    fn wait_operation(&self, operation: &str) {
        let until = Instant::now() + Duration::from_secs(2);
        while !self.calls().iter().any(|call| call["args"][0] == operation) {
            assert!(Instant::now() < until, "fixture did not enter {operation}");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn copied_binary(&mut self) {
        let copy = self.root.path().join("fixture");
        // `install` writes the copy in its own process. A write descriptor
        // held by this test process leaks into a child that another test
        // thread forks meanwhile, and Linux then refuses to execute the
        // copy with ETXTBSY until that child has exec'd.
        let installed = std::process::Command::new("install")
            .args(["-m", "0700"])
            .arg(&self.config.executable)
            .arg(&copy)
            .status()
            .unwrap();
        assert!(installed.success(), "install fixture: {installed}");
        self.config.executable = copy;
        // A newly copied image can incur the host's first-execution checks.
        // These identity tests use the production read bound, not a cold-start
        // performance assertion. Dedicated timeout cases inject shorter limits.
        self.config.read_timeout = Duration::from_secs(10);
        self.config.launch_timeout = Duration::from_secs(10);
    }
}

/// Cargo's fixture binary with group and other write cleared. Cargo writes it
/// under the builder's umask, and under 0002 it is group-writable, which `Cli`
/// rightly refuses. Clearing the bits in place keeps the built image, so the
/// tests do not pay a fresh copy's first-execution checks.
fn fixture() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_lamp-tasks-harness-fixture"));
    let mode = fs::metadata(&path).unwrap().permissions().mode();
    if mode & 0o022 != 0 {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode & !0o022)).unwrap();
    }
    path
}

fn launch() -> Launch {
    Launch {
        name: "lamp-task-001".into(),
        prompt: "Inspect the authored fixture".into(),
        target: Target {
            machine_id: MACHINE.into(),
            project: "fixture-project".into(),
            engine: EngineKind::Codex,
        },
        session: None,
    }
}
fn created() -> Value {
    json!({"ok":true,"machineId":MACHINE,"agent":{"id":AGENT,"name":"lamp-task-001","engine":"codex","permissionMode":"auto","bypassPermission":true}})
}
fn shown() -> Value {
    json!({"id":AGENT,"state":"idle","ask":"An older unrelated question","answer":"Observed output only","truncated":false,"tools":2})
}
fn step(reply: Value) -> Value {
    json!({"stdout":format!("{reply}\n")})
}
fn uncertain(code: &str) -> LaunchOutcome {
    LaunchOutcome::Uncertain { code: code.into() }
}
fn not_sent(code: &str) -> LaunchOutcome {
    LaunchOutcome::NotSent { code: code.into() }
}

#[test]
fn launch_uses_exact_argv_auto_mode_null_stdin_and_stable_generic_identity() {
    let rig = Rig::new();
    rig.plan(json!({"machines":{"default":true,"stdin_eof":true},"new":{"default":true,"stdin_eof":true}}));
    let mut request = launch();
    request.prompt = "  literal '$HOME' $(touch never) ; --mode full\nnext line  ".into();
    assert_eq!(
        rig.cli().launch(&request),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    let calls = rig.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["args"], json!(["machines", "--json"]));
    assert_eq!(
        calls[1]["args"],
        json!([
            "new",
            "codex",
            "--machine",
            MACHINE,
            "fixture-project",
            "--name",
            "lamp-task-001",
            "--mode",
            "auto",
            "--json",
            "--task",
            request.prompt
        ])
    );
    for call in calls {
        let environment = call["environment"].as_object().unwrap();
        assert_eq!(environment.len(), 5);
        assert_eq!(environment["AI_AGENT"], "lampos");
        assert_eq!(environment["HARNESS_AGENT_SESSION"], "stable-session_123");
        for forbidden in [
            "TMUX_PANE",
            "CLAUDECODE",
            "CLAUDE_CODE_SESSION_ID",
            "CODEX_THREAD_ID",
        ] {
            assert!(!environment.contains_key(forbidden));
        }
    }
    assert!(!rig.root.path().join("never").exists());
}

#[test]
fn as_the_person_the_call_carries_no_agent_mark_and_is_otherwise_the_same() {
    let mut rig = Rig::new();
    rig.config.as_person = true;
    rig.plan(json!({"machines":{"default":true,"stdin_eof":true},"new":{"default":true,"stdin_eof":true}}));
    assert_eq!(
        rig.cli().launch(&launch()),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    let calls = rig.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1]["args"][0], "new");
    for call in calls {
        let environment = call["environment"].as_object().unwrap();
        assert_eq!(environment.len(), 3, "{environment:?}");
        assert!(!environment.contains_key("AI_AGENT"));
        assert!(!environment.contains_key("HARNESS_AGENT_SESSION"));
    }
}

#[test]
fn claude_and_explicit_folder_are_preserved_without_shell_expansion() {
    let rig = Rig::new();
    let mut request = launch();
    request.target.engine = EngineKind::Claude;
    request.target.project = "~/code/my project".into();
    assert!(matches!(
        rig.cli().launch(&request),
        LaunchOutcome::Created { .. }
    ));
    assert_eq!(rig.calls()[1]["args"][1], "claude");
    assert_eq!(rig.calls()[1]["args"][4], "~/code/my project");
}

/// OpenCode, as Harness 0.4.2's `new --help` names it, in auto mode like the
/// others. Only a receipt that says auto mode binds the task; one that does
/// not (what a real OpenCode receipt says is not yet seen) leaves it
/// uncertain, never sent again.
/// #1854: Devin, the terminal program on the computer, is asked for by name
/// with no `--mode` (it has none: Harness leaves it out and refuses one), and
/// its receipt is taken with no mode and no bypass, or a mode that does not
/// approve on its own; never one claiming `auto` or `full`, or a bypass.
#[test]
fn devin_is_asked_for_by_name_with_no_mode_and_its_receipt_claims_none() {
    let rig = Rig::new();
    let mut request = launch();
    request.target.engine = EngineKind::Devin;
    assert_eq!(
        rig.cli().launch(&request),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    let args = rig.calls()[1]["args"].as_array().unwrap().clone();
    assert_eq!(args[1], "devin");
    assert!(!args.iter().any(|arg| arg == "--mode"), "{args:?}");
    assert_eq!(args[7], "--json");
    assert_eq!(args.len(), 8, "{args:?}");
    let receipt = |mode: Value, bypass: Option<Value>| {
        let mut agent = json!({"id":AGENT,"name":"lamp-task-001","engine":"devin"});
        if !mode.is_null() {
            agent["permissionMode"] = mode;
        }
        if let Some(bypass) = bypass {
            agent["bypassPermission"] = bypass;
        }
        json!({"ok":true,"machineId":MACHINE,"agent":agent})
    };
    for (mode, bypass) in [
        (Value::Null, None),
        (Value::Null, Some(json!(false))),
        (Value::Null, Some(Value::Null)),
        (json!("default"), Some(json!(false))),
    ] {
        rig.new_step(step(receipt(mode.clone(), bypass.clone())));
        assert!(
            matches!(rig.cli().launch(&request), LaunchOutcome::Created { .. }),
            "{mode} {bypass:?}"
        );
    }
    for (mode, bypass) in [
        (json!("auto"), Some(json!(true))),
        (json!("full"), Some(json!(false))),
        (Value::Null, Some(json!(true))),
    ] {
        rig.new_step(step(receipt(mode.clone(), bypass.clone())));
        assert_eq!(
            rig.cli().launch(&request),
            uncertain("invalid_reply"),
            "{mode} {bypass:?}"
        );
    }
}

/// Devin can't start with a first message (Harness 0.5.6, 29 Oct), so its
/// task is typed in once Harness reads it idle: `new` with no `--task`,
/// `show` until idle, then `harness reply` with the task (openharness#2157).
#[test]
fn devins_task_is_typed_in_with_reply_once_it_is_idle() {
    let rig = Rig::new();
    let mut request = launch();
    request.target.engine = EngineKind::Devin;
    let seen = |state: &str| {
        step(json!({"id":AGENT,"state":state,"ask":null,"answer":"","truncated":false,"tools":0}))
    };
    rig.plan(json!({
        "machines":{"default":true},"new":{"default":true},
        "show":{"each":[seen("starting"), seen("working"), seen("idle")]},
        "reply":{"default":true}
    }));
    assert_eq!(
        rig.cli().launch(&request),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    let calls = rig.calls();
    let verbs: Vec<&str> = calls
        .iter()
        .map(|call| call["args"][0].as_str().unwrap())
        .collect();
    assert_eq!(verbs, ["machines", "new", "show", "show", "show", "reply"]);
    assert!(
        !calls[1]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == "--task")
    );
    assert_eq!(
        calls[5]["args"],
        json!([
            "reply",
            "--machine",
            MACHINE,
            AGENT,
            "--json",
            "--",
            "Inspect the authored fixture"
        ])
    );
    // Other engines still get their task with `new`, and no reply.
    let rig = Rig::new();
    assert!(matches!(
        rig.cli().launch(&launch()),
        LaunchOutcome::Created { .. }
    ));
    let calls = rig.calls();
    assert_eq!(calls.len(), 2);
    assert!(
        calls[1]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == "--task")
    );
}

/// #6226, lamp-c0a4: OpenClaw's first prompt is lost when its Gateway's
/// handshake times out. After `new`, `show` is read: asked already (`ask`
/// set), nothing is typed; idle with no ask, the task is typed in once
/// with `harness reply`.
#[test]
fn openclaws_lost_first_prompt_is_typed_in_once_and_a_taken_one_is_left() {
    let mut request = launch();
    request.target.engine = EngineKind::Openclaw;
    let seen = |state: &str, ask: Value| {
        step(json!({"id":AGENT,"state":state,"ask":ask,"answer":"","truncated":false,"tools":0}))
    };
    let verbs = |rig: &Rig| -> Vec<String> {
        rig.calls()
            .iter()
            .map(|call| call["args"][0].as_str().unwrap().to_owned())
            .collect()
    };
    // Taken: the prompt arrived, no reply.
    let rig = Rig::new();
    rig.plan(json!({
        "machines":{"default":true},"new":{"default":true},
        "show":{"each":[seen("working", json!("Inspect the authored fixture"))]},
        "reply":{"default":true}
    }));
    assert_eq!(
        rig.cli().launch(&request),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    assert_eq!(verbs(&rig), ["machines", "new", "show"]);
    let new = rig.calls()[1]["args"].as_array().unwrap().clone();
    assert!(new.iter().any(|arg| arg == "--task"), "{new:?}");
    // Lost: idle with no ask, typed in once.
    let rig = Rig::new();
    rig.plan(json!({
        "machines":{"default":true},"new":{"default":true},
        "show":{"each":[seen("starting", Value::Null), seen("idle", Value::Null)]},
        "reply":{"default":true}
    }));
    assert_eq!(
        rig.cli().launch(&request),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    assert_eq!(verbs(&rig), ["machines", "new", "show", "show", "reply"]);
    assert_eq!(
        rig.calls()[4]["args"],
        json!([
            "reply",
            "--machine",
            MACHINE,
            AGENT,
            "--json",
            "--",
            "Inspect the authored fixture"
        ])
    );
}

/// A Devin whose task couldn't be typed in was still started, so it is
/// uncertain, never sent again, with Harness's reason.
#[test]
fn a_devin_whose_task_is_not_typed_in_is_uncertain_with_the_reason() {
    let mut request = launch();
    request.target.engine = EngineKind::Devin;
    let refused = |code: &str| json!({"stdout":format!("{}\n", json!({"error":code,"detail":"…","code":code,"next":[]})),"exit":5});
    for (reply, code) in [
        (refused("REPLY_OFF"), "reply_off"),
        (refused("NOT_RUNNING"), "reply_not_running"),
        (
            json!({"stdout":format!("{}\n", json!({"error":"NOT_IDLE","detail":"…"})),"exit":1}),
            "reply_not_idle",
        ),
        (
            step(json!({"ok":false,"name":"lamp-task-001","id":AGENT,"state":"failed"})),
            "reply_failed",
        ),
    ] {
        let rig = Rig::new();
        rig.plan(json!({"machines":{"default":true},"new":{"default":true},"show":{"default":true},"reply":reply}));
        assert_eq!(rig.cli().launch(&request), uncertain(code), "{code}");
    }
    // Stopped before it was idle: nothing is typed.
    let rig = Rig::new();
    rig.plan(json!({"machines":{"default":true},"new":{"default":true},
        "show":step(json!({"id":AGENT,"state":"stopped","ask":null,"answer":"","truncated":false,"tools":0}))}));
    assert_eq!(
        rig.cli().launch(&request),
        uncertain("stopped_before_message")
    );
    assert!(!rig.calls().iter().any(|call| call["args"][0] == "reply"));
    // Never idle within the launch's bound: nothing is typed.
    let mut rig = Rig::new();
    rig.config.launch_timeout = Duration::from_millis(1500);
    rig.plan(json!({"machines":{"default":true},"new":{"default":true},
        "show":step(json!({"id":AGENT,"state":"working","ask":null,"answer":"","truncated":false,"tools":0}))}));
    assert_eq!(rig.cli().launch(&request), uncertain("never_idle"));
    assert!(!rig.calls().iter().any(|call| call["args"][0] == "reply"));
    // A `show` that fails to run ends it under its own code, at once.
    let mut rig = Rig::new();
    rig.config.read_timeout = Duration::from_millis(300);
    rig.plan(json!({"machines":{"default":true},"new":{"default":true},
        "show":{"delay_ms":3000,"default":true},"reply":{"default":true}}));
    let began = Instant::now();
    let outcome = rig.cli().launch(&request);
    assert!(
        matches!(outcome, LaunchOutcome::Uncertain { ref code } if code != "never_idle"),
        "{outcome:?}"
    );
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "{:?}",
        began.elapsed()
    );
}

/// A Harness before the fresh-Devin fix (0.5.6) reads a Devin that has done
/// nothing yet as `needs-input`. Nothing is asked of it, so the reply is
/// tried at once, and Harness's answer stands: never 60 s of looks (the
/// Reviewer-lead's P2 on #4374). One that has asked something waits.
#[test]
fn a_fresh_devin_read_as_needs_input_is_replied_to_at_once() {
    let mut request = launch();
    request.target.engine = EngineKind::Devin;
    let shown = |ask: Value, tools: u64| {
        step(
            json!({"id":AGENT,"state":"needs-input","ask":ask,"answer":"","truncated":false,"tools":tools}),
        )
    };
    let rig = Rig::new();
    rig.plan(json!({"machines":{"default":true},"new":{"default":true},"show":shown(Value::Null, 0),"reply":{"default":true}}));
    let began = Instant::now();
    assert_eq!(
        rig.cli().launch(&request),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    assert!(began.elapsed() < Duration::from_secs(5));
    let verbs: Vec<String> = rig
        .calls()
        .iter()
        .map(|call| call["args"][0].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(verbs, ["machines", "new", "show", "reply"]);
    // Harness refusing it as not idle ends it at once, under that code.
    let rig = Rig::new();
    let refused =
        json!({"stdout":format!("{}\n", json!({"error":"NOT_IDLE","detail":"…"})),"exit":1});
    rig.plan(json!({"machines":{"default":true},"new":{"default":true},"show":shown(Value::Null, 0),"reply":refused}));
    assert_eq!(rig.cli().launch(&request), uncertain("reply_not_idle"));
    // Asked something (a question or a permission), or it has worked: it isn't fresh, and nothing is typed.
    for (ask, tools) in [(json!("Trust this folder?"), 0), (Value::Null, 3)] {
        let mut rig = Rig::new();
        rig.config.launch_timeout = Duration::from_millis(1500);
        rig.plan(json!({"machines":{"default":true},"new":{"default":true},"show":shown(ask.clone(), tools)}));
        assert_eq!(
            rig.cli().launch(&request),
            uncertain("never_idle"),
            "{ask} {tools}"
        );
        assert!(!rig.calls().iter().any(|call| call["args"][0] == "reply"));
    }
}

/// `serve --route`: Harness's router is asked first, with `--send`. A
/// session it chose and typed into is the task's agent (`joined`), with no
/// `new`; new work, or a router that can't or won't decide, is `new` as
/// before; a busy pick is `agent_busy`, never new; an answer it can't read
/// after it may have typed is uncertain. Off, nothing is routed.
#[test]
fn with_route_the_router_is_asked_first_and_its_pick_is_the_tasks_agent() {
    let verbs = |rig: &Rig| -> Vec<String> {
        rig.calls()
            .iter()
            .map(|call| call["args"][0].as_str().unwrap().to_owned())
            .collect()
    };
    let routed = |answer: Value| {
        let mut rig = Rig::new();
        rig.config.route = true;
        rig.plan(json!({"machines":{"default":true},"new":{"default":true},"route":answer}));
        let outcome = rig.cli().launch(&launch());
        (outcome, verbs(&rig), rig)
    };
    let sent = json!({"decided":"session","id":AGENT,"name":"Fix login","machine":"desk-pc","confidence":0.82,"reason":"same work","sent":{"state":"queued"}});
    let (outcome, called, rig) = routed(step(sent));
    assert_eq!(
        outcome,
        LaunchOutcome::Joined {
            agent_id: AGENT.into(),
            session: None,
        }
    );
    assert_eq!(called, ["machines", "route"]);
    assert_eq!(
        rig.calls()[1]["args"],
        json!([
            "route",
            "--machine",
            MACHINE,
            "--json",
            "--send",
            "--",
            "Inspect the authored fixture"
        ])
    );
    // New work: `new` as before.
    let (outcome, called, _) = routed(json!({"default":true}));
    assert!(
        matches!(outcome, LaunchOutcome::Created { .. }),
        "{outcome:?}"
    );
    assert_eq!(called, ["machines", "route", "new"]);
    // Busy: not sent, and no new session for it.
    let busy =
        json!({"decided":"session","id":AGENT,"name":"Fix login","sent":{"error":"NOT_IDLE"}});
    let (outcome, called, _) = routed(step(busy));
    assert_eq!(outcome, not_sent("agent_busy"));
    assert_eq!(called, ["machines", "route"]);
    let busy = json!({"stdout":format!("{}\n", json!({"error":"NOT_IDLE","detail":"…"})),"exit":1});
    assert_eq!(routed(busy).0, not_sent("agent_busy"));
    // Can't or won't decide, or its pick is gone: new work, nothing typed.
    for (stdout, exit) in [
        (
            json!({"error":"ROUTER_UNAVAILABLE","detail":"Nothing was decided"}),
            1,
        ),
        (
            json!({"decided":"session","id":AGENT,"sent":{"error":"FAILED","detail":"its machine refused it"}}),
            0,
        ),
        (
            json!({"error":"REPLY_OFF","detail":"…","code":"REPLY_OFF"}),
            5,
        ),
        (json!({"error":"NOT_YOURS","detail":"…"}), 5),
        (
            json!({"decided":"session","id":AGENT,"sent":{"error":"NOT_RUNNING"}}),
            0,
        ),
        (
            json!({"decided":"session","id":AGENT,"sent":{"state":"failed"}}),
            1,
        ),
    ] {
        let (outcome, called, _) = routed(json!({"stdout":format!("{stdout}\n"),"exit":exit}));
        assert!(
            matches!(outcome, LaunchOutcome::Created { .. }),
            "{stdout}: {outcome:?}"
        );
        assert_eq!(called, ["machines", "route", "new"], "{stdout}");
    }
    // A send that ended unconfirmed (openharness#2252), or the same codes
    // bare from a route before it: the words may be there, so uncertain.
    for stdout in [
        json!({"decided":"session","id":AGENT,"sent":{"error":"UNCONFIRMED","detail":"…","uncertain":true}}),
        json!({"decided":"session","id":AGENT,"sent":{"error":"DISCONNECTED","detail":"…","uncertain":true}}),
        json!({"error":"INVALID_RESPONSE","detail":"…"}),
        json!({"error":"CANCELLED","detail":"…"}),
    ] {
        let (outcome, called, _) = routed(json!({"stdout":format!("{stdout}\n"),"exit":1}));
        assert_eq!(outcome, uncertain("route_uncertain"), "{stdout}");
        assert_eq!(called, ["machines", "route"], "{stdout}");
    }
    // Its machine's own code under `sent`, with nothing typed: new.
    let (outcome, called, _) = routed(step(
        json!({"decided":"session","id":AGENT,"sent":{"error":"SESSION_GONE","detail":"…"}}),
    ));
    assert!(
        matches!(outcome, LaunchOutcome::Created { .. }),
        "{outcome:?}"
    );
    assert_eq!(called, ["machines", "route", "new"]);
    // A session chosen, and an answer that can't say whether it was typed.
    for stdout in [
        "{\"decided\":\"session\",\"id\":\"11111111-1111-4111-8111-111111111111\"}\n",
        "not json\n",
    ] {
        let (outcome, called, _) = routed(json!({"stdout":stdout}));
        assert_eq!(outcome, uncertain("route_uncertain"), "{stdout}");
        assert_eq!(called, ["machines", "route"], "{stdout}");
    }
    // Off: no route.
    let rig = Rig::new();
    assert!(matches!(
        rig.cli().launch(&launch()),
        LaunchOutcome::Created { .. }
    ));
    assert_eq!(verbs(&rig), ["machines", "new"]);
}

/// One `harness ps --json` row, as Harness gives it.
fn ps_row(id: &str, name: &str, state: &str, folder: Option<&str>) -> Value {
    json!({"name":name,"id":id,"agent":"Claude Code","machine":"desk-pc","state":state,
        "folder":folder,"branch":null,"startedBy":"person","yours":false,"self":false,
        "lastActive":"2026-11-09T05:00:00.000Z","asks":[],"about":"older work"})
}

fn worked_row(id: &str, name: &str, state: &str, asks: &[&str], about: &str) -> Value {
    let mut row = ps_row(id, name, state, Some("/home/person/harnesses/lamp-tasks"));
    row["asks"] = json!(asks);
    row["about"] = json!(about);
    row
}

/// A join with how its session is known left out: what the tests of which
/// session was found compare.
fn bare(outcome: LaunchOutcome) -> LaunchOutcome {
    match outcome {
        LaunchOutcome::Joined { agent_id, .. } => LaunchOutcome::Joined {
            agent_id,
            session: None,
        },
        outcome => outcome,
    }
}

/// #5535: the sessions are listed within the launch's bound, not the read
/// bound. On lamp-c0a4 on 9 Nov, `ps --all` of the person's Mac took up to
/// 10 s, and a hand-over said yes to twice ended `sessions_timeout`.
#[test]
fn with_reply_named_a_slow_session_list_waits_for_the_launch_bound() {
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.config.route = true;
    rig.config.engines_ready = true;
    rig.config.read_timeout = Duration::from_secs(3);
    rig.config.launch_timeout = Duration::from_secs(12);
    let rows = json!([ps_row(AGENT, "docs", "idle", None)]);
    let mut ps = step(json!({"you":null,"machine":"desk-pc","harnesses":rows,"next":[]}));
    ps["delay_ms"] = json!(5000);
    rig.plan(json!({
        "machines":{"default":true},"new":{"default":true},"route":{"default":true},
        "ps":ps,
        "reply":step(json!({"ok":true,"name":"x","id":AGENT,"message":"m1","state":"queued"}))
    }));
    let mut request = launch();
    request.session = Some("docs".into());
    assert_eq!(
        bare(rig.cli().launch(&request)),
        LaunchOutcome::Joined {
            agent_id: AGENT.into(),
            session: None,
        }
    );
}

/// #5535, the Reviewer-lead's P3 on #5784 at 71bf964baa: a session list
/// that leaves the reply under a second of the launch bound is
/// `sessions_timeout`, and nothing is typed, never a reply cut short.
#[test]
fn with_reply_named_a_list_that_leaves_no_time_types_nothing() {
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.config.route = true;
    rig.config.engines_ready = true;
    rig.config.read_timeout = Duration::from_secs(3);
    rig.config.launch_timeout = Duration::from_millis(5600);
    let rows = json!([ps_row(AGENT, "docs", "idle", None)]);
    let mut ps = step(json!({"you":null,"machine":"desk-pc","harnesses":rows,"next":[]}));
    ps["delay_ms"] = json!(5000);
    rig.plan(json!({
        "machines":{"default":true},"new":{"default":true},"route":{"default":true},
        "ps":ps,
        "reply":step(json!({"ok":true,"name":"x","id":AGENT,"message":"m1","state":"queued"}))
    }));
    let mut request = launch();
    request.session = Some("docs".into());
    assert_eq!(
        rig.cli().launch(&request),
        LaunchOutcome::NotSent {
            code: "sessions_timeout".into()
        }
    );
    assert!(rig.calls().iter().all(|call| call["args"][0] != "reply"));
}

/// #5535, the owner on 9 Nov: a person remembers what a session worked on,
/// never its name ("lamp-b1287be5…") or id. With no name or folder fitting,
/// the session is found by its work (`asks`, `about`); two as good are
/// asked about, never guessed, a live one beside a stopped one too; words
/// that only say "which one" find nothing.
#[test]
fn with_reply_named_a_session_is_found_by_what_it_worked_on() {
    let pick = |session: &str, rows: Value| {
        let mut rig = Rig::new();
        rig.config.reply_named = true;
        rig.config.route = true;
        rig.config.engines_ready = true;
        let id = rows[0]["id"].as_str().unwrap().to_owned();
        rig.plan(json!({
            "machines":{"default":true},"new":{"default":true},"route":{"default":true},
            "ps":step(json!({"you":null,"machine":"desk-pc","harnesses":rows,"next":[]})),
            "reply":step(json!({"ok":true,"name":"x","id":id,"message":"m1","state":"queued"}))
        }));
        let mut request = launch();
        request.session = Some(session.into());
        let outcome = bare(rig.cli().launch(&request));
        let replied_to = rig
            .calls()
            .iter()
            .find(|call| call["args"][0] == "reply")
            .map(|call| call["args"][3].as_str().unwrap().to_owned());
        (outcome, replied_to)
    };
    let books = worked_row(
        AGENT,
        "lamp-b1287be53f02104bd6a17de348f1afde",
        "idle",
        &["Write a short book program."],
        "I wrote books.py, a short command-line program for keeping a reading list.",
    );
    let weather = worked_row(
        OTHER,
        "lamp-e8a0b789c7047e25824836509849649b",
        "idle",
        &["Research free weather APIs and build a small weather app."],
        "I built weather.py, which shows tomorrow's forecast.",
    );
    let rows = json!([books.clone(), weather.clone()]);
    for said in [
        "the book program one",
        "the reading list",
        "the session that wrote books.py",
    ] {
        assert_eq!(
            pick(said, rows.clone()),
            (
                LaunchOutcome::Joined {
                    agent_id: AGENT.into(),
                    session: None,
                },
                Some(AGENT.into())
            ),
            "{said}"
        );
    }
    let weather_first = json!([weather.clone(), books.clone()]);
    assert_eq!(
        pick("the weather app", weather_first.clone()).0,
        LaunchOutcome::Joined {
            agent_id: OTHER.into(),
            session: None,
        }
    );
    // Nothing of its work, or only words that say "which one".
    assert_eq!(
        pick("the billing dashboard", rows.clone()),
        (not_sent("session_not_found"), None)
    );
    assert_eq!(
        pick("the one from before", rows.clone()),
        (not_sent("session_not_found"), None)
    );
    // Two sessions as good: asked about.
    let twin = worked_row(
        OTHER,
        "lamp-cc366a4c7b85ee72036f1f5418cda0d5",
        "idle",
        &["Add a book search."],
        "I added search to the book program.",
    );
    assert_eq!(
        pick("the book program", json!([books.clone(), twin])),
        (not_sent("session_ambiguous"), None)
    );
    // Busy: said busy, nothing typed.
    let busy = worked_row(
        AGENT,
        "lamp-b1287be53f02104bd6a17de348f1afde",
        "working",
        &["Write a short book program."],
        "",
    );
    assert_eq!(
        pick("the book program", json!([busy])),
        (not_sent("session_busy"), None)
    );
    // A live one and a stopped one that both fit are asked about: the one
    // meant may be the stopped one (the Reviewer-lead's P2 on #5784 at
    // 71bf964baa), and nothing is typed into the live one.
    let old = worked_row(
        OTHER,
        "lamp-7f8a8dcb047b88eeca3ea3b8b634b420",
        "stopped",
        &["Write a book program for my reading list."],
        "I wrote a reading list program.",
    );
    assert_eq!(
        pick(
            "the book program reading list",
            json!([books.clone(), old.clone()])
        ),
        (not_sent("session_ambiguous"), None)
    );
    assert_eq!(
        pick("the book program", json!([old])),
        (not_sent("session_stopped"), None)
    );
    // Every word of the work, not half (Reviewer-three's P2 on #5784 at
    // 3d90e56736): no payroll work, nothing sent to the weather session.
    let dashboard = worked_row(
        AGENT,
        "lamp-a17b",
        "idle",
        &["Build a weather dashboard"],
        "I built the weather dashboard",
    );
    assert_eq!(
        pick("the payroll dashboard one", json!([dashboard.clone()])),
        (not_sent("session_not_found"), None)
    );
    assert_eq!(
        pick("the weather dashboard one", json!([dashboard])).0,
        LaunchOutcome::Joined {
            agent_id: AGENT.into(),
            session: None,
        }
    );
    // A word with an ending is the word; a longer word is not (the
    // Reviewer-lead's P2 on #5784 at de9bebdb41).
    let tests = worked_row(
        AGENT,
        "lamp-c3d4",
        "idle",
        &["Fix the failing tests"],
        "I fixed two tests in the notebook.",
    );
    assert_eq!(
        pick("the test fix", json!([tests])).0,
        LaunchOutcome::Joined {
            agent_id: AGENT.into(),
            session: None,
        }
    );
    let testament = worked_row(
        AGENT,
        "lamp-e5f6",
        "idle",
        &["Summarize the testament"],
        "I read the notebook.",
    );
    assert_eq!(
        pick("the test note", json!([testament])),
        (not_sent("session_not_found"), None)
    );
    // Reviewer-three's P2 at 71bf964baa: a two-letter word takes no
    // ending, so "us" is never "user"; "user" itself still joins.
    let user_tax = worked_row(
        AGENT,
        "lamp-a7b8",
        "idle",
        &["Update user tax settings"],
        "I changed the user tax settings.",
    );
    assert_eq!(
        pick("the US tax session", json!([user_tax.clone()])),
        (not_sent("session_not_found"), None)
    );
    assert_eq!(
        pick("the user tax session", json!([user_tax])).0,
        LaunchOutcome::Joined {
            agent_id: AGENT.into(),
            session: None,
        }
    );
}

/// #5535, the Reviewer-lead's P2 on #5784 at 71bf964baa: the session a
/// named hand-over joined is kept with it, so the voice can say which one
/// took the work and a wrong join is heard at once. Its own name, never a
/// lamp-made `lamp-<hex>` one, else its folder's last part when it has no
/// name, and the first thing it was asked, cut at a word. A routed join
/// keeps none.
#[test]
fn with_reply_named_the_joined_session_is_kept_by_its_name_and_first_ask() {
    let joined = |session: &str, rows: Value| -> Value {
        let mut rig = Rig::new();
        rig.config.reply_named = true;
        rig.config.route = true;
        rig.config.engines_ready = true;
        let id = rows[0]["id"].as_str().unwrap().to_owned();
        rig.plan(json!({
            "machines":{"default":true},"new":{"default":true},"route":{"default":true},
            "ps":step(json!({"you":null,"machine":"desk-pc","harnesses":rows,"next":[]})),
            "reply":step(json!({"ok":true,"name":"x","id":id,"message":"m1","state":"queued"}))
        }));
        let mut request = launch();
        request.session = Some(session.into());
        let outcome = serde_json::to_value(rig.cli().launch(&request)).unwrap();
        assert_eq!(outcome["kind"], "joined", "{outcome}");
        outcome
    };
    // A lamp-made name says nothing: what it was asked does.
    let books = worked_row(
        AGENT,
        "lamp-b1287be53f02104bd6a17de348f1afde",
        "idle",
        &["  Write a short\nbook program.  ", "Add a search."],
        "I wrote books.py.",
    );
    assert_eq!(
        joined("the book program one", json!([books]))["session"],
        json!({"ask":"Write a short book program."})
    );
    // Its own name, and its first ask.
    let mut docs = ps_row(AGENT, "Docs-Update", "idle", Some("/home/person/work/site"));
    docs["asks"] = json!(["Update the setup page"]);
    assert_eq!(
        joined("docs update", json!([docs]))["session"],
        json!({"name":"Docs-Update","ask":"Update the setup page"})
    );
    // No name: its folder's last part; nothing asked yet: no ask.
    let site = ps_row(AGENT, "", "idle", Some("/home/person/work/site/"));
    assert_eq!(
        joined("site", json!([site]))["session"],
        json!({"name":"site"})
    );
    // A long first ask is cut at a word, to at most 80 characters.
    let long = "Rewrite the reading list program so that every book keeps its author, its year and a short note of what the person thought";
    let wordy = worked_row(AGENT, "lamp-a17b", "idle", &[long], "");
    let ask = joined("the reading list", json!([wordy]))["session"]["ask"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(ask.chars().count() <= 80, "{ask}");
    assert!(ask.ends_with('…'), "{ask}");
    assert!(
        long.starts_with(ask.trim_end_matches('…').trim_end()),
        "{ask}"
    );
    assert!(
        long[ask.trim_end_matches('…').len()..].starts_with(' '),
        "{ask}"
    );
    // The router's join: no session kept.
    let mut rig = Rig::new();
    rig.config.route = true;
    rig.plan(
        json!({"machines":{"default":true},"new":{"default":true},"route":step(
            json!({"decided":"session","id":AGENT,"name":"Fix login","sent":{"state":"queued"}})
        )}),
    );
    let routed = serde_json::to_value(rig.cli().launch(&launch())).unwrap();
    assert_eq!(routed, json!({"kind":"joined","agent_id":AGENT}));
}

/// #5535, `serve --reply-named`: a launch that names a session is typed
/// into that session with `harness reply` (`joined`), found among the
/// computer's sessions (`ps --all`) by name or folder, however it is said.
/// Nothing is routed and nothing is started new, whatever else it meets.
#[test]
fn with_reply_named_a_named_session_is_typed_into_and_never_started_new() {
    let verbs = |rig: &Rig| -> Vec<String> {
        rig.calls()
            .iter()
            .map(|call| call["args"][0].as_str().unwrap().to_owned())
            .collect()
    };
    let named = |session: &str, rows: Value, reply: Value| {
        let mut rig = Rig::new();
        rig.config.reply_named = true;
        rig.config.route = true;
        rig.config.engines_ready = true;
        rig.plan(json!({
            "machines":{"default":true},"new":{"default":true},"route":{"default":true},
            "ps":step(json!({"you":null,"machine":"desk-pc","harnesses":rows,"next":[]})),
            "reply":reply
        }));
        let mut request = launch();
        request.session = Some(session.into());
        let outcome = bare(rig.cli().launch(&request));
        (outcome, verbs(&rig), rig)
    };
    let took =
        |id: &str| step(json!({"ok":true,"name":"x","id":id,"message":"m1","state":"queued"}));
    let rows = json!([
        ps_row(AGENT, "Docs-Update", "idle", Some("/home/person/work/site")),
        ps_row(
            OTHER,
            "login fix",
            "working",
            Some("/home/person/work/auth/")
        ),
    ]);
    let (outcome, called, rig) = named("docs update", rows.clone(), took(AGENT));
    assert_eq!(
        outcome,
        LaunchOutcome::Joined {
            agent_id: AGENT.into(),
            session: None,
        }
    );
    assert_eq!(called, ["machines", "ps", "reply"]);
    assert_eq!(
        rig.calls()[1]["args"],
        json!(["ps", "--machine", MACHINE, "--all", "--json"])
    );
    assert_eq!(
        rig.calls()[2]["args"],
        json!([
            "reply",
            "--machine",
            MACHINE,
            AGENT,
            "--json",
            "--",
            "Inspect the authored fixture"
        ])
    );
    // By its folder's last part, and by part of its name.
    assert_eq!(
        named("Site", rows.clone(), took(AGENT)).0,
        LaunchOutcome::Joined {
            agent_id: AGENT.into(),
            session: None,
        }
    );
    assert_eq!(
        named("docs", rows.clone(), took(AGENT)).0,
        LaunchOutcome::Joined {
            agent_id: AGENT.into(),
            session: None,
        }
    );
    // Busy, asking, or only stopped: not sent, never new.
    let (outcome, called, _) = named("auth", rows.clone(), took(OTHER));
    assert_eq!(outcome, not_sent("session_busy"));
    assert_eq!(called, ["machines", "ps"]);
    let asking = json!([ps_row(AGENT, "docs", "needs-input", None)]);
    assert_eq!(
        named("docs", asking, took(AGENT)).0,
        not_sent("session_busy")
    );
    let stopped = json!([ps_row(AGENT, "docs", "stopped", Some("/w/docs"))]);
    let (outcome, called, _) = named("docs", stopped, took(AGENT));
    assert_eq!(outcome, not_sent("session_stopped"));
    assert_eq!(called, ["machines", "ps"]);
    // A stopped one beside a live one of the same name: the live one.
    let both = json!([
        ps_row(OTHER, "docs", "stopped", None),
        ps_row(AGENT, "docs", "idle", None),
    ]);
    assert_eq!(
        named("docs", both, took(AGENT)).0,
        LaunchOutcome::Joined {
            agent_id: AGENT.into(),
            session: None,
        }
    );
    // None fits, or two live ones do: asked about, never guessed.
    assert_eq!(
        named("billing", rows.clone(), took(AGENT)).0,
        not_sent("session_not_found")
    );
    let two = json!([
        ps_row(AGENT, "docs one", "idle", None),
        ps_row(OTHER, "docs two", "idle", None),
    ]);
    let (outcome, called, _) = named("docs", two, took(AGENT));
    assert_eq!(outcome, not_sent("session_ambiguous"));
    assert_eq!(called, ["machines", "ps"]);
    // Too short to be part of a name: only a whole name fits.
    assert_eq!(
        named("do", rows.clone(), took(AGENT)).0,
        not_sent("session_not_found")
    );
    // The reply refused, or its machine refused the words: nothing typed.
    for (stdout, code) in [
        (
            json!({"error":"REPLY_OFF","detail":"…","code":"REPLY_OFF"}),
            "reply_off",
        ),
        (json!({"error":"NOT_IDLE","detail":"…"}), "reply_not_idle"),
        (
            json!({"error":"NOT_RUNNING","detail":"…"}),
            "session_closed",
        ),
        (
            json!({"ok":false,"name":"x","id":AGENT,"message":null,"state":"failed"}),
            "reply_failed",
        ),
    ] {
        let reply = json!({"stdout":format!("{stdout}\n"),"exit":1});
        let (outcome, called, _) = named("docs update", rows.clone(), reply);
        assert_eq!(outcome, not_sent(code), "{stdout}");
        assert_eq!(called, ["machines", "ps", "reply"], "{stdout}");
    }
    // Taken, with an answer for another session or none readable: the
    // words may be there, so uncertain, never asked again.
    let (outcome, _, _) = named("docs update", rows.clone(), took(OTHER));
    assert_eq!(outcome, uncertain("reply_uncertain"));
    let (outcome, _, _) = named("docs update", rows.clone(), json!({"stdout":"not json\n"}));
    assert_eq!(outcome, uncertain("reply_uncertain"));
    // Only at a word's start (the Reviewer-lead's P2 on #5779): "api" is
    // inside "rapid-prototype", never the start of one of its words.
    let near = json!([ps_row(AGENT, "rapid-prototype", "idle", Some("/w/rapid"))]);
    let (outcome, called, _) = named("api", near, took(AGENT));
    assert_eq!(outcome, not_sent("session_not_found"));
    assert_eq!(called, ["machines", "ps"]);
    // Each word said begins a word of the name, in order.
    let worded = json!([ps_row(AGENT, "Fix the API client", "idle", None)]);
    assert_eq!(
        named("api cli", worded.clone(), took(AGENT)).0,
        LaunchOutcome::Joined {
            agent_id: AGENT.into(),
            session: None,
        }
    );
    assert_eq!(
        named("client api", worded, took(AGENT)).0,
        not_sent("session_not_found")
    );
    // Words around the name are not stripped: the voice sends the name.
    assert_eq!(
        named("the docs one", rows.clone(), took(AGENT)).0,
        not_sent("session_not_found")
    );
    // A list that can't be read: not sent.
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.plan(json!({"machines":{"default":true},"ps":{"stdout":"not json\n"}}));
    let mut request = launch();
    request.session = Some("docs".into());
    assert_eq!(rig.cli().launch(&request), not_sent("sessions_failed"));
    assert_eq!(verbs(&rig), ["machines", "ps"]);
    // Off: a named session is not sent, and nothing runs at all.
    let rig = Rig::new();
    let mut request = launch();
    request.session = Some("docs".into());
    assert_eq!(rig.cli().launch(&request), not_sent("reply_named_off"));
    assert!(verbs(&rig).is_empty());
    // On with no session named: as before.
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    assert!(matches!(
        rig.cli().launch(&launch()),
        LaunchOutcome::Created { .. }
    ));
    assert_eq!(verbs(&rig), ["machines", "new"]);
    // Session words that are not a name are refused before any process.
    let long = "x".repeat(129);
    for words in ["", "   ", "docs\nrm", long.as_str()] {
        let mut rig = Rig::new();
        rig.config.reply_named = true;
        let mut request = launch();
        request.session = Some(words.into());
        assert_eq!(rig.cli().launch(&request), not_sent("invalid_session"));
        assert!(verbs(&rig).is_empty());
    }
}

/// #5535, the first builder's ruling on #5784's follow-up: a named session
/// Harness says was closed or stopped (`harness reply`'s `NOT_RUNNING`) is
/// not sent with its own reason, `session_closed`, so the lamp can say how
/// to open it again, and nothing is routed or started new in its place.
#[test]
fn with_reply_named_a_closed_session_is_told_closed_and_nothing_new_starts() {
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.config.route = true;
    rig.config.engines_ready = true;
    rig.plan(json!({
        "machines":{"default":true},"new":{"default":true},"route":{"default":true},
        "ps":step(json!({"you":null,"machine":"desk-pc","harnesses":[
            ps_row(AGENT, "Docs-Update", "idle", Some("/w/site"))
        ],"next":[]})),
        "reply":{"stdout":format!("{}\n", json!({
            "error":"NOT_RUNNING",
            "detail":"was closed or stopped. Run harness resume, or start a new harness with the whole task.",
            "code":"NOT_RUNNING",
            "next":[]
        })),"exit":5}
    }));
    let mut request = launch();
    request.session = Some("docs update".into());
    assert_eq!(bare(rig.cli().launch(&request)), not_sent("session_closed"));
    let called: Vec<String> = rig
        .calls()
        .iter()
        .map(|call| call["args"][0].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(called, ["machines", "ps", "reply"]);
}

/// `LAMP_TASKS_RESUME_CLOSED` (openharness#3606's provisional contract): a
/// named session `harness reply` finds closed (`NOT_RUNNING`) is reopened
/// once with `harness resume` and the words typed again, waited for first
/// while it says it is working; any other end of the resume is
/// `session_closed` (`session_resume_unconfirmed` for `RESUME_UNCONFIRMED`),
/// nothing typed and nothing started new. Off, it is never resumed.
#[test]
fn with_resume_closed_a_closed_named_session_is_reopened_once_and_typed_into() {
    let closed = json!({
        "stdout":format!("{}\n", json!({
            "error":"NOT_RUNNING","detail":"… Run harness resume …","code":"NOT_RUNNING","next":[]
        })),"exit":5
    });
    let took = step(json!({"ok":true,"name":"x","id":AGENT,"message":"m1","state":"queued"}));
    let reopened = |resumed: bool, state: &str| {
        step(json!({
            "ok":true,"name":"Docs-Update","id":AGENT,"state":state,
            "resumed":resumed,"effect":"reopened","next":[]
        }))
    };
    let named = |on: bool, plan: Value| {
        let mut rig = Rig::new();
        rig.config.reply_named = true;
        rig.config.resume_closed = on;
        rig.config.route = true;
        let mut full = json!({
            "machines":{"default":true},"new":{"default":true},"route":{"default":true},
            // Harness 0.5.98's own help lists `resume`.
            "--help":{"stdout":"Commands:\n  reply    Type into a harness\n  resume   Reopen a closed harness\n"},
            "ps":step(json!({"you":null,"machine":"desk-pc","harnesses":[
                ps_row(AGENT, "Docs-Update", "idle", Some("/w/site"))
            ],"next":[]})),
        });
        for (verb, answer) in plan.as_object().unwrap() {
            full[verb] = answer.clone();
        }
        rig.plan(full);
        let mut request = launch();
        request.session = Some("docs update".into());
        let outcome = rig.cli().launch(&request);
        let called: Vec<String> = rig
            .calls()
            .iter()
            .map(|call| call["args"][0].as_str().unwrap().to_owned())
            .collect();
        (outcome, called, rig)
    };
    let joined = |resumed: bool| LaunchOutcome::Joined {
        agent_id: AGENT.into(),
        session: Some(lamp_tasks::harness::JoinedSession {
            name: Some("Docs-Update".into()),
            ask: None,
            resumed: Some(resumed),
        }),
    };
    // Reopened with its conversation, and typed into.
    let (outcome, called, rig) = named(
        true,
        json!({"reply":{"each":[closed.clone(), took.clone()]},"resume":reopened(true, "idle")}),
    );
    assert_eq!(outcome, joined(true));
    assert_eq!(
        called,
        ["machines", "ps", "reply", "--help", "resume", "reply"]
    );
    assert_eq!(
        rig.calls()[4]["args"],
        json!(["resume", "--machine", MACHINE, AGENT, "--json"])
    );
    // Open again in a new conversation: typed into, and said so.
    let (outcome, called, _) = named(
        true,
        json!({"reply":{"each":[closed.clone(), took.clone()]},"resume":reopened(false, "idle")}),
    );
    assert_eq!(outcome, joined(false));
    assert_eq!(
        called,
        ["machines", "ps", "reply", "--help", "resume", "reply"]
    );
    // Working once reopened: waited for, then typed into.
    let idle = step(json!({"harnesses":[{"id":AGENT,"state":"idle"}]}));
    let (outcome, called, rig) = named(
        true,
        json!({
            "reply":{"each":[closed.clone(), took.clone()]},
            "resume":reopened(true, "working"),"wait":idle
        }),
    );
    assert_eq!(outcome, joined(true));
    assert_eq!(
        called,
        [
            "machines", "ps", "reply", "--help", "resume", "wait", "reply"
        ]
    );
    let wait = rig.calls()[5]["args"].as_array().unwrap().clone();
    assert_eq!(
        wait[0..4],
        json!(["wait", "--machine", MACHINE, AGENT])
            .as_array()
            .unwrap()[..]
    );
    assert_eq!(wait[4], "--timeout");
    // Still working at the wait's end, or asking the person: busy.
    let still = json!({"stdout":format!("{}\n", json!({"harnesses":[]})),"exit":124});
    for plan in [
        json!({"reply":closed.clone(),"resume":reopened(true, "working"),"wait":still}),
        json!({"reply":closed.clone(),"resume":reopened(true, "needs-input")}),
    ] {
        let (outcome, called, _) = named(true, plan.clone());
        assert_eq!(outcome, not_sent("session_busy"), "{plan}");
        assert_eq!(called.iter().filter(|verb| *verb == "reply").count(), 1);
    }
    // Every failure: the session still closed, nothing typed, nothing new.
    // As Harness answers them: `{"error","detail","code","next"}`.
    let failed = |exit: i64, answer: Value| {
        let mut answer = answer;
        answer["code"] = answer["error"].clone();
        answer["next"] = json!([]);
        json!({"stdout":format!("{answer}\n"),"exit":exit})
    };
    for (resume, code) in [
        (
            failed(5, json!({"error":"NOT_YOURS","detail":"…"})),
            "session_closed",
        ),
        (
            failed(5, json!({"error":"REPLY_OFF","detail":"…"})),
            "session_closed",
        ),
        (
            failed(1, json!({"error":"RESUME_UNCONFIRMED","detail":"…"})),
            "session_resume_unconfirmed",
        ),
        (
            failed(1, json!({"error":"AGENT_CHANGED"})),
            "session_closed",
        ),
        (
            failed(1, json!({"error":"AGENT_NOT_FOUND"})),
            "session_closed",
        ),
        (failed(1, json!({"error":"AGENT_BUSY"})), "session_closed"),
        (
            failed(1, json!({"error":"DELETE_IN_PROGRESS"})),
            "session_closed",
        ),
        (
            failed(1, json!({"error":"UNSUPPORTED_ON_REMOTE"})),
            "session_closed",
        ),
        (
            failed(1, json!({"error":"RESUME_PREPARATION_FAILED"})),
            "session_closed",
        ),
        (failed(1, json!({"error":"TIMEOUT"})), "session_closed"),
        (
            failed(1, json!({"error":"UNKNOWN_HARNESS"})),
            "session_closed",
        ),
        (
            failed(1, json!({"error":"SOMETHING_NEW"})),
            "session_closed",
        ),
        // Only `code`, as a later shape might give it.
        (
            json!({"stdout":format!("{}\n", json!({"code":"RESUME_UNCONFIRMED"})),"exit":1}),
            "session_resume_unconfirmed",
        ),
        (
            json!({"stdout":"Unknown command: resume\n","exit":1}),
            "session_closed",
        ),
        // Exit 0 and not `ok`, another session, or no `resumed`: lenient.
        (
            step(json!({"ok":false,"id":AGENT,"state":"idle","resumed":true})),
            "session_closed",
        ),
        (
            step(
                json!({"ok":true,"id":"33333333-3333-4333-8333-333333333333","state":"idle","resumed":true}),
            ),
            "session_closed",
        ),
        (
            step(json!({"ok":true,"id":AGENT,"state":"idle"})),
            "session_closed",
        ),
        (json!({"stdout":"not json\n"}), "session_closed"),
    ] {
        let (outcome, called, _) = named(
            true,
            json!({"reply":{"each":[closed.clone(), took.clone()]},"resume":resume}),
        );
        assert_eq!(outcome, not_sent(code), "{resume}");
        assert_eq!(
            called,
            ["machines", "ps", "reply", "--help", "resume"],
            "{resume}"
        );
    }
    // Closed again after it reopened: never resumed a second time.
    let (outcome, called, _) = named(
        true,
        json!({"reply":closed.clone(),"resume":reopened(true, "idle")}),
    );
    assert_eq!(outcome, not_sent("session_closed"));
    assert_eq!(
        called,
        ["machines", "ps", "reply", "--help", "resume", "reply"]
    );
    // Off: as ruling 1, never resumed.
    let (outcome, called, _) = named(
        false,
        json!({"reply":closed.clone(),"resume":reopened(true, "idle")}),
    );
    assert_eq!(outcome, not_sent("session_closed"));
    assert_eq!(called, ["machines", "ps", "reply"]);
}

/// `LAMP_TASKS_RESUME_CLOSED`: a resume that leaves the reply under a second
/// of the launch bound types nothing (`sessions_timeout`).
#[test]
fn with_resume_closed_a_resume_that_leaves_no_time_types_nothing() {
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.config.resume_closed = true;
    rig.config.read_timeout = Duration::from_secs(3);
    rig.config.launch_timeout = Duration::from_millis(5600);
    let mut resume = step(json!({
        "ok":true,"name":"docs","id":AGENT,"state":"idle","resumed":true,"effect":"x","next":[]
    }));
    resume["delay_ms"] = json!(5000);
    rig.plan(json!({
        "machines":{"default":true},"new":{"default":true},
        "--help":{"stdout":"Commands:\n  resume   Reopen a closed harness\n"},
        "ps":step(json!({"you":null,"machine":"desk-pc","harnesses":[
            ps_row(AGENT, "docs", "idle", None)
        ],"next":[]})),
        "reply":{"each":[
            {"stdout":format!("{}\n", json!({"error":"NOT_RUNNING","code":"NOT_RUNNING","next":[]})),"exit":5},
            step(json!({"ok":true,"name":"x","id":AGENT,"message":"m1","state":"queued"}))
        ]},
        "resume":resume
    }));
    let mut request = launch();
    request.session = Some("docs".into());
    // Under 1 s left after it, `sessions_timeout`; on a loaded host the
    // resume itself may run past the bound, `session_closed`. Either way
    // the words are never typed.
    let outcome = rig.cli().launch(&request);
    assert!(
        outcome == not_sent("sessions_timeout") || outcome == not_sent("session_closed"),
        "{outcome:?}"
    );
    let replies = rig
        .calls()
        .iter()
        .filter(|call| call["args"][0] == "reply")
        .count();
    assert_eq!(replies, 1);
}

/// `LAMP_TASKS_RESUME_CLOSED` on a Harness older than 0.5.98: before the
/// first resume, `harness --help` is asked once a start whether it has
/// `resume`, and `harness resume --help` when its help doesn't list it; with
/// neither, the session is told closed (`session_closed`), as with the
/// switch off, and the answer is kept for the rest of the start. A help
/// that doesn't list it, with `resume --help` exiting 0, has it.
#[test]
fn with_resume_closed_an_older_harness_without_resume_is_told_closed() {
    let closed = json!({
        "stdout":format!("{}\n", json!({"error":"NOT_RUNNING","code":"NOT_RUNNING","next":[]})),
        "exit":5
    });
    let rows = step(json!({"you":null,"machine":"desk-pc","harnesses":[
        ps_row(AGENT, "Docs-Update", "idle", None)
    ],"next":[]}));
    let verbs = |rig: &Rig| -> Vec<String> {
        rig.calls()
            .iter()
            .map(|call| call["args"][0].as_str().unwrap().to_owned())
            .collect()
    };
    let mut request = launch();
    request.session = Some("docs update".into());
    // Too old: no `resume` in its help, and `resume` unknown.
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.config.resume_closed = true;
    rig.plan(json!({
        "machines":{"default":true},"ps":rows.clone(),"reply":closed.clone(),
        "--help":{"stdout":"Commands:\n  reply    Type into a harness\n"},
        "resume":{"stdout":"Unknown command: resume\n","exit":1},
        "--version":{"stdout":"0.5.97\n"}
    }));
    let cli = rig.cli();
    assert_eq!(cli.launch(&request), not_sent("session_closed"));
    assert_eq!(
        verbs(&rig),
        ["machines", "ps", "reply", "--help", "resume", "--version"]
    );
    assert_eq!(rig.calls()[4]["args"], json!(["resume", "--help"]));
    // Kept for the start: asked no more.
    assert_eq!(cli.launch(&request), not_sent("session_closed"));
    let asked = verbs(&rig);
    assert_eq!(asked.iter().filter(|verb| *verb == "--help").count(), 1);
    assert_eq!(asked.iter().filter(|verb| *verb == "resume").count(), 1);
    // Not in its help, and `resume --help` exits 0: it has it.
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.config.resume_closed = true;
    rig.plan(json!({
        "machines":{"default":true},"ps":rows,
        "reply":{"each":[
            closed,
            step(json!({"ok":true,"name":"x","id":AGENT,"message":"m1","state":"queued"}))
        ]},
        "--help":{"stdout":"Usage: harness <command>\n"},
        "resume":{"each":[
            {"stdout":"Usage: harness resume [--machine <name>] <name|id> [--json]\n"},
            step(json!({"ok":true,"name":"Docs-Update","id":AGENT,"state":"idle","resumed":true,"effect":"x","next":[]}))
        ]}
    }));
    assert!(matches!(
        rig.cli().launch(&request),
        LaunchOutcome::Joined { .. }
    ));
    assert_eq!(
        verbs(&rig),
        [
            "machines", "ps", "reply", "--help", "resume", "resume", "reply"
        ]
    );
}

/// A stopped `harness ps --all` row as Harness CLI 0.6.2 lists it: no
/// `about` and no `asks` (openharness cli commands.ts 602-605).
fn stopped_row(id: &str, name: &str, last_active: &str) -> Value {
    json!({"name":name,"id":id,"agent":"Hermes","machine":"lamp","state":"stopped",
        "folder":"/var/lib/lampos-hermes/lamp-tasks","branch":null,"startedBy":"lamp",
        "yours":true,"self":false,"lastActive":last_active})
}

/// What `harness show --json` gives of a stopped harness: its last turn.
fn shown_stopped(id: &str, ask: &str) -> Value {
    step(json!({"id":id,"state":"stopped","ask":ask,"answer":"Done.","truncated":false,"tools":1}))
}

/// #5978, lamp-c0a4 on 10 Nov: with `--close-told` the lamp closes a
/// session once its done line is told, and `harness ps --all` then lists
/// it stopped, without `about` or `asks`. So when no row fits by name,
/// folder or work, the newest stopped rows' last asks are read with
/// `harness show` (at most four, newest first by `lastActive`) and matched
/// by work as a live row's asks are. Off, one that fits is said stopped,
/// nothing typed; with `LAMP_TASKS_RESUME_CLOSED=on` it is reopened once
/// with `harness resume` and typed into; two that fit are asked about.
#[test]
fn a_stopped_session_is_found_by_its_last_ask_and_reopened_with_the_switch() {
    const THIRD: &str = "44444444-4444-4444-8444-444444444444";
    const FOURTH: &str = "55555555-5555-4555-8555-555555555555";
    const FIFTH: &str = "66666666-6666-4666-8666-666666666666";
    let lamp_made = "lamp-0a1b2c3d";
    // The newest last: read newest first.
    let rows = json!([
        stopped_row(FIFTH, lamp_made, "2026-11-10T01:00:00.000Z"),
        stopped_row(AGENT, lamp_made, "2026-11-10T05:00:00.000Z"),
        stopped_row(OTHER, lamp_made, "2026-11-10T04:00:00.000Z"),
        stopped_row(THIRD, lamp_made, "2026-11-10T03:00:00.000Z"),
        stopped_row(FOURTH, lamp_made, "2026-11-10T02:00:00.000Z"),
    ]);
    // Answered by the row each show asks about: the shows run together.
    let shows = |asks: [&str; 4]| {
        json!({"by_arg":{
            AGENT: shown_stopped(AGENT, asks[0]),
            OTHER: shown_stopped(OTHER, asks[1]),
            THIRD: shown_stopped(THIRD, asks[2]),
            FOURTH: shown_stopped(FOURTH, asks[3]),
        }})
    };
    let run = |on: bool, show: Value| {
        let mut rig = Rig::new();
        rig.config.reply_named = true;
        rig.config.resume_closed = on;
        rig.plan(json!({
            "machines":{"default":true},
            "ps":step(json!({"you":null,"machine":"lamp","harnesses":rows,"next":[]})),
            "show":show,
            "--help":{"stdout":"Commands:\n  resume   Reopen a closed harness\n"},
            "resume":step(json!({"ok":true,"name":lamp_made,"id":AGENT,"state":"idle","resumed":true,"effect":"reopened","next":[]})),
            "reply":step(json!({"ok":true,"name":"x","id":AGENT,"message":"m1","state":"queued"})),
        }));
        let mut request = launch();
        request.session = Some("the moon poem".into());
        let outcome = rig.cli().launch(&request);
        let calls: Vec<Value> = rig
            .calls()
            .iter()
            .map(|call| call["args"].clone())
            .collect();
        (outcome, calls)
    };
    let verbs = |calls: &[Value]| -> Vec<String> {
        calls
            .iter()
            .map(|call| call[0].as_str().unwrap().to_owned())
            .collect()
    };
    let moon = [
        "Write a short poem about the moon",
        "Fix the login page",
        "Sort my notes",
        "Plan the trip",
    ];
    // Off: said stopped, nothing reopened or typed. The four newest are read
    // (together, so in no set order); the fifth is never read.
    let (outcome, calls) = run(false, shows(moon));
    assert_eq!(outcome, not_sent("session_stopped"));
    assert_eq!(
        verbs(&calls),
        ["machines", "ps", "show", "show", "show", "show"]
    );
    assert!(calls[2..].contains(&json!(["show", "--machine", MACHINE, AGENT, "--json"])));
    let mut shown: Vec<&str> = calls[2..]
        .iter()
        .map(|call| call[3].as_str().unwrap())
        .collect();
    shown.sort_unstable();
    let mut newest = [AGENT, OTHER, THIRD, FOURTH];
    newest.sort_unstable();
    assert_eq!(shown, newest);
    // On: reopened once, then typed into, and known by its last ask.
    let (outcome, calls) = run(true, shows(moon));
    assert_eq!(
        outcome,
        LaunchOutcome::Joined {
            agent_id: AGENT.into(),
            session: Some(lamp_tasks::harness::JoinedSession {
                name: None,
                ask: Some("Write a short poem about the moon".into()),
                resumed: Some(true),
            }),
        }
    );
    assert_eq!(
        verbs(&calls),
        [
            "machines", "ps", "show", "show", "show", "show", "--help", "resume", "reply"
        ]
    );
    assert_eq!(
        calls[7],
        json!(["resume", "--machine", MACHINE, AGENT, "--json"])
    );
    assert_eq!(calls[8][3], AGENT);
    // Two stopped ones hold the work: asked about, nothing reopened.
    let (outcome, calls) = run(
        true,
        shows([moon[0], "A moon poem for the kids", moon[2], moon[3]]),
    );
    assert_eq!(outcome, not_sent("session_ambiguous"));
    assert!(
        !verbs(&calls)
            .iter()
            .any(|verb| verb == "resume" || verb == "reply")
    );
    // None holds it: not found, nothing reopened.
    let (outcome, calls) = run(true, shows([moon[1], moon[1], moon[2], moon[3]]));
    assert_eq!(outcome, not_sent("session_not_found"));
    assert!(
        !verbs(&calls)
            .iter()
            .any(|verb| verb == "resume" || verb == "reply")
    );
    // A show that can't be read is passed over.
    let (outcome, _) = run(false, json!({"stdout":"not json\n"}));
    assert_eq!(outcome, not_sent("session_not_found"));
}

/// #5978, lamp-c0a4 on 10 Nov: each stopped row's `harness show` took about
/// 2.3 s, and four read one after another cost about 9.2 s of a follow-up.
/// They are read together: four shows of about 1 s each are all read in
/// well under 4 s, and the one that fits is found as before.
#[test]
fn a_closed_sessions_last_asks_are_read_together() {
    const THIRD: &str = "44444444-4444-4444-8444-444444444444";
    const FOURTH: &str = "55555555-5555-4555-8555-555555555555";
    let lamp_made = "lamp-0a1b2c3d";
    let slow = |id: &str, ask: &str| {
        let mut show = shown_stopped(id, ask);
        show["delay_ms"] = json!(1000);
        show
    };
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.plan(json!({
        "machines":{"default":true},
        "ps":step(json!({"you":null,"machine":"lamp","harnesses":[
            stopped_row(AGENT, lamp_made, "2026-11-10T05:00:00.000Z"),
            stopped_row(OTHER, lamp_made, "2026-11-10T04:00:00.000Z"),
            stopped_row(THIRD, lamp_made, "2026-11-10T03:00:00.000Z"),
            stopped_row(FOURTH, lamp_made, "2026-11-10T02:00:00.000Z"),
        ],"next":[]})),
        "show":{"by_arg":{
            AGENT: slow(AGENT, "Fix the login page"),
            OTHER: slow(OTHER, "Sort my notes"),
            THIRD: slow(THIRD, "Write a short poem about the moon"),
            FOURTH: slow(FOURTH, "Plan the trip"),
        }},
    }));
    let mut request = launch();
    request.session = Some("the moon poem".into());
    let began = Instant::now();
    let outcome = rig.cli().launch(&request);
    let took = began.elapsed();
    assert_eq!(outcome, not_sent("session_stopped"));
    let shows = rig
        .calls()
        .iter()
        .filter(|call| call["args"][0] == "show")
        .count();
    assert_eq!(shows, 4);
    assert!(took < Duration::from_millis(2500), "took {took:?}");
}

/// #5978: words that name no work ("the one from before") read no stopped
/// session's last ask: nothing to match it by.
#[test]
fn words_that_name_no_work_read_no_stopped_session() {
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.plan(json!({
        "machines":{"default":true},
        "ps":step(json!({"you":null,"machine":"lamp","harnesses":[
            stopped_row(AGENT, "lamp-0a1b2c3d", "2026-11-10T05:00:00.000Z")
        ],"next":[]})),
        "show":shown_stopped(AGENT, "Write a short poem about the moon"),
    }));
    let mut request = launch();
    request.session = Some("the one from before".into());
    assert_eq!(rig.cli().launch(&request), not_sent("session_not_found"));
    assert!(!rig.calls().iter().any(|call| call["args"][0] == "show"));
}

/// #5978: a session stopped and found by its name is reopened with
/// `LAMP_TASKS_RESUME_CLOSED=on`, its last ask never read.
#[test]
fn with_resume_closed_a_stopped_session_found_by_name_is_reopened() {
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.config.resume_closed = true;
    rig.plan(json!({
        "machines":{"default":true},
        "ps":step(json!({"you":null,"machine":"lamp","harnesses":[
            stopped_row(AGENT, "Docs-Update", "2026-11-10T05:00:00.000Z")
        ],"next":[]})),
        "--help":{"stdout":"Commands:\n  resume   Reopen a closed harness\n"},
        "resume":step(json!({"ok":true,"name":"Docs-Update","id":AGENT,"state":"idle","resumed":false,"effect":"x","next":[]})),
        "reply":step(json!({"ok":true,"name":"x","id":AGENT,"message":"m1","state":"queued"})),
    }));
    let mut request = launch();
    request.session = Some("docs update".into());
    let outcome = rig.cli().launch(&request);
    assert!(
        matches!(&outcome, LaunchOutcome::Joined { session: Some(session), .. }
            if session.resumed == Some(false)),
        "{outcome:?}"
    );
    let verbs: Vec<String> = rig
        .calls()
        .iter()
        .map(|call| call["args"][0].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(verbs, ["machines", "ps", "--help", "resume", "reply"]);
}

/// #5978, lamp-c0a4 on 10 Nov: a named hand-over to the lamp does not list
/// a computer the last `harness machines` look said is offline (each such
/// list only came back DISCONNECTED after about 2.2 s). Nowhere fitting is
/// then said as a computer not read (`sessions_offline`), not as no such
/// session; a computer still online is listed.
#[test]
fn a_computer_said_offline_is_not_listed_for_a_named_session() {
    const LAPTOP: &str = "fedcba9876543210fedcba9876543210";
    const DESK: &str = "abababababababababababababababab";
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.config.session_machines = Some(Arc::new(Mutex::new(SessionMachines {
        lamp: Some(MACHINE.into()),
        computers: vec![
            ("office".into(), LAPTOP.into(), EngineKind::Claude),
            ("desk".into(), DESK.into(), EngineKind::Codex),
        ],
        offline: [LAPTOP.to_owned()].into_iter().collect(),
    })));
    rig.plan(json!({
        "machines":{"default":true},
        "ps":step(json!({"you":null,"machine":"m","harnesses":[],"next":[]})),
    }));
    let mut request = launch();
    request.target.engine = EngineKind::Hermes;
    request.session = Some("docs".into());
    assert_eq!(rig.cli().launch(&request), not_sent("sessions_offline"));
    let listed: Vec<String> = rig
        .calls()
        .iter()
        .filter(|call| call["args"][0] == "ps")
        .map(|call| call["args"][2].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(listed, [MACHINE, DESK]);
}

/// #5978, the owner's rule: a lamp with no computer paired looks for a
/// named session on the lamp alone, a closed one too: found by its last
/// ask, reopened with the switch, typed into on the lamp.
#[test]
fn a_lamp_with_no_computer_finds_and_reopens_its_closed_session() {
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.config.resume_closed = true;
    rig.config.session_machines = Some(Arc::new(Mutex::new(SessionMachines {
        lamp: Some(MACHINE.into()),
        computers: Vec::new(),
        offline: Default::default(),
    })));
    rig.plan(json!({
        "machines":{"default":true},
        "ps":step(json!({"you":null,"machine":"lamp","harnesses":[
            stopped_row(AGENT, "lamp-0a1b2c3d", "2026-11-10T05:00:00.000Z")
        ],"next":[]})),
        "show":shown_stopped(AGENT, "Write a short poem about the moon"),
        "--help":{"stdout":"Commands:\n  resume   Reopen a closed harness\n"},
        "resume":step(json!({"ok":true,"name":"lamp-0a1b2c3d","id":AGENT,"state":"idle","resumed":true,"effect":"x","next":[]})),
        "reply":step(json!({"ok":true,"name":"x","id":AGENT,"message":"m1","state":"queued"})),
    }));
    let mut request = launch();
    request.target.engine = EngineKind::Hermes;
    request.session = Some("the moon poem".into());
    let outcome = rig.cli().launch(&request);
    assert!(
        matches!(&outcome, LaunchOutcome::Joined { agent_id, .. } if agent_id == AGENT),
        "{outcome:?}"
    );
    let verbs: Vec<String> = rig
        .calls()
        .iter()
        .map(|call| call["args"][0].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        verbs,
        ["machines", "ps", "show", "--help", "resume", "reply"]
    );
}

/// A Devin Cloud agent's id, as Harness 0.4.27 gave one on 20 October.
const CLOUD: &str = "cloud_4b0055ec2843353e";

/// #1854, openharness#1444: Devin Cloud is asked for by name with no folder
/// and no `--mode`, and its receipt (Harness 0.4.27's, as it came on 20
/// October: a `cloud_` id, no mode, a null bypass) binds the task.
#[test]
fn devin_cloud_is_asked_for_with_no_folder_or_mode_and_its_cloud_receipt_binds() {
    let rig = Rig::new();
    let mut request = launch();
    request.target.engine = EngineKind::DevinCloud;
    request.target.project = String::new();
    rig.new_step(step(json!({"ok":true,"machineId":MACHINE,"agent":{
        "id":CLOUD,"name":"lamp-task-001","engine":"devin-cloud",
        "permissionMode":null,"bypassPermission":null,"location":"cloud",
        "cloud":{"place":"Devin's cloud","state":"starting"}}})));
    assert_eq!(
        rig.cli().launch(&request),
        LaunchOutcome::Created {
            agent_id: CLOUD.into()
        }
    );
    let args = rig.calls()[1]["args"].as_array().unwrap().clone();
    assert_eq!(
        args[..6],
        [
            json!("new"),
            json!("devin-cloud"),
            json!("--machine"),
            json!(MACHINE),
            json!("--name"),
            json!("lamp-task-001")
        ]
    );
    assert!(!args.iter().any(|arg| arg == "--mode"), "{args:?}");
    assert_eq!(args[6], "--json");
    // Its target names no folder; another agent's must.
    assert_eq!(request.target.validate(), Ok(()));
    request.target.project = "desk-work".into();
    assert_eq!(request.target.validate(), Err("invalid_project".into()));
    request.target.engine = EngineKind::Codex;
    request.target.project = String::new();
    assert_eq!(request.target.validate(), Err("invalid_project".into()));
}

/// Hermes (my person on #3504, 27 Oct: an ask for Hermes went to Claude
/// Code). Harness launches it with its first prompt as `hermes chat -q`,
/// and it has no permission modes (openharness `engineLaunch.ts`:
/// `PERMISSION_MODES` has no `hermes`, `BYPASS_PERMISSION_FLAGS` is null),
/// so it is asked for by name with no `--mode`, as Devin is, and its
/// receipt is taken only when it claims no mode that approves on its own.
#[test]
fn hermes_is_asked_for_by_name_with_no_mode() {
    let rig = Rig::new();
    let mut request = launch();
    request.target.engine = EngineKind::Hermes;
    assert_eq!(
        rig.cli().launch(&request),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    let args = rig.calls()[1]["args"].as_array().unwrap().clone();
    assert_eq!(args[1], "hermes");
    assert!(!args.iter().any(|arg| arg == "--mode"), "{args:?}");
    assert_eq!(EngineKind::Hermes.bus_name(), "hermes");
    assert_eq!(
        serde_json::from_value::<EngineKind>(json!("hermes")).unwrap(),
        EngineKind::Hermes
    );
    // A receipt claiming auto mode is not one Harness gives Hermes.
    let rig = Rig::new();
    rig.new_step(step(json!({"ok":true,"machineId":MACHINE,"agent":{
        "id":AGENT,"name":"lamp-task-001","engine":"hermes","permissionMode":"auto","bypassPermission":true}})));
    assert!(matches!(
        rig.cli().launch(&request),
        LaunchOutcome::Uncertain { .. }
    ));
}

/// #6226: OpenClaw has no permission modes either (Harness CLI 0.6.11):
/// asked for by name, `openclaw`, with no `--mode`, and a receipt claiming
/// a mode that approves on its own is not taken.
#[test]
fn openclaw_is_asked_for_by_name_with_no_mode() {
    let rig = Rig::new();
    let mut request = launch();
    request.target.engine = EngineKind::Openclaw;
    assert_eq!(
        rig.cli().launch(&request),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    let args = rig.calls()[1]["args"].as_array().unwrap().clone();
    assert_eq!(args[1], "openclaw");
    assert!(!args.iter().any(|arg| arg == "--mode"), "{args:?}");
    assert_eq!(EngineKind::Openclaw.bus_name(), "openclaw");
    assert!(EngineKind::Openclaw.on_lamp() && EngineKind::Hermes.on_lamp());
    assert!(!EngineKind::Claude.on_lamp());
    assert_eq!(
        serde_json::from_value::<EngineKind>(json!("openclaw")).unwrap(),
        EngineKind::Openclaw
    );
    let rig = Rig::new();
    rig.new_step(step(json!({"ok":true,"machineId":MACHINE,"agent":{
        "id":AGENT,"name":"lamp-task-001","engine":"openclaw","permissionMode":"auto","bypassPermission":true}})));
    assert!(matches!(
        rig.cli().launch(&request),
        LaunchOutcome::Uncertain { .. }
    ));
}

/// The reviewer's P1 on #2240: every engine's name on the bus passes
/// `tasks/status`' check, Devin Cloud's as `devin_cloud`, while Harness is
/// still asked for `devin-cloud`.
#[test]
fn every_engines_bus_name_passes_the_status_check() {
    for engine in [
        EngineKind::Codex,
        EngineKind::Claude,
        EngineKind::Opencode,
        EngineKind::Devin,
        EngineKind::DevinCloud,
        EngineKind::Hermes,
        EngineKind::Openclaw,
    ] {
        let settled = lamp_msgs::TaskSettled {
            task: 1,
            engine: engine.bus_name(),
            state: lamp_msgs::AgentState::Idle,
            settled: true,
            at_unix_ms: 1_791_964_900_000,
        };
        assert_eq!(settled.check(), Ok(()), "{engine:?}");
    }
    assert_eq!(EngineKind::DevinCloud.bus_name(), "devin_cloud");
    assert_eq!(EngineKind::Codex.bus_name(), "codex");
}

/// openharness#1449: a Devin Cloud agent that has answered is `needs-input`
/// whether or not it asked anything, so its `show` and `wait` read it as
/// `idle` and its answer is told; a computer's agent waiting for the person
/// is still `needs-input`. Harness closes no cloud harness, so none is
/// asked to.
#[test]
fn a_devin_cloud_answer_is_read_as_idle_and_it_is_never_asked_to_close() {
    let rig = Rig::new();
    let mut reply = shown();
    reply["id"] = json!(CLOUD);
    reply["state"] = json!("needs-input");
    reply["answer"] = json!("Posted: https://example.slack.com/archives/C1/p1");
    reply["links"] = json!([{"label":"Session","url":"https://app.devin.ai/sessions/1"}]);
    reply["needsPerson"] = json!(true);
    rig.show_step(step(reply));
    let observation = rig.cli().observe(MACHINE, CLOUD).unwrap();
    assert_eq!(
        (observation.agent_id.as_str(), observation.state.as_str()),
        (CLOUD, "idle")
    );
    assert!(observation.answer.starts_with("Posted:"));
    // Only `needs-input` is read so: a cloud agent working or failed is as
    // Harness says (the reviewer's P3 4 on #2240).
    // A finished Devin Cloud session ends `stopped` (lamp-75c9, 20 Oct): with
    // an answer it is done, in `show`, `wait` and `ps`; with none, stopped.
    let mut reply = shown();
    reply["id"] = json!(CLOUD);
    reply["state"] = json!("stopped");
    reply["answer"] = json!("I replied to the 4 new messages in #lamp-devin-demo.");
    rig.show_step(step(reply.clone()));
    assert_eq!(rig.cli().observe(MACHINE, CLOUD).unwrap().state, "idle");
    reply["answer"] = json!("  ");
    rig.show_step(step(reply));
    assert_eq!(rig.cli().observe(MACHINE, CLOUD).unwrap().state, "stopped");
    let row = json!({"harnesses":[{"id":CLOUD,"state":"stopped"}]}).to_string();
    assert_eq!(parse_ps(row.as_bytes()).unwrap()[0].state, "idle");
    assert_eq!(
        parse_wait(Some(4), row.as_bytes(), &[CLOUD.to_owned()]).unwrap()[0].state,
        "idle"
    );
    // A computer agent's `stopped` is as Harness says.
    let row = json!({"harnesses":[{"id":AGENT,"state":"stopped"}]}).to_string();
    assert_eq!(parse_ps(row.as_bytes()).unwrap()[0].state, "stopped");
    for state in ["working", "failed"] {
        let mut reply = shown();
        reply["id"] = json!(CLOUD);
        reply["state"] = json!(state);
        rig.show_step(step(reply));
        assert_eq!(rig.cli().observe(MACHINE, CLOUD).unwrap().state, state);
        let row = json!({"harnesses":[{"id":CLOUD,"state":state}]}).to_string();
        assert_eq!(parse_ps(row.as_bytes()).unwrap()[0].state, state);
        if state == "failed" {
            assert_eq!(
                parse_wait(Some(4), row.as_bytes(), &[CLOUD.to_owned()]).unwrap()[0].state,
                state
            );
        }
    }
    let agents = vec![CLOUD.to_owned(), AGENT.to_owned()];
    let both = json!({"harnesses":[
        {"id":CLOUD,"state":"needs-input","needsPerson":true},
        {"id":AGENT,"state":"needs-input"}]})
    .to_string();
    assert_eq!(
        parse_wait(Some(3), both.as_bytes(), &agents).unwrap(),
        [
            Ended {
                agent_id: CLOUD.into(),
                state: "idle".into()
            },
            Ended {
                agent_id: AGENT.into(),
                state: "needs-input".into()
            }
        ]
    );
    // `ps` too, so progress doesn't say it waits for the person (Jarvis's P2).
    let listed = json!({"harnesses":[
        {"id":CLOUD,"state":"needs-input"},
        {"id":AGENT,"state":"needs-input"}]})
    .to_string();
    let states: Vec<(String, String)> = parse_ps(listed.as_bytes())
        .unwrap()
        .into_iter()
        .map(|row| (row.agent_id, row.state))
        .collect();
    assert_eq!(
        states,
        [
            (CLOUD.to_owned(), "idle".to_owned()),
            (AGENT.to_owned(), "needs-input".to_owned())
        ]
    );
    let calls = rig.calls().len();
    assert_eq!(rig.cli().close(MACHINE, CLOUD), Ok(Closed::Refused));
    assert_eq!(rig.calls().len(), calls, "close was never run");
    // Only Harness's own form of a cloud id is taken.
    for bad in [
        "cloud_4b0055ec2843353",
        "cloud_4B0055EC2843353E",
        "cloud_4b0055ec2843353e0",
        "cloud-4b0055ec2843353e",
    ] {
        assert_eq!(
            rig.cli().observe(MACHINE, bad),
            Err("invalid_agent_id".into()),
            "{bad}"
        );
    }
}

#[test]
fn opencode_is_asked_for_by_name_in_auto_mode_and_bound_only_by_its_own_receipt() {
    let rig = Rig::new();
    let mut request = launch();
    request.target.engine = EngineKind::Opencode;
    assert_eq!(
        rig.cli().launch(&request),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    let args = &rig.calls()[1]["args"];
    assert_eq!(args[1], "opencode");
    assert_eq!((&args[7], &args[8]), (&json!("--mode"), &json!("auto")));
    for reply in [
        json!({"ok":true,"machineId":MACHINE,"agent":{"id":AGENT,"name":"lamp-task-001","engine":"opencode","permissionMode":null,"bypassPermission":null}}),
        json!({"ok":true,"machineId":MACHINE,"agent":{"id":AGENT,"name":"lamp-task-001","engine":"opencode","permissionMode":"ask","bypassPermission":null}}),
    ] {
        rig.new_step(step(reply));
        assert_eq!(rig.cli().launch(&request), uncertain("invalid_reply"));
    }
}

#[test]
fn input_limits_and_parser_ambiguity_are_refused_before_any_process() {
    let rig = Rig::new();
    let cli = rig.cli();
    let mut requests = Vec::new();
    for prompt in [
        "".into(),
        "--mode full".into(),
        "a\0b".into(),
        "é".repeat(lamp_tasks::harness::MAX_PROMPT_BYTES / 2 + 1),
    ] {
        let mut request = launch();
        request.prompt = prompt;
        requests.push(request);
    }
    for machine in ["01234567", "0123456789ABCDEF0123456789ABCDEF", "not-an-id"] {
        let mut request = launch();
        request.target.machine_id = machine.into();
        requests.push(request);
    }
    for project in ["", "--full", "@another", "./relative", "a/b", "a\nb"] {
        let mut request = launch();
        request.target.project = project.into();
        requests.push(request);
    }
    for name in ["--name", "space name", "é"] {
        let mut request = launch();
        request.name = name.into();
        requests.push(request);
    }
    for request in requests {
        assert!(matches!(
            cli.launch(&request),
            LaunchOutcome::NotSent { .. }
        ));
    }
    let mut boundary = launch();
    boundary.prompt = "é".repeat(500);
    assert!(boundary.validate().is_ok());
    assert!(rig.calls().is_empty());
}

#[test]
fn missing_or_competing_environment_and_unsafe_sessions_are_refused() {
    let rig = Rig::new();
    for name in [
        "TMUX_PANE",
        "CLAUDECODE",
        "CLAUDE_CODE_SESSION_ID",
        "CODEX_THREAD_ID",
        "AI_AGENT",
        "HARNESS_AGENT_SESSION",
        "NODE_OPTIONS",
        "TOKEN",
    ] {
        let mut config = rig.config.clone();
        config.environment.insert(name.into(), "inherited".into());
        assert!(Cli::new(config, "stable".into()).is_err(), "{name}");
    }
    for name in ["HOME", "PATH"] {
        let mut config = rig.config.clone();
        config.environment.remove(name);
        assert!(Cli::new(config, "stable".into()).is_err());
    }
    for session in [
        "".to_string(),
        " leading".into(),
        "a:b".into(),
        "é".into(),
        "x".repeat(81),
    ] {
        assert!(Cli::new(rig.config.clone(), session).is_err());
    }
    assert!(rig.calls().is_empty());
}

#[test]
fn executable_is_canonicalized_and_replacement_or_unsafe_modes_are_refused() {
    let mut rig = Rig::new();
    rig.copied_binary();
    let real = rig.config.executable.clone();
    let link = rig.root.path().join("selected");
    symlink(&real, &link).unwrap();
    rig.config.executable = link.clone();
    let cli = rig.cli();
    fs::remove_file(link).unwrap();
    let outcome = cli.launch(&launch());
    assert!(
        matches!(outcome, LaunchOutcome::Created { .. }),
        "{outcome:?}"
    );
    fs::set_permissions(&real, fs::Permissions::from_mode(0o722)).unwrap();
    assert_eq!(
        cli.launch(&launch()),
        not_sent("preflight_executable_changed")
    );
    rig.config.executable = real.clone();
    assert!(Cli::new(rig.config.clone(), "stable".into()).is_err());
    fs::set_permissions(real, fs::Permissions::from_mode(0o4700)).unwrap();
    assert!(Cli::new(rig.config.clone(), "stable".into()).is_err());
}

#[test]
fn a_missing_working_directory_is_a_proven_preflight_spawn_failure() {
    let mut rig = Rig::new();
    rig.config.environment.insert(
        "HOME".into(),
        rig.root.path().join("missing-home").into_os_string(),
    );
    // Under qemu-user (the ARM64 cross image) this is "preflight_failed":
    // qemu runs glibc's `posix_spawn` vfork as a plain fork, so the child's
    // failed chdir never reaches the parent. The spawn looks made and the
    // child exits 127. Native Linux and the lamp report the spawn's failure
    // (the first builder on #705, 18 Oct).
    assert_eq!(
        rig.cli().launch(&launch()),
        not_sent("preflight_spawn_failed"),
        "\"preflight_failed\" here is qemu-user losing the spawn's error: see above"
    );
    assert!(rig.calls().is_empty());
}

#[test]
fn preflight_requires_exact_unique_full_machine_ids_in_ndjson() {
    let cases = [
        ("".to_string(), "machine_unavailable"),
        (
            format!(
                "{}\n",
                json!({"machineId":"ffffffffffffffffffffffffffffffff","name":MACHINE,"current":true})
            ),
            "machine_unavailable",
        ),
        (
            format!("{}\n", json!([{"machineId":MACHINE,"current":true}])),
            "preflight_invalid_reply",
        ),
        (
            format!("{}\n", json!({"machineId":"01234567","current":true})),
            "preflight_invalid_reply",
        ),
        (
            format!(
                "{}\n{}\n",
                json!({"machineId":MACHINE,"current":true}),
                json!({"machineId":MACHINE,"current":false})
            ),
            "preflight_invalid_reply",
        ),
        ("{malformed}\n".into(), "preflight_invalid_reply"),
    ];
    for (stdout, code) in cases {
        let rig = Rig::new();
        rig.plan(json!({"machines":{"stdout":stdout},"new":{"default":true}}));
        assert_eq!(rig.cli().launch(&launch()), not_sent(code));
        assert_eq!(rig.calls().len(), 1);
    }
}

#[test]
fn a_machine_list_error_or_timeout_cannot_issue_new() {
    for (step, expected) in [
        (
            json!({"exit":1,"stdout":"human secret detail"}),
            "preflight_failed",
        ),
        (json!({"delay_ms":5000}), "preflight_timeout"),
        (json!({"stdout_bytes":65537}), "preflight_stdout_limit"),
    ] {
        let mut rig = Rig::new();
        if expected == "preflight_timeout" {
            rig.config.read_timeout = Duration::from_millis(500);
        }
        rig.plan(json!({"machines":step,"new":{"default":true}}));
        assert_eq!(rig.cli().launch(&launch()), not_sent(expected));
        assert_eq!(rig.calls().len(), 1);
    }
}

#[test]
fn pending_keeps_creation_identity_without_guessing_an_agent() {
    let rig = Rig::new();
    rig.new_step(step(json!({"ok":true,"pending":true,"machineId":MACHINE,"creationId":CREATION,"name":"lamp-task-001","note":"installing"})));
    assert_eq!(
        rig.cli().launch(&launch()),
        LaunchOutcome::Pending {
            creation_id: CREATION.into()
        }
    );
    assert_eq!(rig.calls().len(), 2);
}

#[test]
fn created_requires_every_identity_and_permission_binding() {
    let mut cases = Vec::new();
    for (field, value) in [
        ("id", json!(CREATION.replace('2', "z"))),
        ("name", json!("other")),
        ("engine", json!("claude")),
        ("permissionMode", json!("full")),
        ("permissionMode", json!("ask")),
        ("bypassPermission", json!("yes")),
    ] {
        let mut reply = created();
        reply["agent"][field] = value;
        cases.push(reply);
    }
    for field in ["permissionMode", "bypassPermission", "id"] {
        let mut reply = created();
        reply["agent"].as_object_mut().unwrap().remove(field);
        cases.push(reply);
    }
    let mut contradiction = created();
    contradiction["error"] = json!("contradictory");
    cases.push(contradiction);
    for reply in cases {
        let rig = Rig::new();
        rig.new_step(step(reply));
        assert_eq!(rig.cli().launch(&launch()), uncertain("invalid_reply"));
        assert_eq!(rig.calls().len(), 2);
    }
    // Auto mode with the bypass reported as true, false or null.
    for bypass in [json!(true), json!(false), Value::Null] {
        let rig = Rig::new();
        let mut reply = created();
        reply["agent"]["bypassPermission"] = bypass;
        rig.new_step(step(reply));
        assert!(matches!(
            rig.cli().launch(&launch()),
            LaunchOutcome::Created { .. }
        ));
    }
}

#[test]
fn machine_change_and_existing_name_heuristic_remain_uncertain() {
    let mut wrong = created();
    wrong["machineId"] = json!("ffffffffffffffffffffffffffffffff");
    for (reply, code) in [
        (wrong, "machine_mismatch"),
        (
            json!({"ok":true,"existing":true,"machineId":MACHINE,"name":"lamp-task-001","state":"running","agentId":AGENT}),
            "existing_unbound",
        ),
    ] {
        let rig = Rig::new();
        rig.new_step(step(reply));
        assert_eq!(rig.cli().launch(&launch()), uncertain(code));
        assert_eq!(rig.calls().len(), 2);
    }
}

/// A folder that isn't on the computer is refused before anything is made
/// (Harness 0.4.41, lamp-75c9, 22 Oct), so with `--folder-missing` the task
/// surely did not start (#2840). Any other words, or the switch off, keep
/// the failure uncertain.
#[test]
fn a_missing_folder_after_new_is_not_sent_with_the_switch_and_never_retried() {
    for (error, outcome) in [
        ("CWD_NOT_FOUND", not_sent("folder_missing")),
        (
            "CWD_NOT_FOUND (The saved project folder is no longer available.)",
            not_sent("folder_missing"),
        ),
        (
            "That folder does not exist on the machine.",
            not_sent("folder_missing"),
        ),
        ("CWD_NOT_FOUNDX", uncertain("new_failed")),
        ("SPAWN_FAILED", uncertain("new_failed")),
    ] {
        for switch in [true, false] {
            let mut rig = Rig::new();
            rig.config.folder_missing = switch;
            let reply = json!({"ok": false, "error": error}).to_string();
            rig.new_step(json!({"exit": 1, "stdout": format!("{reply}\n")}));
            let expected = if switch {
                outcome.clone()
            } else {
                uncertain("new_failed")
            };
            assert_eq!(rig.cli().launch(&launch()), expected, "{error} {switch}");
            assert_eq!(
                rig.calls()
                    .iter()
                    .filter(|call| call["args"][0] == "new")
                    .count(),
                1
            );
        }
    }
    // Said with success is not a refusal Harness gives.
    let mut rig = Rig::new();
    rig.config.folder_missing = true;
    rig.new_step(json!({"stdout":"{\"ok\":false,\"error\":\"CWD_NOT_FOUND\"}\n"}));
    assert_eq!(rig.cli().launch(&launch()), uncertain("new_failed"));
}

/// `harness machines engines`' row for this computer and Codex
/// (openharness#1888), with these readiness fields.
fn engines_row(fields: Value) -> Value {
    let mut row = json!({"machineId":MACHINE,"engine":"codex","installed":true,"installable":true});
    row.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    json!({"stdout":format!("{row}\n")})
}

fn engines_asked(rig: &Rig) -> usize {
    rig.calls()
        .iter()
        .filter(|call| call["args"][0] == "machines" && call["args"][1] == "engines")
        .count()
}

fn news(rig: &Rig) -> usize {
    rig.calls()
        .iter()
        .filter(|call| call["args"][0] == "new")
        .count()
}

/// With `--engines-ready` (openharness#1788), a computer's known no ends
/// the hand-over before `new`: `signedIn: false` as `engine_signed_out`,
/// `firstPrompt: false` as `engine_no_prompt`. Anything else it says, or
/// nothing it can say, hands the work over as before; so does the switch
/// off, which asks nothing.
#[test]
fn a_known_no_from_the_computer_is_not_sent_and_anything_else_hands_over() {
    let created = LaunchOutcome::Created {
        agent_id: AGENT.into(),
    };
    let other_machine = format!(
        "{}\n",
        json!({"machineId":"fedcba9876543210fedcba9876543210","engine":"codex","signedIn":false})
    );
    let other_engine = format!(
        "{}\n",
        json!({"machineId":MACHINE,"engine":"claude","signedIn":false})
    );
    for (engines, outcome) in [
        (
            engines_row(json!({"signedIn":false,"firstPrompt":true})),
            not_sent("engine_signed_out"),
        ),
        (
            engines_row(json!({"signedIn":true,"firstPrompt":false})),
            not_sent("engine_no_prompt"),
        ),
        (
            engines_row(json!({"signedIn":false,"firstPrompt":false})),
            not_sent("engine_no_prompt"),
        ),
        // The machine id as another Harness may print it.
        (
            json!({"stdout":format!("{}\n", json!({"machineId":MACHINE.to_uppercase(),"engine":"codex","signedIn":false}))}),
            not_sent("engine_signed_out"),
        ),
        (
            engines_row(json!({"signedIn":true,"firstPrompt":true})),
            created.clone(),
        ),
        (
            engines_row(json!({"signedIn":null,"firstPrompt":null})),
            created.clone(),
        ),
        (engines_row(json!({})), created.clone()),
        (json!({"stdout":other_machine}), created.clone()),
        (json!({"stdout":other_engine}), created.clone()),
        // An older Harness: no such command, or its machine list.
        (
            json!({"exit":2,"stderr":"Usage: harness machines [--json]\n"}),
            created.clone(),
        ),
        (
            json!({"stdout":format!("{}\n", json!({"id":MACHINE,"engine":"codex","current":true}))}),
            created.clone(),
        ),
        (json!({"stdout":"not json\n"}), created.clone()),
        // A failed exit's words are never read as an answer.
        (
            json!({"exit":1,"stdout":format!("{}\n", json!({"machineId":MACHINE,"engine":"codex","signedIn":false}))}),
            created.clone(),
        ),
    ] {
        for switch in [true, false] {
            let mut rig = Rig::new();
            rig.config.engines_ready = switch;
            rig.plan(json!({"machines":{"default":true},"engines":engines,"new":{"default":true}}));
            let expected = if switch {
                outcome.clone()
            } else {
                created.clone()
            };
            assert_eq!(rig.cli().launch(&launch()), expected, "{engines} {switch}");
            assert_eq!(engines_asked(&rig), usize::from(switch), "{engines}");
            assert_eq!(news(&rig), usize::from(expected == created), "{engines}");
        }
    }
    // Asked of that computer, by its id, for the named engine only.
    let mut rig = Rig::new();
    rig.config.engines_ready = true;
    rig.cli().launch(&launch());
    let calls = rig.calls();
    assert_eq!(
        calls[1]["args"],
        json!([
            "machines", "engines", MACHINE, "--engine", "codex", "--json"
        ])
    );
}

/// #5199: `harness machines engines <id> --json` read whole, one call, as
/// Harness gives it: each engine's `installed` and `signedIn` `true`,
/// `false` or `null` (unknown, never missing), other machines' rows and
/// other fields passed over. Exit 1 is `unreachable`, 2 `usage`; a reply
/// that isn't one, or has no row of this computer, is no answer.
#[test]
fn a_computers_engines_are_one_call_read_as_harness_gives_them() {
    let rows = [
        json!({"machineId":MACHINE,"engine":"claude","installed":true,"signedIn":false,"installable":true,"firstPrompt":true}),
        json!({"machineId":MACHINE,"engine":"hermes","installed":null,"signedIn":null,"installable":null,"firstPrompt":null}),
        json!({"machineId":MACHINE.to_uppercase(),"engine":"opencode","installed":false}),
        // A computer without Codex, as Harness prints it.
        json!({"machineId":MACHINE,"engine":"codex","installed":null,"signedIn":null,"installable":true,"firstPrompt":null}),
        json!({"machineId":"fedcba9876543210fedcba9876543210","engine":"cursor","installed":true}),
    ];
    let stdout: String = rows.iter().map(|row| format!("{row}\n")).collect();
    let rig = Rig::new();
    rig.plan(json!({"engines":{"stdout":stdout}}));
    assert_eq!(
        rig.cli().engines(MACHINE).unwrap(),
        [
            EngineRow {
                engine: "claude".into(),
                installed: Some(true),
                installable: Some(true),
                signed_in: Some(false),
            },
            EngineRow {
                engine: "hermes".into(),
                installed: None,
                installable: None,
                signed_in: None,
            },
            EngineRow {
                engine: "opencode".into(),
                installed: Some(false),
                installable: None,
                signed_in: None,
            },
            EngineRow {
                engine: "codex".into(),
                installed: None,
                installable: Some(true),
                signed_in: None,
            },
        ]
    );
    let calls = rig.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0]["args"],
        json!(["machines", "engines", MACHINE, "--json"])
    );
    for (plan, code) in [
        (
            json!({"exit":1,"stderr":"machine unreachable\n"}),
            "unreachable",
        ),
        (
            json!({"exit":2,"stderr":"Usage: harness machines\n"}),
            "usage",
        ),
        (json!({"exit":3}), "engines_failed"),
        (json!({"stdout":"not json\n"}), "engines_invalid_reply"),
        // An older Harness's machine list.
        (
            json!({"stdout":format!("{}\n", json!({"machineId":MACHINE,"current":false}))}),
            "engines_invalid_reply",
        ),
        (json!({"stdout":""}), "engines_none"),
    ] {
        let rig = Rig::new();
        rig.plan(json!({"engines":plan}));
        assert_eq!(rig.cli().engines(MACHINE), Err(code.into()), "{plan}");
    }
    assert_eq!(
        Rig::new().cli().engines("not a machine"),
        Err("invalid_machine".into())
    );
    // Strict past those: an engine twice, or a name Harness doesn't give.
    for bad in [
        json!({"machineId":MACHINE,"engine":"claude","installed":"yes"}),
        json!({"machineId":MACHINE,"engine":"Claude Code","installed":true}),
    ] {
        assert!(
            parse_engines(format!("{bad}\n").as_bytes(), MACHINE).is_err(),
            "{bad}"
        );
    }
    let twice = format!("{}\n{}\n", rows[0], rows[0]);
    assert!(parse_engines(twice.as_bytes(), MACHINE).is_err());
}

/// One computer's answer for one engine that refuses nothing is kept
/// (`ENGINES_KEPT`): a second hand-over asks nothing, and another engine
/// is asked on its own. A refusal is asked again each time, so a person who
/// signs in and asks again isn't refused on an old answer (the
/// Reviewer-lead's P2 on #4033).
#[test]
fn a_computers_answer_is_kept_for_each_engine_unless_it_refused() {
    let mut rig = Rig::new();
    rig.config.engines_ready = true;
    rig.plan(json!({
        "machines":{"default":true},
        "engines":{"each":[
            engines_row(json!({"signedIn":false})),
            engines_row(json!({"signedIn":false})),
            engines_row(json!({"signedIn":true})),
        ]},
        "new":{"default":true}
    }));
    let cli = rig.cli();
    assert_eq!(cli.launch(&launch()), not_sent("engine_signed_out"));
    assert_eq!(cli.launch(&launch()), not_sent("engine_signed_out"));
    assert_eq!(engines_asked(&rig), 2);
    // Signed in at the computer: the next hand-over goes, and is kept.
    assert!(matches!(
        cli.launch(&launch()),
        LaunchOutcome::Created { .. }
    ));
    assert!(matches!(
        cli.launch(&launch()),
        LaunchOutcome::Created { .. }
    ));
    assert_eq!(engines_asked(&rig), 3);
    let mut claude = launch();
    claude.target.engine = EngineKind::Claude;
    assert!(matches!(cli.launch(&claude), LaunchOutcome::Created { .. }));
    assert_eq!(engines_asked(&rig), 4);
    assert_eq!(news(&rig), 3);
    // A new run of the part asks again.
    assert!(matches!(
        rig.cli().launch(&launch()),
        LaunchOutcome::Created { .. }
    ));
    assert_eq!(engines_asked(&rig), 5);
}

/// Devin Cloud isn't one of Harness's engines (`--engine devin-cloud` is a
/// usage error there), so its hand-over asks nothing (Ten's P3 on #4033).
#[test]
fn a_devin_cloud_hand_over_asks_the_computer_nothing() {
    let mut rig = Rig::new();
    rig.config.engines_ready = true;
    rig.plan(json!({
        "machines":{"default":true},
        "engines":engines_row(json!({"signedIn":false})),
        "new":step(json!({"ok":true,"machineId":MACHINE,"agent":{
            "id":CLOUD,"name":"lamp-task-001","engine":"devin-cloud",
            "permissionMode":null,"bypassPermission":null,"location":"cloud",
            "cloud":{"place":"Devin's cloud","state":"starting"}}}))
    }));
    let mut cloud = launch();
    cloud.target.engine = EngineKind::DevinCloud;
    cloud.target.project = String::new();
    assert_eq!(
        rig.cli().launch(&cloud),
        LaunchOutcome::Created {
            agent_id: CLOUD.into()
        }
    );
    assert_eq!(engines_asked(&rig), 0);
    assert_eq!(news(&rig), 1);
}

/// A computer slow to answer costs the hand-over at most the read bound
/// (`ENGINES_PROBE_TIMEOUT`, here the test's shorter one), and the work
/// still goes; its no-answer is kept, so the next hand-over waits for
/// nothing.
#[test]
fn a_computer_slow_to_answer_is_no_refusal_and_is_not_asked_again() {
    let mut rig = Rig::new();
    rig.config.engines_ready = true;
    rig.config.read_timeout = Duration::from_secs(3);
    rig.plan(json!({
        "machines":{"default":true},
        "engines":{"delay_ms":5000},
        "new":{"default":true}
    }));
    let cli = rig.cli();
    let began = Instant::now();
    assert!(matches!(
        cli.launch(&launch()),
        LaunchOutcome::Created { .. }
    ));
    assert!(
        began.elapsed() < Duration::from_millis(4900),
        "{:?}",
        began.elapsed()
    );
    let again = Instant::now();
    assert!(matches!(
        cli.launch(&launch()),
        LaunchOutcome::Created { .. }
    ));
    assert!(
        again.elapsed() < Duration::from_secs(3),
        "{:?}",
        again.elapsed()
    );
    assert_eq!(engines_asked(&rig), 1);
    assert_eq!(news(&rig), 2);
}

/// Harness 0.4.55's refusal of a bare project name whose folder it made on
/// an earlier `new` (Mobile on lamp-0c89, 23 Oct; the same words on the
/// laptop beside lamp-75c9), as `--json` gives it.
fn name_taken(name: &str, computer: &str) -> Value {
    name_taken_at(name, computer, &format!("~/harnesses/{name}"))
}

/// The same refusal with the folder as the computer reports it: Harness
/// prints another machine's folder whole (openharness `newCommand.ts`).
fn name_taken_at(name: &str, computer: &str, folder: &str) -> Value {
    let reply = json!({
        "ok": false,
        "error": format!(
            "“{name}” already exists on {computer}: {folder}. Choose another name \
             for the new project, or start in that folder: harness new claude --machine {MACHINE} \
             {folder} --name lamp-task-001 -- …"
        )
    });
    json!({"exit": 1, "stdout": format!("{reply}\n")})
}

/// The `new` calls' project arguments, in order.
fn new_projects(rig: &Rig) -> Vec<String> {
    rig.calls()
        .iter()
        .filter(|call| call["args"][0] == "new")
        .map(|call| call["args"][4].as_str().unwrap().to_owned())
        .collect()
}

/// A bare-name target gets one hand-over, then Harness refuses every later
/// `new` because the folder it made is there (the first builder on #3126,
/// 11:45). On that refusal alone, the launch is asked once more in
/// `~/harnesses/<name>`, the folder Harness named, and starts there.
#[test]
fn a_bare_name_whose_folder_harness_made_is_started_in_that_folder() {
    for computer in ["this computer", "lamp-office-mac"] {
        let rig = Rig::new();
        rig.plan(json!({
            "machines": {"default": true},
            "new": {"each": [name_taken("fixture-project", computer), step(created())]}
        }));
        assert_eq!(
            rig.cli().launch(&launch()),
            LaunchOutcome::Created {
                agent_id: AGENT.into()
            },
            "{computer}"
        );
        assert_eq!(
            new_projects(&rig),
            ["fixture-project", "~/harnesses/fixture-project"]
        );
        // Everything else in the second `new` is the first's.
        let calls = rig.calls();
        let news: Vec<_> = calls
            .iter()
            .filter(|call| call["args"][0] == "new")
            .collect();
        let mut second = news[1]["args"].clone();
        second[4] = json!("fixture-project");
        assert_eq!(second, news[0]["args"]);
    }
}

/// The folder is taken from Harness's words, as the computer reports it:
/// for another machine its absolute path, spaces and all, after a label
/// that may hold ": " itself (blue's P2 on #3217). It is asked with exactly
/// that string. One that doesn't end in this name, or isn't a folder, is
/// never asked.
#[test]
fn the_folder_is_the_one_harness_names_whole() {
    for (computer, folder) in [
        ("Mac mini", "/home/lee/Harness Projects/fixture-project"),
        ("Office: desk 2", "/Users/lee/harnesses/fixture-project"),
        ("lamp-office-mac", "~/harnesses/fixture-project"),
    ] {
        let rig = Rig::new();
        rig.plan(json!({
            "machines": {"default": true},
            "new": {"each": [name_taken_at("fixture-project", computer, folder), step(created())]}
        }));
        assert_eq!(
            rig.cli().launch(&launch()),
            LaunchOutcome::Created {
                agent_id: AGENT.into()
            },
            "{computer} {folder}"
        );
        assert_eq!(new_projects(&rig), ["fixture-project", folder]);
    }
    for folder in [
        "/home/lee/Harness Projects/other-project",
        "/home/lee/fixture-project/sub",
        "fixture-project",
        "harnesses/fixture-project",
    ] {
        let rig = Rig::new();
        rig.new_step(name_taken_at("fixture-project", "Mac mini", folder));
        assert_eq!(
            rig.cli().launch(&launch()),
            uncertain("new_failed"),
            "{folder}"
        );
        assert_eq!(new_projects(&rig).len(), 1, "{folder}");
    }
}

/// The retry has only what is left of `new`'s one bound, so a hand-over
/// waits at most one `launch_timeout`, not two (the Reviewer-lead's P3 on
/// #3217).
#[test]
fn the_retry_shares_the_one_launch_bound() {
    let mut rig = Rig::new();
    rig.config.launch_timeout = Duration::from_millis(2500);
    let mut refused = name_taken("fixture-project", "this computer");
    refused["delay_ms"] = json!(1500);
    rig.plan(json!({
        "machines": {"default": true},
        "new": {"each": [refused, {"delay_ms": 5000}]}
    }));
    let cli = rig.cli();
    let began = Instant::now();
    let outcome = cli.launch(&launch());
    let took = began.elapsed();
    assert_eq!(outcome, uncertain("timeout"));
    assert_eq!(new_projects(&rig).len(), 2);
    // Two full bounds would be past 4 s; one, with the kill's cleanup, is
    // well under it.
    assert!(took < Duration::from_millis(3800), "{took:?}");
}

/// Two tasks in a row on one name: the first makes the folder, the second
/// is refused for it and starts in it. Neither is lost.
#[test]
fn two_hand_overs_in_a_row_on_one_name_both_start() {
    let rig = Rig::new();
    rig.plan(json!({
        "machines": {"default": true},
        "new": {"each": [step(created()), name_taken("fixture-project", "this computer"), step(created())]}
    }));
    let cli = rig.cli();
    let started = LaunchOutcome::Created {
        agent_id: AGENT.into(),
    };
    assert_eq!(cli.launch(&launch()), started);
    assert_eq!(cli.launch(&launch()), started);
    assert_eq!(
        new_projects(&rig),
        [
            "fixture-project",
            "fixture-project",
            "~/harnesses/fixture-project"
        ]
    );
}

/// Only that refusal, for this name and its own folder, is asked again,
/// and only once: a folder target, another name's folder, a second refusal,
/// another error or a timeout is never asked twice, so nothing can start
/// twice.
#[test]
fn only_the_name_taken_refusal_is_asked_again_and_only_once() {
    // A folder target is never a name Harness made.
    let mut rig = Rig::new();
    let mut folder = launch();
    folder.target.project = "~/fixture-project".into();
    rig.new_step(name_taken("fixture-project", "this computer"));
    assert_eq!(rig.cli().launch(&folder), uncertain("new_failed"));
    assert_eq!(new_projects(&rig), ["~/fixture-project"]);
    // Another name's folder.
    rig = Rig::new();
    rig.new_step(name_taken("other-project", "this computer"));
    assert_eq!(rig.cli().launch(&launch()), uncertain("new_failed"));
    assert_eq!(new_projects(&rig).len(), 1);
    // The same words with success, or with `ok` true, are not the refusal.
    rig = Rig::new();
    let mut said_ok = name_taken("fixture-project", "this computer");
    said_ok["exit"] = json!(0);
    rig.new_step(said_ok);
    assert_eq!(rig.cli().launch(&launch()), uncertain("new_failed"));
    assert_eq!(new_projects(&rig).len(), 1);
    // Refused again in the folder: uncertain, and not asked a third time.
    rig = Rig::new();
    rig.new_step(json!({"each": [
        name_taken("fixture-project", "this computer"),
        json!({"exit": 1, "stdout": "{\"ok\":false,\"error\":\"SPAWN_FAILED\"}\n"})
    ]}));
    assert_eq!(rig.cli().launch(&launch()), uncertain("new_failed"));
    assert_eq!(new_projects(&rig).len(), 2);
    // A first `new` that timed out is never asked again.
    rig = Rig::new();
    rig.config.launch_timeout = Duration::from_millis(300);
    rig.new_step(json!({"delay_ms": 2000}));
    let outcome = rig.cli().launch(&launch());
    assert!(
        matches!(outcome, LaunchOutcome::Uncertain { .. }),
        "{outcome:?}"
    );
    assert_eq!(new_projects(&rig).len(), 1);
}

/// A launch to the `~/harnesses/lamp-tasks` folder, as
/// `--targets-from-machines` writes it.
fn home_launch() -> Launch {
    let mut request = launch();
    request.target.project = "~/harnesses/lamp-tasks".into();
    request
}

/// Harness's `new --json` reply, with the folder the agent works in.
fn created_in(cwd: &str) -> Value {
    let mut reply = created();
    reply["agent"]["project"] = json!({"cwd": cwd});
    step(reply)
}

/// `serve --absolute-folder` (P4 step 1): Harness asks the computer for its
/// home on each `~/…` folder, one more round trip through the relay. Once
/// a `new` there showed the folder it started in, the next hand-over to
/// that computer sends the absolute folder; the first still goes as `~/…`.
/// Off, every one goes as the target names it, as before.
#[test]
fn a_home_folder_is_sent_absolute_once_the_computer_showed_its_home() {
    for (switch, second) in [
        (true, "/Users/lee/harnesses/lamp-tasks"),
        (false, "~/harnesses/lamp-tasks"),
    ] {
        let mut rig = Rig::new();
        rig.config.absolute_folder = switch;
        rig.plan(json!({
            "machines": {"default": true},
            "new": {"each": [created_in("/Users/lee/harnesses/lamp-tasks")]}
        }));
        let cli = rig.cli();
        let started = LaunchOutcome::Created {
            agent_id: AGENT.into(),
        };
        assert_eq!(cli.launch(&home_launch()), started, "{switch}");
        assert_eq!(cli.launch(&home_launch()), started, "{switch}");
        assert_eq!(
            new_projects(&rig),
            ["~/harnesses/lamp-tasks", second],
            "{switch}"
        );
        // The home learned serves any `~/…` folder on that computer.
        let mut other = home_launch();
        other.target.project = "~/code/site".into();
        assert_eq!(cli.launch(&other), started, "{switch}");
        let third = if switch {
            "/Users/lee/code/site"
        } else {
            "~/code/site"
        };
        assert_eq!(new_projects(&rig)[2], third, "{switch}");
    }
}

/// A `new` sent in a learned home that fails forgets the home: the next
/// hand-over goes as `~/…` again, so a home that moved costs one, never
/// every one. Nothing is asked twice.
#[test]
fn a_failed_new_in_a_learned_home_forgets_it() {
    let mut rig = Rig::new();
    rig.config.absolute_folder = true;
    rig.config.folder_missing = true;
    rig.plan(json!({
        "machines": {"default": true},
        "new": {"each": [
            created_in("/Users/lee/harnesses/lamp-tasks"),
            {"stdout":"{\"ok\":false,\"error\":\"CWD_NOT_FOUND\"}\n", "exit": 1},
            created_in("/Users/lee/harnesses/lamp-tasks"),
        ]}
    }));
    let cli = rig.cli();
    let started = LaunchOutcome::Created {
        agent_id: AGENT.into(),
    };
    assert_eq!(cli.launch(&home_launch()), started);
    assert_eq!(cli.launch(&home_launch()), not_sent("folder_missing"));
    assert_eq!(cli.launch(&home_launch()), started);
    assert_eq!(
        new_projects(&rig),
        [
            "~/harnesses/lamp-tasks",
            "/Users/lee/harnesses/lamp-tasks",
            "~/harnesses/lamp-tasks"
        ]
    );
}

/// Only a plain absolute home, from an `ok` reply for that computer whose
/// folder ends in the target's own words, is learned; anything else leaves
/// the next hand-over as `~/…`. A target that is already absolute, or a
/// project name, is sent as it is.
#[test]
fn only_a_plain_home_from_that_computer_is_learned() {
    let other_machine = {
        let mut reply = created();
        reply["machineId"] = json!("fedcba9876543210fedcba9876543210");
        reply["agent"]["project"] = json!({"cwd": "/Users/lee/harnesses/lamp-tasks"});
        step(reply)
    };
    let no_project = step(created());
    for first in [
        created_in("/Users/lee/elsewhere/lamp-tasks"),
        created_in("/harnesses/lamp-tasks"),
        created_in("/Users/../lee/harnesses/lamp-tasks"),
        created_in("/Users//lee/harnesses/lamp-tasks"),
        created_in("Users/lee/harnesses/lamp-tasks"),
        created_in("~/harnesses/lamp-tasks"),
        created_in("/Users/le\u{7}e/harnesses/lamp-tasks"),
        created_in(&format!("/{}/harnesses/lamp-tasks", "a".repeat(500))),
        other_machine,
        no_project,
    ] {
        let mut rig = Rig::new();
        rig.config.absolute_folder = true;
        rig.plan(json!({
            "machines": {"default": true},
            "new": {"each": [first.clone(), step(created())]}
        }));
        let cli = rig.cli();
        let _ = cli.launch(&home_launch());
        assert_eq!(
            cli.launch(&home_launch()),
            LaunchOutcome::Created {
                agent_id: AGENT.into()
            },
            "{first}"
        );
        assert_eq!(new_projects(&rig)[1], "~/harnesses/lamp-tasks", "{first}");
    }
    for project in ["/srv/work/lamp-tasks", "fixture-project"] {
        let mut rig = Rig::new();
        rig.config.absolute_folder = true;
        rig.plan(json!({
            "machines": {"default": true},
            "new": {"each": [created_in("/Users/lee/harnesses/lamp-tasks")]}
        }));
        let mut request = launch();
        request.target.project = project.into();
        let cli = rig.cli();
        cli.launch(&request);
        cli.launch(&request);
        assert_eq!(new_projects(&rig), [project, project]);
    }
}

/// The journal lines one launch says, with the rig's journal kept.
fn journal_of(rig: &mut Rig, request: &Launch) -> (LaunchOutcome, Vec<String>) {
    let journal = Arc::new(Mutex::new(Vec::new()));
    rig.config.journal = Some(journal.clone());
    let outcome = rig.cli().launch(request);
    let lines = journal.lock().unwrap().clone();
    (outcome, lines)
}

/// No journal line holds the ask, or any of its words of four letters or
/// more, or a word that looks secret.
fn assert_said_nothing_private(line: &str, request: &Launch) {
    assert!(!line.contains(&request.prompt), "{line}");
    for word in request.prompt.split_whitespace() {
        if word.chars().count() >= 4 {
            assert!(
                !line.to_lowercase().contains(&word.to_lowercase()),
                "{word}: {line}"
            );
        }
    }
    for secret in ["token", "hunter2", "sk-live", "@example.com"] {
        assert!(!line.contains(secret), "{secret}: {line}");
    }
}

/// An `uncertain` dispatch left lamp-0c89's journal empty for 16 minutes
/// (Mobile, #2813). Every dispatch now says one line: the task's label, its
/// state and code, the CLI's exit status, the time, and stderr's last line
/// with the ask's words and secret-looking words dropped (the first builder
/// on #3126, 11:42).
#[test]
fn every_dispatch_says_one_journal_line_without_the_ask() {
    let mut request = launch();
    request.prompt = "Summarise quarterly invoices for Brightwater".into();
    // Refused after `new` started: exit 1, with stderr.
    let mut rig = Rig::new();
    rig.new_step(json!({
        "exit": 1,
        "stdout": "{\"ok\":false,\"error\":\"SPAWN_FAILED\"}\n",
        "stderr": "warming up\nerror: could not start Summarise for Brightwater token=sk-live-9f8e7d6c5b4a3921 as lee@example.com\n\n"
    }));
    let (outcome, lines) = journal_of(&mut rig, &request);
    assert_eq!(outcome, uncertain("new_failed"));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("tasks: lamp-task-001: dispatch uncertain new_failed (exit 1, "),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].contains(", stderr: error: could not start … as …)"),
        "{}",
        lines[0]
    );
    assert_said_nothing_private(&lines[0], &request);
    // A `new` that never answered: no exit, and no stderr.
    rig = Rig::new();
    rig.config.launch_timeout = Duration::from_millis(300);
    rig.new_step(json!({"delay_ms": 2000, "stderr": "Summarise Brightwater\n"}));
    let (outcome, lines) = journal_of(&mut rig, &request);
    assert!(
        matches!(outcome, LaunchOutcome::Uncertain { .. }),
        "{outcome:?}"
    );
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains(": dispatch uncertain "), "{}", lines[0]);
    assert!(lines[0].contains("(no exit, "), "{}", lines[0]);
    assert!(!lines[0].contains("stderr"), "{}", lines[0]);
    assert_said_nothing_private(&lines[0], &request);
    // Accepted: exit 0, and an stderr of only the ask's words says none.
    rig = Rig::new();
    rig.new_step(
        json!({"stdout": format!("{}\n", created()), "stderr": "Summarise quarterly invoices\n"}),
    );
    let (outcome, lines) = journal_of(&mut rig, &request);
    assert_eq!(
        outcome,
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("tasks: lamp-task-001: dispatch accepted (exit 0, ")
            && lines[0].ends_with(" s)"),
        "{}",
        lines[0]
    );
    assert_said_nothing_private(&lines[0], &request);
    // Refused before any call: one line too, with no exit.
    rig = Rig::new();
    let mut invalid = request.clone();
    invalid.target.project = "--bad".into();
    let (outcome, lines) = journal_of(&mut rig, &invalid);
    assert_eq!(outcome, not_sent("invalid_project"));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("dispatch not_sent invalid_project (no exit, "),
        "{}",
        lines[0]
    );
    assert!(rig.calls().is_empty());
}

/// Fourteen's case on #3727: a name of two given names and no hostname
/// ("Minh Anh’s PC"), each said alone in stderr, is kept from the line.
#[test]
fn a_dispatch_line_keeps_each_given_name_of_a_machines_name() {
    let mut rig = Rig::new();
    rig.plan(json!({
        "machines": {"stdout": format!(
            "{}\n",
            json!({"machineId": MACHINE, "current": false, "name": "Minh Anh’s PC"})
        )},
        "new": {
            "exit": 1,
            "stdout": "{\"ok\":false,\"error\":\"SPAWN_FAILED\"}\n",
            "stderr": "error: minh refused, anh away\n",
        }
    }));
    let (_, lines) = journal_of(&mut rig, &launch());
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert!(
        line.ends_with(", stderr: error: … refused, … away)"),
        "{line}"
    );
    assert!(!line.contains("minh") && !line.contains("anh"), "{line}");
}

/// The Reviewer-lead's P2 on #3727: a Mac's default name and hostname
/// ("Lan’s MacBook Air", `lans-macbook-air.local`) hide the owner's name
/// alone too, and so does a two-letter one (`Vy-PC`) and an inner slug
/// (`lan-mbp` of `dev-lan-mbp`). `local` is kept for the diagnostics.
#[test]
fn a_dispatch_line_keeps_no_owner_of_a_macs_default_name() {
    let mut rig = Rig::new();
    rig.plan(json!({
        "machines": {"stdout": format!(
            "{}\n{}\n{}\n",
            json!({"machineId": MACHINE, "current": false, "name": "Lan’s MacBook Air", "hostname": "lans-macbook-air.local"}),
            json!({"machineId": "fedcba9876543210fedcba9876543210", "current": false, "name": "Vy-PC"}),
            json!({"machineId": "00112233445566778899aabbccddeeff", "current": true, "hostname": "dev-lan-mbp"})
        )},
        "new": {
            "exit": 1,
            "stdout": "{\"ok\":false,\"error\":\"SPAWN_FAILED\"}\n",
            "stderr": "error: user lan refused on lan-mbp, vy and could not reach local daemon\n",
        }
    }));
    let (_, lines) = journal_of(&mut rig, &launch());
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert!(
        line.ends_with(", stderr: error: user … refused on … and could not reach local daemon)"),
        "{line}"
    );
    for private in [" lan", "vy", "mbp"] {
        assert!(!line.contains(private), "{private}: {line}");
    }
}

/// The P3s of Reviewer-three, Fourteen and the Reviewer-lead on #3700: a
/// listed name's parts (`lan` of `lan-macbook-air`) and its shorter slugs
/// (`lan-macbook`) are put as "…" too.
#[test]
fn a_dispatch_line_keeps_no_part_of_a_machine_name_from_stderr() {
    let mut rig = Rig::new();
    rig.plan(json!({
        "machines": {"stdout": format!(
            "{}\n",
            json!({"machineId": MACHINE, "current": false, "name": "Minh_PC", "hostname": "lan-macbook-air.local"})
        )},
        "new": {
            "exit": 1,
            "stdout": "{\"ok\":false,\"error\":\"SPAWN_FAILED\"}\n",
            "stderr": "error: user lan refused on lan-macbook and minh for air\n",
        }
    }));
    let (_, lines) = journal_of(&mut rig, &launch());
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert!(
        line.ends_with(", stderr: error: user … refused on … and … for …)"),
        "{line}"
    );
    for private in ["lan", "minh", "macbook", "air"] {
        assert!(!line.contains(private), "{private}: {line}");
    }
}

/// The P3 Fourteen and the Reviewer-lead left on #3234: a lowercase
/// computer name passes as a plain word, and hostnames often carry the
/// owner's name. Every name and hostname the preflight's `machines` listed,
/// each machine's id, and any long id of letters and digits are put as "…".
#[test]
fn a_dispatch_line_keeps_no_machine_name_or_id_from_stderr() {
    let mut rig = Rig::new();
    rig.plan(json!({
        "machines": {"stdout": format!(
            "{}\n{}\n",
            json!({"machineId": MACHINE, "current": false, "name": "lan-macbook", "hostname": "lans-mbp.local"}),
            json!({"machineId": "fedcba9876543210fedcba9876543210", "current": true, "name": "office imac"})
        )},
        "new": {
            "exit": 1,
            "stdout": "{\"ok\":false,\"error\":\"SPAWN_FAILED\"}\n",
            "stderr": format!(
                "error: spawn refused on lan-macbook (lans-mbp.local) for office machine {MACHINE}, \
                 computer b96b89281e33431eb7e3f49012e7bed7\n"
            ),
        }
    }));
    let (_, lines) = journal_of(&mut rig, &launch());
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert!(
        line.ends_with(", stderr: error: spawn refused on … for … machine … computer …)"),
        "{line}"
    );
    for private in ["lan", "lans", "imac", "office", MACHINE, "b96b8928"] {
        assert!(!line.contains(private), "{private}: {line}");
    }
}

/// Fourteen's privacy probe on #3234: the ask's short words (the people in
/// it), a quoted echo, and the name-taken refusal's computer label, home
/// folder and project never reach the journal; only plain words and codes
/// do.
#[test]
fn a_dispatch_line_keeps_no_name_folder_or_computer_from_stderr() {
    let mut request = launch();
    request.prompt = "Remind Lan to call Mai about the dentist at 5".into();
    for (stderr, said) in [
        (
            "error: could not start: Remind Lan to call Mai about the dentist at 5",
            Some("error: could not start: …"),
        ),
        (
            "harness: task 'remind Lan to call Mai' refused",
            Some("harness: task … refused"),
        ),
        (
            "error: “Ở nhà” already exists on Vy's MacBook: /Users/vy/harnesses/Ở nhà. Choose another name",
            Some("error: … already exists on …"),
        ),
        (
            "SPAWN_FAILED: spawn refused (exit 7)",
            Some("SPAWN_FAILED: spawn refused (exit 7)"),
        ),
        ("Lan Mai Vy", None),
    ] {
        let mut rig = Rig::new();
        rig.new_step(json!({
            "exit": 1,
            "stdout": "{\"ok\":false,\"error\":\"SPAWN_FAILED\"}\n",
            "stderr": format!("{stderr}\n"),
        }));
        let (_, lines) = journal_of(&mut rig, &request);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let line = &lines[0];
        match said {
            Some(said) => assert!(line.ends_with(&format!(", stderr: {said})")), "{line}"),
            None => assert!(!line.contains("stderr"), "{line}"),
        }
        for private in [
            "Lan", "Mai", "Vy", "/Users", "nhà", "MacBook", "dentist", "Remind",
        ] {
            assert!(!line.contains(private), "{private}: {line}");
        }
    }
}

#[test]
fn errors_and_malformed_success_after_new_are_never_retried() {
    for (step, code) in [
        (
            json!({"exit":1,"stdout":"{\"ok\":false,\"error\":\"private detail\"}\n"}),
            "new_failed",
        ),
        (json!({"stdout":"not JSON"}), "invalid_reply"),
        (json!({"stdout":"{} {}"}), "invalid_reply"),
        (
            json!({"stdout":"{\"ok\":true,\"agent\":{}}"}),
            "invalid_reply",
        ),
    ] {
        let rig = Rig::new();
        rig.new_step(step);
        assert_eq!(rig.cli().launch(&launch()), uncertain(code));
        assert_eq!(
            rig.calls()
                .iter()
                .filter(|call| call["args"][0] == "new")
                .count(),
            1
        );
    }
}

#[test]
fn both_pipe_caps_and_exit_deadline_are_enforced_after_new() {
    for (step, code) in [
        (json!({"stdout_bytes":65537}), "stdout_limit"),
        (json!({"stderr_bytes":16385,"default":true}), "stderr_limit"),
        (json!({"default":true,"after_output_ms":5000}), "timeout"),
    ] {
        let mut rig = Rig::new();
        if code == "timeout" {
            rig.config.launch_timeout = Duration::from_millis(500);
        }
        rig.new_step(step);
        let began = Instant::now();
        assert_eq!(rig.cli().launch(&launch()), uncertain(code));
        assert!(began.elapsed() < Duration::from_secs(2));
        assert_eq!(rig.calls().len(), 2);
    }
}

#[test]
fn a_descendant_holding_pipes_is_killed_without_an_unbounded_drain() {
    let rig = Rig::new();
    rig.new_step(json!({"held_descendant":true,"default":true}));
    let began = Instant::now();
    assert_eq!(rig.cli().launch(&launch()), uncertain("pipes_open"));
    assert!(began.elapsed() < Duration::from_secs(2));
    rig.assert_descendant_killed();
    // Reaping the direct child is observable: it cannot be waited for again.
    let pid = rig.calls()[1]["pid"].as_u64().unwrap() as i32;
    let error = rustix::process::waitpid(
        rustix::process::Pid::from_raw(pid),
        rustix::process::WaitOptions::NOHANG,
    )
    .unwrap_err();
    assert_eq!(error, rustix::io::Errno::CHILD);
}

#[test]
fn a_live_descendant_with_closed_pipes_is_still_killed_on_normal_exit() {
    let rig = Rig::new();
    rig.new_step(json!({"held_descendant":true,"redirect_descendant":true,"default":true}));
    assert_eq!(
        rig.cli().launch(&launch()),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    rig.assert_descendant_killed();
}

#[test]
fn changed_executable_after_preflight_proves_new_was_not_spawned() {
    let mut rig = Rig::new();
    rig.copied_binary();
    rig.plan(json!({"machines":{"default":true,"remove_executable":true},"new":{"default":true}}));
    assert_eq!(rig.cli().launch(&launch()), not_sent("executable_changed"));
    assert_eq!(rig.calls().len(), 1);
}

#[test]
fn cancellation_before_spawn_does_not_run_preflight() {
    let mut rig = Rig::new();
    rig.config.cancel = Some(Arc::new(AtomicBool::new(true)));
    let cli = rig.cli();
    assert_eq!(cli.launch(&launch()), not_sent("cancelled"));
    assert_eq!(cli.observe(MACHINE, AGENT), Err("cancelled".into()));
    assert!(rig.calls().is_empty());
}

#[test]
fn cancellation_during_preflight_is_not_sent_and_never_runs_new() {
    let mut rig = Rig::new();
    let cancel = Arc::new(AtomicBool::new(false));
    rig.config.cancel = Some(cancel.clone());
    rig.plan(json!({"machines":{"delay_ms":5000},"new":{"default":true}}));
    let cli = rig.cli();
    let running = thread::spawn(move || cli.launch(&launch()));
    rig.wait_operation("machines");
    cancel.store(true, Ordering::Release);
    assert_eq!(running.join().unwrap(), not_sent("cancelled"));
    assert_eq!(rig.calls().len(), 1);
}

#[test]
fn cancellation_after_new_and_busy_calls_do_not_start_another_process() {
    let mut rig = Rig::new();
    let cancel = Arc::new(AtomicBool::new(false));
    rig.config.cancel = Some(cancel.clone());
    rig.new_step(json!({"delay_ms":5000}));
    let cli = Arc::new(rig.cli());
    let worker = cli.clone();
    let running = thread::spawn(move || worker.launch(&launch()));
    rig.wait_operation("new");
    assert_eq!(cli.launch(&launch()), not_sent("busy"));
    assert_eq!(cli.observe(MACHINE, AGENT), Err("busy".into()));
    let began = Instant::now();
    cancel.store(true, Ordering::Release);
    assert_eq!(running.join().unwrap(), uncertain("cancelled"));
    assert!(began.elapsed() < Duration::from_secs(1));
    assert_eq!(rig.calls().len(), 2);
}

#[test]
fn show_uses_full_agent_id_and_preserves_observation_only_fields() {
    let rig = Rig::new();
    let reply = shown();
    rig.show_step(step(reply));
    let observation = rig.cli().observe(MACHINE, AGENT).unwrap();
    assert_eq!(observation.agent_id, AGENT);
    assert_eq!(observation.state, "idle");
    assert_eq!(
        observation.ask.as_deref(),
        Some("An older unrelated question")
    );
    assert_eq!(observation.answer, "Observed output only");
    assert_eq!(
        rig.calls()[0]["args"],
        json!([
            "show",
            "--machine",
            MACHINE,
            AGENT,
            "--chars",
            "4000",
            "--json"
        ])
    );
    let rig = Rig::new();
    let mut reply = shown();
    reply["state"] = json!("needs-input");
    reply["truncated"] = json!(true);
    rig.show_step(step(reply));
    let observation = rig.cli().observe(MACHINE, AGENT).unwrap();
    assert!(observation.truncated);
    assert_eq!(observation.state, "needs-input");
}

#[test]
fn observation_bounds_wrong_identity_and_invented_completion_are_rejected() {
    let mut cases = Vec::new();
    for state in ["done", "completed", "gone", "closed", "timeout"] {
        let mut reply = shown();
        reply["state"] = json!(state);
        cases.push(reply);
    }
    for (field, value) in [
        ("tools", json!(-1)),
        ("tools", json!(1.5)),
        ("tools", json!(1000001)),
        ("answer", json!("x".repeat(16385))),
        ("ask", json!("x".repeat(4097))),
        ("error", json!("contradicts a successful observation")),
        ("ok", json!(false)),
    ] {
        let mut reply = shown();
        reply[field] = value;
        cases.push(reply);
    }
    let mut absent = shown();
    absent.as_object_mut().unwrap().remove("ask");
    cases.push(absent);
    for reply in cases {
        let rig = Rig::new();
        rig.show_step(step(reply));
        assert_eq!(
            rig.cli().observe(MACHINE, AGENT),
            Err("invalid_observation".into())
        );
    }
    let rig = Rig::new();
    let mut wrong = shown();
    wrong["id"] = json!(CREATION);
    rig.show_step(step(wrong));
    assert_eq!(
        rig.cli().observe(MACHINE, AGENT),
        Err("agent_mismatch".into())
    );
    assert_eq!(
        rig.cli().observe(MACHINE, "111111"),
        Err("invalid_agent_id".into())
    );
    assert_eq!(
        rig.cli().observe("office", AGENT),
        Err("invalid_machine".into())
    );
    assert_eq!(rig.calls().len(), 1);
}

#[test]
fn read_failure_codes_do_not_echo_private_cli_details() {
    let rig = Rig::new();
    rig.show_step(
        json!({"exit":1,"stdout":"{\"error\":\"private account detail\"}","stderr_bytes":100}),
    );
    assert_eq!(
        rig.cli().observe(MACHINE, AGENT),
        Err("observe_failed".into())
    );
}

#[test]
fn read_timeout_and_output_cap_never_retry_show() {
    for (step, code) in [
        (json!({"delay_ms":5000}), "timeout"),
        (json!({"stdout_bytes":65537}), "stdout_limit"),
        (json!({"stderr_bytes":16385}), "stderr_limit"),
    ] {
        let mut rig = Rig::new();
        if code == "timeout" {
            rig.config.read_timeout = Duration::from_millis(500);
        }
        rig.show_step(step);
        let began = Instant::now();
        assert_eq!(rig.cli().observe(MACHINE, AGENT), Err(code.into()));
        assert!(began.elapsed() < Duration::from_secs(2));
        assert_eq!(rig.calls().len(), 1);
    }
}

/// A `harness ps --machine <id> --json` reply, written by hand in the shape
/// Harness 0.4.2's CLI makes (`JSON.stringify(..., null, 1)` of `{you,
/// machine, harnesses}`), read from its source, not from any real session:
/// the ids, names, folders and lines are invented.
fn listing() -> Value {
    json!({"you":null,"machine":"office","harnesses":[
        {"name":"lamp-0a1b","id":AGENT,"agent":"Claude Code","machine":"office","state":"working",
         "folder":"/home/person/harnesses/lamp-tasks","branch":null,"startedBy":"person","yours":false,"self":false,
         "lastActive":"2026-10-15T00:05:12.345Z","asks":["Suggest three names for a desk lamp"],
         "about":"Read the brief / Wrote three names to names.md / All 3 tests pass"},
        {"name":"other","id":"33333333-3333-4333-8333-333333333333","agent":"Codex","machine":"office","state":"idle",
         "folder":null,"branch":null,"startedBy":"person","yours":false,"self":false,"lastActive":null},
        // Still being made: listed by its creation id, which is passed over.
        {"name":"lamp-9z","id":"creation-abcdef0123456789","agent":"Claude Code","machine":"office","state":"starting",
         "folder":null,"branch":null,"startedBy":"you","yours":true,"self":false,"lastActive":"2026-10-15T00:06:00.000Z",
         "about":"lamp-9z has not started yet: it is starting."}
    ]})
}

#[test]
fn a_listing_is_read_for_state_last_activity_and_the_line_and_nothing_else() {
    let text = serde_json::to_string_pretty(&listing()).unwrap();
    let listed = parse_ps(text.as_bytes()).unwrap();
    assert_eq!(
        listed,
        vec![
            Listed {
                agent_id: AGENT.into(),
                state: "working".into(),
                last_active_unix_ms: Some(1_792_022_712_345),
                about: Some(
                    "Read the brief / Wrote three names to names.md / All 3 tests pass".into()
                ),
            },
            Listed {
                agent_id: "33333333-3333-4333-8333-333333333333".into(),
                state: "idle".into(),
                last_active_unix_ms: None,
                about: None,
            },
        ]
    );
    // A long line is cut at a whole character; control characters go.
    let mut long = listing();
    long["harnesses"][0]["about"] = json!(format!("é\n{}", "ü".repeat(MOST_ABOUT_BYTES)));
    let about = parse_ps(long.to_string().as_bytes()).unwrap()[0]
        .about
        .clone()
        .unwrap();
    assert!(
        about.len() <= MOST_ABOUT_BYTES && about.starts_with("é ü"),
        "{}",
        about.len()
    );
    // A time in another form is not known; the rest is read.
    let mut local = listing();
    local["harnesses"][0]["lastActive"] = json!("2026-10-15 07:05:12+07:00");
    let read = parse_ps(local.to_string().as_bytes()).unwrap();
    assert_eq!(
        (read[0].last_active_unix_ms, read[0].state.as_str()),
        (None, "working")
    );
    // Refused whole: not the shape, too many, a state that is no word.
    let mut many = listing();
    many["harnesses"] = json!(vec![listing()["harnesses"][1].clone(); MOST_LISTED + 1]);
    let mut spaced = listing();
    spaced["harnesses"][0]["state"] = json!("hard at work");
    for wrong in [
        json!([]),
        json!({"machine":"office"}),
        json!({"harnesses":[{"id":AGENT}]}),
        many,
        spaced,
    ] {
        assert_eq!(
            parse_ps(wrong.to_string().as_bytes()),
            Err("status_invalid_reply"),
            "{wrong}"
        );
    }
    assert_eq!(parse_ps(b"not json"), Err("status_invalid_reply"));
}

#[test]
fn times_are_read_only_in_the_form_harness_writes() {
    for (text, ms) in [
        ("1970-01-01T00:00:00.000Z", 0),
        ("2026-10-15T00:05:12.345Z", 1_792_022_712_345),
        ("2026-10-14T17:31:02.000Z", 1_791_999_062_000),
        ("2000-02-29T12:00:00Z", 951_825_600_000),
        ("9999-12-31T23:59:59.999Z", 253_402_300_799_999),
    ] {
        assert_eq!(iso_unix_ms(text), Some(ms), "{text}");
    }
    for text in [
        "",
        "2026-10-15",
        "2026-13-01T00:00:00.000Z",
        "2026-10-15T24:00:00.000Z",
        "2026-10-15T00:05:12.345",
        "2026-10-15T00:05:12.34Z",
        "2026-10-15 00:05:12.345Z",
        "1969-12-31T23:59:59.999Z",
        "+2026-10-15T00:05:12Z",
        "2026-10-15T00:05:12.345+07:00",
    ] {
        assert_eq!(iso_unix_ms(text), None, "{text}");
    }
}

#[test]
fn status_is_one_ps_of_the_full_machine_id_bounded_and_never_retried() {
    let rig = Rig::new();
    rig.plan(json!({"ps":step(listing())}));
    let listed = rig.cli().status(MACHINE).unwrap();
    assert_eq!(listed.len(), 2);
    let calls = rig.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0]["args"],
        json!(["ps", "--machine", MACHINE, "--json"])
    );
    // The same caller identity and environment as every other call.
    assert_eq!(calls[0]["environment"]["AI_AGENT"], "lampos");
    assert!(calls[0]["environment"].get("OPENAI_API_KEY").is_none());
    // A listing larger than a read of one agent may be is taken, up to its
    // own bound.
    let rig = Rig::new();
    let mut large = listing();
    large["harnesses"] = json!(vec![listing()["harnesses"][0].clone(); 200]);
    let text = large.to_string();
    assert!(
        text.len() > 64 * 1024 && text.len() < PS_STDOUT_LIMIT,
        "{}",
        text.len()
    );
    rig.plan(json!({"ps":{"stdout":text}}));
    assert_eq!(rig.cli().status(MACHINE).unwrap().len(), 200);
    for (step, code) in [
        (
            json!({"stdout":"x".repeat(PS_STDOUT_LIMIT + 1)}),
            "stdout_limit",
        ),
        (json!({"delay_ms":5000}), "timeout"),
        (
            json!({"exit":1,"stdout":"{\"error\":\"private account detail\"}"}),
            "status_failed",
        ),
        (
            json!({"stdout":"{\"harnesses\":7}"}),
            "status_invalid_reply",
        ),
    ] {
        let mut rig = Rig::new();
        if code == "timeout" {
            rig.config.read_timeout = Duration::from_millis(500);
        }
        rig.plan(json!({"ps":step}));
        let began = Instant::now();
        assert_eq!(rig.cli().status(MACHINE), Err(code.into()));
        assert!(began.elapsed() < Duration::from_secs(2));
        assert_eq!(rig.calls().len(), 1);
    }
    let rig = Rig::new();
    assert_eq!(rig.cli().status("office"), Err("invalid_machine".into()));
    assert!(rig.calls().is_empty());
}

/// The sign-in's lines as they come, up to its end, for at most `within`.
fn sign_in_lines(
    link: &mut dyn lamp_tasks::sign_in::Link,
    within: Duration,
    until_end: bool,
) -> Vec<Result<lamp_tasks::sign_in::Event, &'static str>> {
    let until = Instant::now() + within;
    let mut lines = Vec::new();
    while Instant::now() < until {
        match link.poll() {
            Some(read) => {
                let ended = read.is_err()
                    || matches!(read, Ok(lamp_tasks::sign_in::Event::Confirm { .. })) && !until_end
                    || matches!(read, Ok(lamp_tasks::sign_in::Event::Result(_)));
                lines.push(read);
                if ended {
                    break;
                }
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
    }
    lines
}

/// #1676: Harness's sign-in by a spoken code is its own `login --code
/// --json`, run as every call is (the same marks and HOME), its lines read
/// as they come, the person's answer written to its stdin, and its result
/// read after it.
#[test]
fn the_sign_in_by_code_reads_harness_lines_and_writes_the_answer() {
    use lamp_tasks::sign_in::{Done, Event};
    for (yes, done) in [
        (
            true,
            Done::Success {
                email: Some("someone@example.com".into()),
            },
        ),
        (
            false,
            Done::Error {
                code: "DENIED".into(),
            },
        ),
    ] {
        let rig = Rig::new();
        rig.plan(json!({"login":{"code_flow":true}}));
        let mut link = rig.cli().sign_in_by_code().unwrap();
        let code = Event::Code {
            code: "12345678".into(),
            expires_in: 120,
        };
        assert_eq!(
            sign_in_lines(link.as_mut(), Duration::from_secs(10), false),
            [
                Ok(code.clone()),
                Ok(code),
                Ok(Event::Confirm {
                    email: "someone@example.com".into()
                })
            ]
        );
        let calls = rig.calls();
        assert_eq!(calls[0]["args"], json!(["login", "--code", "--json"]));
        assert_eq!(calls[0]["environment"]["AI_AGENT"], "lampos");
        assert_eq!(
            calls[0]["environment"]["HOME"],
            rig.root.path().to_str().unwrap()
        );
        link.answer(yes).unwrap();
        let lines = sign_in_lines(link.as_mut(), Duration::from_secs(10), true);
        assert_eq!(lines, [Ok(Event::Result(done))]);
        assert_eq!(
            fs::read_to_string(rig.root.path().join("answer")).unwrap(),
            if yes { "yes\n" } else { "no\n" }
        );
        // Then its end, and nothing after.
        assert_eq!(
            sign_in_lines(link.as_mut(), Duration::from_secs(10), true),
            [Err("ended")]
        );
        assert_eq!(link.poll(), None);
    }
}

/// #2839's played check: with `code_hold_until`, the stand-in keeps its code
/// the only thing said until the file appears, then asks as before.
#[test]
fn the_stand_ins_code_is_held_until_its_release_file_appears() {
    use lamp_tasks::sign_in::Event;
    let rig = Rig::new();
    let release = rig.root.path().join("release");
    rig.plan(json!({"login":{"code_flow":true,"code_hold_until":release.to_str().unwrap()}}));
    let mut link = rig.cli().sign_in_by_code().unwrap();
    let code = Event::Code {
        code: "12345678".into(),
        expires_in: 120,
    };
    let mut lines = Vec::new();
    let until = Instant::now() + Duration::from_secs(10);
    while lines.len() < 2 && Instant::now() < until {
        match link.poll() {
            Some(read) => lines.push(read),
            None => thread::sleep(Duration::from_millis(5)),
        }
    }
    assert_eq!(lines, [Ok(code.clone()), Ok(code)]);
    // Held: nothing more until the release. This can only pass wrongly,
    // never fail wrongly, whatever the host's load.
    let held = Instant::now() + Duration::from_millis(300);
    while Instant::now() < held {
        assert_eq!(link.poll(), None, "said while held");
        thread::sleep(Duration::from_millis(5));
    }
    fs::write(&release, b"").unwrap();
    assert_eq!(
        sign_in_lines(link.as_mut(), Duration::from_secs(10), false),
        [Ok(Event::Confirm {
            email: "someone@example.com".into()
        })]
    );
    link.answer(true).unwrap();
}

/// #5198: the app's QR sign-in is Harness's own `login --qr --json`, run
/// as the code's is: its link read in place of the code, then the same
/// question, answer and result.
#[test]
fn the_qr_sign_in_runs_login_qr_and_reads_its_link() {
    use lamp_tasks::sign_in::{Done, Event, Method};
    let rig = Rig::new();
    rig.plan(json!({"login":{"code_flow":true}}));
    let mut link = rig.cli().sign_in(Method::Qr).unwrap();
    let qr = Event::Qr {
        url: "https://harness.autonomous.ai/signin#k=hnq_abcDEF123".into(),
        expires_in: 120,
    };
    assert_eq!(
        sign_in_lines(link.as_mut(), Duration::from_secs(10), false),
        [
            Ok(qr.clone()),
            Ok(qr),
            Ok(Event::Confirm {
                email: "someone@example.com".into()
            })
        ]
    );
    assert_eq!(rig.calls()[0]["args"], json!(["login", "--qr", "--json"]));
    link.answer(true).unwrap();
    assert_eq!(
        sign_in_lines(link.as_mut(), Duration::from_secs(10), true),
        [Ok(Event::Result(Done::Success {
            email: Some("someone@example.com".into())
        }))]
    );
}

/// #1676: a Harness before 0.4.23 doesn't know `--code`; the sign-in ends
/// saying so, not merely that it ended. Stopped early, the program goes.
#[test]
fn a_harness_without_the_code_sign_in_is_told_apart_and_a_dropped_one_stops() {
    let rig = Rig::new();
    rig.plan(json!({"login":{"stderr":"error: unknown option '--code'\n","exit":1}}));
    let mut link = rig.cli().sign_in_by_code().unwrap();
    assert_eq!(
        sign_in_lines(link.as_mut(), Duration::from_secs(10), true),
        [Err("harness_too_old")]
    );
    let rig = Rig::new();
    rig.plan(json!({"login":{"exit":1}}));
    let mut link = rig.cli().sign_in_by_code().unwrap();
    assert_eq!(
        sign_in_lines(link.as_mut(), Duration::from_secs(10), true),
        [Err("ended")]
    );
    // Waiting on its person for minutes: dropped, it is stopped at once.
    let rig = Rig::new();
    rig.plan(json!({"login":{"delay_ms":5000}}));
    let link = rig.cli().sign_in_by_code().unwrap();
    rig.wait_operation("login");
    let pid = rig.calls()[0]["pid"].as_u64().unwrap();
    let began = Instant::now();
    drop(link);
    assert!(began.elapsed() < Duration::from_secs(2));
    // Gone, and reaped: `kill -0` finds nothing (on macOS too).
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(!alive);
}

/// #5978: Harness's daemon is started again with `harness start
/// --restart` (Harness CLI 0.6.2), and a failure is said.
#[test]
fn the_daemon_is_started_again_with_harness_start_restart() {
    let rig = Rig::new();
    rig.plan(json!({"start":step(json!({}))}));
    assert_eq!(rig.cli().restart_daemon(), Ok(()));
    let calls = rig.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["args"], json!(["start", "--restart"]));
    let rig = Rig::new();
    rig.plan(json!({"start":{"exit":1}}));
    assert_eq!(rig.cli().restart_daemon(), Err("restart_failed".into()));
}

/// openharness#1463: after a sign-in by code, Harness's daemon is started
/// with `harness start`, the CLI's own (through systemd where `harness
/// service install` put it, as on a lamp), and a failure is said, not hidden.
#[test]
fn the_daemon_is_started_with_harness_start() {
    let rig = Rig::new();
    rig.plan(json!({"start":step(json!({}))}));
    assert_eq!(rig.cli().start_daemon(), Ok(()));
    let calls = rig.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["args"], json!(["start"]));
    let rig = Rig::new();
    rig.plan(json!({"start":{"exit":1}}));
    assert_eq!(rig.cli().start_daemon(), Err("start_failed".into()));
}

/// #1676: the sign-in is one `auth status --json`, read as Harness gives it
/// (openharness `authStatusCommand`): signed in, signed in and offline, or
/// not, with the other fields passed over. A failed or unreadable reply is
/// not taken as signed out.
#[test]
fn sign_in_is_one_auth_status_read_as_harness_gives_it() {
    let rig = Rig::new();
    rig.plan(json!({"auth":step(json!({"loggedIn":true,"computerId":"c","machineId":MACHINE,"method":"qr"}))}));
    assert_eq!(rig.cli().signed_in(), Ok(SignIn::In));
    let calls = rig.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["args"], json!(["auth", "status", "--json"]));
    for (reply, sign_in) in [
        (json!({"loggedIn":true,"offline":true}), SignIn::Offline),
        (json!({"loggedIn":false,"computerId":"c"}), SignIn::Out),
        (json!({"loggedIn":true,"offline":false}), SignIn::In),
    ] {
        assert_eq!(parse_auth_status(reply.to_string().as_bytes()), Ok(sign_in));
    }
    for (step, code) in [
        (
            json!({"exit":1,"stdout":"{\"loggedIn\":false}"}),
            "auth_status_failed",
        ),
        (
            json!({"stdout":"{\"loggedIn\":\"no\"}"}),
            "auth_status_invalid_reply",
        ),
        (json!({"stdout":"not json"}), "auth_status_invalid_reply"),
    ] {
        let rig = Rig::new();
        rig.plan(json!({"auth":step}));
        assert_eq!(rig.cli().signed_in(), Err(code.into()));
    }
}

/// #6407: signed out, `auth status --json` names this computer's own id
/// (`computerId`), the one Harness starts work under then. It is kept only
/// as Harness mints it, a UUID; anything else is none, and the sign-in is
/// read as before.
#[test]
fn signed_out_the_computers_own_id_is_read_with_the_sign_in() {
    let rig = Rig::new();
    rig.plan(json!({"auth":step(json!({"loggedIn":false,"computerId":COMPUTER}))}));
    assert_eq!(
        rig.cli().signed_in_here(),
        Ok((SignIn::Out, Some(COMPUTER.to_owned())))
    );
    assert_eq!(rig.calls().len(), 1);
    for (reply, read) in [
        (
            json!({"loggedIn":false,"computerId":"c"}),
            (SignIn::Out, None),
        ),
        (json!({"loggedIn":false}), (SignIn::Out, None)),
        (
            json!({"loggedIn":false,"computerId":COMPUTER.to_uppercase()}),
            (SignIn::Out, None),
        ),
        (
            json!({"loggedIn":true,"computerId":COMPUTER,"machineId":MACHINE}),
            (SignIn::In, Some(COMPUTER.to_owned())),
        ),
    ] {
        assert_eq!(parse_auth_here(reply.to_string().as_bytes()), Ok(read));
    }
    assert!(this_computer(COMPUTER));
    assert!(!this_computer(MACHINE));
    assert!(!this_computer("lamp"));
}

/// #6407, a product lamp never signed in: Harness 0.6 refuses `machines`
/// and every `--machine` then, and `new` with none starts the work on this
/// computer under its own id. The lamp's own target named by that id is
/// started with no machines look and no `--machine`, and read, listed and
/// closed the same way; an account machine's target keeps both.
#[test]
fn the_lamps_own_target_signed_out_is_asked_with_no_machines_look_and_no_machine() {
    let rig = Rig::new();
    rig.plan(json!({
        "machines": {"exit": 1, "stdout": "{\"error\":\"NOT_SIGNED_IN\"}\n"},
        "new": step(json!({"ok":true,"machineId":COMPUTER,"agent":{"id":AGENT,"name":"lamp-task-001","engine":"hermes","permissionMode":null,"bypassPermission":false}})),
        "show": step(json!({"id":AGENT,"state":"idle","ask":"Inspect the authored fixture","answer":"Observed answer","truncated":false,"tools":2})),
        "ps": step(json!({"harnesses":[{"id":AGENT,"state":"idle","lastActive":null}]})),
        "close": {"exit": 0},
    }));
    let mut request = launch();
    request.target = Target {
        machine_id: COMPUTER.into(),
        project: "~/harnesses/lamp-tasks/lamp".into(),
        engine: EngineKind::Hermes,
    };
    assert_eq!(request.target.validate(), Ok(()));
    let cli = rig.cli();
    assert_eq!(
        cli.launch(&request),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    assert!(cli.observe(COMPUTER, AGENT).is_ok());
    assert_eq!(cli.status(COMPUTER).map(|listed| listed.len()), Ok(1));
    assert_eq!(cli.close(COMPUTER, AGENT), Ok(Closed::Closed));
    let calls: Vec<Value> = rig
        .calls()
        .into_iter()
        .map(|call| call["args"].clone())
        .collect();
    assert_eq!(
        calls,
        [
            json!([
                "new",
                "hermes",
                "~/harnesses/lamp-tasks/lamp",
                "--name",
                "lamp-task-001",
                "--json",
                "--task",
                "Inspect the authored fixture"
            ]),
            json!(["show", AGENT, "--chars", "4000", "--json"]),
            json!(["ps", "--json"]),
            json!(["close", AGENT, "--json"]),
        ]
    );
    // Work started under an account machine still names it, after the
    // machines look: signed out, that is refused, as before.
    let rig = Rig::new();
    rig.plan(json!({"machines": {"exit": 1, "stdout": "{\"error\":\"NOT_SIGNED_IN\"}\n"}}));
    assert_eq!(rig.cli().launch(&launch()), not_sent("preflight_failed"));
    assert_eq!(rig.calls()[0]["args"], json!(["machines", "--json"]));
}

/// #1676: the machines as `harness machines --json` lists them (openharness
/// `machinesListCommand`: the backend's row with `current` added), one call,
/// each named by its own name, else its hostname, with other fields passed
/// over; the same strictness as the launch's preflight.
#[test]
fn the_machines_are_one_call_read_as_harness_lists_them() {
    let other = "ffffffffffffffffffffffffffffffff";
    let listing = format!(
        "{}\n{}\n{}\n",
        json!({"machineId":MACHINE,"name":"Office iMac","hostname":"imac.local","status":"online","agentCount":3,"current":false}),
        json!({"machineId":other,"name":null,"hostname":"  lamp  ","status":"online","agentCount":0,"current":true}),
        json!({"machineId":"0123456789abcdef0123456789abcdee","name":" ","hostname":null,"status":"offline","agentCount":0,"current":false}),
    );
    let rig = Rig::new();
    rig.plan(json!({"machines":{"stdout":listing}}));
    assert_eq!(
        rig.cli().machines().unwrap(),
        [
            AccountMachine {
                machine_id: MACHINE.into(),
                label: "Office iMac".into(),
                current: false,
                // A word Harness doesn't give (#5199): unknown.
                status: None,
            },
            AccountMachine {
                machine_id: other.into(),
                label: "lamp".into(),
                current: true,
                status: None,
            },
            AccountMachine {
                machine_id: "0123456789abcdef0123456789abcdee".into(),
                label: "".into(),
                current: false,
                status: Some("offline".into()),
            },
        ]
    );
    let calls = rig.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["args"], json!(["machines", "--json"]));
    assert_eq!(parse_machines(b""), Ok(vec![]));
    for bad in [
        format!("{}\n", json!({"machineId":"01234567","current":false})),
        format!(
            "{}\n{}\n",
            json!({"machineId":MACHINE,"current":true}),
            json!({"machineId":other,"current":true})
        ),
        format!(
            "{}\n{}\n",
            json!({"machineId":MACHINE,"current":false}),
            json!({"machineId":MACHINE,"current":false})
        ),
        "{malformed}\n".into(),
    ] {
        assert_eq!(
            parse_machines(bad.as_bytes()),
            Err("machines_invalid_reply"),
            "{bad}"
        );
    }
    let rig = Rig::new();
    rig.plan(json!({"machines":{"exit":1,"stdout":"{}"}}));
    assert_eq!(rig.cli().machines(), Err("machines_failed".into()));
}

const OTHER: &str = "33333333-3333-4333-8333-333333333333";

fn waited(state: &str, id: &str) -> String {
    json!({"harnesses":[{"name":"lamp-task-1","id":id,"state":state,"next":[]}]}).to_string()
}

#[test]
fn wait_holds_harness_own_wait_on_named_agents_beside_the_operation_lane() {
    let rig = Rig::new();
    rig.plan(json!({
        "wait":{"stdout":waited("idle", AGENT),"delay_ms":600},
        "show":{"default":true},
    }));
    let cli = Arc::new(rig.cli());
    let lane = cli.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let agents = vec![AGENT.to_owned(), OTHER.to_owned()];
    let held = thread::spawn(move || lane.wait(MACHINE, &agents, &flag));
    rig.wait_operation("wait");
    // A read goes on while the wait is held: it never takes the one lane.
    let seen = cli.observe(MACHINE, AGENT).unwrap();
    assert_eq!(seen.answer, "Observed answer");
    assert_eq!(
        held.join().unwrap().unwrap(),
        vec![Ended {
            agent_id: AGENT.into(),
            state: "idle".into(),
        }]
    );
    let call = rig
        .calls()
        .into_iter()
        .find(|call| call["args"][0] == "wait")
        .unwrap();
    assert_eq!(
        call["args"],
        json!([
            "wait",
            "--machine",
            MACHINE,
            format!("{AGENT},{OTHER}"),
            "--any",
            "--timeout",
            "600s",
            "--json"
        ])
    );
    assert!(!cancel.load(Ordering::Acquire));
}

#[test]
fn wait_reads_each_exit_and_refuses_what_it_cannot_tell() {
    let rig = Rig::new();
    let cancel = AtomicBool::new(false);
    let agents = [AGENT.to_owned()];
    let run = |plan: Value| {
        rig.plan(json!({ "wait": plan }));
        rig.cli().wait(MACHINE, &agents, &cancel)
    };
    // The time passed with none ended: no agent, not a failure.
    assert_eq!(
        run(json!({"stdout":waited("timeout", AGENT),"exit":124})),
        Ok(vec![])
    );
    // Waits for the person, or stopped: each said as Harness says it.
    assert_eq!(
        run(json!({"stdout":waited("needs-input", AGENT),"exit":3})).unwrap()[0].state,
        "needs-input"
    );
    assert_eq!(
        run(json!({"stdout":waited("stopped", AGENT),"exit":4})).unwrap()[0].state,
        "stopped"
    );
    // A usage error, a reply that is not the listing, or an end for an agent
    // not waited on: failed, never taken for an end.
    assert_eq!(
        run(json!({"stdout":"","exit":2})),
        Err("wait_failed".into())
    );
    assert_eq!(
        run(json!({"stdout":"not json"})),
        Err("wait_invalid_reply".into())
    );
    assert_eq!(
        run(json!({"stdout":waited("idle", OTHER)})),
        Err("wait_invalid_reply".into())
    );
    // Bad names never start a process.
    let calls = rig.calls().len();
    let cli = rig.cli();
    assert_eq!(
        cli.wait("@desk", &agents, &cancel),
        Err("invalid_machine".into())
    );
    assert_eq!(
        cli.wait(MACHINE, &[], &cancel),
        Err("invalid_agent_id".into())
    );
    assert_eq!(
        cli.wait(MACHINE, &["not-an-agent".into()], &cancel),
        Err("invalid_agent_id".into())
    );
    let many = vec![AGENT.to_owned(); MOST_WAITED + 1];
    assert_eq!(
        cli.wait(MACHINE, &many, &cancel),
        Err("invalid_agent_id".into())
    );
    assert_eq!(rig.calls().len(), calls);
}

#[test]
fn a_held_wait_ends_at_once_when_cancelled() {
    let rig = Rig::new();
    rig.plan(json!({"wait":{"stdout":waited("idle", AGENT),"delay_ms":5000}}));
    let cli = Arc::new(rig.cli());
    let cancel = Arc::new(AtomicBool::new(false));
    let (lane, flag) = (cli.clone(), cancel.clone());
    let held = thread::spawn(move || lane.wait(MACHINE, &[AGENT.to_owned()], &flag));
    rig.wait_operation("wait");
    let asked = Instant::now();
    cancel.store(true, Ordering::Release);
    assert_eq!(held.join().unwrap(), Err("cancelled".into()));
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "{:?}",
        asked.elapsed()
    );
    // Cancelled before it starts: no process at all.
    let calls = rig.calls().len();
    assert_eq!(
        cli.wait(MACHINE, &[AGENT.to_owned()], &cancel),
        Err("cancelled".into())
    );
    assert_eq!(rig.calls().len(), calls);
}

#[test]
fn the_wait_listing_is_read_only_for_the_agents_waited_on() {
    let agents = [AGENT.to_owned()];
    let both = json!({"harnesses":[
        {"id":OTHER,"state":"idle"},
        {"id":AGENT,"state":"idle","needsPerson":false,"extra":1},
        {"id":"creation-key","state":"failed","detail":"refused"}
    ]})
    .to_string();
    assert_eq!(
        parse_wait(Some(0), both.as_bytes(), &agents).unwrap(),
        vec![Ended {
            agent_id: AGENT.into(),
            state: "idle".into(),
        }]
    );
    let odd = json!({"harnesses":[{"id":AGENT,"state":"has space"}]}).to_string();
    assert_eq!(
        parse_wait(Some(0), odd.as_bytes(), &agents),
        Err("wait_invalid_reply")
    );
    assert_eq!(parse_wait(None, b"", &agents), Err("wait_failed"));
    assert_eq!(parse_wait(Some(124), b"", &agents), Ok(vec![]));
}

/// #1390: `harness close` on the machine the agent was started on, by the
/// agent's id, never `--now`; its exit code read as Harness 0.4.18's CLI
/// gives it: 0 closed, 5 refused, and 1 for everything else (working,
/// waiting for the person, changed, gone), which is a failure to try again
/// after reading the agent. An id or a machine that isn't one starts no
/// process at all.
#[test]
fn close_names_the_agent_on_its_machine_never_now_and_reads_the_exit_code() {
    for (exit, said) in [
        (0, Ok(Closed::Closed)),
        (5, Ok(Closed::Refused)),
        (1, Err("close_failed".to_string())),
        (2, Err("close_failed".to_string())),
    ] {
        let rig = Rig::new();
        rig.plan(json!({"close":{"exit":exit}}));
        assert_eq!(rig.cli().close(MACHINE, AGENT), said, "exit {exit}");
        let calls = rig.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0]["args"],
            json!(["close", "--machine", MACHINE, AGENT, "--json"])
        );
    }
    let rig = Rig::new();
    for (machine, agent) in [
        (MACHINE, "lamp-coffee-poem"),
        (MACHINE, "--now"),
        ("office", AGENT),
    ] {
        assert!(
            rig.cli().close(machine, agent).is_err(),
            "{machine} {agent}"
        );
    }
    assert!(rig.calls().is_empty());
}

/// `serve --new-folder` (subdarren-P3 part 1), with its file in the rig.
fn new_folder_rig() -> Rig {
    let mut rig = Rig::new();
    rig.config.new_folder = Some(rig.root.path().join("folders-seen.json"));
    rig
}

fn in_made_folder() -> Launch {
    let mut request = launch();
    request.target.engine = EngineKind::Claude;
    request.target.project = "~/harnesses/lamp-tasks".into();
    request
}

/// Harness's `PROJECT_EXISTS` refusal of `--new NAME`, with the folder
/// (another computer, or one that says it) or without it.
fn project_exists(name: &str, folder: Option<&str>) -> Value {
    let error = match folder {
        Some(folder) => format!(
            "“{name}” already exists on lamp-office-mac: {folder}. Choose another name for the new project, \
             or start in that folder: harness new claude --machine {MACHINE} {folder} --name lamp-task-001 -- …"
        ),
        None => format!(
            "“{name}” already exists on this computer. Choose another name for the new project."
        ),
    };
    let reply = json!({"ok": false, "error": error});
    json!({"exit": 1, "stdout": format!("{reply}\n")})
}

/// Each `new` call's arguments, in order.
fn new_args(rig: &Rig) -> Vec<Value> {
    rig.calls()
        .into_iter()
        .filter(|call| call["args"][0] == "new")
        .map(|call| call["args"].clone())
        .collect()
}

/// The first hand-over to a `~/harnesses/<name>` folder asks Harness to make
/// it (`--new <name>`), so it is made and trusted, and nothing else in the
/// call changes; every later one names the folder, as before.
#[test]
fn with_new_folder_the_first_hand_over_asks_harness_to_make_the_folder() {
    let rig = new_folder_rig();
    rig.plan(
        json!({"machines":{"default":true},"new":{"each":[{"default":true}, {"default":true}]}}),
    );
    let cli = rig.cli();
    for _ in 0..2 {
        assert_eq!(
            cli.launch(&in_made_folder()),
            LaunchOutcome::Created {
                agent_id: AGENT.into()
            }
        );
    }
    let news = new_args(&rig);
    assert_eq!(news.len(), 2);
    let request = in_made_folder();
    assert_eq!(
        news[0],
        json!([
            "new",
            "claude",
            "--machine",
            MACHINE,
            "--new",
            "lamp-tasks",
            "--name",
            "lamp-task-001",
            "--mode",
            "auto",
            "--json",
            "--task",
            request.prompt
        ])
    );
    assert_eq!(
        news[1],
        json!([
            "new",
            "claude",
            "--machine",
            MACHINE,
            "~/harnesses/lamp-tasks",
            "--name",
            "lamp-task-001",
            "--mode",
            "auto",
            "--json",
            "--task",
            request.prompt
        ])
    );
    let kept: Value =
        serde_json::from_slice(&fs::read(rig.root.path().join("folders-seen.json")).unwrap())
            .unwrap();
    assert_eq!(
        kept,
        json!({"version":1,"seen":[{"machine_id":MACHINE,"project":"~/harnesses/lamp-tasks"}]})
    );
    assert_eq!(
        fs::metadata(rig.root.path().join("folders-seen.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

/// A folder already there (made by hand, or trusted already, as on the
/// test Mac) is refused by `--new` as `PROJECT_EXISTS`: the hand-over is
/// asked once more in the folder itself, in the same bound, starts there,
/// and later ones name it at once. With or without the folder in
/// Harness's words.
#[test]
fn with_new_folder_a_folder_already_there_still_works() {
    for folder in [
        Some("~/harnesses/lamp-tasks"),
        Some("/Users/lee/harnesses/lamp-tasks"),
        None,
    ] {
        let refusal = project_exists("lamp-tasks", folder);
        let rig = new_folder_rig();
        rig.plan(json!({
            "machines": {"default": true},
            "new": {"each": [refusal, {"default":true}, {"default":true}]}
        }));
        let cli = rig.cli();
        for _ in 0..2 {
            assert_eq!(
                cli.launch(&in_made_folder()),
                LaunchOutcome::Created {
                    agent_id: AGENT.into()
                },
                "{folder:?}"
            );
        }
        let news = new_args(&rig);
        assert_eq!(news.len(), 3, "{folder:?}");
        assert_eq!(news[0][4], "--new");
        assert_eq!(news[0][5], "lamp-tasks");
        // The retry is the first call with the folder in place of `--new
        // NAME`, and the target's own folder, whatever Harness printed.
        let mut retry = news[0].as_array().unwrap().clone();
        retry.splice(4..6, [json!("~/harnesses/lamp-tasks")]);
        assert_eq!(news[1], Value::Array(retry));
        assert_eq!(news[2], news[1]);
    }
}

/// The folders seen are kept in the tasks folder: a restarted part names a
/// folder it saw before, and asks `--new` only for one on another computer.
#[test]
fn with_new_folder_the_folders_seen_survive_a_restart() {
    let rig = new_folder_rig();
    rig.plan(json!({"machines":{"default":true},"new":{"each":[{"default":true}, {"default":true}, {"default":true}]}}));
    assert!(matches!(
        rig.cli().launch(&in_made_folder()),
        LaunchOutcome::Created { .. }
    ));
    let again = rig.cli();
    assert!(matches!(
        again.launch(&in_made_folder()),
        LaunchOutcome::Created { .. }
    ));
    let news = new_args(&rig);
    assert_eq!(news[0][4], "--new");
    assert_eq!(news[1][4], "~/harnesses/lamp-tasks");
    // Another computer's folder of the same name is its own.
    let mut elsewhere = in_made_folder();
    elsewhere.target.machine_id = "fedcba9876543210fedcba9876543210".into();
    rig.plan(json!({"machines":{"stdout":format!("{{\"machineId\":\"{MACHINE}\",\"current\":true}}\n{{\"machineId\":\"{}\",\"current\":false,\"name\":\"other\"}}\n", elsewhere.target.machine_id)},"new":{"default":true}}));
    assert!(matches!(
        again.launch(&elsewhere),
        LaunchOutcome::Created { .. }
    ));
    assert_eq!(new_args(&rig)[2][4], "--new");
}

/// A `new` that failed otherwise, or never answered, marks nothing: the next
/// hand-over asks `--new` again, and nothing is asked twice in one.
#[test]
fn with_new_folder_another_failure_marks_nothing() {
    let exists = project_exists("lamp-tasks", None);
    for failure in [
        json!({"exit":1,"stdout":"{\"ok\":false,\"error\":\"SPAWN_FAILED\"}\n"}),
        // Only exit 1 with the sentence is "there already".
        json!({"exit":5,"stdout":exists["stdout"]}),
        json!({"exit":1,"stdout":"{\"error\":\"PROJECT_EXISTS\"}\n"}),
    ] {
        let rig = new_folder_rig();
        rig.plan(json!({"machines":{"default":true},"new":{"each":[failure, {"default":true}]}}));
        let cli = rig.cli();
        assert_eq!(
            cli.launch(&in_made_folder()),
            uncertain("new_failed"),
            "{failure}"
        );
        assert!(matches!(
            cli.launch(&in_made_folder()),
            LaunchOutcome::Created { .. }
        ));
        let news = new_args(&rig);
        assert_eq!(news.len(), 2, "{failure}");
        assert_eq!(news[0][4], "--new");
        assert_eq!(news[1][4], "--new");
    }
}

/// A fixture Harness that refuses a plain `new` in the folder the machines'
/// targets name, as a computer without it does (`CWD_NOT_FOUND`, lamp-a63e
/// on 6 Nov), and makes it on `--new lamp-tasks`.
fn folder_not_there_rig() -> Rig {
    let rig = Rig::new();
    rig.plan(json!({
        "machines": {"default": true},
        "new": {
            "exit": 1,
            "stdout": "{\"ok\":false,\"error\":\"CWD_NOT_FOUND\"}\n",
            "with_new": {"default": true}
        }
    }));
    rig
}

/// The launch of a target written from the person's machines (`serve
/// --targets-from-machines`), on the rig's computer.
fn from_the_machines() -> Launch {
    let machines = [AccountMachine {
        machine_id: MACHINE.into(),
        label: "Office Mac".into(),
        current: false,
        status: None,
    }];
    let targets = targets_from_machines(&machines, EngineKind::Claude);
    Launch {
        name: "lamp-task-001".into(),
        prompt: "Inspect the authored fixture".into(),
        target: targets["office"].clone(),
        session: None,
    }
}

/// #5541: a target written by `--targets-from-machines` names
/// `~/harnesses/lamp-tasks`, which nothing else makes. With that switch on,
/// and `--new-folder` off as in the product, the first hand-over asks
/// Harness to make the folder (`--new lamp-tasks`) and starts; later ones
/// name it.
#[test]
fn a_target_from_the_machines_has_its_folder_made_on_the_first_hand_over() {
    let mut rig = folder_not_there_rig();
    let root = rig.root.path().to_owned();
    HarnessSwitches::default().apply(&mut rig.config, &root, true);
    let cli = rig.cli();
    assert_eq!(
        cli.launch(&from_the_machines()),
        LaunchOutcome::Created {
            agent_id: AGENT.into()
        }
    );
    let news = new_args(&rig);
    assert_eq!(news.len(), 1);
    assert_eq!(news[0][4], "--new");
    assert_eq!(news[0][5], "lamp-tasks");
    // Made: the next hand-over names the folder, as before.
    rig.plan(json!({"machines":{"default":true},"new":{"default":true}}));
    assert!(matches!(
        cli.launch(&from_the_machines()),
        LaunchOutcome::Created { .. }
    ));
    assert_eq!(new_args(&rig)[1][4], "~/harnesses/lamp-tasks");
    assert!(root.join(NEW_FOLDER_FILE).exists());
}

/// Hand-written targets keep today's behaviour unless `--new-folder` is
/// set: with `--targets-from-machines` on, another `~/harnesses/<name>`
/// folder is sent as it is; and with neither switch, the machines' folder
/// is sent as it is too, so a refusal is the same as before.
#[test]
fn only_the_machines_folder_is_made_unless_new_folder_is_set() {
    let mut rig = Rig::new();
    let root = rig.root.path().to_owned();
    HarnessSwitches::default().apply(&mut rig.config, &root, true);
    rig.plan(json!({"machines":{"default":true},"new":{"default":true}}));
    let mut by_hand = from_the_machines();
    by_hand.target.project = "~/harnesses/my-project".into();
    assert!(matches!(
        rig.cli().launch(&by_hand),
        LaunchOutcome::Created { .. }
    ));
    assert_eq!(new_args(&rig)[0][4], "~/harnesses/my-project");

    let mut rig = folder_not_there_rig();
    let root = rig.root.path().to_owned();
    HarnessSwitches::default().apply(&mut rig.config, &root, false);
    assert_eq!(
        rig.cli().launch(&from_the_machines()),
        uncertain("new_failed")
    );
    assert_eq!(new_args(&rig)[0][4], "~/harnesses/lamp-tasks");
    assert!(!root.join(NEW_FOLDER_FILE).exists());

    // `--new-folder` set: every made folder, as #5121 has it.
    let mut rig = Rig::new();
    let root = rig.root.path().to_owned();
    let switches = HarnessSwitches {
        new_folder: true,
        ..Default::default()
    };
    switches.apply(&mut rig.config, &root, true);
    rig.plan(json!({"machines":{"default":true},"new":{"default":true}}));
    assert!(matches!(
        rig.cli().launch(&by_hand),
        LaunchOutcome::Created { .. }
    ));
    assert_eq!(new_args(&rig)[0][4], "--new");
    assert_eq!(new_args(&rig)[0][5], "my-project");
}

/// Only `~/harnesses/<name>` with a name Harness keeps as it is is asked
/// for with `--new`; any other folder or a bare name goes as before, and
/// without the switch nothing changes.
#[test]
fn new_folder_is_only_for_a_folder_harness_would_make_and_only_when_set() {
    for project in [
        "~/code/lamp-tasks",
        "/Users/lee/harnesses/lamp-tasks",
        "~/harnesses/Lamp Tasks",
        "~/harnesses/a/b",
        "~/harnesses/-x",
        "~/harnesses/.x",
        "~/harnesses/..",
        "~/harnesses/",
        "~/harnesses/café",
        "lamp-tasks",
    ] {
        let rig = new_folder_rig();
        let mut request = in_made_folder();
        request.target.project = project.into();
        rig.plan(json!({"machines":{"default":true},"new":{"default":true}}));
        assert!(
            matches!(rig.cli().launch(&request), LaunchOutcome::Created { .. }),
            "{project}"
        );
        assert_eq!(new_args(&rig)[0][4], project, "{project}");
        assert!(!rig.root.path().join("folders-seen.json").exists());
    }
    let rig = Rig::new();
    rig.plan(json!({"machines":{"default":true},"new":{"default":true}}));
    assert!(matches!(
        rig.cli().launch(&in_made_folder()),
        LaunchOutcome::Created { .. }
    ));
    assert_eq!(new_args(&rig)[0][4], "~/harnesses/lamp-tasks");
}

/// `serve --new-folder` and `serve --absolute-folder` together: the first
/// hand-over to a `~/harnesses/<name>` folder goes as `--new <name>`, which
/// names no folder to make absolute, and its reply still shows the
/// computer's home (`<home>/harnesses/<name>`). Later hand-overs to a `~/…`
/// folder on that computer go absolute, the plain-folder ask after
/// `PROJECT_EXISTS` included.
#[test]
fn new_folder_and_absolute_folder_work_side_by_side() {
    let mut rig = new_folder_rig();
    rig.config.absolute_folder = true;
    rig.plan(json!({
        "machines": {"default": true},
        "new": {"each": [
            created_in("/Users/lee/harnesses/lamp-tasks"),
            created_in("/Users/lee/harnesses/lamp-tasks"),
            created_in("/Users/lee/code/site"),
            project_exists("other", None),
            created_in("/Users/lee/harnesses/other"),
        ]}
    }));
    let cli = rig.cli();
    let started = LaunchOutcome::Created {
        agent_id: AGENT.into(),
    };
    let mut site = home_launch();
    site.target.project = "~/code/site".into();
    let mut other = home_launch();
    other.target.project = "~/harnesses/other".into();
    for request in [&home_launch(), &home_launch(), &site, &other] {
        assert_eq!(cli.launch(request), started, "{}", request.target.project);
    }
    let folders: Vec<_> = new_args(&rig)
        .iter()
        .map(|args| match args[4].as_str().unwrap() {
            "--new" => format!("--new {}", args[5].as_str().unwrap()),
            folder => folder.to_owned(),
        })
        .collect();
    assert_eq!(
        folders,
        [
            "--new lamp-tasks",
            "/Users/lee/harnesses/lamp-tasks",
            "/Users/lee/code/site",
            "--new other",
            "/Users/lee/harnesses/other",
        ]
    );
}

/// A kept file that can't be read is none seen: one `--new`, refused as
/// there already, and the hand-over still starts in the folder.
#[test]
fn with_new_folder_an_unreadable_file_costs_one_ask() {
    let rig = new_folder_rig();
    fs::write(rig.root.path().join("folders-seen.json"), b"not json").unwrap();
    rig.plan(json!({
        "machines": {"default": true},
        "new": {"each": [project_exists("lamp-tasks", None), {"default":true}]}
    }));
    assert!(matches!(
        rig.cli().launch(&in_made_folder()),
        LaunchOutcome::Created { .. }
    ));
    let news = new_args(&rig);
    assert_eq!(news[0][4], "--new");
    assert_eq!(news[1][4], "~/harnesses/lamp-tasks");
}

/// The call lines one Cli says, with the rig's call journal kept.
fn calls_of(rig: &mut Rig) -> (Cli, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    rig.config.calls = Some(calls.clone());
    (rig.cli(), calls)
}

/// No call line holds the ask, a word of it, a question, an answer, a
/// progress line, a name, a folder, or a whole machine or agent id.
fn assert_call_said_nothing_private(line: &str, private: &[&str]) {
    for word in private {
        assert!(!line.contains(word), "{word}: {line}");
    }
    for whole in [MACHINE, AGENT, OTHER] {
        assert!(!line.contains(whole), "{whole}: {line}");
    }
}

/// Proposal L1 and L2: every Harness call says one journal line: its verb,
/// the machine and agent ids cut to 8, the exit, the time, stdout's size,
/// the state and tool count read, and the ids Harness gave. Never the ask.
#[test]
fn every_harness_call_says_one_line_with_ids_cut_and_no_words() {
    const SECRET: &str = "Zanzibar-Quokka whispers lavender passcode 4417";
    let private = [
        "Zanzibar",
        "Quokka",
        "lavender",
        "passcode",
        "4417",
        "Earlier unrelated question",
        "Observed answer",
        "Suggest three names",
        "Read the brief",
        "lamp-task-001",
        "lamp-0a1b",
        "fixture-project",
        "/home/person",
    ];
    // A Devin hand-over: machines, new, show, then reply with the ask.
    let mut rig = Rig::new();
    let mut request = launch();
    request.prompt = SECRET.into();
    request.target.engine = EngineKind::Devin;
    let (cli, calls) = calls_of(&mut rig);
    assert!(matches!(
        cli.launch(&request),
        LaunchOutcome::Created { .. }
    ));
    let lines = calls.lock().unwrap().clone();
    assert_eq!(lines.len(), 4, "{lines:?}");
    for line in &lines {
        assert_call_said_nothing_private(line, &private);
    }
    assert!(
        lines[0].starts_with("tasks: harness machines: exit=0 ms=")
            && lines[0].ends_with(" listed=1"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("tasks: harness new: machine=01234567 exit=0 ms=")
            && lines[1].ends_with(" state=ok id=11111111"),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("tasks: harness show: machine=01234567 agent=11111111 exit=0 ms=")
            && lines[2].ends_with(" state=idle tools=2 id=11111111"),
        "{}",
        lines[2]
    );
    assert!(
        lines[3].starts_with("tasks: harness reply: machine=01234567 agent=11111111 exit=0 ms=")
            && lines[3].ends_with(" state=queued id=11111111 message=m1"),
        "{}",
        lines[3]
    );
    // The same ask through `new --task` and a route first.
    let mut rig = Rig::new();
    rig.config.route = true;
    rig.plan(json!({"machines":{"default":true},"route":{"default":true},"new":{"default":true}}));
    let mut request = launch();
    request.prompt = SECRET.into();
    let (cli, calls) = calls_of(&mut rig);
    assert!(matches!(
        cli.launch(&request),
        LaunchOutcome::Created { .. }
    ));
    let lines = calls.lock().unwrap().clone();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines[1].starts_with("tasks: harness route: machine=01234567 exit=0 ms=")
            && lines[1].ends_with(" state=new"),
        "{}",
        lines[1]
    );
    for line in &lines {
        assert_call_said_nothing_private(line, &private);
    }
    // `ps`: how many, in which state, never the about lines or names.
    let mut rig = Rig::new();
    rig.plan(json!({"ps":step(listing())}));
    let (cli, calls) = calls_of(&mut rig);
    cli.status(MACHINE).unwrap();
    let line = calls.lock().unwrap()[0].clone();
    assert!(
        line.starts_with("tasks: harness ps: machine=01234567 exit=0 ms=")
            && line.contains(" out=")
            && line.ends_with(" listed=3 states=idle:1,starting:1,working:1"),
        "{line}"
    );
    assert_call_said_nothing_private(&line, &private);
    // `wait`: how many waited on, and the ids it came back for.
    let mut rig = Rig::new();
    rig.plan(json!({"wait":{"stdout":waited("idle", AGENT)}}));
    let (cli, calls) = calls_of(&mut rig);
    cli.wait(
        MACHINE,
        &[AGENT.into(), OTHER.into()],
        &AtomicBool::new(false),
    )
    .unwrap();
    let line = calls.lock().unwrap()[0].clone();
    assert!(
        line.starts_with("tasks: harness wait: machine=01234567 agents=2 exit=0 ms=")
            && line.ends_with(" listed=1 states=idle:1 ids=11111111"),
        "{line}"
    );
    assert_call_said_nothing_private(&line, &private);
    // `close`, refused with a nonzero exit and Harness's words, one of them
    // a single word in `message` (the reviewer-lead's P2 on #5092).
    let mut rig = Rig::new();
    rig.plan(json!({"close":{"exit":1,"stdout":
        "{\"ok\":false,\"error\":\"Zanzibar is working\",\"message\":\"Zanzibar\"}\n"}}));
    let (cli, calls) = calls_of(&mut rig);
    assert_eq!(cli.close(MACHINE, AGENT), Err("close_failed".into()));
    let line = calls.lock().unwrap()[0].clone();
    assert!(
        line.starts_with("tasks: harness close: machine=01234567 agent=11111111 exit=1 ms=")
            && line.ends_with(" state=failed error=?"),
        "{line}"
    );
    assert_call_said_nothing_private(&line, &private);
    // A code Harness gives is said as it is.
    let mut rig = Rig::new();
    rig.new_step(json!({"exit":1,"stdout":"{\"ok\":false,\"error\":\"SPAWN_FAILED\"}\n"}));
    let (cli, calls) = calls_of(&mut rig);
    cli.launch(&launch());
    let line = calls.lock().unwrap()[1].clone();
    assert!(
        line.contains(" exit=1 ms=") && line.ends_with(" state=failed error=SPAWN_FAILED"),
        "{line}"
    );
    // A failed `new` or `reply` never journals `message` or `creationId`:
    // there they are Harness's words (twelve's P2 on #5117), with no `ok`
    // at all, or with `ok: false` and a code.
    let words = ["Zanzibar", "Quokka"];
    let mut rig = Rig::new();
    rig.new_step(
        json!({"exit":1,"stdout":"{\"message\":\"Zanzibar\",\"creationId\":\"Quokka\"}\n"}),
    );
    let (cli, calls) = calls_of(&mut rig);
    cli.launch(&launch());
    let line = calls.lock().unwrap()[1].clone();
    assert!(
        line.starts_with("tasks: harness new: ") && line.contains(" exit=1 ms="),
        "{line}"
    );
    assert!(
        !line.contains("message=") && !line.contains("creation="),
        "{line}"
    );
    assert_call_said_nothing_private(&line, &words);
    let mut rig = Rig::new();
    rig.plan(json!({"machines":{"default":true},"new":{"default":true},"show":{"default":true},
        "reply":{"exit":1,"stdout":
            "{\"ok\":false,\"error\":\"NOT_IDLE\",\"message\":\"Zanzibar\",\"creationId\":\"Quokka\"}\n"}}));
    let mut request = launch();
    request.target.engine = EngineKind::Devin;
    let (cli, calls) = calls_of(&mut rig);
    cli.launch(&request);
    let lines = calls.lock().unwrap().clone();
    let line = lines
        .iter()
        .find(|line| line.starts_with("tasks: harness reply: "))
        .unwrap_or_else(|| panic!("{lines:?}"))
        .clone();
    assert!(
        line.contains(" exit=1 ms=") && line.contains(" error=NOT_IDLE"),
        "{line}"
    );
    assert!(
        !line.contains("message=") && !line.contains("creation="),
        "{line}"
    );
    assert_call_said_nothing_private(&line, &words);
    // A timeout: no exit, no stdout, only the adapter's code.
    let mut rig = Rig::new();
    rig.config.read_timeout = Duration::from_millis(300);
    rig.show_step(json!({"delay_ms":2000}));
    let (cli, calls) = calls_of(&mut rig);
    assert_eq!(cli.observe(MACHINE, AGENT), Err("timeout".into()));
    let line = calls.lock().unwrap()[0].clone();
    assert!(
        line.starts_with("tasks: harness show: machine=01234567 agent=11111111 exit=timeout ms=")
            && !line.contains("out="),
        "{line}"
    );
}

/// A cloud agent's id keeps its `cloud_` and the first 8 after it.
#[test]
fn a_call_line_cuts_a_cloud_agents_id_after_its_kind() {
    let mut rig = Rig::new();
    rig.show_step(step(
        json!({"id":CLOUD,"state":"idle","ask":null,"answer":"","truncated":false,"tools":0}),
    ));
    let (cli, calls) = calls_of(&mut rig);
    cli.observe(MACHINE, CLOUD).unwrap();
    let line = calls.lock().unwrap()[0].clone();
    let cut = "cloud_4b0055ec";
    assert!(
        line.contains(&format!(" agent={cut} ")) && line.ends_with(&format!(" id={cut}")),
        "{line}"
    );
    assert!(!line.contains(CLOUD), "{line}");
}

/// Seen on a lamp on 3 Nov: `show` against the person's Mac took 16 to 39
/// s, and the read before each close ran out at the 10 s `read_timeout`.
/// The read before a close is the same `show`, given
/// `CLOSE_READ_TIMEOUT` (no more than one `launch_timeout`) in its place:
/// here, scaled down, a `show` slower than `read_timeout` runs out for an
/// ordinary read and reads through before a close; one slower than the
/// close's own bound still runs out there, as `timeout`, never an answer.
#[test]
fn the_read_before_a_close_is_given_longer_than_any_other_read() {
    let mut rig = Rig::new();
    rig.config.read_timeout = Duration::from_millis(300);
    rig.config.launch_timeout = Duration::from_secs(4);
    rig.show_step(json!({"default":true,"delay_ms":900}));
    assert_eq!(rig.cli().observe(MACHINE, AGENT), Err("timeout".into()));
    let read = rig.cli().observe_to_close(MACHINE, AGENT).unwrap();
    assert_eq!(read.state, "idle");
    let calls = rig.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1]["args"][0], "show");
    drop(rig);

    let mut rig = Rig::new();
    rig.config.read_timeout = Duration::from_millis(300);
    rig.config.launch_timeout = Duration::from_millis(800);
    rig.show_step(json!({"default":true,"delay_ms":5000}));
    let began = Instant::now();
    assert_eq!(
        rig.cli().observe_to_close(MACHINE, AGENT),
        Err("timeout".into())
    );
    assert!(began.elapsed() < Duration::from_secs(3));
    assert!(CLOSE_READ_TIMEOUT > Duration::from_secs(10));
    assert!(CLOSE_READ_TIMEOUT < CLOSE_BUDGET);
}

/// A close is given only what is left of the close's budget after its
/// read: a `close` slower than that runs out (`timeout`), and nothing left
/// at all starts no process (`close_no_time`). Either is a failed close,
/// tried again later, read first.
#[test]
fn a_close_is_given_only_what_is_left_of_its_budget() {
    let rig = Rig::new();
    assert_eq!(
        rig.cli().close_within(MACHINE, AGENT, Duration::ZERO),
        Err("close_no_time".into())
    );
    assert!(rig.calls().is_empty());

    rig.plan(json!({"close":{"exit":0,"delay_ms":3000}}));
    let began = Instant::now();
    assert_eq!(
        rig.cli()
            .close_within(MACHINE, AGENT, Duration::from_millis(300)),
        Err("timeout".into())
    );
    assert!(began.elapsed() < Duration::from_secs(2));
    // Killed at 300 ms, the fixture may not have kept its call yet on a
    // loaded host; it is never started twice.
    assert!(rig.calls().len() <= 1);
}

/// Harness 0.5.42's `harness start` reaches the service user's systemd user
/// manager (`systemctl --user`), which a system unit's process can't find
/// without `XDG_RUNTIME_DIR` (#5385). Each call is given the user's own
/// runtime folder and its bus when they exist, and never overrides a set one.
#[test]
fn every_harness_call_is_given_the_service_users_own_bus() {
    use lamp_tasks::harness::with_user_bus;
    use std::path::Path;
    let root = Path::new("/run/user");
    let base: BTreeMap<String, OsString> = [
        ("HOME".to_owned(), OsString::from("/var/lib/lampos-tasks")),
        (
            "PATH".to_owned(),
            OsString::from("/usr/local/bin:/usr/bin:/bin"),
        ),
    ]
    .into_iter()
    .collect();
    let lingering = |place: &Path| {
        place == Path::new("/run/user/996") || place == Path::new("/run/user/996/bus")
    };
    let given = with_user_bus(base.clone(), root, 996, &lingering);
    assert_eq!(given["XDG_RUNTIME_DIR"], OsString::from("/run/user/996"));
    assert_eq!(
        given["DBUS_SESSION_BUS_ADDRESS"],
        OsString::from("unix:path=/run/user/996/bus")
    );
    assert_eq!(given["HOME"], base["HOME"]);

    // No user manager (not lingering): nothing is made up.
    assert_eq!(
        with_user_bus(base.clone(), root, 996, &|_: &Path| false),
        base
    );

    // A runtime folder with no bus yet: the folder only.
    let folder_only = with_user_bus(base.clone(), root, 996, &|place: &Path| {
        place == Path::new("/run/user/996")
    });
    assert!(folder_only.contains_key("XDG_RUNTIME_DIR"));
    assert!(!folder_only.contains_key("DBUS_SESSION_BUS_ADDRESS"));

    // Set by the unit: kept as it is.
    let mut set = base.clone();
    set.insert("XDG_RUNTIME_DIR".into(), "/run/user/1000".into());
    set.insert(
        "DBUS_SESSION_BUS_ADDRESS".into(),
        "unix:path=/elsewhere".into(),
    );
    assert_eq!(with_user_bus(set.clone(), root, 996, &lingering), set);
}

/// The reboot race (#5393's reads): lamp-tasks may start before logind has
/// made the lingering user's runtime folder. Each call looks again, so a
/// folder that appears after the start is given to the next call, with no
/// restart of lamp-tasks.
#[test]
fn a_runtime_folder_made_after_the_start_reaches_the_next_harness_call() {
    use std::os::unix::fs::MetadataExt;
    let rig = Rig::new();
    let uid = fs::metadata(rig.root.path()).unwrap().uid();
    let cli = Cli::new(rig.config.clone(), "stable".into()).unwrap();
    let before = cli.child_environment();
    assert!(!before.contains_key("XDG_RUNTIME_DIR"), "{before:?}");
    assert!(!before.contains_key("DBUS_SESSION_BUS_ADDRESS"));

    let folder = rig.config.user_runtime_root.join(uid.to_string());
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("bus"), b"").unwrap();
    let after = cli.child_environment();
    assert_eq!(after["XDG_RUNTIME_DIR"], folder.as_os_str());
    let mut address = OsString::from("unix:path=");
    address.push(folder.join("bus").as_os_str());
    assert_eq!(after["DBUS_SESSION_BUS_ADDRESS"], address);
    assert_eq!(after["HOME"], before["HOME"]);
}

/// One `harness watch --json` line as openharness prints it.
fn watched_line(event: &str, id: &str) -> String {
    json!({"event":event,"at":"2026-11-04T09:00:00.000Z","name":"lamp-task-1","id":id,"agent":"claude","state":"idle"}).to_string()
}

/// #5230: `watch` hands on each event as it comes, and on `done` kills the
/// watch itself, which would otherwise stay held on an idle pane harness.
#[test]
fn watch_hands_on_each_event_and_ends_itself_on_done() {
    let rig = Rig::new();
    rig.plan(json!({"watch":{"lines":[
        {"text":watched_line("working", AGENT)},
        {"text":"not json"},
        {"text":watched_line("needs_person", AGENT),"delay_ms":50},
        {"text":watched_line("done", AGENT),"delay_ms":50},
    ],"after_output_ms":5000}}));
    let cancel = AtomicBool::new(false);
    let mut events = Vec::new();
    let began = Instant::now();
    let end = rig
        .cli()
        .watch(MACHINE, AGENT, &cancel, &mut |event| events.push(event));
    assert_eq!(end, WatchEnd::Settled);
    assert_eq!(
        events,
        vec![
            WatchEvent::Working,
            WatchEvent::NeedsPerson,
            WatchEvent::Done
        ]
    );
    // Killed at `done`, not held for the fixture's five seconds.
    assert!(
        began.elapsed() < Duration::from_secs(4),
        "{:?}",
        began.elapsed()
    );
    let call = rig
        .calls()
        .into_iter()
        .find(|call| call["args"][0] == "watch")
        .unwrap();
    assert_eq!(
        call["args"],
        json!([
            "watch",
            "--machine",
            MACHINE,
            AGENT,
            "--chars",
            "200",
            "--timeout",
            "10m",
            "--json"
        ])
    );
}

/// #5230: each way `watch` ends, by its exit.
#[test]
fn watch_reads_each_exit() {
    let rig = Rig::new();
    let cancel = AtomicBool::new(false);
    let run = |plan: Value| {
        rig.plan(json!({ "watch": plan }));
        let mut events = Vec::new();
        let end = rig
            .cli()
            .watch(MACHINE, AGENT, &cancel, &mut |event| events.push(event));
        (end, events)
    };
    assert_eq!(
        run(json!({"stdout":format!("{}\n", watched_line("stopped", AGENT)),"exit":4})),
        (WatchEnd::Stopped, vec![WatchEvent::Stopped])
    );
    assert_eq!(
        run(json!({"stdout":format!("{}\n", watched_line("working", AGENT)),"exit":124})),
        (WatchEnd::Timeout, vec![WatchEvent::Working])
    );
    // An older Harness: a usage error.
    assert_eq!(run(json!({"stdout":"","exit":2})).0, WatchEnd::Missing);
    // An error ends it with Harness's code; its detail is never kept.
    assert_eq!(
        run(
            json!({"stdout":"{\"error\":\"DISCONNECTED\",\"detail\":\"/home/someone\"}\n","exit":1})
        )
        .0,
        WatchEnd::Failed("DISCONNECTED".into())
    );
    assert_eq!(
        run(json!({"stdout":"","exit":1})).0,
        WatchEnd::Failed("watch_failed".into())
    );
    // A line for another agent is passed over.
    assert_eq!(
        run(json!({"stdout":format!("{}\n", watched_line("done", OTHER)),"exit":124})),
        (WatchEnd::Timeout, vec![])
    );
    // `failed` ends it, as `done` does.
    assert_eq!(
        run(json!({"lines":[{"text":watched_line("failed", AGENT)}],"after_output_ms":5000})),
        (WatchEnd::Settled, vec![WatchEvent::Failed])
    );
}

/// #5230: a cancel ends a held `watch`, and its process is killed with it.
#[test]
fn watch_is_cancelled_and_its_process_killed() {
    let rig = Rig::new();
    rig.plan(json!({"watch":{"lines":[{"text":watched_line("working", AGENT)}],"held_descendant":true,"after_output_ms":5000}}));
    let cli = Arc::new(rig.cli());
    let cancel = Arc::new(AtomicBool::new(false));
    let (lane, flag) = (cli.clone(), cancel.clone());
    let held = thread::spawn(move || lane.watch(MACHINE, AGENT, &flag, &mut |_| {}));
    rig.wait_operation("watch");
    thread::sleep(Duration::from_millis(100));
    cancel.store(true, Ordering::Release);
    assert_eq!(held.join().unwrap(), WatchEnd::Cancelled);
    rig.assert_descendant_killed();
}

/// #5230: a watch line is read for its event and id only, and one longer
/// than the bound is refused.
#[test]
fn a_watch_line_is_read_for_its_event_only() {
    assert_eq!(
        parse_watch_line(watched_line("working", AGENT).as_bytes(), AGENT),
        Ok(WatchLine::Event(WatchEvent::Working))
    );
    assert_eq!(
        parse_watch_line(watched_line("working", AGENT).as_bytes(), OTHER),
        Err("watch_agent_mismatch")
    );
    assert_eq!(
        parse_watch_line(watched_line("exploded", AGENT).as_bytes(), AGENT),
        Err("watch_invalid_line")
    );
    assert_eq!(
        parse_watch_line(br#"{"error":"lowercase words"}"#, AGENT),
        Ok(WatchLine::Error("?".into()))
    );
    let long = vec![b' '; WATCH_LINE_LIMIT + 1];
    assert_eq!(parse_watch_line(&long, AGENT), Err("watch_invalid_line"));
    for event in [
        "started",
        "working",
        "needs_person",
        "done",
        "idle",
        "failed",
        "stopped",
    ] {
        let read = parse_watch_line(watched_line(event, AGENT).as_bytes(), AGENT).unwrap();
        let WatchLine::Event(read) = read else {
            panic!()
        };
        assert_eq!(read.name(), event);
        assert_eq!(read.settled(), !matches!(event, "started" | "working"));
    }
}

/// #5978, `LAMP_HERMES_ON_LAMP`: a named hand-over to the lamp's own
/// target (Hermes on the lamp's machine) looks for the session on the lamp
/// and on each computer: one on the lamp wins a tie with a computer's, the
/// lamp's own rules first (a stopped one there is said stopped); one only
/// on a computer is typed into there and said so (`joined_on`); two
/// computers that both fit are asked about. A hand-over to a computer, the
/// person having named it, looks on that computer alone.
#[test]
fn a_named_session_is_looked_for_on_the_lamp_first_then_the_computers() {
    const LAPTOP: &str = "fedcba9876543210fedcba9876543210";
    const DESK: &str = "abababababababababababababababab";
    let lamp_launch = || {
        let mut request = launch();
        request.target.engine = EngineKind::Hermes;
        request.target.project = "/var/lib/lampos-hermes/lamp-tasks".into();
        request.session = Some("docs".into());
        request
    };
    let ps = |rows: Value| step(json!({"you":null,"machine":"m","harnesses":rows,"next":[]}));
    let took =
        |id: &str| step(json!({"ok":true,"name":"x","id":id,"message":"m1","state":"queued"}));
    let run = |request: Launch, each: Vec<Value>, computers: &[(&str, &str)], reply: &str| {
        let mut rig = Rig::new();
        rig.config.reply_named = true;
        rig.config.session_machines = Some(Arc::new(Mutex::new(SessionMachines {
            lamp: Some(MACHINE.into()),
            computers: computers
                .iter()
                .map(|(alias, id)| ((*alias).into(), (*id).into(), EngineKind::Claude))
                .collect(),
            offline: Default::default(),
        })));
        rig.plan(json!({
            "machines":{"default":true},"ps":{"each":each},"reply":took(reply)
        }));
        let outcome = rig.cli().launch(&request);
        let calls: Vec<Value> = rig
            .calls()
            .iter()
            .map(|call| call["args"].clone())
            .collect();
        (outcome, calls)
    };
    let docs = |id: &str, state: &str| json!([ps_row(id, "docs", state, None)]);
    let none = json!([]);
    // Both fit: the lamp's, and the computer's list is never read (#5978:
    // the lookup took 15.2 s on lamp-c0a4 reading every computer).
    let (outcome, calls) = run(
        lamp_launch(),
        vec![ps(docs(AGENT, "idle")), ps(docs(OTHER, "idle"))],
        &[("office", LAPTOP)],
        AGENT,
    );
    assert!(
        matches!(&outcome, LaunchOutcome::Joined { agent_id, .. } if agent_id == AGENT),
        "{outcome:?}"
    );
    assert_eq!(
        calls[1],
        json!(["ps", "--machine", MACHINE, "--all", "--json"])
    );
    assert_eq!(calls[2][0], "reply");
    assert_eq!(calls[2][2], MACHINE);
    assert_eq!(calls.iter().filter(|call| call[0] == "ps").count(), 1);
    // Only on a computer: typed there, and followed there.
    let (outcome, calls) = run(
        lamp_launch(),
        vec![ps(none.clone()), ps(docs(OTHER, "idle"))],
        &[("office", LAPTOP)],
        OTHER,
    );
    match outcome {
        LaunchOutcome::JoinedOn { agent_id, on, .. } => {
            assert_eq!(agent_id, OTHER);
            assert_eq!(
                on,
                JoinedOn {
                    alias: "office".into(),
                    machine_id: LAPTOP.into(),
                    engine: EngineKind::Claude,
                }
            );
        }
        outcome => panic!("{outcome:?}"),
    }
    assert_eq!(
        calls[3],
        json!([
            "reply",
            "--machine",
            LAPTOP,
            OTHER,
            "--json",
            "--",
            "Inspect the authored fixture"
        ])
    );
    // A stopped one on the lamp and a live one on a computer: the lamp's
    // rule first, nothing typed.
    let (outcome, _) = run(
        lamp_launch(),
        vec![ps(docs(AGENT, "stopped")), ps(docs(OTHER, "idle"))],
        &[("office", LAPTOP)],
        OTHER,
    );
    assert_eq!(
        outcome,
        LaunchOutcome::NotSent {
            code: "session_stopped".into()
        }
    );
    // Two computers both fit, none on the lamp: asked about.
    let (outcome, calls) = run(
        lamp_launch(),
        vec![
            ps(none.clone()),
            ps(docs(OTHER, "idle")),
            ps(docs(AGENT, "idle")),
        ],
        &[("office", LAPTOP), ("desk", DESK)],
        OTHER,
    );
    assert_eq!(
        outcome,
        LaunchOutcome::NotSent {
            code: "session_ambiguous".into()
        }
    );
    assert!(!calls.iter().any(|call| call[0] == "reply"), "{calls:?}");
    // Nowhere: not found.
    let (outcome, _) = run(
        lamp_launch(),
        vec![ps(none.clone()), ps(none.clone())],
        &[("office", LAPTOP)],
        OTHER,
    );
    assert_eq!(
        outcome,
        LaunchOutcome::NotSent {
            code: "session_not_found".into()
        }
    );
    // A machine named (a computer's target, not the lamp's): that one only.
    let mut computer = lamp_launch();
    computer.target.engine = EngineKind::Codex;
    let (outcome, calls) = run(
        computer,
        vec![ps(docs(AGENT, "idle")), ps(docs(OTHER, "idle"))],
        &[("office", LAPTOP)],
        AGENT,
    );
    assert!(
        matches!(outcome, LaunchOutcome::Joined { .. }),
        "{outcome:?}"
    );
    assert_eq!(calls.iter().filter(|call| call[0] == "ps").count(), 1);
}

/// #6226: a named hand-over to the lamp's own target with OpenClaw looks
/// among the computers too, as with Hermes: one only on a computer is
/// typed into there and said so (`joined_on`).
#[test]
fn a_named_session_for_the_lamps_openclaw_is_looked_for_on_the_computers_too() {
    const LAPTOP: &str = "fedcba9876543210fedcba9876543210";
    let mut request = launch();
    request.target.engine = EngineKind::Openclaw;
    request.session = Some("docs".into());
    let ps = |rows: Value| step(json!({"you":null,"machine":"m","harnesses":rows,"next":[]}));
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.config.session_machines = Some(Arc::new(Mutex::new(SessionMachines {
        lamp: Some(MACHINE.into()),
        computers: vec![("office".into(), LAPTOP.into(), EngineKind::Claude)],
        offline: Default::default(),
    })));
    rig.plan(json!({
        "machines":{"default":true},
        "ps":{"each":[ps(json!([])), ps(json!([ps_row(OTHER, "docs", "idle", None)]))]},
        "reply":step(json!({"ok":true,"name":"x","id":OTHER,"message":"m1","state":"queued"}))
    }));
    match rig.cli().launch(&request) {
        LaunchOutcome::JoinedOn { agent_id, on, .. } => {
            assert_eq!(agent_id, OTHER);
            assert_eq!(on.machine_id, LAPTOP);
            assert_eq!(on.engine, EngineKind::Claude);
        }
        outcome => panic!("{outcome:?}"),
    }
}

/// #5978: the machines a named hand-over to the lamp looks among, from
/// the targets: the lamp's own (Hermes) apart, each computer once, no cloud
/// agent.
#[test]
fn the_session_machines_come_from_the_targets() {
    let target = |id: char, engine: EngineKind| Target {
        machine_id: id.to_string().repeat(32),
        project: "~/harnesses/lamp-tasks".into(),
        engine,
    };
    let targets = BTreeMap::from([
        ("lamp".to_owned(), target('a', EngineKind::Hermes)),
        ("office".to_owned(), target('b', EngineKind::Claude)),
        ("office-codex".to_owned(), target('b', EngineKind::Codex)),
        (
            "devin".to_owned(),
            Target {
                project: String::new(),
                ..target('c', EngineKind::DevinCloud)
            },
        ),
        ("laptop".to_owned(), target('d', EngineKind::Codex)),
    ]);
    let machines = SessionMachines::from_targets(&targets);
    assert_eq!(machines.lamp, Some("a".repeat(32)));
    assert_eq!(
        machines.computers,
        [
            ("laptop".to_owned(), "d".repeat(32), EngineKind::Codex),
            ("office".to_owned(), "b".repeat(32), EngineKind::Claude),
        ]
    );
    // No lamp target: no lamp, the computers as they are.
    let mut without = targets.clone();
    without.remove("lamp");
    assert_eq!(SessionMachines::from_targets(&without).lamp, None);
    // #6226: the lamp with OpenClaw is the lamp's own as with Hermes.
    let mut openclaw = targets.clone();
    openclaw.insert("lamp".to_owned(), target('a', EngineKind::Openclaw));
    let machines = SessionMachines::from_targets(&openclaw);
    assert_eq!(machines.lamp, Some("a".repeat(32)));
    assert_eq!(machines.computers.len(), 2);
}

/// #6226: OpenClaw needs Harness CLI 0.6.11 or later; the lamp's pinned
/// 0.6.2 is refused, and a version not read is no version.
#[test]
fn the_harness_version_is_read_for_openclaw() {
    use lamp_tasks::harness::{OPENCLAW_HARNESS, harness_version};
    assert_eq!(harness_version(b"0.6.12_cli\n"), Some((0, 6, 12)));
    assert_eq!(harness_version(b"harness v0.6.11\n"), Some((0, 6, 11)));
    assert_eq!(harness_version(b"0.6.2\n"), Some((0, 6, 2)));
    assert_eq!(harness_version(b"0.6\n"), None);
    assert_eq!(harness_version(b""), None);
    assert_eq!(harness_version(b"x\n0.6.12\n"), None, "first line only");
    assert!(harness_version(b"0.6.2").unwrap() < OPENCLAW_HARNESS);
    assert!(harness_version(b"0.6.10").unwrap() < OPENCLAW_HARNESS);
    assert!(harness_version(b"0.6.11_cli").unwrap() >= OPENCLAW_HARNESS);
    assert!(harness_version(b"1.0.0").unwrap() >= OPENCLAW_HARNESS);
}

/// #6226: the Harness on the lamp is asked its version before OpenClaw is
/// offered: 0.6.2 (the lamp's pin) and a failed answer are no, 0.6.12 yes.
#[test]
fn openclaw_is_offered_only_by_a_harness_from_0_6_11() {
    for (version, want) in [
        (json!({"stdout":"0.6.12_cli\n"}), true),
        (json!({"stdout":"0.6.11\n"}), true),
        (json!({"stdout":"0.6.2\n"}), false),
        (json!({"stdout":"0.6.12\n","exit":1}), false),
    ] {
        let rig = Rig::new();
        rig.plan(json!({"--version": version.clone()}));
        assert_eq!(rig.cli().runs_openclaw(), want, "{version}");
    }
}

/// #6274: a name said nearly right, one word misheard or misspelt, finds
/// the one closest session when a look at it (`look_session`) finds
/// nothing by its name, folder or work: one slip in a word of 4 to 7
/// letters, two from 8, none under 4 ("api" never fits
/// "rapid-prototype"); two as close are asked about. The hand-over that
/// types into a session takes a name as it is only (the Reviewer-lead's
/// P2 on #6422): its read-back says the person's own words, so a near
/// pick could not be caught by their yes.
#[test]
fn a_name_said_nearly_right_finds_the_one_closest_session() {
    let looked = |session: &str, rows: Value| {
        let mut rig = Rig::new();
        rig.config.reply_named = true;
        rig.plan(json!({
            "machines":{"default":true},
            "ps":step(json!({"you":null,"machine":"desk-pc","harnesses":rows,"next":[]})),
            "reply":{"default":true},
        }));
        rig.cli()
            .look_session(&launch().target, session, false)
            .map(|seen| (seen.name.unwrap_or_default(), seen.near))
    };
    let survey = || Ok(("Customer Survey".to_owned(), true));
    let rows = json!([
        ps_row(AGENT, "Customer Survey", "idle", Some("/w/survey")),
        ps_row(OTHER, "login fix", "idle", Some("/w/auth")),
    ]);
    for said in [
        "costumer survey",
        "survay",
        "customer survay",
        "cusotmer survey",
        "Customer Srvey",
        // Two slips in a word of eight letters or more.
        "custommmer",
    ] {
        assert_eq!(looked(said, rows.clone()), survey(), "{said}");
    }
    // As it is: found so, not near.
    assert_eq!(
        looked("customer survey", rows.clone()),
        Ok(("Customer Survey".to_owned(), false))
    );
    // Two slips in a shorter word, or a slip in a word under four letters:
    // not that session.
    let none = || Err("session_not_found".to_owned());
    assert_eq!(looked("svrvay", rows.clone()), none());
    let short = json!([ps_row(AGENT, "docs", "idle", None)]);
    assert_eq!(looked("dox", short), none());
    // Never "api" for "rapid-prototype" (the Reviewer-lead's P2 on #5779).
    let near = json!([ps_row(AGENT, "rapid-prototype", "idle", Some("/w/rapid"))]);
    assert_eq!(looked("api", near), none());
    // Two as close: asked about, never guessed.
    let two = json!([
        ps_row(AGENT, "Customer Survey", "idle", None),
        ps_row(OTHER, "Customer Surveys", "idle", None),
    ]);
    assert_eq!(
        looked("costumer survey", two),
        Err("session_ambiguous".to_owned())
    );
    // The closer of two is the one.
    let closer = json!([
        ps_row(OTHER, "Customers Survey", "idle", None),
        ps_row(AGENT, "Customer Survey", "idle", None),
    ]);
    assert_eq!(looked("costumer survey", closer), survey());
    // The hand-over that types: a name said nearly right is no session of
    // theirs, nothing typed.
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.plan(json!({
        "machines":{"default":true},
        "ps":step(json!({"you":null,"machine":"desk-pc","harnesses":rows,"next":[]})),
        "reply":step(json!({"ok":true,"name":"x","id":AGENT,"message":"m1","state":"queued"})),
    }));
    let mut request = launch();
    request.session = Some("costumer survey".into());
    assert_eq!(
        bare(rig.cli().launch(&request)),
        not_sent("session_not_found")
    );
    assert!(
        rig.calls().iter().all(|call| call["args"][0] != "reply"),
        "typed into a near pick"
    );
}

/// #6274, the Reviewer-lead's P2 on #6422: "open the costumer survey one"
/// never reopens a closed session found only by a name said nearly right:
/// it is told as closed, the closest found, and only its name said as it
/// is reopens it (`LAMP_TASKS_RESUME_CLOSED`), nothing resumed before.
#[test]
fn a_near_name_never_reopens_a_closed_session() {
    let mut rig = Rig::new();
    rig.config.reply_named = true;
    rig.config.resume_closed = true;
    let stopped = json!([stopped_row(
        AGENT,
        "Customer Survey",
        "2026-11-10T05:00:00.000Z"
    )]);
    rig.plan(json!({
        "machines":{"default":true},
        "ps":step(json!({"you":null,"machine":"desk-pc","harnesses":stopped,"next":[]})),
        "--help":{"stdout":"Commands:\n  resume   Reopen a closed harness\n"},
        "resume":step(json!({"ok":true,"name":"Customer Survey","id":AGENT,"state":"idle","resumed":true,"effect":"x","next":[]})),
        "reply":{"default":true},
        "new":{"default":true},
    }));
    let seen = rig
        .cli()
        .look_session(&launch().target, "costumer survey", true)
        .unwrap();
    assert_eq!(
        (seen.state.as_str(), seen.resumed, seen.near),
        ("stopped", None, true)
    );
    let verbs: Vec<String> = rig
        .calls()
        .iter()
        .map(|call| call["args"][0].as_str().unwrap().to_owned())
        .collect();
    // Read only: its list, and its last ask (`show`), never `resume`.
    assert!(
        verbs.iter().all(|verb| verb == "ps" || verb == "show"),
        "{verbs:?}"
    );
}

/// #6274, the Reviewer-lead's P2 on #6422: a near match on the lamp's own
/// machine never wins before the computers are read. Every machine is
/// listed, a session named as it is anywhere beats any near one ("test":
/// the Mac's "Test runner", not the lamp's "Text formatter"), and near
/// ones as close on two machines are asked about.
#[test]
fn a_name_as_it_is_on_a_computer_beats_a_near_one_on_the_lamp() {
    const LAPTOP: &str = "fedcba9876543210fedcba9876543210";
    let ps = |rows: Value| step(json!({"you":null,"machine":"m","harnesses":rows,"next":[]}));
    let looked = |session: &str, each: Vec<Value>| {
        let mut rig = Rig::new();
        rig.config.reply_named = true;
        rig.config.session_machines = Some(Arc::new(Mutex::new(SessionMachines {
            lamp: Some(MACHINE.into()),
            computers: vec![("office".into(), LAPTOP.into(), EngineKind::Claude)],
            offline: Default::default(),
        })));
        rig.plan(json!({"machines":{"default":true},"ps":{"each":each},"show":{"default":true}}));
        let mut target = launch().target;
        target.engine = EngineKind::Hermes;
        target.project = "/var/lib/lampos-hermes/lamp-tasks".into();
        let seen = rig.cli().look_session(&target, session, false);
        let lists = rig
            .calls()
            .iter()
            .filter(|call| call["args"][0] == "ps")
            .count();
        (
            seen.map(|seen| (seen.name.unwrap_or_default(), seen.on, seen.near)),
            lists,
        )
    };
    let lamp = json!([ps_row(AGENT, "Text formatter", "idle", None)]);
    let mac = json!([ps_row(OTHER, "Test runner", "idle", None)]);
    let (seen, lists) = looked("test", vec![ps(lamp.clone()), ps(mac)]);
    assert_eq!(
        seen,
        Ok(("Test runner".to_owned(), Some("office".to_owned()), false))
    );
    assert_eq!(lists, 2);
    // Near only on the lamp: found there, but only once the Mac was read.
    let (seen, lists) = looked("test", vec![ps(lamp.clone()), ps(json!([]))]);
    assert_eq!(seen, Ok(("Text formatter".to_owned(), None, true)));
    assert_eq!(lists, 2);
    // As near on both: asked about.
    let mac = json!([ps_row(OTHER, "Tent formatter", "idle", None)]);
    let (seen, _) = looked("test", vec![ps(lamp), ps(mac)]);
    assert_eq!(seen, Err("session_ambiguous".to_owned()));
}

/// #6274: a look at the session the person named, with no words to type.
/// To open it (`join`): one idle live session is open as it is, one busy
/// says so, one stopped is reopened only with `LAMP_TASKS_RESUME_CLOSED`.
/// To say how it is: whatever fits is told as it stands, stopped too, a
/// session the lamp never handed anything included. More than one fitting
/// is `session_ambiguous`, none `session_not_found`; nothing is ever typed
/// or started, and with the switch off nothing runs.
#[test]
fn a_named_session_is_looked_at_to_open_or_say_how_it_is_with_nothing_typed() {
    let looked = |session: &str, rows: Value, join: bool, resume: bool| {
        let mut rig = Rig::new();
        rig.config.reply_named = true;
        rig.config.resume_closed = resume;
        rig.plan(json!({
            "machines":{"default":true},
            "ps":step(json!({"you":null,"machine":"desk-pc","harnesses":rows,"next":[]})),
            "--help":{"stdout":"Commands:\n  resume   Reopen a closed harness\n"},
            "resume":step(json!({"ok":true,"name":"Customer Survey","id":AGENT,"state":"idle","resumed":true,"effect":"x","next":[]})),
            "reply":{"default":true},
            "new":{"default":true},
        }));
        let target = launch().target;
        let seen = rig.cli().look_session(&target, session, join);
        let verbs: Vec<String> = rig
            .calls()
            .iter()
            .map(|call| call["args"][0].as_str().unwrap().to_owned())
            .collect();
        (seen, verbs)
    };
    let mut theirs = ps_row(AGENT, "Customer Survey", "working", Some("/w/survey"));
    theirs["asks"] = json!(["Summarise the customer survey answers"]);
    theirs["about"] = json!("Reading   the answers\n of week two");
    let rows = json!([theirs, ps_row(OTHER, "login fix", "idle", None)]);
    // How it is: its own state, by a name said nearly right, nothing typed.
    let (seen, verbs) = looked("costumer survey", rows.clone(), false, false);
    let seen = seen.unwrap();
    assert_eq!(seen.state, "working");
    assert_eq!(seen.name.as_deref(), Some("Customer Survey"));
    assert_eq!(
        seen.ask.as_deref(),
        Some("Summarise the customer survey answers")
    );
    assert_eq!(
        seen.about.as_deref(),
        Some("Reading the answers of week two")
    );
    assert!(seen.last_active_unix_ms.is_some());
    assert_eq!(seen.resumed, None);
    assert_eq!(verbs, ["ps"]);
    // Opened while it works: said busy as it stands, nothing typed.
    let (seen, verbs) = looked("customer survey", rows.clone(), true, false);
    assert_eq!(seen.unwrap().state, "working");
    assert_eq!(verbs, ["ps"]);
    // Opened while idle: open as it is.
    let idle = json!([ps_row(AGENT, "Customer Survey", "idle", None)]);
    let (seen, verbs) = looked("customer survey", idle, true, false);
    assert_eq!(seen.unwrap().state, "idle");
    assert_eq!(verbs, ["ps"]);
    // Stopped: told stopped when asked about, reopened to open it only with
    // the switch, and said stopped without it.
    let stopped = json!([stopped_row(
        AGENT,
        "Customer Survey",
        "2026-11-10T05:00:00.000Z"
    )]);
    let (seen, verbs) = looked("customer survey", stopped.clone(), false, true);
    assert_eq!(seen.unwrap().state, "stopped");
    assert_eq!(verbs, ["ps"]);
    let (seen, verbs) = looked("customer survey", stopped.clone(), true, false);
    assert_eq!(seen, Err("session_stopped".to_owned()));
    assert_eq!(verbs, ["ps"]);
    let (seen, verbs) = looked("customer survey", stopped, true, true);
    let seen = seen.unwrap();
    assert_eq!((seen.state.as_str(), seen.resumed), ("idle", Some(true)));
    assert_eq!(verbs, ["ps", "--help", "resume"]);
    // None fits, or two do: never a guess.
    let (seen, _) = looked("billing", rows.clone(), false, false);
    assert_eq!(seen, Err("session_not_found".to_owned()));
    let two = json!([
        ps_row(AGENT, "survey one", "idle", None),
        ps_row(OTHER, "survey two", "idle", None),
    ]);
    let (seen, _) = looked("survey", two, true, false);
    assert_eq!(seen, Err("session_ambiguous".to_owned()));
    // Off: nothing runs.
    let rig = Rig::new();
    assert_eq!(
        rig.cli()
            .look_session(&launch().target, "customer survey", true),
        Err("reply_named_off".to_owned())
    );
    assert!(rig.calls().is_empty());
}

/// #6274, the first builder's ruling of 11 Nov: the sessions the lamp
/// itself started or handed work to, kept in an index of their id, name
/// and last use only (`lamp_tasks::session_index`), in the tasks folder.
fn indexed(rig: &mut Rig) -> PathBuf {
    let file = rig.root.path().join(SESSION_INDEX_FILE);
    rig.config.session_index = Some(file.clone());
    file
}

/// What `harness show --json` gives of a session by its id, as Harness
/// 0.6.12 answers it: its name and state among the rest.
fn shown_as(id: &str, name: &str, state: &str) -> Value {
    step(
        json!({"name":name,"id":id,"agent":"Hermes","machine":"lamp","state":state,
        "startedBy":"you","folder":"/var/lib/lampos-hermes/lamp-tasks","branch":null,
        "ask":"Find the Christmas dates","answer":"Done.","truncated":false,"tools":1,"next":[]}),
    )
}

/// The verbs called, in turn, each with the id it named if any.
fn verbs_named(rig: &Rig) -> Vec<String> {
    rig.calls()
        .iter()
        .map(|call| {
            let args = call["args"].as_array().unwrap();
            let verb = args[0].as_str().unwrap().to_owned();
            match args
                .iter()
                .filter_map(Value::as_str)
                .find(|arg| *arg == AGENT || *arg == OTHER)
            {
                Some(id) => format!("{verb} {}", &id[..8]),
                None => verb,
            }
        })
        .collect()
}

/// #6274: a session the lamp started more than seven days ago is no longer
/// in `harness ps --all`, so "open the Christmas dates one" found nothing.
/// Its id and name in the index find it: `show <id>` says it is there and
/// how, and it is opened or reopened by its id as a listed one is (resume
/// only with `LAMP_TASKS_RESUME_CLOSED`, the hand-over typed after it).
#[test]
fn an_older_session_not_listed_is_found_by_its_name_through_the_index() {
    let none = || step(json!({"you":null,"machine":"lamp","harnesses":[],"next":[]}));
    let lamp_side = |rig: &mut Rig, show: Value| {
        let file = indexed(rig);
        session_index::record(&file, MACHINE, AGENT, Some("Christmas dates"), 1_000);
        session_index::record(&file, MACHINE, OTHER, Some("Login fix"), 2_000);
        rig.config.reply_named = true;
        rig.config.resume_closed = true;
        rig.plan(json!({
            "machines":{"default":true},
            "ps":none(),
            "show":{"by_arg":{AGENT:show}},
            "--help":{"stdout":"Commands:\n  resume   Reopen a closed harness\n"},
            "resume":step(json!({"ok":true,"name":"Christmas dates","id":AGENT,"state":"idle","resumed":true,"effect":"x","next":[]})),
            "reply":step(json!({"ok":true,"name":"Christmas dates","id":AGENT,"message":"m1","state":"queued"})),
        }));
        file
    };
    // Open it, stopped: reopened by its id.
    let mut rig = Rig::new();
    let file = lamp_side(&mut rig, shown_as(AGENT, "Christmas dates", "stopped"));
    let seen = rig
        .cli()
        .look_session(&launch().target, "christmas dates", true)
        .unwrap();
    assert_eq!(
        (
            seen.state.as_str(),
            seen.resumed,
            seen.near,
            seen.name.as_deref()
        ),
        ("idle", Some(true), false, Some("Christmas dates"))
    );
    assert_eq!(
        verbs_named(&rig),
        ["ps", "show 11111111", "--help", "resume 11111111"]
    );
    // Opened, so used now: later than the 1 000 it was kept with.
    let kept = session_index::read(&file);
    let entry = kept[MACHINE]
        .iter()
        .find(|entry| entry.id == AGENT)
        .unwrap();
    assert!(entry.last_used > 1_000, "{entry:?}");
    // Idle: open as it is, nothing reopened.
    let mut rig = Rig::new();
    lamp_side(&mut rig, shown_as(AGENT, "Christmas dates", "idle"));
    let seen = rig
        .cli()
        .look_session(&launch().target, "christmas dates", true)
        .unwrap();
    assert_eq!((seen.state.as_str(), seen.resumed), ("idle", None));
    assert_eq!(verbs_named(&rig), ["ps", "show 11111111"]);
    // A hand-over that names it: reopened by its id, and the words typed.
    let mut rig = Rig::new();
    lamp_side(&mut rig, shown_as(AGENT, "Christmas dates", "stopped"));
    let mut request = launch();
    request.session = Some("christmas dates".into());
    let outcome = rig.cli().launch(&request);
    assert!(
        matches!(&outcome, LaunchOutcome::Joined { agent_id, session: Some(session) }
            if agent_id == AGENT && session.resumed == Some(true)),
        "{outcome:?}"
    );
    assert_eq!(
        verbs_named(&rig),
        [
            "machines",
            "ps",
            "show 11111111",
            "--help",
            "resume 11111111",
            "reply 11111111"
        ]
    );
    // Without the switch: said stopped, nothing reopened or typed.
    let mut rig = Rig::new();
    lamp_side(&mut rig, shown_as(AGENT, "Christmas dates", "stopped"));
    rig.config.resume_closed = false;
    assert_eq!(
        rig.cli()
            .look_session(&launch().target, "christmas dates", true),
        Err("session_stopped".to_owned())
    );
    assert_eq!(verbs_named(&rig), ["ps", "show 11111111"]);
}

/// #6274: a session listed by `ps --all` is found there as before, never
/// read again through the index; and the index never finds what the lamp
/// did not start or hand work to.
#[test]
fn a_listed_session_is_found_as_before_and_the_index_only_adds() {
    let mut rig = Rig::new();
    let file = indexed(&mut rig);
    session_index::record(&file, MACHINE, AGENT, Some("Christmas dates"), 1_000);
    rig.config.reply_named = true;
    rig.plan(json!({
        "machines":{"default":true},
        "ps":step(json!({"you":null,"machine":"lamp","harnesses":[
            ps_row(AGENT, "Christmas dates", "idle", None)
        ],"next":[]})),
        "show":{"default":true},
    }));
    let seen = rig
        .cli()
        .look_session(&launch().target, "christmas dates", false)
        .unwrap();
    assert_eq!(seen.state, "idle");
    assert_eq!(verbs_named(&rig), ["ps"]);
    let mut rig = Rig::new();
    indexed(&mut rig);
    rig.config.reply_named = true;
    rig.plan(json!({
        "machines":{"default":true},
        "ps":step(json!({"you":null,"machine":"lamp","harnesses":[],"next":[]})),
        "show":{"default":true},
    }));
    assert_eq!(
        rig.cli()
            .look_session(&launch().target, "christmas dates", false),
        Err("session_not_found".to_owned())
    );
    assert_eq!(verbs_named(&rig), ["ps"]);
}

/// #6274: an indexed session `show` says is no longer there
/// (`UNKNOWN_HARNESS`, `GONE`) is dropped from the index, and the lookup
/// goes on: none other fits, so `session_not_found`; a later lookup reads
/// nothing for it.
#[test]
fn an_indexed_session_that_is_gone_is_dropped() {
    for code in ["UNKNOWN_HARNESS", "GONE"] {
        let mut rig = Rig::new();
        let file = indexed(&mut rig);
        session_index::record(&file, MACHINE, AGENT, Some("Christmas dates"), 1_000);
        session_index::record(&file, MACHINE, OTHER, Some("Login fix"), 2_000);
        rig.config.reply_named = true;
        rig.plan(json!({
            "machines":{"default":true},
            "ps":step(json!({"you":null,"machine":"lamp","harnesses":[],"next":[]})),
            "show":{"by_arg":{AGENT:{"stdout":format!("{}\n", json!({"error":code,"detail":"No harness named that."})),"exit":1}}},
        }));
        assert_eq!(
            rig.cli()
                .look_session(&launch().target, "christmas dates", true),
            Err("session_not_found".to_owned()),
            "{code}"
        );
        let kept = session_index::read(&file);
        let ids: Vec<&str> = kept[MACHINE]
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        assert_eq!(ids, [OTHER], "{code}");
        // A show that only failed (a timeout, a computer away) drops nothing.
        let mut rig = Rig::new();
        let file = indexed(&mut rig);
        session_index::record(&file, MACHINE, AGENT, Some("Christmas dates"), 1_000);
        rig.config.reply_named = true;
        rig.plan(json!({
            "machines":{"default":true},
            "ps":step(json!({"you":null,"machine":"lamp","harnesses":[],"next":[]})),
            "show":{"by_arg":{AGENT:{"stdout":format!("{}\n", json!({"error":"DISCONNECTED"})),"exit":1}}},
        }));
        assert!(
            rig.cli()
                .look_session(&launch().target, "christmas dates", true)
                .is_err()
        );
        assert_eq!(session_index::read(&file)[MACHINE].len(), 1);
    }
}

/// #6274: two indexed sessions that both fit what was said, and are both
/// still there, are asked about, never guessed; one of them gone leaves the
/// other.
#[test]
fn an_ambiguous_index_match_is_asked_about() {
    let looked = |show_other: Value| {
        let mut rig = Rig::new();
        let file = indexed(&mut rig);
        session_index::record(&file, MACHINE, AGENT, Some("Christmas dates"), 1_000);
        session_index::record(&file, MACHINE, OTHER, Some("Christmas cards"), 2_000);
        rig.config.reply_named = true;
        rig.config.resume_closed = true;
        rig.plan(json!({
            "machines":{"default":true},
            "ps":step(json!({"you":null,"machine":"lamp","harnesses":[],"next":[]})),
            "show":{"by_arg":{
                AGENT:shown_as(AGENT, "Christmas dates", "stopped"),
                OTHER:show_other,
            }},
            "--help":{"stdout":"Commands:\n  resume   Reopen a closed harness\n"},
            "resume":step(json!({"ok":true,"name":"x","id":AGENT,"state":"idle","resumed":true,"effect":"x","next":[]})),
        }));
        let seen = rig.cli().look_session(&launch().target, "christmas", true);
        let resumed = rig.calls().iter().any(|call| call["args"][0] == "resume");
        (seen.map(|seen| seen.name), resumed)
    };
    assert_eq!(
        looked(shown_as(OTHER, "Christmas cards", "idle")),
        (Err("session_ambiguous".to_owned()), false)
    );
    let gone = json!({"stdout":format!("{}\n", json!({"error":"UNKNOWN_HARNESS"})),"exit":1});
    assert_eq!(looked(gone), (Ok(Some("Christmas dates".to_owned())), true));
}

/// #6274, the Reviewer-lead's P2 on #6422 held for the index too: a name
/// said nearly right finds an indexed session for a look only, said as it
/// stands (closed, and closest), never reopened; the hand-over that types
/// takes a name as it is only.
#[test]
fn the_index_never_makes_a_near_name_reopen_a_closed_session() {
    let rig_for = |rig: &mut Rig| {
        let file = indexed(rig);
        session_index::record(&file, MACHINE, AGENT, Some("Christmas dates"), 1_000);
        rig.config.reply_named = true;
        rig.config.resume_closed = true;
        rig.plan(json!({
            "machines":{"default":true},
            "ps":step(json!({"you":null,"machine":"lamp","harnesses":[],"next":[]})),
            "show":{"by_arg":{AGENT:shown_as(AGENT, "Christmas dates", "stopped")}},
            "--help":{"stdout":"Commands:\n  resume   Reopen a closed harness\n"},
            "resume":step(json!({"ok":true,"name":"x","id":AGENT,"state":"idle","resumed":true,"effect":"x","next":[]})),
            "reply":{"default":true},
        }));
    };
    let mut rig = Rig::new();
    rig_for(&mut rig);
    let seen = rig
        .cli()
        .look_session(&launch().target, "chrismas dates", true)
        .unwrap();
    assert_eq!(
        (seen.state.as_str(), seen.resumed, seen.near),
        ("stopped", None, true)
    );
    assert!(
        rig.calls().iter().all(|call| call["args"][0] != "resume"),
        "{:?}",
        verbs_named(&rig)
    );
    let mut rig = Rig::new();
    rig_for(&mut rig);
    let mut request = launch();
    request.session = Some("chrismas dates".into());
    assert_eq!(
        bare(rig.cli().launch(&request)),
        not_sent("session_not_found")
    );
    assert!(
        rig.calls()
            .iter()
            .all(|call| call["args"][0] != "resume" && call["args"][0] != "reply"),
        "{:?}",
        verbs_named(&rig)
    );
}

/// #6274: the lamp keeps each session it starts (`new`, its id under
/// `agent.id`) and each it types into or opens, with the time it was
/// used; the name a later list shows (the agent's own title) replaces the
/// lamp-made one, never with anything of its work.
#[test]
fn the_lamp_keeps_the_sessions_it_starts_and_their_names_as_listed() {
    let mut rig = Rig::new();
    let file = indexed(&mut rig);
    rig.new_step(step(created()));
    assert!(matches!(
        rig.cli().launch(&launch()),
        LaunchOutcome::Created { .. }
    ));
    let kept = session_index::read(&file);
    assert_eq!(kept[MACHINE].len(), 1);
    assert_eq!(kept[MACHINE][0].id, AGENT);
    // A lamp-made name says nothing a person would say: none kept.
    assert_eq!(kept[MACHINE][0].name, "");
    assert!(kept[MACHINE][0].last_used > 0);
    // Its title, once a list shows it.
    rig.config.reply_named = true;
    let mut row = ps_row(AGENT, "Christmas dates", "idle", None);
    row["asks"] = json!(["Find the Christmas dates for the school"]);
    rig.plan(json!({
        "machines":{"default":true},
        "ps":step(json!({"you":null,"machine":"lamp","harnesses":[row],"next":[]})),
    }));
    rig.cli()
        .look_session(&launch().target, "anything else", false)
        .unwrap_err();
    let kept = session_index::read(&file);
    assert_eq!(kept[MACHINE][0].name, "Christmas dates");
    let text = fs::read_to_string(&file).unwrap();
    assert!(!text.contains("school"), "{text}");
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

/// #6274: the index is a file of the tasks folder (`--dir`), which the
/// lamp's reset wipes whole: the product's unit names
/// `/var/lib/lampos-tasks/store` as `--dir`, and the product's reset plan
/// lists that folder.
#[test]
fn the_index_lives_in_the_tasks_folder_the_reset_wipes() {
    let dir = PathBuf::from("/var/lib/lampos-tasks/store");
    let mut cli = CliConfig::new(fixture());
    HarnessSwitches {
        reply_named: true,
        ..Default::default()
    }
    .apply(&mut cli, &dir, false);
    assert_eq!(cli.session_index, Some(dir.join(SESSION_INDEX_FILE)));
    let unit = include_str!("../../../deploy/product/lamp-tasks.service");
    assert!(
        unit.contains("--reply-named") && unit.contains(" --dir /var/lib/lampos-tasks/store "),
        "the product's unit"
    );
    let plan: Value =
        serde_json::from_str(include_str!("../../setup/fixtures/reset-plan-product.json")).unwrap();
    assert!(
        plan["wipe"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "/var/lib/lampos-tasks/store"),
        "the reset plan wipes the tasks folder"
    );
}
