//! The skill-start launcher (#7133, the reviewer's shape on #7135; the
//! script started outside the gateway, #7024): how a skill's run on a
//! gateway agent (OpenClaw) gets its script run with its token, without
//! either sitting in `WORK`, which every agent in the gateway can read and
//! write (they all share `lampos` and the gateway's mounts), and without
//! the script's process ever being seen from the gateway.
//!
//! - **The launcher** ([`LAUNCHER`]): a byte copy of lamp-tasks that
//!   lamp-update installs root-owned (0755), the one path OpenClaw's exec
//!   allow-list holds. Invoked as [`NAME`] (its file name, or `lamp-tasks
//!   skill-start`), it is [`launcher_main`]: `skill-start <run-id>`. It
//!   claims the run, then only relays what lamp-tasks sends it: it never
//!   gets the run's token and never starts the script.
//! - **The socket** ([`START_SOCKET`], 0600, in [`START_DIR`]): lamp-tasks'
//!   [`Server`]. The folder is made by systemd as the OpenClaw gateway
//!   unit's `RuntimeDirectory` (0755, `lampos`'s,
//!   `RuntimeDirectoryPreserve=yes`, so kept until reboot), installed by
//!   lamp-update's one-shot (#7135); OpenClaw's namespace binds it
//!   read-only, and each run's fence hides it ([`crate::gateway::HIDDEN`]).
//!   lamp-tasks never makes or removes it. The talk, in order:
//!   1. The launcher sends `<run-id>\n` with its credentials
//!      (`SCM_CREDENTIALS`; the socket has `SO_PASSCRED`). lamp-tasks reads
//!      it, then checks: the uid (`SO_PEERCRED`, its own), that the line came
//!      from the connecting pid itself (not a child that kept the socket),
//!      and that pid now ([`peer_may`]): its program is [`LAUNCHER`], that
//!      very file, root's and not group- or world-writable; no `LD_`
//!      variable; only root's code mapped; and nothing traces it
//!      (`TracerPid` 0). It refuses if any byte is queued after the id
//!      line already, then answers [`CHECKED`] with a fresh random nonce:
//!      `checked <16 lowercase hex>`.
//!   2. The launcher makes itself non-dumpable (`PR_SET_DUMPABLE` 0: no
//!      process of its uid can ptrace it from then on, or read its
//!      `/proc` files, which is why the checks of 1 come first) and sends
//!      `ready <nonce>` ([`READY`]), with its credentials again. lamp-tasks
//!      checks the nonce is exactly the one it gave (bytes queued before the
//!      launcher was exec'd cannot hold it), that the line came from the
//!      same pid, that the pid is non-dumpable (its `/proc`
//!      folder is root's) and still untraced ([`undumped_may`]).
//!   3. Only then is the run claimed ([`Starts::claim`]) and its [`Grant`]
//!      answered, one JSON line (the script, its folders, the socket and
//!      the SDK's path; never the token), or `{"error":"<words>"}`. Any
//!      refusal after a live run's id was read ends that run's start for
//!      good ([`Starts::refuse`]): it is never claimable again.
//!   4. **The relay** ([`Frame`]): lamp-tasks starts the run's fence
//!      itself and sends, on the same connection, the script's stdout and
//!      stderr in chunks and then its exit code (or an error's words); the
//!      launcher ([`relay`]) writes each chunk to its own stdout or stderr
//!      and exits with that code, so the agent sees the script's output as
//!      if the launcher had run it. The launcher sends nothing more: if its
//!      connection closes (or it sends a byte) before the end, lamp-tasks
//!      kills the fence and with it the script.
//! - **The fence**: lamp-tasks (`lampos`, in its own namespaces, outside
//!   the gateway's `unshare --map-root-user --mount --pid --mount-proc`)
//!   runs `unshare` with [`crate::gateway::fence_args`]: a new user,
//!   network, mount and PID namespace of the run's own, a sibling of the
//!   gateway's, never inside it. The token goes on fd 3 through a pipe
//!   from lamp-tasks' own memory (never an argument, the environment, a
//!   file or the launcher's socket); fd 4 is a pipe lamp-tasks holds. Just
//!   before the script is exec'd, the fence writes [`STARTED`] to fd 4 and
//!   closes it; lamp-tasks reads it there ([`Live::ran`]), and kills a
//!   fence that has not said it within [`STARTED_WITHIN`]. The fence is
//!   killed too when its run ends ([`Live`]'s drop).
//!
//! Why the script's token is out of the gateway's reach though the script
//! runs as the same kuid (`lampos`) as every agent there: `/proc/<pid>`
//! (its `environ`, `mem`) and `ptrace` both name a process by its pid, and
//! a process sees only the pids of its own PID namespace and those below
//! it. The script's pid is in a namespace that is a sibling of the
//! gateway's, so no process in the gateway has a pid for it at all: its
//! `/proc` holds no entry for it and `ptrace`, `kill` and `pidfd_open`
//! find nothing to name. Before #7024 the fence ran inside the gateway's
//! PID namespace, its script dumpable, so any agent there could read its
//! `environ` or attach to it and take `LAMP_CONTROL_TOKEN`.
//!
//! A sibling agent could race the launcher for a live run's id: through
//! the launcher it gets only that run's own approved script's output, run
//! once, and the real run then fails closed. No token is ever in a file,
//! and none ever reaches the gateway.
//!
//! Out of reach of these checks: code that already runs as `lampos`
//! outside the fence in the gateway's namespace, and holds a launcher's
//! socket fd (shared with it), could still race the claim on that fd, and
//! so read the script's output (never its token). The exact exec
//! allow-list and a static launcher keep such code out; the checks here
//! do not.
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// The launcher's one path: a root-owned byte copy of lamp-tasks, written
/// by lamp-update, the exact entry in OpenClaw's exec allow-list.
pub const LAUNCHER: &str = "/usr/local/lib/lampos/skill-start";
/// The name lamp-tasks acts as the launcher by: its file's name, or its
/// first argument.
pub const NAME: &str = "skill-start";
/// The launcher socket's folder: the OpenClaw gateway unit's
/// `RuntimeDirectory` (0755, `lampos`'s, kept until reboot), outside
/// lamp-tasks' own runtime folder, which systemd wipes at each stop.
pub const START_DIR: &str = "/run/lampos-skill-start";
/// The launcher socket, 0600, in [`START_DIR`].
pub const START_SOCKET: &str = "/run/lampos-skill-start/start.sock";
/// The line the fence writes to fd 4 just before the script is exec'd.
pub const STARTED: &str = "started";
/// lamp-tasks' line after the launcher's id and its image were checked,
/// sent as `checked <nonce>`: [`NONCE_HEX`] lowercase hex of fresh random
/// bytes, new for each connection.
pub const CHECKED: &str = "checked";
/// The launcher's line once it is non-dumpable, sent as `ready <nonce>`
/// with the nonce its [`CHECKED`] gave: only then is its grant sent.
pub const READY: &str = "ready";
/// The nonce's length in lowercase hex (8 random bytes).
pub const NONCE_HEX: usize = 16;
/// [`CHECKED`] and the space before its nonce.
const CHECKED_SPACE: &str = "checked ";
/// The refusal of bytes already queued after the run id, before [`CHECKED`].
const QUEUED: &str = "bytes were queued after the run id before it was checked";
/// How long the launcher's id, and lamp-tasks' answer, may take.
pub const LINE_WITHIN: Duration = Duration::from_secs(5);
/// How long after a claim the fence may take to say [`STARTED`]; a fence
/// that has not said it by then is killed.
pub const STARTED_WITHIN: Duration = Duration::from_secs(60);
/// How long one relay frame's write to the launcher may take: a launcher
/// that stops reading for longer has its run killed.
pub const RELAY_WRITE_WITHIN: Duration = Duration::from_secs(30);
/// How long the fence's pipes are waited for once it has exited.
const PIPES_AFTER_EXIT: Duration = Duration::from_secs(2);
/// How often the relay looks at its fence between frames.
const RELAY_TICK: Duration = Duration::from_millis(20);
/// The longest run id taken (lamp-control's own bound).
const MOST_ID_BYTES: usize = 64;
/// The longest answer the launcher reads.
const MOST_ANSWER_BYTES: usize = 64 * 1024;
/// The most connections served at once; one more is closed at once.
const MOST_CONNECTIONS: usize = 4;
/// How often the socket is looked at while no one connects.
const ACCEPT_TICK: Duration = Duration::from_millis(20);
/// How often [`START_SOCKET`] is bound again while its folder is not
/// there, and looked at while served (the Reviewer-lead's boot-order P2 on
/// #7133): a skill run waits at most this long after the folder is made.
pub const BIND_AGAIN: Duration = Duration::from_secs(2);

/// A run's start as its fence is made from it, and as its launcher is
/// told it was claimed, one JSON line: its script and interpreter, its
/// skill's folder, its run's folder, lamp-control's socket and the SDK's
/// path. Never the run's token (#7024): that stays in lamp-tasks
/// ([`Starts::open`]) and goes only to the fence's fd 3.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub script: PathBuf,
    pub interpreter: String,
    pub skill: PathBuf,
    pub run: PathBuf,
    pub socket: String,
    pub pythonpath: String,
}

/// A run id as taken on the socket: letters, digits and `-`, not empty,
/// at most [`MOST_ID_BYTES`].
fn plain_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MOST_ID_BYTES
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

struct Entry {
    grant: Grant,
    /// The run's token: only ever written to its fence's fd 3.
    token: String,
    fence: Fence,
    claimed: bool,
    started: bool,
    /// Why its start was refused, after which it is never claimed.
    refused: Option<String>,
}

/// The live gateway runs of this lamp-tasks, by id: each claimed at most
/// once, and gone at its run's end ([`Live`]'s drop).
#[derive(Clone, Default)]
pub struct Starts(Arc<Mutex<HashMap<String, Entry>>>);

impl Starts {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Run `id` live with `grant` and its `token`, until the [`Live`] is
    /// dropped.
    pub fn open(&self, id: &str, grant: Grant, token: &str) -> Result<Live, String> {
        if !plain_id(id) {
            return Err("the run's id is not plain, so nothing was started".into());
        }
        let mut live = self.lock();
        if live.contains_key(id) {
            return Err("a run with this id is already live, so nothing was started".into());
        }
        live.insert(
            id.to_owned(),
            Entry {
                grant,
                token: token.to_owned(),
                fence: Fence::default(),
                claimed: false,
                started: false,
                refused: None,
            },
        );
        Ok(Live {
            starts: self.clone(),
            id: id.to_owned(),
        })
    }

    /// Live run `id`, claimed now: refused for an id not live and for one
    /// claimed already.
    pub fn claim(&self, id: &str) -> Result<Claimed, String> {
        let mut live = self.lock();
        let entry = live
            .get_mut(id)
            .ok_or("no live skill run of this lamp has that id")?;
        if entry.refused.is_some() {
            return Err("that run's start was refused already".into());
        }
        if entry.claimed {
            return Err("that run's start was claimed already".into());
        }
        entry.claimed = true;
        Ok(Claimed {
            grant: entry.grant.clone(),
            token: entry.token.clone(),
            fence: entry.fence.clone(),
        })
    }

    /// Live run `id`'s start refused for `why`: never claimed after, and
    /// the run fails in these words ([`Live::ran`]). The first why is kept.
    pub fn refuse(&self, id: &str, why: &str) {
        if let Some(entry) = self.lock().get_mut(id)
            && entry.refused.is_none()
        {
            entry.refused = Some(why.to_owned());
        }
    }

    /// Why live run `id`'s start was refused, if it was.
    pub fn refused(&self, id: &str) -> Option<String> {
        self.lock().get(id).and_then(|entry| entry.refused.clone())
    }

    /// Run `id`'s fence said [`STARTED`]: only a claimed live run.
    fn started(&self, id: &str) {
        if let Some(entry) = self.lock().get_mut(id)
            && entry.claimed
        {
            entry.started = true;
        }
    }

    /// Whether run `id` is live now.
    pub fn is_live(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }

    /// Run `id`'s (claimed, started); both false when it is not live.
    fn state(&self, id: &str) -> (bool, bool) {
        self.lock()
            .get(id)
            .map_or((false, false), |entry| (entry.claimed, entry.started))
    }
}

/// A claimed run: its grant, its token (for its fence's fd 3 only) and
/// its fence's place, which the run's [`Live`] kills at the run's end.
pub struct Claimed {
    pub grant: Grant,
    token: String,
    fence: Fence,
}

/// A run's fence, once started: shared by its relay ([`Server::one`]) and
/// its run's entry, so the run's end kills it however the relay is doing.
#[derive(Clone, Default)]
struct Fence(Arc<Mutex<FenceState>>);

#[derive(Default)]
struct FenceState {
    child: Option<Child>,
    /// The run ended: no fence is started from now on.
    ended: bool,
}

impl Fence {
    fn lock(&self) -> MutexGuard<'_, FenceState> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The fence started by `spawn`, unless its run has ended already.
    fn start<T>(&self, spawn: impl FnOnce() -> io::Result<(Child, T)>) -> io::Result<T> {
        let mut state = self.lock();
        if state.ended {
            return Err(io::Error::other("the run ended before its script started"));
        }
        let (child, rest) = spawn()?;
        state.child = Some(child);
        Ok(rest)
    }

    /// The fence killed, if it still runs: its process group, then its
    /// `unshare` (`--kill-child`; its PID namespace's init dies with it,
    /// and every process in that namespace with that).
    fn kill(&self) {
        kill_child(&mut self.lock());
    }

    /// The run ended: the fence killed and none started after.
    fn end(&self) {
        let mut state = self.lock();
        state.ended = true;
        kill_child(&mut state);
    }

    /// How the fence ended, once it has.
    fn exited(&self) -> Option<ExitStatus> {
        self.lock().child.as_mut()?.try_wait().ok().flatten()
    }
}

fn kill_child(state: &mut FenceState) {
    let Some(child) = state.child.as_mut() else {
        return;
    };
    // Only one not reaped yet: its pid is still its own.
    if matches!(child.try_wait(), Ok(None)) {
        if let Some(pid) = i32::try_from(child.id())
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = child.kill();
    }
}

/// A live run's place in [`Starts`]; dropped (at the run's end, however it
/// ends), the run is not live, any later claim is refused, and its fence,
/// if started, is killed.
pub struct Live {
    starts: Starts,
    id: String,
}

