//! Who may ask for a play: only `lamp-control`, known by its cgroup
//! (`lamp-control.service`'s own), read from the socket's credentials
//! (`SO_PEERCRED`'s pid, then `/proc/<pid>/cgroup`), never from anything the
//! caller says. The user is not enough: 22 product units run as `lampos`,
//! among them the ones that start skill and agent code. A peer whose cgroup
//! cannot be read, or whose process changed while it was read, is no peer.

/// The unit that may ask.
pub const ASKER: &str = "lamp-control.service";

/// Whether `/proc/<pid>/cgroup`'s text puts the process in `unit`'s own
/// cgroup (`.../<unit>`, nothing below it, no other unit that merely ends
/// the same way).
pub fn cgroup_is(cgroup: &str, unit: &str) -> bool {
    cgroup.lines().any(|line| {
        let mut fields = line.splitn(3, ':');
        let (Some(_), Some(controllers), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return false;
        };
        (controllers.is_empty() || controllers == "name=systemd")
            && path.rsplit_once('/').is_some_and(|(_, last)| last == unit)
    })
}

/// The start time (field 22, in clock ticks since boot) from a
/// `/proc/<pid>/stat` line; the command's name may hold spaces and brackets,
/// so the fields are counted after its last `)`.
pub fn start_of_stat(stat: &str) -> Option<u64> {
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(19)?.parse().ok()
}

fn start_of(pid: i32) -> Option<u64> {
    start_of_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// Whether the peer of a connected unix socket is in `unit`'s cgroup: its
/// pid from the kernel, its cgroup read between two reads of its start time,
/// which must agree (a pid that went and came back as another process is
/// refused).
pub fn peer_in_unit(socket: impl std::os::fd::AsFd, unit: &str) -> bool {
    let Ok(credentials) = rustix::net::sockopt::socket_peercred(socket) else {
        return false;
    };
    let pid = credentials.pid.as_raw_nonzero().get();
    let Some(before) = start_of(pid) else {
        return false;
    };
    let Ok(cgroup) = std::fs::read_to_string(format!("/proc/{pid}/cgroup")) else {
        return false;
    };
    start_of(pid) == Some(before) && cgroup_is(&cgroup, unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_units_own_cgroup_is_the_unit() {
        let unit = "lamp-control.service";
        for yes in [
            "0::/system.slice/lamp-control.service\n",
            "0::/lampos.slice/lamp-control.service",
            "1:name=systemd:/system.slice/lamp-control.service\n0::/system.slice/lamp-control.service\n",
        ] {
            assert!(cgroup_is(yes, unit), "{yes}");
        }
        for no in [
            "",
            "0::/system.slice/lamp-skills.service\n",
            "0::/system.slice/lamp-tasks.service",
            "0::/system.slice/lamp-addons.service",
            "0::/system.slice/not-lamp-control.service",
            "0::/system.slice/lamp-control.service.evil",
            "0::/system.slice/lamp-control.service/child",
            "0::/lamp-control.service-x",
            "0::/user.slice/user-1000.slice/session-3.scope",
            "0::/lampos-apps.slice/lamp-app-net@radio.service",
            "lamp-control.service",
            "0::lamp-control.service",
            "2:cpu:/system.slice/lamp-control.service",
        ] {
            assert!(!cgroup_is(no, unit), "{no}");
        }
    }

    #[test]
    fn the_start_time_is_counted_after_the_commands_last_bracket() {
        let stat = "1234 (my (odd) name) S 1 1234 1234 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 3 0 987654 1000 100 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 1 0 0 0 0 0";
        assert_eq!(start_of_stat(stat), Some(987654));
        assert_eq!(start_of_stat("garbage"), None);
        assert_eq!(start_of_stat("1 (x) S"), None);
    }

    #[test]
    fn a_peer_that_is_this_process_is_in_its_own_cgroup_and_in_no_other_unit() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        // This test process is not lamp-control.service, whatever the unit.
        assert!(!peer_in_unit(&a, ASKER));
        // And the pair's peer is this process: its own cgroup is readable.
        let own = std::fs::read_to_string("/proc/self/cgroup").unwrap();
        let path = own
            .lines()
            .next()
            .and_then(|line| line.splitn(3, ':').nth(2))
            .unwrap_or("");
        if let Some((_, unit)) = path.rsplit_once('/')
            && !unit.is_empty()
        {
            assert!(peer_in_unit(&a, unit), "{path}");
        }
    }
}
