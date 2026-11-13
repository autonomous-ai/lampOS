//! A voice make the lamp's own model couldn't make, handed to the lamp's
//! own agent (a proposal on #6059, the owner's OK and the first builder's
//! yes, 11 Nov): with `LAMP_MAKE_SKILL_AGENT=on` ([`ENVIRONMENT`], off
//! unless set; on in the product's lamp-voice).
//!
//! The order: DeepSeek writes first ([`crate::make_skill_local`]). With
//! Hermes as the lamp's agent (the tasks part's `lamp` target; only Hermes
//! has a namespace runner for a make today, so another engine, OpenClaw,
//! is never handed one and DeepSeek makes it alone) it gets one try and
//! no repairs: an answer not kept (the tidies, the check, the say-back, its
//! own `cannot:`, or no answer) goes at once, with the same system prompt
//! and request, to the agent, which writes the blocks again and gets the
//! repairs. Its answers go through the very same checks. With no agent on
//! the lamp DeepSeek keeps its repairs, as today; with no answer from the
//! agent within [`WRITE_WITHIN`], DeepSeek's failure stands.
//!
//! The agent writes in a skill run's namespace (#6643, the first builder's
//! guard, 12 Nov 18:15): the make goes to lamp-tasks' make socket
//! ([`MAKE_SOCKET_NAME`], `lamp_tasks::skill_make`), which runs the lamp's
//! Hermes exactly as a skill's `do.task` run is started (row-6443's path),
//! through its namespace wrapper: `control.sock` alone and never
//! `admin.sock`. Its network is kept by default, as V1's agent had it (the
//! owner's ask, 12 Nov); lamp-tasks' `LAMP_MAKE_AGENT_NET=held` gives the
//! strict run, `LAMP_NS_NET=run`: only `lo` and the kernel's `@NONE`
//! tunnels, the model only through `model.sock`. It writes its answer in a
//! make's folder of its own, which lamp-tasks reads back. Never through a
//! Harness task with the agent's own tools: when lamp-tasks can't give that
//! run (an older build with no make socket, the switch off, a wrapper that
//! can't hold it off when held, or a wrapper or namespace script with no
//! make-home line, `no_namespace`), the make is not handed over and DeepSeek's
//! failure stands ([`NotWritten::NoMakeRun`]).
use crate::agent_tasks;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::OnceLock,
    time::Duration,
};

/// The switch: `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_MAKE_SKILL_AGENT";
/// The longest one of the agent's answers is waited for, from the hand-over
/// to its answer kept: the make run's own deadline.
pub const WRITE_WITHIN: Duration = Duration::from_secs(45);
/// The longest the agent's part of a make goes on, its repairs included: a
/// repair is begun only while one more answer ([`WRITE_WITHIN`]) still
/// ends within it.
pub const MAKE_WITHIN: Duration = Duration::from_secs(120);
/// lamp-tasks' make socket in its folder (`lamp_tasks::skill_make::MAKE_SOCKET`).
pub const MAKE_SOCKET_NAME: &str = "skill-make.sock";
/// What the lamp says, through the say service, as the agent begins, so
/// the person isn't left in silence.
pub const WAITING_LINE: &str = "Let me think about that a little longer.";
/// What the lamp says when the agent's own answer is `cannot:`: never the
/// agent's words.
pub const CANNOT_LINE: &str = "I couldn't make that one.";
/// How long the waiting line may wait to be said: past it, it is old.
const WAITING_WITHIN_MS: u64 = 15_000;
/// How long the line may take to send.
const SEND_WITHIN: Duration = Duration::from_secs(5);
/// The most of an answer taken, as the lamp's own model's.
const MOST_ANSWER_BYTES: usize = 20 * 1024;
/// lamp-tasks' refusals that mean no make run can be had here.
/// `no_namespace`: the wrapper or the namespace script has no make-home
/// line yet (an older lamp-update round).
const NOT_HELD_OFF_CODES: [&str; 3] = ["off", "not_held_off", "no_namespace"];
/// The one engine a make is handed to: only Hermes has a namespace runner
/// for a make today (lamp-tasks' `MakeHermes`, #6643's 19:14 ruling).
pub const MAKE_ENGINE: &str = "Hermes";

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
    /// The tasks part's folder: `request.sock` and [`MAKE_SOCKET_NAME`].
    pub tasks: PathBuf,
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
    /// lamp-tasks can't run the agent in a skill run's namespace here: an
    /// older build with no make socket, its switch off, or (held) a wrapper
    /// that can't hold it off. Never handed over any other way.
    NoMakeRun,
    /// No answer within [`Agent::within`].
    Timeout,
    /// Anything else, in the lamp's own words (never the agent's).
    Failed(String),
}

impl std::fmt::Display for NotWritten {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAgent => f.write_str("no agent on the lamp"),
            Self::NoMakeRun => f.write_str("no make run in the agent's namespace can be had here"),
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

