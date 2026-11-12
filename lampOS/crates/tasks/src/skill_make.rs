//! A voice make handed to the lamp's own Hermes in a skill run's namespace
//! (#6643, the first builder's guard, 12 Nov 18:15): the voice's make that
//! DeepSeek couldn't keep (`lamp_voice::make_skill_agent`,
//! `LAMP_MAKE_SKILL_AGENT`) is written by Hermes started exactly as a
//! skill's `do.task` run is ([`crate::skill_task::HermesRunner`],
//! row-6443's path), through its namespace wrapper: only its own folder,
//! its work folder and the skills, lamp-control's `control.sock` alone and
//! never `admin.sock`. Its terminal and tools work only inside that
//! namespace. No token is minted: a make has no grants.
//!
//! Its network ([`NET_SWITCH`], the owner's ask, 12 Nov): kept by default,
//! as V1's agent had it. `LAMP_MAKE_AGENT_NET=held` exactly gives the
//! strict run, [`crate::skill_task::NET_ENV`] `run`: a network namespace of
//! its own with only `lo` (and the kernel's `@NONE` tunnels), the model
//! only through [`crate::net_bridge::MODEL_SOCKET`]; refused when the
//! wrapper can't hold it off ([`crate::skill_task::NOT_HELD_OFF`]).
//!
//! - **The line** ([`MakeLine`]): `{"op":"skill_make","rules","within_ms"}`,
//!   the system prompt and the brief as one text, and how long the run may
//!   take, checked whole ([`MakeLine::parse`]).
//! - **The make's folder**: `make-<run id>` in the lamp's work folder (the
//!   one a skill's run starts in, shown to Hermes in its namespace), 0700,
//!   with the rules in [`RULES_FILE`], 0600. The run starts there and is
//!   told to write its answer to [`ANSWER_FILE`] there and nothing else.
//!   The answer is read back (a regular file, never a link, at most
//!   [`MOST_ANSWER_BYTES`]) once the run has ended and its whole process
//!   group is gone, and the folder removed however the make ends. The
//!   voice puts the answer through the same checks as DeepSeek's.
//! - **The answers**: `{"ok":true,"started":true}` once the run is about
//!   to start (so the voice says its waiting line only then), then
//!   `{"ok":true,"answer"}`; or one refusal: `off`, `bad_request`,
//!   `too_long`, `busy`, `not_held_off`, `run_failed` or `no_answer`.
//! - **The socket** ([`MakeHandOvers`]): with `LAMP_SKILL_TASKS` on (the
//!   switch lamp-update's namespace edits are written with), `serve` binds
//!   [`MAKE_SOCKET`] for lamp-voice alone, as root, one make at a time.
use crate::protocol::refusal;
use crate::skill_task::{NOT_HELD_OFF, Runner, SkillRun, read_line_within, run_id, write_answer};
use serde::Deserialize;
use serde_json::{Value, json};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

/// lamp-tasks' make socket, beside its others in its own runtime folder
/// (`lampos`'s, 0700), 0600: lamp-voice, run as root, reaches it as it does
/// `request.sock` (`LAMP_TASKS_DIR`).
pub const MAKE_SOCKET: &str = "/run/lampos-tasks/skill-make.sock";
/// The one unit whose lines are taken, run as root.
pub const CALLER_UNIT: &str = "lamp-voice.service";
/// The most bytes of the rules: the system prompt and the brief.
pub const MOST_RULES_BYTES: usize = 48 * 1024;
/// The most bytes of a line: the rules, every byte escaped at worst once.
pub const MOST_LINE_BYTES: usize = 2 * MOST_RULES_BYTES + 256;
/// The most bytes of an answer read back, as the voice's own model's.
pub const MOST_ANSWER_BYTES: u64 = 20 * 1024;
/// The least and most a run may be given (`within_ms`).
pub const LEAST_WITHIN: Duration = Duration::from_secs(1);
pub const MOST_WITHIN: Duration = Duration::from_secs(120);
/// The rules' file in a make's folder.
pub const RULES_FILE: &str = "rules.md";
/// The one file the run writes in a make's folder.
pub const ANSWER_FILE: &str = "answer.md";
/// How a make's folder's name begins.
pub const FOLDER_HEAD: &str = "make-";
/// Said when the run ended with no answer to read.
pub const NO_ANSWER: &str = "the lamp's Hermes wrote no answer";
/// What a second make is told while one runs.
pub const BUSY: &str = "a skill is already being made on this lamp; one is made at a time";
/// How long a connection has to send its line, and to take each answer.
pub const LINE_WAIT: Duration = Duration::from_secs(5);
/// The most connections read or answered at once.
pub const MOST_CONNECTIONS: usize = 2;
/// How often the socket is looked at while no one connects.
const ACCEPT_TICK: Duration = Duration::from_millis(20);

