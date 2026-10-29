//! Scripted local executable fixtures. No installed Harness, daemon, account,
//! remote agent, credentials or provider is invoked by these tests.
use lamp_tasks::harness::{
    AccountMachine, Cli, CliConfig, Closed, Ended, EngineKind, Launch, LaunchOutcome, Listed,
    MOST_ABOUT_BYTES, MOST_LISTED, MOST_WAITED, PS_STDOUT_LIMIT, SignIn, Target, iso_unix_ms,
    parse_auth_status, parse_machines, parse_ps, parse_wait,
};
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
            journal: None,
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
    let sent = json!({"decided":"session","id":AGENT,"name":"Fix login","machine":"daniel-pc","confidence":0.82,"reason":"same work","sent":{"state":"queued"}});
    let (outcome, called, rig) = routed(step(sent));
    assert_eq!(
        outcome,
        LaunchOutcome::Joined {
            agent_id: AGENT.into()
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
                current: false
            },
            AccountMachine {
                machine_id: other.into(),
                label: "lamp".into(),
                current: true
            },
            AccountMachine {
                machine_id: "0123456789abcdef0123456789abcdee".into(),
                label: "".into(),
                current: false
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
