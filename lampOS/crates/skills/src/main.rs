//! `lamp-skills`: the physical skills part (`docs/skills.md`). Loads every
//! skill folder, reloads them when one changes, listens to the topics their
//! triggers name, and runs their steps through the body's sockets, the
//! actions part and the voice's say service.
use lamp_bus::{Bus, Declaration, Kind, Publisher, Subscriber};
use lamp_msgs::{
    BodyGesture, Decision, GestureState, Hand, Light, LightOutranked, LightReached, LightRequest,
    MessageOut, Pause, SkillEvent, SkillStage, Speech, Voice, VoiceState,
};
use lamp_skills::glow::{GLOW_EVERY, Glow, Glows, REQUESTS_ENVIRONMENT};
use lamp_skills::routine::Permit;
use lamp_skills::watch::{self, Out, Outs, Watches};
use lamp_skills::{
    Admission, BODY_REQUESTS_ENVIRONMENT, CLIP_GAP_US, Call, Engine, Permissions, Run, body_ask,
    clip_us, clock, duration_us, load_all, package, run_tests, told, tools_file,
};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::{
        fs::FileTypeExt,
        net::{UnixDatagram, UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant, SystemTime},
};

const USAGE: &str = "usage: lamp-skills [--dir DIR] [--bus-root DIR] [--gaze-socket PATH] \
                     [--mood-socket PATH] [--actions-socket PATH] [--say-socket PATH] \
                     [--tools-dir DIR] [--state-dir DIR] [--boot-id ID] [--clock-control PATH] \
                     [--known-file PATH] [--daily-state PATH] (or LAMP_SKILLS_DAILY_FILE)\n       \
                     [--zone system|NAME | --utc-offset-minutes N] [--trust-clock]\n       \
                     lamp-skills test SKILL_DIR...\n       \
                     lamp-skills install [--dir DIR] SKILL_DIR|PACKAGE.tar\n       \
                     lamp-skills remove [--dir DIR] NAME\n       \
                     lamp-skills list [--dir DIR]\n       \
                     lamp-skills pack [--out DIR] SKILL_DIR\n       \
                     lamp-skills asked [--dir DIR]";
/// The daily journal's file, as the product unit passes it; `--daily-state`
/// wins when both are given.
const DAILY_ENVIRONMENT: &str = "LAMP_SKILLS_DAILY_FILE";
/// Where the product keeps its skills (`deploy/product/lamp-skills.service`):
/// the core ones `lamp-update install` writes and the installable ones.
const SKILLS_DIR: &str = "/var/lib/lampOS/skills";
/// How often the folder is looked at for a change.
const RELOAD_EVERY: Duration = Duration::from_secs(2);
const ACTIONS_TIMEOUT: Duration = Duration::from_secs(2);
/// The longest the voice may take between two lines of an answer.
const SAY_TIMEOUT: Duration = Duration::from_secs(180);
/// The longest a tool call waits for the main loop, and for the voice's
/// line: the voice gives up on a call after 1.5 s.
const TOOL_TIMEOUT: Duration = Duration::from_secs(1);
/// The longest a call with a look waits for its body and light to be made
/// before it is answered (#1146): within `TOOL_TIMEOUT`, which the caller
/// waits again once the answer is committed.
const MADE_BEFORE_ANSWER: Duration = Duration::from_millis(800);
/// How often a held pose is said again: the body lets one go after 1.5 s.
const MOOD_EVERY: Duration = Duration::from_secs(1);

/// Each skill's pose held now, by the number of its hold: a skill's newer
/// pose takes the place of its older one, and a call again with no pose
/// ends it ([`end_hold`]), so two holds of one skill never take turns at
/// the head (#1679: a scene changed, or the light turned to another place).
static HOLDS: Mutex<std::collections::BTreeMap<String, u64>> =
    Mutex::new(std::collections::BTreeMap::new());

/// A new hold of `skill`'s pose: its number, which ends every older one.
fn new_hold(skill: &str) -> u64 {
    let mut holds = HOLDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let number = holds.get(skill).map_or(1, |last| last + 1);
    holds.insert(skill.to_owned(), number);
    number
}

/// Whether `skill`'s hold `number` is still its newest.
fn holding(skill: &str, number: u64) -> bool {
    HOLDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(skill)
        == Some(&number)
}

/// Ends `skill`'s hold: the body comes back 1.5 s after the pose was last said.
fn end_hold(skill: &str) {
    new_hold(skill);
}
/// How long an ask waits for the Policy: its own 2 s from the receipt
/// (#958), and a little for the bus each way.
const ASK_WAIT: Duration = Duration::from_millis(2_500);
/// The longest a tool's run waits for the voice to begin the model's line,
/// for steps after the say.
const SPEAK_BEGIN: Duration = Duration::from_secs(5);
/// The longest a tool's line is waited for once the voice has stopped
/// speaking but not ended its turn (`thinking`).
const LINE_LONGEST: Duration = Duration::from_secs(4);
/// How often the voice's state, the hand and the pause are read while a run
/// waits on them.
const POLL: Duration = Duration::from_millis(20);
/// The most runs of a list's items that wait for a skill's run under way:
/// as many tasks as the tasks part keeps.
const MOST_WAITING: usize = 16;

#[derive(Clone)]
struct Sockets {
    gaze: PathBuf,
    mood: PathBuf,
    actions: PathBuf,
    say: PathBuf,
    /// Whose run these are for, and what that skill may use: checked where
    /// each call is sent. The program's own, before a run, may use nothing.
    may: May,
}

/// A run's skill and its `[permissions]`, checked where each call leaves
/// the program: the body's sockets, the ring, the actions part, the say
/// service and a tool's look. A skill that loaded makes only what it
/// declared; this holds that whatever made the call.
#[derive(Clone, Default)]
struct May {
    skill: String,
    permissions: Permissions,
}

impl May {
    /// Why `call` is not made, said in the journal too, if it is not.
    fn refuses(&self, call: &Call) -> Option<String> {
        let why = self.permissions.refuses(call)?;
        Some(self.refused(call.named().0, why))
    }

    /// Why the actions part's `name` is not called, said in the journal
    /// too, if it is not.
    fn refuses_action(&self, name: &str) -> Option<String> {
        let why = self.permissions.refuses_action(name)?;
        Some(self.refused(name, why))
    }

    fn refused(&self, what: &str, why: String) -> String {
        eprintln!("lamp-skills: skill:{}: refused {what}: {why}", self.skill);
        format!("{what}: refused, {why}")
    }
}

fn main() -> ExitCode {
    let given: Vec<String> = std::env::args().skip(1).collect();
    match given.first().map(String::as_str) {
        Some("test") => return test(&given[1..]),
        Some(command @ ("install" | "remove" | "list" | "pack" | "asked")) => {
            return manage(command, &given[1..]);
        }
        _ => {}
    }
    let mut dir = PathBuf::from(SKILLS_DIR);
    let mut root = None;
    let mut tools = None;
    let mut state = None;
    let mut boot_id = None;
    let mut clock_control = None;
    let mut known_file = None;
    let mut daily_state = None;
    let mut trust_clock = false;
    let mut offset = None;
    let mut zone = None;
    let mut sockets = Sockets {
        gaze: "/run/lampos-body/gaze.sock".into(),
        mood: "/run/lampos-body/mood.sock".into(),
        actions: "/run/lampos-actions/call.sock".into(),
        say: "/run/lampos-say/say.sock".into(),
        may: May::default(),
    };
    let mut arguments = given.iter();
    while let Some(flag) = arguments.next() {
        if flag == "--trust-clock" && !trust_clock {
            trust_clock = true;
            continue;
        }
        let Some(value) = arguments.next() else {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        };
        if flag == "--utc-offset-minutes" {
            // The lamp's local time east of UTC, as the Policy and
            // Expression take it: a skill's quiet hours need it. Once, as
            // memory takes it: a second would silently replace the first.
            if offset.is_some() {
                eprintln!("--utc-offset-minutes is given once");
                return ExitCode::from(2);
            }
            match value.parse::<i32>() {
                Ok(minutes) if (-720..=840).contains(&minutes) => offset = Some(minutes),
                _ => {
                    eprintln!("--utc-offset-minutes is a whole number from -720 to 840");
                    return ExitCode::from(2);
                }
            }
            continue;
        }
        if flag == "--zone" {
            // The lamp's zone, `system` for /etc/localtime (#1384). Once.
            if zone.is_some() {
                eprintln!("--zone is given once");
                return ExitCode::from(2);
            }
            if value.is_empty() || value.chars().any(char::is_control) {
                eprintln!("--zone is system or a time zone's name");
                return ExitCode::from(2);
            }
            zone = Some(value.clone());
            continue;
        }
        let value = PathBuf::from(value);
        match flag.as_str() {
            "--dir" => dir = value,
            "--bus-root" => root = Some(value),
            "--gaze-socket" => sockets.gaze = value,
            "--mood-socket" => sockets.mood = value,
            "--actions-socket" => sockets.actions = value,
            "--say-socket" => sockets.say = value,
            "--tools-dir" => tools = Some(value),
            "--state-dir" => state = Some(value),
            "--boot-id" => boot_id = value.to_str().map(str::to_owned),
            "--clock-control" => clock_control = Some(value),
            "--known-file" => known_file = Some(value),
            "--daily-state" if daily_state.is_none() => daily_state = Some(value),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    // One or the other; a zone that can't be read ends the start, never a
    // silent UTC, as in memory, Expression and the Policy.
    let local_time = match (zone, offset) {
        (Some(_), Some(_)) => {
            eprintln!("--zone and --utc-offset-minutes are one or the other");
            return ExitCode::from(2);
        }
        (Some(zone), None) => match lamp_local_time::LocalTime::zone(&zone) {
            Ok(local) => Some(Arc::new(local)),
            Err(error) => {
                eprintln!("lamp-skills: {error}");
                return ExitCode::from(2);
            }
        },
        (None, Some(minutes)) => Some(Arc::new(lamp_local_time::LocalTime::Offset(minutes))),
        (None, None) => None,
    };
    // The product unit gives the daily journal as an `Environment=` line, which
    // a lamp-skills built before it ignores, where an unknown `--daily-state`
    // would end that older binary at its start (#2157). The flag wins.
    let daily_state = daily_state.or_else(|| {
        std::env::var_os(DAILY_ENVIRONMENT)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    let ask = std::env::var("LAMP_SKILLS_ASK").unwrap_or_default();
    let options = Options {
        tools,
        state,
        ask,
        light_requests: std::env::var(REQUESTS_ENVIRONMENT).is_ok_and(|value| value == "on"),
        body_requests: std::env::var(BODY_REQUESTS_ENVIRONMENT).is_ok_and(|value| value == "on"),
        boot_id,
        clock_control,
        daily_state,
        trust_clock,
        routines: std::env::var(lamp_skills::routine::ENVIRONMENT).is_ok_and(|value| value == "on"),
        local_time,
        known_file,
    };
    match serve(&dir, root, &sockets, &options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-skills: {error}");
            ExitCode::FAILURE
        }
    }
}

fn test(dirs: &[String]) -> ExitCode {
    if dirs.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let mut failed = false;
    for dir in dirs {
        match run_tests(Path::new(dir)) {
            Ok((passed, unrecorded)) => {
                println!("{dir}: {passed} cases pass, {unrecorded} have no recording yet")
            }
            Err(why) => {
                println!("{dir}: {why}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// `install`, `remove`, `list` and `pack`: an installable skill put on or
/// taken off this lamp's skills folder (`--dir`, the product's by default),
/// which the running program loads or lets go within 2 s, with nothing
/// restarted; the folder listed; a skill packed for the Store (`--out`, the
/// working folder by default). What was done goes to the standard output,
/// a refusal and why to the standard error.
fn manage(command: &str, given: &[String]) -> ExitCode {
    let mut dir = PathBuf::from(SKILLS_DIR);
    let mut out = PathBuf::from(".");
    let mut named = Vec::new();
    let mut given = given.iter();
    while let Some(word) = given.next() {
        let flag = match word.as_str() {
            "--dir" if command != "pack" => &mut dir,
            "--out" if command == "pack" => &mut out,
            flag if flag.starts_with("--") => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
            word => {
                named.push(word.to_owned());
                continue;
            }
        };
        let Some(value) = given.next() else {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        };
        *flag = value.into();
    }
    let done = match (command, named.as_slice()) {
        ("install", [from]) => package::install(&dir, Path::new(from)),
        ("remove", [name]) => package::remove(&dir, name),
        ("list", []) => package::list(&dir).map(|listed| listed.trim_end().to_owned()),
        // The manage helper (#1722): one request on the standard input, its
        // answer on the standard output, as systemd hands it a connection
        // to `lamp-skills-manage.socket`; the journal says what was done.
        ("asked", []) => {
            return match package::asked(&dir, std::io::stdin(), std::io::stdout()) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => ExitCode::FAILURE,
                Err(error) => {
                    eprintln!("lamp-skills: asked: not answered: {error}");
                    ExitCode::FAILURE
                }
            };
        }
        ("pack", [skill]) => {
            package::pack(Path::new(skill), &out).map(|path| format!("packed {}", path.display()))
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match done {
        Ok(said) => {
            println!("{said}");
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("lamp-skills: {why}");
            ExitCode::FAILURE
        }
    }
}

/// What the folder holds, to see that it changed: every file but the tests,
/// with its size and when it was written.
fn fingerprint(dir: &Path) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
    let mut found = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            // A Mac's `._NAME` files are not the skill's: no reload for them.
            if lamp_skills::hidden(&path) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if entry.file_name() != "tests" {
                    folders.push(path);
                }
            } else {
                found.push((path, meta.len(), meta.modified().ok()));
            }
        }
    }
    found.sort();
    found
}

fn publish(out: &mut Publisher, mut event: SkillEvent) {
    if event.why.len() > 2_000 {
        let mut end = 2_000;
        while !event.why.is_char_boundary(end) {
            end -= 1;
        }
        event.why.truncate(end);
    }
    eprintln!(
        "lamp-skills: {} {:?}: {}",
        event.skill, event.stage, event.why
    );
    // Stamped on the host's clock, as every part stamps the bus.
    if let Err(error) = out.publish_json(lamp_ipc::monotonic_us(), &event) {
        eprintln!("lamp-skills: skills/events not published: {error}");
    }
}

/// The light shown now, if the ring's owner has said within 3 s (as the
/// actions part counts it).
fn light_now(bus: &Bus) -> Option<Light> {
    said_lately(bus, Light::TOPICS[1])
}

/// The newest value of `topic`, if it was said within 3 s: on the host's
/// wall clock, as its publisher stamped it, never the program's own.
fn said_lately<T: serde::de::DeserializeOwned>(bus: &Bus, topic: &str) -> Option<T> {
    let message = bus.latest(topic).ok()??;
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    (now_ms.saturating_sub(message.header.at_unix_ms) <= 3_000)
        .then(|| message.parse().ok())
        .flatten()
}

/// How the program serves, beyond its sockets.
struct Options {
    tools: Option<PathBuf>,
    /// `--state-dir`: the part's own folder, where the skills a person
    /// turned off are kept across a restart (#1685); without it, until the
    /// program stops.
    state: Option<PathBuf>,
    /// `LAMP_SKILLS_ASK`: `skip`, `policy`, or anything else for nothing done.
    ask: String,
    routines: bool,
    /// `LAMP_LIGHT_REQUESTS=on`: a skill's light is asked of the ring.
    light_requests: bool,
    /// `LAMP_BODY_REQUESTS=on`: a skill's gestures and moves say who asks,
    /// for a body with `--arbitration on` (#940).
    body_requests: bool,
    /// `--boot-id`: the boot a light request's claim is on, in place of the
    /// host's own, which only Linux has.
    boot_id: Option<String>,
    /// `--clock-control`: a test's socket to skip the program's clock
    /// ([`clock_control`]).
    clock_control: Option<PathBuf>,
    /// Durable consumption of named daily chances. Optional: without it,
    /// daily triggers fail quiet while ordinary skills keep working.
    daily_state: Option<PathBuf>,
    /// Explicit development override; production scheduled work requires
    /// positive kernel synchronization evidence.
    trust_clock: bool,
    /// `--zone` or `--utc-offset-minutes`: the lamp's local time, for quiet
    /// hours.
    local_time: Option<Arc<lamp_local_time::LocalTime>>,
    /// `--known-file`: the eyes' file with the name of the known person at
    /// the lamp (`lamp-eyes --known-file`), for a watch to tell a known face
    /// from one it does not know, and the person who asked from anyone else.
    known_file: Option<PathBuf>,
}

/// A skip of the clock asked on `--clock-control`, and where to say it is
/// done.
type Skip = (Duration, mpsc::Sender<Result<(), String>>);

/// The longest the main loop waits for a message, a call, a skip or a run's
/// end before it looks at the skills folder, the held runs and every topic's
/// `latest` again. A turn of its old round of the topics took as long.
const WAIT_AT_MOST: Duration = Duration::from_millis(250);

/// A sender to the main loop that wakes its wait ([`wait_any`]) with each
/// send, so a tool call, a skip or a run's end is taken at once, not after
/// the wait runs out. Before #1806 a tool call waited out a topic's turn,
/// about 50 ms each on a lamp's skills.
struct Waking<T> {
    sender: mpsc::Sender<T>,
    waker: Arc<std::os::fd::OwnedFd>,
}

impl<T> Clone for Waking<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            waker: self.waker.clone(),
        }
    }
}

impl<T> Waking<T> {
    fn send(&self, value: T) -> Result<(), mpsc::SendError<T>> {
        self.sender.send(value)?;
        // A full pipe already wakes the wait; a write that fails leaves it
        // to its time.
        let _ = rustix::io::write(&*self.waker, &[1]);
        Ok(())
    }
}

/// A channel whose sends wake the main loop through `waker`.
fn waking<T>(waker: &Arc<std::os::fd::OwnedFd>) -> (Waking<T>, mpsc::Receiver<T>) {
    let (sender, receiver) = mpsc::channel();
    (
        Waking {
            sender,
            waker: waker.clone(),
        },
        receiver,
    )
}

/// The main loop's wake: read end and write end, neither ever blocking.
/// Made with `pipe` and `fcntl`, as the voice's controls make theirs, since
/// `pipe2` is not on macOS.
fn wake_pipe() -> io::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    let (read, write) = rustix::pipe::pipe()?;
    for end in [&read, &write] {
        rustix::io::fcntl_setfd(end, rustix::io::FdFlags::CLOEXEC)?;
        rustix::fs::fcntl_setfl(end, rustix::fs::OFlags::NONBLOCK)?;
    }
    Ok((read, write))
}

/// Until a topic's or the Policy's socket is readable, the wake is written,
/// or `wait` passes; at once if one already holds a datagram read ahead.
/// The wake is emptied. A socket that says an error or hang-up with nothing
/// to read makes it wait out `wait` instead, so the loop cannot spin.
fn wait_any(
    subscribers: &[Subscriber],
    answers: Option<&Subscriber>,
    woken: &std::os::fd::OwnedFd,
    wait: Duration,
) {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let mut drained = [0u8; 64];
    if !subscribers.iter().chain(answers).any(Subscriber::holds) {
        let mut fds: Vec<PollFd<'_>> = subscribers
            .iter()
            .chain(answers)
            .map(|subscriber| PollFd::new(subscriber, PollFlags::IN))
            .collect();
        fds.push(PollFd::new(woken, PollFlags::IN));
        let timeout = Timespec::try_from(wait).unwrap_or(Timespec {
            tv_sec: 0,
            tv_nsec: 250_000_000,
        });
        let broken = match poll(&mut fds, Some(&timeout)) {
            Ok(_) => fds.iter().any(|fd| {
                let flags = fd.revents();
                !flags.contains(PollFlags::IN)
                    && flags.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL)
            }),
            Err(rustix::io::Errno::INTR) => false,
            Err(_) => true,
        };
        if broken {
            std::thread::sleep(wait);
        }
    }
    while matches!(rustix::io::read(woken, &mut drained), Ok(read) if read > 0) {}
}

