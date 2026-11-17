//! The stand-in Harness's openharness#4393 program-session contract v1
//! (lampOS #7440): `capabilities`, `ensure`, the acknowledged `reply`,
//! `wait --after` and `close`, run as the fixture binary itself, as the
//! Harness builder answered the contract. No installed Harness, daemon,
//! account or agent is invoked.
use lamp_tasks::harness::parse_wait;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::Barrier,
};

const TOKEN: &[u8] = b"0123456789abcdef0123456789abcdef";

struct Rig {
    home: tempfile::TempDir,
}

impl Rig {
    /// A HOME with `plan` (each verb a `{"program": true, …}` step) and a
    /// good token file, `token`.
    fn new(plan: Value) -> Self {
        let home = tempfile::Builder::new()
            .prefix("tasks-program-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let rig = Self { home };
        rig.plan(plan);
        rig.token("token", TOKEN, 0o600);
        rig
    }
    fn program() -> Self {
        Self::new(json!({
            "capabilities":{"program":true},"ensure":{"program":true},"reply":{"program":true},
            "wait":{"program":true},"close":{"program":true},
        }))
    }
    fn plan(&self, plan: Value) {
        private_write(
            &self.home.path().join("plan.json"),
            plan.to_string().as_bytes(),
        );
    }
    fn token(&self, name: &str, bytes: &[u8], mode: u32) -> String {
        let path = self.home.path().join(name);
        private_write(&path, bytes);
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path.to_str().unwrap().to_owned()
    }
    fn path(&self, name: &str) -> String {
        self.home.path().join(name).to_str().unwrap().to_owned()
    }
    /// The fixture run with `args`: its exit and its stdout as JSON.
    fn run(&self, args: &[&str]) -> (i32, Value) {
        let output = Command::new(fixture())
            .args(args)
            .env_clear()
            .env("HOME", self.home.path())
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        let said = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "fixture stdout not JSON: {:?} / {:?}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.code().unwrap(), said)
    }
    /// `ensure openclaw --name lamp` with the good token, and `extra`.
    fn ensure(&self, extra: &[&str]) -> (i32, Value) {
        self.ensure_as("openclaw", "lamp", "token", extra)
    }
    fn ensure_as(&self, engine: &str, name: &str, token: &str, extra: &[&str]) -> (i32, Value) {
        let token = self.path(token);
        let mut args = vec![
            "ensure",
            engine,
            "--name",
            name,
            "--program-token-file",
            &token,
        ];
        args.extend_from_slice(extra);
        args.push("--json");
        self.run(&args)
    }
    fn reply(&self, text: &str) -> (i32, Value) {
        self.reply_with("token", text)
    }
    fn reply_with(&self, token: &str, text: &str) -> (i32, Value) {
        let token = self.path(token);
        self.run(&[
            "reply",
            "lamp",
            "--program-token-file",
            &token,
            "--json",
            "--",
            text,
        ])
    }
    fn taken(&self, text: &str) -> String {
        let (exit, said) = self.reply(text);
        assert_eq!((exit, &said["state"]), (0, &json!("taken")), "{said}");
        said["inputId"].as_str().unwrap().to_owned()
    }
    fn wait(&self, after: &str) -> (i32, Value) {
        self.run(&["wait", "lamp", "--after", after, "--json"])
    }
    fn close(&self, extra: &[&str]) -> (i32, Value) {
        self.close_with("token", extra)
    }
    fn close_with(&self, token: &str, extra: &[&str]) -> (i32, Value) {
        let token = self.path(token);
        let mut args = vec!["close", "lamp", "--program-token-file", &token];
        args.extend_from_slice(extra);
        args.push("--json");
        self.run(&args)
    }
    fn sessions(&self) -> Value {
        serde_json::from_slice(&fs::read(self.home.path().join("program-sessions.json")).unwrap())
            .unwrap()
    }
}

fn private_write(path: &Path, bytes: &[u8]) {
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

/// Cargo's fixture binary with group and other write cleared, as
/// `harness.rs` keeps it.
fn fixture() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_lamp-tasks-harness-fixture"));
    let mode = fs::metadata(&path).unwrap().permissions().mode();
    if mode & 0o022 != 0 {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode & !0o022)).unwrap();
    }
    path
}