impl Live {
    /// Whether the run's script started: not refused ([`Starts::refuse`]),
    /// claimed and [`STARTED`] read ([`crate::gateway::start_ran`]).
    pub fn ran(&self) -> Result<(), String> {
        if let Some(why) = self.starts.refused(&self.id) {
            return Err(format!("the skill's start was refused: {why}"));
        }
        let (claimed, started) = self.starts.state(&self.id);
        crate::gateway::start_ran(claimed, started)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let entry = self.starts.lock().remove(&self.id);
        if let Some(entry) = entry {
            entry.fence.end();
        }
    }
}

/// A connected peer as the kernel names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    pub pid: u32,
    pub uid: u32,
}

/// A file as `lstat` (or `stat`, through `/proc/<pid>/exe`) sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSeen {
    pub file: bool,
    pub uid: u32,
    pub mode: u32,
    pub dev: u64,
    pub ino: u64,
}

impl FileSeen {
    fn of(meta: &std::fs::Metadata) -> Self {
        Self {
            file: meta.file_type().is_file(),
            uid: meta.uid(),
            mode: meta.mode(),
            dev: meta.dev(),
            ino: meta.ino(),
        }
    }
}

/// How a peer is looked at: the kernel's on a lamp ([`Kernel`]), faked in
/// tests.
pub trait Peers: Send + Sync {
    /// The peer's pid and uid (`SO_PEERCRED`).
    fn peer(&self, stream: &UnixStream) -> Option<Peer>;
    /// `readlink /proc/<pid>/exe`.
    fn exe_link(&self, pid: u32) -> io::Result<PathBuf>;
    /// `stat /proc/<pid>/exe`: the program file itself, whatever its name
    /// in the peer's own mount namespace.
    fn exe_file(&self, pid: u32) -> io::Result<FileSeen>;
    /// `lstat` of `path`.
    fn file(&self, path: &Path) -> io::Result<FileSeen>;
    /// `/proc/<pid>/environ`.
    fn environ(&self, pid: u32) -> io::Result<Vec<u8>>;
    /// `/proc/<pid>/maps`.
    fn maps(&self, pid: u32) -> io::Result<String>;
    /// `/proc/<pid>/status` (readable even when the pid is non-dumpable).
    fn status(&self, pid: u32) -> io::Result<String>;
    /// The owner of `/proc/<pid>/status`: the root of the pid's user
    /// namespace outside it once the pid is non-dumpable, or 0 when that
    /// root is not mapped (as in OpenClaw's gateway, #7133); else its euid.
    /// Not the `/proc/<pid>` folder, which the kernel leaves at the euid
    /// whatever the dumpable flag.
    fn proc_owner(&self, pid: u32) -> io::Result<u32>;
    /// `None` when `pid` is in lamp-tasks' own user namespace; else the
    /// uid that namespace's root is outside it (`/proc/<pid>/uid_map`'s
    /// `0 <outer> …` line), as in OpenClaw's gateway (`unshare
    /// --map-root-user`, #7133). Not readable: taken as the same namespace.
    fn user_ns_root(&self, _pid: u32) -> io::Result<Option<u32>> {
        Ok(None)
    }
    /// One line of at most `most` bytes from `stream`, and the pid that
    /// sent it: `Some` only when every byte of it came with credentials
    /// (`SCM_CREDENTIALS`) naming that one pid.
    fn read_line(&self, stream: &UnixStream, most: usize) -> io::Result<(String, Option<u32>)>;
}

/// The kernel's view of a peer.
pub struct Kernel;

impl Peers for Kernel {
    fn peer(&self, stream: &UnixStream) -> Option<Peer> {
        lamp_ipc::caller::of_stream(stream).map(|caller| Peer {
            pid: caller.pid,
            uid: caller.uid,
        })
    }

    fn exe_link(&self, pid: u32) -> io::Result<PathBuf> {
        std::fs::read_link(format!("/proc/{pid}/exe"))
    }

    fn exe_file(&self, pid: u32) -> io::Result<FileSeen> {
        std::fs::metadata(format!("/proc/{pid}/exe")).map(|meta| FileSeen::of(&meta))
    }

    fn file(&self, path: &Path) -> io::Result<FileSeen> {
        std::fs::symlink_metadata(path).map(|meta| FileSeen::of(&meta))
    }

    fn environ(&self, pid: u32) -> io::Result<Vec<u8>> {
        std::fs::read(format!("/proc/{pid}/environ"))
    }

    fn maps(&self, pid: u32) -> io::Result<String> {
        std::fs::read_to_string(format!("/proc/{pid}/maps"))
    }

    fn status(&self, pid: u32) -> io::Result<String> {
        std::fs::read_to_string(format!("/proc/{pid}/status"))
    }

    fn proc_owner(&self, pid: u32) -> io::Result<u32> {
        std::fs::metadata(format!("/proc/{pid}/status")).map(|meta| meta.uid())
    }

    fn user_ns_root(&self, pid: u32) -> io::Result<Option<u32>> {
        let theirs = std::fs::read_link(format!("/proc/{pid}/ns/user"))?;
        if theirs == std::fs::read_link("/proc/self/ns/user")? {
            return Ok(None);
        }
        let map = std::fs::read_to_string(format!("/proc/{pid}/uid_map"))?;
        Ok(ns_root_outside(&map))
    }

    fn read_line(&self, stream: &UnixStream, most: usize) -> io::Result<(String, Option<u32>)> {
        read_line_sent(stream, most)
    }
}

/// [`Peers::read_line`] on a lamp: a byte at a time by `recvmsg`, each
/// byte's credentials taken (a stream never joins bytes of two senders in
/// one read).
#[cfg(target_os = "linux")]
fn read_line_sent(stream: &UnixStream, most: usize) -> io::Result<(String, Option<u32>)> {
    use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags};
    use std::mem::MaybeUninit;
    let mut line = Vec::new();
    let mut sender: Option<Option<u32>> = None;
    loop {
        let mut byte = [0u8; 1];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmCredentials(1))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let got = rustix::net::recvmsg(
            stream,
            &mut [io::IoSliceMut::new(&mut byte)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC,
        )?;
        if got.bytes == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let mut pid = None;
        for message in control.drain() {
            if let RecvAncillaryMessage::ScmCredentials(creds) = message {
                pid = u32::try_from(creds.pid.as_raw_pid()).ok();
            }
        }
        sender = match sender {
            None => Some(pid),
            Some(seen) if seen == pid => Some(seen),
            Some(_) => Some(None),
        };
        match byte[0] {
            b'\n' => break,
            _ if line.len() >= most => return Err(io::ErrorKind::InvalidData.into()),
            other => line.push(other),
        }
    }
    let line = String::from_utf8(line).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    Ok((line, sender.flatten()))
}

/// Off Linux there are no credentials on a message: no sender, so every
/// launcher is refused.
#[cfg(not(target_os = "linux"))]
fn read_line_sent(stream: &UnixStream, most: usize) -> io::Result<(String, Option<u32>)> {
    read_line(stream, most).map(|line| (line, None))
}

/// `stream`'s messages carry their sender's credentials (`SO_PASSCRED`).
#[cfg(target_os = "linux")]
fn pass_creds(stream: impl AsFd) -> io::Result<()> {
    Ok(rustix::net::sockopt::set_socket_passcred(stream, true)?)
}

#[cfg(not(target_os = "linux"))]
fn pass_creds(_: impl AsFd) -> io::Result<()> {
    Ok(())
}

/// One line sent with this process's own credentials (`SCM_CREDENTIALS`):
/// the kernel checks they are the sender's, so lamp-tasks knows which pid
/// sent it.
#[cfg(target_os = "linux")]
fn send_line(stream: &UnixStream, line: &str) -> io::Result<()> {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, UCred};
    use std::mem::MaybeUninit;
    let bytes = format!("{line}\n");
    let creds = UCred {
        pid: rustix::process::getpid(),
        uid: rustix::process::getuid(),
        gid: rustix::process::getgid(),
    };
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmCredentials(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmCredentials(creds)) {
        return Err(io::Error::other("no room for the launcher's credentials"));
    }
    let sent = rustix::net::sendmsg(
        stream,
        &[io::IoSlice::new(bytes.as_bytes())],
        &mut control,
        SendFlags::NOSIGNAL,
    )?;
    if sent != bytes.len() {
        return Err(io::Error::other("the launcher's line was cut short"));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn send_line(stream: &UnixStream, line: &str) -> io::Result<()> {
    write_line(stream, line)
}

/// This process made non-dumpable (`PR_SET_DUMPABLE` 0): no process of its
/// uid may ptrace it or read its memory from now on.
#[cfg(target_os = "linux")]
fn undumpable() -> io::Result<()> {
    Ok(rustix::process::set_dumpable_behavior(
        rustix::process::DumpableBehavior::NotDumpable,
    )?)
}

/// Off Linux the launcher never runs: refused.
#[cfg(not(target_os = "linux"))]
fn undumpable() -> io::Result<()> {
    Err(io::Error::other("only on Linux"))
}

/// The `TracerPid` in `/proc/<pid>/status`' text: 0 when nothing traces it.
fn tracer(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("TracerPid:"))
        .and_then(|value| value.trim().parse().ok())
}

/// Whether nothing traces `pid`.
fn untraced(peers: &dyn Peers, pid: u32) -> Result<(), String> {
    let status = peers
        .status(pid)
        .map_err(|_| "the caller's status can't be read")?;
    match tracer(&status) {
        Some(0) => Ok(()),
        Some(_) => Err("the caller is being traced".into()),
        None => Err("the caller's status can't be read".into()),
    }
}

/// After [`READY`]: `pid` is non-dumpable (its `/proc/<pid>/status` is
/// root's, so no process of lamp-tasks' uid can attach to it any more, and
/// it has not exec'd since, which would make it dumpable again) and still
/// untraced.
///
/// The kernel gives a non-dumpable task's `/proc` files to the root of the
/// task's user namespace, or to uid 0 when that root is not mapped
/// (`task_dump_owner`); the `/proc/<pid>` folder stays at its euid. OpenClaw
/// runs its agents in a user namespace whose root is not mapped (the
/// wrapper's `unshare --user --map-user` inside the gateway's `unshare
/// --map-root-user`, #7133, c0a4): there a non-dumpable launcher's status
/// is 0's and a dumpable one's `lampos`'s, so the check is real. In a
/// namespace whose root is mapped, the owner must be that root outside,
/// as for a dumpable task whose euid it is, and it tells no more.
pub fn undumped_may(peers: &dyn Peers, pid: u32) -> Result<(), String> {
    let owner = peers
        .proc_owner(pid)
        .map_err(|_| "the caller's status can't be read")?;
    let wanted = match peers.user_ns_root(pid) {
        Ok(Some(root)) => root,
        Ok(None) | Err(_) => 0,
    };
    if owner != wanted {
        return Err("the caller can still be traced (it is dumpable)".into());
    }
    untraced(peers, pid)
}

/// The uid outside a user namespace its root (uid 0 inside) is, by its
/// `uid_map` text (`<inside> <outside> <count>` lines); `None` when root
/// is not mapped.
pub fn ns_root_outside(map: &str) -> Option<u32> {
    map.lines().find_map(|line| {
        let mut fields = line.split_whitespace().map(str::parse::<u32>);
        match (fields.next(), fields.next(), fields.next()) {
            (Some(Ok(inside)), Some(Ok(outside)), Some(Ok(count))) if inside == 0 && count >= 1 => {
                Some(outside)
            }
            _ => None,
        }
    })
}

/// Whether an environment (`/proc/<pid>/environ`'s bytes, or the
/// launcher's own names) sets any `LD_` variable: the dynamic loader's
/// ways to run code before `main`.
pub fn sets_ld<'a>(names: impl IntoIterator<Item = &'a [u8]>) -> bool {
    names.into_iter().any(|entry| entry.starts_with(b"LD_"))
}

/// Folders an executable mapping is never taken from: writable by
/// lamp-tasks' user or by anyone.
const UNTRUSTED: [&str; 5] = ["/var", "/tmp", "/run", "/home", "/dev/shm"];

/// Whether every executable mapping in `maps` (`/proc/<pid>/maps`' text)
/// is the kernel's own (`[vdso]`, `[vsyscall]`) or a regular file of
/// root's that neither group nor others may write (`file`: `lstat`), the
/// same device and inode the map names, and outside [`UNTRUSTED`]. An anonymous one,
/// a deleted file, a memfd or anything else is refused in words.
pub fn maps_may(maps: &str, file: impl Fn(&Path) -> io::Result<FileSeen>) -> Result<(), String> {
    for line in maps.lines() {
        let mut rest = line;
        let mut fields = [""; 5];
        for field in &mut fields {
            rest = rest.trim_start();
            let end = rest.find(' ').unwrap_or(rest.len());
            *field = &rest[..end];
            rest = &rest[end..];
        }
        let [_, perms, _, device, inode] = fields;
        if perms.as_bytes().get(2) != Some(&b'x') {
            continue;
        }
        let path = rest.trim();
        if path.is_empty() {
            return Err("the caller has an anonymous executable mapping".into());
        }
        if matches!(path, "[vdso]" | "[vsyscall]") {
            continue;
        }
        if path.ends_with("(deleted)") || path.contains("memfd:") || !path.starts_with('/') {
            return Err(format!("the caller runs code from {path:?}"));
        }
        let at = Path::new(path);
        if UNTRUSTED.iter().any(|folder| at.starts_with(folder)) {
            return Err(format!("the caller runs code from {path:?}"));
        }
        let seen = file(at).map_err(|_| format!("the caller's code {path:?} can't be read"))?;
        if !seen.file
            || seen.uid != 0
            || seen.mode & 0o022 != 0
            || inode.parse::<u64>().ok() != Some(seen.ino)
            || map_device(device) != Some(dev_parts(seen.dev))
        {
            return Err(format!(
                "the caller's code {path:?} is not root's own, or others may change it"
            ));
        }
    }
    Ok(())
}

/// A maps line's device (`major:minor`, in hex).
fn map_device(device: &str) -> Option<(u64, u64)> {
    let (major, minor) = device.split_once(':')?;
    Some((
        u64::from_str_radix(major, 16).ok()?,
        u64::from_str_radix(minor, 16).ok()?,
    ))
}

/// `st_dev`'s major and minor, as Linux encodes them.
fn dev_parts(dev: u64) -> (u64, u64) {
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & 0xffff_f000);
    let minor = (dev & 0xff) | ((dev >> 12) & 0xffff_ff00);
    (major, minor)
}