/// Listens on `path` for a test: each line `{"skip_us": N}` skips the
/// program's clock N µs, which the main loop does, and is answered with
/// `{"now_us": ...}` once it is done, or `{"error": ...}`. Production has
/// no such socket.
fn clock_control(path: &Path, skips: Waking<Skip>) -> io::Result<()> {
    // An old socket of its own is taken over; anything else at `path` is
    // left, and the bind fails on it.
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_socket()) {
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    clock::stepped();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut lines = BufReader::new((&stream).take(8_192));
            let mut line = String::new();
            // Up to 8 KiB a line.
            while {
                lines.get_mut().set_limit(8_192);
                lines.read_line(&mut line).is_ok_and(|read| read > 0)
            } {
                let by = serde_json::from_str::<serde_json::Value>(&line)
                    .ok()
                    .and_then(|asked| asked["skip_us"].as_u64());
                line.clear();
                let answer = match by {
                    Some(by) => {
                        let (done, taken) = mpsc::channel();
                        if skips.send((Duration::from_micros(by), done)).is_err() {
                            return;
                        }
                        match taken.recv() {
                            Ok(Ok(())) => json!({ "now_us": clock::now_us() }),
                            Ok(Err(why)) => json!({ "error": why }),
                            Err(_) => return,
                        }
                    }
                    None => json!({ "error": "a line is {\"skip_us\": N}" }),
                };
                if writeln!(&stream, "{answer}").is_err() {
                    break;
                }
            }
        }
    });
    Ok(())
}

/// The skips a test asked for: what was published before each is taken in
/// first, on the clock as it was, and the Policy's answers with it; then the
/// clock moves on, and an ask it made late is given up, before the skip is
/// answered.
fn take_skips(
    skips: &mpsc::Receiver<Skip>,
    subscribers: &mut [Subscriber],
    engine: &mut Engine,
    runs: &mut Runs,
    out: &mut Publisher,
    runner: &Runner,
    asking: &mut Asking,
) {
    while let Ok((by, done)) = skips.try_recv() {
        for subscriber in subscribers.iter_mut() {
            while let Some(message) = receive(subscriber, Duration::ZERO) {
                observe(&message, engine, runs, out, runner, asking);
            }
        }
        if let Some((_, answers)) = &mut asking.gate {
            answer_asks(answers, &mut asking.waiting, engine, runs, out, runner);
        }
        let skipped = clock::skip(by);
        // The asks now late are given up before any answer that came since.
        give_up_late(&mut asking.waiting, out);
        if let Some((_, answers)) = &mut asking.gate {
            answer_asks(answers, &mut asking.waiting, engine, runs, out, runner);
        }
        let _ = done.send(skipped);
    }
}

/// The boot the claims are on: `given`, or the host's on Linux.
fn boot_clock(given: Option<&str>) -> io::Result<String> {
    match given {
        Some(id) => Ok(id.to_owned()),
        None if cfg!(target_os = "linux") => {
            Ok(std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
                .trim()
                .to_owned())
        }
        None => Err(io::Error::other(
            "with LAMP_LIGHT_REQUESTS=on, supply --boot-id on this development host",
        )),
    }
}

fn serve(
    dir: &Path,
    root: Option<PathBuf>,
    sockets: &Sockets,
    options: &Options,
) -> io::Result<()> {
    let tools = options.tools.as_deref();
    let (skip_ask, ask_policy) = (options.ask == "skip", options.ask == "policy");
    let bus = match root {
        Some(root) => Bus::new(root)?,
        None => Bus::from_environment()?,
    };
    let mut out =
        bus.publisher_declared(SkillEvent::TOPIC, &SkillEvent::declaration(), "skills")?;
    let glows = if options.light_requests {
        Some(Glows {
            bus: bus.clone(),
            clock_id: boot_clock(options.boot_id.as_deref())?,
            out: Arc::new(Mutex::new(bus.publisher_declared(
                LightRequest::TOPIC,
                &LightRequest::declaration(),
                "skills",
            )?)),
            every: GLOW_EVERY,
            pulses: Arc::default(),
        })
    } else {
        None
    };
    let (woken, waker) = wake_pipe()?;
    let waker = Arc::new(waker);
    let (finished, done) = waking::<SkillEvent>(&waker);
    let limits =
        if options.routines && std::env::var("LAMP_OUTPUT_CAPS").is_ok_and(|value| value == "on") {
            let local = options
                .local_time
                .clone()
                .ok_or_else(|| io::Error::other("skill output limits require a local zone"))?;
            Some(lamp_skills::limits::Client::start_with_clock_trust(
                sockets.actions.clone(),
                local,
                options.trust_clock,
            )?)
        } else {
            None
        };
    let (mut routines, rest_client) = if options.routines {
        let (manager, client) = lamp_skills::routine::Manager::new(
            bus.clone(),
            boot_clock(options.boot_id.as_deref())?,
        )?;
        (Some(manager), Some(client))
    } else {
        (None, None)
    };
    let mut engine = Engine::new(Vec::new(), skip_ask);
    // Who was here's mark of someone the lamp knows, never a name, only
    // with LAMP_WHO_WAS_HERE_KNOWN=on and the eyes' known file (#1685).
    if lamp_skills::known::marking() {
        match &options.known_file {
            Some(path) => {
                eprintln!(
                    "lamp-skills: Who was here may say someone the lamp knows was here, never who ({})",
                    lamp_skills::known::ENVIRONMENT
                );
                engine = engine.marking_known(path.clone());
                if lamp_skills::known::naming() {
                    eprintln!(
                        "lamp-skills: Who was here may name someone who said yes to being named ({})",
                        lamp_skills::known::NAMES_ENVIRONMENT
                    );
                    engine = engine.naming_who_said_yes();
                }
            }
            None => eprintln!(
                "lamp-skills: {} is on without --known-file, so Who was here marks nobody",
                lamp_skills::known::ENVIRONMENT
            ),
        }
    }
    if let Some(state) = &options.state {
        match engine.keep_off_in(state) {
            Ok(off) => eprintln!(
                "lamp-skills: the skills a person turned off kept in {}: {off:?}",
                state.display()
            ),
            Err(error) => eprintln!("lamp-skills: the skills turned off not read: {error}"),
        }
    }
    if let Some(path) = &options.daily_state {
        match lamp_skills::daily::Journal::open(path) {
            Ok(journal) => engine = engine.with_daily_journal(journal),
            Err(error) => eprintln!("lamp-skills: daily chances disabled: {error}"),
        }
    }
    // The Policy's gate (#958): asks out on `skills/ask`, its answers in on
    // `policy/decisions`, by the ask's number. The ask's wire is #958's
    // `lamp_msgs::SkillAsk`, written here as JSON until it is on lamp-voice.
    let mut gate = None;
    if ask_policy {
        engine = engine.asking_the_policy();
        gate = Some((
            bus.publisher_declared(
                "skills/ask",
                &Declaration::new("lamp_msgs/SkillAsk", 1, Kind::Event),
                "skills",
            )?,
            bus.subscribe(Decision::TOPIC, "skills")?,
        ));
    }
    // Runs waiting for the Policy's answer, by the ask's number.
    let mut asking = Asking {
        gate,
        waiting: HashMap::new(),
        last: 0,
    };
    let runner = Runner {
        sockets: sockets.clone(),
        finished,
        bus: bus.clone(),
        glows,
        body_requests: options.body_requests,
        ends: RefCell::new(HashMap::new()),
        local_time: options.local_time.clone(),
        trust_clock: options.trust_clock,
        clock_waiting: Cell::new(false),
        rest_client,
        permissions: RefCell::new(HashMap::new()),
        looks: RefCell::new(HashMap::new()),
        watching: RefCell::new(Watching {
            watches: Watches::default(),
            marks: HashMap::new(),
            texts: None,
            known_file: options.known_file.clone(),
            signs: None,
            retaken: HashMap::new(),
        }),
    };
    let (ask, asks) = waking::<ToolAsk>(&waker);
    if let Some(tools) = tools {
        listen(tools, ask.clone())?;
    }
    let (skip, skips) = waking::<Skip>(&waker);
    if let Some(path) = &options.clock_control {
        clock_control(path, skip)?;
    }
    // One run a skill at a time: what is under way, by skill, and the runs
    // of a list's items waiting their turn.
    let mut runs = Runs::default();
    let mut subscribers: Vec<Subscriber> = Vec::new();
    let mut seen = None;
    let mut looked: Option<Instant> = None;
    eprintln!(
        "lamp-skills: skills from {}, the Policy's ask {}, a skill's light {}",
        dir.display(),
        if skip_ask {
            "skipped"
        } else if ask_policy {
            "asked on skills/ask"
        } else {
            "not asked: nothing is done"
        },
        if runner.glows.is_some() {
            "asked of the ring on light/request"
        } else {
            "set through the actions part and given back"
        }
    );
    loop {
        // Pending Policy answers and queued runs must see loss of clock
        // trust before they can start scheduled output.
        sample_clock(&mut engine, &runner, clock::now_us());
        while let Ok(event) = done.try_recv() {
            let skill = event.skill.clone();
            publish(&mut out, event);
            runs.done(&skill, &engine, &mut out, &runner);
        }
        take_skips(
            &skips,
            &mut subscribers,
            &mut engine,
            &mut runs,
            &mut out,
            &runner,
            &mut asking,
        );
        if let Some((_, answers)) = &mut asking.gate {
            answer_asks(
                answers,
                &mut asking.waiting,
                &mut engine,
                &mut runs,
                &mut out,
                &runner,
            );
        }
        if looked.is_none_or(|at| clock::now().saturating_duration_since(at) >= RELOAD_EVERY) {
            looked = Some(clock::now());
            let print = fingerprint(dir);
            if seen.as_ref() != Some(&print) {
                seen = Some(print);
                let (mut skills, errors) = load_all(dir);
                skills.retain(|skill| options.routines || !skill.is_routine());
                for error in errors {
                    eprintln!("lamp-skills: not loaded: {error}");
                }
                // Keep the previous accepted generation intact if its
                // cap declarations cannot be installed as one bounded set.
                if let Some(limits) = &limits
                    && let Err(error) = limits.replace(&skills)
                {
                    eprintln!("lamp-skills: reload refused: {error}");
                    continue;
                }
                let names: Vec<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
                eprintln!("lamp-skills: loaded {names:?}");
                if let Some(manager) = &mut routines {
                    manager.reload_skills(&skills, lamp_ipc::monotonic_us());
                }
                *runner.permissions.borrow_mut() = skills
                    .iter()
                    .map(|skill| (skill.name.clone(), skill.permissions.clone()))
                    .collect();
                // A watch whose skill is gone, or changed, is off.
                {
                    let mut watching = runner.watching.borrow_mut();
                    let outs = watching.watches.replace(&skills);
                    watched(outs, &mut watching, &mut out, &runner);
                }
                // A held dim whose skill or trigger is gone is given back.
                for run in engine.replace(skills) {
                    handle(run, &mut runs, &mut out, &runner);
                }
                if let Some(tools) = tools {
                    write_tools(tools, &engine);
                }
                let topics: Vec<_> = engine
                    .topics()
                    .into_iter()
                    .filter(|topic| topic != lamp_skills::LOCAL_CLOCK_TOPIC)
                    .collect();
                subscribers
                    .retain(|subscriber| topics.iter().any(|topic| topic == subscriber.topic()));
                for topic in topics {
                    if !subscribers
                        .iter()
                        .any(|subscriber| subscriber.topic() == topic)
                    {
                        match bus.subscribe(&topic, "skills") {
                            Ok(subscriber) => subscribers.push(subscriber),
                            Err(error) => eprintln!("lamp-skills: {topic} not subscribed: {error}"),
                        }
                    }
                }
            }
            if let Some(tools) = tools {
                keep_tools(tools, &ask, &engine)?;
            }
            let _ = out.alive();
        }
        let now_us = clock::now_us();
        // Anchor the wall sample before reading it or checking trust. Slow
        // clock/zone work must not move a routine's window deadline later.
        let wall_sampled_at_us = lamp_ipc::monotonic_us();
        let trusted_wall = sample_clock(&mut engine, &runner, now_us);
        if let Some(manager) = &mut routines {
            manager.tick(
                lamp_ipc::monotonic_us(),
                wall_sampled_at_us,
                runner.local_time.as_deref(),
                trusted_wall.unwrap_or(0),
            );
        }
        dispatch(
            engine.clock_tick(now_us),
            &mut runs,
            &mut out,
            &runner,
            &mut asking,
        );
        for run in engine.tick(now_us) {
            handle(run, &mut runs, &mut out, &runner);
        }
        // One wait for any topic's message, the Policy's answer, or a call,
        // a skip or a run's end (the wake), not each topic in its turn: a
        // tool call waited out the turn of whichever topic the loop was on
        // (#1806). While the Policy is asked, its answers are looked for
        // every 5 ms, so a late one is given up on time.
        let wait = if asking.waiting.is_empty() {
            WAIT_AT_MOST
        } else {
            Duration::from_millis(5)
        };
        wait_any(
            &subscribers,
            asking.gate.as_ref().map(|(_, answers)| answers),
            &woken,
            clock::poll(wait),
        );
        take_skips(
            &skips,
            &mut subscribers,
            &mut engine,
            &mut runs,
            &mut out,
            &runner,
            &mut asking,
        );
        while let Ok(call) = asks.try_recv() {
            // What was heard before the call, read first: `[again]` asks
            // whether it asked for the call.
            for subscriber in subscribers
                .iter_mut()
                .filter(|subscriber| subscriber.topic() == Speech::TOPICS[0])
            {
                while let Some(message) = receive(subscriber, Duration::ZERO) {
                    observe(
                        &message,
                        &mut engine,
                        &mut runs,
                        &mut out,
                        &runner,
                        &mut asking,
                    );
                }
            }
            tool_call(call, &mut engine, &mut runs.busy, &mut out, &runner);
        }
        if let Some((_, answers)) = &mut asking.gate {
            answer_asks(
                answers,
                &mut asking.waiting,
                &mut engine,
                &mut runs,
                &mut out,
                &runner,
            );
        }
        // A message a topic, as its turn did; one waiting still makes the
        // next wait end at once.
        for subscriber in &mut subscribers {
            if let Some(message) = receive(subscriber, Duration::ZERO) {
                observe(
                    &message,
                    &mut engine,
                    &mut runs,
                    &mut out,
                    &runner,
                    &mut asking,
                );
            }
        }
        // A held run whose trigger topic went silent ends.
        for run in engine.tick(clock::now_us()) {
            handle(run, &mut runs, &mut out, &runner);
        }
        watch_look(&mut out, &runner);
    }
}

