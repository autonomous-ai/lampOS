//! Who is on the other end of a connection, by what the kernel says of it:
//! the peer's user and process (`SO_PEERCRED`), its cgroup (`/proc/<pid>/cgroup`)
//! and its start time. Used both ways: the helper admits only lamp-cloud and
//! the voice, and takes the owner's yes only from lamp-control itself.
//!
//! The lamps run kernel 5.15 (75c9: 5.15.147): it has NO `SO_PEERPIDFD` (added
//! in 6.5), so a pid read after the connection can in principle belong to a
//! process that came after the peer's. The fallback is what is checked here:
//! the peer's start time is the same before and after its cgroup is read, and
//! it is not later than this helper's own (the helper is started for a
//! connection that the peer made, so the peer was already there); and the
//! user must be root or lampos. A pid reused in the narrow window by a process
//! of the same user, inside the same unit, would not be seen: it is not a
//! different caller.
use std::os::fd::AsFd;

/// `lamp-cloud`, where the system's manager puts it.
pub const CLOUD: &str = "/lampos.slice/lampos-parts.slice/lamp-cloud.service";
/// `lamp-voice`, where the system's manager puts it.
pub const VOICE: &str = "/lampos.slice/lampos-voice.slice/lamp-voice.service";
/// `lamp-control`, which keeps the owner's yes.
pub const CONTROL: &str = "/lampos.slice/lampos-parts.slice/lamp-control.service";

/// What a peer that is not one of them is told.
pub const NOT_AN_ASKER: &str = "only lamp-cloud and the voice may ask for the apps";

/// Whether `/proc/<pid>/cgroup`'s text puts the process in exactly one of
/// `units` (its own cgroup, not one below it).
pub fn in_one_of(cgroup: &str, units: &[&str]) -> bool {
    cgroup.lines().any(|line| {
        let mut fields = line.splitn(3, ':');
        let (Some(_), Some(controllers), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return false;
        };
        (controllers.is_empty() || controllers == "name=systemd") && units.contains(&path)
    })
}

pub fn is_an_asker(cgroup: &str) -> bool {
    in_one_of(cgroup, &[CLOUD, VOICE])
}

/// Whether a peer may ask: its cgroup is lamp-cloud's or the voice's AND its
/// user is root or lampos (`owner`): every `DynamicUser` app is turned away.
pub fn peer_ok(uid: u32, owner: u32, cgroup: &str) -> bool {
    (uid == 0 || uid == owner) && is_an_asker(cgroup)
}

/// The start time (field 22, in clock ticks since boot) from a
/// `/proc/<pid>/stat` line; the command's name may hold spaces and brackets,
/// so the fields are counted after its last `)`.
pub fn start_of_stat(stat: &str) -> Option<u64> {
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(19)?.parse().ok()
}

/// A peer that started no later than this helper did.
pub fn there_first(peer_start: u64, own_start: u64) -> bool {
    peer_start <= own_start
}

#[derive(Debug, PartialEq, Eq)]
pub struct Peer {
    pub uid: u32,
    pub cgroup: String,
}

fn start_of(pid: i32) -> Result<u64, String> {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| start_of_stat(&stat))
        .ok_or_else(|| "the peer's start time cannot be read".to_owned())
}

/// The peer on a connected socket: its user and its cgroup, read only when
/// its start time held still and is not later than this helper's.
#[cfg(target_os = "linux")]
pub fn peer_of(socket: impl AsFd) -> Result<Peer, String> {
    let credentials =
        rustix::net::sockopt::socket_peercred(socket).map_err(|e| format!("no peer: {e}"))?;
    let pid = credentials.pid.as_raw_nonzero().get();
    let before = start_of(pid)?;
    let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
        .map_err(|e| format!("the peer's cgroup cannot be read: {e}"))?;
    let after = start_of(pid)?;
    if before != after {
        return Err("the peer changed while it was read".into());
    }
    let own = start_of(i32::try_from(std::process::id()).map_err(|_| "own pid".to_owned())?)?;
    if !there_first(before, own) {
        return Err("the peer started after this helper".into());
    }
    Ok(Peer {
        uid: credentials.uid.as_raw(),
        cgroup,
    })
}

#[cfg(not(target_os = "linux"))]
pub fn peer_of(_socket: impl AsFd) -> Result<Peer, String> {
    Err("peers are read on Linux only".into())
}

/// How long a read of the request may wait for a caller that is silent.
pub const READ_WITHIN: std::time::Duration = std::time::Duration::from_secs(5);

/// A read timeout on a connected socket (the helper's standard input).
#[cfg(target_os = "linux")]
pub fn set_read_timeout(socket: impl AsFd, within: std::time::Duration) -> Result<(), String> {
    rustix::net::sockopt::set_socket_timeout(
        socket,
        rustix::net::sockopt::Timeout::Recv,
        Some(within),
    )
    .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "linux"))]
pub fn set_read_timeout(_socket: impl AsFd, _within: std::time::Duration) -> Result<(), String> {
    Err("not linux".into())
}