/// A refusal's code. It says exactly `{"error","detail"}`, and beside
/// them only a reply's `TIMEOUT` fields.
fn refusal(said: &Value) -> &str {
    let keys = said.as_object().expect("an object");
    assert!(
        keys.keys()
            .all(|key| ["error", "detail", "cancelled", "inputId"].contains(&key.as_str())),
        "{said}"
    );
    assert!(
        said["detail"]
            .as_str()
            .is_some_and(|detail| !detail.is_empty()),
        "{said}"
    );
    said["error"].as_str().expect("an error code")
}

/// What the adapter's own `parse_wait` reads of a `wait` reply.
fn parsed(exit: i32, said: &Value, id: &Value) -> Result<Vec<(String, String)>, &'static str> {
    parse_wait(
        Some(exit),
        said.to_string().as_bytes(),
        &[id.as_str().unwrap().to_owned()],
    )
    .map(|ended| {
        ended
            .into_iter()
            .map(|ended| (ended.agent_id, ended.state))
            .collect()
    })
}

#[test]
fn capabilities_says_program_sessions_one() {
    let rig = Rig::program();
    assert_eq!(
        rig.run(&["capabilities", "--json"]),
        (0, json!({"programSessions":1}))
    );
}

/// A step without `program` is answered as before: the plan's own stdout.
#[test]
fn capabilities_without_program_step_is_as_planned() {
    let rig = Rig::new(json!({"capabilities":{"stdout":"{\"other\":2}\n","exit":1}}));
    assert_eq!(
        rig.run(&["capabilities", "--json"]),
        (1, json!({"other":2}))
    );
}

#[test]
fn ensure_new_then_live_then_resumed() {
    let rig = Rig::program();
    let (exit, first) = rig.ensure(&[]);
    assert_eq!(exit, 0, "{first}");
    assert_eq!(
        (
            &first["result"],
            &first["name"],
            &first["engine"],
            &first["state"]
        ),
        (
            &json!("new"),
            &json!("lamp"),
            &json!("openclaw"),
            &json!("idle")
        )
    );
    // Said only when it started the daemon.
    assert_eq!(
        (&first["daemonStarted"], &first["daemonStartMs"]),
        (&Value::Null, &Value::Null)
    );
    let id = first["id"].as_str().unwrap().to_owned();
    let conversation = first["engineSession"].as_str().unwrap().to_owned();
    assert!(!id.is_empty() && !conversation.is_empty(), "{first}");
    // Only the token's hash is kept, never the token.
    let kept = rig.sessions().to_string();
    assert!(
        !kept.contains(std::str::from_utf8(TOKEN).unwrap()),
        "{kept}"
    );

    let (exit, live) = rig.ensure(&[]);
    assert_eq!(
        (exit, &live["result"], &live["id"]),
        (0, &json!("live"), &json!(id))
    );
    assert_eq!(live["engineSession"], conversation.as_str());

    assert_eq!(rig.close(&[]).0, 0);
    let (exit, resumed) = rig.ensure(&[]);
    assert_eq!(exit, 0, "{resumed}");
    assert_eq!(resumed["result"], "resumed");
    assert_eq!(resumed["state"], "idle");
    assert_eq!(
        (resumed["id"].as_str(), resumed["engineSession"].as_str()),
        (Some(id.as_str()), Some(conversation.as_str()))
    );
}

/// `--fresh` on a live session closes it first, then gives a new id and a
/// new conversation; the old one's input is still followed, as stopped.
#[test]
fn fresh_on_a_live_session_gives_a_new_id() {
    let rig = Rig::program();
    let old = rig.ensure(&[]).1;
    let input = rig.taken("task");
    let (exit, fresh) = rig.ensure(&["--fresh"]);
    assert_eq!((exit, &fresh["result"]), (0, &json!("new")), "{fresh}");
    assert_ne!(fresh["id"], old["id"]);
    assert_ne!(fresh["engineSession"], old["engineSession"]);
    assert_eq!(fresh["state"], "idle");
    let closed = &rig.sessions()["closed"][0];
    assert_eq!(
        (&closed["id"], &closed["state"]),
        (&old["id"], &json!("stopped"))
    );
    let (exit, said) = rig.wait(&input);
    assert_eq!(said["harnesses"][0]["id"], old["id"]);
    assert_eq!(
        parsed(exit, &said, &old["id"]),
        Ok(vec![(
            old["id"].as_str().unwrap().to_owned(),
            "stopped".to_owned()
        )])
    );
    // The new one is idle and takes words.
    rig.taken("again");
}

