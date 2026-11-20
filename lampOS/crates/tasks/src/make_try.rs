//! A made skill's tried run before it is kept (#8157): the voice, once its
//! own check has passed a made skill, sends its files on the make socket
//! (`{"op":"make_try","name","files"}`, [`TryLine`]) and keeps the skill
//! only when lamp-tasks answers that its try passed.
//!
//! - **What is tried** ([`plan`]): a `run` step's script (`[[do]]`
//!   `run = "scripts/main.py"`: the script does the whole job and speaks),
//!   or, for a skill with a `task` step and no `run` step, each script under
//!   `scripts/` (the task's own and its helpers). A skill with no script has
//!   nothing to try (`"tried":false`).
//! - **How**: the same fence a gateway agent's run gets from the
//!   skill-start launcher (`crate::gateway::FENCE_TEXT`), started by
//!   lamp-tasks itself, never by an agent: whichever agent made the skill
//!   (OpenClaw or Hermes), its try is the same. Its token is lamp-control's,
//!   minted for the draft's own grants and marked tried
//!   (`{"mint":{"try":[...]}}`): its linked service writes are answered as
//!   if done from a stub and never sent, its reads are live. A draft
//!   granted `net` gets its own egress socket, as a skill run does.
//! - **Judged** ([`judged`]): each script exits 0 within [`TRY_WITHIN`];
//!   a `run` step's script said at least one line through `lamp.say` (the
//!   run's say count, lamp-control's `end` answer's `"said"`); a task's
//!   script printed something.
//! - **The answer**: `{"ok":true,"tried":false}` (nothing to try),
//!   `{"ok":true,"tried":true,"passed":true,"said":N}`, or
//!   `{"ok":true,"tried":true,"passed":false,"why":"..."}`, whose words go
//!   back to the agent as one of the voice's repairs. A try that could not
//!   be started at all is a refusal (`{"ok":false,"error":"not_tried"}`).
use crate::protocol::refusal;
use crate::skill_task::{Minted, Tokens};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The line's op.
pub const OP: &str = "make_try";
/// The longest one script of a try may run.
pub const TRY_WITHIN: Duration = Duration::from_secs(30);
/// The most scripts one try runs.
pub const MOST_SCRIPTS: usize = 4;
/// The most files a try's line carries.
pub const MOST_FILES: usize = 64;
/// The most of a script's output kept to judge it.
pub const MOST_OUTPUT_BYTES: usize = 8 * 1024;
/// How much of a failed script's error output goes back in the repair.
pub const MOST_WHY_BYTES: usize = 600;
/// lamp-tasks' own argument that becomes a try's fence
/// ([`fence_main`]).
pub const FENCE_ARG: &str = "make-try-fence";
/// A busy try: one at a time.
pub const BUSY: &str = "a made skill is already being tried; one at a time";
/// What a `run` step's script that never spoke is told.
pub const SAID_NOTHING: &str = "said nothing through lamp.say: a run step's script says its own success line and its failure line with lamp.say";

/// The voice's line.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TryLine {
    pub op: String,
    /// The skill's name, its folder's.
    pub name: String,
    /// Its files by their path in its folder: `skill.toml`, `SKILL.md`,
    /// `scripts/...`, `shows/...`.
    pub files: BTreeMap<String, String>,
}

/// Whether `line` is a try's line.
pub fn is_try(line: &[u8]) -> bool {
    serde_json::from_slice::<Value>(line).is_ok_and(|line| line["op"] == OP)
}

/// A made skill's step kind, as its try judges it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `run = "scripts/main.py"`: the script does the whole job and speaks.
    Run,
    /// A `task` step: the agent runs its scripts and sums up.
    Task,
}

/// What a try runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub kind: Kind,
    /// Each script's path in the skill's folder.
    pub scripts: Vec<String>,
    /// The skill's grants, as its `skill.toml` names them.
    pub grants: Vec<String>,
}