/// One make handed over by the voice, as its line says it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MakeLine {
    pub op: MakeOp,
    pub rules: String,
    pub within_ms: u64,
}

/// The line's `op`: `skill_make` only.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MakeOp {
    SkillMake,
}

impl MakeLine {
    /// The line, checked whole: the refusal's code and words otherwise.
    pub fn parse(line: &[u8]) -> Result<Self, (&'static str, String)> {
        if line.len() > MOST_LINE_BYTES {
            return Err(("too_long", "the line is too long".into()));
        }
        let make: Self = serde_json::from_slice(line)
            .map_err(|_| ("bad_request", "not a make's line".to_owned()))?;
        if make.rules.trim().is_empty() || make.rules.len() > MOST_RULES_BYTES {
            return Err(("bad_request", "the rules are empty or too long".into()));
        }
        if make.rules.contains('\0') {
            return Err(("bad_request", "the rules have a NUL".into()));
        }
        if !(LEAST_WITHIN..=MOST_WITHIN).contains(&make.within()) {
            return Err(("bad_request", "within_ms is out of its bounds".into()));
        }
        Ok(make)
    }

    pub fn within(&self) -> Duration {
        Duration::from_millis(self.within_ms)
    }
}

/// The run's words: where the rules are and the one file to write.
pub fn prompt(folder: &Path) -> String {
    format!(
        "Everything you need is in the file {rules} in this folder, {folder}. Read it and \
         answer exactly as it says: the fenced blocks, or its one line when the skill can't be \
         made. Write that answer, and nothing else, to the file {answer} in this folder. Write \
         no other file anywhere, install nothing and run nothing else.",
        rules = RULES_FILE,
        answer = ANSWER_FILE,
        folder = folder.display(),
    )
}

/// The run of a make in `folder`: its words, no environment of its own (no
/// token, no grant), at most `within`, started in its folder.
pub fn make_run(folder: &Path, within: Duration) -> SkillRun {
    SkillRun {
        // The make's own run id, from its folder's name (`make-<run id>`).
        run: folder
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix(FOLDER_HEAD))
            .unwrap_or_default()
            .into(),
        skill: "make".into(),
        prompt: prompt(folder),
        folder: folder.to_path_buf(),
        env: Vec::new(),
        deadline: within,
        start_in: Some(folder.to_path_buf()),
    }
}

/// A make's folder, removed however the make ends.
struct MadeFolder(PathBuf);

impl Drop for MadeFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The folder and its rules, made 0700 and 0600 before a byte is written.
fn put(work: &Path, rules: &str) -> std::io::Result<MadeFolder> {
    use std::io::Write;
    let folder = work.join(format!("{FOLDER_HEAD}{}", run_id()));
    std::fs::DirBuilder::new().mode(0o700).create(&folder)?;
    let made = MadeFolder(folder);
    std::fs::set_permissions(&made.0, std::fs::Permissions::from_mode(0o700))?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(made.0.join(RULES_FILE))?
        .write_all(rules.as_bytes())?;
    Ok(made)
}

