//! Harness's own updates held, and its health read with no credential
//! (#5229; docs/tasks.md, "Harness held at its pinned version"). Off unless
//! `LAMP_HARNESS_HOLD_UPDATES=on` in lamp-tasks' environment.
//!
//! From Harness's source (openharness main 577954332): its daemon reads
//! `ADAPTER_UPDATE_DISABLE` from its own environment (`config/env.ts`) and
//! holds its self-update when it is `true` (`masterProcess.ts:278`), except
//! on a Team account, which ignores it on purpose. `harness start` either
//! spawns the daemon with the caller's environment, or rewrites the user
//! unit `harness service install` wrote from the caller's values of a fixed
//! list (`lib/platformDaemon.ts:42`) that has both `ADAPTER_UPDATE_DISABLE`
//! and `HARNESS_ENV_FILE`; a start from a shell without them drops them.
//! The one file Harness loads is `HARNESS_ENV_FILE` (dotenv, not
//! overriding). So with the switch on, lamp-tasks writes [`FILE`] in
//! Harness's home with `ADAPTER_UPDATE_DISABLE=true` and gives every Harness
//! command both variables ([`hold`]).
//!
//! The daemon answers `GET /api/health` on 127.0.0.1 with no credential,
//! `{"ok":true,"version":"x.y.z"}` once its server is up, signed in or not
//! (`hookServer.ts:527`), on `PORT`, 18473 unless set (`config/env.ts:124`).
//! After each `harness start` that succeeded, [`watch_health`] asks it, in a
//! thread of its own so nothing waits on it, and journals one fixed line.
//!
//! A Harness command run outside lamp-tasks (by hand, over ssh) has neither
//! variable, and its `logout` or `start` restarts the daemon from that
//! environment, self-update on (#5493, lamp-c0a4). So with the switch on,
//! lamp-tasks also writes Harness's launcher (`<HOME>/.local/bin/harness`)
//! in Harness's own pinned shape at its start ([`pin_at_start`]): the
//! preamble Harness's installer writes with `--no-updates`, both variables
//! exported before its `exec` line, which Harness's own updates keep (they
//! rewrite only that line). `lamp-update harness-install` writes the same
//! after it installs (crates/update/src/harness_install/launcher.rs).
//!
//! On a dev or beta lamp, whose updater follows the lamp-voice feed, the
//! switch's hold is lifted (#6300, (B)): the file is written without it,
//! no Harness command is given it, and the launcher's preamble is taken out,
//! so Harness's own updater runs and these lamps are the canary for its
//! releases. Which lamp that is, is decided once, for lamp-update too, in
//! `lamp_msgs::harness_hold`; every other lamp keeps the hold as before.
use lamp_msgs::harness_hold::{HarnessHold, REFUSE_UPDATE};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

/// The switch, in lamp-tasks' own unit: `on` exactly.
pub const SWITCH: &str = "LAMP_HARNESS_HOLD_UPDATES";
/// The variable Harness's daemon holds its updates by.
pub const DISABLE: &str = "ADAPTER_UPDATE_DISABLE";
/// The file Harness loads, by this variable.
pub const ENV_FILE: &str = "HARNESS_ENV_FILE";
/// The file's name, in Harness's home (`HOME`), 0600.
pub const FILE: &str = "harness-hold.env";
/// What the file says.
pub const TEXT: &str = "ADAPTER_UPDATE_DISABLE=true\n";
/// The variable Harness's daemon takes its grid binary from (#6283). Its
/// `ensureManagedGrid` (openharness `lib/runtimeInstall.ts`, v0.6.13_cli)
/// never reads [`DISABLE`]: on each start and every 10 minutes it follows
/// the grid pin at `ADAPTER_GRID_RUNTIME_METADATA_URL` (Harness's CDN), so a
/// held lamp would still move its grid. With this variable set it returns
/// at once, and the grid stays the one the build's pinned install laid down.
pub const GRID_BIN: &str = "HARNESS_GRID_BIN";
/// Where Harness's installer records the managed grid it laid down (the
/// binary's path, one line), under Harness's home.
pub const CURRENT_GRID: &str = ".harness/runtime/current-grid";
/// The folder that path must be in, under Harness's home.
const RUNTIME: &str = ".harness/runtime";
/// `LAMP_HERMES_ON_LAMP` (#5978): the variable Harness finds Hermes'
/// sessions by (openharness `config/env.ts`, `engines/hermes/home.ts`).
/// With the switch it is in the file too, and given to every Harness
/// command ([`env_file`]), whether or not updates are held.
pub const HERMES_HOME: &str = "HERMES_HOME";
/// `LAMP_OPENCLAW_ON_LAMP` (#6226): the variable Harness finds OpenClaw's
/// state, and its sessions under it, by. In the file with the switch, as
/// [`HERMES_HOME`] is.
pub const OPENCLAW_STATE_DIR: &str = "OPENCLAW_STATE_DIR";
/// With the lamp's own Hermes (#5978): the shell Harness reads an engine's
/// launch environment through (`process.env.SHELL` first). `lampos`'s own
/// is `/usr/sbin/nologin`, with which `harness new` waits on that read
/// until it times out ("Waiting to read the engine's launch environment").
pub const SHELL: &str = "SHELL";
/// What [`SHELL`] is set to.
pub const LAUNCH_SHELL: &str = "/bin/sh";
/// Harness's launcher, under Harness's home.
pub const LAUNCHER: &str = ".local/bin/harness";
/// The temporary file the launcher is written through, beside it.
const LAUNCHER_NEXT: &str = ".harness.lampos-pin";
/// Harness's port unless `PORT` is set.
pub const DEFAULT_PORT: u16 = 18473;
/// How long after a start the health is asked for, at most.
pub const HEALTH_WITHIN: Duration = Duration::from_secs(20);
/// Between two asks.
const HEALTH_EVERY: Duration = Duration::from_millis(500);
/// One ask's bound, to connect, write and read.
const ASK_WITHIN: Duration = Duration::from_secs(2);
/// The longest answer read.
const MOST_ANSWER: u64 = 16 * 1024;