/// Whether `path` is a script's own path in its folder: `scripts/...`,
/// relative, no `..`, ending `.py`.
fn script_path(path: &str) -> bool {
    path.starts_with("scripts/")
        && path.ends_with(".py")
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// What a skill's `toml` and its `files` have to try: none for a skill with
/// no script step.
pub fn plan(toml: &str, files: &BTreeMap<String, String>) -> Result<Option<Plan>, String> {
    let table: toml::Table = toml
        .parse()
        .map_err(|_| "its skill.toml can't be read".to_owned())?;
    let grants = match table.get("grants") {
        None => Vec::new(),
        Some(toml::Value::Array(grants)) => grants
            .iter()
            .map(|grant| grant.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or("its grants are not text")?,
        Some(_) => return Err("its grants are not a list".into()),
    };
    let steps = table
        .get("do")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut runs = Vec::new();
    let mut tasks = false;
    for step in &steps {
        if let Some(run) = step.get("run").and_then(toml::Value::as_str) {
            runs.push(run.to_owned());
        }
        if step
            .get("task")
            .is_some_and(|task| task.get("run").is_some())
        {
            tasks = true;
        }
    }
    let (kind, scripts) = if !runs.is_empty() {
        (Kind::Run, runs)
    } else if tasks {
        (
            Kind::Task,
            files
                .keys()
                .filter(|path| script_path(path))
                .cloned()
                .collect(),
        )
    } else {
        return Ok(None);
    };
    if let Some(bad) = scripts.iter().find(|path| !script_path(path)) {
        return Err(format!("{bad} is not a script in its scripts/ folder"));
    }
    if let Some(missing) = scripts.iter().find(|path| !files.contains_key(*path)) {
        return Err(format!("{missing} is named by a step but was not written"));
    }
    if scripts.is_empty() {
        return Err("its task step names a script, but none was written".into());
    }
    if scripts.len() > MOST_SCRIPTS {
        return Err(format!("at most {MOST_SCRIPTS} scripts are tried"));
    }
    Ok(Some(Plan {
        kind,
        scripts,
        grants,
    }))
}

/// One script's tried run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ran {
    /// Its exit code; none when it was ended by a signal.
    pub code: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
    /// The `say`s of its run the voice said whole, from lamp-control; none
    /// when an older lamp-control didn't say.
    pub said: Option<u32>,
}

/// The last `most` bytes of `text`, on a character's edge, in one line.
fn tail(text: &str, most: usize) -> String {
    let text = text.trim();
    let mut start = text.len().saturating_sub(most);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..]
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Whether `script`'s run `ran` passes a try of a `kind` skill; else its
/// words for the repair.
pub fn judged(kind: Kind, script: &str, ran: &Ran) -> Result<(), String> {
    if ran.timed_out {
        return Err(format!(
            "{script} did not end within {} s when tried",
            TRY_WITHIN.as_secs()
        ));
    }
    if ran.code != Some(0) {
        let code = ran
            .code
            .map_or("by a signal".to_owned(), |code| format!("{code}"));
        let error = tail(&ran.stderr, MOST_WHY_BYTES);
        return Err(match error.is_empty() {
            true => format!("{script} exited {code} when tried"),
            false => format!("{script} exited {code} when tried: {error}"),
        });
    }
    match kind {
        Kind::Run if ran.said == Some(0) => Err(format!("{script} {SAID_NOTHING}")),
        Kind::Task if ran.stdout.trim().is_empty() => Err(format!(
            "{script} printed nothing when tried: a task's script prints what the agent sums up"
        )),
        _ => Ok(()),
    }
}

/// How a try's script is run: in its fence on a lamp ([`FenceLaunch`]).
pub trait Launch: Send + Sync {
    /// `script` (its path in `skill`) run once, in `run`'s folder, with
    /// `grants`, ended at `within`.
    fn launch(
        &self,
        skill: &Path,
        run: &Path,
        script: &str,
        grants: &[String],
        within: Duration,
    ) -> Result<Ran, String>;
}

/// The voice's tries: one at a time, each in its own folder in `work`.
pub struct Tries {
    pub work: PathBuf,
    pub launch: Box<dyn Launch>,
    pub busy: AtomicBool,
}

/// One try's folder, removed however the try ends.
struct TryFolder(PathBuf);

impl Drop for TryFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Whether `name` is a skill's plain name.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Tries {
    /// lamp-tasks' answer to one try's `line`.
    pub fn answer(&self, line: &[u8]) -> Value {
        let asked: TryLine = match serde_json::from_slice(line) {
            Ok(asked) => asked,
            Err(_) => return refusal("bad_request", "not a try's line"),
        };
        if asked.op != OP || !plain_name(&asked.name) || asked.files.len() > MOST_FILES {
            return refusal("bad_request", "not a try's line");
        }
        let Some(toml) = asked.files.get("skill.toml") else {
            return refusal("bad_request", "a try's line names no skill.toml");
        };
        let plan = match plan(toml, &asked.files) {
            Ok(Some(plan)) => plan,
            Ok(None) => return json!({"ok": true, "tried": false}),
            Err(why) => return json!({"ok": true, "tried": true, "passed": false, "why": why}),
        };
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return refusal("busy", BUSY);
        }
        let answered = self.tried(&asked, &plan);
        self.busy.store(false, Ordering::Release);
        answered
    }

    fn tried(&self, asked: &TryLine, plan: &Plan) -> Value {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let place = self.work.join(format!(
            "{}try-{}-{}",
            crate::skill_make::FOLDER_HEAD,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&place);
        let folder = TryFolder(place);
        let skill = folder.0.join("skill").join(&asked.name);
        let runs = folder.0.join("runs");
        let written = (|| {
            std::fs::create_dir_all(&skill)?;
            std::fs::create_dir_all(&runs)?;
            for (path, text) in &asked.files {
                if !Path::new(path)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
                {
                    return Err(std::io::Error::other("a file's path outside its folder"));
                }
                let file = skill.join(path);
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(file, text)?;
            }
            Ok::<_, std::io::Error>(())
        })();
        if let Err(error) = written {
            return refusal(
                "not_tried",
                &format!("the try's folder was not made: {error}"),
            );
        }
        let mut said = 0u32;
        for (n, script) in plan.scripts.iter().enumerate() {
            let run = runs.join(format!("run-{n}"));
            if let Err(error) = std::fs::create_dir_all(&run) {
                return refusal(
                    "not_tried",
                    &format!("the try's run folder was not made: {error}"),
                );
            }
            let ran = match self
                .launch
                .launch(&skill, &run, script, &plan.grants, TRY_WITHIN)
            {
                Ok(ran) => ran,
                Err(why) => {
                    eprintln!("[lamp-tasks] make try: {script} was not run: {why}");
                    return refusal("not_tried", &why);
                }
            };
            eprintln!(
                "[lamp-tasks] make try: {script}: exit {}, {} s limit{}, said {}",
                ran.code.map_or("none".to_owned(), |code| code.to_string()),
                TRY_WITHIN.as_secs(),
                if ran.timed_out { " passed" } else { "" },
                ran.said
                    .map_or("unknown".to_owned(), |said| said.to_string()),
            );
            if let Err(why) = judged(plan.kind, script, &ran) {
                return json!({"ok": true, "tried": true, "passed": false, "why": why});
            }
            said = said.saturating_add(ran.said.unwrap_or(0));
        }
        json!({"ok": true, "tried": true, "passed": true, "said": said})
    }
}

