//! A voice make the lamp's own model couldn't make, handed to the lamp's
//! own agent, OpenClaw or Hermes (a proposal on #6059, the owner's OK and
//! the first builder's yes, 11 Nov): with `LAMP_MAKE_SKILL_AGENT=on`
//! ([`ENVIRONMENT`], off unless set, in no product unit).
//!
//! The order: DeepSeek writes first ([`crate::make_skill_local`]). With an
//! agent on the lamp it gets one try and no repairs: an answer not kept
//! (the tidies, the check, the say-back, its own `cannot:`, or no answer)
//! goes at once, with the same system prompt and request, to the agent,
//! which writes the two blocks again and gets the repairs. Its answers go
//! through the very same checks. With no agent on the lamp DeepSeek keeps
//! its repairs, as today; with no answer from the agent within
//! [`WRITE_WITHIN`], DeepSeek's failure stands.
//!
//! The agent is reached the way the voice hands it any work: the tasks
//! part's `lamp` target ([`crate::agent_tasks`], #5978, #6226), prepared on
//! its request socket and confirmed at once on the voice's own socket (the
//! person's spoken ask is the ask, as a voice make's always was), then
//! read back from the task's observations, which `lamp-tasks serve
//! --push-wait` keeps as soon as the agent ends its turn. The tasks part
//! takes at most 2,000 bytes of text, and the system prompt is longer, so
//! the system prompt and the brief go in a file of their own
//! ([`RULES_DIR`]) that the task names; the task's text begins as a brief
//! does ([`crate::made_skill::is_brief`]), so its answer is never told as
//! news. Once read, the task is marked told, so `--close-told` closes the
//! agent's session.
use crate::agent_tasks;
use serde_json::{Value, json};
use std::{
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

/// The switch: `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_MAKE_SKILL_AGENT";
/// The longest one of the agent's answers is waited for, from the hand-over
/// to its answer kept.
pub const WRITE_WITHIN: Duration = Duration::from_secs(45);
/// The longest the agent's part of a make goes on, its repairs included: a
/// repair is begun only while one more answer ([`WRITE_WITHIN`]) still
/// ends within it.
pub const MAKE_WITHIN: Duration = Duration::from_secs(120);
/// Where the system prompt and the brief are put for the agent to read:
/// the folder may be passed through by anyone and listed by no one but
/// root, and each file has a name nobody can guess and is removed once the
/// answer is in.
pub const RULES_DIR: &str = "/run/lampos-make-skill";
/// What the lamp says, through the say service, as the agent begins, so
/// the person isn't left in silence.
pub const WAITING_LINE: &str = "Let me think about that a little longer.";
/// How long the waiting line may wait to be said: past it, it is old.
const WAITING_WITHIN_MS: u64 = 15_000;
/// Between two looks at the task.
const LOOK_EVERY: Duration = Duration::from_millis(500);
/// The most of an answer taken, as the lamp's own model's.
const MOST_ANSWER_BYTES: usize = 20 * 1024;

/// Whether the switch is on.
pub fn on() -> bool {
    on_with(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether `value` turns the switch on: `on`, and nothing else.
fn on_with(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The tasks part's folder, as the second route's worker was given it
/// (`--tasks-dir`).
static TASKS: OnceLock<PathBuf> = OnceLock::new();

/// Kept by the second route's worker as it starts, with a tasks part.
pub fn set_tasks_dir(directory: PathBuf) {
    let _ = TASKS.set(directory);
}

/// The lamp's agent, reached through the tasks part.
#[derive(Clone, Debug)]
pub struct Agent {
    /// The tasks part's folder: `request.sock` and `voice.sock`.
    pub tasks: PathBuf,
    /// Where the rules file is put ([`RULES_DIR`]).
    pub rules: PathBuf,
    /// The say socket the waiting line goes to; none says nothing.
    pub say: Option<PathBuf>,
    /// [`WRITE_WITHIN`], or a test's own.
    pub within: Duration,
    /// [`MAKE_WITHIN`], or a test's own.
    pub make_within: Duration,
}

/// Why the agent gave no answer to take.
#[derive(Debug, PartialEq, Eq)]
pub enum NotWritten {
    /// The tasks part has no `lamp` target, or couldn't be asked.
    NoAgent,
    /// No answer within [`Agent::within`].
    Timeout,
    /// Anything else, in the lamp's own words (never the agent's).
    Failed(String),
}

impl std::fmt::Display for NotWritten {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAgent => f.write_str("no agent on the lamp"),
            Self::Timeout => write!(f, "no answer within {} s", WRITE_WITHIN.as_secs()),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

impl Agent {
    /// The agent for a voice make, with the switch on and a tasks part;
    /// none otherwise.
    pub fn for_voice() -> Option<Self> {
        if !on() {
            return None;
        }
        let tasks = TASKS.get()?.clone();
        Some(Self {
            tasks,
            rules: PathBuf::from(RULES_DIR),
            say: Some(PathBuf::from(crate::say::DEFAULT_SOCKET)),
            within: WRITE_WITHIN,
            make_within: MAKE_WITHIN,
        })
    }

    /// Who the lamp's agent is, as the person calls it ("Hermes",
    /// "OpenClaw"), from the tasks part's targets; none without one.
    pub fn engine(&self) -> Option<&'static str> {
        let answer = agent_tasks::ask(
            &self.tasks.join("request.sock"),
            &json!({"op":"targets"}),
            Duration::from_millis(1_500),
        )?;
        agent_tasks::lamp_target_in(&answer)
    }

    /// Say [`WAITING_LINE`] on a thread of its own; nothing waits on it.
    pub fn say_waiting(&self) {
        let Some(socket) = self.say.clone() else {
            return;
        };
        let _ = std::thread::Builder::new()
            .name("lamp-voice-make-waiting".into())
            .spawn(move || {
                crate::app_make::say_waiting_at(&socket, WAITING_LINE, WAITING_WITHIN_MS);
            });
    }

    /// The agent's answer to `system` and `brief`, as it wrote it, within
    /// [`Agent::within`].
    pub fn write(&self, system: &str, brief: &str) -> Result<String, NotWritten> {
        let began = Instant::now();
        let left = || self.within.saturating_sub(began.elapsed());
        let rules = Rules::put(&self.rules, system, brief)
            .map_err(|error| NotWritten::Failed(format!("its rules not written: {error}")))?;
        let text = task_text(&rules.path);
        let (store, session, id) = self.hand_over(&text, &left)?;
        let answer = self.answered(id, &left)?;
        self.told(&store, &session, id, answer.observation, &left);
        if answer.truncated || answer.text.len() > MOST_ANSWER_BYTES {
            return Err(NotWritten::Failed("its answer was cut".into()));
        }
        Ok(answer.text)
    }

    /// `text` prepared for the lamp's own target and confirmed at once:
    /// the store, the session and the task's id.
    fn hand_over(
        &self,
        text: &str,
        left: &impl Fn() -> Duration,
    ) -> Result<(String, String, u64), NotWritten> {
        let request = self.tasks.join("request.sock");
        let status = agent_tasks::ask(&request, &json!({"op":"status"}), left())
            .ok_or(NotWritten::NoAgent)?;
        let guard = agent_tasks::guard(&status, format!("make-{}", agent_tasks::nonce()))
            .ok_or(NotWritten::NoAgent)?;
        let prepared = agent_tasks::ask_until_sure(
            &request,
            &json!({"op":"prepare","guard":guard,"text":text,"target":agent_tasks::LAMP_TARGET}),
            left,
        )
        .ok_or(NotWritten::NoAgent)?;
        if prepared.get("error") == Some(&json!("target")) {
            return Err(NotWritten::NoAgent);
        }
        let (Some(id), Some(digest)) = (
            prepared.get("id").and_then(Value::as_u64),
            prepared.get("prepared_digest").and_then(Value::as_str),
        ) else {
            return Err(NotWritten::Failed(format!(
                "the tasks part didn't prepare it ({})",
                code(&prepared)
            )));
        };
        let status = agent_tasks::ask(&request, &json!({"op":"status"}), left())
            .ok_or(NotWritten::NoAgent)?;
        let (Some(guard), Some(session), Some(store)) = (
            agent_tasks::guard(&status, format!("make-{}", agent_tasks::nonce())),
            status["summary"]["session_id"].as_str(),
            status["store_id"].as_str(),
        ) else {
            return Err(NotWritten::NoAgent);
        };
        let confirmed = agent_tasks::ask_until_sure(
            &self.tasks.join("voice.sock"),
            &json!({"op":"confirm","guard":guard,"id":id,"prepared_digest":digest,"session_id":session}),
            left,
        )
        .ok_or(NotWritten::Timeout)?;
        if confirmed.get("ok") != Some(&json!(true)) {
            return Err(NotWritten::Failed(format!(
                "the tasks part didn't start it ({})",
                code(&confirmed)
            )));
        }
        Ok((store.to_owned(), session.to_owned(), id))
    }

    /// Task `id`'s answer, looked at every [`LOOK_EVERY`] until its agent
    /// ends its turn with one, stops, fails or asks, or the time is up.
    fn answered(&self, id: u64, left: &impl Fn() -> Duration) -> Result<Answer, NotWritten> {
        let request = self.tasks.join("request.sock");
        let mut read = None;
        loop {
            if left().is_zero() {
                return Err(NotWritten::Timeout);
            }
            if let Some(task) =
                agent_tasks::ask(&request, &json!({"op":"get_task","id":id}), left())
            {
                let state = task["task"]["dispatch"]["state"].as_str().unwrap_or("");
                if ["not_sent", "cancelled", "invalidated"].contains(&state) {
                    return Err(NotWritten::Failed(format!("its work was {state}")));
                }
                let newest = task["observations"]
                    .as_array()
                    .and_then(|seen| seen.iter().filter_map(|seen| seen["id"].as_u64()).max());
                if let Some(observation) = newest.filter(|newest| read != Some(*newest)) {
                    read = Some(observation);
                    let seen = agent_tasks::ask(
                        &request,
                        &json!({"op":"get_observation","id":id,"observation_id":observation}),
                        left(),
                    );
                    let data = seen.as_ref().map_or(&Value::Null, |seen| &seen["data"]);
                    let text = data["answer"].as_str().unwrap_or_default();
                    match data["state"].as_str().unwrap_or("unknown") {
                        "idle" if !text.trim().is_empty() => {
                            return Ok(Answer {
                                text: text.to_owned(),
                                observation,
                                truncated: data["truncated"] == json!(true),
                            });
                        }
                        state @ ("needs-input" | "stopped" | "failed") => {
                            return Err(NotWritten::Failed(format!("its agent {state}")));
                        }
                        _ => {}
                    }
                }
            }
            std::thread::sleep(LOOK_EVERY.min(left()));
        }
    }

    /// Task `id`'s answer marked told on the voice's socket, so the tasks
    /// part closes the agent's session (`--close-told`). Best effort.
    fn told(
        &self,
        store: &str,
        session: &str,
        id: u64,
        observation: u64,
        left: &impl Fn() -> Duration,
    ) {
        let request = self.tasks.join("request.sock");
        let Some(status) = agent_tasks::ask(&request, &json!({"op":"status"}), left()) else {
            return;
        };
        if status["store_id"].as_str() != Some(store) {
            return;
        }
        let Some(guard) = agent_tasks::guard(&status, format!("make-{}", agent_tasks::nonce()))
        else {
            return;
        };
        let _ = agent_tasks::ask(
            &self.tasks.join("voice.sock"),
            &json!({"op":"told","guard":guard,"id":id,"observation_id":observation,"session_id":session}),
            left().max(Duration::from_millis(500)),
        );
    }
}

/// The agent's answer as the tasks part kept it.
struct Answer {
    text: String,
    observation: u64,
    truncated: bool,
}

/// The tasks part's code only, never the task's words.
fn code(answer: &Value) -> String {
    answer
        .get("error")
        .and_then(Value::as_str)
        .filter(|code| code.len() <= 40 && code.bytes().all(|b| b.is_ascii_graphic()))
        .unwrap_or("no code")
        .to_owned()
}

/// The task's text: a brief's head ([`crate::made_skill::BRIEF_HEAD`]), so
/// it is never told as news, and where its rules and request are.
pub fn task_text(rules: &Path) -> String {
    format!(
        "{}: everything you need is in the file {}. Read it and answer exactly as it says, in \
         your reply alone: the two fenced blocks and nothing else, or its one line when the \
         skill can't be made. Write no files, install nothing and run nothing else.",
        crate::made_skill::BRIEF_HEAD,
        rules.display()
    )
}

/// One make's rules file: the system prompt, then the brief. Removed when
/// dropped.
struct Rules {
    path: PathBuf,
}

impl Rules {
    fn put(directory: &Path, system: &str, brief: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o711))?;
        let path = directory.join(format!("make-{:032x}.md", agent_tasks::nonce()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&path)?;
        let rules = Self { path };
        file.write_all(format!("{system}\n\n{brief}\n").as_bytes())?;
        Ok(rules)
    }
}

impl Drop for Rules {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Mutex};

    /// A stub tasks part in a folder of its own: its `lamp` target runs
    /// `engine` (none for no target), and its agent answers each task's
    /// rules with `answer(rules, brief)` after `after`, or never with
    /// `None`. The lines it was sent are kept.
    pub(crate) struct StubTasks {
        pub dir: Folder,
        pub sent: Arc<Mutex<Vec<Value>>>,
    }

    /// A folder of a test's own, short enough for a socket's path, removed
    /// when dropped.
    pub(crate) struct Folder(PathBuf);

    impl Folder {
        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    impl StubTasks {
        pub(crate) fn agent(&self) -> Agent {
            Agent {
                tasks: self.dir.path().to_path_buf(),
                rules: self.dir.path().join("rules"),
                say: None,
                within: Duration::from_millis(1_500),
                make_within: Duration::from_secs(10),
            }
        }

        pub(crate) fn prepares(&self) -> usize {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .filter(|line| line["op"] == "prepare")
                .count()
        }
    }

    type Answers = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;
    /// Each task's text, and its agent's answer once read.
    type Handed = Arc<Mutex<Vec<(String, Option<String>)>>>;

    pub(crate) fn stub(engine: Option<&'static str>, answers: Answers) -> StubTasks {
        let dir = Folder(PathBuf::from(format!(
            "/tmp/msa-{}-{:x}",
            std::process::id(),
            agent_tasks::nonce() & 0xffff_ffff
        )));
        std::fs::create_dir_all(dir.path()).unwrap();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let tasks: Handed = Arc::default();
        for name in ["request.sock", "voice.sock"] {
            let listener = UnixListener::bind(dir.path().join(name)).unwrap();
            let (sent, tasks, answers) = (sent.clone(), tasks.clone(), answers.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { return };
                    let mut line = String::new();
                    if BufReader::new(&stream).read_line(&mut line).is_err() {
                        continue;
                    }
                    let asked: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                    sent.lock().unwrap().push(asked.clone());
                    let answer = match asked["op"].as_str().unwrap_or_default() {
                        "targets" => match engine {
                            Some(engine) => {
                                json!({"ok":true,"targets":[{"alias":"lamp","engine":engine}]})
                            }
                            None => json!({"ok":true,"targets":[]}),
                        },
                        "status" => json!({"ok":true,"store_id":"s","revision":1,
                            "summary":{"session_id":"x"}}),
                        "prepare" if engine.is_none() => json!({"ok":false,"error":"target"}),
                        "prepare" => {
                            let mut tasks = tasks.lock().unwrap();
                            tasks.push((asked["text"].as_str().unwrap().to_owned(), None));
                            json!({"ok":true,"id":tasks.len(),"prepared_digest":"d"})
                        }
                        "confirm" | "told" => json!({"ok":true}),
                        "get_task" => {
                            let id = asked["id"].as_u64().unwrap() as usize;
                            let mut tasks = tasks.lock().unwrap();
                            let (text, answer) = &mut tasks[id - 1];
                            if answer.is_none() {
                                // The rules file the task names, read as the agent would.
                                let path = text
                                    .split("in the file ")
                                    .nth(1)
                                    .and_then(|rest| rest.split(". Read").next())
                                    .unwrap();
                                let rules = std::fs::read_to_string(path).unwrap();
                                *answer = answers(&rules);
                            }
                            let observations = if answer.is_some() {
                                json!([{"id": id * 10, "state": "idle"}])
                            } else {
                                json!([])
                            };
                            json!({"ok":true,"task":{"dispatch":{"state":"accepted"}},
                                "text":text,"observations":observations})
                        }
                        "get_observation" => {
                            let id = asked["id"].as_u64().unwrap() as usize;
                            let tasks = tasks.lock().unwrap();
                            json!({"ok":true,"data":{"state":"idle",
                                "answer":tasks[id - 1].1.clone().unwrap_or_default()}})
                        }
                        _ => json!({"ok":false,"error":"bad_request"}),
                    };
                    let _ = writeln!(stream, "{answer}");
                }
            });
        }
        StubTasks { dir, sent }
    }

    #[test]
    fn the_agent_is_handed_the_rules_in_a_file_and_its_answer_read_back() {
        let tasks = stub(
            Some("hermes"),
            Arc::new(|rules: &str| {
                assert!(rules.starts_with("SYSTEM\n\nBRIEF"), "{rules}");
                Some("```skill.toml\nx\n```".to_owned())
            }),
        );
        let agent = tasks.agent();
        assert_eq!(agent.engine(), Some("Hermes"));
        assert_eq!(
            agent.write("SYSTEM", "BRIEF").unwrap(),
            "```skill.toml\nx\n```"
        );
        let sent = tasks.sent.lock().unwrap().clone();
        let prepare = sent.iter().find(|line| line["op"] == "prepare").unwrap();
        let text = prepare["text"].as_str().unwrap();
        // Never told as news, and within the tasks part's bound.
        assert!(crate::made_skill::is_brief(text), "{text}");
        assert!(text.len() <= agent_tasks::MOST_TASK_BYTES);
        assert_eq!(prepare["target"], "lamp");
        assert!(sent.iter().any(|line| line["op"] == "confirm"));
        assert!(sent.iter().any(|line| line["op"] == "told"));
        // The rules file is gone once the answer is in.
        assert_eq!(
            std::fs::read_dir(tasks.dir.path().join("rules"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn no_lamp_target_is_no_agent_and_an_agent_that_never_answers_times_out() {
        let none = stub(None, Arc::new(|_: &str| None));
        assert_eq!(none.agent().engine(), None);
        assert_eq!(none.agent().write("S", "B"), Err(NotWritten::NoAgent));
        let silent = stub(Some("openclaw"), Arc::new(|_: &str| None));
        let began = Instant::now();
        assert_eq!(silent.agent().write("S", "B"), Err(NotWritten::Timeout));
        assert!(began.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn off_unless_set() {
        assert!(on_with(Some("on")));
        for off in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!on_with(off), "{off:?}");
        }
    }
}