/// The engine is the first word, wherever the options stand.
#[test]
fn ensure_reads_the_engine_as_the_first_word() {
    let rig = Rig::program();
    let token = rig.path("token");
    let (exit, said) = rig.run(&[
        "ensure",
        "--name",
        "lamp",
        "--program-token-file",
        &token,
        "openclaw",
        "--json",
    ]);
    assert_eq!(
        (exit, &said["engine"], &said["name"]),
        (0, &json!("openclaw"), &json!("lamp")),
        "{said}"
    );
    let (exit, said) = rig.run(&["ensure", "--name", "lamp", "--program-token-file", &token]);
    assert_eq!((exit, refusal(&said)), (2, "USAGE"));
}

/// A closed session the engine cannot continue starts new, with `why`.
#[test]
fn ensure_says_why_when_engine_cannot_continue() {
    let rig = Rig::program();
    let first = rig.ensure(&[]).1;
    rig.close(&[]);
    rig.plan(json!({"ensure":{"program":true,"why":"session file missing"}}));
    let (exit, again) = rig.ensure(&[]);
    assert_eq!(exit, 0, "{again}");
    assert_eq!(
        (&again["result"], &again["why"]),
        (&json!("new"), &json!("session file missing"))
    );
    assert_ne!(again["engineSession"], first["engineSession"]);
}

#[test]
fn ensure_reports_daemon_started() {
    let rig = Rig::new(json!({"ensure":{"program":true,"daemon_start_ms":850}}));
    let (exit, said) = rig.ensure(&[]);
    assert_eq!(exit, 0, "{said}");
    assert_eq!(
        (&said["daemonStarted"], &said["daemonStartMs"]),
        (&json!(true), &json!(850))
    );
}

#[test]
fn ensure_refuses_a_name_another_token_holds() {
    let rig = Rig::program();
    rig.ensure(&[]);
    rig.token("other", &[b'z'; 40], 0o600);
    let (exit, said) = rig.ensure_as("openclaw", "lamp", "other", &[]);
    assert_eq!((exit, refusal(&said)), (5, "NAME_TAKEN"));
}

/// Another engine for a held name, live or closed: `NAME_TAKEN`, and the
/// held one stays as it was.
#[test]
fn ensure_another_engine_on_a_held_name_is_name_taken() {
    let rig = Rig::program();
    let first = rig.ensure(&[]).1;
    let (exit, said) = rig.ensure_as("hermes", "lamp", "token", &[]);
    assert_eq!((exit, refusal(&said)), (5, "NAME_TAKEN"));
    rig.close(&[]);
    let (exit, said) = rig.ensure_as("hermes", "lamp", "token", &["--fresh"]);
    assert_eq!((exit, refusal(&said)), (5, "NAME_TAKEN"));
    let held = &rig.sessions()["sessions"]["lamp"];
    assert_eq!(
        (&held["id"], &held["engine"]),
        (&first["id"], &json!("openclaw"))
    );
}

/// `ensure` never refuses `ENGINE_NOT_READY`: it says `starting`. A reply
/// before the engine is ready is refused and types nothing, so the first
/// input is not lost once it is.
#[test]
fn ensure_says_starting_and_reply_waits_for_the_engine() {
    let rig = Rig::new(json!({
        "ensure":{"program":true,"starting":true},"reply":{"program":true},
    }));
    let (exit, said) = rig.ensure(&[]);
    assert_eq!((exit, &said["state"]), (0, &json!("starting")), "{said}");
    let (exit, said) = rig.reply("task");
    assert_eq!((exit, refusal(&said)), (5, "ENGINE_NOT_READY"));
    let session = &rig.sessions()["sessions"]["lamp"];
    assert_eq!(
        (&session["state"], &session["inputs"]),
        (&json!("starting"), &json!([]))
    );
    rig.plan(json!({"reply":{"program":true,"ready":true}}));
    rig.taken("task");
}