/// The peer on `stream` as the kernel names it at connect
/// (`SO_PEERCRED`), when its uid is `own_uid`.
pub fn peer_of(peers: &dyn Peers, stream: &UnixStream, own_uid: u32) -> Result<Peer, String> {
    let peer = peers
        .peer(stream)
        .ok_or("the caller is not known to the kernel")?;
    if peer.uid != own_uid {
        return Err("the caller is not lamp-tasks' own user".into());
    }
    Ok(peer)
}

/// Whether process `pid` may claim a run, in order: its program's link
/// names `launcher`; `launcher` is a plain
/// file of root's that neither group nor others may write; the peer's
/// program is that very file (the same device and inode, so a file of the
/// same name bound over it in another mount namespace is not it); its
/// environment sets no `LD_` variable; and every executable mapping it has
/// is root's own code ([`maps_may`]), since a dynamically linked launcher
/// could have been given code to run before `main`; and nothing traces it.
pub fn peer_may(peers: &dyn Peers, pid: u32, launcher: &Path) -> Result<(), String> {
    let link = peers
        .exe_link(pid)
        .map_err(|_| "the caller's program can't be read")?;
    if link != launcher {
        return Err("the caller is not the skill-start launcher".into());
    }
    let file = peers
        .file(launcher)
        .map_err(|_| "the skill-start launcher is not there")?;
    if !file.file || file.uid != 0 || file.mode & 0o022 != 0 {
        return Err("the skill-start launcher is not root's own, or others may change it".into());
    }
    let exe = peers
        .exe_file(pid)
        .map_err(|_| "the caller's program can't be read")?;
    if (exe.dev, exe.ino) != (file.dev, file.ino) {
        return Err("the caller's program is not the skill-start launcher itself".into());
    }
    let environ = peers
        .environ(pid)
        .map_err(|_| "the caller's environment can't be read")?;
    if sets_ld(environ.split(|byte| *byte == 0)) {
        return Err("the caller has an LD_ variable set".into());
    }
    let maps = peers
        .maps(pid)
        .map_err(|_| "the caller's mappings can't be read")?;
    maps_may(&maps, |path| peers.file(path))?;
    untraced(peers, pid)
}

/// How a claimed run's fence is made: the command lamp-tasks starts, its
/// fds 3 and 4, stdin and stdout/stderr set by [`spawn_fence`].
pub type FenceCommand = Arc<dyn Fn(&Grant) -> Command + Send + Sync>;

/// The lamp's fence for `grant`: `unshare` with
/// [`crate::gateway::fence_args`] for lamp-tasks' own uid and gid and
/// [`crate::gateway::HIDDEN`], an environment of
/// [`crate::gateway::SCRIPT_PATH`] only, from `/`.
pub fn lamp_fence(grant: &Grant) -> Command {
    let hidden: Vec<&Path> = crate::gateway::HIDDEN.iter().map(Path::new).collect();
    let mut command = Command::new("unshare");
    command
        .args(crate::gateway::fence_args(
            grant,
            rustix::process::getuid().as_raw(),
            rustix::process::getgid().as_raw(),
            &hidden,
        ))
        .env_clear()
        .env("PATH", crate::gateway::SCRIPT_PATH)
        .current_dir("/");
    command
}

/// lamp-tasks' side of the launcher: [`START_SOCKET`] served.
pub struct Server {
    pub socket: PathBuf,
    pub starts: Starts,
    pub peers: Arc<dyn Peers>,
    pub own_uid: u32,
    /// [`LAUNCHER`] on a lamp; another path only in tests.
    pub launcher: PathBuf,
    pub started_within: Duration,
    /// [`lamp_fence`] on a lamp; a stand-in only in tests.
    pub fence: FenceCommand,
}

impl Server {
    /// The lamp's: [`START_SOCKET`], the kernel's view, [`LAUNCHER`],
    /// [`lamp_fence`].
    pub fn lamp(starts: Starts) -> Self {
        Self {
            socket: PathBuf::from(START_SOCKET),
            starts,
            peers: Arc::new(Kernel),
            own_uid: rustix::process::geteuid().as_raw(),
            launcher: PathBuf::from(LAUNCHER),
            started_within: STARTED_WITHIN,
            fence: Arc::new(lamp_fence),
        }
    }

    /// Served on a thread of its own until `stop`: the socket bound
    /// ([`bind`]) as soon as its folder is there, tried again every
    /// [`BIND_AGAIN`] until then (the Reviewer-lead's boot-order P2 on
    /// #7133: lamp-tasks may start before the gateway's unit makes the
    /// folder, at a boot or when the switch is turned on while it runs),
    /// and bound again when the socket is gone (the folder taken out and
    /// made again). One journal line a change, never one a try. Removed at
    /// `stop`.
    pub fn start(self, stop: Arc<AtomicBool>) -> io::Result<std::thread::JoinHandle<()>> {
        std::thread::Builder::new()
            .name("tasks-skill-start".into())
            .spawn(move || Arc::new(self).serve(&stop))
    }

    fn serve(self: Arc<Self>, stop: &AtomicBool) {
        let mut said: Option<String> = None;
        while !stop.load(Ordering::Acquire) {
            // Each bind passes credentials (`SO_PASSCRED`), so every id line
            // carries its sender's pid, uid and gid (Twelve's race on #7133).
            let bound = bind(&self.socket, self.own_uid)
                .and_then(|(listener, bound)| pass_creds(&listener).map(|()| (listener, bound)));
            match bound {
                Ok((listener, bound)) => {
                    eprintln!(
                        "tasks: a gateway agent's skill runs are claimed on {}",
                        self.socket.display()
                    );
                    said = None;
                    self.accept(&listener, &bound, stop);
                    if !stop.load(Ordering::Acquire) {
                        eprintln!(
                            "tasks: {} is gone; bound again once its folder is there",
                            self.socket.display()
                        );
                    }
                }
                Err(error) => {
                    let why = error.to_string();
                    if said.as_deref() != Some(why.as_str()) {
                        eprintln!(
                            "tasks: the skill-start launcher's socket is not served yet: {why}; tried again every {} s",
                            BIND_AGAIN.as_secs()
                        );
                        said = Some(why);
                    }
                    let until = std::time::Instant::now() + BIND_AGAIN;
                    while std::time::Instant::now() < until && !stop.load(Ordering::Acquire) {
                        std::thread::sleep(ACCEPT_TICK);
                    }
                }
            }
        }
    }

    /// Connections taken until `stop`, or until the socket bound is no
    /// longer at its path (looked at every [`BIND_AGAIN`]).
    fn accept(self: &Arc<Self>, listener: &UnixListener, bound: &Bound, stop: &AtomicBool) {
        let this = self.clone();
        let open = Arc::new(AtomicUsize::new(0));
        let mut looked = std::time::Instant::now();
        while !stop.load(Ordering::Acquire) {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if looked.elapsed() >= BIND_AGAIN {
                        looked = std::time::Instant::now();
                        if !bound.there() {
                            return;
                        }
                    }
                    std::thread::sleep(ACCEPT_TICK);
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    eprintln!("[lamp-tasks] skill start: accept: {error}");
                    std::thread::sleep(ACCEPT_TICK);
                    continue;
                }
            };
            if open.fetch_add(1, Ordering::AcqRel) >= MOST_CONNECTIONS {
                open.fetch_sub(1, Ordering::AcqRel);
                continue;
            }
            let (served, open) = (this.clone(), open.clone());
            let held = Counted(open);
            let _ = std::thread::Builder::new()
                .name("tasks-skill-start-one".into())
                .spawn(move || {
                    let _held = held;
                    served.one(stream);
                });
        }
    }

    /// One launcher's connection, in the module's order: its id read and
    /// its sender and image checked, [`CHECKED`]; its [`READY`] read and it
    /// checked non-dumpable and untraced; only then the run claimed and its
    /// grant answered, one JSON line (never the token); then the run's
    /// fence started here and relayed on the same connection
    /// ([`Server::relay_run`]). A refused peer gets no answer at all; after
    /// a live run's id was read, any refusal ends that run's start
    /// ([`Starts::refuse`]).
    pub fn one(&self, stream: UnixStream) {
        if stream.set_nonblocking(false).is_err()
            || stream.set_read_timeout(Some(LINE_WITHIN)).is_err()
            || stream.set_write_timeout(Some(LINE_WITHIN)).is_err()
            || pass_creds(&stream).is_err()
        {
            return;
        }
        let peers = &*self.peers;
        let peer = match peer_of(peers, &stream, self.own_uid) {
            Ok(peer) => peer,
            Err(why) => {
                eprintln!("[lamp-tasks] skill start: refused a caller: {why}");
                return;
            }
        };
        let answer = |line: &str| {
            let _ = write_line(&stream, line);
        };
        let Ok((id, sender)) = peers.read_line(&stream, MOST_ID_BYTES) else {
            return answer(&error_line("no plain run id was sent"));
        };
        // From here a live run's id is known: any refusal ends its start.
        let silent = |why: &str| {
            eprintln!("[lamp-tasks] skill start: refused a caller: {why}");
            if plain_id(&id) {
                self.starts.refuse(&id, why);
            }
        };
        let checked = (sender == Some(peer.pid))
            .then_some(())
            .ok_or_else(|| "the run id was not sent by the caller itself".to_owned())
            .and_then(|()| peer_may(peers, peer.pid, &self.launcher));
        if let Err(why) = checked {
            return silent(&why);
        }
        if !plain_id(&id) {
            return answer(&error_line("no plain run id was sent"));
        }
        if !self.starts.is_live(&id) {
            return answer(&error_line("no live skill run of this lamp has that id"));
        }
        if let Some(why) = self.starts.refused(&id) {
            eprintln!("[lamp-tasks] skill start: run {id} was refused already: {why}");
            return answer(&error_line("that run's start was refused already"));
        }
        // Nothing may be queued after the id yet: a ready sent before
        // CHECKED was not the checked launcher's.
        if !matches!(queued(&stream), Ok(false)) {
            return silent(QUEUED);
        }
        let Ok(nonce) = nonce() else {
            return silent("no nonce could be made");
        };
        answer(&format!("{CHECKED_SPACE}{nonce}"));
        let want = format!("{READY} {nonce}");
        let ready = match peers.read_line(&stream, want.len()) {
            Ok((line, sender)) if line == want && sender == Some(peer.pid) => {
                undumped_may(peers, peer.pid)
            }
            Ok((line, _)) if line == want => {
                Err("the launcher's ready was not sent by the caller itself".into())
            }
            _ => Err("the launcher never said it was ready".into()),
        };
        if let Err(why) = ready {
            return silent(&why);
        }
        let claimed = match self.starts.claim(&id) {
            Ok(claimed) => claimed,
            Err(why) => {
                self.starts.refuse(&id, &why);
                eprintln!("[lamp-tasks] skill start: refused: {why}");
                return answer(&error_line(&why));
            }
        };
        let Ok(line) = serde_json::to_string(&claimed.grant) else {
            let why = "the run's start could not be written";
            self.starts.refuse(&id, why);
            return answer(&error_line(why));
        };
        eprintln!(
            "[lamp-tasks] skill start: run {id} claimed by the launcher (pid {})",
            peer.pid
        );
        if write_line(&stream, &line).is_err() {
            return;
        }
        drop(line);
        self.relay_run(&id, &stream, claimed);
    }

    /// Claimed run `id`'s fence started here, outside the gateway
    /// ([`spawn_fence`] with [`Server::fence`]), and relayed to its
    /// launcher on `stream`: the script's stdout and stderr as they come,
    /// then its exit code ([`Frame`]). [`STARTED`] on the fence's fd 4
    /// marks the run started ([`Live::ran`]); a fence that has not said it
    /// within [`Server::started_within`] is killed. The launcher's
    /// connection closed, a byte from it, or a frame not written within
    /// [`RELAY_WRITE_WITHIN`], and the fence is killed: its script never
    /// runs on with no one to relay to.
    fn relay_run(&self, id: &str, stream: &UnixStream, claimed: Claimed) {
        let Claimed {
            grant,
            token,
            fence,
        } = claimed;
        let fail = |why: &str| {
            eprintln!("[lamp-tasks] skill start: run {id}'s fence was not started: {why}");
            let _ = write_frame(stream, &Frame::Error(why.to_owned()));
        };
        if stream.set_write_timeout(Some(RELAY_WRITE_WITHIN)).is_err()
            || stream.set_read_timeout(None).is_err()
        {
            return fail("the launcher's connection could not be kept");
        }
        let Ok(watched) = stream.try_clone() else {
            return fail("the launcher's connection could not be watched");
        };
        let spawned = fence.start(|| {
            let fenced = spawn_fence((self.fence)(&grant), &token)?;
            Ok((fenced.child, (fenced.out, fenced.err, fenced.said)))
        });
        drop(token);
        let (out, err, said) = match spawned {
            Ok(pipes) => pipes,
            Err(error) => return fail(&error.to_string()),
        };
        eprintln!("[lamp-tasks] skill start: run {id}'s fence started outside the gateway");
        let (events, seen) = mpsc::channel();
        pump(out, events.clone(), Event::Out);
        pump(err, events.clone(), Event::Err);
        let saying = events.clone();
        let _ = std::thread::Builder::new()
            .name("tasks-skill-start-said".into())
            .spawn(move || {
                let line = read_line(said, STARTED.len());
                let _ = saying.send(Event::Said(line.is_ok_and(|line| line == STARTED)));
            });
        let _ = std::thread::Builder::new()
            .name("tasks-skill-start-watch".into())
            .spawn(move || {
                let mut byte = [0u8; 1];
                let _ = (&watched).read(&mut byte);
                let _ = events.send(Event::Gone);
            });
        let begun = Instant::now();
        let (mut shut, mut said, mut gone) = (0, None::<bool>, false);
        let mut why: Option<&str> = None;
        let mut exited_at: Option<Instant> = None;
        let status = loop {
            let mut frame = None;
            match seen.recv_timeout(RELAY_TICK) {
                Ok(Event::Out(bytes)) => frame = Some(Frame::Stdout(bytes)),
                Ok(Event::Err(bytes)) => frame = Some(Frame::Stderr(bytes)),
                Ok(Event::Shut) => shut += 1,
                Ok(Event::Said(true)) => {
                    said = Some(true);
                    self.starts.started(id);
                    eprintln!("[lamp-tasks] skill start: run {id}'s script started");
                }
                Ok(Event::Said(false)) => {
                    said = Some(false);
                    // Not the fence's word: whatever runs there is not ours.
                    fence.kill();
                }
                Ok(Event::Gone) => {
                    if !gone {
                        gone = true;
                        eprintln!(
                            "[lamp-tasks] skill start: run {id}'s launcher left before its end: its fence killed"
                        );
                    }
                    fence.kill();
                }
                Err(_) => {}
            }
            if let Some(frame) = frame
                && !gone
                && write_frame(stream, &frame).is_err()
            {
                gone = true;
                eprintln!(
                    "[lamp-tasks] skill start: run {id}'s launcher took no more: its fence killed"
                );
                fence.kill();
            }
            if said.is_none() && why.is_none() && begun.elapsed() >= self.started_within {
                why = Some("the run's fence never said it started");
                eprintln!("[lamp-tasks] skill start: run {id}'s fence never said it started");
                fence.kill();
            }
            if let Some(status) = fence.exited() {
                let at = *exited_at.get_or_insert_with(Instant::now);
                if (shut >= 2 && said.is_some()) || at.elapsed() >= PIPES_AFTER_EXIT {
                    break status;
                }
            }
        };
        if !gone {
            let last = match why {
                Some(why) => Frame::Error(why.to_owned()),
                None => Frame::Exit(exit_code(status)),
            };
            let _ = write_frame(stream, &last);
        }
        // Wakes the watching thread.
        let _ = stream.shutdown(std::net::Shutdown::Both);
        eprintln!(
            "[lamp-tasks] skill start: run {id}'s fence ended ({})",
            exit_code(status)
        );
    }
}

