//! A skill's `do.task` handed to the lamp's own Hermes (#6306, #6071's
//! PR 3). lamp-skills sends one line for each run (#6305's
//! `lamp_skills::task::HandOver::line`):
//! `{"op":"skill_task","skill","task","run","folder","sha256","grants"}`,
//! and is answered once the run ends: `{"ok":true,"id":N}`, or a refusal
//! `{"ok":false,"error","message"}` ([`crate::protocol::refusal`]).
//!
//! - **Off unless [`SWITCH`] is `on`** (the same switch lamp-skills reads):
//!   off, every line is refused `off` and nothing is minted or run.
//! - **The line is checked whole** ([`HandOver::parse`]): a field more or
//!   less, a skill folder outside the lamp's skills, a `run` outside its
//!   `scripts/`, a grant written twice or `skills` is refused.
//! - **A one-run token** ([`Tokens`]): minted by lamp-control (#6304,
//!   PR #6408) on its admin socket ([`ControlTokens`]) for this skill and a
//!   run id of lamp-tasks' own ([`run_id`]). lamp-control reads the owner's
//!   yes itself and names its grants; a run whose minted grants are not
//!   exactly the ones its line asks is refused (`no_token`) and its token
//!   ended. The token is given to the run as [`TOKEN_ENV`] beside
//!   [`SOCKET_ENV`], and ended when the run ends, however it ends.
//!   [`NoTokens`] refuses every run: nothing runs without a token.
//! - **The run** ([`Runner`]): one task for target `lamp` (the lamp's own
//!   Hermes), its words the skill's task and its script's place
//!   ([`SkillRun::prompt`]). The runner blocks until the run ends.
use crate::protocol::refusal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The switch, `on` exactly; off unless set. lamp-skills reads the same one
/// (#6305): a skill's tasks are on or off as one.
pub const SWITCH: &str = "LAMP_SKILL_TASKS";
/// The lamp's skills, shown read-only inside Hermes's namespace
/// (`lamp_update::hermes_install::SKILLS`): a skill's folder is one of its
/// folders, never another place.
pub const SKILLS: &str = "/var/lib/lampOS/skills";
/// lamp-control's local door (#6304, the first builder's 12:45 conditions
/// on #6071): the one way a run reaches the lamp's hardware.
pub const CONTROL_SOCKET: &str = "/run/lampos-control/control.sock";
/// The variable a run is told [`CONTROL_SOCKET`] in.
pub const SOCKET_ENV: &str = "LAMP_CONTROL_SOCKET";
/// The variable a run is given its one-run token in.
pub const TOKEN_ENV: &str = "LAMP_CONTROL_TOKEN";
/// lamp-control's admin socket (lampos's, 0600 in a 0700 folder), where
/// lamp-tasks mints and ends a run's token (#6408, docs/control.md, "The
/// control socket, lamp-control's side").
pub const ADMIN_SOCKET: &str = "/run/lampos-control/admin.sock";
/// How long one ask on [`ADMIN_SOCKET`] may take, each way.
pub const ADMIN_WAIT: Duration = Duration::from_secs(5);
/// The folder a skill's script is in (#6305, `lamp_skills::task::SCRIPTS`).
pub const SCRIPTS: &str = "scripts";
/// The most bytes of a task's words (#6305, `MOST_TASK_BYTES`).
pub const MOST_TASK_BYTES: usize = 500;
/// lamp-control's grants (protocol.rs, 13 of them): never more on one run.
pub const MOST_GRANTS: usize = 13;

/// Whether [`SWITCH`]'s `value` is on: `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// One `do.task` handed over by lamp-skills, as its line says it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HandOver {
    pub op: Op,
    pub skill: String,
    pub task: String,
    pub run: String,
    pub folder: PathBuf,
    pub sha256: String,
    pub grants: Vec<String>,
}

/// The line's `op`: `skill_task` only.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    SkillTask,
}

impl HandOver {
    /// The line, checked whole against `skills` (on a lamp, [`SKILLS`]):
    /// the refusal's code and words otherwise.
    pub fn parse(line: &[u8], skills: &Path) -> Result<Self, (&'static str, String)> {
        if line.len() > crate::protocol::MAX_LINE_BYTES {
            return Err(("too_long", "the line is too long".into()));
        }
        let hand: Self = serde_json::from_slice(line)
            .map_err(|_| ("bad_request", "not a skill's task line".to_owned()))?;
        hand.check(skills)
            .map_err(|why| ("bad_request", why.to_owned()))?;
        Ok(hand)
    }