/// Eight `ensure` calls released at once with one name and token give one
/// harness: the sessions file is read and written under one lock.
#[test]
fn concurrent_ensure_gives_one_harness() {
    const CALLERS: usize = 8;
    let rig = Rig::program();
    let start = Barrier::new(CALLERS);
    let results: Vec<(i32, Value)> = std::thread::scope(|scope| {
        let calls: Vec<_> = (0..CALLERS)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    rig.ensure(&[])
                })
            })
            .collect();
        calls.into_iter().map(|call| call.join().unwrap()).collect()
    });
    let ids: std::collections::BTreeSet<_> = results
        .iter()
        .map(|(_, said)| said["id"].to_string())
        .collect();
    assert_eq!(ids.len(), 1, "{results:?}");
    assert!(results.iter().all(|(exit, _)| *exit == 0), "{results:?}");
    let new = results
        .iter()
        .filter(|(_, said)| said["result"] == "new")
        .count();
    assert_eq!(new, 1, "{results:?}");
}

#[test]
fn reply_is_taken_then_not_idle_until_wait_says_idle() {
    let rig = Rig::program();
    let id = rig.ensure(&[]).1["id"].clone();
    let (exit, taken) = rig.reply("first task");
    assert_eq!(exit, 0, "{taken}");
    let first = taken["inputId"].as_str().unwrap().to_owned();
    assert_eq!(taken, json!({"state":"taken","inputId":first}));

    let (exit, busy) = rig.reply("second task");
    assert_eq!((exit, refusal(&busy)), (5, "NOT_IDLE"));

    let (exit, ended) = rig.wait(&first);
    assert_eq!(
        parsed(exit, &ended, &id),
        Ok(vec![(id.as_str().unwrap().to_owned(), "idle".to_owned())])
    );
    assert_eq!(ended["harnesses"][0]["name"], "lamp");

    let again = rig.taken("second task");
    assert_ne!(again, first);
    let inputs = &rig.sessions()["sessions"]["lamp"]["inputs"];
    assert_eq!(inputs[0]["text"], "first task");
    assert_eq!(inputs[1]["text"], "second task");
    assert_eq!(inputs.as_array().unwrap().len(), 2, "{inputs}");
}

/// While the session waits for the person, a reply is never taken.
#[test]
fn reply_while_the_person_is_asked_is_not_idle() {
    let rig = Rig::program();
    rig.ensure(&[]);
    let input = rig.taken("task");
    rig.plan(json!({"wait":{"program":true,"ends":"person"},"reply":{"program":true}}));
    assert_eq!(rig.wait(&input).0, 3);
    let (exit, said) = rig.reply("more");
    assert_eq!((exit, refusal(&said)), (5, "NOT_IDLE"));
    assert!(
        said["detail"]
            .as_str()
            .unwrap()
            .contains("waits for the person"),
        "{said}"
    );
    let inputs = &rig.sessions()["sessions"]["lamp"]["inputs"];
    assert_eq!(inputs.as_array().unwrap().len(), 1, "{inputs}");
}

#[test]
fn reply_unknown_harness_and_not_running_when_closed() {
    let rig = Rig::program();
    let (exit, said) = rig.reply("task");
    assert_eq!((exit, refusal(&said)), (1, "UNKNOWN_HARNESS"));
    rig.ensure(&[]);
    rig.close(&[]);
    let (exit, said) = rig.reply("task");
    assert_eq!((exit, refusal(&said)), (4, "NOT_RUNNING"));
}

/// Not seen to start in time, nothing typed: `cancelled: true`, no input,
/// safe to send again.
#[test]
fn reply_timeout_cancelled_types_nothing() {
    let rig = Rig::program();
    rig.ensure(&[]);
    rig.plan(json!({"reply":{"program":true,"timeout":"cancelled"}}));
    let (exit, said) = rig.reply("task");
    assert_eq!((exit, refusal(&said)), (124, "TIMEOUT"));
    assert_eq!(
        (&said["cancelled"], &said["inputId"]),
        (&json!(true), &Value::Null)
    );
    let session = &rig.sessions()["sessions"]["lamp"];
    assert_eq!(
        (&session["state"], &session["inputs"]),
        (&json!("idle"), &json!([]))
    );
}