/// `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// With `environment`'s `HOME`, [`FILE`] written there (0600, whole) and
/// both [`ENV_FILE`] and [`DISABLE`] added to `environment`. What was
/// written is returned; an error when there is no absolute `HOME` or the
/// file cannot be written, with `environment` unchanged.
pub fn hold(environment: &mut BTreeMap<String, OsString>) -> Result<PathBuf, String> {
    env_file(environment, true, None, None)
}

/// [`FILE`] written in `environment`'s `HOME` (0600, whole) with what is
/// asked: [`TEXT`] with `hold`, `HERMES_HOME=<hermes_home>` with the
/// lamp's own Hermes (#5978), `OPENCLAW_STATE_DIR=<openclaw_state>` with
/// its own OpenClaw (#6226), and `SHELL=/bin/sh` ([`LAUNCH_SHELL`]) with
/// either; [`ENV_FILE`] and each variable the file
/// names added to `environment`. With the hold off, Harness's launcher is
/// not pinned ([`pin_at_start`]), so only Harness commands lamp-tasks
/// runs, and the daemon they start, have them. An error, with
/// `environment` unchanged, as [`hold`]'s.
pub fn env_file(
    environment: &mut BTreeMap<String, OsString>,
    hold: bool,
    hermes_home: Option<&Path>,
    openclaw_state: Option<&Path>,
) -> Result<PathBuf, String> {
    env_file_changed(environment, hold, hermes_home, openclaw_state).map(|(path, _)| path)
}

/// The env file as `hold` asks (#6300), with what lamp-tasks' Hermes asks:
/// held, with [`TEXT`] and [`DISABLE`] given, as before; lifted (the
/// canary), written without them, so it is said changed on a lamp that held
/// before and a daemon running with the hold is started again without it.
/// `None`, nothing written, with the switch off and neither lamp engine.
pub fn env_file_for(
    hold: HarnessHold,
    environment: &mut BTreeMap<String, OsString>,
    hermes_home: Option<&Path>,
    openclaw_state: Option<&Path>,
) -> Option<Result<(PathBuf, bool), String>> {
    (hold != HarnessHold::Off || hermes_home.is_some() || openclaw_state.is_some()).then(|| {
        env_file_changed(
            environment,
            hold == HarnessHold::Held,
            hermes_home,
            openclaw_state,
        )
    })
}

/// As [`env_file`], and whether the file now says something else than it
/// did (or was not there): Harness's daemon reads [`ENV_FILE`] only when it
/// starts, so a daemon already running keeps the old words until it is
/// started again (lamp-c0a4, 10 Nov: `SHELL` and `HERMES_HOME` written
/// while harnessd ran, and `harness new hermes` waited on "Waiting to read
/// the engine's launch environment" until it was restarted by hand).
pub fn env_file_changed(
    environment: &mut BTreeMap<String, OsString>,
    hold: bool,
    hermes_home: Option<&Path>,
    openclaw_state: Option<&Path>,
) -> Result<(PathBuf, bool), String> {
    let home = environment
        .get("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .ok_or("no absolute HOME")?;
    let hermes = plain(hermes_home, "HERMES_HOME is not a plain absolute path")?;
    let openclaw = plain(
        openclaw_state,
        "OPENCLAW_STATE_DIR is not a plain absolute path",
    )?;
    let grid = hold.then(|| pinned_grid(&home)).flatten();
    let mut text = String::new();
    if hold {
        text.push_str(TEXT);
    }
    if let Some(grid) = &grid {
        text.push_str(&format!("{GRID_BIN}={grid}\n"));
    }
    if let Some(hermes) = hermes {
        text.push_str(&format!("{HERMES_HOME}={hermes}\n"));
    }
    if let Some(openclaw) = openclaw {
        text.push_str(&format!("{OPENCLAW_STATE_DIR}={openclaw}\n"));
    }
    let lamp_engine = hermes.is_some() || openclaw.is_some();
    if lamp_engine {
        text.push_str(&format!("{SHELL}={LAUNCH_SHELL}\n"));
    }
    let path = home.join(FILE);
    let changed = !matches!(fs::read(&path), Ok(was) if was == text.as_bytes());
    write_whole(&path, text.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))?;
    environment.insert(ENV_FILE.into(), path.clone().into());
    if hold {
        environment.insert(DISABLE.into(), "true".into());
    }
    match grid {
        Some(grid) => environment.insert(GRID_BIN.into(), grid.into()),
        None => environment.remove(GRID_BIN),
    };
    if let Some(hermes) = hermes {
        environment.insert(HERMES_HOME.into(), hermes.into());
    }
    if let Some(openclaw) = openclaw {
        environment.insert(OPENCLAW_STATE_DIR.into(), openclaw.into());
    }
    if lamp_engine {
        environment.insert(SHELL.into(), LAUNCH_SHELL.into());
    }
    Ok((path, changed))
}

/// `path` as text when it is a plain absolute path, one an env file line
/// can carry; `why` otherwise.
fn plain<'a>(path: Option<&'a Path>, why: &'static str) -> Result<Option<&'a str>, &'static str> {
    path.map(|path| {
        path.to_str()
            .filter(|text| path.is_absolute() && !text.contains(['\n', '\0', '"']))
            .ok_or(why)
    })
    .transpose()
}

/// The managed grid Harness's pinned install laid down (#6283): the first
/// line of [`CURRENT_GRID`] in `home`, when it is a plain absolute path to
/// a file named `bin/grid` inside `<home>/.harness/runtime`. `None` when
/// there is no such record (no grid installed: then the daemon has none to
/// follow either), or it names anything else, so nothing odd is ever
/// written into Harness's env file.
pub fn pinned_grid(home: &Path) -> Option<String> {
    let record = fs::read_to_string(home.join(CURRENT_GRID)).ok()?;
    let line = record.lines().next()?;
    let path = Path::new(line);
    let plain = !line.is_empty()
        && line
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c));
    (plain
        && path.is_absolute()
        && path.starts_with(home.join(RUNTIME))
        && path.ends_with("bin/grid")
        && path
            .components()
            .all(|part| !matches!(part, std::path::Component::ParentDir))
        && path.is_file())
    .then(|| line.to_owned())
}

fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let next = path.with_extension("env.next");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&next)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&next, path)
}