/// The Policy's gate, when it is asked: where the asks go and its answers
/// come from, the runs waiting for an answer by the ask's number, and the
/// last ask's number.
struct Asking {
    gate: Option<(Publisher, Subscriber)>,
    waiting: HashMap<u64, (lamp_skills::Run, Instant)>,
    last: u64,
}

/// The next message of one subscriber, within `wait`.
fn receive(subscriber: &mut Subscriber, wait: Duration) -> Option<lamp_bus::Message> {
    match subscriber.recv(wait) {
        Ok(message) => message,
        Err(error) => {
            eprintln!("lamp-skills: {} not received: {error}", subscriber.topic());
            None
        }
    }
}

/// One message to the skills, and the runs it starts, or asks the Policy
/// for.
fn observe(
    message: &lamp_bus::Message,
    engine: &mut Engine,
    runs: &mut Runs,
    out: &mut Publisher,
    runner: &Runner,
    asking: &mut Asking,
) {
    // Received now, on the program's clock: the publisher's own stamps are
    // on its clock, not this one.
    let now_us = clock::now_us();
    sample_clock(engine, runner, now_us);
    if message.header.topic == Speech::TOPICS[0]
        && let Some(turn) = message
            .parse::<Speech>()
            .ok()
            .and_then(|speech| speech.request)
    {
        let mut newest = NEWEST_HEARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *newest = newest.max(Some(turn));
    }
    let made = engine.observe(
        &message.header.topic,
        now_us,
        &message.payload,
        message.header.expires_ms,
    );
    {
        let mut watching = runner.watching.borrow_mut();
        let outs = watching.watches.observe(
            &message.header.topic,
            &message.payload,
            now_us,
            local_minute(runner),
        );
        watched(outs, &mut watching, out, runner);
    }
    dispatch(made, runs, out, runner, asking);
}

fn dispatch(
    made: Vec<Run>,
    runs: &mut Runs,
    out: &mut Publisher,
    runner: &Runner,
    asking: &mut Asking,
) {
    for mut run in made {
        for event in std::mem::take(&mut run.events) {
            publish(out, event);
        }
        if let (Some(question), Some((asks, _))) = (run.ask.take(), &mut asking.gate) {
            ask_policy_for(
                question,
                run,
                asks,
                &runner.bus,
                &mut asking.last,
                &mut asking.waiting,
                out,
            );
        } else {
            handle(run, runs, out, runner);
        }
    }
}

/// A run the engine made: its events published, and its calls started (or,
/// an item's, queued), or its held run told that its trigger ended.
fn handle(mut run: Run, runs: &mut Runs, out: &mut Publisher, runner: &Runner) {
    for event in std::mem::take(&mut run.events) {
        publish(out, event);
    }
    if run.calls == [Call::Ends] {
        // Its run, waiting, gives back what it held. Dropping the sender
        // ends the wait as well.
        if let Some(end) = runner.ends.borrow_mut().remove(&run.skill) {
            let _ = end.send(());
        }
    } else if !run.calls.is_empty() {
        let calls = std::mem::take(&mut run.calls);
        start_or_queue(run, calls, runs, out, runner);
    }
}

/// By skill, the runs of a list's items waiting for its run under way.
type Queued = HashMap<String, VecDeque<lamp_skills::Run>>;

/// The runs under way, one a skill, and the items' runs waiting for them.
#[derive(Default)]
struct Runs {
    busy: HashSet<String>,
    queued: Queued,
}

impl Runs {
    /// `skill`'s run is done: the next of its queued runs whose item still
    /// holds starts, its calls made again now, a look at where the face is
    /// now (the reviewer on #1042: the second of two runs starts 10 to 20 s
    /// after it was queued). One that no longer holds is let go, said so,
    /// and the one after it is tried.
    fn done(&mut self, skill: &str, engine: &Engine, out: &mut Publisher, runner: &Runner) {
        self.busy.remove(skill);
        while let Some(run) = self.queued.get_mut(skill).and_then(VecDeque::pop_front) {
            match engine.still(&run, clock::now_us()) {
                Some(calls) if !calls.is_empty() => {
                    let _ = start(run.skill, calls, &mut self.busy, out, runner);
                    return;
                }
                _ => publish(
                    out,
                    SkillEvent {
                        skill: run.skill,
                        stage: SkillStage::Done,
                        why: "not made: its item no longer holds as its turn came".into(),
                        allowed: None,
                    },
                ),
            }
        }
    }
}

/// Starts a run with `calls`, or, for an item's run (`queue`) while the
/// skill's last is under way, keeps the run for when that one is done: an
/// item's news is its own, never let go or put together with the run before
/// it. Its calls are made again when it starts ([`Runs::done`]).
fn start_or_queue(
    run: lamp_skills::Run,
    calls: Vec<Call>,
    runs: &mut Runs,
    out: &mut Publisher,
    runner: &Runner,
) {
    if !(run.queue && runs.busy.contains(&run.skill)) {
        // A trigger's run: nobody waits for its body and light.
        let _ = start(run.skill, calls, &mut runs.busy, out, runner);
        return;
    }
    let waiting = runs.queued.entry(run.skill.clone()).or_default();
    if waiting.len() < MOST_WAITING {
        waiting.push_back(run);
    } else {
        publish(
            out,
            SkillEvent {
                skill: run.skill,
                stage: SkillStage::Done,
                why: format!("not made: {MOST_WAITING} runs already wait"),
                allowed: None,
            },
        );
    }
}

/// What a run needs from the program.
struct Runner {
    sockets: Sockets,
    finished: Waking<SkillEvent>,
    bus: Bus,
    /// With `LAMP_LIGHT_REQUESTS=on`, how a skill's light is asked of the ring.
    glows: Option<Glows>,
    /// `LAMP_BODY_REQUESTS=on`: a skill's gestures and moves say who asks.
    body_requests: bool,
    /// By skill, the run that holds something until its trigger ends.
    ends: RefCell<HashMap<String, mpsc::Sender<()>>>,
    /// The lamp's local time, for quiet hours.
    local_time: Option<Arc<lamp_local_time::LocalTime>>,
    trust_clock: bool,
    /// One diagnostic per missing-trust interval, only for enabled schedules.
    clock_waiting: Cell<bool>,
    rest_client: Option<lamp_skills::routine::Client>,
    /// By skill, what each loaded skill may use, as of the last load: a
    /// skill not loaded may use nothing.
    permissions: RefCell<HashMap<String, Permissions>>,
    /// By skill, the looks its search under way has taken (#1685).
    looks: RefCell<HashMap<String, u32>>,
    /// The watches (guard mode), their marks and their texts.
    watching: RefCell<Watching>,
}

/// The watch skills' watches ([`watch`]), the mark each shows while on,
/// where their texts go, and the eyes' known file.
struct Watching {
    watches: Watches,
    marks: HashMap<String, Glow>,
    /// `messages/out`, made when the first text goes.
    texts: Option<Publisher>,
    known_file: Option<PathBuf>,
    /// The ring's word on the marks: `light/outranked` and `light/reached`,
    /// subscribed to before the first mark is asked, so no answer is missed.
    signs: Option<(Option<Subscriber>, Option<Subscriber>)>,
    /// When each mark was last asked for again with a new claim.
    retaken: HashMap<String, Instant>,
}

/// A mark the ring says is not shown is asked for again with a new claim at
/// most this often: one a stronger light hides is left hidden, the watch not
/// counting, rather than asked for in a loop.
const MARK_AGAIN: Duration = Duration::from_secs(30);

/// The eyes renew their known file every 2 s while the known person is at
/// the lamp and remove it when nobody is near: older than this, it says
/// nothing.
const KNOWN_FRESH: Duration = Duration::from_secs(5);

/// The name in the eyes' known file, if it is fresh: kept in the program
/// only, to tell a known face from one it does not know and the person who
/// asked from anyone else; never in a text, an event or the journal.
fn known_now(path: &Path) -> Option<String> {
    lamp_skills::known::name_in(path, KNOWN_FRESH)
}

/// The lamp's local minute of the day, if its time is known.
fn local_minute(runner: &Runner) -> Option<u16> {
    let local = runner.local_time.as_ref()?;
    let now_ms = unix_ms();
    Some(watch::minute_of(now_ms, local.offset_minutes_at(now_ms)))
}

/// Whether a watch's mark can be shown: the ring takes `light/request`.
fn can_mark(runner: &Runner) -> bool {
    runner.glows.as_ref().is_some_and(|glows| {
        glows
            .bus
            .subscribers(LightRequest::TOPIC)
            .is_ok_and(|subscribers| !subscribers.is_empty())
    })
}