/// Typed and not seen to start: `cancelled: false` with the input's id,
/// which `wait --after` follows.
#[test]
fn reply_timeout_typed_gives_the_input_to_follow() {
    let rig = Rig::program();
    let id = rig.ensure(&[]).1["id"].clone();
    rig.plan(json!({"reply":{"program":true,"timeout":"typed"},"wait":{"program":true}}));
    let (exit, said) = rig.reply("task");
    assert_eq!((exit, refusal(&said)), (124, "TIMEOUT"));
    assert_eq!(said["cancelled"], false);
    let input = said["inputId"].as_str().expect("an inputId").to_owned();
    let (exit, said) = rig.wait(&input);
    assert_eq!(
        parsed(exit, &said, &id),
        Ok(vec![(id.as_str().unwrap().to_owned(), "idle".to_owned())])
    );
}

#[test]
fn reply_and_close_with_another_token_are_not_yours() {
    let rig = Rig::program();
    rig.ensure(&[]);
    rig.token("other", &[b'z'; 40], 0o600);
    let (exit, said) = rig.reply_with("other", "task");
    assert_eq!((exit, refusal(&said)), (5, "NOT_YOURS"));
    let (exit, said) = rig.close_with("other", &[]);
    assert_eq!((exit, refusal(&said)), (5, "NOT_YOURS"));
    let session = &rig.sessions()["sessions"]["lamp"];
    assert_eq!(
        (&session["state"], &session["inputs"]),
        (&json!("idle"), &json!([]))
    );
}

#[test]
fn wait_ends_for_the_person_stopped_and_timeout() {
    for (ends, exit, state, read) in [
        ("person", 3, "needs-input", Some("needs-input")),
        ("stopped", 4, "stopped", Some("stopped")),
        ("timeout", 124, "timeout", None),
    ] {
        let rig = Rig::program();
        let id = rig.ensure(&[]).1["id"].clone();
        let input = rig.taken("task");
        rig.plan(json!({"wait":{"program":true,"ends":ends}}));
        let (code, said) = rig.wait(&input);
        assert_eq!(
            (code, &said["harnesses"][0]["state"]),
            (exit, &json!(state)),
            "{ends}: {said}"
        );
        let expected = read
            .map(|read| vec![(id.as_str().unwrap().to_owned(), read.to_owned())])
            .unwrap_or_default();
        assert_eq!(parsed(code, &said, &id), Ok(expected), "{ends}");
    }
}

/// A closed session's input ends `stopped` with its id, as `parse_wait`
/// reads it.
#[test]
fn wait_on_a_closed_session_is_stopped() {
    let rig = Rig::program();
    let id = rig.ensure(&[]).1["id"].clone();
    let input = rig.taken("task");
    rig.close(&[]);
    let (exit, said) = rig.wait(&input);
    assert_eq!(
        said,
        json!({"harnesses":[{"id":id,"name":"lamp","state":"stopped"}]})
    );
    assert_eq!(
        parsed(exit, &said, &id),
        Ok(vec![(
            id.as_str().unwrap().to_owned(),
            "stopped".to_owned()
        )])
    );
}

/// An input no harness here took, or one under another name, or any input
/// for a name never started: `UNKNOWN_INPUT`, exit 1, never a made-up row.
#[test]
fn wait_after_an_unknown_input_is_unknown_input() {
    let rig = Rig::program();
    let (exit, said) = rig.wait("in-1");
    assert_eq!((exit, refusal(&said)), (1, "UNKNOWN_INPUT"));
    assert_eq!(said["detail"], "No harness here took an input in-1.");
    rig.ensure(&[]);
    rig.ensure_as("openclaw", "desk", "token", &[]);
    let input = rig.taken("task");
    let (exit, said) = rig.wait("in-99");
    assert_eq!((exit, refusal(&said)), (1, "UNKNOWN_INPUT"));
    let (exit, said) = rig.run(&["wait", "desk", "--after", &input, "--json"]);
    assert_eq!((exit, refusal(&said)), (1, "UNKNOWN_INPUT"));
    assert_eq!(
        said["detail"],
        format!("No harness here took an input {input}.")
    );
}

