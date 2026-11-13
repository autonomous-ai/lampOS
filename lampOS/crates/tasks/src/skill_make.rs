//! A voice make handed to the lamp's own Hermes in a skill run's namespace
//! (#6643, the first builder's guard, 12 Nov 18:15): the voice's make that
//! DeepSeek couldn't keep (`lamp_voice::make_skill_agent`,
//! `LAMP_MAKE_SKILL_AGENT`) is written by Hermes started exactly as a
//! skill's `do.task` run is ([`crate::skill_task::HermesRunner`],
//! row-6443's path), through its namespace wrapper: only its own folder,
//! its work folder and the skills, lamp-control's `control.sock` alone and
//! never `admin.sock`. Its terminal and tools work only inside that
//! namespace. No token is minted: a make has no grants. Hermes only
//! ([`MakeHermes`], the first builder's ruling, 12 Nov 19:14): only Hermes
//! has a namespace runner today. Held or not, the wrapper and the namespace
//! script must carry lamp-update's make-home lines ([`WRAPPER_HOME_LINE`],
//! [`NS_HOME_LINE`]), or, with lamp-update's one run path, the one run
//! path's ([`ONE_PATH_HOME_LINE`], [`ONE_PATH_NS_MAKE_LINES`]), or the make
//! is refused [`NO_NAMESPACE`].
//!
//! Each make has a fresh home of its own, `make-<run id>/home`, 0700, as
//! `HOME` and `HERMES_HOME`: empty but for the provider config (its key
//! named by its variable, never a key) and two links to Hermes's prepared
//! runtime ([`RUNTIME_LINKS`]), so a make never prepares dependencies of
//! its own; never Hermes's memory, sessions or state. It goes with the
//! make. lamp-update's namespace script hides
//! Hermes's lasting home from a make (an empty read-only tmpfs over it,
//! only the checkout, its Python tools and its dependency record shown
//! again, read-only) and the rest of the work
//! folder's `lamp` (only the make's folder shown, read-write), and leaves
//! Hermes's folder and the work folder read-only.
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
//!   The answer is read back with `openat` from the folder's own
//!   descriptor, `O_NOFOLLOW` (a regular file, never a link, at most
//!   [`MOST_ANSWER_BYTES`]) once the run has ended and its whole process
//!   group is gone, and the folder removed however the make ends. The
//!   voice puts the answer through the same checks as DeepSeek's.
//! - **The answers**: `{"ok":true,"started":true}` once the run is about
//!   to start (so the voice says its waiting line only then), then
//!   `{"ok":true,"answer"}`; or one refusal: `off`, `bad_request`,
//!   `too_long`, `busy`, `not_held_off`, `no_namespace`, `run_failed` or
//!   `no_answer`.
//! - **The socket** ([`MakeHandOvers`]): with `LAMP_SKILL_TASKS` on (the
//!   switch lamp-update's namespace edits are written with), `serve` binds
//!   [`MAKE_SOCKET`] for lamp-voice alone, as root, one make at a time,
//!   sweeping old make folders first ([`sweep`]). It is not bound only
//!   when the hand-over is on: that switch, `LAMP_MAKE_SKILL_AGENT`, is
//!   lamp-voice's alone (set in its unit, which the unit census keeps), so
//!   lamp-tasks can't see it.
use crate::protocol::refusal;
use crate::skill_task::{NOT_HELD_OFF, Runner, SkillRun, read_line_within, run_id, write_answer};
use serde::Deserialize;
use serde_json::{Value, json};
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

/// The run of a make in `folder`: its words, at most `within`, started in
/// its folder, with its own fresh home ([`HOME_FOLDER`]) as `HOME`,
/// `HERMES_HOME` under it and [`HOME_ENV`] naming it, and nothing else of
/// its own (no token, no grant).
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
        env: home_env(folder).to_vec(),
        deadline: within,
        started: Default::default(),
        start_in: Some(folder.to_path_buf()),
    }
}