/// What the watches do: each mark shown or given back, each text sent on
/// `messages/out`, each held to its skill's permissions where it leaves the
/// program, and said on `skills/events`.
fn watched(outs: Outs, watching: &mut Watching, out: &mut Publisher, runner: &Runner) {
    for (skill, did) in outs {
        let may = runner
            .permissions
            .borrow()
            .get(&skill)
            .cloned()
            .unwrap_or_default();
        let said = |out: &mut Publisher, why: String| {
            publish(
                out,
                SkillEvent {
                    skill: skill.clone(),
                    stage: SkillStage::Done,
                    why,
                    allowed: None,
                },
            );
        };
        match did {
            Out::Mark(call) => {
                if let Some(why) = may.refuses(&call) {
                    eprintln!("lamp-skills: skill:{skill}: refused its mark: {why}");
                    continue;
                }
                let Some(glows) = &runner.glows else {
                    said(
                        out,
                        format!("mark: not shown: the ring needs {REQUESTS_ENVIRONMENT}=on"),
                    );
                    continue;
                };
                if let Some(old) = watching.marks.remove(&skill) {
                    old.give_back();
                }
                if watching.signs.is_none() {
                    let subscribe = |topic: &str, kind: &str| {
                        runner
                            .bus
                            .subscribe_typed(topic, "skills_watch", kind)
                            .map_err(|error| {
                                eprintln!("lamp-skills: {topic} not subscribed to: {error}")
                            })
                            .ok()
                    };
                    watching.signs = Some((
                        subscribe(LightOutranked::TOPIC, LightOutranked::TYPE),
                        subscribe(LightReached::TOPIC, LightReached::TYPE),
                    ));
                }
                let mut glow = Glow::new(glows, &skill, light_now(&runner.bus));
                let how = glow.ask(&call);
                watching.marks.insert(skill.clone(), glow);
                said(out, format!("mark: {how}"));
            }
            Out::Unmark => {
                if let Some(back) = watching.marks.remove(&skill).and_then(Glow::give_back) {
                    said(out, format!("mark: {back}"));
                }
            }
            Out::Text(text) => {
                if let Some(why) = may.refuses(&Call::Text(text.clone())) {
                    eprintln!("lamp-skills: skill:{skill}: refused its text: {why}");
                    continue;
                }
                let message = MessageOut {
                    from: format!("skill:{skill}").chars().take(64).collect(),
                    text,
                };
                if let Err(why) = message.check() {
                    said(out, format!("text: not sent: {why}"));
                    continue;
                }
                if watching.texts.is_none() {
                    match runner.bus.publisher_declared(
                        MessageOut::TOPIC,
                        &MessageOut::declaration(),
                        "skills",
                    ) {
                        Ok(texts) => watching.texts = Some(texts),
                        Err(error) => {
                            said(
                                out,
                                format!("text: not sent: {} ({error})", MessageOut::TOPIC),
                            );
                            continue;
                        }
                    }
                }
                let sent = watching
                    .texts
                    .as_mut()
                    .map(|texts| texts.publish_json(lamp_ipc::monotonic_us(), &message));
                match sent {
                    Some(Ok(_)) => said(
                        out,
                        format!("text: sent on {}: {}", MessageOut::TOPIC, message.text),
                    ),
                    Some(Err(error)) => said(out, format!("text: not sent: {error}")),
                    None => {}
                }
            }
            Out::Said(why) => said(out, why),
        }
    }
}

/// A watch's tool called: committed first, so a caller that gave up turns
/// nothing; then turned on or off, the model told what to say, and what the
/// watch does done.
fn watch_call((name, args, reply, admission): ToolAsk, out: &mut Publisher, runner: &Runner) {
    if !admission.commit() {
        eprintln!("lamp-skills: {name} with {args} not run: the caller gave up");
        return;
    }
    let mut watching = runner.watching.borrow_mut();
    let asker = watching.known_file.as_deref().and_then(known_now);
    let marks = can_mark(runner);
    match watching
        .watches
        .tool(&name, &args, clock::now_us(), asker.as_deref(), marks)
    {
        Some(Ok((skill, answer, outs))) => {
            let _ = reply.send(answer);
            publish(
                out,
                SkillEvent {
                    skill: skill.clone(),
                    stage: SkillStage::Triggered,
                    why: format!("the model called {name} with {args}"),
                    allowed: None,
                },
            );
            let outs = outs.into_iter().map(|did| (skill.clone(), did)).collect();
            watched(outs, &mut watching, out, runner);
        }
        Some(Err(why)) => {
            eprintln!("lamp-skills: {name} with {args} not run: {why}");
            let _ = reply.send(json!({ "error": why }));
        }
        None => {
            let _ = reply.send(json!({ "error": "No skill of mine does that just now." }));
        }
    }
}

/// While a watch is on: the ring's word on its mark, its time limit, the
/// known face at the lamp and the privacy switch, looked at.
fn watch_look(out: &mut Publisher, runner: &Runner) {
    let mut watching = runner.watching.borrow_mut();
    if !watching.watches.any_on() {
        return;
    }
    let (now_us, minute) = (clock::now_us(), local_minute(runner));
    let mut outs = mark_signs(&mut watching, out, now_us);
    outs.extend(watching.watches.tick(now_us));
    let known = watching.known_file.as_deref().and_then(known_now);
    outs.extend(watching.watches.known(known.as_deref(), now_us, minute));
    let closed = said_lately::<serde_json::Value>(&runner.bus, watch::PRIVACY)
        .is_some_and(|privacy| privacy["physical_muted"] == json!(true));
    outs.extend(watching.watches.privacy(closed, now_us));
    watched(outs, &mut watching, out, runner);
}

/// What the ring said of the marks since the last look (the peer read on
/// #1807): shown (`light/reached`), the watch counts; not shown
/// (`light/outranked`), it counts nothing. By another program's light, its
/// mark is asked for again with a new claim, which the ring hears over an
/// older one of the same priority, at most once each `MARK_AGAIN`; by the
/// person's own light, never: the watch pauses until they ask again. Each
/// word is taken at `now_us`: a mark the ring has not said shows for 30 s
/// counts nothing ([`watch::CONFIRMED_WITHIN_US`], Third's privacy read on
/// #1963).
fn mark_signs(watching: &mut Watching, out: &mut Publisher, now_us: u64) -> Outs {
    let mut said: Vec<(String, bool, Option<String>)> = Vec::new();
    if let Some((outranked, reached)) = &mut watching.signs {
        if let Some(outranked) = outranked {
            while let Ok(Some(message)) = outranked.recv(Duration::ZERO) {
                if let Ok(told) = message.parse::<LightOutranked>() {
                    said.push((told.source, false, Some(told.by)));
                }
            }
        }
        if let Some(reached) = reached {
            while let Ok(Some(message)) = reached.recv(Duration::ZERO) {
                if let Ok(drawn) = message.parse::<LightReached>() {
                    said.push((drawn.source, true, None));
                }
            }
        }
    }
    let mut outs = Vec::new();
    for (source, shown, by) in said {
        let Some(skill) = source.strip_prefix("skill:").map(str::to_owned) else {
            continue;
        };
        if !watching.marks.contains_key(&skill) {
            continue;
        }
        outs.extend(
            watching
                .watches
                .ring_said(&skill, shown, by.as_deref(), now_us),
        );
        // The person's own light is never asked over (the first builder's
        // ruling on #1937): the watch pauses until they ask again.
        if shown || by.as_deref() == Some(LightOutranked::PERSON) {
            continue;
        }
        let now = clock::now();
        let again = watching
            .retaken
            .get(&skill)
            .is_none_or(|at| now.saturating_duration_since(*at) >= MARK_AGAIN);
        if let (true, Some(mark)) = (again, watching.watches.mark_of(&skill)) {
            watching.retaken.insert(skill.clone(), now);
            outs.push((skill.clone(), Out::Mark(mark)));
            publish(
                out,
                SkillEvent {
                    skill,
                    stage: SkillStage::Done,
                    why: format!(
                        "mark: outranked by {}: asked for again with a new claim",
                        by.as_deref().unwrap_or("something")
                    ),
                    allowed: None,
                },
            );
        }
    }
    outs
}

impl Runner {
    /// The sockets for a run of `skill`, held to what it may use.
    fn sockets_for(&self, skill: &str) -> Sockets {
        let mut sockets = self.sockets.clone();
        sockets.may = May {
            skill: skill.to_owned(),
            permissions: self
                .permissions
                .borrow()
                .get(skill)
                .cloned()
                .unwrap_or_default(),
        };
        sockets
    }
}

/// Keep legacy quiet-hour time separate from trusted daily/routine time.
/// The predicate is injectable so the default-off path can prove it never
/// queries kernel synchronization, including on non-Linux hosts.
fn update_clock(
    engine: &mut Engine,
    local: &lamp_local_time::LocalTime,
    now_us: u64,
    wall_ms: u64,
    trust_clock: bool,
    synchronized: impl FnOnce() -> Option<bool>,
) -> Option<u64> {
    let trusted = if engine.needs_daily_clock() {
        lamp_skills::daily::trusted_unix_ms(
            wall_ms,
            if trust_clock {
                Some(true)
            } else {
                synchronized()
            },
        )
    } else {
        None
    };
    engine.clock_in_with_trust(local, now_us, wall_ms, trusted.is_some());
    trusted
}

fn sample_clock(engine: &mut Engine, runner: &Runner, now_us: u64) -> Option<u64> {
    let trusted = runner.local_time.as_deref().and_then(|local| {
        update_clock(
            engine,
            local,
            now_us,
            unix_ms(),
            runner.trust_clock,
            lamp_ipc::clock_synced,
        )
    });
    if let Some(message) = clock_diagnostic(engine, trusted, &runner.clock_waiting) {
        eprintln!("lamp-skills: {message}");
    }
    trusted
}

fn clock_diagnostic(
    engine: &Engine,
    trusted: Option<u64>,
    waiting: &Cell<bool>,
) -> Option<&'static str> {
    let blocked = engine.needs_daily_clock() && trusted.is_none();
    let was_blocked = waiting.replace(blocked);
    (blocked && !was_blocked)
        .then_some("scheduled skills are waiting for a configured, trusted local clock")
}

/// Milliseconds since the Unix epoch, by the wall clock.
fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// Starts one run of `skill`, unless its last is still under way.
/// Starts a run on its own thread: what it gives says when its body and
/// light have been made (or the run is over), for a call's answer to wait on.
fn start(
    skill: String,
    calls: Vec<Call>,
    busy: &mut HashSet<String>,
    out: &mut Publisher,
    runner: &Runner,
) -> mpsc::Receiver<()> {
    let (made, made_rx) = mpsc::channel();
    if !busy.insert(skill.clone()) {
        publish(
            out,
            SkillEvent {
                skill,
                stage: SkillStage::Done,
                why: "not made: its last run is still under way".into(),
                allowed: None,
            },
        );
        return made_rx;
    }
    // A run waits on the voice for as long as the say takes (at most
    // SAY_TIMEOUT a line) or a light's for (at most 10 min): its own
    // thread, one a skill, so the skills go on listening.
    // ponytail: a skill removed meanwhile still finishes its run.
    let (sockets, finished, bus, glows, asks_body, rest_client) = (
        runner.sockets_for(&skill),
        runner.finished.clone(),
        runner.bus.clone(),
        runner.glows.clone(),
        runner.body_requests,
        runner
            .rest_client
            .as_ref()
            .map(lamp_skills::routine::Client::for_run),
    );
    let before = light_now(&bus);
    // A run that holds something until its trigger ends waits for the end.
    let end = calls.iter().any(Call::held_until_ends).then(|| {
        let (end, ended) = mpsc::channel();
        runner.ends.borrow_mut().insert(skill.clone(), end);
        ended
    });
    std::thread::spawn(move || {
        // Rest bypasses perform's per-output checks. Check the whole plan
        // first: a denied accompanying cue must never cause an earlier wake.
        if calls.iter().any(|call| matches!(call, Call::Rest(_)))
            && let Some(why) = calls.iter().find_map(|call| sockets.may.refuses(call))
        {
            let _ = finished.send(SkillEvent {
                skill,
                stage: SkillStage::Done,
                why,
                allowed: Some(false),
            });
            return;
        }
        // Rest admission, or completion of the owner's planned wake,
        // precedes accompanying sound/light. Say can change the Voice
        // owner, so starting its hum during the stretch would cancel it.
        let mut rest_permits = Vec::new();
        for call in &calls {
            if let Call::Rest(rest) = call {
                let result = rest_client
                    .as_ref()
                    .ok_or_else(|| "skill routines are disabled".to_owned())
                    .and_then(|client| client.request(&skill, rest.clone()));
                match result {
                    Ok(permit) => rest_permits.push(permit),
                    Err(why) => {
                        let _ = finished.send(SkillEvent {
                            skill,
                            stage: SkillStage::Done,
                            why,
                            allowed: Some(false),
                        });
                        return;
                    }
                }
            }
        }
        if rest_client.as_ref().is_some_and(|client| !client.current()) {
            let _ = finished.send(SkillEvent {
                skill,
                stage: SkillStage::Done,
                why: "skill definitions changed before presentation".into(),
                allowed: Some(false),
            });
            return;
        }
        if let Err(why) = rest_permits.iter().try_for_each(|permit| permit.check()) {
            let _ = finished.send(SkillEvent {
                skill,
                stage: SkillStage::Done,
                why,
                allowed: Some(false),
            });
            return;
        }
        let why = perform(
            &skill,
            (&calls, &rest_permits),
            &sockets,
            &bus,
            before,
            (glows.as_ref(), asks_body),
            (made, end),
        );
        let _ = finished.send(SkillEvent {
            skill,
            stage: SkillStage::Done,
            why,
            allowed: None,
        });
    });
    made_rx
}

/// Puts a run's question to the Policy and keeps the run until it answers.
fn ask_policy_for(
    mut question: serde_json::Value,
    run: lamp_skills::Run,
    asks: &mut Publisher,
    bus: &Bus,
    last: &mut u64,
    waiting: &mut HashMap<u64, (lamp_skills::Run, Instant)>,
    out: &mut Publisher,
) {
    // Nobody to answer (lamp-policy down, or without --gate): no at once.
    if bus
        .subscribers("skills/ask")
        .is_ok_and(|subs| subs.is_empty())
    {
        publish(
            out,
            SkillEvent {
                skill: run.skill,
                stage: SkillStage::Answered,
                why: "not asked: nobody is subscribed to skills/ask (lamp-policy --gate)".into(),
                allowed: Some(false),
            },
        );
        return;
    }
    // The ask's number: this clock in µs, never 0 and always above the last.
    let number = (*last + 1).max(clock::now_us());
    *last = number;
    question["ask"] = json!(number);
    match asks.publish_json(1, &question) {
        Ok(_) => {
            waiting.insert(number, (run, clock::now() + ASK_WAIT));
        }
        Err(error) => publish(
            out,
            SkillEvent {
                skill: run.skill,
                stage: SkillStage::Answered,
                why: format!("not asked: skills/ask was not written ({error})"),
                allowed: Some(false),
            },
        ),
    }
}

/// The Policy's answers to the asks waiting: a `say_now` not in shadow
/// starts the run, made again now, if its trigger still holds. Anything
/// else, or nothing within `ASK_WAIT`, leaves it undone.
fn answer_asks(
    answers: &mut Subscriber,
    waiting: &mut HashMap<u64, (lamp_skills::Run, Instant)>,
    engine: &mut Engine,
    runs: &mut Runs,
    out: &mut Publisher,
    runner: &Runner,
) {
    while let Ok(Some(message)) = answers.recv(Duration::ZERO) {
        let Ok(decision) = serde_json::from_slice::<serde_json::Value>(&message.payload) else {
            continue;
        };
        let Some((run, _)) = decision["ask"]
            .as_u64()
            .and_then(|number| waiting.remove(&number))
        else {
            continue;
        };
        let choice = decision["choice"].as_str().unwrap_or("");
        // A decision without `shadow` counts as one in shadow: "would say".
        let shadow = decision["shadow"].as_bool() != Some(false);
        let mut why = format!(
            "the Policy answered {choice} ({}, by {})",
            decision["about"].as_str().unwrap_or("?"),
            decision["by"].as_str().unwrap_or("?")
        );
        let mut calls = None;
        if choice == "say_now" && shadow {
            why += " in shadow: nothing is done";
        } else if choice == "say_now" {
            calls = engine.still(&run, clock::now_us());
            if calls.is_none() {
                why += ", but its trigger no longer holds: nothing is done";
            }
        }
        publish(
            out,
            SkillEvent {
                skill: run.skill.clone(),
                stage: SkillStage::Answered,
                why,
                allowed: Some(calls.is_some()),
            },
        );
        if let Some(calls) = calls.filter(|calls| !calls.is_empty()) {
            // A `[rest]` watches from here whether it is noticed (#1685).
            engine.made(&run.skill, clock::now_us());
            start_or_queue(run, calls, runs, out, runner);
        }
    }
    give_up_late(waiting, out);
}

