//! Who is asking, from the kernel, never from the caller's word: the lamp
//! programming protocol's phase 0 (Nine's design on #3504, 27 Oct, §6).
//! Today any process in group `lampos` may claim any sender and priority on
//! the parts' sockets; a part that reads its caller here can name the unit
//! that asked and hold it to that unit's own priority.
//!
//! - A stream socket's peer is read once with `SO_PEERCRED`
//!   ([`of_stream`]): `say.sock`, the actions part's `call.sock`.
//! - A datagram socket is told to pass credentials ([`pass_credentials`],
//!   `SO_PASSCRED`) and each datagram is received with its sender's
//!   ([`recv_with_caller`], `SCM_CREDENTIALS`): `mood.sock`, `gaze.sock`,
//!   and a bus subscriber, since the bus is a socket per subscriber with no
//!   process in the middle (`docs/bus.md`).
//! - The unit is the system service the pid's cgroup names ([`unit_of`]):
//!   only one under `/system.slice/` or `/lampos.slice/`, as `lamp-skills`'
//!   manage helper anchors its paths (`crates/skills/src/package.rs`), so a
//!   login user's own `systemd-run --user --unit=lamp-voice` names none. A
//!   part holding a unit to its priority checks the uid too: a known name
//!   with another unit's uid is unknown.
//! - A sender may attach credentials of its own, but the kernel refuses any
//!   pid but its own unless it is privileged. Root may attach any pid; root
//!   is trusted anyway.
//!
//! Best effort: the unit is read from `/proc/<pid>/cgroup` when the message
//! is received, not when it was sent, so a sender that has already exited
//! names no unit, and in the rare case its pid was reused, another's. A
//! stream's credentials are its connector's, even if the socket was since
//! passed on. `SO_PEERPIDFD` / `SCM_PIDFD` would close both, if needed. A
//! sender in a pid namespace the receiver can't see would come as pid 0; no
//! lamp unit runs in one.
//!
//! Linux only: elsewhere no caller is known ([`Caller`] is `None`), and a
//! part treats an unknown caller as the least it trusts.
use std::io;
use std::os::unix::net::{UnixDatagram, UnixStream};

/// A caller as the kernel names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    pub pid: u32,
    pub uid: u32,
    /// The system service its cgroup names (`lamp-skills.service`), if its
    /// pid could still be read and it is in one ([`unit_of`]).
    pub unit: Option<String>,
}

/// The system service named by `/proc/<pid>/cgroup`'s text, on the unified
/// hierarchy's line (`0::…`) or systemd's own on a hybrid one
/// (`…:name=systemd:…`): the first part after `/system.slice/` or
/// `/lampos.slice/` and its sub-slices, if it ends in `.service`. A process
/// in a service's own sub-cgroup is that service's, whatever the sub-cgroup
/// is named; anything under `user.slice` or another slice names none.
pub fn unit_of(cgroup: &str) -> Option<String> {
    cgroup.lines().find_map(|line| {
        let mut fields = line.splitn(3, ':');
        let (Some(_), Some(controllers), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return None;
        };
        if !(controllers.is_empty() || controllers == "name=systemd") {
            return None;
        }
        let mut parts = path.split('/');
        if parts.next() != Some("")
            || !matches!(parts.next(), Some("system.slice" | "lampos.slice"))
        {
            return None;
        }
        parts
            .find(|part| !part.ends_with(".slice"))
            .filter(|part| part.ends_with(".service") && part.len() > ".service".len())
            .map(str::to_owned)
    })
}

/// The uid of the user `name` in `passwd`'s text (`/etc/passwd`'s form),
/// for a part that holds a unit to the user it runs as (`lampos`).
pub fn uid_in_passwd(passwd: &str, name: &str) -> Option<u32> {
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next() == Some(name))
            .then(|| fields.nth(1)?.parse().ok())
            .flatten()
    })
}

/// [`uid_in_passwd`] of `/etc/passwd`, read now; `None` if it can't be.
pub fn uid_named(name: &str) -> Option<u32> {
    uid_in_passwd(&std::fs::read_to_string("/etc/passwd").ok()?, name)
}