/// What the relay's threads say to it.
enum Event {
    /// A chunk of the script's stdout.
    Out(Vec<u8>),
    /// A chunk of the script's stderr.
    Err(Vec<u8>),
    /// One of the two pipes ended.
    Shut,
    /// fd 4's line: whether it was [`STARTED`].
    Said(bool),
    /// The launcher's connection closed, or sent a byte.
    Gone,
}

/// `from` read in chunks of at most [`MOST_FRAME_BYTES`] on a thread of its
/// own, each sent as `kind`, then [`Event::Shut`].
fn pump(mut from: impl Read + Send + 'static, to: mpsc::Sender<Event>, kind: fn(Vec<u8>) -> Event) {
    let _ = std::thread::Builder::new()
        .name("tasks-skill-start-pipe".into())
        .spawn(move || {
            let mut chunk = vec![0u8; MOST_FRAME_BYTES];
            loop {
                match from.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(got) => {
                        if to.send(kind(chunk[..got].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            let _ = to.send(Event::Shut);
        });
}

/// A process's exit as a shell says it: its code, or 128 and its signal.
fn exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1)
}

/// A fence started: its process, its stdout and stderr, and fd 4's
/// reading end.
pub struct Spawned {
    pub child: Child,
    pub out: std::process::ChildStdout,
    pub err: std::process::ChildStderr,
    pub said: std::fs::File,
}

/// `command` started as a run's fence: stdin `/dev/null`, stdout and stderr
/// piped, a process group of its own, `token` (and a newline) in a pipe on
/// its fd 3 and the writing end of a pipe lamp-tasks reads on its fd 4.
/// Every other fd of lamp-tasks is close-on-exec, as Rust opens them, and
/// both pipes are made close-on-exec too: only this child gets them, at 3
/// and 4, never another child started meanwhile.
#[allow(unsafe_code)]
pub fn spawn_fence(mut command: Command, token: &str) -> io::Result<Spawned> {
    use std::os::unix::process::CommandExt;
    // std's pipes are close-on-exec (`pipe2(O_CLOEXEC)` on Linux).
    let (token_reader, mut token_writer) = io::pipe()?;
    let (said_reader, said_writer) = io::pipe()?;
    // A token is far smaller than a pipe's buffer: never blocks.
    token_writer.write_all(format!("{token}\n").as_bytes())?;
    drop(token_writer);
    let (at_3, at_4) = (token_reader.as_raw_fd(), said_writer.as_raw_fd());
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // SAFETY: the closure runs in the child between fork and exec, and
    // calls only fcntl, dup2 and close, which are async-signal-safe; it
    // allocates nothing and takes no lock.
    unsafe {
        command.pre_exec(move || place_at_3_and_4(at_3, at_4));
    }
    let mut child = command.spawn()?;
    drop((token_reader, said_writer));
    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::other("the fence's output was not piped"));
    };
    Ok(Spawned {
        child,
        out,
        err,
        said: std::fs::File::from(OwnedFd::from(said_reader)),
    })
}

/// In a fence's child, before its exec: raw fds `token` and `said` (both
/// close-on-exec, still open) duplicated at exactly 3 and 4, which stay
/// open across the exec. Each is first copied above 9, so neither is
/// overwritten by the other's `dup2` whatever their numbers were.
#[allow(unsafe_code)]
fn place_at_3_and_4(token: i32, said: i32) -> io::Result<()> {
    use std::mem::ManuallyDrop;
    use std::os::fd::{BorrowedFd, FromRawFd};
    // SAFETY: both fds were opened by the parent before the fork and are
    // open in this child; they are only borrowed for these calls.
    let (token, said) = unsafe { (BorrowedFd::borrow_raw(token), BorrowedFd::borrow_raw(said)) };
    let token = rustix::io::fcntl_dupfd_cloexec(token, 10)?;
    let said = rustix::io::fcntl_dupfd_cloexec(said, 10)?;
    for (from, at) in [(&token, 3), (&said, 4)] {
        // SAFETY: `at` is only dup2's target, which it closes first if open;
        // it is never closed here (`ManuallyDrop`), so it stays the child's.
        let mut target = ManuallyDrop::new(unsafe { OwnedFd::from_raw_fd(at) });
        // dup2 to another number clears close-on-exec on the copy.
        rustix::io::dup2(from, &mut target)?;
    }
    Ok(())
}