#[test]
fn wait_with_another_token_is_not_yours() {
    let rig = Rig::program();
    rig.ensure(&[]);
    let input = rig.taken("task");
    let other = rig.token("other", &[b'z'; 40], 0o600);
    let (exit, said) = rig.run(&[
        "wait",
        "lamp",
        "--after",
        &input,
        "--program-token-file",
        &other,
        "--json",
    ]);
    assert_eq!((exit, refusal(&said)), (5, "NOT_YOURS"));
}

/// `close --json` says what it always says; `--forget` is not built, and
/// changes nothing it says.
#[test]
fn close_prints_the_close_reply() {
    let rig = Rig::program();
    let id = rig.ensure(&[]).1["id"].clone();
    let expected = json!({
        "name":"lamp","id":id,"closed":true,
        "effect":"Its conversation is kept; the person can resume it.","next":[],
    });
    assert_eq!(rig.close(&[]), (0, expected.clone()));
    assert_eq!(rig.sessions()["sessions"]["lamp"]["state"], "stopped");
    assert_eq!(rig.close(&["--forget"]), (0, expected));
}

#[test]
fn close_of_an_unknown_name_is_unknown_harness() {
    let rig = Rig::program();
    let (exit, said) = rig.close(&[]);
    assert_eq!((exit, refusal(&said)), (1, "UNKNOWN_HARNESS"));
}

/// Not 0600 (0644, 0640, 0400), under 32 bytes once whitespace is taken
/// off, a link to a good file, a folder, or missing: `BAD_TOKEN_FILE`
/// (exit 5) from every verb that takes a token, and nothing kept. Exactly
/// 32 bytes is enough.
#[test]
fn bad_token_file_is_refused() {
    let rig = Rig::program();
    let link = rig.path("link");
    std::os::unix::fs::symlink(rig.path("token"), &link).unwrap();
    fs::create_dir(rig.path("folder")).unwrap();
    let mut padded = TOKEN[..31].to_vec();
    padded.extend_from_slice(b"\n\n");
    let bad = [
        rig.token("open", TOKEN, 0o644),
        rig.token("group", TOKEN, 0o640),
        rig.token("read-only", TOKEN, 0o400),
        rig.token("short", &TOKEN[..31], 0o600),
        rig.token("padded", &padded, 0o600),
        link,
        rig.path("folder"),
        rig.path("missing"),
    ];
    for token in &bad {
        for args in [
            vec![
                "ensure",
                "openclaw",
                "--name",
                "lamp",
                "--program-token-file",
                token,
                "--json",
            ],
            vec![
                "reply",
                "lamp",
                "--program-token-file",
                token,
                "--json",
                "--",
                "task",
            ],
            vec!["close", "lamp", "--program-token-file", token, "--json"],
        ] {
            let (exit, said) = rig.run(&args);
            assert_eq!((exit, refusal(&said)), (5, "BAD_TOKEN_FILE"), "{args:?}");
        }
    }
    assert_eq!(rig.sessions()["sessions"], json!({}));
    assert_eq!(rig.ensure(&[]).0, 0);
}

/// A link is refused even to the program's own good token, which works
/// named directly.
#[test]
fn a_symlinked_token_file_is_refused() {
    let rig = Rig::program();
    let link = rig.path("link");
    std::os::unix::fs::symlink(rig.path("token"), &link).unwrap();
    let (exit, said) = rig.run(&[
        "ensure",
        "openclaw",
        "--name",
        "lamp",
        "--program-token-file",
        &link,
        "--json",
    ]);
    assert_eq!((exit, refusal(&said)), (5, "BAD_TOKEN_FILE"));
    assert_eq!(rig.ensure(&[]).0, 0);
}

/// A step's `exits` sets a refusal's exit, for a contract still settling.
#[test]
fn a_step_sets_a_refusal_exit() {
    let rig = Rig::new(json!({"reply":{"program":true,"exits":{"UNKNOWN_HARNESS":9}}}));
    assert_eq!(rig.reply("task").0, 9);
}