/// The asks waiting past their `ASK_WAIT`, given up: nothing is done.
fn give_up_late(waiting: &mut HashMap<u64, (lamp_skills::Run, Instant)>, out: &mut Publisher) {
    let now = clock::now();
    let late: Vec<u64> = waiting
        .iter()
        .filter(|(_, (_, until))| *until <= now)
        .map(|(number, _)| *number)
        .collect();
    for number in late {
        if let Some((run, _)) = waiting.remove(&number) {
            publish(
                out,
                SkillEvent {
                    skill: run.skill,
                    stage: SkillStage::Answered,
                    why: "the Policy did not answer in time: nothing is done".into(),
                    allowed: Some(false),
                },
            );
        }
    }
}

/// A call of a skill's tool from the voice, its arguments, and where its
/// answer goes.
type ToolAsk = (
    String,
    serde_json::Value,
    mpsc::Sender<serde_json::Value>,
    Admission,
);

/// The model called a skill's tool: its steps run, and the voice is told
/// what the model is to say: at once, or, with a look, once the body and
/// the light are made (#1146). The say is the model's and the look the
/// voice's, asked in the answer; the body and the light are made here.
fn tool_call(
    (name, args, reply, admission): ToolAsk,
    engine: &mut Engine,
    busy: &mut HashSet<String>,
    out: &mut Publisher,
    runner: &Runner,
) {
    // A watch's tool turns its watch, with no steps (guard mode, #1685).
    let watched_tool = runner.watching.borrow().watches.owner(&name).is_some();
    if watched_tool {
        return watch_call((name, args, reply, admission), out, runner);
    }
    let refuse = |why: &str| {
        if admission.commit() {
            let _ = reply.send(json!({ "error": why }));
        }
    };
    // A run with no pose to hold, under way: refused before it is a run,
    // so nothing moves, the line is not said again, and `[again]`'s while
    // still runs from the run under way (#999: "again!" mid-dance).
    if let Some((skill, still)) = engine.refused_under_way(&name)
        && busy.contains(skill)
    {
        eprintln!("lamp-skills: {name} with {args} not run: its run is under way");
        return refuse(still);
    }
    // A search's look (#1685): its first at the desk, its looks counted,
    // none past the last, the model told to give up kindly instead.
    let searching = engine
        .skills()
        .iter()
        .find(|skill| skill.tool.as_deref() == Some(name.as_str()))
        .filter(|skill| skill.search.is_some())
        .map(|skill| {
            let looked = if busy.contains(&skill.name) {
                runner.looks.borrow().get(&skill.name).copied().unwrap_or(0)
            } else {
                0
            };
            (skill.name.clone(), looked, skill.search_look(&args, looked))
        });
    let counted = searching
        .as_ref()
        .map(|(skill, looked, _)| (skill.clone(), *looked));
    let args = match searching {
        Some((_, looked, Err(give_up))) => {
            eprintln!(
                "lamp-skills: {name} with {args} not run: its search took its {looked} looks"
            );
            return refuse(&give_up);
        }
        Some((_, _, Ok(look))) => look,
        None => args,
    };
    let mut run = match engine.tool(&name, &args, clock::now_us()) {
        Ok(run) => run,
        Err(why) => {
            eprintln!("lamp-skills: {name} with {args} not run: {why}");
            return refuse(&why);
        }
    };
    // The look's picture goes to the model only for a skill that may see it.
    let sockets = runner.sockets_for(&run.skill);
    run.calls
        .retain(|call| *call != Call::Look || sockets.may.refuses(call).is_none());
    let made: Vec<Call> = run
        .calls
        .iter()
        .filter(|call| {
            !matches!(
                call,
                Call::Look | Call::Say(_) | Call::Words(_) | Call::SayFrom(..)
            )
        })
        .cloned()
        .collect();
    // Nothing but the wait for the say is nothing to make.
    let nothing = made.iter().all(|call| *call == Call::AfterSay);
    // A run under way is left as it is: a call again meanwhile is told
    // again what to do next (a student who asks a minute later gets a new
    // look), and nothing more is made; the light already on is given back
    // when its `for` ends. A call that is nothing but made steps has
    // nothing to tell, and is refused.
    let under_way = !nothing && busy.contains(&run.skill);
    if under_way && made.len() == run.calls.len() {
        return refuse("I'm still doing that.");
    }
    // The caller told it timed out: nothing is done.
    if !admission.commit() {
        return;
    }
    if let Some((skill, looked)) = counted {
        runner.looks.borrow_mut().insert(skill, looked + 1);
    }
    // A call with a look is answered only once its body and light are made:
    // the voice asks the eyes as the answer's first audio plays, and by
    // then the head is on its way down over the page (#941, #1146). Any
    // other call is answered at once.
    let told = told(&run.calls);
    let looks = run.calls.contains(&Call::Look);
    if !looks {
        let _ = reply.send(told.clone());
    }
    for event in run.events {
        publish(out, event);
    }
    if under_way {
        // Each look comes with the head down over the page again: only the
        // run's poses are made again, and a light the person keeps, never a
        // light given back at the run's end (#1051, Ten's P2). The call's
        // pose takes the place of the one held, and a call with none ends
        // it: a scene changed, or the light turned elsewhere (#1679).
        if !made.iter().any(|call| matches!(call, Call::Mood { .. })) {
            end_hold(&run.skill);
        }
        for call in made
            .iter()
            .filter(|call| matches!(call, Call::Mood { .. } | Call::Light { keep: true, .. }))
        {
            if sockets.may.refuses(call).is_none() {
                make(call, &sockets, &runner.bus, None);
            }
        }
    }
    if nothing || under_way {
        publish(
            out,
            SkillEvent {
                skill: run.skill,
                stage: SkillStage::Done,
                why: if under_way {
                    "the model was told again what to do next; its last run is still under way, left as it is but for its poses, held again"
                } else {
                    "the model was told what to do next"
                }
                .into(),
                allowed: None,
            },
        );
    } else {
        // A fresh run with no pose ends a hold that outlived the run that
        // held it again (a call again under way holds its pose its own
        // `for`): "relax" after "reading" brings the head up even then (the
        // review on #1730).
        if !made.iter().any(|call| matches!(call, Call::Mood { .. })) {
            end_hold(&run.skill);
        }
        let made = start(run.skill, made, busy, out, runner);
        if looks {
            // Bounded: the caller waits at most `TOOL_TIMEOUT` once more.
            let _ = clock::recv_timeout(&made, MADE_BEFORE_ANSWER);
        }
    }
    if looks {
        let _ = reply.send(told);
    }
}

/// The skills' tools for the voice: `tools.json` written beside a
/// `call.sock` in `dir`, which the voice finds in the actions part's
/// directory (`docs/tools.md`). No tool, no file.
fn write_tools(dir: &Path, engine: &Engine) {
    let file = tools_file(engine.skills());
    let path = dir.join("tools.json");
    let written = if file["tools"].as_array().is_some_and(Vec::is_empty) {
        std::fs::remove_file(&path).or_else(|error| match error.kind() {
            io::ErrorKind::NotFound => Ok(()),
            _ => Err(error),
        })
    } else {
        // Whole or not at all: the voice may read it as it is written.
        let fresh = dir.join(".tools.json");
        std::fs::write(&fresh, file.to_string()).and_then(|()| std::fs::rename(&fresh, &path))
    };
    if let Err(error) = written {
        eprintln!("lamp-skills: {} not written: {error}", path.display());
    }
}

/// The tools' folder looked after at a look. Inside the actions part's
/// folder, systemd removed it at each of actions' restarts, and the voice
/// had no skill functions until lamp-skills restarted (lamp-4ace, 19 Oct
/// 04:33); it is lamp-skills' own folder now, so this is the guard.
/// - Its socket gone from a folder still there: bound again, and its file
///   written. The old socket's thread is left waiting on a path no caller
///   can reach any more.
/// - The folder itself gone: lamp-skills ends, with a line. It cannot make
///   it again as its user, under a sandbox whose writable bind went with
///   the folder (the reviewer on #1656); systemd makes the folder again as
///   it starts it once more (`Restart=on-failure`).
fn keep_tools(dir: &Path, asks: &Waking<ToolAsk>, engine: &Engine) -> io::Result<()> {
    if dir.join("call.sock").exists() {
        return Ok(());
    }
    if !dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{} is gone; ending, so that the folder is made again as lamp-skills starts",
                dir.display()
            ),
        ));
    }
    listen(dir, asks.clone())?;
    write_tools(dir, engine);
    eprintln!(
        "lamp-skills: {} had no call.sock: made again",
        dir.display()
    );
    Ok(())
}

/// Listens on `dir/call.sock` as the actions part does: one line in
/// (`{"op":"call","name":...,"args":{...}}`), one line out, the main loop
/// answering.
fn listen(dir: &Path, asks: Waking<ToolAsk>) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let socket = dir.join("call.sock");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            // On the host's clock, as the socket timeouts to the other
            // parts are: the voice gives up on a call on its own.
            let answer = || -> io::Result<()> {
                stream.set_read_timeout(Some(TOOL_TIMEOUT))?;
                stream.set_write_timeout(Some(TOOL_TIMEOUT))?;
                let mut line = String::new();
                BufReader::new((&stream).take(8_192)).read_line(&mut line)?;
                let call: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
                let (reply, told) = mpsc::channel();
                let admission = Admission::default();
                let result = (call["op"] == "call")
                    .then(|| call["name"].as_str())
                    .flatten()
                    .filter(|name| {
                        asks.send((
                            (*name).to_owned(),
                            call["args"].clone(),
                            reply,
                            admission.clone(),
                        ))
                        .is_ok()
                    })
                    .and_then(|_| match told.recv_timeout(TOOL_TIMEOUT) {
                        Ok(result) => Some(result),
                        // Late: given up, unless the loop committed first,
                        // whose answer is then being sent.
                        Err(_) if admission.cancel() => None,
                        Err(_) => told.recv_timeout(TOOL_TIMEOUT).ok(),
                    });
                let line = match result {
                    Some(result) => match result.get("error") {
                        Some(error) => json!({"ok": false, "error": error}),
                        None => json!({"ok": true, "result": result}),
                    },
                    None => json!({"ok": false, "error": "My skills did not answer in time."}),
                };
                writeln!(&stream, "{line}")
            };
            if let Err(error) = answer() {
                eprintln!("lamp-skills: a tool call was not answered: {error}");
            }
        }
    });
    Ok(())
}

/// How the light was changed, to give it back.
#[derive(Default)]
struct Changed {
    coloured: bool,
    dimmed: u8,
    lasting_us: u64,
    /// The light as the run's steps left it, from the light shown before
    /// it: a light held until its trigger ends is given back only if it is
    /// still this.
    left: Option<Light>,
}

/// What the body was told in this run: until when its last clip plays, and
/// why no more are sent, once the body was pushed or the lamp paused.
#[derive(Default)]
struct Played {
    /// The run's last clip sent.
    last: Option<Sent>,
    stopped: Option<String>,
}

/// A clip sent to the body, for the next to wait on: when it was sent, its
/// length and `CLIP_GAP_US`, its name, the source it was sent with (the
/// body's 64 characters of `skill:<name>`, with `LAMP_BODY_REQUESTS=on`),
/// and `body/gesture`, subscribed to before it was sent.
struct Sent {
    at: Instant,
    rest: Duration,
    name: String,
    source: Option<String>,
    gestures: Option<Subscriber>,
}

impl Sent {
    /// When it has played, had the body begun it as it was sent.
    fn until(&self) -> Instant {
        self.at + self.rest
    }
}

/// Whether `gesture` is about the clip `name` sent with `source`: its
/// name, and no other asker's, when the body names whose it was (#940).
fn is_clip(gesture: &BodyGesture, name: &str, source: Option<&str>) -> bool {
    gesture.name == name
        && gesture
            .source
            .as_deref()
            .is_none_or(|said| Some(said) == source)
}

/// A clip of the run's dropped by the body, as `body/gesture` said it:
/// with `lamp-body --arbitration on`, why and who holds the body, and said
/// in the journal too (#940); without, the words they always were, and no
/// journal line (the reviewer on #1473: a product lamp's runs unchanged).
fn dropped(gesture: &BodyGesture) -> String {
    let Some(why) = &gesture.why else {
        return format!("the body dropped {} (body/gesture)", gesture.name);
    };
    let held = gesture
        .by
        .as_deref()
        .map_or(String::new(), |by| format!(", held by {by}"));
    let said = format!("the body dropped {} ({why}{held})", gesture.name);
    let who = gesture.source.as_deref().unwrap_or("a skill");
    eprintln!("lamp-skills: {who}: {said}");
    said
}

/// How long after a clip's length the next waits, at most, for the body to
/// say the clip is `over`: a gesture asked of a lamp still rising waits up
/// to 3 s for the rise (`crates/body/src/body.rs`, `GESTURE_WAIT`).
const OVER_WITHIN: Duration = Duration::from_secs(3);

/// `body/gesture`, subscribed to under a name of its own for each clip.
fn body_gestures(bus: &Bus) -> Option<Subscriber> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    bus.subscribe_typed(
        BodyGesture::TOPIC,
        &format!("skills_clip_{number}"),
        BodyGesture::TYPE,
    )
    .map_err(|error| eprintln!("lamp-skills: body/gesture not subscribed to: {error}"))
    .ok()
}