/// `text`, a Harness launcher, in Harness's pinned shape: every
/// `export ADAPTER_UPDATE_DISABLE=` and `export HARNESS_ENV_FILE=` line
/// before its first `exec` line taken out, then `export
/// ADAPTER_UPDATE_DISABLE=true`, `export HARNESS_ENV_FILE=<env_file>` and
/// the line refusing `harness update` ([`REFUSE_UPDATE`], #5957) put just
/// before it, that line taken out first too. The `exec` line and every
/// other line are kept as they are, so a launcher already pinned comes back
/// byte for byte. `None` when there is no `exec` line.
pub fn pinned_launcher(text: &str, env_file: &str) -> Option<String> {
    launcher_shaped(text, Some(env_file))
}

/// `text`, a Harness launcher, with the hold taken out (#6300): every
/// `export ADAPTER_UPDATE_DISABLE=` and `export HARNESS_ENV_FILE=` line
/// and the [`REFUSE_UPDATE`] line before its first `exec` line taken out,
/// so Harness's own `update` runs on a canary lamp, every other line kept, so one
/// with neither comes back byte for byte. `None` when there is no `exec`
/// line.
pub fn unpinned_launcher(text: &str) -> Option<String> {
    launcher_shaped(text, None)
}

fn launcher_shaped(text: &str, env_file: Option<&str>) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let exec = lines
        .iter()
        .position(|line| line.trim_start().starts_with("exec "))?;
    let mut pinned = String::with_capacity(text.len() + 96);
    for line in &lines[..exec] {
        let line_text = line.trim();
        if line_text.starts_with(&format!("export {DISABLE}="))
            || line_text.starts_with(&format!("export {ENV_FILE}="))
            || line_text == REFUSE_UPDATE
        {
            continue;
        }
        pinned.push_str(line);
    }
    if let Some(env_file) = env_file {
        pinned.push_str(&format!("export {DISABLE}=true\n"));
        pinned.push_str(&format!("export {ENV_FILE}={env_file}\n"));
        pinned.push_str(REFUSE_UPDATE);
        pinned.push('\n');
    }
    for line in &lines[exec..] {
        pinned.push_str(line);
    }
    Some(pinned)
}

/// What [`pin_launcher`] or [`unpin_launcher`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Launcher {
    /// Written in the shape asked.
    Written,
    /// Already in it: nothing written.
    Kept,
    /// No launcher there: nothing written, none made.
    Missing,
}

/// The launcher at `launcher` written in the pinned shape
/// ([`pinned_launcher`]), whole: through a file beside it, with its mode
/// (executable) and owner, then renamed over it. A launcher already pinned
/// is not written; a missing one is not made. An error, with the launcher
/// unchanged, when it is not a plain file, has no `exec` line, or cannot be
/// written.
pub fn pin_launcher(launcher: &Path, env_file: &Path) -> Result<Launcher, String> {
    shape_launcher(launcher, Some(env_file))
}

/// The launcher at `launcher` with the hold taken out
/// ([`unpinned_launcher`]), written as [`pin_launcher`] writes it: one with
/// no hold is not written, a missing one is not made (#6300).
pub fn unpin_launcher(launcher: &Path) -> Result<Launcher, String> {
    shape_launcher(launcher, None)
}

fn shape_launcher(launcher: &Path, env_file: Option<&Path>) -> Result<Launcher, String> {
    let said = |error: &dyn std::fmt::Display| format!("{}: {error}", launcher.display());
    let metadata = match fs::symlink_metadata(launcher) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Launcher::Missing),
        Err(error) => return Err(said(&error)),
    };
    if !metadata.is_file() {
        return Err(said(&"not a plain file"));
    }
    let env_file = env_file
        .map(|env_file| {
            env_file
                .to_str()
                .filter(|path| {
                    path.starts_with('/')
                        && path
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
                })
                .ok_or_else(|| said(&"the hold file's path is not plain"))
        })
        .transpose()?;
    let text = fs::read_to_string(launcher).map_err(|error| said(&error))?;
    let pinned = launcher_shaped(&text, env_file).ok_or_else(|| said(&"no exec line"))?;
    if pinned == text {
        return Ok(Launcher::Kept);
    }
    let next = launcher.with_file_name(LAUNCHER_NEXT);
    let written = write_launcher(&next, pinned.as_bytes(), &metadata)
        .and_then(|()| fs::rename(&next, launcher));
    if let Err(error) = written {
        let _ = fs::remove_file(&next);
        return Err(said(&error));
    }
    Ok(Launcher::Written)
}