/// The lamp's launch: a token from lamp-control for the draft's grants,
/// marked tried; the run's egress socket when granted `net`; then
/// lamp-tasks itself started as [`FENCE_ARG`], which becomes the same fence
/// a gateway agent's run gets ([`crate::gateway::fence_args`]), the script
/// run in it with only the SDK, the socket and the token.
pub struct FenceLaunch {
    pub tokens: Box<dyn Tokens + Send + Sync>,
    /// The program started as the fence: lamp-tasks' own.
    pub program: PathBuf,
    /// The egress a draft granted `net` gets (`crate::egress::Grant::lamp`).
    pub egress: Option<crate::egress::Grant>,
}

impl FenceLaunch {
    /// The lamp's: lamp-control's tokens, this program, the lamp's egress
    /// with `LAMP_TASK_NET` on.
    pub fn lamp() -> Self {
        let net = std::env::var(crate::skill_task::NET_SWITCH).ok().as_deref() == Some("on");
        Self {
            tokens: Box::new(crate::skill_task::ControlTokens::default()),
            program: std::env::current_exe()
                .unwrap_or_else(|_| PathBuf::from(crate::gateway::RELAY)),
            egress: net.then(crate::egress::Grant::lamp),
        }
    }
}

/// `reader` read whole on a thread of its own, at most
/// [`MOST_OUTPUT_BYTES`] kept.
fn drained(reader: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let _ = reader.take(MOST_OUTPUT_BYTES as u64).read_to_end(&mut kept);
        String::from_utf8_lossy(&kept).into_owned()
    })
}