    /// The lamp's agent when it is Hermes, the one engine a make is handed
    /// to ([`MAKE_ENGINE`]); any other (OpenClaw has no namespace runner for
    /// a make) is journaled with its name and not handed over, so
    /// DeepSeek's answer stands.
    pub fn hermes(&self) -> Option<&'static str> {
        let engine = self.engine()?;
        if engine == MAKE_ENGINE {
            return Some(engine);
        }
        eprintln!(
            "lamp-voice: a skill made here: the lamp's agent is {engine}: only Hermes has a namespace runner for a make today: not handed over, DeepSeek makes it alone"
        );
        None
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

    /// The agent's answer to `system` and `brief`, as it wrote it in its
    /// make run, within [`Agent::within`]. With `waiting`,
    /// [`WAITING_LINE`] is said once lamp-tasks says the run starts, never
    /// when it was refused at once.
    pub fn write(&self, system: &str, brief: &str, waiting: bool) -> Result<String, NotWritten> {
        let mut stream = UnixStream::connect(self.tasks.join(MAKE_SOCKET_NAME))
            .map_err(|_| NotWritten::NoMakeRun)?;
        // lamp-tasks ends the run at `within`; its answer comes just after.
        let grace = (self.within / 2).min(Duration::from_secs(5));
        let line = json!({
            "op": "skill_make",
            "rules": format!("{system}\n\n{brief}\n"),
            "within_ms": self.within.as_millis() as u64,
        });
        stream
            .set_write_timeout(Some(SEND_WITHIN))
            .and_then(|()| stream.set_read_timeout(Some(self.within + grace)))
            .and_then(|()| writeln!(stream, "{line}"))
            .map_err(|error| NotWritten::Failed(format!("the make was not sent: {error}")))?;
        let mut lines = BufReader::new(std::io::Read::take(stream, 2 * MOST_ANSWER_BYTES as u64));
        let mut read = || -> Result<Value, NotWritten> {
            let mut text = String::new();
            match lines.read_line(&mut text) {
                Ok(0) => Err(NotWritten::Failed("lamp-tasks gave no answer".into())),
                Ok(_) => serde_json::from_str(&text)
                    .map_err(|_| NotWritten::Failed("lamp-tasks' answer can't be read".into())),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    Err(NotWritten::Timeout)
                }
                Err(error) => Err(NotWritten::Failed(format!("lamp-tasks' answer: {error}"))),
            }
        };
        let mut answer = read()?;
        if answer == json!({"ok": true, "started": true}) {
            if waiting {
                self.say_waiting();
            }
            answer = read()?;
        }
        if answer.get("ok") == Some(&json!(true))
            && let Some(text) = answer.get("answer").and_then(Value::as_str)
        {
            return match text.len() > MOST_ANSWER_BYTES {
                true => Err(NotWritten::Failed("its answer was cut".into())),
                false => Ok(text.to_owned()),
            };
        }
        let code = code(&answer);
        if NOT_HELD_OFF_CODES.contains(&code.as_str()) {
            return Err(NotWritten::NoMakeRun);
        }
        Err(NotWritten::Failed(format!(
            "lamp-tasks didn't make it ({code})"
        )))
    }
}

/// Whether a make of a skill that uses a linked service is the lamp's
/// agent's (the owner's ask, 13 Nov): with `LAMP_AGENT_SERVICES` on and a
/// made skill's code on, as the making model is then told to make such a
/// skill as one `do.task` for the agent (`make_skill_local::agent_services`).
pub fn services_through_agent() -> bool {
    crate::made_skill::agent_services_here() && crate::made_skill::scripts_on()
}

/// Words that ask for a linked service whether or not it is linked, each
/// as whole words in order, with the service's code: none of them is an
/// everyday word for something the lamp sees or does itself.
const SERVICE_WORDS: &[(&str, &[&str])] = &[
    ("gmail", &["mail"]),
    ("gmail", &["mails"]),
    ("gmail", &["email"]),
    ("gmail", &["emails"]),
    ("gmail", &["emailed"]),
    ("gmail", &["emailing"]),
    ("gmail", &["inbox"]),
    ("gmail", &["gmail"]),
    ("google_calendar", &["calendar"]),
    ("google_calendar", &["calendars"]),
    ("google_calendar", &["meeting"]),
    ("google_calendar", &["meetings"]),
    ("google_calendar", &["appointment"]),
    ("google_calendar", &["appointments"]),
    ("google_drive", &["google", "drive"]),
    ("google_drive", &["drive", "file"]),
    ("google_drive", &["drive", "files"]),
    ("google_drive", &["drive", "folder"]),
    ("github", &["github"]),
    ("github", &["pull", "request"]),
    ("github", &["pull", "requests"]),
];