/// The answer the run wrote: a regular file, never a link, of at most
/// [`MOST_ANSWER_BYTES`] of text.
fn answer_read(folder: &Path) -> Option<String> {
    use std::io::Read;
    let path = folder.join(ANSWER_FILE);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.file_type().is_file() || meta.len() > MOST_ANSWER_BYTES {
        return None;
    }
    let mut text = String::new();
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(&path)
        .ok()?
        .take(MOST_ANSWER_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    (!text.trim().is_empty() && text.len() as u64 <= MOST_ANSWER_BYTES).then_some(text)
}

/// lamp-tasks' answer to one make `line`: with the switch `on`, the line
/// checked, the run's namespace checked ([`Runner::ready`]), its folder
/// made in `work`, `started` told, the run made and waited for, its answer
/// read and the folder removed; else the refusal.
pub fn answer(
    line: &[u8],
    on: bool,
    work: &Path,
    runner: &dyn Runner,
    started: &mut dyn FnMut(),
) -> Value {
    if !on {
        return refusal(
            "off",
            "a skill run's held-off namespace is off on this lamp (LAMP_SKILL_TASKS)",
        );
    }
    let make = match MakeLine::parse(line) {
        Ok(make) => make,
        Err((code, why)) => return refusal(code, &why),
    };
    if let Err(why) = runner.ready() {
        return refusal("not_held_off", &why);
    }
    let folder = match put(work, &make.rules) {
        Ok(folder) => folder,
        Err(error) => {
            return refusal(
                "run_failed",
                &format!("the make's folder was not made: {error}"),
            );
        }
    };
    started();
    // A make has no token to end.
    let ran = runner.run(&make_run(&folder.0, make.within()), &|| {});
    match ran {
        Err(why) if why == NOT_HELD_OFF => refusal("not_held_off", &why),
        Err(why) => refusal("run_failed", &why),
        Ok(_) => match answer_read(&folder.0) {
            Some(text) => json!({"ok": true, "answer": text}),
            None => refusal("no_answer", NO_ANSWER),
        },
    }
}

/// Whether `caller` is [`CALLER_UNIT`] run as root; an unknown caller is
/// refused.
pub fn caller_may(caller: Option<&lamp_ipc::caller::Caller>) -> bool {
    caller.is_some_and(|caller| caller.uid == 0 && caller.unit.as_deref() == Some(CALLER_UNIT))
}

/// The switch for a make's network (the owner's ask, 12 Nov: the agent
/// keeps the network during a make, as V1's did; the first builder's
/// ruling pending on Queue #6059): exactly [`NET_HELD`] holds the run off
/// it ([`crate::skill_task::NET_ENV`] `run`: only `lo` and the kernel's
/// `@NONE` tunnels, the model through `model.sock`); unset or anything
/// else, the run keeps the lamp's network. Set in no unit.
pub const NET_SWITCH: &str = "LAMP_MAKE_AGENT_NET";
/// [`NET_SWITCH`]'s one value that holds the run off the network.
pub const NET_HELD: &str = "held";

/// Whether `value` of [`NET_SWITCH`] holds a make's run off the network:
/// [`NET_HELD`] exactly.
pub fn held(value: Option<&str>) -> bool {
    value == Some(NET_HELD)
}

/// The lamp's Hermes for a make ([`crate::skill_task::HermesRunner::lamp`]),
/// started through its namespace wrapper either way (only its own folder,
/// its work folder and the skills, `control.sock` alone of lamp-control's
/// and never `admin.sock`). `held`: with [`crate::skill_task::NET_ENV`]
/// `run`, refused when the wrapper can't hold it off. Not held: without
/// it, so the wrapper keeps the lamp's network, as Harness's own panes do
/// (the model then reached at lamp-cloud's forwarder, the one
/// `model.sock` pipes to).
pub fn lamp_runner(home: &Path, held: bool) -> crate::skill_task::HermesRunner {
    let mut runner = crate::skill_task::HermesRunner::lamp(home);
    if !held {
        runner
            .env
            .retain(|(name, _)| name != crate::skill_task::NET_ENV);
        runner.held_by = None;
    }
    runner
}

/// The voice's makes served: one line a connection, answered with
/// [`answer`], one make at a time (a second is refused `busy`).
pub struct MakeHandOvers {
    pub socket: PathBuf,
    /// The lamp's work folder, where each make's folder is made.
    pub work: PathBuf,
    pub runner: Arc<dyn Runner + Send + Sync>,
    pub caller: fn(Option<&lamp_ipc::caller::Caller>) -> bool,
}

impl MakeHandOvers {
    /// The lamp's: [`MAKE_SOCKET`], the lamp's Hermes in its namespace
    /// ([`lamp_runner`], the network as [`NET_SWITCH`] says) and its work
    /// folder, lamp-voice alone as caller.
    pub fn lamp(home: &Path) -> Self {
        let held = held(std::env::var(NET_SWITCH).ok().as_deref());
        eprintln!(
            "[lamp-tasks] skill make: a make's run {}",
            if held {
                "is held off the network (LAMP_MAKE_AGENT_NET=held)"
            } else {
                "keeps the network (LAMP_MAKE_AGENT_NET is not held)"
            }
        );
        let runner = lamp_runner(home, held);
        Self {
            socket: PathBuf::from(MAKE_SOCKET),
            work: runner.folder.clone(),
            runner: Arc::new(runner),
            caller: caller_may,
        }
    }

    /// With the switch `on`, the socket bound and served on a thread of its
    /// own until `stop`, the socket removed then; off, nothing is bound.
    pub fn start(
        self,
        on: bool,
        stop: Arc<AtomicBool>,
    ) -> std::io::Result<Option<std::thread::JoinHandle<()>>> {
        if !on {
            return Ok(None);
        }
        let (listener, binding) = crate::runtime::bind(&self.socket)?;
        let served = std::thread::Builder::new()
            .name("tasks-skill-make".into())
            .spawn(move || {
                let _binding = binding;
                self.accept(&listener, &stop);
            })?;
        Ok(Some(served))
    }

    fn accept(self, listener: &std::os::unix::net::UnixListener, stop: &AtomicBool) {
        let this = Arc::new(self);
        let running = Arc::new(AtomicBool::new(false));
        let open = Arc::new(AtomicUsize::new(0));
        while !stop.load(Ordering::Acquire) {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(ACCEPT_TICK);
                    continue;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    eprintln!("[lamp-tasks] skill make: accept: {error}");
                    std::thread::sleep(ACCEPT_TICK);
                    continue;
                }
            };
            let caller = lamp_ipc::caller::of_stream(&stream);
            if !(this.caller)(caller.as_ref()) {
                eprintln!(
                    "[lamp-tasks] skill make: refused a caller that is not {CALLER_UNIT} (pid {})",
                    caller.map_or_else(|| "unknown".into(), |caller| caller.pid.to_string())
                );
                continue;
            }
            if stream.set_nonblocking(false).is_err()
                || stream.set_read_timeout(Some(LINE_WAIT)).is_err()
                || stream.set_write_timeout(Some(LINE_WAIT)).is_err()
            {
                continue;
            }
            if open.fetch_add(1, Ordering::AcqRel) >= MOST_CONNECTIONS {
                open.fetch_sub(1, Ordering::AcqRel);
                write_answer(&stream, &refusal("busy", BUSY));
                continue;
            }
            let held = Counted(open.clone());
            let (served, running) = (this.clone(), running.clone());
            let _ = std::thread::Builder::new()
                .name("tasks-skill-make-run".into())
                .spawn(move || {
                    let _held = held;
                    served.one(&stream, &running);
                });
        }
    }

    fn one(&self, stream: &UnixStream, running: &AtomicBool) {
        let line = match read_line_within(stream, MOST_LINE_BYTES) {
            Ok(line) => line,
            Err(refused) => {
                write_answer(stream, &refused);
                return;
            }
        };
        if running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            eprintln!("[lamp-tasks] skill make: refused busy, one is being made");
            write_answer(stream, &refusal("busy", BUSY));
            return;
        }
        let place = Place(running);
        let answered = answer(&line, true, &self.work, &*self.runner, &mut || {
            write_answer(stream, &json!({"ok": true, "started": true}));
        });
        drop(place);
        // The outcome's code only: never the rules or the answer.
        match answered["ok"].as_bool() {
            Some(true) => eprintln!("[lamp-tasks] skill make: the run ended with an answer"),
            _ => eprintln!(
                "[lamp-tasks] skill make: refused {}",
                answered["error"].as_str().unwrap_or("?")
            ),
        }
        write_answer(stream, &answered);
    }
}