    fn check(&self, skills: &Path) -> Result<(), &'static str> {
        let name_ok = |name: &str| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        };
        if !name_ok(&self.skill) {
            return Err("the skill's name is not a skill's name");
        }
        if self.task.trim().is_empty() || self.task.len() > MOST_TASK_BYTES {
            return Err("the task's words are empty or too long");
        }
        if self.folder != skills.join(&self.skill) {
            return Err("the skill's folder is not its own among the lamp's skills");
        }
        let run = Path::new(&self.run);
        let mut parts = run.components();
        if parts.next() != Some(Component::Normal(SCRIPTS.as_ref())) {
            return Err("the script is not in the skill's scripts/");
        }
        let mut rest = 0;
        for part in parts {
            match part {
                Component::Normal(part) if !part.to_string_lossy().starts_with('.') => rest += 1,
                _ => return Err("the script's path has a part that leaves scripts/"),
            }
        }
        if rest == 0 {
            return Err("the script is not a file in scripts/");
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("the folder's SHA-256 is not one");
        }
        if self.grants.len() > MOST_GRANTS {
            return Err("more grants than lamp-control has");
        }
        let mut seen = std::collections::BTreeSet::new();
        for grant in &self.grants {
            if grant == "skills" || !name_ok(grant) || !seen.insert(grant) {
                return Err("a grant is unknown, written twice or `skills`");
            }
        }
        Ok(())
    }
}

/// A one-run token from lamp-control (#6304). Its text is never shown in a
/// debug line or the journal.
pub struct RunToken(String);

impl RunToken {
    pub fn new(secret: String) -> Self {
        Self(secret)
    }

    pub fn secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for RunToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunToken(..)")
    }
}

/// A token lamp-control minted, and the grants it names: the owner's yes
/// for the skill, read by lamp-control, never what the asker said.
#[derive(Debug)]
pub struct Minted {
    pub token: RunToken,
    pub grants: Vec<String>,
}

/// lamp-control's one-run tokens (#6304): minted from the owner's yes for
/// `skill`, one for run id `run`, ended with the run.
pub trait Tokens {
    fn mint(&self, skill: &str, run: &str) -> Result<Minted, String>;
    /// The run ended: its token is refused from now on.
    fn end(&self, run: &str);
}

/// No lamp-control token: every run is refused.
pub struct NoTokens;

impl Tokens for NoTokens {
    fn mint(&self, _: &str, _: &str) -> Result<Minted, String> {
        Err("no one-run token on this lamp yet (lamp-control's control.sock, #6304)".into())
    }

    fn end(&self, _: &str) {}
}

/// lamp-control's admin socket: one JSON line asked, one answered, a
/// connection each (#6408): `{"mint": {"skill", "run"}}` answered
/// `{"token", "run", "skill", "grants", "expires_in_ms"}` or `{"error"}`,
/// and `{"end": {"run"}}`.
pub struct ControlTokens {
    pub socket: PathBuf,
    pub wait: Duration,
}

impl Default for ControlTokens {
    fn default() -> Self {
        Self {
            socket: PathBuf::from(ADMIN_SOCKET),
            wait: ADMIN_WAIT,
        }
    }
}

impl ControlTokens {
    fn ask(&self, asked: &Value) -> Result<Value, String> {
        let unreachable = |error: std::io::Error| format!("lamp-control is not reachable: {error}");
        let mut stream = UnixStream::connect(&self.socket).map_err(unreachable)?;
        stream
            .set_read_timeout(Some(self.wait))
            .map_err(unreachable)?;
        stream
            .set_write_timeout(Some(self.wait))
            .map_err(unreachable)?;
        let mut line = asked.to_string();
        line.push('\n');
        stream.write_all(line.as_bytes()).map_err(unreachable)?;
        let mut answer = String::new();
        BufReader::new(std::io::Read::take(stream, MOST_ANSWER_BYTES))
            .read_line(&mut answer)
            .map_err(unreachable)?;
        serde_json::from_str(answer.trim()).map_err(|_| "lamp-control's answer is not JSON".into())
    }
}

/// At most this many bytes of an admin answer are read.
const MOST_ANSWER_BYTES: u64 = 64 * 1024;