/// The variables naming a run's fresh home in `folder` ([`fresh_home`]):
/// `HOME`, `HERMES_HOME` under it and [`HOME_ENV`], the wrapper's mark.
pub(crate) fn home_env(folder: &Path) -> [(&'static str, String); 3] {
    let home = folder.join(HOME_FOLDER);
    let shown = |path: &Path| path.to_string_lossy().into_owned();
    [
        ("HOME", shown(&home)),
        (crate::hold::HERMES_HOME, shown(&home.join(HERMES_FOLDER))),
        (HOME_ENV, shown(&home)),
    ]
}

/// A make's own home in its folder (the first builder's 19:14 ruling on
/// #6643): a fresh, empty folder, 0700, the run's `HOME`, removed with the
/// make.
pub const HOME_FOLDER: &str = "home";
/// Hermes's own folder in it, `HERMES_HOME`, holding only its provider
/// config ([`CONFIG_FILE`]) and the links of [`RUNTIME_LINKS`].
pub const HERMES_FOLDER: &str = ".hermes";
/// The provider config copied in: the lamp's provider, its key a
/// variable's name only (`key_env`, [`crate::hermes_config::TOKEN_ENV`]),
/// never a key, memory, sessions or state.
pub const CONFIG_FILE: &str = "config.yaml";
/// Hermes's prepared runtime, each a link in a make's `HERMES_HOME` to
/// the lasting home's own (#6643, lamp-c0a4): `installs`, its dependency
/// record (`installs/<key>/facts.json`) and environments, and `tools`, its
/// uv, Python and package manager. Hermes 0.22.2 keeps dependency state
/// per data root (`pm/environments.py`, `<HERMES_HOME>/installs/<key>`);
/// with none in a fresh home it syncs one first ("preparing dependencies
/// for this data root"), which a make's run cannot finish, so it ended
/// `run_failed` in under a second. Links, not binds: Hermes takes a
/// recorded environment only under `<HERMES_HOME>/installs/<key>/environments`
/// once resolved, and a link resolves to the lasting path the record
/// names. lamp-update's namespace script ([`NS_HOME_LINE`]) shows both
/// targets read-only, so nothing in them can be changed but what Hermes
/// writes at every start, bound back read-write: each
/// `installs/<key>/.install.lock` (`runtime_lock`) and each lease-managed
/// generation's `installs/<key>/environments/<gen>/.leases`
/// (`lease_directory`), and `installs/<key>/pm-runtime/.prepare.lock`
/// (`pm/runtime.py` `prepare_runtime`) with its generations' `.leases`;
/// and nothing else of the lasting home: never its
/// memory, sessions or state.
pub const RUNTIME_LINKS: [(&str, &str); 2] = [
    ("installs", "/var/lib/lampos-hermes/.hermes/installs"),
    ("tools", "/var/lib/lampos-hermes/.hermes/tools"),
];
/// lamp-update's four Hermes settings (`lamp_update::hermes_install::CONFIG`,
/// a test holds them equal), written into each make's fresh config too:
/// no update checks of its own over the kept network, no language servers,
/// and a `chat -q` run that ends with its turn, not up to 600 s later
/// (which `within_ms` would cut, its answer lost as `run_failed`).
pub const MAKE_CONFIG: [(&str, &str); 4] = [
    ("updates.check", "false"),
    ("plugins.auto_update_check_hours", "0"),
    ("lsp.enabled", "false"),
    ("terminal.oneshot_completion_wait_seconds", "0"),
];
/// The work folder the wrapper takes a make's home in
/// (`lamp_update::hermes_install::MAKE_HOME_GLOB`'s, `make-*/home` in it):
/// a make whose work folder is any other is refused [`NO_NAMESPACE`], as
/// the wrapper would run it in Hermes's lasting home.
pub const MAKE_WORK: &str = "/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp";
/// The variable naming the make's home to Hermes's wrapper
/// (`lamp_update::hermes_install::HOME_ENV`).
pub const HOME_ENV: &str = "LAMP_NS_HOME";
/// The namespace script Hermes's wrapper runs (`lamp_update::hermes_install::NS`).
pub const NS: &str = "/usr/local/lib/lampos/hermes-ns";
/// The wrapper's line for a make's home (`lamp_update::hermes_install::WRAPPER_HOME_LINE`,
/// held equal to it by a test): its mark, checked before every make, held or not.
pub const WRAPPER_HOME_LINE: &str = "case ${LAMP_NS_HOME-} in *..*) unset LAMP_NS_HOME ;; /var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-*/home) export HERMES_HOME=\"$LAMP_NS_HOME/.hermes\" LAMP_NS_HOME ;; *) unset LAMP_NS_HOME ;; esac\n";
/// The namespace script's line for it (`lamp_update::hermes_install::NS_HOME_LINE`).
pub const NS_HOME_LINE: &str = "if [ -n \"${LAMP_NS_HOME-}\" ]; then\n  F=${LAMP_NS_HOME%/home} X=/tmp/.lampos-mk\n  [ -d \"$LAMP_NS_HOME\" ] && [ -n \"$W\" ] && [ \"${F%/*}\" = \"$W/lamp\" ] || fail \"no $LAMP_NS_HOME\"\n  mkdir -m 0700 \"$X\" \"$X/f\" \"$X/c\" \"$X/t\" \"$X/i\" || fail \"no make staging\"\n  mount -n --bind \"$F\" \"$X/f\" || fail \"no $F\"\n  mount -n --bind \"$H/.hermes/hermes-agent\" \"$X/c\" || fail \"no Hermes checkout\"\n  mount -n --bind \"$H/.hermes/tools\" \"$X/t\" || fail \"no Hermes tools\"\n  mount -n --bind \"$H/.hermes/installs\" \"$X/i\" || fail \"no Hermes installs\"\n  k=\n  for p in \"$H/.hermes/installs\"/*/.install.lock; do\n    [ -f \"$p\" ] && [ ! -L \"$p\" ] && [ ! -L \"${p%/.install.lock}\" ] || continue\n    mount -n --bind \"$p\" \"$X/i/${p#\"$H/.hermes/installs/\"}\" || fail \"Hermes install lock not shown\"\n    k=1\n  done\n  [ -n \"$k\" ] || fail \"no Hermes install lock: the lasting Hermes has not run\"\n  for p in \"$H/.hermes/installs\"/*/pm-runtime; do\n    [ -d \"$p\" ] || continue\n    [ ! -L \"$p\" ] && [ ! -L \"${p%/pm-runtime}\" ] && [ -f \"$p/.prepare.lock\" ] && [ ! -L \"$p/.prepare.lock\" ] && mount -n --bind \"$p/.prepare.lock\" \"$X/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\" || fail \"Hermes runtime lock not shown\"\n  done\n  for p in \"$H/.hermes/installs\"/*/environments/*/.lease-managed \"$H/.hermes/installs\"/*/pm-runtime/generations/*/.lease-managed; do\n    [ -f \"$p\" ] || continue\n    g=${p%/.lease-managed} r=${p#\"$H/.hermes/installs/\"}\n    [ ! -L \"$g\" ] && [ ! -L \"$H/.hermes/installs/${r%%/*}\" ] && { [ -d \"$g/.leases\" ] || mkdir \"$g/.leases\"; } && [ ! -L \"$g/.leases\" ] && mount -n --bind \"$g/.leases\" \"$X/i/${g#\"$H/.hermes/installs/\"}/.leases\" || fail \"Hermes leases not shown\"\n  done\n  mount -n -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$H/.hermes\" || fail \"$H/.hermes not hidden\"\n  mkdir \"$H/.hermes/hermes-agent\" && mount -n --move \"$X/c\" \"$H/.hermes/hermes-agent\" && ro \"$H/.hermes/hermes-agent\" || fail \"Hermes checkout not shown\"\n  mkdir \"$H/.hermes/tools\" && mount -n --move \"$X/t\" \"$H/.hermes/tools\" && ro \"$H/.hermes/tools\" || fail \"Hermes tools not shown\"\n  mkdir \"$H/.hermes/installs\" && mount -n --move \"$X/i\" \"$H/.hermes/installs\" && ro \"$H/.hermes/installs\" || fail \"Hermes installs not shown\"\n  ro \"$H/.hermes\"; ro \"$H\"; ro \"$W\"\n  mount -n -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$W/lamp\" || fail \"$W/lamp not hidden\"\n  mkdir \"$F\" && mount -n --move \"$X/f\" \"$F\" || fail \"$F not shown\"\n  ro \"$W/lamp\"\n  rmdir \"$X/f\" \"$X/c\" \"$X/t\" \"$X/i\" \"$X\" || fail \"make staging left\"\n  cd \"$F\" || fail \"$F not visible\"\n  HOME=$LAMP_NS_HOME\nfi\nunset LAMP_NS_HOME F X k p g r\n";
/// With lamp-update's one run path (`LAMP_SKILL_DEFAULT_AGENT` on, #6685),
/// Hermes's wrapper is the one run path's
/// ([`crate::skill_task::wrapper_takes_runs`]) and runs
/// [`crate::skill_task::AGENT_NS`]; its data's make-home line
/// (`lamp_update::hermes_install::ONE_PATH_HOME_LINE`, held equal to it by
/// a test) hands the make's home on as `LAMP_NS_MAKE`.
pub const ONE_PATH_HOME_LINE: &str = "  case ${LAMP_NS_HOME-} in *..*) unset LAMP_NS_MAKE ;; /var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-*/home) LAMP_NS_MAKE=$LAMP_NS_HOME HERMES_HOME=\"$LAMP_NS_HOME/.hermes\"; export LAMP_NS_MAKE HERMES_HOME ;; *) unset LAMP_NS_MAKE ;; esac\n";
/// The one agent namespace's make lines for Hermes
/// (`lamp_update::hermes_install::ONE_PATH_NS_MAKE_LINES`).
pub const ONE_PATH_NS_MAKE_LINES: &str = "  F=${LAMP_NS_MAKE%/home} Y=/tmp/.lampos-mk\n  [ -d \"$LAMP_NS_MAKE\" ] && [ -n \"$W\" ] && [ \"${F%/*}\" = \"$W/lamp\" ] || fail \"no $LAMP_NS_MAKE\"\n  mkdir -m 0700 \"$Y\" \"$Y/f\" \"$Y/c\" \"$Y/t\" \"$Y/i\" || fail \"no make staging\"\n  mount -n --bind \"$F\" \"$Y/f\" || fail \"no $F\"\n  mount -n --bind \"$H/.hermes/hermes-agent\" \"$Y/c\" || fail \"no Hermes checkout\"\n  mount -n --bind \"$H/.hermes/tools\" \"$Y/t\" || fail \"no Hermes tools\"\n  mount -n --bind \"$H/.hermes/installs\" \"$Y/i\" || fail \"no Hermes installs\"\n  k=\n  for p in \"$H/.hermes/installs\"/*/.install.lock; do\n    [ -f \"$p\" ] && [ ! -L \"$p\" ] && [ ! -L \"${p%/.install.lock}\" ] || continue\n    mount -n --bind \"$p\" \"$Y/i/${p#\"$H/.hermes/installs/\"}\" || fail \"Hermes install lock not shown\"\n    k=1\n  done\n  [ -n \"$k\" ] || fail \"no Hermes install lock: the lasting Hermes has not run\"\n  for p in \"$H/.hermes/installs\"/*/pm-runtime; do\n    [ -d \"$p\" ] || continue\n    [ ! -L \"$p\" ] && [ ! -L \"${p%/pm-runtime}\" ] && [ -f \"$p/.prepare.lock\" ] && [ ! -L \"$p/.prepare.lock\" ] && mount -n --bind \"$p/.prepare.lock\" \"$Y/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\" || fail \"Hermes runtime lock not shown\"\n  done\n  for p in \"$H/.hermes/installs\"/*/environments/*/.lease-managed \"$H/.hermes/installs\"/*/pm-runtime/generations/*/.lease-managed; do\n    [ -f \"$p\" ] || continue\n    g=${p%/.lease-managed} r=${p#\"$H/.hermes/installs/\"}\n    [ ! -L \"$g\" ] && [ ! -L \"$H/.hermes/installs/${r%%/*}\" ] && { [ -d \"$g/.leases\" ] || mkdir \"$g/.leases\"; } && [ ! -L \"$g/.leases\" ] && mount -n --bind \"$g/.leases\" \"$Y/i/${g#\"$H/.hermes/installs/\"}/.leases\" || fail \"Hermes leases not shown\"\n  done\n  mount -n -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$H/.hermes\" || fail \"$H/.hermes not hidden\"\n  mkdir \"$H/.hermes/hermes-agent\" && mount -n --move \"$Y/c\" \"$H/.hermes/hermes-agent\" && ro \"$H/.hermes/hermes-agent\" || fail \"Hermes checkout not shown\"\n  mkdir \"$H/.hermes/tools\" && mount -n --move \"$Y/t\" \"$H/.hermes/tools\" && ro \"$H/.hermes/tools\" || fail \"Hermes tools not shown\"\n  mkdir \"$H/.hermes/installs\" && mount -n --move \"$Y/i\" \"$H/.hermes/installs\" && ro \"$H/.hermes/installs\" || fail \"Hermes installs not shown\"\n  ro \"$H/.hermes\"; ro \"$H\"; ro \"$W\"\n  mount -n -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$W/lamp\" || fail \"$W/lamp not hidden\"\n  mkdir \"$F\" && mount -n --move \"$Y/f\" \"$F\" || fail \"$F not shown\"\n  ro \"$W/lamp\"\n  rmdir \"$Y/f\" \"$Y/c\" \"$Y/t\" \"$Y/i\" \"$Y\" || fail \"make staging left\"\n  cd \"$F\" || fail \"$F not visible\"\n  HOME=$LAMP_NS_MAKE\n  unset F Y k p g r\n";
/// Said when Hermes's wrapper or namespace script has no make-home line.
pub const NO_NAMESPACE: &str = "the lamp's Hermes can't be given a make's own home yet (its wrapper or namespace script has no make-home line: lamp-update's LAMP_SKILL_TASKS is off or its round hasn't run), so nothing was started";

/// A make's folder, held open from when it was made, and removed however
/// the make ends: an answer, a failure, its deadline or a panic. An agent
/// skill's run starts in one of its own too (`crate::skill_task`,
/// `LAMP_UPLOAD_TO_AGENT`).
pub(crate) struct MadeFolder {
    pub(crate) path: PathBuf,
    fd: rustix::fd::OwnedFd,
}

impl Drop for MadeFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

const DIR_FLAGS: rustix::fs::OFlags = rustix::fs::OFlags::DIRECTORY
    .union(rustix::fs::OFlags::NOFOLLOW)
    .union(rustix::fs::OFlags::RDONLY)
    .union(rustix::fs::OFlags::CLOEXEC);

/// Folder `name` made 0700 in `at` and opened, never through a link.
fn folder_in(at: &rustix::fd::OwnedFd, name: &str) -> std::io::Result<rustix::fd::OwnedFd> {
    use rustix::fs::{Mode, fchmod, mkdirat, openat};
    mkdirat(at, name, Mode::from_raw_mode(0o700))?;
    let fd = openat(at, name, DIR_FLAGS, Mode::empty())?;
    fchmod(&fd, Mode::from_raw_mode(0o700))?;
    Ok(fd)
}

/// File `name` made new, 0600, in `at` with `text`, never through a link.
fn file_in(at: &rustix::fd::OwnedFd, name: &str, text: &str) -> std::io::Result<()> {
    use rustix::fs::{Mode, OFlags, openat};
    use std::io::Write;
    let fd = openat(
        at,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?;
    std::fs::File::from(fd).write_all(text.as_bytes())
}

/// The make's folder in `work` with its rules, and its fresh home with
/// only the provider config and the runtime links ([`RUNTIME_LINKS`]):
/// each folder 0700 and each file 0600, made
/// before a byte is written, each part opened from the one above, never
/// through a link.
fn put(work: &Path, rules: &str) -> std::io::Result<MadeFolder> {
    let made = fresh_home(work)?;
    file_in(&made.fd, RULES_FILE, rules)?;
    Ok(made)
}

/// A run's own folder in `work`, `make-<run id>`, and its fresh home in it
/// ([`HOME_FOLDER`], [`HERMES_FOLDER`]) with only the provider config
/// ([`make_config`]): a make's, and an agent skill's run's or a `net`
/// run's (`crate::skill_task`), so the wrapper takes it as the run's home
/// and its namespace script hides Hermes's lasting home, the person's
/// memories and sessions, from the run ([`WRAPPER_HOME_LINE`],
/// [`NS_HOME_LINE`]). Removed however the run ends.
pub(crate) fn fresh_home(work: &Path) -> std::io::Result<MadeFolder> {
    fresh_home_for(work, false)
}

/// [`fresh_home`], its config with the lamp's agent's connectors
/// (`mcp_servers.lamp-services`, [`crate::hermes_config::mcp_entry`]) when
/// `services`: an agent skill's run granted a linked service
/// (`crate::skill_task`, `LAMP_AGENT_SERVICES` on in lamp-tasks), which
/// reaches it only through lamp-control under the run's one-run token.
pub(crate) fn fresh_home_for(work: &Path, services: bool) -> std::io::Result<MadeFolder> {
    let made = fresh_folder(work, FOLDER_HEAD)?;
    let home = folder_in(&made.fd, HOME_FOLDER)?;
    let hermes = folder_in(&home, HERMES_FOLDER)?;
    file_in(&hermes, CONFIG_FILE, &make_config(services)?)?;
    for (name, target) in RUNTIME_LINKS {
        rustix::fs::symlinkat(target, &hermes, name)?;
    }
    Ok(made)
}

/// A fresh folder `<head><run id>` in `work`, 0700 before anything is in
/// it, opened never through a link and removed however its run ends.
fn fresh_folder(work: &Path, head: &str) -> std::io::Result<MadeFolder> {
    use rustix::fs::{Mode, open};
    let name = format!("{head}{}", run_id());
    let at = open(work, DIR_FLAGS, Mode::empty())?;
    let fd = folder_in(&at, &name)?;
    Ok(MadeFolder {
        path: work.join(&name),
        fd,
    })
}

/// A make's fresh config: the lamp's provider ([`crate::hermes_config::merge_config`],
/// its key a variable's name only), the lamp's agent's connectors only when
/// `services`, and [`MAKE_CONFIG`]. A setting whose top-level block the
/// provider config already has (`terminal:`, with its `shell_init_files`)
/// goes into that block, as its first line: a second block of the same
/// name is a duplicate key, and Hermes refuses the whole file (exit 2,
/// c0a4, 14 Nov). Any other is a block of its own.
fn make_config(services: bool) -> std::io::Result<String> {
    let mut config =
        crate::hermes_config::merge_config_for("", crate::runtime::HERMES_FORWARDER_PORT, services)
            .map_err(|_| std::io::Error::other("the provider config was not made"))?;
    for (key, value) in MAKE_CONFIG {
        let (block, name) = key.split_once('.').unwrap_or(("", key));
        let head = format!("{block}:\n");
        let line = format!("  {name}: {value}\n");
        let at = if config.starts_with(&head) {
            Some(0)
        } else {
            config.find(&format!("\n{head}")).map(|at| at + 1)
        };
        match at {
            Some(at) => config.insert_str(at + head.len(), &line),
            None => config.push_str(&format!("{head}{line}")),
        }
    }
    Ok(config)
}

/// The answer the run wrote, read from the make's folder as it was made
/// (its descriptor, so a folder swapped for a link is never followed): a
/// regular file, never a link, of at most [`MOST_ANSWER_BYTES`] of text.
fn answer_read(folder: &rustix::fd::OwnedFd) -> Option<String> {
    use rustix::fs::{Mode, OFlags, openat};
    use std::io::Read;
    let fd = openat(
        folder,
        ANSWER_FILE,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .ok()?;
    let file = std::fs::File::from(fd);
    let meta = file.metadata().ok()?;
    if !meta.file_type().is_file() || meta.len() > MOST_ANSWER_BYTES {
        return None;
    }
    let mut text = String::new();
    file.take(MOST_ANSWER_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    (!text.trim().is_empty() && text.len() as u64 <= MOST_ANSWER_BYTES).then_some(text)
}

/// Make folders an earlier lamp-tasks left in `work` (stopped while a make
/// ran), removed as it starts: only folders named `make-*`, never a link.
pub fn sweep(work: &Path) {
    let Ok(entries) = std::fs::read_dir(work) else {
        return;
    };
    let mut swept = 0;
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(FOLDER_HEAD)
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && std::fs::remove_dir_all(entry.path()).is_ok()
        {
            swept += 1;
        }
    }
    if swept > 0 {
        eprintln!("[lamp-tasks] skill make: {swept} old make folder(s) removed");
    }
}

/// The refusal's code for why a run can't start.
fn not_started(why: &str) -> &'static str {
    match why {
        NOT_HELD_OFF => "not_held_off",
        NO_NAMESPACE => "no_namespace",
        _ => "run_failed",
    }
}

/// lamp-tasks' answer to one make `line`: with the switch `on`, the line
/// checked, the run's namespace checked ([`Runner::ready`]), its folder
/// and fresh home made in `work`, `started` told, the run made and waited
/// for, its answer read and the folder removed; else the refusal.
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
            "a skill run's namespace is off on this lamp (LAMP_SKILL_TASKS)",
        );
    }
    let make = match MakeLine::parse(line) {
        Ok(make) => make,
        Err((code, why)) => return refusal(code, &why),
    };
    if let Err(why) = runner.ready() {
        return refusal(not_started(&why), &why);
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
    let ran = runner.run(&make_run(&folder.path, make.within()), &|| {});
    match ran {
        Err(why) => refusal(not_started(&why), &why),
        Ok(_) => match answer_read(&folder.fd) {
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

/// The switch for a make's network: the first builder's 19:14 ruling on
/// #6643 keeps it (the owner's ask, as V1's agent had it), on the
/// condition that each make has a fresh home and work folder, dropped
/// after ([`put`]). Exactly [`NET_HELD`] holds the run off it
/// ([`crate::skill_task::NET_ENV`] `run`: only `lo` and the kernel's
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

/// The lamp's Hermes for a make, and only Hermes: it alone has a namespace
/// runner today ([`crate::skill_task::HermesRunner`]); the voice hands a
/// make over only when the lamp's agent is Hermes. Before every make,
/// held or not, its wrapper and namespace script must carry the make-home
/// lines ([`WRAPPER_HOME_LINE`], [`NS_HOME_LINE`]: lamp-update's, so the
/// wrapper is its namespacing one, never Hermes's own program), or, with
/// lamp-update's one run path, the one run path's Hermes wrapper with its
/// make-home line ([`ONE_PATH_HOME_LINE`]) and the one agent namespace
/// with Hermes's make lines ([`ONE_PATH_NS_MAKE_LINES`]); and, held, the
/// network lines too (the one run path holds only a skill's run off the
/// network, so a held make is refused there).
pub struct MakeHermes {
    pub hermes: crate::skill_task::HermesRunner,
    /// The work folder the wrapper takes a make's home in ([`MAKE_WORK`]);
    /// the runner's own must be it.
    pub make_work: PathBuf,
    /// Hermes's wrapper ([`crate::skill_task::HERMES`]).
    pub wrapper: PathBuf,
    /// Its namespace script ([`NS`]).
    pub ns: PathBuf,
    /// The one agent namespace ([`crate::skill_task::AGENT_NS`]), which
    /// the wrapper runs instead with the one run path.
    pub agent_ns: PathBuf,
}

impl Runner for MakeHermes {
    fn ready(&self) -> Result<(), String> {
        self.hermes.ready()?;
        let has = |path: &Path, line: &str| {
            std::fs::read_to_string(path).is_ok_and(|text| text.contains(line))
        };
        let own = has(&self.wrapper, WRAPPER_HOME_LINE) && has(&self.ns, NS_HOME_LINE);
        let one_path = std::fs::read_to_string(&self.wrapper).is_ok_and(|text| {
            crate::skill_task::wrapper_takes_runs(&text, crate::harness::EngineKind::Hermes)
                && text.contains(ONE_PATH_HOME_LINE)
        }) && has(&self.agent_ns, ONE_PATH_NS_MAKE_LINES);
        if self.hermes.folder != self.make_work || !(own || one_path) {
            return Err(NO_NAMESPACE.into());
        }
        Ok(())
    }

    fn run(&self, run: &SkillRun, end_token: &dyn Fn()) -> Result<u64, String> {
        self.ready()?;
        self.hermes.run(run, end_token)
    }
}

/// The lamp's Hermes for a make ([`crate::skill_task::HermesRunner::lamp`]),
/// started through its namespace wrapper either way (only its own folder,
/// its lasting home hidden from a make but for the checkout, its work
/// folder, all but the make's own read-only, and the skills, `control.sock`
/// alone of lamp-control's and never `admin.sock`), with the make's own
/// home. `held`: with [`crate::skill_task::NET_ENV`] `run`, refused when
/// the wrapper can't hold it off. Not held: without it, so the wrapper
/// keeps the lamp's network, as Harness's own panes do (the model then
/// reached at lamp-cloud's forwarder, the one `model.sock` pipes to).
pub fn lamp_runner(home: &Path, held: bool) -> MakeHermes {
    let mut hermes = crate::skill_task::HermesRunner::lamp(home);
    if !held {
        hermes
            .env
            .retain(|(name, _)| name != crate::skill_task::NET_ENV);
        hermes.held_by = None;
    }
    MakeHermes {
        hermes,
        make_work: PathBuf::from(MAKE_WORK),
        wrapper: PathBuf::from(crate::skill_task::HERMES),
        ns: PathBuf::from(NS),
        agent_ns: PathBuf::from(crate::skill_task::AGENT_NS),
    }
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
            work: runner.hermes.folder.clone(),
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
        // What a lamp-tasks stopped mid-make left, before any make of its own.
        sweep(&self.work);
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
        eprintln!("{}", journal_line(&answered));
        write_answer(stream, &answered);
    }
}

/// The journal's line for a make's outcome: its code and, refused, the
/// refusal's reason in one line. Every reason is a fixed form (a path or
/// an exit status at most), never the rules or the answer.
fn journal_line(answered: &Value) -> String {
    if answered["ok"].as_bool() == Some(true) {
        return "[lamp-tasks] skill make: the run ended with an answer".into();
    }
    let why: String = answered["message"]
        .as_str()
        .unwrap_or("?")
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    format!(
        "[lamp-tasks] skill make: refused {}: {why}",
        answered["error"].as_str().unwrap_or("?")
    )
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
    /// environment, deadline and what its home held at its start.
    type SeenRun = (
        String,
        PathBuf,
        Vec<(&'static str, String)>,
        Duration,
        Vec<String>,
    );

    /// What a test's runner saw of a make's runs, kept by it.
    #[derive(Default)]
    struct Seen(Mutex<Vec<SeenRun>>);
    use crate::skill_task::{ADMIN_SOCKET, HERMES, NET_ENV, NET_RUN, TOKEN_ENV, WRAPPER_NET_LINES};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;

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
        /// What the run ends with, after it wrote in its home.
        result: Result<u64, String>,
    }

    impl Runner for Writing {
        fn ready(&self) -> Result<(), String> {
            self.ready.clone()
        }

        fn run(&self, run: &SkillRun, _: &dyn Fn()) -> Result<u64, String> {
            let folder = run.start_in.clone().unwrap();
            let home = folder.join(HOME_FOLDER);
            self.seen.0.lock().unwrap().push((
                std::fs::read_to_string(folder.join(RULES_FILE)).unwrap(),
                folder.clone(),
                run.env.clone(),
                run.deadline,
                listing(&home),
            ));
            // What a run leaves in its home: never seen by the next make.
            std::fs::write(home.join("memory.md"), "LEFT BEHIND").unwrap();
            std::fs::write(home.join(HERMES_FOLDER).join("state.db"), "S").unwrap();
            if let Some(answer) = self.answer {
                std::fs::write(folder.join(ANSWER_FILE), answer).unwrap();
            }
            self.result.clone()
        }
    }

    fn writing(answer: Option<&'static str>) -> Writing {
        Writing {
            answer,
            seen: Seen::default(),
            ready: Ok(()),
            result: Ok(0),
        }
    }

    /// What a make's own home held when its run began: every path in it.
    fn listing(home: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let mut folders = vec![home.to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = entry.path();
                found.push(path.strip_prefix(home).unwrap().display().to_string());
                if entry.file_type().unwrap().is_dir() {
                    folders.push(path);
                }
            }
        }
        found.sort();
        found
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
        let (rules, folder, env, deadline, _) = &seen[0];
        assert_eq!(rules, "SYSTEM\n\nBRIEF");
        assert!(folder.starts_with(work.path()), "{folder:?}");
        // Its own home and nothing else: no token, no grant, no socket.
        let home = folder.join(HOME_FOLDER).display().to_string();
        assert_eq!(
            env,
            &vec![
                ("HOME", home.clone()),
                ("HERMES_HOME", format!("{home}/.hermes")),
                (HOME_ENV, home),
            ]
        );
        assert_eq!(*deadline, Duration::from_secs(3));
        // The folder is gone once the answer is in.
        assert!(!folder.exists());
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
    }

    /// The first builder's 19:14 ruling on #6643: each make starts with a
    /// fresh, empty home but for the provider config (a variable's name for
    /// its key, never a key), never sees what an earlier make left in its
    /// home, and its home is gone after however it ended: answered, failed
    /// or past its deadline.
    #[test]
    fn each_make_has_a_fresh_home_dropped_after() {
        let work = tempfile::tempdir().unwrap();
        for result in [
            Ok(0),
            Err("the lamp's Hermes ended with status 1".to_owned()),
            Err("the run was ended at its deadline, 2 s".to_owned()),
            Ok(0),
        ] {
            let mut runner = writing(Some("A"));
            runner.result = result.clone();
            let answered = answer(&line("R", 2_000), true, work.path(), &runner, &mut || {});
            assert_eq!(answered["ok"], result.is_ok(), "{answered}");
            let seen = runner.seen.0.lock().unwrap();
            let (_, folder, _, _, at_start) = &seen[0];
            assert_eq!(
                at_start,
                &vec![
                    ".hermes".to_owned(),
                    ".hermes/config.yaml".to_owned(),
                    ".hermes/installs".to_owned(),
                    ".hermes/tools".to_owned(),
                ],
                "only the provider config and the runtime links, never what an earlier make left"
            );
            assert!(!folder.exists(), "dropped after {result:?}");
            assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
        }
        // The provider config names its key's variable, never a key.
        let made = put(work.path(), "R").unwrap();
        let config = std::fs::read_to_string(
            made.path
                .join(HOME_FOLDER)
                .join(HERMES_FOLDER)
                .join(CONFIG_FILE),
        )
        .unwrap();
        assert!(
            config.contains("key_env: \"LAMP_HERMES_TOKEN\""),
            "{config}"
        );
        assert!(config.contains("127.0.0.1:7741"), "{config}");
        // Hermes's prepared runtime (#6643, c0a4): its dependency record
        // and its store, each a link to the lasting home's own, which the
        // namespace shows read-only; never its memory, sessions or state.
        let hermes = made.path.join(HOME_FOLDER).join(HERMES_FOLDER);
        for name in ["installs", "tools"] {
            assert_eq!(
                std::fs::read_link(hermes.join(name)).unwrap(),
                Path::new("/var/lib/lampos-hermes/.hermes").join(name),
                "{name}"
            );
        }
        // Each top-level key once: a second `terminal:` is a duplicate key
        // and Hermes refuses the whole file (exit 2, c0a4, 14 Nov).
        let mut tops: Vec<&str> = config
            .lines()
            .filter(|line| !line.starts_with([' ', '#', '-']) && line.contains(':'))
            .map(|line| line.split(':').next().unwrap())
            .collect();
        let all = tops.len();
        tops.sort_unstable();
        tops.dedup();
        assert_eq!(tops.len(), all, "{config}");
        assert!(
            config.contains("\nterminal:\n") || config.starts_with("terminal:\n"),
            "{config}"
        );
        // lamp-update's four settings, the same as its own.
        for (key, value) in MAKE_CONFIG {
            let (block, name) = key.split_once('.').unwrap();
            assert!(
                config.contains(&format!("\n{block}:\n  {name}: {value}\n")),
                "{config}"
            );
        }
        let source = include_str!("../../update/src/hermes_install.rs");
        let theirs = source
            .split("pub const CONFIG: [(&str, &str); 4] = [\n")
            .nth(1)
            .and_then(|rest| rest.split("];").next())
            .unwrap();
        let ours: String = MAKE_CONFIG
            .iter()
            .map(|(key, value)| format!("    ({key:?}, {value:?}),\n"))
            .collect();
        assert_eq!(theirs, ours);
        assert!(!config.contains("tok_"), "{config}");
        use std::os::unix::fs::MetadataExt;
        for (path, mode) in [
            (made.path.clone(), 0o700),
            (made.path.join(HOME_FOLDER), 0o700),
            (made.path.join(HOME_FOLDER).join(HERMES_FOLDER), 0o700),
            (made.path.join(RULES_FILE), 0o600),
        ] {
            assert_eq!(
                std::fs::metadata(&path).unwrap().mode() & 0o777,
                mode,
                "{path:?}"
            );
        }
        let path = made.path.clone();
        drop(made);
        assert!(!path.exists());
        // A panic while the make runs drops it too.
        let panicking = std::panic::catch_unwind(|| {
            let made = put(work.path(), "R").unwrap();
            let _keep = &made;
            panic!("in the run");
        });
        assert!(panicking.is_err());
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
    }

    /// A run that can't be held off the network, or whose namespace has no
    /// make-home line, starts nothing and makes no folder; one with no
    /// answer, or an answer that is a link, is `no_answer`; the switch off
    /// and a bad line refuse before anything.
    #[test]
    fn no_namespace_no_make_and_no_answer_is_refused() {
        let work = tempfile::tempdir().unwrap();
        for (why, code) in [
            (NOT_HELD_OFF, "not_held_off"),
            (NO_NAMESPACE, "no_namespace"),
        ] {
            let mut refusing = writing(Some("x"));
            refusing.ready = Err(why.into());
            let mut started = false;
            let refused = answer(&line("R", 2_000), true, work.path(), &refusing, &mut || {
                started = true
            });
            assert_eq!(refused["error"], code);
            assert!(!started);
            assert!(refusing.seen.0.lock().unwrap().is_empty());
            assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
        }
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
        // The answer is read from the folder as made, never through a link.
        let made = put(work.path(), "R").unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::write(elsewhere.path().join("secret"), "SECRET").unwrap();
        assert_eq!(answer_read(&made.fd), None);
        std::os::unix::fs::symlink(elsewhere.path().join("secret"), made.path.join(ANSWER_FILE))
            .unwrap();
        assert_eq!(answer_read(&made.fd), None, "never through a link");
        std::fs::remove_file(made.path.join(ANSWER_FILE)).unwrap();
        std::fs::write(made.path.join(ANSWER_FILE), "MINE").unwrap();
        // The folder swapped for a link after it was made: still its own.
        let moved = work.path().join("moved");
        std::fs::rename(&made.path, &moved).unwrap();
        std::fs::write(elsewhere.path().join(ANSWER_FILE), "THEIRS").unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), &made.path).unwrap();
        assert_eq!(answer_read(&made.fd).as_deref(), Some("MINE"));
        drop(made);
        std::fs::remove_dir_all(&moved).unwrap();
    }

    /// Stand-ins for lamp-update's wrapper and namespace script, with the
    /// lines given.
    fn namespace(dir: &Path, wrapper: &str, ns: &str) -> (PathBuf, PathBuf) {
        let (w, n) = (dir.join("hermes"), dir.join("hermes-ns"));
        std::fs::write(&w, wrapper).unwrap();
        std::fs::write(&n, ns).unwrap();
        (w, n)
    }

    /// The make's run is the lamp's Hermes exactly as a skill's run is
    /// started (row-6443's path), through its wrapper: held, with
    /// `LAMP_NS_NET=run`; never a token or the admin socket; started in its
    /// make's folder with its own home. Held or not, the wrapper and the
    /// namespace script must carry the make-home lines, lamp-update's own
    /// (so never Hermes's own program); held, the network lines too.
    #[test]
    fn a_make_is_started_through_the_marked_namespace_held_or_not() {
        let dir = tempfile::tempdir().unwrap();
        let lamp = MakeHandOvers::lamp(Path::new("/var/lib/lampos-tasks"));
        assert_eq!(
            lamp.work,
            Path::new("/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp")
        );
        let work = dir.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let script = format!(
            "pwd > {seen}/cwd; env > {seen}/env; printf '%s' \"$1\" > {seen}/words; \
             ls -A \"$HOME\" > {seen}/home; cp {RULES_FILE} {seen}/rules; \
             printf 'ANSWER' > {ANSWER_FILE}",
            seen = dir.path().display()
        );
        let make = |held: bool, wrapper: &str, ns: &str| {
            let (w, n) = namespace(dir.path(), wrapper, ns);
            let mut runner = lamp_runner(dir.path(), held);
            runner.hermes.program = PathBuf::from("/bin/sh");
            runner.hermes.args = vec!["-c".into(), script.clone(), "sh".into()];
            runner.hermes.folder = work.clone();
            runner.make_work = work.clone();
            runner.hermes.tick = Duration::from_millis(10);
            if held {
                runner.hermes.held_by = Some(w.clone());
            }
            runner.wrapper = w;
            runner.ns = n;
            let _ = std::fs::remove_file(dir.path().join("env"));
            answer(&line("RULES", 5_000), true, &work, &runner, &mut || {})
        };
        let net = format!("{}{}", WRAPPER_NET_LINES[0], WRAPPER_NET_LINES[1]);
        let marked =
            format!("#!/bin/sh\n{WRAPPER_HOME_LINE}unset T\n{net}--pid hermes-ns \"$@\"\n");
        // Not held: Hermes's own program, or a wrapper or script without
        // the make-home line, starts nothing.
        for (wrapper, ns) in [
            ("#!/usr/bin/env python3\n# hermes\n", NS_HOME_LINE),
            (net.as_str(), NS_HOME_LINE),
            (marked.as_str(), "#!/bin/sh\n"),
        ] {
            let refused = make(false, wrapper, ns);
            assert_eq!(refused["error"], "no_namespace", "{refused}");
            assert!(!dir.path().join("env").exists(), "nothing started");
        }
        // A work folder other than the one the wrapper takes a make's
        // home in: the make would run in Hermes's lasting home.
        let (w, n) = namespace(dir.path(), &marked, NS_HOME_LINE);
        let mut elsewhere = lamp_runner(dir.path(), false);
        elsewhere.hermes.folder = work.clone();
        (elsewhere.wrapper, elsewhere.ns) = (w, n);
        assert_eq!(elsewhere.ready(), Err(NO_NAMESPACE.to_owned()));
        assert_eq!(
            lamp_runner(Path::new("/var/lib/lampos-tasks"), false)
                .hermes
                .folder,
            Path::new(MAKE_WORK)
        );
        assert!(WRAPPER_HOME_LINE.contains(&format!(";; {MAKE_WORK}/make-*/home)")));
        // Held: the network lines too.
        let unheld = format!("#!/bin/sh\n{WRAPPER_HOME_LINE}unset T\n");
        let refused = make(true, &unheld, NS_HOME_LINE);
        assert_eq!(refused["error"], "not_held_off", "{refused}");
        assert!(!dir.path().join("env").exists());
        for held in [false, true] {
            let answered = make(held, &marked, NS_HOME_LINE);
            assert_eq!(answered, json!({"ok": true, "answer": "ANSWER"}), "{held}");
            let env = std::fs::read_to_string(dir.path().join("env")).unwrap();
            assert_eq!(env.contains("LAMP_NS_NET=run"), held, "{env}");
            assert!(!env.contains(TOKEN_ENV), "{env}");
            assert!(!env.contains(ADMIN_SOCKET), "{env}");
            assert!(!env.contains("CARGO"), "{env}");
            let cwd = std::fs::read_to_string(dir.path().join("cwd")).unwrap();
            let cwd = Path::new(cwd.trim());
            assert!(
                cwd.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(FOLDER_HEAD),
                "{cwd:?}"
            );
            // pwd names the folder through /private on macOS; the home is
            // the same folder's.
            let home = work
                .join(cwd.file_name().unwrap())
                .join(HOME_FOLDER)
                .display()
                .to_string();
            let lines: Vec<&str> = env.lines().collect();
            assert!(lines.contains(&format!("HOME={home}").as_str()), "{env}");
            assert!(lines.contains(&format!("HERMES_HOME={home}/.hermes").as_str()));
            assert!(lines.contains(&format!("LAMP_NS_HOME={home}").as_str()));
            assert_eq!(
                std::fs::read_to_string(dir.path().join("home")).unwrap(),
                ".hermes\n"
            );
            let words = std::fs::read_to_string(dir.path().join("words")).unwrap();
            assert!(words.contains(ANSWER_FILE) && words.contains(RULES_FILE));
            assert_eq!(
                std::fs::read_to_string(dir.path().join("rules")).unwrap(),
                "RULES"
            );
            assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0, "removed");
        }
        // The lines are lamp-update's own, word for word.
        let source = include_str!("../../update/src/hermes_install.rs");
        for (name, line) in [
            ("WRAPPER_HOME_LINE", WRAPPER_HOME_LINE),
            ("NS_HOME_LINE", NS_HOME_LINE),
        ] {
            assert!(
                source.contains(&format!("pub const {name}: &str = {line:?};")),
                "{name}"
            );
        }
        assert!(source.contains(&format!("pub const NS: &str = {NS:?};")));
        assert!(source.contains(&format!("pub const HOME_ENV: &str = {HOME_ENV:?};")));
        assert!(
            WRAPPER_HOME_LINE
                .contains("/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-*/home")
        );
        // The namespace shows control.sock alone and never names admin.sock.
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

    /// Hermes 0.22.2 writes under `installs` at every start
    /// (`hermes_cli/runtime_state.py`): `runtime_lock` opens
    /// `<key>/.install.lock` `O_CREAT|O_RDWR`, and `lease_directory`
    /// creates and flocks a lease in `<key>/environments/<gen>/.leases`;
    /// its PM runtime (`pm/runtime.py` `prepare_runtime`) opens
    /// `<key>/pm-runtime/.prepare.lock` `a+b` and leases
    /// `<key>/pm-runtime/generations/<gen>/.leases`.
    /// The make's namespace binds exactly those back read-write, from the
    /// lasting home before it is hidden, onto the shown `installs` before
    /// its read-only remount; no lock at all ends it before Hermes runs.
    #[test]
    fn a_make_may_write_hermess_install_lock_and_leases_only() {
        let shown = NS_HOME_LINE.find("mount -n --move \"$X/i\"").unwrap();
        let hidden = NS_HOME_LINE.find("none \"$H/.hermes\"").unwrap();
        for bind in [
            "mount -n --bind \"$p\" \"$X/i/${p#\"$H/.hermes/installs/\"}\"",
            "mount -n --bind \"$p/.prepare.lock\" \"$X/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\"",
            "mount -n --bind \"$g/.leases\" \"$X/i/${g#\"$H/.hermes/installs/\"}/.leases\"",
        ] {
            let at = NS_HOME_LINE.find(bind).expect(bind);
            assert!(at < hidden && at < shown, "{bind}");
        }
        assert!(NS_HOME_LINE.contains("\"$H/.hermes/installs\"/*/.install.lock"));
        assert!(NS_HOME_LINE.contains(
            "\"$H/.hermes/installs\"/*/environments/*/.lease-managed \"$H/.hermes/installs\"/*/pm-runtime/generations/*/.lease-managed"
        ));
        assert!(NS_HOME_LINE.contains("\"$H/.hermes/installs\"/*/pm-runtime; do"));
        assert!(NS_HOME_LINE.contains(
            "[ -n \"$k\" ] || fail \"no Hermes install lock: the lasting Hermes has not run\""
        ));
        // Nothing else of `installs`, nor `tools`, is bound back.
        assert_eq!(NS_HOME_LINE.matches("mount -n --bind").count(), 7);
        for kept in [
            "facts.json",
            "publication.json",
            "selected.json",
            "venv",
            "tools\"/",
        ] {
            assert!(!NS_HOME_LINE.contains(kept), "{kept}");
        }
        assert!(NS_HOME_LINE.contains("ro \"$H/.hermes/installs\""));
        assert!(NS_HOME_LINE.contains("ro \"$H/.hermes/tools\""));
    }

    /// The make's lines run after `/run` is read-only, where util-linux
    /// cannot write its utab: a `--move` without `-n` failed there on a
    /// lamp ("Hermes checkout not shown"). Every mount the lines make is
    /// `mount -n` (the `ro` helper's remounts passed there as they are).
    #[test]
    fn every_mount_a_makes_lines_make_skips_the_utab() {
        assert_eq!(NS_HOME_LINE.matches("mount -n --move").count(), 4);
        assert_eq!(
            NS_HOME_LINE.matches("mount ").count(),
            NS_HOME_LINE.matches("mount -n ").count()
        );
    }

    /// A refused make is journalled with its code and its reason, in one
    /// line; an answered one with no words of it.
    #[test]
    fn a_refused_make_is_journalled_with_its_reason() {
        assert_eq!(
            journal_line(&refusal(
                "run_failed",
                "the lamp's Hermes ended with status 1"
            )),
            "[lamp-tasks] skill make: refused run_failed: the lamp's Hermes ended with status 1"
        );
        assert_eq!(
            journal_line(&refusal("no_answer", "a\nb")),
            "[lamp-tasks] skill make: refused no_answer: a b"
        );
        assert_eq!(
            journal_line(&json!({"ok": true, "answer": "THE ANSWER"})),
            "[lamp-tasks] skill make: the run ended with an answer"
        );
    }

    /// The first builder's 19:14 ruling: by default a make's run keeps the
    /// network, still in Hermes's namespace, so `control.sock` alone and
    /// never `admin.sock`; `LAMP_MAKE_AGENT_NET=held` exactly gives the
    /// strict held-off run, `LAMP_NS_NET=run`. Both check the wrapper's
    /// mark.
    #[test]
    fn the_default_keeps_the_network_and_held_holds_it_off() {
        for value in [None, Some(""), Some("HELD"), Some("on"), Some("run")] {
            assert!(!held(value), "{value:?}");
        }
        assert!(held(Some("held")));
        let home = Path::new("/var/lib/lampos-tasks");
        let kept = lamp_runner(home, false);
        assert_eq!(
            kept.hermes.program,
            Path::new(HERMES),
            "through its namespace"
        );
        assert_eq!(kept.wrapper, Path::new(HERMES));
        assert_eq!(kept.ns, Path::new(NS));
        assert!(
            kept.hermes.env.iter().all(|(name, _)| name != NET_ENV),
            "{:?}",
            kept.hermes.env
        );
        assert!(
            kept.hermes
                .env
                .iter()
                .all(|(name, value)| !value.contains("admin.sock") && name != TOKEN_ENV)
        );
        let strict = lamp_runner(home, true);
        assert_eq!(strict.hermes.program, Path::new(HERMES));
        assert!(
            strict
                .hermes
                .env
                .contains(&(NET_ENV.to_owned(), NET_RUN.to_owned()))
        );
        assert_eq!(strict.hermes.held_by.as_deref(), Some(Path::new(HERMES)));
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

    /// Make folders an earlier lamp-tasks left are swept as the socket is
    /// started; nothing else in the work folder is.
    #[test]
    fn old_make_folders_are_swept_at_the_start() {
        let dir = tempfile::Builder::new()
            .prefix("tms-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(work.join("make-task-1/home/.hermes")).unwrap();
        std::fs::write(work.join("make-task-1/rules.md"), "R").unwrap();
        std::fs::create_dir(work.join("other")).unwrap();
        std::fs::write(work.join("make-notes.md"), "kept").unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = MakeHandOvers {
            socket: dir.path().join("skill-make.sock"),
            work: work.clone(),
            runner: Arc::new(writing(None)),
            caller: |_| true,
        }
        .start(true, stop.clone())
        .unwrap()
        .unwrap();
        assert!(!work.join("make-task-1").exists());
        assert!(work.join("other").exists() && work.join("make-notes.md").exists());
        stop.store(true, Ordering::Release);
        handle.join().unwrap();
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

    /// #6643 with #6685's one run path (`LAMP_SKILL_DEFAULT_AGENT` on in
    /// lamp-update): Hermes's wrapper is the one run path's (lamp-update's
    /// pinned bytes, with Hermes's make-home line in its data) and runs the
    /// one agent namespace, which carries Hermes's make lines: a make is
    /// ready, Hermes's own `hermes-ns` gone. Without either line, or with
    /// another agent's wrapper, it is refused [`NO_NAMESPACE`]; held, it
    /// is refused [`NOT_HELD_OFF`], as the one run path holds only a
    /// skill's run off the network. The lines are lamp-update's own.
    #[test]
    fn with_the_one_run_path_a_make_is_ready() {
        let dir = tempfile::tempdir().unwrap();
        let hermes = include_str!("../../update/src/agent_ns/wrapper/one-path-hermes.sh");
        let claude = include_str!("../../update/src/agent_ns/wrapper/one-path-claude.sh");
        assert!(hermes.contains(ONE_PATH_HOME_LINE));
        let with_make = format!("#!/bin/sh\nH=${{LAMP_NS_HOME-}} \n{ONE_PATH_NS_MAKE_LINES}");
        let runner = |held: bool, wrapper: &str, agent_ns: &str| {
            let (w, n) = (dir.path().join("hermes"), dir.path().join("agent-ns"));
            std::fs::write(&w, wrapper).unwrap();
            std::fs::write(&n, agent_ns).unwrap();
            let mut runner = lamp_runner(Path::new("/var/lib/lampos-tasks"), held);
            if held {
                runner.hermes.held_by = Some(w.clone());
            }
            runner.wrapper = w;
            runner.ns = dir.path().join("hermes-ns-gone");
            runner.agent_ns = n;
            runner
        };
        assert_eq!(runner(false, hermes, &with_make).ready(), Ok(()));
        let without = hermes.replacen(ONE_PATH_HOME_LINE, "", 1);
        for (wrapper, agent_ns) in [
            (without.as_str(), with_make.as_str()),
            (hermes, "#!/bin/sh\n"),
            (claude, with_make.as_str()),
        ] {
            assert_eq!(
                runner(false, wrapper, agent_ns).ready(),
                Err(NO_NAMESPACE.to_owned())
            );
        }
        assert_eq!(
            runner(true, hermes, &with_make).ready(),
            Err(NOT_HELD_OFF.to_owned())
        );
        assert_eq!(
            lamp_runner(Path::new("/var/lib/lampos-tasks"), false).agent_ns,
            Path::new(crate::skill_task::AGENT_NS)
        );
        let source = include_str!("../../update/src/hermes_install.rs");
        for (name, line) in [
            ("ONE_PATH_HOME_LINE", ONE_PATH_HOME_LINE),
            ("ONE_PATH_NS_MAKE_LINES", ONE_PATH_NS_MAKE_LINES),
        ] {
            assert!(
                source.contains(&format!("pub const {name}: &str = {line:?};")),
                "{name}"
            );
        }
    }
}