/// Whether any byte is queued on `stream` already (peeked, not waited
/// for).
fn queued(stream: &UnixStream) -> io::Result<bool> {
    use rustix::net::RecvFlags;
    let mut byte = [0u8; 1];
    match rustix::net::recv(stream, &mut byte[..], RecvFlags::PEEK | RecvFlags::DONTWAIT) {
        Ok((got, _)) => Ok(got > 0),
        Err(rustix::io::Errno::AGAIN) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// A fresh nonce: [`NONCE_HEX`] lowercase hex of random bytes.
fn nonce() -> io::Result<String> {
    let mut bytes = [0u8; NONCE_HEX / 2];
    getrandom::fill(&mut bytes).map_err(|_| io::Error::other("no random bytes"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// An answer of refusal: `{"error":"<why>"}`.
fn error_line(why: &str) -> String {
    eprintln!("[lamp-tasks] skill start: refused: {why}");
    serde_json::json!({ "error": why }).to_string()
}

struct Counted(Arc<AtomicUsize>);
impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One line of at most `most` bytes, its `\n` taken off; read a byte at a
/// time so nothing after it is taken.
fn read_line(mut stream: impl Read, most: usize) -> io::Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte)? {
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            _ if byte[0] == b'\n' => break,
            _ if line.len() >= most => return Err(io::ErrorKind::InvalidData.into()),
            _ => line.push(byte[0]),
        }
    }
    String::from_utf8(line).map_err(|_| io::ErrorKind::InvalidData.into())
}

fn write_line(mut stream: &UnixStream, line: &str) -> io::Result<()> {
    stream.write_all(format!("{line}\n").as_bytes())
}

/// The bound socket, removed at the drop if it is still the one bound.
pub struct Bound {
    path: PathBuf,
    dev: u64,
    ino: u64,
}

impl Bound {
    /// Whether the socket bound is still at its path: the same socket,
    /// not gone with its folder or replaced.
    fn there(&self) -> bool {
        std::fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.file_type().is_socket() && m.dev() == self.dev && m.ino() == self.ino)
    }
}

impl Drop for Bound {
    fn drop(&mut self) {
        if self.there() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// `socket` bound 0600 in its folder, which lamp-update makes: taken only
/// when it is a real folder of `own_uid`'s that neither group nor others
/// may write, never made, changed or removed here (missing, nothing is
/// served). A socket of `own_uid`'s left there by an earlier lamp-tasks is
/// removed first; anything else there is refused.
pub fn bind(socket: &Path, own_uid: u32) -> io::Result<(UnixListener, Bound)> {
    let refused = |why: &str| io::Error::new(io::ErrorKind::InvalidInput, why.to_owned());
    let dir = socket
        .parent()
        .ok_or_else(|| refused("the launcher socket has no folder"))?;
    match std::fs::symlink_metadata(dir) {
        Ok(meta) => {
            if !meta.is_dir() || meta.uid() != own_uid || meta.mode() & 0o022 != 0 {
                return Err(refused(
                    "the launcher socket's folder is not lamp-tasks' own, or others may write it",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{} is not there (lamp-update makes it), so the launcher is not served",
                    dir.display()
                ),
            ));
        }
        Err(error) => return Err(error),
    }
    match std::fs::symlink_metadata(socket) {
        Ok(meta) if meta.file_type().is_socket() && meta.uid() == own_uid => {
            std::fs::remove_file(socket)?;
        }
        Ok(_) => {
            return Err(refused(
                "the launcher socket's path is not a socket of ours",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(socket)?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let meta = std::fs::symlink_metadata(socket)?;
    Ok((
        listener,
        Bound {
            path: socket.to_owned(),
            dev: meta.dev(),
            ino: meta.ino(),
        },
    ))
}

/// Whether this process's own program is the launcher ([`LAUNCHER`]), by
/// `/proc/self/exe` resolved: then it is the launcher whatever its
/// arguments, never lamp-tasks' own commands.
///
/// On Linux it fails closed: a program whose own file can't be read is
/// taken as the launcher (which then refuses or asks for one run only), so
/// the launcher's copy never falls back to lamp-tasks' commands.
pub fn exe_is_launcher() -> bool {
    is_launcher_exe(std::fs::canonicalize("/proc/self/exe").ok().as_deref())
}

/// [`exe_is_launcher`] for the program's own file as read (`None`: not
/// read).
pub fn is_launcher_exe(exe: Option<&Path>) -> bool {
    match exe {
        Some(exe) => exe == Path::new(LAUNCHER),
        None => cfg!(target_os = "linux"),
    }
}

/// The launcher's arguments when this process is the launcher: always
/// when its program is [`LAUNCHER`] (`exe_is_launcher`), whatever `argv[1]`
/// says; else as [`invoked_as_launcher`] finds.
pub fn launcher_args(exe_is_launcher: bool, argv: &[OsString]) -> Option<Vec<OsString>> {
    if exe_is_launcher {
        return Some(argv.get(1..).unwrap_or_default().to_vec());
    }
    invoked_as_launcher(argv)
}

/// The launcher's own checks, before it connects anywhere: exactly one
/// argument, a run id as lamp-tasks makes them
/// ([`crate::skill_task::made_run_id`]), and no `LD_` variable among the
/// names of its environment (`env`). The run id, or why not.
pub fn launcher_refusal<'a>(
    args: &[OsString],
    env: impl IntoIterator<Item = &'a [u8]>,
) -> Result<String, String> {
    let [id] = args else {
        return Err("usage: skill-start <run-id>".into());
    };
    let Some(id) = id
        .to_str()
        .filter(|id| id.len() <= MOST_ID_BYTES && crate::skill_task::made_run_id(id))
    else {
        return Err("the run id is not one lamp-tasks makes".into());
    };
    if sets_ld(env) {
        return Err("refused: an LD_ variable is set".into());
    }
    Ok(id.to_owned())
}

/// Whether this process was invoked as the launcher: its file named
/// [`NAME`], or [`NAME`] its first argument. The launcher's own arguments
/// when it was.
pub fn invoked_as_launcher(argv: &[OsString]) -> Option<Vec<OsString>> {
    let zero = argv.first()?;
    if Path::new(zero).file_name() == Some(std::ffi::OsStr::new(NAME)) {
        return Some(argv[1..].to_vec());
    }
    (argv.get(1).map(OsString::as_os_str) == Some(std::ffi::OsStr::new(NAME)))
        .then(|| argv[2..].to_vec())
}

/// lamp-tasks' answer read: the grant, or its error's words.
pub fn parse_answer(line: &str) -> Result<Grant, String> {
    if let Ok(serde_json::Value::Object(object)) = serde_json::from_str(line)
        && let Some(serde_json::Value::String(why)) = object.get("error")
    {
        return Err(why.clone());
    }
    serde_json::from_str::<Grant>(line)
        .map_err(|_| "lamp-tasks' answer was not a run's start".into())
}

/// The launcher: `skill-start <run-id>`. Refused (exit 2) before it
/// connects anywhere by [`launcher_refusal`]; else claims the run over
/// [`START_SOCKET`] ([`ask`]; exit 2 when it can't be made non-dumpable),
/// then relays the script lamp-tasks runs outside the gateway ([`relay`]).
/// Its exit status, and why when it is not the script's own (empty words
/// otherwise).
pub fn launcher_main(args: &[OsString]) -> (i32, String) {
    use std::os::unix::ffi::OsStrExt;
    let names: Vec<OsString> = std::env::vars_os().map(|(name, _)| name).collect();
    let id = match launcher_refusal(args, names.iter().map(|name| name.as_bytes())) {
        Ok(id) => id,
        Err(why) => return (2, why),
    };
    launch(&id)
}

/// The launcher's side of the talk on `stream` (the module's order): `id`
/// sent with its credentials; on [`CHECKED`] and its nonce, made
/// non-dumpable (`undumpable`), and only then [`READY`] with that nonce
/// sent and the grant (no token) read. Never
/// ready unless non-dumpable: if that fails, nothing more is sent (exit 2).
pub fn ask(
    stream: &UnixStream,
    id: &str,
    undumpable: impl FnOnce() -> io::Result<()>,
) -> Result<Grant, (i32, String)> {
    let refused = |why: String| (1, format!("the run's start was refused: {why}"));
    send_line(stream, id).map_err(|_| (1, "lamp-tasks was not asked".to_owned()))?;
    let nonce = match read_line(stream, MOST_ANSWER_BYTES) {
        Ok(line) => match line.strip_prefix(CHECKED_SPACE) {
            Some(nonce) if !nonce.is_empty() => nonce.to_owned(),
            _ => return Err(refused(parse_answer(&line).err().unwrap_or_default())),
        },
        Err(_) => return Err(refused("lamp-tasks did not answer".into())),
    };
    undumpable().map_err(|error| {
        (
            2,
            format!("refused: the launcher could not be made non-dumpable: {error}"),
        )
    })?;
    send_line(stream, &format!("{READY} {nonce}"))
        .map_err(|_| (1, "lamp-tasks was not told ready".to_owned()))?;
    read_line(stream, MOST_ANSWER_BYTES)
        .map_err(|_| "lamp-tasks did not answer".to_owned())
        .and_then(|line| parse_answer(&line))
        .map_err(refused)
}

/// The launcher past its own checks: lamp-tasks asked for run `id` on
/// [`START_SOCKET`].
fn launch(id: &str) -> (i32, String) {
    launch_at(Path::new(START_SOCKET), id)
}

/// The launcher's talk for run `id` on `socket` ([`START_SOCKET`] on a
/// lamp): the run claimed ([`ask`]), then what lamp-tasks relays written
/// to this process's stdout and stderr ([`relay`]) until the script's exit
/// code, which it returns. The launcher never starts anything itself.
pub fn launch_at(socket: &Path, id: &str) -> (i32, String) {
    let stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(error) => {
            return (
                1,
                format!(
                    "lamp-tasks can't be reached on {}: {error}",
                    socket.display()
                ),
            );
        }
    };
    if stream.set_read_timeout(Some(LINE_WITHIN)).is_err()
        || stream.set_write_timeout(Some(LINE_WITHIN)).is_err()
    {
        return (1, "lamp-tasks was not asked".into());
    }
    if let Err(refused) = ask(&stream, id, undumpable) {
        return refused;
    }
    // From here lamp-tasks paces the run (its fence's start, the run's own
    // deadline): no read is cut short.
    if stream.set_read_timeout(None).is_err() {
        return (1, "the launcher socket could not be kept".into());
    }
    relay(&stream, io::stdout().lock(), io::stderr().lock())
}

/// The most bytes one relay frame carries.
pub const MOST_FRAME_BYTES: usize = 16 * 1024;
/// A frame's kind byte: a chunk of the script's stdout.
const KIND_STDOUT: u8 = 1;
/// A chunk of the script's stderr.
const KIND_STDERR: u8 = 2;
/// The script's exit code, 4 bytes (big-endian `i32`); the last frame.
const KIND_EXIT: u8 = 3;
/// Why the run did not end with an exit code, UTF-8 words; the last frame.
const KIND_ERROR: u8 = 4;

/// One frame lamp-tasks relays to the launcher after the claim: one kind
/// byte, the payload's length as a big-endian `u32` (at most
/// [`MOST_FRAME_BYTES`]), then the payload. [`Frame::Exit`] or
/// [`Frame::Error`] is the last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Exit(i32),
    Error(String),
}

impl Frame {
    /// The frame's bytes; a payload over [`MOST_FRAME_BYTES`] is refused
    /// (an error's words are cut to fit, at a character's edge).
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let (kind, payload): (u8, std::borrow::Cow<'_, [u8]>) = match self {
            Self::Stdout(bytes) => (KIND_STDOUT, bytes.as_slice().into()),
            Self::Stderr(bytes) => (KIND_STDERR, bytes.as_slice().into()),
            Self::Exit(code) => (KIND_EXIT, code.to_be_bytes().to_vec().into()),
            Self::Error(words) => {
                let mut end = words.len().min(MOST_FRAME_BYTES);
                while !words.is_char_boundary(end) {
                    end -= 1;
                }
                (KIND_ERROR, words.as_bytes()[..end].into())
            }
        };
        if payload.len() > MOST_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a relay frame over its bound",
            ));
        }
        let len = u32::try_from(payload.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
        let mut bytes = Vec::with_capacity(5 + payload.len());
        bytes.push(kind);
        bytes.extend_from_slice(&len.to_be_bytes());
        bytes.extend_from_slice(&payload);
        Ok(bytes)
    }

    /// The next frame on `from`: `None` at its end between two frames; an
    /// unknown kind, a length over [`MOST_FRAME_BYTES`], an exit code not 4
    /// bytes, words not UTF-8 or a frame cut short is an error.
    pub fn read(mut from: impl Read) -> io::Result<Option<Self>> {
        let invalid = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_owned());
        let mut head = [0u8; 5];
        let mut got = 0;
        while got < head.len() {
            match from.read(&mut head[got..]) {
                Ok(0) if got == 0 => return Ok(None),
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(more) => got += more,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        let [kind, a, b, c, d] = head;
        let len = usize::try_from(u32::from_be_bytes([a, b, c, d]))
            .map_err(|_| invalid("a relay frame over its bound"))?;
        if len > MOST_FRAME_BYTES {
            return Err(invalid("a relay frame over its bound"));
        }
        if !matches!(kind, KIND_STDOUT | KIND_STDERR | KIND_EXIT | KIND_ERROR) {
            return Err(invalid("a relay frame of no known kind"));
        }
        if kind == KIND_EXIT && len != 4 {
            return Err(invalid("an exit frame not of 4 bytes"));
        }
        let mut payload = vec![0u8; len];
        from.read_exact(&mut payload)?;
        Ok(Some(match kind {
            KIND_STDOUT => Self::Stdout(payload),
            KIND_STDERR => Self::Stderr(payload),
            KIND_EXIT => Self::Exit(i32::from_be_bytes(
                payload
                    .try_into()
                    .map_err(|_| invalid("an exit frame not of 4 bytes"))?,
            )),
            _ => Self::Error(
                String::from_utf8(payload).map_err(|_| invalid("an error frame not UTF-8"))?,
            ),
        }))
    }
}

/// `frame` written to `to` whole.
fn write_frame(mut to: impl Write, frame: &Frame) -> io::Result<()> {
    to.write_all(&frame.encode()?)
}

/// The launcher's relay: frames read from `from` until the last, each
/// chunk written (and flushed) to `out` or `err`. The exit status and why:
/// [`Frame::Exit`]'s code (outside 0-255, 1) and no words; else 1 and why
/// not, for an error frame, an end before the last frame, a frame not
/// understood, or output that could not be written (the launcher then
/// leaves, and lamp-tasks kills the run).
pub fn relay(mut from: impl Read, mut out: impl Write, mut err: impl Write) -> (i32, String) {
    loop {
        let (to, bytes): (&mut dyn Write, Vec<u8>) = match Frame::read(&mut from) {
            Ok(Some(Frame::Stdout(bytes))) => (&mut out, bytes),
            Ok(Some(Frame::Stderr(bytes))) => (&mut err, bytes),
            Ok(Some(Frame::Exit(code))) => {
                return (
                    if (0..=255).contains(&code) { code } else { 1 },
                    String::new(),
                );
            }
            Ok(Some(Frame::Error(words))) => {
                return (
                    1,
                    format!("the skill's script did not run to its end: {words}"),
                );
            }
            Ok(None) => return (1, "lamp-tasks ended the run before its end".into()),
            Err(_) => return (1, "lamp-tasks' relay was not understood".into()),
        };
        if to.write_all(&bytes).and_then(|()| to.flush()).is_err() {
            return (1, "the script's output could not be written".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant() -> Grant {
        Grant {
            script: "/var/lib/lampOS/skills/photo-booth/scripts/main.py".into(),
            interpreter: "python3".into(),
            skill: "/var/lib/lampOS/skills/photo-booth".into(),
            run: "/var/lib/lampos-tasks/harnesses/lamp-tasks/skill-task-1".into(),
            socket: "/run/lampos-control/control.sock".into(),
            pythonpath: "/opt/lampOS/sdk".into(),
        }
    }

    /// The run's token in these tests.
    const TOKEN: &str = "secret-7133";

    #[test]
    fn the_launchers_own_file_unread_is_taken_as_the_launcher_on_linux() {
        assert!(is_launcher_exe(Some(Path::new(LAUNCHER))));
        assert!(!is_launcher_exe(Some(Path::new(
            "/opt/lampOS/tasks/current/lamp-tasks"
        ))));
        assert_eq!(is_launcher_exe(None), cfg!(target_os = "linux"));
    }

    const LAUNCHER_AT: &str = "/test/skill-start";
    const OWN: u32 = 990;

    /// A peer as the test says: its uid, its program's link, its program
    /// file's identity, the launcher file's, its environment, its maps and
    /// the files they name, its tracer, its `/proc` folder's owner, the pid
    /// its lines are sent by, and from which line read on (0: the id, 1:
    /// ready) its program is another's.
    #[derive(Clone)]
    struct Fake {
        uid: u32,
        link: PathBuf,
        exe: FileSeen,
        launcher: FileSeen,
        environ: Vec<u8>,
        maps: String,
        files: HashMap<PathBuf, FileSeen>,
        tracer: u32,
        owner: u32,
        ns_root: Option<u32>,
        sender: Option<u32>,
        ready_sender: Option<u32>,
        other_from: usize,
        lines: Arc<AtomicUsize>,
    }

    const PID: u32 = 4242;

    fn launcher_file() -> FileSeen {
        root_file(42)
    }

    /// `b3:02`, as Linux encodes it in `st_dev`.
    const DEV: u64 = (0xb3 << 8) | 0x02;

    fn root_file(ino: u64) -> FileSeen {
        FileSeen {
            file: true,
            uid: 0,
            mode: 0o100755,
            dev: DEV,
            ino,
        }
    }

    const LIBC: &str = "/usr/lib/aarch64-linux-gnu/libc.so.6";
    const LD: &str = "/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1";

    /// A launcher's maps as a lamp's would read: itself, libc and the
    /// loader, the kernel's vdso, and data that runs nothing.
    fn good_maps() -> String {
        [
            "aaaa0000-aaaa1000 r--p 00000000 b3:02 42                         /test/skill-start"
                .to_owned(),
            "aaaa1000-aaab0000 r-xp 00001000 b3:02 42                         /test/skill-start"
                .to_owned(),
            "aaac0000-aaad0000 rw-p 00000000 00:00 0                          [heap]".to_owned(),
            format!("ffff0000-ffff9000 r-xp 00000000 b3:02 100                        {LIBC}"),
            format!("ffffa000-ffffb000 r-xp 00000000 b3:02 101                        {LD}"),
            "ffffc000-ffffd000 rw-p 00000000 00:00 0 ".to_owned(),
            "ffffd000-ffffe000 r-xp 00000000 00:00 0                          [vdso]".to_owned(),
        ]
        .join("\n")
            + "\n"
    }

    impl Fake {
        fn good() -> Self {
            Self {
                uid: OWN,
                link: LAUNCHER_AT.into(),
                exe: launcher_file(),
                launcher: launcher_file(),
                environ: b"PATH=/usr/bin\0HOME=/var/lib/lampos\0".to_vec(),
                maps: good_maps(),
                files: [(LIBC.into(), root_file(100)), (LD.into(), root_file(101))].into(),
                tracer: 0,
                owner: 0,
                ns_root: None,
                sender: Some(PID),
                ready_sender: Some(PID),
                other_from: usize::MAX,
                lines: Arc::default(),
            }
        }

        fn file_at(&self, path: &Path) -> io::Result<FileSeen> {
            if path == Path::new(LAUNCHER_AT) {
                return Ok(self.launcher);
            }
            self.files
                .get(path)
                .copied()
                .ok_or_else(|| io::ErrorKind::NotFound.into())
        }
    }

    impl Peers for Fake {
        fn peer(&self, _: &UnixStream) -> Option<Peer> {
            Some(Peer {
                pid: PID,
                uid: self.uid,
            })
        }
        fn exe_link(&self, _: u32) -> io::Result<PathBuf> {
            if self.lines.load(Ordering::Acquire) > self.other_from {
                return Ok("/usr/bin/python3".into());
            }
            Ok(self.link.clone())
        }
        fn exe_file(&self, _: u32) -> io::Result<FileSeen> {
            Ok(self.exe)
        }
        fn file(&self, path: &Path) -> io::Result<FileSeen> {
            self.file_at(path)
        }
        fn environ(&self, _: u32) -> io::Result<Vec<u8>> {
            Ok(self.environ.clone())
        }
        fn maps(&self, _: u32) -> io::Result<String> {
            Ok(self.maps.clone())
        }
        fn status(&self, _: u32) -> io::Result<String> {
            Ok(format!(
                "Name:\tskill-start\nState:\tS (sleeping)\nTracerPid:\t{}\nUid:\t990\n",
                self.tracer
            ))
        }
        fn user_ns_root(&self, _: u32) -> io::Result<Option<u32>> {
            Ok(self.ns_root)
        }
        fn proc_owner(&self, _: u32) -> io::Result<u32> {
            // Dumpable (its uid's) until it said ready.
            Ok(if self.lines.load(Ordering::Acquire) >= 2 {
                self.owner
            } else {
                OWN
            })
        }
        fn read_line(&self, stream: &UnixStream, most: usize) -> io::Result<(String, Option<u32>)> {
            let line = read_line(stream, most)?;
            let read = self.lines.fetch_add(1, Ordering::AcqRel);
            Ok((
                line,
                if read == 0 {
                    self.sender
                } else {
                    self.ready_sender
                },
            ))
        }
    }

    /// Why `fake` may not claim, if it may not.
    fn peer_why(fake: &Fake) -> Result<(), String> {
        let (near, _far) = UnixStream::pair().unwrap();
        let peer = peer_of(fake, &near, OWN)?;
        peer_may(fake, peer.pid, Path::new(LAUNCHER_AT))
    }

    /// `fake` refused in `words`: no answer at all on its connection, the
    /// live run `task-1` never claimed, and its start ended for good.
    fn refused(fake: Fake, words: &str) {
        assert_eq!(peer_why(&fake), Err(words.to_owned()));
        refused_in_talk(fake, words);
    }

    /// `fake` refused in `words` in the talk itself: no grant, nothing
    /// claimed, the run's start ended (the uid's refusal comes before any
    /// id is read, so it ends nothing).
    fn refused_in_talk(fake: Fake, words: &str) {
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        let answer = claim(&starts, fake, "task-1", Some(STARTED));
        assert!(parse_answer(&answer).is_err(), "no grant: {answer}");
        assert!(!starts.state("task-1").0, "nothing claimed");
        if words == "the caller is not lamp-tasks' own user" {
            assert_eq!(answer, "", "no answer");
            assert_eq!(starts.refused("task-1"), None);
            return;
        }
        assert_eq!(starts.refused("task-1").as_deref(), Some(words));
        assert_eq!(
            live.ran(),
            Err(format!("the skill's start was refused: {words}"))
        );
        // Never claimable again, even by the launcher itself.
        assert_eq!(
            error_of(&claim(&starts, Fake::good(), "task-1", None)),
            "that run's start was refused already"
        );
        assert!(!starts.state("task-1").0);
    }

    #[test]
    fn a_peer_whose_program_is_not_the_launcher_is_refused_and_nothing_claimed() {
        let mut other = Fake::good();
        other.link = "/var/lib/lampos-tasks/harnesses/lamp-tasks/skill-task-1/start".into();
        refused(other, "the caller is not the skill-start launcher");
        // The same name bound over in another mount namespace: its own file.
        let mut bound_over = Fake::good();
        bound_over.exe.ino = 43;
        refused(
            bound_over,
            "the caller's program is not the skill-start launcher itself",
        );
        // A launcher others may write, or not root's.
        for (uid, mode) in [(0, 0o100775), (0, 0o100757), (OWN, 0o100755)] {
            let mut loose = Fake::good();
            loose.launcher.uid = uid;
            loose.launcher.mode = mode;
            loose.exe = loose.launcher;
            refused(
                loose,
                "the skill-start launcher is not root's own, or others may change it",
            );
        }
        // The launcher itself gets it.
        assert_eq!(peer_why(&Fake::good()), Ok(()));
        let starts = Starts::default();
        let _live = starts.open("task-1", grant(), TOKEN).unwrap();
        assert_eq!(error_of(&claim(&starts, Fake::good(), "task-1", None)), "");
        assert_eq!(starts.state("task-1"), (true, false));
    }

    #[test]
    fn a_peer_of_another_uid_is_refused() {
        let mut stranger = Fake::good();
        stranger.uid = OWN + 1;
        refused(stranger, "the caller is not lamp-tasks' own user");
    }

    #[test]
    fn a_peer_with_an_ld_variable_is_refused() {
        for set in [
            "LD_PRELOAD=/var/lib/lampos/x.so",
            "LD_AUDIT=/tmp/a.so",
            "LD_LIBRARY_PATH=/tmp",
        ] {
            let mut loaded = Fake::good();
            loaded.environ = format!("PATH=/usr/bin\0{set}\0").into_bytes();
            refused(loaded, "the caller has an LD_ variable set");
        }
        // A value naming LD_ is not a variable of it.
        let mut quoted = Fake::good();
        quoted.environ = b"NOTE=LD_PRELOAD\0".to_vec();
        assert_eq!(peer_why(&quoted), Ok(()));
    }

    #[test]
    fn executable_mappings_must_all_be_roots_own_code() {
        let files = Fake::good();
        let file = |path: &Path| files.file_at(path);
        assert_eq!(maps_may(&good_maps(), file), Ok(()));
        let with = |line: &str| format!("{}{line}\n", good_maps());
        // A lampos-owned library.
        let mut owned = Fake::good();
        owned.files.insert(
            "/usr/local/lib/evil.so".into(),
            FileSeen {
                uid: OWN,
                ..root_file(7)
            },
        );
        let owned_line = with("eeee0000-eeee1000 r-xp 00000000 b3:02 7 /usr/local/lib/evil.so");
        assert!(maps_may(&owned_line, |path| owned.file_at(path)).is_err());
        // Root's but group-writable, or another inode than mapped.
        let mut loose = Fake::good();
        loose.files.insert(
            "/usr/local/lib/x.so".into(),
            FileSeen {
                mode: 0o100775,
                ..root_file(8)
            },
        );
        let loose_line = with("eeee0000-eeee1000 r-xp 00000000 b3:02 8 /usr/local/lib/x.so");
        assert!(maps_may(&loose_line, |path| loose.file_at(path)).is_err());
        let swapped = with(&format!("eeee0000-eeee1000 r-xp 00000000 b3:02 9 {LIBC}"));
        assert!(maps_may(&swapped, file).is_err());
        // The same inode on another device, or a device not read.
        for device in ["b3:03", "b4:02", "00:00", "b3", "zz:02"] {
            let moved = with(&format!(
                "eeee0000-eeee1000 r-xp 00000000 {device} 100 {LIBC}"
            ));
            assert_eq!(
                maps_may(&moved, file),
                Err(format!(
                    "the caller's code {LIBC:?} is not root's own, or others may change it"
                )),
                "{device}"
            );
        }
        // A big device number as Linux encodes it.
        assert_eq!(
            dev_parts((0x12000_u64 << 32) | (0x345 << 8) | 0x67),
            (0x12345, 0x67)
        );
        assert_eq!(map_device("103:0a"), Some((0x103, 0x0a)));
        for (line, words) in [
            (
                "eeee0000-eeee1000 r-xp 00000000 00:00 0 ",
                "the caller has an anonymous executable mapping",
            ),
            (
                "eeee0000-eeee1000 rwxp 00000000 00:00 0",
                "the caller has an anonymous executable mapping",
            ),
            (
                "eeee0000-eeee1000 r-xp 00000000 b3:02 100 /usr/lib/libc.so.6 (deleted)",
                "the caller runs code from \"/usr/lib/libc.so.6 (deleted)\"",
            ),
            (
                "eeee0000-eeee1000 r-xp 00000000 00:01 5 /memfd:x (deleted)",
                "the caller runs code from \"/memfd:x (deleted)\"",
            ),
            (
                "eeee0000-eeee1000 r-xp 00000000 00:01 5 /memfd:x",
                "the caller runs code from \"/memfd:x\"",
            ),
            (
                "eeee0000-eeee1000 r-xp 00000000 b3:02 5 /var/lib/lampos/x.so",
                "the caller runs code from \"/var/lib/lampos/x.so\"",
            ),
            (
                "eeee0000-eeee1000 r-xp 00000000 b3:02 5 /dev/shm/x",
                "the caller runs code from \"/dev/shm/x\"",
            ),
            (
                "eeee0000-eeee1000 r-xp 00000000 00:00 0 [uprobes]",
                "the caller runs code from \"[uprobes]\"",
            ),
        ] {
            assert_eq!(maps_may(&with(line), file), Err(words.to_owned()), "{line}");
        }
        // Through the peer check: refused, no answer, nothing claimed.
        let mut anon = Fake::good();
        anon.maps = with("eeee0000-eeee1000 r-xp 00000000 00:00 0");
        refused(anon, "the caller has an anonymous executable mapping");
    }

    #[test]
    fn from_the_launchers_program_it_is_the_launcher_whatever_argv_says() {
        let os = |words: &[&str]| words.iter().map(OsString::from).collect::<Vec<_>>();
        let argv = os(&[
            "/usr/local/lib/lampos/skill-start",
            "serve",
            "--harness",
            "/tmp/x",
        ]);
        assert_eq!(
            launcher_args(true, &argv),
            Some(os(&["serve", "--harness", "/tmp/x"]))
        );
        let renamed = os(&["lamp-tasks", "serve", "--enable"]);
        assert_eq!(
            launcher_args(true, &renamed),
            Some(os(&["serve", "--enable"]))
        );
        assert_eq!(launcher_args(true, &os(&["x"])), Some(Vec::new()));
        // And then refused: never lamp-tasks' own commands.
        assert!(launcher_refusal(&os(&["serve", "--enable"]), []).is_err());
        assert_eq!(launcher_args(false, &renamed), None);
    }

    #[test]
    fn the_launcher_takes_exactly_one_run_id_as_lamp_tasks_makes_them() {
        let os = |words: &[&str]| words.iter().map(OsString::from).collect::<Vec<_>>();
        let made = crate::skill_task::run_id();
        assert_eq!(launcher_refusal(&os(&[&made]), []), Ok(made.clone()));
        let long = format!("task-{}-1-1", "9".repeat(70));
        for args in [
            os(&[]),
            os(&[&made, &made]),
            os(&["serve"]),
            os(&["--x"]),
            os(&["task-1-2-3;id"]),
            os(&["task-1-2-3 x"]),
            os(&["task-1"]),
            os(&[&long]),
        ] {
            assert!(launcher_refusal(&args, []).is_err(), "{args:?}");
        }
        let env: [&[u8]; 2] = [b"PATH", b"LD_PRELOAD"];
        assert_eq!(
            launcher_refusal(&os(&[&made]), env),
            Err("refused: an LD_ variable is set".into())
        );
        let env: [&[u8]; 2] = [b"PATH", b"OLD_X"];
        assert!(launcher_refusal(&os(&[&made]), env).is_ok());
    }

    /// A stand-in fence, as lamp-tasks starts one ([`spawn_fence`]): the
    /// token read from fd 3 and written to the file `$0`; `$1`, when not
    /// empty, said on fd 4, which is then closed (kept open otherwise);
    /// then `$2` run (by default: `out` on stdout, `err` on stderr, exit 3).
    const FAKE_FENCE: &str = r#"IFS= read -r t <&3 || exit 9; exec 3<&-; printf '%s' "$t" > "$0"; [ -z "$1" ] || { printf '%s\n' "$1" >&4; exec 4>&-; }; eval "$2""#;
    const FAKE_BODY: &str = "printf out; printf err >&2; exit 3";

    /// [`FAKE_FENCE`] with the token written to `token_at`, `said` on fd 4
    /// and `body` run.
    fn fake_fence(token_at: &Path, said: Option<&str>, body: &str) -> FenceCommand {
        let (token_at, said, body) = (
            token_at.to_owned(),
            said.unwrap_or_default().to_owned(),
            body.to_owned(),
        );
        Arc::new(move |_: &Grant| {
            let mut command = Command::new("/bin/sh");
            command
                .args(["-c", FAKE_FENCE])
                .arg(&token_at)
                .arg(&said)
                .arg(&body);
            command
        })
    }

    /// lamp-tasks' side for `fake`, its fence saying `started` and
    /// running [`FAKE_BODY`].
    fn server(starts: &Starts, fake: Fake) -> Server {
        server_saying(starts, fake, Some(STARTED))
    }

    /// lamp-tasks' side for `fake`, its fence saying `said`.
    fn server_saying(starts: &Starts, fake: Fake, said: Option<&str>) -> Server {
        Server {
            socket: PathBuf::from("/unused"),
            starts: starts.clone(),
            peers: Arc::new(fake),
            own_uid: OWN,
            launcher: LAUNCHER_AT.into(),
            started_within: Duration::from_secs(5),
            fence: fake_fence(Path::new("/dev/null"), said, FAKE_BODY),
        }
    }

    /// One launcher's talk with `served`, as [`ask`] then [`relay`] talk.
    struct Talk {
        /// Its last answer line: the grant, an error, or nothing.
        answer: String,
        /// Every byte the launcher was sent.
        heard: Vec<u8>,
        /// After a grant: the relay's exit status and words, and what it
        /// wrote to stdout and stderr.
        relayed: Option<(i32, String, Vec<u8>, Vec<u8>)>,
    }

    /// A launcher's claim of `id` on `served`, relayed to its end; returns
    /// once lamp-tasks has done with it.
    fn talk(served: Server, id: &str) -> Talk {
        let (near, far) = UnixStream::pair().unwrap();
        let serving = std::thread::spawn(move || served.one(far));
        let mut heard = Vec::new();
        write_line(&near, id).unwrap();
        let mut answer = read_line(&near, MOST_ANSWER_BYTES).unwrap_or_default();
        heard.extend(format!("{answer}\n").bytes());
        if let Some(nonce) = answer.strip_prefix(CHECKED_SPACE).map(str::to_owned) {
            let _ = write_line(&near, &format!("{READY} {nonce}"));
            answer = read_line(&near, MOST_ANSWER_BYTES).unwrap_or_default();
            heard.extend(format!("{answer}\n").bytes());
        }
        let relayed = parse_answer(&answer).is_ok().then(|| {
            let mut rest = Vec::new();
            let _ = (&near).read_to_end(&mut rest);
            heard.extend(&rest);
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let (code, why) = relay(&rest[..], &mut out, &mut err);
            (code, why, out, err)
        });
        drop(near);
        serving.join().unwrap();
        Talk {
            answer,
            heard,
            relayed,
        }
    }

    /// One launcher's claim of `id` from a peer `fake`, its fence saying
    /// `then` on fd 4 (`started`, or nothing): its last answer line (the
    /// grant, an error, or nothing); returns once lamp-tasks has done with
    /// it.
    fn claim(starts: &Starts, fake: Fake, id: &str, then: Option<&str>) -> String {
        talk(server_saying(starts, fake, then), id).answer
    }

    fn error_of(answer: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(answer).unwrap_or_default();
        value["error"].as_str().unwrap_or_default().to_owned()
    }

    #[test]
    fn an_unknown_or_ended_run_is_refused() {
        let starts = Starts::default();
        assert_eq!(
            error_of(&claim(&starts, Fake::good(), "task-9", None)),
            "no live skill run of this lamp has that id"
        );
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        drop(live);
        assert!(!starts.is_live("task-1"));
        assert_eq!(
            error_of(&claim(&starts, Fake::good(), "task-1", None)),
            "no live skill run of this lamp has that id"
        );
        assert_eq!(
            error_of(&claim(&starts, Fake::good(), "../x", None)),
            "no plain run id was sent"
        );
    }

    #[test]
    fn a_good_claim_answers_the_grant_once_and_a_second_is_refused() {
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        let answer = claim(&starts, Fake::good(), "task-1", None);
        let value: serde_json::Value = serde_json::from_str(&answer).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            [
                "interpreter",
                "pythonpath",
                "run",
                "script",
                "skill",
                "socket"
            ]
        );
        assert!(!answer.contains(TOKEN), "never the token");
        assert_eq!(parse_answer(&answer), Ok(grant()));
        assert_eq!(starts.state("task-1"), (true, false));
        assert_eq!(
            error_of(&claim(&starts, Fake::good(), "task-1", Some(STARTED))),
            "that run's start was claimed already"
        );
        // A refused second claim ends the run's start in its words.
        assert_eq!(
            live.ran(),
            Err("the skill's start was refused: that run's start was claimed already".into())
        );
    }

    #[test]
    fn claimed_and_started_ran_and_claimed_alone_did_not() {
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        assert_eq!(live.ran(), Err(crate::gateway::NEVER_STARTED.into()));
        claim(&starts, Fake::good(), "task-1", None);
        assert_eq!(
            live.ran(),
            Err(
                "the skill's start was claimed but its script never started (its fence failed)"
                    .into()
            )
        );
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        claim(&starts, Fake::good(), "task-1", Some("nope"));
        assert!(live.ran().is_err(), "only the word started counts");
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        claim(&starts, Fake::good(), "task-1", Some(STARTED));
        assert_eq!(live.ran(), Ok(()));
        drop(live);
        assert_eq!(
            error_of(&claim(&starts, Fake::good(), "task-1", None)),
            "no live skill run of this lamp has that id"
        );
    }

    #[test]
    fn an_answer_with_an_error_is_its_words() {
        assert_eq!(
            parse_answer(r#"{"error":"no live skill run of this lamp has that id"}"#),
            Err("no live skill run of this lamp has that id".into())
        );
        assert!(parse_answer(r#"{"script":"/x"}"#).is_err());
        // A grant with a token is not one: the launcher never takes one.
        let mut with_token = serde_json::to_value(grant()).unwrap();
        with_token["token"] = TOKEN.into();
        assert!(parse_answer(&with_token.to_string()).is_err());
    }

    #[test]
    fn the_launcher_is_its_file_name_or_first_argument() {
        let os = |words: &[&str]| words.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            invoked_as_launcher(&os(&[LAUNCHER, "task-1"])),
            Some(os(&["task-1"]))
        );
        assert_eq!(
            invoked_as_launcher(&os(&[
                "/opt/lampOS/tasks/current/lamp-tasks",
                NAME,
                "task-1"
            ])),
            Some(os(&["task-1"]))
        );
        assert_eq!(
            invoked_as_launcher(&os(&["/opt/lampOS/tasks/current/lamp-tasks", "serve"])),
            None
        );
        assert_eq!(LAUNCHER, "/usr/local/lib/lampos/skill-start");
        assert_eq!(START_SOCKET, "/run/lampos-skill-start/start.sock");
        assert_eq!(Path::new(START_SOCKET).parent(), Some(Path::new(START_DIR)));
    }

    /// The fence's last shell, run as the fence runs it: the token read
    /// from fd 3 into the script's environment, `started` written to fd 4
    /// before the script runs, and neither fd left to the script.
    #[test]
    fn the_fences_last_shell_takes_the_token_on_fd_3_and_says_started_on_fd_4() {
        let inner = crate::gateway::FENCE_TEXT
            .split_once("/bin/sh -c '")
            .and_then(|(_, rest)| rest.split_once("' run \"$@\""))
            .unwrap()
            .0;
        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("token");
        let said = dir.path().join("said");
        std::fs::write(&token, "secret-7133\n").unwrap();
        // Each fd probe runs in a subshell: in dash (the lamps' /bin/sh) a
        // failed redirection on the special builtin `:` exits the whole shell.
        let script = r#"printf 'token=%s\n' "$LAMP_CONTROL_TOKEN"; printf 'said=%s\n' "$(cat "$0")"; ( : <&3 ) 2>/dev/null && echo fd3; ( : >&4 ) 2>/dev/null && echo fd4; true"#;
        let out = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                r#"exec 3<"$1" 4>"$2"; exec /bin/sh -c "$0" run /bin/sh -c "$3" "$2""#,
                inner,
            ])
            .arg(&token)
            .arg(&said)
            .arg(script)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            "token=secret-7133\nsaid=started\n"
        );
        // No token on fd 3: nothing is said and the script never runs.
        std::fs::write(&token, "").unwrap();
        std::fs::remove_file(&said).unwrap();
        let out = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                r#"exec 3<"$1" 4>"$2"; exec /bin/sh -c "$0" run /bin/sh -c "echo ran""#,
                inner,
            ])
            .arg(&token)
            .arg(&said)
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert_eq!(std::fs::read_to_string(&said).unwrap(), "");
    }

    /// lamp-c0a4: `/run` is read-only in the gateway's namespace, so a
    /// `mount` that writes libmount's utab fails after mounting; each of
    /// the fence's mounts is `mount -n`.
    #[test]
    fn every_mount_in_the_fence_writes_no_utab() {
        let mounts: Vec<&str> = crate::gateway::FENCE_TEXT
            .lines()
            .map(str::trim_start)
            .filter(|line| line.contains("mount "))
            .collect();
        assert_eq!(mounts.len(), 14, "{mounts:?}");
        for line in mounts {
            assert_eq!(
                line.matches("mount ").count(),
                line.matches("mount -n ").count(),
                "{line}"
            );
        }
    }

    /// The launcher's `unshare` arguments: the fence and its arguments,
    /// never the token.
    #[test]
    fn the_fences_arguments_never_carry_the_token() {
        let args = crate::gateway::fence_args(&grant(), 990, 990, &[Path::new("/var/lib")]);
        let words: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(words.iter().all(|word| !word.contains(TOKEN)));
        assert_eq!(&words[..3], ["--user", "--map-root-user", "--net"]);
        let fence = words.iter().position(|word| word == "fence").unwrap();
        assert_eq!(
            &words[fence + 1..],
            [
                "990",
                "990",
                "/var/lib/lampos-tasks/harnesses/lamp-tasks",
                "/var/lib/lampos-tasks/harnesses/lamp-tasks/skill-task-1",
                // The run's folder's `dev:inode`: none here, so `-`, which
                // no bound folder matches.
                "-",
                "/var/lib/lampOS/skills/photo-booth",
                "/run/lampos-control/control.sock",
                "/opt/lampOS/sdk",
                "python3",
                "/var/lib/lampOS/skills/photo-booth/scripts/main.py",
                crate::gateway::SCRIPT_MEMORY_KB,
                "/var/lib",
            ]
        );
        assert!(words[fence - 1].contains("printf \"started\\n\" >&4"));
        assert!(!words[fence - 1].contains("$SCRIPT_PATH"));
    }

    /// #7133 (Twelve's HOLD): a process that connected and then exec'd the
    /// launcher, with a child keeping the socket, is refused: the id's
    /// sender must be the connecting pid, and its image is checked after
    /// the id was read, not at the connect.
    #[test]
    fn the_id_must_come_from_the_caller_itself_its_image_checked_after_it() {
        let mut child = Fake::good();
        child.sender = Some(PID + 1);
        refused_in_talk(child, "the run id was not sent by the caller itself");
        let mut no_creds = Fake::good();
        no_creds.sender = None;
        refused_in_talk(no_creds, "the run id was not sent by the caller itself");
        // The launcher's image at the connect, another's once the id came.
        let mut changed = Fake::good();
        changed.other_from = 0;
        refused_in_talk(changed, "the caller is not the skill-start launcher");
        // And its ready from another pid.
        let mut ready_child = Fake::good();
        ready_child.ready_sender = Some(PID + 1);
        refused_in_talk(
            ready_child,
            "the launcher's ready was not sent by the caller itself",
        );
    }

    /// #7133 (c0a4's first OpenClaw run): the launcher started in
    /// OpenClaw's gateway (a user namespace whose root is lampos outside)
    /// is non-dumpable at ready with its `/proc` lampos's, as the kernel
    /// gives it there: taken, still untraced. Another owner there, or a
    /// traced one, is refused; in lamp-tasks' own namespace only root's.
    #[test]
    fn in_the_gateways_user_namespace_its_root_owns_the_undumped_launcher() {
        const LAMPOS: u32 = 1001;
        let mut gateway = Fake::good();
        gateway.ns_root = Some(LAMPOS);
        gateway.owner = LAMPOS;
        gateway.lines.store(2, Ordering::Release);
        assert_eq!(undumped_may(&gateway, PID), Ok(()));
        let mut other = Fake::good();
        other.ns_root = Some(LAMPOS);
        other.owner = 4242;
        other.lines.store(2, Ordering::Release);
        assert!(undumped_may(&other, PID).is_err());
        let mut traced = Fake::good();
        traced.ns_root = Some(LAMPOS);
        traced.owner = LAMPOS;
        traced.tracer = 77;
        traced.lines.store(2, Ordering::Release);
        assert_eq!(
            undumped_may(&traced, PID),
            Err("the caller is being traced".into())
        );
        let mut own = Fake::good();
        own.owner = LAMPOS;
        own.lines.store(2, Ordering::Release);
        assert!(
            undumped_may(&own, PID).is_err(),
            "lamp-tasks' namespace: root's only"
        );
        assert_eq!(
            ns_root_outside("         0       1001          1\n"),
            Some(1001)
        );
        assert_eq!(ns_root_outside("0 0 4294967295\n"), Some(0));
        assert_eq!(ns_root_outside("1 1001 1\n"), None);
        assert_eq!(ns_root_outside(""), None);
    }

    #[test]
    fn a_traced_caller_or_one_still_dumpable_at_ready_is_refused() {
        let mut traced = Fake::good();
        traced.tracer = 77;
        refused(traced, "the caller is being traced");
        let mut dumpable = Fake::good();
        dumpable.owner = OWN;
        refused_in_talk(dumpable, "the caller can still be traced (it is dumpable)");
        assert_eq!(tracer("Name:\tx\nTracerPid:\t0\n"), Some(0));
        assert_eq!(tracer("Name:\tx\nTracerPid:\t812\n"), Some(812));
        assert_eq!(tracer("Name:\tx\n"), None);
    }

    /// The grant is never sent before [`READY`]; a peer that never says it
    /// gets nothing and claims nothing, and the run's start is ended.
    #[test]
    fn the_grant_comes_only_after_ready() {
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        let (near, far) = UnixStream::pair().unwrap();
        let served = server(&starts, Fake::good());
        let serving = std::thread::spawn(move || served.one(far));
        write_line(&near, "task-1").unwrap();
        assert!(
            read_line(&near, MOST_ANSWER_BYTES)
                .unwrap()
                .starts_with(CHECKED_SPACE)
        );
        near.set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let mut byte = [0u8; 1];
        assert!(
            (&near).read(&mut byte).is_err(),
            "nothing more before ready"
        );
        assert_eq!(starts.state("task-1"), (false, false));
        drop(near);
        serving.join().unwrap();
        assert_eq!(starts.state("task-1"), (false, false), "nothing claimed");
        assert_eq!(
            live.ran(),
            Err("the skill's start was refused: the launcher never said it was ready".into())
        );
        // With ready, the grant.
        let starts = Starts::default();
        let _live = starts.open("task-1", grant(), TOKEN).unwrap();
        let answer = claim(&starts, Fake::good(), "task-1", None);
        assert_eq!(parse_answer(&answer), Ok(grant()));
    }

    /// A ready queued with the id, before [`CHECKED`] (a process that
    /// queued it and then exec'd the launcher), is refused outright: no
    /// grant, nothing claimed, the run's start ended.
    #[test]
    fn a_ready_queued_before_checked_is_refused() {
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        let (near, far) = UnixStream::pair().unwrap();
        let served = server(&starts, Fake::good());
        let serving = std::thread::spawn(move || served.one(far));
        write_line(&near, "task-1\nready").unwrap();
        let mut answer = read_line(&near, MOST_ANSWER_BYTES).unwrap_or_default();
        if answer.starts_with(CHECKED) {
            // Nothing more sent: only the queued ready.
            answer = read_line(&near, MOST_ANSWER_BYTES).unwrap_or_default();
        }
        drop(near);
        serving.join().unwrap();
        assert!(parse_answer(&answer).is_err(), "no grant: {answer}");
        assert_eq!(starts.state("task-1"), (false, false), "nothing claimed");
        assert_eq!(
            live.ran(),
            Err(format!("the skill's start was refused: {QUEUED}"))
        );
    }

    /// A ready whose nonce is not the one [`CHECKED`] gave, or has none,
    /// is refused and ends the run's start.
    #[test]
    fn a_ready_with_a_wrong_or_no_nonce_is_refused() {
        for ready in [format!("{READY} 0000000000000000"), READY.to_owned()] {
            let starts = Starts::default();
            let live = starts.open("task-1", grant(), TOKEN).unwrap();
            let (near, far) = UnixStream::pair().unwrap();
            let served = server(&starts, Fake::good());
            let serving = std::thread::spawn(move || served.one(far));
            write_line(&near, "task-1").unwrap();
            let answer = read_line(&near, MOST_ANSWER_BYTES).unwrap();
            let nonce = answer.strip_prefix(CHECKED_SPACE).unwrap();
            assert_eq!(nonce.len(), 16);
            assert!(
                nonce
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            );
            assert_ne!(ready, format!("{READY} {nonce}"));
            write_line(&near, &ready).unwrap();
            let answer = read_line(&near, MOST_ANSWER_BYTES).unwrap_or_default();
            drop(near);
            serving.join().unwrap();
            assert!(parse_answer(&answer).is_err(), "no grant: {answer}");
            assert_eq!(starts.state("task-1"), (false, false), "nothing claimed");
            assert_eq!(
                live.ran(),
                Err("the skill's start was refused: the launcher never said it was ready".into())
            );
        }
    }

    /// The launcher's side: non-dumpable before it says [`READY`], and
    /// never ready (exit 2) when it can't be.
    #[test]
    fn the_launcher_is_non_dumpable_before_it_says_ready() {
        let talk = |undumpable_ok: bool| {
            let (near, far) = UnixStream::pair().unwrap();
            let undumped = Arc::new(AtomicBool::new(false));
            let seen = undumped.clone();
            let lamp = std::thread::spawn(move || {
                assert_eq!(read_line(&far, MOST_ID_BYTES).unwrap(), "task-1");
                write_line(&far, "checked 0123456789abcdef").unwrap();
                let ready = read_line(&far, READY.len() + 1 + NONCE_HEX);
                let at_ready = seen.load(Ordering::Acquire);
                if ready.is_ok() {
                    write_line(&far, &serde_json::to_string(&grant()).unwrap()).unwrap();
                }
                (ready.ok(), at_ready)
            });
            let asked = ask(&near, "task-1", || {
                undumped.store(true, Ordering::Release);
                if undumpable_ok {
                    Ok(())
                } else {
                    Err(io::Error::other("no"))
                }
            });
            drop(near);
            (asked, lamp.join().unwrap())
        };
        let (asked, (ready, undumped_at_ready)) = talk(true);
        assert_eq!(asked, Ok(grant()));
        assert_eq!(ready.as_deref(), Some("ready 0123456789abcdef"));
        assert!(undumped_at_ready, "non-dumpable before ready");
        let (asked, (ready, _)) = talk(false);
        assert_eq!(
            asked,
            Err((
                2,
                "refused: the launcher could not be made non-dumpable: no".into()
            ))
        );
        assert_eq!(ready, None, "never ready");
        // Against lamp-tasks' own side, end to end.
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        let (near, far) = UnixStream::pair().unwrap();
        let served = server(&starts, Fake::good());
        let serving = std::thread::spawn(move || served.one(far));
        assert_eq!(ask(&near, "task-1", || Ok(())), Ok(grant()));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(relay(&near, &mut out, &mut err), (3, String::new()));
        assert_eq!((&out[..], &err[..]), (&b"out"[..], &b"err"[..]));
        drop(near);
        serving.join().unwrap();
        assert_eq!(live.ran(), Ok(()));
    }

    /// The fence is made in lamp-tasks' own namespaces (#7024): it hides
    /// the launcher's socket and lamp-tasks' own runtime folder, its
    /// sockets and the Hermes runs' token files (all of `/run`).
    #[test]
    fn the_fence_hides_the_launchers_socket_and_lamp_tasks_own_folder() {
        for shown in [
            START_DIR,
            START_SOCKET,
            "/run/lampos-tasks/request.sock",
            "/run/lampos-tasks/app.sock",
            crate::run_token_file::RUNS_DIR,
        ] {
            assert!(
                crate::gateway::HIDDEN
                    .iter()
                    .any(|hidden| Path::new(shown).starts_with(hidden)),
                "{shown}"
            );
        }
    }

    fn mode(path: &Path) -> u32 {
        std::fs::symlink_metadata(path).unwrap().mode() & 0o7777
    }

    #[test]
    fn the_socket_is_bound_0600_in_lamp_updates_folder_never_made_and_a_stale_one_replaced() {
        use std::os::unix::fs::DirBuilderExt;
        let own = rustix::process::geteuid().as_raw();
        let root = tempfile::Builder::new()
            .prefix("ss")
            .tempdir_in("/tmp")
            .unwrap();
        let dir = root.path().join("start");
        let socket = dir.join("start.sock");
        // No folder: nothing served, and it is not made.
        assert_eq!(
            bind(&socket, own).err().map(|error| error.kind()),
            Some(io::ErrorKind::NotFound)
        );
        assert!(!dir.exists(), "never made here");
        // lamp-update's folder: taken as it is, kept at the stop.
        std::fs::DirBuilder::new().mode(0o755).create(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (listener, bound) = bind(&socket, own).unwrap();
        assert_eq!((mode(&dir), mode(&socket)), (0o755, 0o600));
        drop((listener, bound));
        assert!(!socket.exists(), "removed at the stop");
        assert_eq!(mode(&dir), 0o755, "the folder kept as it was");
        let (listener, _bound) = bind(&socket, own).unwrap();
        // A stale socket from an earlier lamp-tasks: replaced.
        std::mem::forget(_bound);
        drop(listener);
        assert!(
            std::fs::symlink_metadata(&socket)
                .unwrap()
                .file_type()
                .is_socket()
        );
        let (_listener, _bound) = bind(&socket, own).unwrap();
        assert!(UnixStream::connect(&socket).is_ok());
        // A folder others may write, or another's: refused.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(bind(&socket, own).is_err());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(bind(&socket, own + 1).is_err());
        // A plain file at the socket's path: refused, kept.
        let other = root.path().join("other");
        std::fs::create_dir(&other).unwrap();
        std::fs::write(other.join("start.sock"), "x").unwrap();
        assert!(bind(&other.join("start.sock"), own).is_err());
        assert!(other.join("start.sock").is_file());
    }

    /// The Reviewer-lead's boot-order P2 on #7133: lamp-tasks started
    /// before the folder is made (a boot with the gateway later, or the
    /// switch turned on while it runs) still serves the launcher once the
    /// folder is there, and again after the folder is taken out and made
    /// again; the socket is removed at the stop.
    #[test]
    fn the_socket_is_bound_once_its_folder_is_there_and_again_when_it_comes_back() {
        use std::os::unix::fs::DirBuilderExt;
        let root = tempfile::Builder::new()
            .prefix("ss")
            .tempdir_in("/tmp")
            .unwrap();
        let dir = root.path().join("start");
        let socket = dir.join("start.sock");
        let served = Server {
            socket: socket.clone(),
            own_uid: rustix::process::geteuid().as_raw(),
            ..server(&Starts::default(), Fake::good())
        };
        let stop = Arc::new(AtomicBool::new(false));
        let serving = served
            .start(stop.clone())
            .expect("served without its folder");
        let connects = |within: Duration| {
            let until = std::time::Instant::now() + within;
            while std::time::Instant::now() < until {
                if UnixStream::connect(&socket).is_ok() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        };
        let make = || {
            std::fs::DirBuilder::new().mode(0o755).create(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        assert!(
            !connects(Duration::from_millis(100)),
            "no folder, nothing served"
        );
        assert!(!dir.exists(), "never made here");
        make();
        assert!(connects(BIND_AGAIN * 2), "bound once the folder is there");
        std::fs::remove_dir_all(&dir).unwrap();
        make();
        assert!(
            connects(BIND_AGAIN * 3),
            "bound again after the folder came back"
        );
        stop.store(true, Ordering::Release);
        serving.join().unwrap();
        assert!(!socket.exists(), "removed at the stop");
    }

    #[test]
    fn relay_frames_round_trip_and_are_bounded() {
        let frames = [
            Frame::Stdout(b"hello\n".to_vec()),
            Frame::Stderr(Vec::new()),
            Frame::Stdout(vec![7; MOST_FRAME_BYTES]),
            Frame::Exit(-3),
            Frame::Exit(0),
            Frame::Error("no fence".into()),
        ];
        let mut bytes = Vec::new();
        for frame in &frames {
            bytes.extend(frame.encode().unwrap());
        }
        let mut from = &bytes[..];
        for frame in &frames {
            assert_eq!(Frame::read(&mut from).unwrap().as_ref(), Some(frame));
        }
        assert_eq!(Frame::read(&mut from).unwrap(), None, "the end");
        assert_eq!(
            Frame::Exit(258).encode().unwrap(),
            [3, 0, 0, 0, 4, 0, 0, 1, 2]
        );
        assert_eq!(
            Frame::Stdout(b"ab".to_vec()).encode().unwrap(),
            [1, 0, 0, 0, 2, b'a', b'b']
        );
        // Over the bound: never written, never read.
        assert!(
            Frame::Stdout(vec![0; MOST_FRAME_BYTES + 1])
                .encode()
                .is_err()
        );
        let long = "é".repeat(MOST_FRAME_BYTES);
        let cut = Frame::Error(long).encode().unwrap();
        assert!(cut.len() <= 5 + MOST_FRAME_BYTES);
        assert!(matches!(Frame::read(&cut[..]), Ok(Some(Frame::Error(_)))));
        let over = u32::try_from(MOST_FRAME_BYTES + 1).unwrap().to_be_bytes();
        for bad in [
            vec![1, over[0], over[1], over[2], over[3]],
            vec![9, 0, 0, 0, 0],
            vec![0, 0, 0, 0, 0],
            vec![3, 0, 0, 0, 3, 0, 0, 0],
            vec![4, 0, 0, 0, 1, 0xff],
            vec![1, 0, 0],
            vec![1, 0, 0, 0, 4, b'a'],
        ] {
            assert!(Frame::read(&bad[..]).is_err(), "{bad:?}");
        }
    }

    /// The launcher's side: each chunk to its own stream, in order, and the
    /// exit code; an error's words, an early end or bytes not understood
    /// are exit 1 in words.
    #[test]
    fn the_launchers_relay_writes_each_chunk_and_exits_with_the_code() {
        let framed = |frames: &[Frame]| -> Vec<u8> {
            frames.iter().flat_map(|f| f.encode().unwrap()).collect()
        };
        let run = |bytes: Vec<u8>| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let (code, why) = relay(&bytes[..], &mut out, &mut err);
            (code, why, out, err)
        };
        let (code, why, out, err) = run(framed(&[
            Frame::Stdout(b"a".to_vec()),
            Frame::Stderr(b"b".to_vec()),
            Frame::Stdout(b"c".to_vec()),
            Frame::Exit(7),
            Frame::Stdout(b"never".to_vec()),
        ]));
        assert_eq!((code, why.as_str()), (7, ""));
        assert_eq!((&out[..], &err[..]), (&b"ac"[..], &b"b"[..]));
        assert_eq!(run(framed(&[Frame::Exit(300)])).0, 1);
        assert_eq!(run(framed(&[Frame::Exit(-1)])).0, 1);
        let (code, why, ..) = run(framed(&[Frame::Error("no fence".into())]));
        assert_eq!(code, 1);
        assert!(why.ends_with("no fence"), "{why}");
        let (code, why, out, _) = run(framed(&[Frame::Stdout(b"x".to_vec())]));
        assert_eq!(
            (code, why.as_str(), &out[..]),
            (1, "lamp-tasks ended the run before its end", &b"x"[..])
        );
        let (code, why, ..) = run(vec![9, 0, 0, 0, 0]);
        assert_eq!(
            (code, why.as_str()),
            (1, "lamp-tasks' relay was not understood")
        );
        // Output that can't be written: the launcher leaves.
        struct Shut;
        impl Write for Shut {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let bytes = framed(&[Frame::Stdout(b"x".to_vec()), Frame::Exit(0)]);
        assert_eq!(relay(&bytes[..], Shut, Vec::new()).0, 1);
    }

    /// #7024: lamp-tasks starts the fence itself, the token on its fd 3,
    /// and the launcher only relays: not one byte it is sent holds the
    /// token, while the fence got it; its output and exit code relayed.
    #[test]
    fn the_token_reaches_only_the_fence_never_the_launcher() {
        let dir = tempfile::tempdir().unwrap();
        let token_at = dir.path().join("token");
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        let served = Server {
            fence: fake_fence(&token_at, Some(STARTED), FAKE_BODY),
            ..server(&starts, Fake::good())
        };
        let talked = talk(served, "task-1");
        assert_eq!(parse_answer(&talked.answer), Ok(grant()));
        assert_eq!(std::fs::read_to_string(&token_at).unwrap(), TOKEN);
        let heard = String::from_utf8_lossy(&talked.heard);
        assert!(
            !heard.contains(TOKEN),
            "the launcher heard the token: {heard}"
        );
        assert_eq!(
            talked.relayed,
            Some((3, String::new(), b"out".to_vec(), b"err".to_vec()))
        );
        assert_eq!(live.ran(), Ok(()));
    }

    /// Waits until run `id` started, at most 10 s.
    fn wait_started(starts: &Starts, id: &str) {
        let until = Instant::now() + Duration::from_secs(10);
        while !starts.state(id).1 {
            assert!(Instant::now() < until, "never started");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The launcher's connection closed before the script's end: the fence
    /// is killed, not left running for no one.
    #[test]
    fn a_launcher_gone_before_the_end_kills_the_run() {
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        let served = Server {
            fence: fake_fence(Path::new("/dev/null"), Some(STARTED), "exec sleep 30"),
            ..server(&starts, Fake::good())
        };
        let (near, far) = UnixStream::pair().unwrap();
        let begun = Instant::now();
        let serving = std::thread::spawn(move || served.one(far));
        assert_eq!(ask(&near, "task-1", || Ok(())), Ok(grant()));
        wait_started(&starts, "task-1");
        drop(near);
        serving.join().unwrap();
        assert!(
            begun.elapsed() < Duration::from_secs(15),
            "killed, not waited"
        );
        assert_eq!(live.ran(), Ok(()));
    }

    /// The run ended (its deadline, its stop): its fence is killed and the
    /// launcher told the script's end (killed: 128 + 9).
    #[test]
    fn a_run_ended_kills_its_fence() {
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        let served = Server {
            fence: fake_fence(Path::new("/dev/null"), Some(STARTED), "exec sleep 30"),
            ..server(&starts, Fake::good())
        };
        let (near, far) = UnixStream::pair().unwrap();
        let begun = Instant::now();
        let serving = std::thread::spawn(move || served.one(far));
        let launcher = std::thread::spawn(move || {
            assert_eq!(ask(&near, "task-1", || Ok(())), Ok(grant()));
            relay(&near, Vec::new(), Vec::new())
        });
        wait_started(&starts, "task-1");
        drop(live);
        assert_eq!(launcher.join().unwrap(), (137, String::new()));
        serving.join().unwrap();
        assert!(
            begun.elapsed() < Duration::from_secs(15),
            "killed, not waited"
        );
        // Ended before its claim: no fence is started at all.
        let fence = Fence::default();
        fence.end();
        assert!(
            fence
                .start(|| -> io::Result<(Child, ())> { panic!("started") })
                .is_err()
        );
    }

    /// A fence that has not said [`STARTED`] in time is killed, and the run
    /// fails closed as one whose fence failed.
    #[test]
    fn a_fence_that_never_says_started_is_killed() {
        let starts = Starts::default();
        let live = starts.open("task-1", grant(), TOKEN).unwrap();
        let served = Server {
            started_within: Duration::from_millis(300),
            fence: fake_fence(Path::new("/dev/null"), None, "exec sleep 30"),
            ..server(&starts, Fake::good())
        };
        let begun = Instant::now();
        let talked = talk(served, "task-1");
        assert!(
            begun.elapsed() < Duration::from_secs(15),
            "killed, not waited"
        );
        let (code, why, ..) = talked.relayed.unwrap();
        assert_eq!(code, 1);
        assert!(
            why.ends_with("the run's fence never said it started"),
            "{why}"
        );
        assert_eq!(live.ran(), Err(crate::gateway::FENCE_FAILED.into()));
    }

    /// The fence's fds 3 and 4 are the token's pipe and lamp-tasks'
    /// `started` pipe, whatever lamp-tasks had at 3 and 4, and no other fd
    /// of lamp-tasks reaches it.
    #[test]
    fn the_fence_gets_the_token_on_3_and_said_on_4_and_nothing_else() {
        let held = tempfile::tempfile().unwrap();
        let stray = rustix::io::fcntl_dupfd_cloexec(&held, 10).unwrap();
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            &format!(
                r#"IFS= read -r t <&3; printf '%s|' "$t"; printf started >&4; [ -e /dev/fd/{} ] && printf leaked; true"#,
                stray.as_raw_fd()
            ),
        ]);
        let mut spawned = spawn_fence(command, TOKEN).unwrap();
        let mut out = String::new();
        spawned.out.read_to_string(&mut out).unwrap();
        let mut said = String::new();
        spawned.said.read_to_string(&mut said).unwrap();
        assert!(spawned.child.wait().unwrap().success());
        assert_eq!(out, format!("{TOKEN}|"));
        assert_eq!(said, "started");
    }
}