/// The caller `pid` and `uid` name, its unit read now.
#[cfg(target_os = "linux")]
fn named(pid: u32, uid: u32) -> Caller {
    let unit = std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
        .ok()
        .and_then(|text| unit_of(&text));
    Caller { pid, uid, unit }
}

/// The peer of a connected stream socket, by `SO_PEERCRED`.
pub fn of_stream(stream: &UnixStream) -> Option<Caller> {
    #[cfg(target_os = "linux")]
    {
        let peer = rustix::net::sockopt::socket_peercred(stream).ok()?;
        Some(named(
            peer.pid.as_raw_nonzero().get() as u32,
            peer.uid.as_raw(),
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = stream;
        None
    }
}

/// Have the kernel attach each sender's credentials to the datagrams
/// `socket` receives (`SO_PASSCRED`), for [`recv_with_caller`].
pub fn pass_credentials(socket: &UnixDatagram) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        rustix::net::sockopt::set_socket_passcred(socket, true)?;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = socket;
    Ok(())
}

/// Receive one datagram into `buffer`: its length, and its sender as the
/// kernel named it, if `socket` passes credentials ([`pass_credentials`]).
/// A datagram longer than `buffer` is consumed and refused
/// (`InvalidData`), never handed on half read; one whose credentials were
/// cut off comes with no caller.
pub fn recv_with_caller(
    socket: &UnixDatagram,
    buffer: &mut [u8],
) -> io::Result<(usize, Option<Caller>)> {
    #[cfg(target_os = "linux")]
    {
        use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags};
        let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmCredentials(1))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        use rustix::net::ReturnFlags;
        let received = rustix::net::recvmsg(
            socket,
            &mut [io::IoSliceMut::new(buffer)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC,
        )?;
        if received.flags.contains(ReturnFlags::TRUNC) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("a datagram longer than {} bytes", buffer.len()),
            ));
        }
        if received.flags.contains(ReturnFlags::CTRUNC) {
            return Ok((received.bytes, None));
        }
        let caller = control.drain().find_map(|message| match message {
            RecvAncillaryMessage::ScmCredentials(sender) => Some(named(
                sender.pid.as_raw_nonzero().get() as u32,
                sender.uid.as_raw(),
            )),
            _ => None,
        });
        Ok((received.bytes, caller))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok((socket.recv(buffer)?, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The unit from either hierarchy's line, a service's own sub-cgroup
    /// included; none outside a service.
    #[test]
    fn the_unit_is_the_service_the_cgroup_names() {
        assert_eq!(
            unit_of("0::/lampos.slice/lampos-parts.slice/lamp-skills.service\n").as_deref(),
            Some("lamp-skills.service")
        );
        assert_eq!(
            unit_of("12:cpu:/x\n1:name=systemd:/system.slice/lamp-voice.service\n").as_deref(),
            Some("lamp-voice.service")
        );
        assert_eq!(
            unit_of("0::/system.slice/lamp-voice.service/worker\n").as_deref(),
            Some("lamp-voice.service"),
            "a service's own sub-cgroup is the service's"
        );
        assert_eq!(
            unit_of("0::/user.slice/user-1000.slice/session-3.scope\n"),
            None
        );
        assert_eq!(
            unit_of("12:cpu:/system.slice/lamp-voice.service\n"),
            None,
            "not systemd's line"
        );
        assert_eq!(unit_of(""), None);
    }

    /// Only a system service names a unit: a login user's own
    /// `systemd-run --user --unit=lamp-voice`, or a `*.service` sub-cgroup a
    /// delegated unit made under itself, is not the part it is named after.
    #[test]
    fn a_user_or_delegated_unit_is_not_a_lamp_part() {
        assert_eq!(
            unit_of(
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/lamp-voice.service\n"
            ),
            None,
            "a login user's own unit"
        );
        assert_eq!(
            unit_of(
                "1:name=systemd:/user.slice/user-1000.slice/user@1000.service/lamp-cloud.service\n"
            ),
            None
        );
        assert_eq!(
            unit_of("0::/system.slice/lamp-skills.service/x.service\n").as_deref(),
            Some("lamp-skills.service"),
            "the first service, not one made under it"
        );
        assert_eq!(
            unit_of("0::/system.slice/docker.service/lamp-voice.service\n").as_deref(),
            Some("docker.service")
        );
        assert_eq!(
            unit_of("0::/lampos.slice/lampos-parts.slice/lamp-cloud.service/a.slice/b.service\n")
                .as_deref(),
            Some("lamp-cloud.service")
        );
        assert_eq!(
            unit_of("0::/machine.slice/lamp-voice.service\n"),
            None,
            "only the system and lampos slices"
        );
        assert_eq!(unit_of("0::/lampos.slice/\n"), None);
        assert_eq!(unit_of("0::/system.slice/.service\n"), None);
    }

    #[test]
    fn a_users_uid_is_read_from_passwd() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\nlampos:x:990:990::/nonexistent:/usr/sbin/nologin\nlamposx:x:7:7::/:/bin/false\nbad:x:nan:1::/:/bin/false\n";
        assert_eq!(uid_in_passwd(passwd, "lampos"), Some(990));
        assert_eq!(uid_in_passwd(passwd, "root"), Some(0));
        assert_eq!(uid_in_passwd(passwd, "lamp"), None);
        assert_eq!(uid_in_passwd(passwd, "bad"), None);
        assert_eq!(uid_in_passwd("", "lampos"), None);
    }

    #[cfg(target_os = "linux")]
    fn me() -> (u32, u32) {
        (std::process::id(), rustix::process::getuid().as_raw())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_stream_peer_is_read_from_the_kernel() {
        let (one, other) = UnixStream::pair().unwrap();
        let caller = of_stream(&one).expect("a connected peer");
        assert_eq!((caller.pid, caller.uid), me());
        let mine = std::fs::read_to_string("/proc/self/cgroup")
            .ok()
            .and_then(|text| unit_of(&text));
        assert_eq!(caller.unit, mine);
        drop(other);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_datagram_comes_with_its_sender_only_when_credentials_are_passed() {
        let directory =
            std::env::temp_dir().join(format!("lamp-ipc-caller-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("in.sock");
        let receiver = UnixDatagram::bind(&path).unwrap();
        let sender = UnixDatagram::unbound().unwrap();
        let mut buffer = [0u8; 64];
        // Without SO_PASSCRED the kernel names nobody.
        sender.send_to(b"plain", &path).unwrap();
        let (length, caller) = recv_with_caller(&receiver, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"plain");
        assert_eq!(caller, None);
        // With it, every datagram says who sent it.
        pass_credentials(&receiver).unwrap();
        sender.send_to(b"{\"gesture\":\"nod\"}", &path).unwrap();
        let (length, caller) = recv_with_caller(&receiver, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"{\"gesture\":\"nod\"}");
        let caller = caller.expect("the sender, from the kernel");
        assert_eq!((caller.pid, caller.uid), me());
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A datagram longer than the buffer is refused, not half read.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_datagram_too_long_for_the_buffer_is_refused() {
        let (one, other) = UnixDatagram::pair().unwrap();
        pass_credentials(&other).unwrap();
        one.send(b"{\"gesture\":\"nod\"}").unwrap();
        let mut buffer = [0u8; 4];
        let refused = recv_with_caller(&other, &mut buffer).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::InvalidData);
        // The next one, short enough, reads whole.
        one.send(b"ok").unwrap();
        let (length, caller) = recv_with_caller(&other, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"ok");
        assert!(caller.is_some());
    }

    /// A sender can't claim another pid: the kernel refuses credentials
    /// not its own to an unprivileged sender.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_sender_cannot_claim_another_pid() {
        use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, UCred};
        if rustix::process::geteuid().is_root() {
            return; // root may send any pid: nothing to show.
        }
        let (one, other) = UnixDatagram::pair().unwrap();
        pass_credentials(&other).unwrap();
        let forged = UCred {
            pid: rustix::process::Pid::from_raw(1).unwrap(),
            uid: rustix::process::getuid(),
            gid: rustix::process::getgid(),
        };
        let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmCredentials(1))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        assert!(control.push(SendAncillaryMessage::ScmCredentials(forged)));
        let sent = rustix::net::sendmsg(
            &one,
            &[std::io::IoSlice::new(b"x")],
            &mut control,
            SendFlags::empty(),
        );
        assert!(sent.is_err(), "the kernel let a forged pid through");
    }
}