impl Tokens for ControlTokens {
    fn mint(&self, skill: &str, run: &str) -> Result<Minted, String> {
        let answer = self.ask(&json!({"mint": {"skill": skill, "run": run}}))?;
        if let Some(why) = answer.get("error") {
            return Err(why
                .as_str()
                .unwrap_or("lamp-control refused the token")
                .into());
        }
        let token = answer["token"].as_str().filter(|token| !token.is_empty());
        let grants = answer["grants"].as_array().and_then(|grants| {
            grants
                .iter()
                .map(|grant| grant.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
        });
        match (token, grants, answer["run"].as_str() == Some(run)) {
            (Some(token), Some(grants), true) => Ok(Minted {
                token: RunToken::new(token.to_owned()),
                grants,
            }),
            _ => {
                self.end(run);
                Err("lamp-control's token answer is not one this lamp-tasks reads".into())
            }
        }
    }

    fn end(&self, run: &str) {
        if let Err(why) = self.ask(&json!({"end": {"run": run}})) {
            // The token still ends at its own time (10 minutes at most).
            eprintln!("[lamp-tasks] skill task: ending run {run}: {why}");
        }
    }
}

/// A run id of lamp-tasks' own, new for each hand-over: plain characters,
/// under lamp-control's 64. A second mint for the same id would replace the
/// first run's token, so two hand-overs never share one.
pub fn run_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    format!(
        "task-{millis}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// One run for the lamp's own Hermes: its words, the skill's folder, and
/// what its environment is given.
#[derive(Debug)]
pub struct SkillRun {
    pub skill: String,
    pub prompt: String,
    pub folder: PathBuf,
    /// [`SOCKET_ENV`] and [`TOKEN_ENV`], and nothing else.
    pub env: Vec<(&'static str, String)>,
}

impl SkillRun {
    fn of(hand: &HandOver, token: &RunToken) -> Self {
        Self {
            skill: hand.skill.clone(),
            prompt: Self::prompt(hand),
            folder: hand.folder.clone(),
            env: vec![
                (SOCKET_ENV, CONTROL_SOCKET.to_owned()),
                (TOKEN_ENV, token.secret().to_owned()),
            ],
        }
    }

    /// The task's words for Hermes: the skill's task, the script to run in
    /// its read-only folder, and the one way to the lamp. Never the token.
    pub fn prompt(hand: &HandOver) -> String {
        format!(
            "A task of the lamp's skill `{skill}`, as the owner said yes to it: {task}\n\
             Run `{run}` from the skill's folder `{folder}` (read-only; work in your own folder). \
             It reaches the lamp only through ${SOCKET_ENV} with the token in ${TOKEN_ENV}; \
             no other network.",
            skill = hand.skill,
            task = hand.task.trim(),
            run = hand.run,
            folder = hand.folder.display(),
        )
    }
}

/// Runs one task on the lamp's own Hermes (target `lamp`) and waits for its
/// end: the task's number, or why it didn't run or end well.
pub trait Runner {
    fn run(&self, run: &SkillRun) -> Result<u64, String>;
}

/// Ends the token however the run ends, a panic too.
struct Ending<'a> {
    tokens: &'a dyn Tokens,
    run: String,
}

impl Drop for Ending<'_> {
    fn drop(&mut self) {
        self.tokens.end(&self.run);
    }
}

/// lamp-tasks' answer to one hand-over `line`: with the switch `on`, the
/// line checked against `skills`, a token minted, the run made and waited
/// for, the token ended; else the refusal.
pub fn answer(
    line: &[u8],
    on: bool,
    skills: &Path,
    tokens: &dyn Tokens,
    runner: &dyn Runner,
) -> Value {
    if !on {
        return refusal(
            "off",
            "a skill's tasks are off on this lamp (LAMP_SKILL_TASKS)",
        );
    }
    let hand = match HandOver::parse(line, skills) {
        Ok(hand) => hand,
        Err((code, why)) => return refusal(code, &why),
    };
    let run_id = run_id();
    let minted = match tokens.mint(&hand.skill, &run_id) {
        Ok(minted) => minted,
        Err(why) => return refusal("no_token", &why),
    };
    let ending = Ending {
        tokens,
        run: run_id,
    };
    let asked: BTreeSet<&String> = hand.grants.iter().collect();
    if minted.grants.iter().collect::<BTreeSet<_>>() != asked {
        drop(ending);
        return refusal(
            "no_token",
            "the owner's yes for this skill names other grants than its task asks: \
             the owner says yes again in the app",
        );
    }
    let run = SkillRun::of(&hand, &minted.token);
    let ran = runner.run(&run);
    drop(run);
    drop(minted);
    drop(ending);
    match ran {
        Ok(id) => json!({"ok": true, "id": id}),
        Err(why) => refusal("run_failed", &why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn line(edit: impl FnOnce(&mut Value)) -> Vec<u8> {
        let mut value = json!({
            "op": "skill_task",
            "skill": "photo-booth",
            "task": "Take a photo and send it to the app.",
            "run": "scripts/main.py",
            "folder": "/var/lib/lampOS/skills/photo-booth",
            "sha256": SHA,
            "grants": ["camera", "light"],
        });
        edit(&mut value);
        value.to_string().into_bytes()
    }

    #[derive(Default)]
    struct FakeTokens {
        refuse: bool,
        /// The grants the owner's yes names; the line's own when `None`.
        yes: Option<Vec<String>>,
        /// (skill, run id) of each mint.
        minted: RefCell<Vec<(String, String)>>,
        /// (run id, token) of each live run.
        live: RefCell<Vec<(String, String)>>,
        ended: RefCell<Vec<String>>,
    }

    impl Tokens for FakeTokens {
        fn mint(&self, skill: &str, run: &str) -> Result<Minted, String> {
            if self.refuse {
                return Err("the owner's yes names no grant for this skill".into());
            }
            self.minted.borrow_mut().push((skill.into(), run.into()));
            let secret = format!("tok-{}", self.minted.borrow().len());
            self.live.borrow_mut().push((run.into(), secret.clone()));
            let grants = self
                .yes
                .clone()
                .unwrap_or_else(|| vec!["light".into(), "camera".into()]);
            Ok(Minted {
                token: RunToken::new(secret),
                grants,
            })
        }

        fn end(&self, run: &str) {
            self.live.borrow_mut().retain(|(live, _)| live != run);
            self.ended.borrow_mut().push(run.into());
        }
    }

    /// A run as the runner saw it: its words, its environment, and whether
    /// its token was live.
    type Seen = (String, Vec<(&'static str, String)>, bool);
    /// One change to a good line.
    type Edit = Box<dyn Fn(&mut Value)>;

    struct FakeRunner<'a> {
        tokens: &'a FakeTokens,
        result: Result<u64, String>,
        seen: RefCell<Vec<Seen>>,
    }

    impl Runner for FakeRunner<'_> {
        fn run(&self, run: &SkillRun) -> Result<u64, String> {
            let token = run
                .env
                .iter()
                .find(|(name, _)| *name == TOKEN_ENV)
                .map(|(_, value)| value.clone())
                .unwrap_or_default();
            let live = self
                .tokens
                .live
                .borrow()
                .iter()
                .any(|(_, live)| *live == token);
            self.seen
                .borrow_mut()
                .push((run.prompt.clone(), run.env.clone(), live));
            self.result.clone()
        }
    }

    fn runner(tokens: &FakeTokens, result: Result<u64, String>) -> FakeRunner<'_> {
        FakeRunner {
            tokens,
            result,
            seen: RefCell::default(),
        }
    }

    fn skills() -> &'static Path {
        Path::new(SKILLS)
    }

    #[test]
    fn the_switch_is_on_only_when_on_exactly() {
        assert!(on(Some("on")));
        for value in [None, Some("off"), Some("ON"), Some("1"), Some("")] {
            assert!(!on(value), "{value:?}");
        }
    }

    #[test]
    fn off_refuses_and_neither_mints_nor_runs() {
        let tokens = FakeTokens::default();
        let runner = runner(&tokens, Ok(7));
        let answer = answer(&line(|_| {}), false, skills(), &tokens, &runner);
        assert_eq!(answer["ok"], false);
        assert_eq!(answer["error"], "off");
        assert!(tokens.minted.borrow().is_empty() && runner.seen.borrow().is_empty());
    }

    #[test]
    fn a_run_gets_its_own_token_and_the_socket_and_the_token_ends_with_it() {
        let tokens = FakeTokens::default();
        let runner = runner(&tokens, Ok(42));
        let answer = answer(&line(|_| {}), true, skills(), &tokens, &runner);
        assert_eq!(answer, json!({"ok": true, "id": 42}));
        let minted = tokens.minted.borrow();
        assert_eq!(minted.len(), 1);
        let (skill, run_id) = &minted[0];
        assert_eq!(skill, "photo-booth");
        assert!(
            run_id.starts_with("task-") && run_id.len() <= 64,
            "{run_id}"
        );
        let seen = runner.seen.borrow();
        let (prompt, env, live) = &seen[0];
        assert!(*live, "the token is live while the run runs");
        assert_eq!(
            *env,
            vec![
                (SOCKET_ENV, CONTROL_SOCKET.to_owned()),
                (TOKEN_ENV, "tok-1".to_owned())
            ]
        );
        assert!(prompt.contains("Take a photo and send it to the app."));
        assert!(prompt.contains("scripts/main.py"));
        assert!(prompt.contains("/var/lib/lampOS/skills/photo-booth"));
        assert!(!prompt.contains("tok-1"), "the token is never in the words");
        assert!(tokens.live.borrow().is_empty());
        assert_eq!(*tokens.ended.borrow(), vec![run_id.clone()]);
    }

    #[test]
    fn a_failed_run_still_ends_its_token() {
        let tokens = FakeTokens::default();
        let runner = runner(&tokens, Err("hermes stopped".into()));
        let answer = answer(&line(|_| {}), true, skills(), &tokens, &runner);
        assert_eq!(answer["error"], "run_failed");
        assert_eq!(answer["message"], "hermes stopped");
        assert!(tokens.live.borrow().is_empty());
        assert_eq!(tokens.ended.borrow().len(), 1);
    }

    #[test]
    fn no_token_no_run() {
        let tokens = FakeTokens {
            refuse: true,
            ..FakeTokens::default()
        };
        let runner = runner(&tokens, Ok(1));
        let answer = answer(&line(|_| {}), true, skills(), &tokens, &runner);
        assert_eq!(answer["error"], "no_token");
        assert!(runner.seen.borrow().is_empty());
        let answer = super::answer(&line(|_| {}), true, skills(), &NoTokens, &runner);
        assert_eq!(answer["error"], "no_token");
        assert!(answer["message"].as_str().unwrap().contains("#6304"));
        assert!(runner.seen.borrow().is_empty());
    }

    #[test]
    fn a_line_out_of_its_bounds_is_refused_before_any_token() {
        let bad: Vec<Edit> = vec![
            Box::new(|v| v["op"] = json!("prepare")),
            Box::new(|v| v["extra"] = json!(1)),
            Box::new(|v| {
                v.as_object_mut().unwrap().remove("grants");
            }),
            Box::new(|v| v["skill"] = json!("../etc")),
            Box::new(|v| v["skill"] = json!("Photo")),
            Box::new(|v| v["task"] = json!("  ")),
            Box::new(|v| v["task"] = json!("x".repeat(MOST_TASK_BYTES + 1))),
            Box::new(|v| v["folder"] = json!("/var/lib/lampOS/skills/other")),
            Box::new(|v| v["folder"] = json!("/home/lampos/photo-booth")),
            Box::new(|v| v["run"] = json!("main.py")),
            Box::new(|v| v["run"] = json!("scripts")),
            Box::new(|v| v["run"] = json!("scripts/../SKILL.md")),
            Box::new(|v| v["run"] = json!("scripts/.hidden.py")),
            Box::new(|v| v["run"] = json!("/var/lib/lampOS/skills/photo-booth/scripts/main.py")),
            Box::new(|v| v["sha256"] = json!("abc")),
            Box::new(|v| v["sha256"] = json!(SHA.to_uppercase())),
            Box::new(|v| v["grants"] = json!(["camera", "camera"])),
            Box::new(|v| v["grants"] = json!(["skills"])),
            Box::new(|v| v["grants"] = json!(["Camera"])),
        ];
        for (index, edit) in bad.iter().enumerate() {
            let tokens = FakeTokens::default();
            let runner = runner(&tokens, Ok(1));
            let answer = answer(&line(edit), true, skills(), &tokens, &runner);
            assert_eq!(answer["error"], "bad_request", "case {index}: {answer}");
            assert!(tokens.minted.borrow().is_empty(), "case {index}");
        }
        let tokens = FakeTokens::default();
        let runner = runner(&tokens, Ok(1));
        let long = vec![b' '; crate::protocol::MAX_LINE_BYTES + 1];
        assert_eq!(
            answer(&long, true, skills(), &tokens, &runner)["error"],
            "too_long"
        );
    }

    #[test]
    fn a_yes_naming_other_grants_ends_the_token_and_runs_nothing() {
        for yes in [vec!["camera"], vec!["camera", "light", "speak"], vec![]] {
            let tokens = FakeTokens {
                yes: Some(yes.iter().map(|grant| grant.to_string()).collect()),
                ..FakeTokens::default()
            };
            let runner = runner(&tokens, Ok(1));
            let answer = answer(&line(|_| {}), true, skills(), &tokens, &runner);
            assert_eq!(answer["error"], "no_token", "{yes:?}");
            assert!(runner.seen.borrow().is_empty(), "{yes:?}");
            assert!(tokens.live.borrow().is_empty(), "{yes:?}");
            assert_eq!(tokens.ended.borrow().len(), 1, "{yes:?}");
        }
    }

    #[test]
    fn each_hand_over_gets_a_run_id_of_its_own() {
        let first = run_id();
        let second = run_id();
        assert_ne!(first, second);
        for id in [&first, &second] {
            assert!(id.len() <= 64, "{id}");
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')),
                "{id}"
            );
        }
    }

