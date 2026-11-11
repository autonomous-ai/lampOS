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
//! - **A one-run token** ([`Tokens`]): minted by lamp-control (#6304) for
//!   exactly this skill, its folder's SHA-256 and its grants, given to the
//!   run as [`TOKEN_ENV`] beside [`SOCKET_ENV`], and ended when the run
//!   ends, however it ends. Until #6304 lands, [`NoTokens`] refuses every
//!   run (`no_token`): nothing runs without a token.
//! - **The run** ([`Runner`]): one task for target `lamp` (the lamp's own
//!   Hermes), its words the skill's task and its script's place
//!   ([`SkillRun::prompt`]). The runner blocks until the run ends.
use crate::protocol::refusal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};

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

/// lamp-control's one-run tokens (#6304): minted from the owner's yes for
/// exactly `skill`, its folder's `sha256` and `grants`, ended with the run.
/// The adapter for its `control.sock` plugs in here once #6304 lands.
pub trait Tokens {
    fn mint(&self, skill: &str, sha256: &str, grants: &[String]) -> Result<RunToken, String>;
    /// The run ended: the token is refused from now on.
    fn end(&self, token: &RunToken);
}

/// No lamp-control token yet (#6304 not landed): every run is refused.
pub struct NoTokens;

impl Tokens for NoTokens {
    fn mint(&self, _: &str, _: &str, _: &[String]) -> Result<RunToken, String> {
        Err("no one-run token on this lamp yet (lamp-control's control.sock, #6304)".into())
    }

    fn end(&self, _: &RunToken) {}
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
    token: RunToken,
}

impl Drop for Ending<'_> {
    fn drop(&mut self) {
        self.tokens.end(&self.token);
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
    let token = match tokens.mint(&hand.skill, &hand.sha256, &hand.grants) {
        Ok(token) => token,
        Err(why) => return refusal("no_token", &why),
    };
    let ending = Ending { tokens, token };
    let run = SkillRun::of(&hand, &ending.token);
    let ran = runner.run(&run);
    drop(run);
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
        minted: RefCell<Vec<(String, String, Vec<String>)>>,
        live: RefCell<Vec<String>>,
        ended: RefCell<Vec<String>>,
    }

    impl Tokens for FakeTokens {
        fn mint(&self, skill: &str, sha256: &str, grants: &[String]) -> Result<RunToken, String> {
            if self.refuse {
                return Err("no yes for these grants".into());
            }
            self.minted
                .borrow_mut()
                .push((skill.into(), sha256.into(), grants.to_vec()));
            let secret = format!("tok-{}", self.minted.borrow().len());
            self.live.borrow_mut().push(secret.clone());
            Ok(RunToken::new(secret))
        }

        fn end(&self, token: &RunToken) {
            self.live.borrow_mut().retain(|live| live != token.secret());
            self.ended.borrow_mut().push(token.secret().into());
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
            let live = self.tokens.live.borrow().contains(&token);
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
        assert_eq!(
            *tokens.minted.borrow(),
            vec![(
                "photo-booth".to_owned(),
                SHA.to_owned(),
                vec!["camera".to_owned(), "light".to_owned()]
            )]
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
        assert_eq!(*tokens.ended.borrow(), vec!["tok-1".to_owned()]);
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
    fn the_token_is_never_in_a_debug_line() {
        let token = RunToken::new("secret-value".into());
        assert_eq!(format!("{token:?}"), "RunToken(..)");
    }
}