impl Launch for FenceLaunch {
    fn launch(
        &self,
        skill: &Path,
        run: &Path,
        script: &str,
        grants: &[String],
        within: Duration,
    ) -> Result<Ran, String> {
        let id = crate::skill_task::run_id();
        let Minted { token, .. } = self.tokens.mint_try(grants, &id)?;
        let until = Instant::now() + within + Duration::from_secs(5);
        let net = grants
            .iter()
            .any(|grant| grant == crate::skill_task::NET_GRANT);
        let way_out = match (net, &self.egress) {
            (false, _) => None,
            (true, Some(egress)) => Some(
                egress
                    .open(&format!("{}{id}", crate::skill_make::FOLDER_HEAD), until)
                    .map_err(|error| {
                        self.tokens.end(&id);
                        format!("the try's way out was not made: {}", error.kind())
                    })?,
            ),
            (true, None) => {
                self.tokens.end(&id);
                return Err(format!(
                    "a draft granted net is tried only with {} on",
                    crate::skill_task::NET_SWITCH
                ));
            }
        };
        let grant = crate::skill_start::Grant {
            script: skill.join(script),
            interpreter: crate::gateway::INTERPRETER.into(),
            skill: skill.to_owned(),
            run: run.to_owned(),
            socket: crate::skill_task::CONTROL_SOCKET.into(),
            pythonpath: crate::skill_task::SDK_PYTHONPATH.into(),
            token: token.secret().to_owned(),
            egress: way_out.as_ref().map(|served| served.path().to_owned()),
        };
        let ran = run_fence(&self.program, &grant, within);
        let said = self.tokens.end_said(&id);
        drop(way_out);
        let mut ran = ran?;
        ran.said = said;
        Ok(ran)
    }
}

/// `program` started as [`FENCE_ARG`] with `grant` on its stdin, waited for
/// up to `within`, then killed (its fence's PID namespace goes with it).
pub fn run_fence(
    program: &Path,
    grant: &crate::skill_start::Grant,
    within: Duration,
) -> Result<Ran, String> {
    let mut child = Command::new(program)
        .arg(FENCE_ARG)
        .env_clear()
        .env("PATH", crate::gateway::SCRIPT_PATH)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("the try's fence was not started: {error}"))?;
    let line = serde_json::to_string(grant).map_err(|_| "the try's grant".to_owned())?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{line}");
    }
    let stdout = drained(child.stdout.take().ok_or("no stdout")?);
    let stderr = drained(child.stderr.take().ok_or("no stderr")?);
    let began = Instant::now();
    let (code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None) if began.elapsed() >= within => {
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(format!("the try's fence: {error}")),
        }
    };
    Ok(Ran {
        code,
        timed_out,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
        said: None,
    })
}

/// How the egress socket is opened for the fence: `O_PATH` on a lamp
/// (Linux), as the launcher is sent it; read-only elsewhere (tests).
fn egress_flags() -> rustix::fs::OFlags {
    #[cfg(target_os = "linux")]
    let flags = rustix::fs::OFlags::PATH | rustix::fs::OFlags::CLOEXEC;
    #[cfg(not(target_os = "linux"))]
    let flags = rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC;
    flags
}