    /// A stand-in admin socket: answers each connection's one line with
    /// `reply(line)` and keeps the lines asked.
    fn admin(
        reply: impl Fn(&Value) -> Value + Send + 'static,
        connections: usize,
    ) -> (
        tempfile::TempDir,
        ControlTokens,
        std::thread::JoinHandle<Vec<Value>>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("admin.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let served = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for _ in 0..connections {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let value: Value = serde_json::from_str(&line).unwrap();
                let mut out = reply(&value).to_string();
                out.push('\n');
                (&stream).write_all(out.as_bytes()).unwrap();
                asked.push(value);
            }
            asked
        });
        let tokens = ControlTokens {
            socket,
            wait: Duration::from_secs(5),
        };
        (dir, tokens, served)
    }

    #[test]
    fn control_tokens_mint_and_end_on_lamp_controls_admin_socket() {
        let (_dir, tokens, served) = admin(
            |asked| match asked.get("mint") {
                Some(mint) => json!({
                    "token": "t".repeat(43),
                    "run": mint["run"],
                    "skill": mint["skill"],
                    "grants": ["camera", "light"],
                    "expires_in_ms": 600_000,
                }),
                None => json!({"ended": true}),
            },
            2,
        );
        let minted = tokens.mint("photo-booth", "task-1").unwrap();
        assert_eq!(minted.token.secret(), "t".repeat(43));
        assert_eq!(minted.grants, vec!["camera", "light"]);
        tokens.end("task-1");
        assert_eq!(
            served.join().unwrap(),
            vec![
                json!({"mint": {"skill": "photo-booth", "run": "task-1"}}),
                json!({"end": {"run": "task-1"}}),
            ]
        );
    }

    #[test]
    fn control_tokens_pass_on_lamp_controls_refusal_and_never_take_a_bad_answer() {
        let (_dir, tokens, served) = admin(
            |_| json!({"error": "no skill yes is kept on this lamp yet"}),
            1,
        );
        assert_eq!(
            tokens.mint("photo-booth", "task-1").unwrap_err(),
            "no skill yes is kept on this lamp yet"
        );
        served.join().unwrap();
        // Another run's token, or none: refused, and the run ended.
        for reply in [
            json!({"token": "t".repeat(43), "run": "task-9", "grants": []}),
            json!({"run": "task-1", "grants": ["light"]}),
            json!({"token": "t".repeat(43), "run": "task-1", "grants": [1]}),
        ] {
            let (_dir, tokens, served) = admin(
                move |asked| {
                    if asked.get("mint").is_some() {
                        reply.clone()
                    } else {
                        json!({"ended": true})
                    }
                },
                2,
            );
            assert!(tokens.mint("photo-booth", "task-1").is_err());
            assert_eq!(served.join().unwrap()[1], json!({"end": {"run": "task-1"}}));
        }
    }

    #[test]
    fn control_tokens_with_no_lamp_control_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let tokens = ControlTokens {
            socket: dir.path().join("admin.sock"),
            wait: Duration::from_secs(1),
        };
        let why = tokens.mint("photo-booth", "task-1").unwrap_err();
        assert!(why.starts_with("lamp-control is not reachable"), "{why}");
    }

    #[test]
    fn the_token_is_never_in_a_debug_line() {
        let token = RunToken::new("secret-value".into());
        assert_eq!(format!("{token:?}"), "RunToken(..)");
    }
}