/// Words that ask for a linked service only when the lamp is told it is
/// linked: a service's own name, or a word that is an everyday one too
/// ("drive", "issue", "slack", "notion").
const LINKED_WORDS: &[(&str, &[&str])] = &[
    ("google_calendar", &["event"]),
    ("google_calendar", &["events"]),
    ("google_calendar", &["agenda"]),
    ("google_drive", &["drive"]),
    ("github", &["issue"]),
    ("github", &["issues"]),
    ("github", &["repo"]),
    ("github", &["repository"]),
    ("figma", &["figma"]),
    ("linear", &["linear"]),
    ("asana", &["asana"]),
    ("ahrefs", &["ahrefs"]),
    ("facebook", &["facebook"]),
    ("slack", &["slack"]),
    ("mondaydotcom", &["monday", "com"]),
    ("hubspot", &["hubspot"]),
    ("amplitude", &["amplitude"]),
    ("notion", &["notion"]),
];

/// The code of the linked service `words` ask for, when they do: one of
/// [`SERVICE_WORDS`], or one of [`LINKED_WORDS`] for a service `linked`
/// (the codes `lamp-services` tells linked now, asked only when no word of
/// the first list is there) names. Whole words only, in any case: "mail"
/// in "voicemail" or "Monday" alone is none. None asks for none.
pub fn service_asked(
    words: &str,
    linked: impl FnOnce() -> Option<Vec<String>>,
) -> Option<&'static str> {
    let words: Vec<String> = words
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let said = |phrase: &[&str]| {
        words
            .windows(phrase.len())
            .any(|run| run.iter().zip(phrase).all(|(word, wanted)| word == wanted))
    };
    if let Some((code, _)) = SERVICE_WORDS.iter().find(|(_, phrase)| said(phrase)) {
        return Some(code);
    }
    let linked = linked()?;
    LINKED_WORDS
        .iter()
        .find(|(code, phrase)| linked.iter().any(|kept| kept == code) && said(phrase))
        .map(|(code, _)| *code)
}