fn write_launcher(next: &Path, bytes: &[u8], was: &fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let mut mode = was.mode() & 0o7777;
    if mode & 0o100 == 0 {
        mode = 0o755;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(next)?;
    file.write_all(bytes)?;
    let now = file.metadata()?;
    if (now.uid(), now.gid()) != (was.uid(), was.gid()) {
        std::os::unix::fs::fchown(&file, Some(was.uid()), Some(was.gid()))?;
    }
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.sync_all()
}

/// At lamp-tasks' start, with an absolute `home`: held, `<home>/`[`LAUNCHER`]
/// pinned to `<home>/`[`FILE`] ([`pin_launcher`]); lifted (the canary,
/// #6300), the hold taken out of it ([`unpin_launcher`]); and the journal
/// line returned. With the switch off, nothing looked at or written, and no
/// line.
pub fn pin_at_start(hold: HarnessHold, home: Option<&Path>) -> Option<String> {
    if hold == HarnessHold::Off {
        return None;
    }
    let Some(home) = home.filter(|home| home.is_absolute()) else {
        return Some("tasks: Harness's launcher not pinned: no absolute HOME".to_owned());
    };
    let launcher = home.join(LAUNCHER);
    if hold == HarnessHold::Lifted {
        return Some(match unpin_launcher(&launcher) {
            Ok(Launcher::Written) => format!(
                "tasks: Harness's launcher unpinned (the lamp-voice feed's canary, #6300): {}",
                launcher.display()
            ),
            Ok(Launcher::Kept) => format!(
                "tasks: Harness's launcher not pinned (the lamp-voice feed's canary, #6300): {}",
                launcher.display()
            ),
            Ok(Launcher::Missing) => format!(
                "tasks: no Harness launcher at {}: left alone",
                launcher.display()
            ),
            Err(why) => format!("tasks: Harness's launcher not unpinned: {why}"),
        });
    }
    Some(match pin_launcher(&launcher, &home.join(FILE)) {
        Ok(Launcher::Written) => format!(
            "tasks: Harness's launcher pinned ({DISABLE}=true and {ENV_FILE} exported before its exec line): {}",
            launcher.display()
        ),
        Ok(Launcher::Kept) => format!(
            "tasks: Harness's launcher already pinned: {}",
            launcher.display()
        ),
        Ok(Launcher::Missing) => format!(
            "tasks: no Harness launcher at {}: left alone",
            launcher.display()
        ),
        Err(why) => format!("tasks: Harness's launcher not pinned: {why}"),
    })
}

/// lamp-update's `ExecStart` as systemd has it loaded (`systemctl show
/// lamp-update.service --property=ExecStart --value`), which names the feed
/// it follows (`lamp_msgs::harness_hold::feed`). `None` when systemctl
/// fails or says nothing: the hold is then kept.
pub fn lamp_update_exec_start() -> Option<String> {
    let shown = std::process::Command::new("systemctl")
        .args([
            "show",
            "lamp-update.service",
            "--property=ExecStart",
            "--value",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|shown| shown.status.success())?;
    String::from_utf8(shown.stdout)
        .ok()
        .filter(|text| !text.trim().is_empty())
}

/// The port Harness listens on, from `environment`'s `PORT`.
pub fn port(environment: &BTreeMap<String, OsString>) -> u16 {
    environment
        .get("PORT")
        .and_then(|port| port.to_str())
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .unwrap_or(DEFAULT_PORT)
}

/// What one ask of the health said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Health {
    /// 200 with `ok: true`, and the version it said.
    Ok(Option<String>),
    /// An answer that isn't that: its fixed reason.
    Bad(&'static str),
}

/// One `GET /api/health` on 127.0.0.1:`port`, no credential, within
/// [`ASK_WITHIN`]. `Err`: nothing listening or nothing answered.
pub fn ask(port: u16) -> io::Result<Health> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, ASK_WITHIN)?;
    stream.set_read_timeout(Some(ASK_WITHIN))?;
    stream.set_write_timeout(Some(ASK_WITHIN))?;
    write!(
        stream,
        "GET /api/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut answer = Vec::new();
    stream.take(MOST_ANSWER).read_to_end(&mut answer)?;
    Ok(read(&answer))
}

/// An http answer read as [`Health`].
fn read(answer: &[u8]) -> Health {
    let Some(split) = answer.windows(4).position(|end| end == b"\r\n\r\n") else {
        return Health::Bad("no_answer");
    };
    let (head, body) = (&answer[..split], &answer[split + 4..]);
    let status = head.split(|&b| b == b'\r').next().unwrap_or_default();
    if !(status.starts_with(b"HTTP/1.1 200 ") || status.starts_with(b"HTTP/1.0 200 ")) {
        return Health::Bad("not_200");
    }
    let chunked = head.split(|&b| b == b'\n').any(|line| {
        let line = String::from_utf8_lossy(line).to_ascii_lowercase();
        line.trim()
            .strip_prefix("transfer-encoding:")
            .is_some_and(|value| value.trim() == "chunked")
    });
    let whole;
    let body = if chunked {
        let Some(joined) = dechunk(body) else {
            return Health::Bad("not_json");
        };
        whole = joined;
        &whole[..]
    } else {
        body
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Health::Bad("not_json");
    };
    if value.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Health::Bad("not_ok");
    }
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .filter(|version| {
            version.len() <= 32
                && version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
        })
        .map(str::to_owned);
    Health::Ok(version)
}

/// A chunked body joined, up to its last (empty) chunk; `None` when a size
/// isn't hex or a chunk is cut short.
fn dechunk(mut body: &[u8]) -> Option<Vec<u8>> {
    let mut whole = Vec::new();
    loop {
        let end = body.windows(2).position(|crlf| crlf == b"\r\n")?;
        let size = std::str::from_utf8(&body[..end]).ok()?;
        let size = size.split(';').next()?.trim();
        let size = usize::from_str_radix(size, 16).ok()?;
        body = &body[end + 2..];
        if size == 0 {
            return Some(whole);
        }
        // A size near usize::MAX is a cut chunk, not an overflow.
        let past = size.checked_add(2)?;
        if body.len() < past || &body[size..past] != b"\r\n" {
            return None;
        }
        whole.extend_from_slice(&body[..size]);
        body = &body[past..];
    }
}

/// The journal line for what the health said within `within`, asking every
/// [`HEALTH_EVERY`] until it answers.
pub fn health_line(port: u16, within: Duration) -> String {
    let started = Instant::now();
    loop {
        let asked = ask(port);
        let waited = started.elapsed();
        match asked {
            Ok(Health::Ok(version)) => {
                return format!(
                    "tasks: Harness health ok on 127.0.0.1:{port}, version {}, {} ms after its start",
                    version.as_deref().unwrap_or("unsaid"),
                    waited.as_millis()
                );
            }
            Ok(Health::Bad(reason)) if waited >= within => {
                return format!("tasks: Harness health failed on 127.0.0.1:{port}: {reason}");
            }
            Err(_) if waited >= within => {
                return format!(
                    "tasks: Harness health failed on 127.0.0.1:{port}: no_answer within {} s",
                    within.as_secs()
                );
            }
            _ => thread::sleep(HEALTH_EVERY),
        }
    }
}