/// Runs one skill's calls and says how each went. With a say, the others
/// are made when the voice begins it, so the look and the light come with
/// the words, and none if it is never begun. Those after `Call::AfterSay`
/// are made when the say ends: the skill's own, or in a tool's run, the
/// line the model says. A light changed is given back when the say ends,
/// after its `for`, or when the last clip ends, whichever is last. With
/// `glows`, the light is asked of the ring instead, and given back the same
/// way ([`Glow`]).
fn perform(
    skill: &str,
    (calls, rest_permits): (&[Call], &[Permit]),
    sockets: &Sockets,
    bus: &Bus,
    before: Option<Light>,
    // How the light is asked, and, with `LAMP_BODY_REQUESTS=on`, that its
    // gestures and moves say they are this skill's (#940).
    (glows, asks_body): (Option<&Glows>, bool),
    // Told once the body and the light are made, and, for a dim held until
    // its trigger ends, told that end.
    (made, end): (mpsc::Sender<()>, Option<mpsc::Receiver<()>>),
) -> String {
    // A light the run gives back. One the person keeps is theirs (#1679).
    let is_light = |call: &Call| {
        matches!(
            call,
            Call::Light { keep: false, .. }
                | Call::Dim { .. }
                | Call::Out { .. }
                | Call::Show { .. }
        )
    };
    let mut said = Vec::new();
    let asker = asks_body.then_some(skill);
    // Asked of the ring, a light is never the person's to give back.
    let mut glow = glows
        .filter(|_| calls.iter().any(is_light))
        .map(|glows| Glow::new(glows, skill, before.clone()));
    if glow.is_none() && before.is_none() && calls.iter().any(is_light) {
        said.push(
            "light: left alone, since the light shown is not known and could not be given back"
                .into(),
        );
    }
    let (first, after) = match calls.iter().position(|call| *call == Call::AfterSay) {
        Some(at) => (&calls[..at], &calls[at + 1..]),
        None => (calls, &[][..]),
    };
    let keep = |calls: &[Call]| -> Vec<Call> {
        calls
            .iter()
            .filter(|call| {
                !matches!(
                    call,
                    Call::Say(_)
                        | Call::Words(_)
                        | Call::SayFrom(..)
                        | Call::Sound { .. }
                        | Call::Cue
                        | Call::Rest(_)
                ) && (glow.is_some() || before.is_some() || !is_light(call))
            })
            .cloned()
            .collect()
    };
    let (first, after) = (keep(first), keep(after));
    // What the voice is asked to say: a note in its own words, or words it
    // keeps itself (#939); or to play the skill's own sound (#938), which
    // it plays at once or refuses at once, with why.
    let asking = calls.iter().find_map(|call| match call {
        Call::Say(note) => Some((
            "say",
            json!({"note": note, "priority": lamp_skills::SAY_PRIORITY}),
        )),
        // Its own words as written, or its note, at its priority and
        // expiry (#1684): the text goes to the voice as it is in the file.
        Call::Words(words) => Some(("say", words.ask())),
        Call::SayFrom(source, _) => {
            // Share the item identity encoding with replay: task news names
            // its task; onboarding names its exact pending invitation.
            let (_, mut ask) = call.named();
            ask["priority"] = json!(source.priority());
            Some(("say", ask))
        }
        Call::Sound { path, .. } => Some(("sound", json!({"sound": path, "priority": 5}))),
        Call::Cue => Some(("sound", json!({"cue": "hum", "priority": 5}))),
        _ => None,
    });
    // A say or a sound the skill may not make is never asked, so it is
    // never begun, and nothing it brings is made either.
    if let Some(refused) = calls
        .iter()
        .filter(|call| {
            matches!(
                call,
                Call::Say(_) | Call::Words(_) | Call::SayFrom(..) | Call::Sound { .. } | Call::Cue
            )
        })
        .find_map(|call| sockets.may.refuses(call))
    {
        said.push(refused);
        return said.join("; ");
    }
    let mut changed = Changed {
        left: before.clone(),
        ..Changed::default()
    };
    let mut played = Played::default();
    match asking {
        Some((what, asking)) => {
            let mut begun = Vec::new();
            let mut was_begun = false;
            let outcome = say(&sockets.say, what, skill, asking, rest_permits, || {
                was_begun = true;
                act(
                    &first,
                    sockets,
                    bus,
                    asker,
                    &mut changed,
                    &mut played,
                    &mut glow,
                    &mut begun,
                );
                let _ = made.send(());
            });
            said.extend(begun);
            said.push(outcome);
            // Nothing after a say never begun, as nothing with it.
            if was_begun {
                act(
                    &after,
                    sockets,
                    bus,
                    asker,
                    &mut changed,
                    &mut played,
                    &mut glow,
                    &mut said,
                );
            }
        }
        None => {
            act(
                &first,
                sockets,
                bus,
                asker,
                &mut changed,
                &mut played,
                &mut glow,
                &mut said,
            );
            let _ = made.send(());
            if !after.is_empty() {
                let (how, cut) = spoken(bus);
                said.push(how);
                if cut {
                    // They spoke over the line: the lamp stops and listens.
                    said.push(
                        "the line was cut (the voice went from speaking to listening): nothing after it is made"
                            .into(),
                    );
                } else {
                    act(
                        &after,
                        sockets,
                        bus,
                        asker,
                        &mut changed,
                        &mut played,
                        &mut glow,
                        &mut said,
                    );
                }
            }
            wait_out(skill, calls, bus, changed.lasting_us, &mut said);
        }
    }
    if let Some(last) = played.last.as_mut() {
        clock::sleep(last.until().saturating_duration_since(clock::now()));
        // The last clip has no next to wait on it: one the body dropped or
        // ended meanwhile is said here, why and who holds the body (#940).
        // Only for a run that named itself to the body
        // (`LAMP_BODY_REQUESTS=on`) and a drop the body told
        // (`--arbitration on`): with both off, a run says what it always
        // did (the reviewer on #1473).
        let named = last.source.is_some();
        if let Some(gestures) = last.gestures.as_mut().filter(|_| named) {
            while let Ok(Some(message)) = gestures.recv(Duration::ZERO) {
                if let Ok(gesture) = message.parse::<BodyGesture>()
                    && gesture.state == GestureState::Dropped
                    && gesture.why.is_some()
                    && is_clip(&gesture, &last.name, last.source.as_deref())
                {
                    said.push(dropped(&gesture));
                    break;
                }
            }
        }
    }
    // A light held until its trigger ends (a dim, or a colour such as a
    // pale breath, #1681) waits for that end, however long, the ring's
    // request renewed meanwhile when it is asked of the ring.
    let held = end.filter(|_| {
        (changed.dimmed > 0 || changed.coloured) && (glow.is_some() || before.is_some())
    });
    let held_until_ends = held.is_some();
    if let Some(end) = held {
        let _ = end.recv();
        said.push("held until its trigger ended".into());
    }
    match glow {
        Some(glow) => said.extend(glow.give_back()),
        None => {
            if let Some(before) = before
                && (changed.coloured || changed.dimmed > 0)
            {
                // Held until its trigger ended: given back only if the
                // light is still as this run left it.
                // After a long hold, the newest light said, however old.
                let now = if held_until_ends {
                    bus.latest(Light::TOPICS[1])
                        .ok()
                        .flatten()
                        .and_then(|message| message.parse::<Light>().ok())
                } else {
                    light_now(bus)
                };
                let left = |now: &Light| {
                    changed.left.as_ref().is_some_and(|left| {
                        now.on && now.colour == left.colour && now.level == left.level
                    })
                };
                if held_until_ends && !now.as_ref().is_some_and(left) {
                    said.push("light: changed meanwhile or not known, so left as it is".into());
                } else {
                    said.push(give_back(&before, now, sockets));
                }
            }
        }
    }
    said.join("; ")
}

/// Makes `calls` in order. A clip after another waits for that one to end,
/// and is not sent, nor any after it, if meanwhile the body was pushed or
/// the lamp paused: the body's own stops hold for the whole run.
#[allow(clippy::too_many_arguments)]
fn act(
    calls: &[Call],
    sockets: &Sockets,
    bus: &Bus,
    asker: Option<&str>,
    changed: &mut Changed,
    played: &mut Played,
    glow: &mut Option<Glow>,
    said: &mut Vec<String>,
) {
    for call in calls {
        // Not made, nor counted as made: nothing to wait on or give back.
        if let Some(refused) = sockets.may.refuses(call) {
            said.push(refused);
            continue;
        }
        match call {
            // The person's light, set as they asked: nothing to give back,
            // and through the actions part, never the ring's skill light.
            Call::Light { keep: true, .. } => {}
            // Each frame in turn, then given back as any light the run made.
            Call::Show { name, frames } => {
                changed.coloured = true;
                said.push(show(name, frames, sockets, bus, glow));
                continue;
            }
            Call::Light { lasting, .. } => {
                changed.left = lamp_skills::glow::asked(call, changed.left.as_ref());
                changed.coloured = true;
                changed.lasting_us = changed.lasting_us.max(
                    lasting
                        .as_deref()
                        .and_then(|text| duration_us(text).ok())
                        .unwrap_or(0),
                );
            }
            Call::Dim { steps, .. } => {
                if let Some(dimmed) = lamp_skills::glow::asked(call, changed.left.as_ref()) {
                    changed.left = Some(dimmed);
                }
                changed.dimmed += steps;
            }
            // Only the ring puts the light out (#1677): what it holds is
            // given back by asking again, as any light of the ring's.
            Call::Out { .. } if glow.is_some() => changed.dimmed += 1,
            // The run lasts while the pose is held.
            Call::Mood { lasting_us, .. } => {
                changed.lasting_us = changed.lasting_us.max(*lasting_us);
            }
            Call::Gesture(_) | Call::Move(_) => {
                let (what, name, length_us) = match call {
                    Call::Gesture(name) => (format!("gesture {name}"), name.clone(), clip_us(name)),
                    Call::Move(clip) => (
                        format!("move {}", clip.name),
                        clip.name.clone(),
                        Some(
                            (clip.frames.len().saturating_sub(1) as u64)
                                * u64::from(clip.step_ms)
                                * 1_000,
                        ),
                    ),
                    _ => unreachable!(),
                };
                if let Some(mut last) = played.last.take()
                    && played.stopped.is_none()
                {
                    played.stopped = held_back(&mut last, bus);
                }
                if let Some(why) = &played.stopped {
                    said.push(format!("{what}: not sent, {why}"));
                    continue;
                }
                // The next clip waits for the body to say this one is over.
                played.last = length_us.map(|us| Sent {
                    gestures: body_gestures(bus),
                    at: clock::now(),
                    rest: Duration::from_micros(us + CLIP_GAP_US),
                    name,
                    // As lamp-body keeps it: 64 characters at most.
                    source: asker.map(|skill| format!("skill:{skill}").chars().take(64).collect()),
                });
            }
            _ => {}
        }
        said.push(match glow {
            Some(glow)
                if matches!(
                    call,
                    Call::Light { keep: false, .. } | Call::Dim { .. } | Call::Out { .. }
                ) =>
            {
                glow.ask(call)
            }
            _ => make(call, sockets, bus, asker),
        });
    }
}

/// A light show (#1679): each frame's light in turn, asked of the ring with
/// `LAMP_LIGHT_REQUESTS=on` or set through the actions part, held its while.
/// It stops before a frame once the body was pushed, the lamp paused, or the
/// voice took a turn (someone spoke: the lamp stops and listens); what it
/// showed is given back with the run's light.
fn show(
    name: &str,
    frames: &[lamp_skills::Frame],
    sockets: &Sockets,
    bus: &Bus,
    glow: &mut Option<Glow>,
) -> String {
    let mut shown = 0;
    for frame in frames {
        if let Some(why) = show_stopped(bus) {
            return format!(
                "show {name}: stopped after {shown} of {} frames, {why}",
                frames.len()
            );
        }
        let light = Call::Light {
            colour: frame.colour.clone(),
            level: Some(frame.level),
            lasting: None,
            pattern: frame.pattern,
            keep: false,
            until_ends: false,
        };
        let made = match glow {
            Some(glow) => glow.ask(&light),
            None => make(&light, sockets, bus, None),
        };
        if shown == 0 {
            eprintln!(
                "lamp-skills: skill:{}: show {name}: {made}",
                sockets.may.skill
            );
        }
        shown += 1;
        // Held its while, looked at every fifth of a second: someone who
        // speaks stops the show within that, not at the frame's end.
        let until = clock::now() + Duration::from_micros(frame.lasting_us);
        while clock::now() < until {
            if let Some(why) = show_stopped(bus) {
                return format!(
                    "show {name}: stopped in frame {shown} of {}, {why}",
                    frames.len()
                );
            }
            clock::sleep(SHOW_LOOKS.min(until.saturating_duration_since(clock::now())));
        }
    }
    format!("show {name}: {shown} frames shown")
}

/// How often a show looks whether it should stop, and a search's pose
/// whether someone speaks.
const SHOW_LOOKS: Duration = Duration::from_millis(200);

/// The run's while, slept out. A search's (its pose held until someone
/// speaks, #1685) ends within a fifth of a second of the voice listening to
/// someone: its pose is let go, and the light then given back.
fn wait_out(skill: &str, calls: &[Call], bus: &Bus, lasting_us: u64, said: &mut Vec<String>) {
    let until_spoken = calls.iter().any(|call| {
        matches!(
            call,
            Call::Mood {
                until_spoken: true,
                ..
            }
        )
    });
    if !until_spoken {
        clock::sleep(Duration::from_micros(lasting_us));
        return;
    }
    let until = clock::now() + Duration::from_micros(lasting_us);
    let mut stop = SearchStop::new(bus);
    while clock::now() < until {
        // Words heard, the lamp paused or the body pushed ends it (the
        // peer read on #1766; a line of no words does not, #1871).
        if let Some(why) = stop.why(bus) {
            end_hold(skill);
            said.push(format!("{why}: the search ended"));
            return;
        }
        clock::sleep(SHOW_LOOKS.min(until.saturating_duration_since(clock::now())));
    }
}

/// The newest turn heard on `voice/heard`, as the main loop read it: the
/// bus keeps no latest of an event topic. Read before each call (#2179).
static NEWEST_HEARD: Mutex<Option<u64>> = Mutex::new(None);

/// What ends a search (#1685): the body pushed, the lamp paused, or words
/// heard. The voice listening is not enough: on lamp-52e6 a line of office
/// noise, 0 words heard, ended a search after its first look (Fifth's row 3,
/// #1871). Words of the turn that asked for the search, or an earlier one,
/// do not end it: on Live the last pieces of the asking line can come after
/// the call. Without `voice/heard` it ends as a show does, on the voice
/// listening.
struct SearchStop {
    heard: Option<Subscriber>,
    /// The turn of the newest words heard as the search began.
    asked: Option<u64>,
}

impl SearchStop {
    fn new(bus: &Bus) -> Self {
        static SEARCHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEARCHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let heard = bus
            .subscribe(Speech::TOPICS[0], &format!("skills_search_{n}"))
            .map_err(|error| eprintln!("lamp-skills: voice/heard not read for a search: {error}"))
            .ok();
        let asked = *NEWEST_HEARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self { heard, asked }
    }

    fn why(&mut self, bus: &Bus) -> Option<String> {
        if let Some(why) = body_stopped(bus) {
            return Some(why);
        }
        let Some(heard) = &mut self.heard else {
            return voice_listening(bus).then(|| "the voice is listening to someone".into());
        };
        let asked = self.asked;
        let later = |speech: &Speech| match (speech.request, asked) {
            (Some(turn), Some(asked)) => turn > asked,
            _ => true,
        };
        while let Some(message) = receive(heard, Duration::ZERO) {
            if message.parse::<Speech>().is_ok_and(|speech| {
                later(&speech) && speech.text.chars().any(char::is_alphanumeric)
            }) {
                return Some("someone spoke to the lamp".into());
            }
        }
        None
    }
}