/// lamp-tasks' code only, never the run's words.
fn code(answer: &Value) -> String {
    answer
        .get("error")
        .and_then(Value::as_str)
        .filter(|code| code.len() <= 40 && code.bytes().all(|b| b.is_ascii_graphic()))
        .unwrap_or("no code")
        .to_owned()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    /// A stub tasks part in a folder of its own: its `lamp` target runs
    /// `engine` (none for no target; "old" for a build with no make
    /// socket; "not_held_off" for one whose run can't be held off;
    /// "no_namespace" for one whose wrapper has no make-home line), and
    /// its make run answers each make's rules with `answer(rules)`, or
    /// never with `None`. The lines it was sent are kept.
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
                say: None,
                within: Duration::from_millis(1_500),
                make_within: Duration::from_secs(10),
            }
        }

        /// The makes handed over to the make run.
        pub(crate) fn hand_overs(&self) -> usize {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .filter(|line| line["op"] == "skill_make")
                .count()
        }
    }

    type Answers = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

    pub(crate) fn stub(engine: Option<&'static str>, answers: Answers) -> StubTasks {
        static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = Folder(PathBuf::from(format!(
            "/tmp/msa-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )));
        let _ = std::fs::remove_dir_all(dir.path());
        std::fs::create_dir_all(dir.path()).unwrap();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut sockets = vec!["request.sock"];
        if engine != Some("old") {
            sockets.push(MAKE_SOCKET_NAME);
        }
        for name in sockets {
            let listener = UnixListener::bind(dir.path().join(name)).unwrap();
            let (sent, answers) = (sent.clone(), answers.clone());
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
                                let engine = match engine {
                                    "old" | "not_held_off" | "no_namespace" => "hermes",
                                    engine => engine,
                                };
                                json!({"ok":true,"targets":[{"alias":"lamp","engine":engine}]})
                            }
                            None => json!({"ok":true,"targets":[]}),
                        },
                        "skill_make" if matches!(engine, Some("not_held_off" | "no_namespace")) => {
                            json!({"ok":false,"error":engine.unwrap(),"message":"no"})
                        }
                        "skill_make" => {
                            let _ = writeln!(stream, "{}", json!({"ok":true,"started":true}));
                            match answers(asked["rules"].as_str().unwrap()) {
                                Some(text) => json!({"ok":true,"answer":text}),
                                None => {
                                    // A run that never ends in the voice's time.
                                    std::thread::sleep(Duration::from_secs(5));
                                    continue;
                                }
                            }
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
    fn the_agent_is_handed_the_rules_as_a_make_run_and_its_answer_read_back() {
        let tasks = stub(
            Some("hermes"),
            Arc::new(|rules: &str| {
                assert_eq!(rules, "SYSTEM\n\nBRIEF\n");
                Some("```skill.toml\nx\n```".to_owned())
            }),
        );
        let agent = tasks.agent();
        assert_eq!(agent.engine(), Some("Hermes"));
        assert_eq!(
            agent.write("SYSTEM", "BRIEF", false).unwrap(),
            "```skill.toml\nx\n```"
        );
        let sent = tasks.sent.lock().unwrap().clone();
        let make = sent.iter().find(|line| line["op"] == "skill_make").unwrap();
        // The make's line, as lamp-tasks' make run takes it, and
        // never a Harness task with the agent's own tools.
        assert_eq!(make["within_ms"], 1_500);
        assert_eq!(
            make.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["op", "rules", "within_ms"]
        );
        assert!(!sent.iter().any(|line| line["op"] == "prepare"));
    }

    /// The first builder's guard on #6643: no make run, no hand-over.
    /// An older lamp-tasks with no make socket, or one whose run can't be
    /// held off, is [`NotWritten::NoMakeRun`]; nothing is prepared.
    #[test]
    fn no_make_run_is_never_handed_over() {
        for engine in ["old", "not_held_off", "no_namespace"] {
            let tasks = stub(Some(engine), Arc::new(|_: &str| Some("x".into())));
            assert_eq!(tasks.agent().engine(), Some("Hermes"));
            assert_eq!(
                tasks.agent().write("S", "B", false),
                Err(NotWritten::NoMakeRun),
                "{engine}"
            );
            assert!(
                !tasks
                    .sent
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|line| line["op"] == "prepare")
            );
        }
    }

    #[test]
    fn no_lamp_target_is_no_agent_and_an_agent_that_never_answers_times_out() {
        let none = stub(None, Arc::new(|_: &str| None));
        assert_eq!(none.agent().engine(), None);
        assert_eq!(none.agent().hermes(), None);
        // OpenClaw is the lamp's agent, but a make is Hermes's alone.
        let openclaw = stub(Some("openclaw"), Arc::new(|_: &str| Some("A".into())));
        assert_eq!(openclaw.agent().engine(), Some("OpenClaw"));
        assert_eq!(openclaw.agent().hermes(), None);
        let hermes = stub(Some("hermes"), Arc::new(|_: &str| Some("A".into())));
        assert_eq!(hermes.agent().hermes(), Some("Hermes"));
        let silent = stub(Some("openclaw"), Arc::new(|_: &str| None));
        let began = Instant::now();
        assert_eq!(
            silent.agent().write("S", "B", false),
            Err(NotWritten::Timeout)
        );
        assert!(began.elapsed() < Duration::from_secs(4));
    }

    /// The owner's ask, 13 Nov: a make naming mail, a calendar, Drive or
    /// GitHub asks for a linked service, as whole words; a word that is an
    /// everyday one too counts only for a service told linked; a make
    /// naming none asks for none.
    #[test]
    fn a_linked_service_is_asked_for_in_whole_words() {
        let none = || -> Option<Vec<String>> { None };
        for (words, code) in [
            ("when I leave my desk, email me that I'm out", "gmail"),
            ("every morning at eight, read me my new mail", "gmail"),
            ("tell me when my inbox has something new", "gmail"),
            ("send an E-mail to Minh when I arrive", "gmail"),
            (
                "every evening, tell me tomorrow's calendar",
                "google_calendar",
            ),
            (
                "glow blue five minutes before each meeting",
                "google_calendar",
            ),
            ("list what's new in my Google Drive at noon", "google_drive"),
            ("say how many pull requests wait for me", "github"),
            ("read my GitHub notifications at nine", "github"),
        ] {
            assert_eq!(service_asked(words, none), Some(code), "{words}");
        }
        for words in [
            "make me a skill that dims the light at 10 pm",
            "play my voicemail jingle when I arrive",
            "nod when someone says emailer",
            "every Monday at nine, glow green",
            "when there's an issue with the air, say so",
            "glow red when I drive home",
        ] {
            assert_eq!(service_asked(words, none), None, "{words}");
        }
        let linked = || Some(vec!["github".to_owned(), "slack".to_owned()]);
        assert_eq!(
            service_asked("say my new issues at nine", linked),
            Some("github")
        );
        assert_eq!(
            service_asked("read my Slack messages when I sit down", linked),
            Some("slack")
        );
        assert_eq!(service_asked("what's on my drive at noon", linked), None);
        assert_eq!(
            service_asked("what's on my drive at noon", || Some(vec![
                "google_drive".to_owned()
            ])),
            Some("google_drive")
        );
        // What is linked is asked only when no plain word is there.
        assert_eq!(
            service_asked("email me", || -> Option<Vec<String>> {
                panic!("not asked")
            }),
            Some("gmail")
        );
    }

    #[test]
    fn off_unless_set() {
        assert!(on_with(Some("on")));
        for off in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!on_with(off), "{off:?}");
        }
    }
}