/// One of [`MOST_CONNECTIONS`] held while its thread lives.
struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// The one make's place, let go however it ends.
struct Place<'a>(&'a AtomicBool);

impl Drop for Place<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// One run as a test's runner saw it: its rules, folder, own
    /// environment and deadline.
    type SeenRun = (String, PathBuf, Vec<(&'static str, String)>, Duration);

    /// What a test's runner saw of a make's runs, kept by it.
    #[derive(Default)]
    struct Seen(Mutex<Vec<SeenRun>>);
    use crate::skill_task::{
        ADMIN_SOCKET, HERMES, HermesRunner, NET_ENV, NET_RUN, TOKEN_ENV, WRAPPER_NET_LINES,
    };
    use std::io::{BufRead, BufReader, Write};

    fn line(rules: &str, within_ms: u64) -> Vec<u8> {
        json!({"op":"skill_make","rules":rules,"within_ms":within_ms})
            .to_string()
            .into_bytes()
    }

    /// Writes `answer` (none: nothing) as the run would, and keeps what it saw.
    struct Writing {
        answer: Option<&'static str>,
        seen: Seen,
        ready: Result<(), String>,
    }

    impl Runner for Writing {
        fn ready(&self) -> Result<(), String> {
            self.ready.clone()
        }

        fn run(&self, run: &SkillRun, _: &dyn Fn()) -> Result<u64, String> {
            let folder = run.start_in.clone().unwrap();
            self.seen.0.lock().unwrap().push((
                std::fs::read_to_string(folder.join(RULES_FILE)).unwrap(),
                folder.clone(),
                run.env.clone(),
                run.deadline,
            ));
            if let Some(answer) = self.answer {
                std::fs::write(folder.join(ANSWER_FILE), answer).unwrap();
            }
            Ok(0)
        }
    }

    fn writing(answer: Option<&'static str>) -> Writing {
        Writing {
            answer,
            seen: Seen::default(),
            ready: Ok(()),
        }
    }

    #[test]
    fn a_make_runs_in_its_own_folder_and_its_answer_is_read_back() {
        let work = tempfile::tempdir().unwrap();
        let runner = writing(Some("```skill.toml\nx\n```"));
        let mut started = 0;
        let answer = answer(
            &line("SYSTEM\n\nBRIEF", 3_000),
            true,
            work.path(),
            &runner,
            &mut || started += 1,
        );
        assert_eq!(
            answer,
            json!({"ok": true, "answer": "```skill.toml\nx\n```"})
        );
        assert_eq!(started, 1);
        let seen = runner.seen.0.lock().unwrap();
        let (rules, folder, env, deadline) = &seen[0];
        assert_eq!(rules, "SYSTEM\n\nBRIEF");
        assert!(folder.starts_with(work.path()), "{folder:?}");
        // No token, no grant, no socket of its own: a make has none.
        assert!(env.is_empty(), "{env:?}");
        assert_eq!(*deadline, Duration::from_secs(3));
        // The folder is gone once the answer is in.
        assert!(!folder.exists());
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
    }

    /// A run that can't be held off the network starts nothing and makes
    /// no folder; one with no answer, or an answer that is a link, is
    /// `no_answer`; the switch off and a bad line refuse before anything.
    #[test]
    fn no_held_off_run_no_make_and_no_answer_is_refused() {
        let work = tempfile::tempdir().unwrap();
        let mut not_held = writing(Some("x"));
        not_held.ready = Err(NOT_HELD_OFF.into());
        let mut started = false;
        let refused = answer(&line("R", 2_000), true, work.path(), &not_held, &mut || {
            started = true
        });
        assert_eq!(refused["error"], "not_held_off");
        assert!(!started);
        assert!(not_held.seen.0.lock().unwrap().is_empty());
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
        let silent = writing(None);
        let refused = answer(&line("R", 2_000), true, work.path(), &silent, &mut || {});
        assert_eq!(refused["error"], "no_answer");
        let off = answer(&line("R", 2_000), false, work.path(), &silent, &mut || {});
        assert_eq!(off["error"], "off");
        for bad in [
            line("", 2_000),
            line("R", 10),
            line("R", 600_000),
            line(&"x".repeat(MOST_RULES_BYTES + 1), 2_000),
            br#"{"op":"skill_make","rules":"R","within_ms":2000,"net":"host"}"#.to_vec(),
            br#"{"op":"skill_task","rules":"R","within_ms":2000}"#.to_vec(),
        ] {
            let refused = answer(&bad, true, work.path(), &silent, &mut || {});
            assert_eq!(refused["ok"], false, "{refused}");
        }
        assert_eq!(silent.seen.0.lock().unwrap().len(), 1);
        let linked = tempfile::tempdir().unwrap();
        std::fs::write(linked.path().join("secret"), "SECRET").unwrap();
        assert_eq!(answer_read(linked.path()), None);
        std::os::unix::fs::symlink(
            linked.path().join("secret"),
            linked.path().join(ANSWER_FILE),
        )
        .unwrap();
        assert_eq!(answer_read(linked.path()), None, "never through a link");
    }

    /// The make's run is the lamp's Hermes exactly as a skill's run is
    /// started (row-6443's path): through its wrapper, which must hold it
    /// off the network, with `LAMP_NS_NET=run` and nothing of the admin
    /// socket or a token; started in its make's folder with the make's
    /// words. A wrapper without the network lines starts nothing.
    #[test]
    fn a_make_is_started_as_a_held_off_skill_run() {
        let home = tempfile::tempdir().unwrap();
        let lamp = MakeHandOvers::lamp(Path::new("/var/lib/lampos-tasks"));
        assert_eq!(
            lamp.work,
            Path::new("/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp")
        );
        let hermes = HermesRunner::lamp(home.path());
        assert_eq!(hermes.held_by.as_deref(), Some(Path::new(HERMES)));
        assert!(
            hermes
                .env
                .contains(&(NET_ENV.to_owned(), NET_RUN.to_owned()))
        );
        // As lamp-tasks starts it, with a stand-in for the wrapper that
        // keeps what it was given.
        let wrapper = home.path().join("hermes");
        let work = home.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let runner = HermesRunner {
            program: PathBuf::from("/bin/sh"),
            args: vec![
                "-c".into(),
                format!(
                    "pwd > {seen}/cwd; env > {seen}/env; printf '%s' \"$1\" > {seen}/words; \
                     cp {RULES_FILE} {seen}/rules; printf 'ANSWER' > {ANSWER_FILE}",
                    seen = home.path().display()
                ),
                "sh".into(),
            ],
            folder: work.clone(),
            env: hermes.env.clone(),
            tick: Duration::from_millis(10),
            held_by: Some(wrapper.clone()),
        };
        let mut started = false;
        let refused = answer(&line("RULES", 5_000), true, &work, &runner, &mut || {
            started = true
        });
        assert_eq!(refused["error"], "not_held_off", "no wrapper: {refused}");
        assert!(!started && !home.path().join("env").exists());
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nunset T\n{}{}--pid hermes-ns \"$@\"\n",
                WRAPPER_NET_LINES[0], WRAPPER_NET_LINES[1]
            ),
        )
        .unwrap();
        let answered = answer(&line("RULES", 5_000), true, &work, &runner, &mut || {});
        assert_eq!(answered, json!({"ok": true, "answer": "ANSWER"}));
        let env = std::fs::read_to_string(home.path().join("env")).unwrap();
        assert!(env.contains("LAMP_NS_NET=run"), "{env}");
        assert!(!env.contains(TOKEN_ENV), "{env}");
        assert!(!env.contains(ADMIN_SOCKET), "{env}");
        assert!(!env.contains("CARGO"), "{env}");
        let cwd = std::fs::read_to_string(home.path().join("cwd")).unwrap();
        assert!(
            Path::new(cwd.trim())
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(FOLDER_HEAD),
            "{cwd}"
        );
        let words = std::fs::read_to_string(home.path().join("words")).unwrap();
        assert!(
            words.contains(ANSWER_FILE) && words.contains(RULES_FILE),
            "{words}"
        );
        assert_eq!(
            std::fs::read_to_string(home.path().join("rules")).unwrap(),
            "RULES"
        );
        assert_eq!(
            std::fs::read_dir(&work).unwrap().count(),
            0,
            "folder removed"
        );
        // The namespace it is held in shows control.sock alone and never
        // names admin.sock in what it runs: lamp-update's own text.
        let source = include_str!("../../update/src/hermes_install.rs");
        let tasks_edits = source
            .split("const TASKS_EDITS")
            .nth(1)
            .and_then(|rest| rest.split("pub fn ns_text").next())
            .unwrap();
        let script: String = tasks_edits
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        assert!(script.contains("L=/run/lampos-control/control.sock"));
        assert!(!script.contains("admin.sock"));
        assert!(script.contains("@NONE"));
    }

    /// The owner's ask (12 Nov): by default a make's run keeps the network,
    /// still in Hermes's namespace, so `control.sock` alone and never
    /// `admin.sock`; `LAMP_MAKE_AGENT_NET=held` exactly gives the strict
    /// held-off run, `LAMP_NS_NET=run`.
    #[test]
    fn the_default_keeps_the_network_and_held_holds_it_off() {
        for value in [None, Some(""), Some("HELD"), Some("on"), Some("run")] {
            assert!(!held(value), "{value:?}");
        }
        assert!(held(Some("held")));
        let home = Path::new("/var/lib/lampos-tasks");
        let kept = lamp_runner(home, false);
        assert_eq!(kept.program, Path::new(HERMES), "through its namespace");
        assert!(
            kept.env.iter().all(|(name, _)| name != NET_ENV),
            "{:?}",
            kept.env
        );
        assert!(
            kept.env
                .iter()
                .all(|(name, value)| !value.contains("admin.sock") && name != TOKEN_ENV)
        );
        assert_eq!(kept.ready(), Ok(()));
        let strict = lamp_runner(home, true);
        assert_eq!(strict.program, Path::new(HERMES));
        assert!(
            strict
                .env
                .contains(&(NET_ENV.to_owned(), NET_RUN.to_owned()))
        );
        assert_eq!(strict.held_by.as_deref(), Some(Path::new(HERMES)));
        // A make's run itself carries no socket or token, either way.
        let run = make_run(Path::new("/tmp/make-x"), Duration::from_secs(5));
        assert!(run.env.is_empty());
        // Either way the namespace shows control.sock alone, never the
        // admin socket: its bind is the file, not the folder, and not
        // only with LAMP_NS_NET=run.
        let source = include_str!("../../update/src/hermes_install.rs");
        assert!(source.contains(
            "if [ -S \\\"$L\\\" ]; then touch \\\"$S/l\\\" && mount --bind \\\"$L\\\" \\\"$S/l\\\""
        ));
        // The kept run reaches the model at lamp-cloud's forwarder, the one
        // model.sock pipes to.
        assert!(source.contains("pub const RELAY_LISTEN: &str = \"127.0.0.1:7741\";"));
    }

    #[test]
    fn only_lamp_voice_as_root_may_hand_a_make_over() {
        use lamp_ipc::caller::Caller;
        let caller = |uid, unit: Option<&str>| Caller {
            pid: 10,
            uid,
            unit: unit.map(str::to_owned),
        };
        assert!(caller_may(Some(&caller(0, Some(CALLER_UNIT)))));
        assert!(!caller_may(Some(&caller(994, Some(CALLER_UNIT)))));
        assert!(!caller_may(Some(&caller(0, Some("lamp-skills.service")))));
        assert!(!caller_may(Some(&caller(0, None))));
        assert!(!caller_may(None));
    }

    #[test]
    fn the_socket_says_started_then_the_answer_and_one_make_at_a_time() {
        let dir = tempfile::Builder::new()
            .prefix("tmk-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let socket = dir.path().join("skill-make.sock");
        let work = dir.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = MakeHandOvers {
            socket: socket.clone(),
            work,
            runner: Arc::new(writing(Some("A"))),
            caller: |_| true,
        }
        .start(true, stop.clone())
        .unwrap()
        .expect("on binds");
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream.write_all(&line("R", 2_000)).unwrap();
        stream.write_all(b"\n").unwrap();
        let mut lines = BufReader::new(stream).lines();
        let first: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(first, json!({"ok": true, "started": true}));
        let second: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(second, json!({"ok": true, "answer": "A"}));
        stop.store(true, Ordering::Release);
        handle.join().unwrap();
        assert!(!socket.exists());
        let off = MakeHandOvers {
            socket: socket.clone(),
            work: dir.path().into(),
            runner: Arc::new(writing(None)),
            caller: |_| true,
        }
        .start(false, Arc::default())
        .unwrap();
        assert!(off.is_none() && !socket.exists());
    }
}