/// Why a show goes no further, if it should not: the body pushed, the lamp
/// paused, or the voice listening to someone.
fn show_stopped(bus: &Bus) -> Option<String> {
    body_stopped(bus)
        .or_else(|| voice_listening(bus).then(|| "the voice is listening to someone".into()))
}

/// The body pushed or the lamp paused.
fn body_stopped(bus: &Bus) -> Option<String> {
    let latest = |topic: &str| bus.latest(topic).ok().flatten();
    if latest(Hand::TOPIC)
        .and_then(|message| message.parse::<Hand>().ok())
        .is_some_and(|hand| hand.pushed)
    {
        return Some("the body was pushed".into());
    }
    if latest(Pause::TOPIC)
        .and_then(|message| message.parse::<Pause>().ok())
        .is_some_and(|pause| pause.paused)
    {
        return Some("the lamp was paused".into());
    }
    None
}

/// Whether the voice is listening to someone now: a person speaks.
fn voice_listening(bus: &Bus) -> bool {
    said_lately::<VoiceState>(bus, VoiceState::TOPIC).map(|now| now.state) == Some(Voice::Listening)
}

/// Waits while the clip `sent` plays, until the body says on `body/gesture`
/// that it is `over`: then the body takes the next. When it began is not
/// known from the send (a standing lamp began one 280–350 ms after it, a
/// rising one later still) nor from `body/joint_states` (the lean as the
/// lamp speaks moves them too), lamp-06c1's asks 19, 22 and 23 (#1199). Why
/// the next is not sent: the body said it `dropped` the clip, said no
/// `over` within its length and `OVER_WITHIN`, or `body/gesture` could not
/// be read; or the body was pushed or the lamp paused meanwhile.
fn held_back(sent: &mut Sent, bus: &Bus) -> Option<String> {
    // The body's own stops hold for the whole run: a clip the body ended
    // because it was pushed or paused says `over` too (#1230), and no more
    // is sent after it.
    let stopped = || {
        if bus
            .latest(Hand::TOPIC)
            .ok()
            .flatten()
            .and_then(|message| message.parse::<Hand>().ok())
            .is_some_and(|hand| hand.pushed)
        {
            return Some("the body was pushed".to_owned());
        }
        bus.latest(Pause::TOPIC)
            .ok()
            .flatten()
            .and_then(|message| message.parse::<Pause>().ok())
            .is_some_and(|pause| pause.paused)
            .then(|| "the lamp was paused".to_owned())
    };
    let Some(gestures) = sent.gestures.as_mut() else {
        return Some(
            "body/gesture could not be read, so when the clip before ended is not known".into(),
        );
    };
    let within = sent.rest + OVER_WITHIN;
    loop {
        if let Some(why) = stopped() {
            return Some(why);
        }
        while let Ok(Some(message)) = gestures.recv(Duration::ZERO) {
            let Ok(gesture) = message.parse::<BodyGesture>() else {
                continue;
            };
            if !is_clip(&gesture, &sent.name, sent.source.as_deref()) {
                continue;
            }
            match gesture.state {
                GestureState::Over => return stopped(),
                GestureState::Dropped => {
                    return Some(format!("{}, so nothing more is sent", dropped(&gesture)));
                }
                GestureState::Began => {}
            }
        }
        if clock::now().saturating_duration_since(sent.at) >= within {
            return Some(format!(
                "the body said no over for {} on body/gesture within {} ms of its send",
                sent.name,
                within.as_millis()
            ));
        }
        clock::sleep(POLL);
    }
}

/// Waits for the model's line after a tool's call: for the voice to begin
/// speaking, at most SPEAK_BEGIN, and then for its turn to end: `resting` or
/// `listening`. `thinking` after `speaking` is still the turn, since the
/// sound plays on (lamp-06c1, 2026-10-16: `speaking` for 20 ms, then
/// `thinking` for 15 s, through a line of about 2 s). So the wait ends at
/// the latest LINE_LONGEST after the speaking began, unless the voice is
/// speaking then. At most SAY_TIMEOUT in all. What it waited for, in words,
/// and whether the line was cut: a line said to its end leaves the voice
/// `resting`, one the person spoke over leaves it `listening`.
fn spoken(bus: &Bus) -> (String, bool) {
    let state = || said_lately::<VoiceState>(bus, VoiceState::TOPIC).map(|now| now.state);
    let asked = clock::now();
    let elapsed = |since: Instant| clock::now().saturating_duration_since(since);
    loop {
        match state() {
            Some(Voice::Speaking) => break,
            None => {
                return (
                    "after the say: the voice's state is not known, so at once".into(),
                    false,
                );
            }
            Some(_) if elapsed(asked) >= SPEAK_BEGIN => {
                return (
                    format!(
                        "after the say: the voice did not begin within {} s",
                        SPEAK_BEGIN.as_secs()
                    ),
                    false,
                );
            }
            Some(_) => clock::sleep(POLL),
        }
    }
    let begun = clock::now();
    let cut = loop {
        let now = state();
        let over = match now {
            Some(Voice::Speaking) => false,
            Some(Voice::Thinking) => elapsed(begun) >= LINE_LONGEST,
            _ => true,
        };
        if over {
            break now == Some(Voice::Listening);
        }
        if elapsed(begun) >= SAY_TIMEOUT {
            return ("after the say: the voice was still speaking".into(), false);
        }
        clock::sleep(POLL);
    };
    (
        format!(
            "after the say: spoken from {} ms after the call, the turn over {} ms later",
            (begun - asked).as_millis(),
            elapsed(begun).as_millis()
        ),
        cut,
    )
}

/// The light as it was before the run, from the light shown now: off, or
/// its colour again and its level stepped back. With the light now not
/// known, its colour only.
fn give_back(before: &Light, now: Option<Light>, sockets: &Sockets) -> String {
    let call = |name: &str, args: serde_json::Value| answer(action(sockets, name, args));
    let mut done = Vec::new();
    if !before.on {
        if now.as_ref().is_none_or(|now| now.on) {
            done.push(call("light_off", json!({})));
        }
    } else {
        if now
            .as_ref()
            .is_none_or(|now| !now.on || now.colour != before.colour)
        {
            done.push(call("light_colour", json!({ "colour": before.colour })));
        }
        match &now {
            Some(now) => done.extend(step(now.level, before.level, sockets)),
            None => done.push("its level is not known, so it is left".into()),
        }
    }
    format!(
        "light given back to {} {} at {}: {}",
        if before.on { "on" } else { "off" },
        before.colour,
        before.level,
        if done.is_empty() {
            "it already was".into()
        } else {
            done.join(", ")
        }
    )
}

/// Steps the light from level `from` to `to`, a `light_brighter` or
/// `light_dimmer` call a level, and says how each went.
fn step(from: u8, to: u8, sockets: &Sockets) -> Vec<String> {
    let (name, steps) = if from < to {
        ("light_brighter", to - from)
    } else {
        ("light_dimmer", from - to)
    };
    (0..steps)
        .map(|_| answer(action(sockets, name, json!({}))))
        .collect()
}

fn answer(result: io::Result<String>) -> String {
    result.unwrap_or_else(|error| format!("the actions part was not reached ({error})"))
}

/// Asks the voice to say `asking`, a `note` or what it keeps `from`, or to
/// play a `sound`, with its `priority` (#939's say service: one line in, a
/// `begun` line when the turn starts, then the outcome). `begun` is called
/// with the first. Rest permits bound connection and carry their original
/// deadline to the voice. What became of it, in words, after `what`.
fn say(
    path: &Path,
    what: &str,
    skill: &str,
    asking: serde_json::Value,
    rest_permits: &[Permit],
    mut begun: impl FnMut(),
) -> String {
    let start_before_us = rest_permits.iter().map(Permit::expires_us).min();
    let mut attempt = || -> io::Result<String> {
        let mut stream = if let Some(expires_us) = start_before_us {
            // Sample Instant first so conversion cannot extend the original
            // shared-clock deadline. Only permit-bearing runs use this path.
            let began = Instant::now();
            let left_us = expires_us
                .checked_sub(lamp_ipc::monotonic_us())
                .filter(|left| *left > 0)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "rest transition confirmation expired",
                    )
                })?;
            let stream =
                lamp_schedule::client::connect(path, began + Duration::from_micros(left_us))
                    .map_err(io::Error::other)?;
            stream.set_nonblocking(false)?;
            stream
        } else {
            UnixStream::connect(path)?
        };
        stream.set_read_timeout(Some(SAY_TIMEOUT))?;
        stream.set_write_timeout(Some(ACTIONS_TIMEOUT))?;
        let mut ask = asking.clone();
        ask["skill"] = json!(skill);
        if ask.get("expires_in_ms").is_none() {
            ask["expires_in_ms"] = json!(lamp_skills::SAY_EXPIRES_MS);
        }
        if let Some(expires_us) = start_before_us {
            ask["start_before_us"] = json!(expires_us);
        }
        // A blocked connect or a reload cannot turn a stale confirmation
        // into a new request. Voice also checks the deadline after transport.
        rest_permits
            .iter()
            .try_for_each(Permit::check)
            .map_err(io::Error::other)?;
        writeln!(stream, "{ask}")?;
        let mut lines = BufReader::new(stream.take(64 * 1_024));
        loop {
            let mut line = String::new();
            if lines.read_line(&mut line)? == 0 {
                return Ok("the voice ended without an outcome".into());
            }
            let answer: serde_json::Value = serde_json::from_str(&line)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            if answer["event"] == "begun" {
                begun();
            } else if let Some(outcome) = answer["outcome"].as_str() {
                return Ok(match answer["why"].as_str() {
                    Some(why) => format!("{outcome} ({why})"),
                    None => outcome.to_owned(),
                });
            }
        }
    };
    match attempt() {
        Ok(outcome) => format!("{what}: {outcome}"),
        Err(error) => format!("{what}: the voice's say service was not reached ({error})"),
    }
}

/// Makes one call and says how it went. A gesture or a move says it is
/// `asker`'s, a skill's name, with `LAMP_BODY_REQUESTS=on` (#940).
fn make(call: &Call, sockets: &Sockets, bus: &Bus, asker: Option<&str>) -> String {
    let tell = |what: &str, text: String, path: &Path| {
        let sent = UnixDatagram::unbound().and_then(|body| body.send_to(text.as_bytes(), path));
        match sent {
            Ok(_) => format!("{what}: told the body"),
            Err(error) => format!("{what}: the body was not reached ({error})"),
        }
    };
    let actions =
        |what: &str, name: &str, args: serde_json::Value| match action(sockets, name, args) {
            Ok(answer) => format!("{what}: {answer}"),
            Err(error) => format!("{what}: the actions part was not reached ({error})"),
        };
    match call {
        Call::Gaze { face: None } => "look at the face: no face in view".into(),
        Call::Gaze {
            face: Some((x, y, size)),
        } => tell(
            "look at the face",
            json!({"face": {"x": x, "y": y, "size": size, "turned": 0.0}}).to_string(),
            &sockets.gaze,
        ),
        Call::Gesture(name) => tell(
            &format!("gesture {name}"),
            body_ask(json!({ "gesture": name }), asker),
            &sockets.mood,
        ),
        Call::Move(clip) => tell(
            &format!("move {}", clip.name),
            body_ask(json!({ "move": clip }), asker),
            &sockets.mood,
        ),
        Call::Light {
            colour,
            level,
            pattern,
            keep,
            ..
        } => {
            let what = if *keep {
                format!("light {colour}, the person's, kept")
            } else {
                format!("light {colour}")
            };
            let mut said = actions(&what, "light_colour", json!({ "colour": colour }));
            // No call sets a level: it is stepped to, from the level shown.
            if let Some(level) = level {
                match light_now(bus) {
                    Some(now) if now.level == *level => {}
                    Some(now) => {
                        said.push_str(&format!(", to level {level}: "));
                        said.push_str(&step(now.level, *level, sockets).join(", "));
                    }
                    None => said.push_str(", its level is not known, so it is left"),
                }
            }
            // A pattern is the ring's (#940 item 3): through the actions part
            // the light is steady.
            if pattern.is_some() {
                said.push_str(&format!(
                    ", steady: its pattern needs the ring's {REQUESTS_ENVIRONMENT}=on"
                ));
            }
            said
        }
        // Given back by `perform`: when the run ends, or with
        // `until = "ends"` when its trigger stops holding.
        Call::Dim { steps, .. } => (0..*steps)
            .map(|_| actions("dim", "light_dimmer", json!({})))
            .collect::<Vec<_>>()
            .join(", "),
        // Through the actions part it would be the person's light turned
        // off, and kept so across a restart: only the ring is asked (#1677).
        Call::Out { .. } => format!(
            "light out: left alone, since only the ring puts it out ({REQUESTS_ENVIRONMENT}=on)"
        ),
        Call::Mood {
            mood,
            lasting_us,
            toward,
            until_spoken,
        } => {
            // Said again every second, as the body needs it at least every
            // 1.5 s (#1011); when it stops being said, the body comes back.
            // A newer pose of the same skill ends this hold (#1679).
            let mut ask = json!({ "mood": mood, "level": 0 });
            if let Some(toward) = toward {
                ask["toward"] = json!(toward);
            }
            let (path, text) = (sockets.mood.clone(), ask.to_string());
            let skill = sockets.may.skill.clone();
            let number = new_hold(&skill);
            let what = match toward {
                Some(toward) => format!("mood {mood} toward {toward}"),
                None => format!("mood {mood}"),
            };
            let first = tell(&what, text.clone(), &path);
            let until = clock::now() + Duration::from_micros(*lasting_us);
            // A search's pose looks every fifth of a second and is let go
            // once someone speaks, the lamp is paused or the body pushed
            // (#1685); any other, each second.
            let (until_spoken, bus) = (*until_spoken, bus.clone());
            let looks = if until_spoken { SHOW_LOOKS } else { MOOD_EVERY };
            std::thread::spawn(move || {
                let mut stop = until_spoken.then(|| SearchStop::new(&bus));
                let mut next = clock::now() + MOOD_EVERY;
                while next < until {
                    clock::sleep(looks.min(next.saturating_duration_since(clock::now())));
                    if !holding(&skill, number) {
                        break;
                    }
                    if stop.as_mut().and_then(|stop| stop.why(&bus)).is_some() {
                        end_hold(&skill);
                        break;
                    }
                    if clock::now() >= next {
                        let _ = UnixDatagram::unbound()
                            .and_then(|body| body.send_to(text.as_bytes(), &path));
                        next += MOOD_EVERY;
                    }
                }
            });
            format!("{first}, held {} s", lasting_us / 1_000_000)
        }
        Call::Show { name, .. } => format!("show {name}: made by its own step"),
        Call::Say(_) | Call::Words(_) | Call::SayFrom(..) => "say: made by its own step".into(),
        Call::Sound { .. } => "sound: made by its own step".into(),
        Call::Cue => "cue: made by its own step".into(),
        Call::Rest(_) => "rest: admitted by Expression before presentation".into(),
        Call::Look => "look: the voice's, asked in the call's answer".into(),
        Call::AfterSay => "after the say".into(),
        Call::Text(_) => "text: a watch's, sent by the watch".into(),
        Call::Ends => "its trigger ended".into(),
    }
}