/// `lamp-tasks make-try-fence`: the grant read from stdin, then this
/// process becomes the run's fence (`crate::skill_start`'s, fd 4 a sink in
/// place of the launcher's socket). Returns only with why it did not.
pub fn fence_main() -> (i32, String) {
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return (1, "no grant on stdin".into());
    }
    let grant: crate::skill_start::Grant = match serde_json::from_str(line.trim()) {
        Ok(grant) => grant,
        Err(_) => return (1, "the grant can't be read".into()),
    };
    let egress = match &grant.egress {
        None => None,
        Some(path) => match rustix::fs::open(path, egress_flags(), rustix::fs::Mode::empty()) {
            Ok(fd) => Some(fd),
            Err(error) => return (1, format!("the try's egress socket: {error}")),
        },
    };
    let sink = match std::fs::OpenOptions::new().write(true).open("/dev/null") {
        Ok(sink) => std::os::fd::OwnedFd::from(sink),
        Err(error) => return (1, format!("no /dev/null: {error}")),
    };
    match crate::skill_start::exec_fence(&grant, sink, egress) {
        Ok(never) => match never {},
        Err(error) => (1, format!("the try's fence was not started: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn files(toml: &str, scripts: &[&str]) -> BTreeMap<String, String> {
        let mut files = BTreeMap::new();
        files.insert("skill.toml".to_owned(), toml.to_owned());
        files.insert("SKILL.md".to_owned(), "---\nname: x\n---\n".to_owned());
        for script in scripts {
            files.insert((*script).to_owned(), "print('x')\n".to_owned());
        }
        files
    }

    const RUN_TOML: &str = "asks = [\"bitcoin price\"]\nkind = \"installable\"\nversion = \"1.0.0\"\ngrants = [\"net\", \"speak\"]\n\n[[do]]\nrun = \"scripts/main.py\"\n";
    const TASK_TOML: &str = "asks = [\"sum up my mail\"]\nkind = \"installable\"\nversion = \"1.0.0\"\ngrants = [\"gmail-read\"]\n\n[[do]]\ntask = { task = \"Sum up today's mail\", run = \"scripts/main.py\" }\n";
    const SEND_TOML: &str = "asks = [\"mail my boss\"]\nkind = \"installable\"\nversion = \"1.0.0\"\ngrants = [\"gmail-write\", \"speak\"]\n\n[[do]]\nrun = \"scripts/main.py\"\n";

    #[test]
    fn a_run_step_tries_its_script_and_a_task_each_of_its_scripts() {
        let plan_of = |toml: &str, scripts: &[&str]| plan(toml, &files(toml, scripts)).unwrap();
        assert_eq!(
            plan_of(RUN_TOML, &["scripts/main.py", "scripts/util.py"]),
            Some(Plan {
                kind: Kind::Run,
                scripts: vec!["scripts/main.py".into()],
                grants: vec!["net".into(), "speak".into()],
            })
        );
        let task = plan_of(TASK_TOML, &["scripts/main.py", "scripts/fetch_mail.py"]).unwrap();
        assert_eq!(task.kind, Kind::Task);
        assert_eq!(
            task.scripts,
            vec![
                "scripts/fetch_mail.py".to_owned(),
                "scripts/main.py".to_owned()
            ]
        );
        // No script step: nothing to try.
        let light = "asks = [\"desk glow\"]\n[[do]]\nlight = { color = \"warm\" }\n";
        assert_eq!(plan_of(light, &[]), None);
        // A run step's script that was never written, or outside scripts/.
        assert!(plan(RUN_TOML, &files(RUN_TOML, &[])).is_err());
        let outside = RUN_TOML.replace("scripts/main.py", "../main.py");
        assert!(plan(&outside, &files(&outside, &["scripts/main.py"])).is_err());
    }

    #[test]
    fn a_try_passes_only_on_exit_0_in_time_and_for_a_run_step_a_say() {
        let ok = Ran {
            code: Some(0),
            said: Some(1),
            stdout: "Said aloud.".into(),
            ..Ran::default()
        };
        assert_eq!(judged(Kind::Run, "scripts/main.py", &ok), Ok(()));
        let silent = Ran {
            said: Some(0),
            ..ok.clone()
        };
        let why = judged(Kind::Run, "scripts/main.py", &silent).unwrap_err();
        assert!(why.contains("lamp.say"), "{why}");
        // An older lamp-control that doesn't count: not held against it.
        let unknown = Ran {
            said: None,
            ..ok.clone()
        };
        assert_eq!(judged(Kind::Run, "scripts/main.py", &unknown), Ok(()));
        let failed = Ran {
            code: Some(1),
            stderr: "Traceback ...\nKeyError: 'price'\n".into(),
            ..ok.clone()
        };
        let why = judged(Kind::Run, "scripts/main.py", &failed).unwrap_err();
        assert!(
            why.contains("exited 1") && why.contains("KeyError"),
            "{why}"
        );
        let late = Ran {
            timed_out: true,
            code: None,
            ..ok.clone()
        };
        assert!(
            judged(Kind::Run, "scripts/main.py", &late)
                .unwrap_err()
                .contains("within 30 s")
        );
        // A task's helper: exit 0 with output, no say needed.
        let helper = Ran {
            said: Some(0),
            stdout: "3 unread".into(),
            ..ok.clone()
        };
        assert_eq!(judged(Kind::Task, "scripts/fetch.py", &helper), Ok(()));
        let empty = Ran {
            stdout: " \n".into(),
            ..helper
        };
        assert!(judged(Kind::Task, "scripts/fetch.py", &empty).is_err());
    }

    /// A launch that records what it was asked and answers `ran`.
    struct Fake {
        asked: Arc<Mutex<Vec<(String, Vec<String>)>>>,
        ran: Ran,
    }

    impl Launch for Fake {
        fn launch(
            &self,
            skill: &Path,
            run: &Path,
            script: &str,
            grants: &[String],
            within: Duration,
        ) -> Result<Ran, String> {
            assert!(skill.join(script).is_file());
            assert!(skill.join("skill.toml").is_file());
            assert!(run.is_dir());
            assert_eq!(within, TRY_WITHIN);
            self.asked
                .lock()
                .unwrap()
                .push((script.to_owned(), grants.to_vec()));
            Ok(self.ran.clone())
        }
    }

    fn tries(
        ran: Ran,
    ) -> (
        tempfile::TempDir,
        Tries,
        Arc<Mutex<Vec<(String, Vec<String>)>>>,
    ) {
        let work = tempfile::tempdir().unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let tries = Tries {
            work: work.path().to_owned(),
            launch: Box::new(Fake {
                asked: asked.clone(),
                ran,
            }),
            busy: AtomicBool::new(false),
        };
        (work, tries, asked)
    }

    fn line(toml: &str, scripts: &[&str]) -> Vec<u8> {
        json!({"op": OP, "name": "bitcoin-price", "files": files(toml, scripts)})
            .to_string()
            .into_bytes()
    }

    #[test]
    fn a_tried_run_is_answered_and_its_folder_removed() {
        let spoke = Ran {
            code: Some(0),
            said: Some(1),
            ..Ran::default()
        };
        let (work, tries, asked) = tries(spoke);
        assert!(is_try(&line(RUN_TOML, &["scripts/main.py"])));
        let answered = tries.answer(&line(RUN_TOML, &["scripts/main.py"]));
        assert_eq!(
            answered,
            json!({"ok": true, "tried": true, "passed": true, "said": 1})
        );
        assert_eq!(
            asked.lock().unwrap()[0],
            (
                "scripts/main.py".to_owned(),
                vec!["net".to_owned(), "speak".to_owned()]
            )
        );
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
        // Nothing to try for a skill with no script.
        let light = "asks = [\"desk glow\"]\n[[do]]\nlight = { color = \"warm\" }\n";
        assert_eq!(
            tries.answer(&line(light, &[])),
            json!({"ok": true, "tried": false})
        );
    }

    #[test]
    fn a_silent_run_step_fails_its_try_with_words_for_the_repair() {
        let silent = Ran {
            code: Some(0),
            said: Some(0),
            ..Ran::default()
        };
        let (_work, tries, _) = tries(silent);
        let answered = tries.answer(&line(RUN_TOML, &["scripts/main.py"]));
        assert_eq!(answered["passed"], false, "{answered}");
        assert!(
            answered["why"].as_str().unwrap().contains("lamp.say"),
            "{answered}"
        );
    }

    /// A skill whose script sends mail is still tried (its token is a
    /// tried run's, whose writes lamp-control stubs): it is asked with its
    /// own write grant, never skipped.
    #[test]
    fn a_skill_that_sends_is_tried_with_its_own_grants() {
        let spoke = Ran {
            code: Some(0),
            said: Some(1),
            ..Ran::default()
        };
        let (_work, tries, asked) = tries(spoke);
        let answered = tries.answer(&line(SEND_TOML, &["scripts/main.py"]));
        assert_eq!(answered["passed"], true, "{answered}");
        assert_eq!(
            asked.lock().unwrap()[0].1,
            vec!["gmail-write".to_owned(), "speak".to_owned()]
        );
    }

    /// Tokens that mint a tried run's and count its says.
    struct TriedTokens {
        minted: Arc<Mutex<Vec<Vec<String>>>>,
    }

    impl Tokens for TriedTokens {
        fn mint(&self, _: &str, _: &str) -> Result<Minted, String> {
            Err("a skill's mint is never a try's".into())
        }
        fn end(&self, _: &str) {}
        fn mint_try(&self, grants: &[String], _: &str) -> Result<Minted, String> {
            self.minted.lock().unwrap().push(grants.to_vec());
            Ok(Minted {
                token: crate::skill_task::RunToken::new("t".repeat(43)),
                grants: grants.to_vec(),
            })
        }
        fn end_said(&self, _: &str) -> Option<u32> {
            Some(2)
        }
    }

    /// The fence's launch mints a tried run's token for the draft's own
    /// grants, refuses `net` without the lamp's egress (ending the token),
    /// and reads the run's say count at its end.
    #[test]
    fn the_fence_launch_mints_a_tried_token_and_reads_its_says() {
        let minted = Arc::new(Mutex::new(Vec::new()));
        let dir = tempfile::tempdir().unwrap();
        // A stand-in for lamp-tasks' fence: prints its grant's script.
        let program = dir.path().join("fence.sh");
        std::fs::write(
            &program,
            "#!/bin/sh\n[ \"$1\" = make-try-fence ] || exit 9\nread -r line\necho \"$line\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let launch = FenceLaunch {
            tokens: Box::new(TriedTokens {
                minted: minted.clone(),
            }),
            program,
            egress: None,
        };
        let skill = dir.path().join("skill");
        let ran = launch
            .launch(
                &skill,
                dir.path(),
                "scripts/main.py",
                &["speak".to_owned()],
                TRY_WITHIN,
            )
            .unwrap();
        assert_eq!(ran.code, Some(0));
        assert_eq!(ran.said, Some(2));
        assert!(ran.stdout.contains("scripts/main.py"), "{}", ran.stdout);
        assert_eq!(minted.lock().unwrap()[0], vec!["speak".to_owned()]);
        let net = launch.launch(
            &skill,
            dir.path(),
            "scripts/main.py",
            &["net".to_owned(), "speak".to_owned()],
            TRY_WITHIN,
        );
        assert!(net.unwrap_err().contains("LAMP_TASK_NET"));
    }

    #[test]
    fn a_fence_that_runs_past_its_limit_is_ended() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("slow.sh");
        std::fs::write(&program, "#!/bin/sh\nexec sleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let grant = crate::skill_start::Grant {
            script: "/x/scripts/main.py".into(),
            interpreter: "python3".into(),
            skill: "/x".into(),
            run: "/r".into(),
            socket: "/s".into(),
            pythonpath: "/p".into(),
            token: "t".into(),
            egress: None,
        };
        let began = Instant::now();
        let ran = run_fence(&program, &grant, Duration::from_millis(300)).unwrap();
        assert!(ran.timed_out);
        assert!(began.elapsed() < Duration::from_secs(10));
    }
}