/// [`health_line`] in a thread of its own, its line journaled.
pub fn watch_health(port: u16) {
    let _ = thread::Builder::new()
        .name("harness-health".into())
        .spawn(move || eprintln!("{}", health_line(port, HEALTH_WITHIN)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::harness_hold::harness_hold;

    /// lamp-update's `ExecStart` on a product lamp, the stable feed.
    const STABLE_LINE: &str = "/opt/lampOS/update/current/lamp-update serve --channel-url https://github.com/autonomous-ai/lampOS/releases/download/lampos-stable/ --channel lamp-voice --key-file /etc/lampOS/update/key.pub --apply on";
    /// The same on a dev or beta lamp, the lamp-voice feed: the canary.
    const CANARY_LINE: &str = "/opt/lampOS/update/current/lamp-update serve --channel-url https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice/ --channel lamp-voice --key-file /etc/lampOS/update/key.pub --apply on";
    /// A feed of neither: a local channel folder.
    const OTHER_LINE: &str = "/opt/lampOS/update/current/lamp-update serve --channel-url file:///var/tmp/lampos-channel/ --channel lamp-voice --key-file /etc/lampOS/update/key.pub --apply on";
    use std::{net::TcpListener, os::unix::fs::PermissionsExt};

    #[test]
    fn the_hold_writes_its_file_and_gives_harness_both_variables() {
        let home = tempfile::tempdir().unwrap();
        let mut environment = BTreeMap::new();
        environment.insert("HOME".to_owned(), OsString::from(home.path()));
        let path = hold(&mut environment).unwrap();
        assert_eq!(path, home.path().join(FILE));
        assert_eq!(fs::read_to_string(&path).unwrap(), TEXT);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(environment[DISABLE], OsString::from("true"));
        assert_eq!(environment[ENV_FILE], OsString::from(&path));
        // Again: the same file, whole.
        fs::write(&path, "ADAPTER_UPDATE_DISABLE=false\n").unwrap();
        hold(&mut environment).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), TEXT);
    }

    #[test]
    fn no_home_holds_nothing() {
        let mut environment = BTreeMap::new();
        assert!(hold(&mut environment).is_err());
        environment.insert("HOME".to_owned(), OsString::from("relative"));
        assert!(hold(&mut environment).is_err());
        assert!(!environment.contains_key(DISABLE));
    }

    /// Harness's launcher as its installer writes it without `--no-updates`.
    const UNPINNED: &str = "#!/bin/sh\nexec \"/var/lib/lampos-tasks/.local/share/harness/node/bin/node\" \"/var/lib/lampos-tasks/.local/share/harness/cli/0.5.39/cli.js\" \"$@\"\n";
    /// The same, pinned.
    const PINNED: &str = "#!/bin/sh\nexport ADAPTER_UPDATE_DISABLE=true\nexport HARNESS_ENV_FILE=/var/lib/lampos-tasks/harness-hold.env\ncase \"${1-}\" in update) echo \"harness: this lamp keeps Harness at its pinned version and updates it itself; nothing was changed\" >&2; exit 1 ;; esac\nexec \"/var/lib/lampos-tasks/.local/share/harness/node/bin/node\" \"/var/lib/lampos-tasks/.local/share/harness/cli/0.5.39/cli.js\" \"$@\"\n";
    const ENV_AT: &str = "/var/lib/lampos-tasks/harness-hold.env";

    #[test]
    fn the_launcher_text_is_harnesss_pinned_shape_and_keeps_its_exec_line() {
        assert_eq!(pinned_launcher(UNPINNED, ENV_AT).unwrap(), PINNED);
        // Already pinned: byte for byte.
        assert_eq!(pinned_launcher(PINNED, ENV_AT).unwrap(), PINNED);
        // Harness's own `--no-updates` preamble, or a hold set otherwise,
        // comes back as the one pinned shape, once.
        let other = "#!/bin/sh\nexport ADAPTER_UPDATE_DISABLE=false\nexec node cli.js \"$@\"\n";
        assert_eq!(
            pinned_launcher(other, ENV_AT).unwrap(),
            format!(
                "#!/bin/sh\nexport ADAPTER_UPDATE_DISABLE=true\nexport HARNESS_ENV_FILE={ENV_AT}\n{REFUSE_UPDATE}\nexec node cli.js \"$@\"\n"
            )
        );
        // An exec line Harness's update rewrote is kept as it is.
        let updated = PINNED.replace("0.5.39", "0.5.57");
        assert_eq!(pinned_launcher(&updated, ENV_AT).unwrap(), updated);
        assert_eq!(pinned_launcher("#!/bin/sh\necho hi\n", ENV_AT), None);
    }

    /// #5957: the pinned launcher, run by `/bin/sh` as Harness's is,
    /// refuses `harness update` before Harness runs (one line on stderr,
    /// exit 1), and runs every other command, `update` as a later word
    /// too. Unpinned, the line goes with the hold.
    #[test]
    fn the_pinned_launcher_refuses_harness_update_and_runs_the_rest() {
        let launcher = pinned_launcher("#!/bin/sh\nexec echo ran \"$@\"\n", ENV_AT).unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(&launcher)
                .arg("harness")
                .args(args)
                .output()
                .unwrap()
        };
        for refused in [
            &["update"][..],
            &["update", "--force"],
            &["update", "--json"],
        ] {
            let out = run(refused);
            assert_eq!(out.status.code(), Some(1), "{refused:?}");
            assert!(out.stdout.is_empty(), "{refused:?}: Harness ran");
            assert_eq!(
                String::from_utf8_lossy(&out.stderr),
                "harness: this lamp keeps Harness at its pinned version and updates it itself; nothing was changed\n"
            );
        }
        for (args, said) in [
            (&["ps", "--all", "--json"][..], "ran ps --all --json\n"),
            (&[], "ran\n"),
            (&["reply", "lamp-1", "update"], "ran reply lamp-1 update\n"),
            (&["updates"], "ran updates\n"),
            (&["--update"], "ran --update\n"),
        ] {
            let out = run(args);
            assert_eq!(out.status.code(), Some(0), "{args:?}");
            assert_eq!(String::from_utf8_lossy(&out.stdout), said);
        }
        assert_eq!(
            unpinned_launcher(&launcher).unwrap(),
            "#!/bin/sh\nexec echo ran \"$@\"\n"
        );
    }

    fn launcher_in(home: &Path, text: &str, mode: u32) -> PathBuf {
        let bin = home.join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        let launcher = bin.join("harness");
        fs::write(&launcher, text).unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(mode)).unwrap();
        launcher
    }

    #[test]
    fn with_the_switch_on_the_launcher_is_pinned_once_and_stays_executable() {
        let home = tempfile::tempdir().unwrap();
        let launcher = launcher_in(home.path(), UNPINNED, 0o755);
        let line = pin_at_start(HarnessHold::Held, Some(home.path())).unwrap();
        assert!(
            line.starts_with("tasks: Harness's launcher pinned ("),
            "{line}"
        );
        let want = PINNED.replace(ENV_AT, &home.path().join(FILE).to_string_lossy());
        assert_eq!(fs::read_to_string(&launcher).unwrap(), want);
        assert_eq!(
            fs::metadata(&launcher).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(!launcher.with_file_name(LAUNCHER_NEXT).exists());
        // Again: kept byte for byte, no second preamble.
        let line = pin_at_start(HarnessHold::Held, Some(home.path())).unwrap();
        assert!(
            line.starts_with("tasks: Harness's launcher already pinned"),
            "{line}"
        );
        assert_eq!(fs::read_to_string(&launcher).unwrap(), want);
        assert_eq!(
            pin_launcher(&launcher, &home.path().join(FILE)),
            Ok(Launcher::Kept)
        );
    }

    #[test]
    fn with_the_switch_off_the_launcher_is_left_as_it_is() {
        let home = tempfile::tempdir().unwrap();
        let launcher = launcher_in(home.path(), UNPINNED, 0o750);
        for switch in [None, Some("off"), Some("ON"), Some("")] {
            for exec in [None, Some(STABLE_LINE), Some(CANARY_LINE)] {
                assert_eq!(
                    pin_at_start(harness_hold(switch, exec), Some(home.path())),
                    None
                );
            }
        }
        assert_eq!(fs::read_to_string(&launcher).unwrap(), UNPINNED);
        assert_eq!(
            fs::metadata(&launcher).unwrap().permissions().mode() & 0o777,
            0o750
        );
    }

    #[test]
    fn a_missing_launcher_is_not_made() {
        let home = tempfile::tempdir().unwrap();
        let line = pin_at_start(HarnessHold::Held, Some(home.path())).unwrap();
        assert!(line.starts_with("tasks: no Harness launcher at "), "{line}");
        assert!(line.ends_with(": left alone"), "{line}");
        assert!(!home.path().join(LAUNCHER).exists());
        assert!(!home.path().join(".local").exists());
        // One with no exec line is left as it is too.
        let launcher = launcher_in(home.path(), "#!/bin/sh\necho hi\n", 0o755);
        let line = pin_at_start(HarnessHold::Held, Some(home.path())).unwrap();
        assert!(line.ends_with("no exec line"), "{line}");
        assert_eq!(
            fs::read_to_string(&launcher).unwrap(),
            "#!/bin/sh\necho hi\n"
        );
    }

    #[test]
    fn only_on_exactly_is_on() {
        assert!(on(Some("on")));
        for value in [None, Some("off"), Some("ON"), Some("true"), Some("")] {
            assert!(!on(value), "{value:?}");
        }
    }

    #[test]
    fn the_port_is_harnesss_own_unless_set() {
        let mut environment = BTreeMap::new();
        assert_eq!(port(&environment), DEFAULT_PORT);
        environment.insert("PORT".to_owned(), OsString::from("18500"));
        assert_eq!(port(&environment), 18500);
        environment.insert("PORT".to_owned(), OsString::from("0"));
        assert_eq!(port(&environment), DEFAULT_PORT);
    }

    #[test]
    fn an_answer_is_read_as_harness_gives_it() {
        let ok = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true,\"version\":\"0.5.39\"}";
        assert_eq!(read(ok), Health::Ok(Some("0.5.39".into())));
        assert_eq!(
            read(b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true}"),
            Health::Ok(None)
        );
        assert_eq!(
            read(b"HTTP/1.1 401 Unauthorized\r\n\r\n{}"),
            Health::Bad("not_200")
        );
        assert_eq!(
            read(b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":false}"),
            Health::Bad("not_ok")
        );
        assert_eq!(
            read(b"HTTP/1.1 200 OK\r\n\r\n<html>"),
            Health::Bad("not_json")
        );
        assert_eq!(read(b"HTTP/1.1 200"), Health::Bad("no_answer"));
    }

    #[test]
    fn a_chunked_answer_is_read_whole() {
        // An HTTP/1.1 server may answer in chunks: on lamp-c0a4
        // (#5379) the line said not_json though the daemon was up.
        let chunked = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n10\r\n{\"ok\":true,\"vers\r\nE\r\nion\":\"0.5.39\"}\r\n0\r\n\r\n";
        assert_eq!(read(chunked), Health::Ok(Some("0.5.39".into())));
        let upper =
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: Chunked\r\n\r\nB\r\n{\"ok\":true}\r\n0\r\n\r\n";
        assert_eq!(read(upper), Health::Ok(None));
        // A chunk cut short or a size that isn't hex is still not json.
        let cut = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n40\r\n{\"ok\":true}";
        assert_eq!(read(cut), Health::Bad("not_json"));
        let bad = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n{}\r\n0\r\n\r\n";
        assert_eq!(read(bad), Health::Bad("not_json"));
        // A size that would overflow is not json, never a panic.
        let huge = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffff\r\n{}\r\n0\r\n\r\n";
        assert_eq!(read(huge), Health::Bad("not_json"));
    }

    /// A one-answer server on loopback, the request it was sent kept.
    fn server(answer: &'static [u8]) -> (u16, thread::JoinHandle<String>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = Vec::new();
            let mut chunk = [0u8; 512];
            while !asked.windows(4).any(|end| end == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                asked.extend_from_slice(&chunk[..read]);
            }
            stream.write_all(answer).unwrap();
            String::from_utf8(asked).unwrap()
        });
        (port, thread)
    }

    #[test]
    fn the_health_is_asked_with_no_credential_and_journaled() {
        let (port, asked) = server(b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true,\"version\":\"0.5.39\"}");
        let line = health_line(port, Duration::from_secs(5));
        let asked = asked.join().unwrap();
        assert!(asked.starts_with("GET /api/health HTTP/1.1\r\n"), "{asked}");
        assert!(
            !asked.to_ascii_lowercase().contains("authorization"),
            "{asked}"
        );
        assert!(!asked.to_ascii_lowercase().contains("cookie"), "{asked}");
        assert!(
            line.starts_with(&format!(
                "tasks: Harness health ok on 127.0.0.1:{port}, version 0.5.39, "
            )),
            "{line}"
        );
    }

    #[test]
    fn nothing_listening_is_said_once_its_time_is_up() {
        let port = {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            listener.local_addr().unwrap().port()
        };
        let line = health_line(port, Duration::ZERO);
        assert_eq!(
            line,
            format!("tasks: Harness health failed on 127.0.0.1:{port}: no_answer within 0 s")
        );
    }

    /// #5978: with the lamp's own Hermes, the file names `HERMES_HOME` and
    /// Harness is given it, with the hold or without; the hold alone is as
    /// before.
    #[test]
    fn the_env_file_names_hermes_home_and_a_shell_with_or_without_the_hold() {
        for hold in [true, false] {
            let home = tempfile::tempdir().unwrap();
            let mut environment =
                BTreeMap::from([("HOME".to_owned(), OsString::from(home.path()))]);
            let path = env_file(
                &mut environment,
                hold,
                Some(Path::new("/var/lib/lampos-hermes/.hermes")),
                None,
            )
            .unwrap();
            let text = fs::read_to_string(&path).unwrap();
            let want = format!(
                "{}HERMES_HOME=/var/lib/lampos-hermes/.hermes\nSHELL=/bin/sh\n",
                if hold { TEXT } else { "" }
            );
            assert_eq!(text, want, "hold {hold}");
            assert_eq!(environment[HERMES_HOME], "/var/lib/lampos-hermes/.hermes");
            assert_eq!(environment[SHELL], "/bin/sh");
            assert_eq!(environment[ENV_FILE], OsString::from(&path));
            assert_eq!(environment.contains_key(DISABLE), hold);
        }
        let home = tempfile::tempdir().unwrap();
        let mut environment = BTreeMap::from([("HOME".to_owned(), OsString::from(home.path()))]);
        hold(&mut environment).unwrap();
        assert_eq!(fs::read_to_string(home.path().join(FILE)).unwrap(), TEXT);
        assert!(!environment.contains_key(HERMES_HOME));
        assert!(!environment.contains_key(SHELL));
        assert!(env_file(&mut environment, false, Some(Path::new("relative")), None).is_err());
    }

    /// #6226: with the lamp's own OpenClaw, the file names
    /// `OPENCLAW_STATE_DIR` and the shell, beside `HERMES_HOME` when both
    /// are on, and Harness is given them.
    #[test]
    fn the_env_file_names_openclaw_state_dir_beside_hermes_home() {
        let state = Path::new("/var/lib/lampos-openclaw/state");
        let hermes = Path::new("/var/lib/lampos-hermes/.hermes");
        for (hermes_home, want) in [
            (
                None,
                "OPENCLAW_STATE_DIR=/var/lib/lampos-openclaw/state\nSHELL=/bin/sh\n",
            ),
            (
                Some(hermes),
                "HERMES_HOME=/var/lib/lampos-hermes/.hermes\n\
                 OPENCLAW_STATE_DIR=/var/lib/lampos-openclaw/state\nSHELL=/bin/sh\n",
            ),
        ] {
            let home = tempfile::tempdir().unwrap();
            let mut environment =
                BTreeMap::from([("HOME".to_owned(), OsString::from(home.path()))]);
            let path = env_file(&mut environment, false, hermes_home, Some(state)).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), want);
            assert_eq!(
                environment[OPENCLAW_STATE_DIR],
                "/var/lib/lampos-openclaw/state"
            );
            assert_eq!(environment[SHELL], "/bin/sh");
            assert_eq!(environment.contains_key(HERMES_HOME), hermes_home.is_some());
        }
        let home = tempfile::tempdir().unwrap();
        let mut environment = BTreeMap::from([("HOME".to_owned(), OsString::from(home.path()))]);
        assert!(env_file(&mut environment, false, None, Some(Path::new("relative"))).is_err());
        assert!(!environment.contains_key(OPENCLAW_STATE_DIR));
    }

    /// #5978, lamp-c0a4 on 10 Nov: the file is said changed when it was not
    /// there or said other words, and not when it is written again the
    /// same, so Harness's daemon is started again only for new words.
    #[test]
    fn the_env_file_is_said_changed_only_when_its_words_change() {
        let home = tempfile::tempdir().unwrap();
        let mut environment = BTreeMap::from([("HOME".to_owned(), OsString::from(home.path()))]);
        let hermes = Some(Path::new("/var/lib/lampos-hermes/.hermes"));
        let (path, changed) = env_file_changed(&mut environment, true, None, None).unwrap();
        assert!(changed, "not there before");
        assert!(
            !env_file_changed(&mut environment, true, None, None)
                .unwrap()
                .1
        );
        assert!(
            env_file_changed(&mut environment, true, hermes, None)
                .unwrap()
                .1
        );
        assert!(
            !env_file_changed(&mut environment, true, hermes, None)
                .unwrap()
                .1
        );
        fs::write(&path, "SHELL=/usr/sbin/nologin\n").unwrap();
        assert!(
            env_file_changed(&mut environment, true, hermes, None)
                .unwrap()
                .1
        );
    }

    /// #6300: lamp-tasks' three writers of the hold (the file, the
    /// variables given to Harness, the launcher) for one lamp.
    fn writers(hold: HarnessHold, launcher: &str) -> (String, BTreeMap<String, OsString>, String) {
        let home = tempfile::tempdir().unwrap();
        let at = launcher_in(home.path(), launcher, 0o755);
        let mut environment = BTreeMap::from([("HOME".to_owned(), OsString::from(home.path()))]);
        env_file_for(hold, &mut environment, None, None)
            .unwrap()
            .unwrap();
        pin_at_start(hold, Some(home.path())).unwrap();
        environment.remove("HOME");
        let env_at = home.path().join(FILE).to_string_lossy().into_owned();
        (
            fs::read_to_string(home.path().join(FILE)).unwrap(),
            environment,
            fs::read_to_string(at).unwrap().replace(&env_at, ENV_AT),
        )
    }

    /// Stable, and any feed but lamp-voice or one not read, keeps every
    /// writer of the hold exactly as before #6300 (#6283's line is
    /// untouched): the file says it, Harness is given it, the launcher is
    /// pinned, from an unpinned launcher or a pinned one.
    #[test]
    fn stable_and_an_unknown_feed_keep_the_hold_in_every_writer() {
        for exec in [Some(STABLE_LINE), Some(OTHER_LINE), None] {
            let hold = harness_hold(Some("on"), exec);
            assert_eq!(hold, HarnessHold::Held, "{exec:?}");
            for launcher in [UNPINNED, PINNED] {
                let (file, environment, launcher) = writers(hold, launcher);
                assert_eq!(file, TEXT, "{exec:?}");
                assert_eq!(environment[DISABLE], OsString::from("true"), "{exec:?}");
                assert!(environment.contains_key(ENV_FILE), "{exec:?}");
                assert_eq!(launcher, PINNED, "{exec:?}");
            }
        }
    }

    /// A dev or beta lamp (the lamp-voice feed) lifts the hold in every
    /// writer: the file without it (said changed where it held, so a
    /// running daemon is started again), no `ADAPTER_UPDATE_DISABLE` given,
    /// and the launcher's preamble taken out, its `exec` line kept.
    #[test]
    fn the_lamp_voice_feed_lifts_the_hold_in_every_writer() {
        let hold = harness_hold(Some("on"), Some(CANARY_LINE));
        assert_eq!(hold, HarnessHold::Lifted);
        for launcher in [UNPINNED, PINNED] {
            let (file, environment, launcher) = writers(hold, launcher);
            assert_eq!(file, "");
            assert!(!environment.contains_key(DISABLE));
            assert_eq!(launcher, UNPINNED);
        }
        // A lamp that held before: the file is said changed.
        let home = tempfile::tempdir().unwrap();
        let mut environment = BTreeMap::from([("HOME".to_owned(), OsString::from(home.path()))]);
        hold_file_then(&mut environment, home.path());
        let (_, changed) = env_file_for(hold, &mut environment, None, None)
            .unwrap()
            .unwrap();
        assert!(changed);
        assert!(!environment.contains_key(DISABLE));
        // An updated exec line is kept as Harness wrote it.
        let updated = PINNED.replace("0.5.39", "0.6.13");
        assert_eq!(
            unpinned_launcher(&updated).unwrap(),
            UNPINNED.replace("0.5.39", "0.6.13")
        );
        assert_eq!(unpinned_launcher("#!/bin/sh\necho hi\n"), None);
    }

    fn hold_file_then(environment: &mut BTreeMap<String, OsString>, home: &Path) {
        hold(environment).unwrap();
        assert_eq!(fs::read_to_string(home.join(FILE)).unwrap(), TEXT);
        environment.remove(DISABLE);
    }

    /// #6283: a held lamp gives Harness the grid its pinned install laid
    /// down, so the daemon's grid recheck never follows Harness's CDN pin;
    /// a lifted lamp (the canary) follows it; and a record that names
    /// anything but a `bin/grid` file in Harness's runtime folder, or none,
    /// adds nothing.
    #[test]
    fn a_held_lamp_keeps_its_pinned_grid_and_a_lifted_one_follows() {
        let home = tempfile::tempdir().unwrap();
        let runtime = home.path().join(RUNTIME);
        let grid_dir = runtime.join("grid-0.3.63-linux-arm64/bin");
        fs::create_dir_all(&grid_dir).unwrap();
        let grid = grid_dir.join("grid");
        fs::write(&grid, "#!/bin/sh\n").unwrap();
        let grid = grid.to_str().unwrap().to_owned();
        let record = home.path().join(CURRENT_GRID);
        let mut environment = BTreeMap::from([("HOME".to_owned(), OsString::from(home.path()))]);

        // No record yet: the file is as before #6283.
        let (path, _) = env_file_changed(&mut environment, true, None, None).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), TEXT);
        assert!(!environment.contains_key(GRID_BIN));

        // Held, with the installer's record: the file and Harness both have it.
        fs::write(&record, format!("{grid}\n")).unwrap();
        let (_, changed) = env_file_changed(&mut environment, true, None, None).unwrap();
        assert!(changed, "the grid line is new words");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{TEXT}{GRID_BIN}={grid}\n")
        );
        assert_eq!(environment[GRID_BIN], OsString::from(&grid));
        assert!(
            !env_file_changed(&mut environment, true, None, None)
                .unwrap()
                .1
        );

        // Lifted: no grid line, and the variable is taken off again.
        let (_, changed) = env_file_for(HarnessHold::Lifted, &mut environment, None, None)
            .unwrap()
            .unwrap();
        assert!(changed);
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        assert!(!environment.contains_key(GRID_BIN));

        // A record naming anything else adds nothing.
        let outside = home.path().join("bin/grid");
        fs::create_dir_all(outside.parent().unwrap()).unwrap();
        fs::write(&outside, "").unwrap();
        for bad in [
            outside.to_str().unwrap().to_owned(),
            format!(
                "{}/grid-0.3.63-linux-arm64/bin/../bin/grid",
                runtime.display()
            ),
            format!("{}/missing/bin/grid", runtime.display()),
            format!("{}/grid-0.3.63-linux-arm64/bin", runtime.display()),
            "grid-0.3.63-linux-arm64/bin/grid".to_owned(),
            format!("{grid} x"),
            String::new(),
        ] {
            fs::write(&record, format!("{bad}\n")).unwrap();
            assert_eq!(pinned_grid(home.path()), None, "{bad:?}");
            env_file_changed(&mut environment, true, None, None).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), TEXT, "{bad:?}");
            assert!(!environment.contains_key(GRID_BIN), "{bad:?}");
        }
    }

    /// With the switch off, no writer runs, whichever the feed.
    #[test]
    fn with_the_switch_off_no_writer_runs_on_any_feed() {
        for exec in [Some(STABLE_LINE), Some(CANARY_LINE), None] {
            let hold = harness_hold(None, exec);
            let mut environment = BTreeMap::from([("HOME".to_owned(), OsString::from("/nowhere"))]);
            assert!(env_file_for(hold, &mut environment, None, None).is_none());
            assert_eq!(pin_at_start(hold, Some(Path::new("/nowhere"))), None);
        }
    }
}