/// One action on the actions part's socket: one line each way. Only one the
/// run's skill may call: the light's with `light = true`, any other only as
/// its `[permissions]` actions name it.
fn action(sockets: &Sockets, name: &str, args: serde_json::Value) -> io::Result<String> {
    if let Some(refused) = sockets.may.refuses_action(name) {
        return Ok(refused);
    }
    let mut stream = UnixStream::connect(&sockets.actions)?;
    stream.set_read_timeout(Some(ACTIONS_TIMEOUT))?;
    stream.set_write_timeout(Some(ACTIONS_TIMEOUT))?;
    let call = json!({"op": "call", "name": name, "args": args});
    writeln!(stream, "{call}")?;
    let mut line = String::new();
    BufReader::new(stream.take(8_193)).read_line(&mut line)?;
    let answer: serde_json::Value = serde_json::from_str(&line)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(if answer["ok"] == true {
        "done".into()
    } else {
        answer["error"].as_str().unwrap_or("refused").to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock_fixture(windowed: bool, enabled: bool) -> lamp_skills::Skill {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("clock-check");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            "---\nname: clock-check\ndescription: Check clock admission.\n---\n",
        )
        .unwrap();
        let during = if windowed {
            "during = { from = \"06:00\", to = \"10:00\" }"
        } else {
            ""
        };
        std::fs::write(folder.join("skill.toml"), format!(
            "enabled = {enabled}\n[permissions]\ntopics = [\"fixture/state\"]\nbody = true\n[trigger]\ntopic = \"fixture/state\"\nfield = \"ready\"\nequals = true\n{during}\n[ask]\npolicy = \"nudge\"\n[[do]]\nbody = {{ gesture = \"stretching\" }}\n"
        )).unwrap();
        lamp_skills::load(&folder).unwrap()
    }

    #[test]
    fn ordinary_and_disabled_skills_do_not_query_kernel_clock_trust() {
        let local = lamp_local_time::LocalTime::Offset(0);
        let wall = 1_767_225_600_000 + 6 * 3_600_000;
        for (windowed, enabled) in [(false, true), (true, false)] {
            let mut engine = Engine::new(vec![clock_fixture(windowed, enabled)], true);
            assert_eq!(
                update_clock(&mut engine, &local, 0, wall, false, || panic!(
                    "unexpected kernel query"
                )),
                None
            );
            let runs = engine.observe("fixture/state", 0, br#"{"ready":true}"#, None);
            assert_eq!(
                runs.iter().map(|run| run.calls.len()).sum::<usize>(),
                usize::from(enabled)
            );
        }
    }

    #[test]
    fn live_scheduled_time_requires_kernel_trust_or_an_explicit_override() {
        let local = lamp_local_time::LocalTime::Offset(0);
        let wall = 1_767_225_600_000 + 6 * 3_600_000;
        for synchronized in [None, Some(false), Some(true)] {
            let mut engine = Engine::new(vec![clock_fixture(true, true)], true);
            let sample = update_clock(&mut engine, &local, 0, wall, false, || synchronized);
            assert_eq!(sample.is_some(), synchronized == Some(true));
            let runs = engine.observe("fixture/state", 0, br#"{"ready":true}"#, None);
            assert_eq!(
                runs.iter().map(|run| run.calls.len()).sum::<usize>(),
                usize::from(synchronized == Some(true))
            );
        }
        let mut engine = Engine::new(vec![clock_fixture(true, true)], true);
        assert_eq!(
            update_clock(&mut engine, &local, 0, wall, true, || panic!(
                "explicit override needs no kernel query"
            )),
            Some(wall)
        );
        assert_eq!(
            engine
                .observe("fixture/state", 0, br#"{"ready":true}"#, None)
                .len(),
            1
        );
        assert_eq!(
            update_clock(&mut engine, &local, 1, 0, true, || panic!(
                "explicit override needs no kernel query"
            )),
            None
        );
    }

    #[test]
    fn clock_warning_tracks_enabled_schedules_without_repeating_each_tick() {
        let local = lamp_local_time::LocalTime::Offset(0);
        let wall = 1_767_225_600_000 + 6 * 3_600_000;
        let waiting = Cell::new(false);
        for (windowed, enabled) in [(false, true), (true, false)] {
            let engine = Engine::new(vec![clock_fixture(windowed, enabled)], true);
            assert_eq!(clock_diagnostic(&engine, None, &waiting), None);
        }

        let mut engine = Engine::new(vec![clock_fixture(true, true)], true);
        // A configured schedule with no local clock has the same visible
        // reason to wait as a configured zone whose clock is untrusted.
        assert!(clock_diagnostic(&engine, None, &waiting).is_some());
        for synchronized in [None, Some(false), None] {
            let trusted = update_clock(&mut engine, &local, 0, wall, false, || synchronized);
            assert_eq!(clock_diagnostic(&engine, trusted, &waiting), None);
            assert!(
                engine
                    .observe("fixture/state", 0, br#"{"ready":true}"#, None)
                    .is_empty()
            );
        }

        let trusted = update_clock(&mut engine, &local, 1, wall, false, || Some(true));
        assert_eq!(clock_diagnostic(&engine, trusted, &waiting), None);
        let trusted = update_clock(&mut engine, &local, 2, wall, false, || Some(false));
        assert!(clock_diagnostic(&engine, trusted, &waiting).is_some());
        assert_eq!(clock_diagnostic(&engine, trusted, &waiting), None);

        let disabled = Engine::new(vec![clock_fixture(true, false)], true);
        assert_eq!(clock_diagnostic(&disabled, None, &waiting), None);
        assert!(clock_diagnostic(&engine, None, &waiting).is_some());
        assert_eq!(clock_diagnostic(&engine, None, &waiting), None);
    }

    /// Sockets where nothing answers, for a run of a skill that may use
    /// `permissions`: a call the gate lets through fails to connect.
    fn nowhere(dir: &Path, permissions: Permissions) -> Sockets {
        Sockets {
            gaze: dir.join("gaze.sock"),
            mood: dir.join("mood.sock"),
            actions: dir.join("actions.sock"),
            say: dir.join("say.sock"),
            may: May {
                skill: "test".into(),
                permissions,
            },
        }
    }

    fn light(colour: &str) -> Call {
        Call::Light {
            colour: colour.into(),
            level: None,
            lasting: Some("4s".into()),
            pattern: None,
            keep: false,
            until_ends: false,
        }
    }

    /// The actions part is called only for what the skill may call: the
    /// light's functions with `light`, any other only by name.
    #[test]
    fn the_actions_part_is_called_only_for_what_the_skill_may_call() {
        let dir = tempfile::tempdir().unwrap();
        let none = nowhere(dir.path(), Permissions::default());
        let refused = action(&none, "light_colour", json!({})).unwrap();
        assert!(refused.contains("light_colour: refused"), "{refused}");
        let lit = nowhere(
            dir.path(),
            Permissions {
                light: true,
                ..Permissions::default()
            },
        );
        // Let through, to an actions part that is not there.
        assert!(action(&lit, "light_colour", json!({})).is_err());
        let refused = action(&lit, "volume_up", json!({})).unwrap();
        assert!(
            refused.contains("not in [permissions] actions"),
            "{refused}"
        );
    }

    /// A step the skill may not make is not sent, and not counted as made:
    /// no clip to wait on and no light to give back.
    #[test]
    fn a_step_the_skill_may_not_make_is_not_sent_nor_counted() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Bus::new(dir.path().join("bus")).unwrap();
        let calls = [
            Call::Gesture("nod".into()),
            light("warm"),
            Call::Dim {
                steps: 1,
                until_ends: false,
            },
            Call::Mood {
                mood: "reading".into(),
                lasting_us: 3_000_000,
                toward: None,
                until_spoken: false,
            },
        ];
        let (mut changed, mut played, mut glow, mut said) =
            (Changed::default(), Played::default(), None, Vec::new());
        let none = nowhere(dir.path(), Permissions::default());
        act(
            &calls,
            &none,
            &bus,
            None,
            &mut changed,
            &mut played,
            &mut glow,
            &mut said,
        );
        assert_eq!(said.len(), 4, "{said:?}");
        assert!(said.iter().all(|line| line.contains("refused")), "{said:?}");
        assert!(!changed.coloured && changed.dimmed == 0 && changed.lasting_us == 0);
        assert!(played.last.is_none());
        // With the body given, the gesture goes (to a body that is not
        // there) and the light is still refused.
        let moved = nowhere(
            dir.path(),
            Permissions {
                body: true,
                ..Permissions::default()
            },
        );
        let mut said = Vec::new();
        act(
            &calls[..2],
            &moved,
            &bus,
            None,
            &mut changed,
            &mut played,
            &mut glow,
            &mut said,
        );
        assert!(
            said[0].starts_with("gesture nod: the body was not reached"),
            "{said:?}"
        );
        assert!(said[1].contains("light: refused"), "{said:?}");
        assert!(!changed.coloured);
    }

    /// A say the skill may not make is never asked of the voice, so nothing
    /// that comes with it is made either.
    #[test]
    fn a_say_the_skill_may_not_make_is_never_asked_and_nothing_comes_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Bus::new(dir.path().join("bus")).unwrap();
        let moved = nowhere(
            dir.path(),
            Permissions {
                body: true,
                ..Permissions::default()
            },
        );
        for speech in [
            Call::Say("Hello.".into()),
            Call::Words(lamp_skills::Words {
                words: "Hello.".into(),
                exact: true,
                priority: 8,
                expires_ms: 3_600_000,
            }),
            Call::Words(lamp_skills::Words {
                words: "Greet them briefly.".into(),
                exact: false,
                priority: 8,
                expires_ms: 3_600_000,
            }),
            Call::Cue,
        ] {
            let (made, told) = mpsc::channel();
            let said = perform(
                "test",
                (&[speech, Call::Gesture("nod".into())], &[]),
                &moved,
                &bus,
                None,
                (None, false),
                (made, None),
            );
            assert!(said.contains("refused") && said.contains("speak"), "{said}");
            assert!(!said.contains("gesture"), "{said}");
            // Never told that its body was made: the sender is gone.
            assert!(told.recv().is_err());
        }
    }

    #[test]
    fn a_forged_rest_plan_is_refused_before_any_rest_or_say_request() {
        use lamp_msgs::ExpressionRestOp as Op;
        use lamp_skills::{daily::Window, routine};

        for (body, speak, refused) in [
            (false, true, "body"),
            (true, false, "speak"),
            (true, true, "light"),
        ] {
            for op in [Op::Rest, Op::Wake, Op::Cancel] {
                let root = tempfile::Builder::new()
                    .prefix("rest-permission-")
                    .tempdir_in("/tmp")
                    .unwrap();
                let bus = Bus::new(root.path().join("bus")).unwrap();
                let (mut manager, rest_client) =
                    routine::Manager::new(bus.clone(), "permission-host".into()).unwrap();
                manager.reload(vec!["test".into()], lamp_ipc::monotonic_us());
                let mut requests = bus
                    .subscribe(lamp_msgs::ExpressionRestRequest::TOPIC, "permission_probe")
                    .unwrap();
                let mut out = bus
                    .publisher_declared(SkillEvent::TOPIC, &SkillEvent::declaration(), "skills")
                    .unwrap();
                let say = UnixListener::bind(root.path().join("say.sock")).unwrap();
                say.set_nonblocking(true).unwrap();
                let (_woken, waker) = wake_pipe().unwrap();
                let (finished, done) = waking(&Arc::new(waker));
                let runner = Runner {
                    sockets: nowhere(root.path(), Permissions::default()),
                    finished,
                    bus,
                    glows: None,
                    body_requests: false,
                    ends: RefCell::default(),
                    local_time: None,
                    trust_clock: false,
                    clock_waiting: Cell::new(false),
                    rest_client: Some(rest_client),
                    permissions: RefCell::new(HashMap::from([
                        (
                            "test".into(),
                            Permissions {
                                body,
                                speak,
                                ..Permissions::default()
                            },
                        ),
                        (
                            "another".into(),
                            Permissions {
                                body: true,
                                speak: true,
                                ..Permissions::default()
                            },
                        ),
                    ])),
                    looks: RefCell::default(),
                    watching: RefCell::new(Watching {
                        watches: Watches::default(),
                        marks: HashMap::new(),
                        texts: None,
                        known_file: None,
                        signs: None,
                        retaken: HashMap::new(),
                    }),
                };
                let told = start(
                    "test".into(),
                    vec![
                        Call::Rest(routine::Rest {
                            op,
                            during: (op == Op::Rest)
                                .then(|| Window::parse("23:00", "10:00").unwrap()),
                        }),
                        Call::Cue,
                        light("warm"),
                    ],
                    &mut HashSet::new(),
                    &mut out,
                    &runner,
                );
                // No manager tick is needed to refuse this plan: permission
                // failure precedes sending even its first Rest request.
                let event = done.recv_timeout(Duration::from_secs(2)).unwrap();
                assert_eq!(event.allowed, Some(false), "{event:?}");
                assert!(event.why.contains("refused") && event.why.contains(refused));
                assert!(told.recv().is_err());
                let now = lamp_ipc::monotonic_us();
                manager.tick(now, now, None, 0);
                assert!(requests.recv(Duration::ZERO).unwrap().is_none());
                assert_eq!(say.accept().unwrap_err().kind(), io::ErrorKind::WouldBlock);
            }
        }
    }

    /// A run's sockets carry its own skill's permissions, as of the last
    /// load; a skill not loaded may use nothing.
    #[test]
    fn a_runs_sockets_carry_its_skills_permissions_and_nothing_for_another() {
        let dir = tempfile::tempdir().unwrap();
        let (_woken, waker) = wake_pipe().unwrap();
        let (finished, _done) = waking(&Arc::new(waker));
        let runner = Runner {
            sockets: nowhere(dir.path(), Permissions::default()),
            finished,
            bus: Bus::new(dir.path().join("bus")).unwrap(),
            glows: None,
            body_requests: false,
            ends: RefCell::default(),
            local_time: None,
            trust_clock: false,
            clock_waiting: Cell::new(false),
            rest_client: None,
            permissions: RefCell::new(HashMap::from([(
                "dance".to_owned(),
                Permissions {
                    body: true,
                    ..Permissions::default()
                },
            )])),
            looks: RefCell::default(),
            watching: RefCell::new(Watching {
                watches: Watches::default(),
                marks: HashMap::new(),
                texts: None,
                known_file: None,
                signs: None,
                retaken: HashMap::new(),
            }),
        };
        let dance = runner.sockets_for("dance");
        assert_eq!(dance.may.skill, "dance");
        assert!(dance.may.permissions.body);
        let other = runner.sockets_for("other");
        assert_eq!(other.may.permissions, Permissions::default());
    }
}
