//! The local agent's socket (docs/control.md, "The local agent"): the owner
//! turns on one named user, and a program of that user, on the lamp itself,
//! reaches the door with no pairing, as a computer with the grants the owner
//! left it. This module reads the user and the group from `passwd` and
//! `group` and makes the socket; `server::serve_agent` is what accepts.
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

/// Where it listens unless `--agent-socket` says another place: a folder of
/// its own (`RuntimeDirectory=lampos-agent`, 0755), since lamp-control's own
/// runtime folder is 0700 and no other user could walk into it.
pub const SOCKET: &str = "/run/lampos-agent/agent.sock";
/// The group of the socket unless `--agent-group` says another: lamp-control's
/// user and the agent's are in it, so lamp-control may give the socket to it;
/// the group is not the guard, the peer's uid is.
pub const GROUP: &str = "lamp-agents";

/// The uid of `name` in the text of a `passwd` file (`name:x:uid:gid:...`).
pub fn uid_of(name: &str, passwd: &str) -> Option<u32> {
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next()? == name).then_some(())?;
        fields.nth(1)?.parse().ok()
    })
}

/// The gid of `name` in the text of a `group` file (`name:x:gid:members`).
pub fn gid_of(name: &str, group: &str) -> Option<u32> {
    group.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next()? == name).then_some(())?;
        fields.nth(1)?.parse().ok()
    })
}

/// A name that can be a user: letters, digits, `_`, `-`, `.`, no more than 32
/// (so it cannot carry a path or a `:` into a record id or a journal line).
pub fn user_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// Binds the socket at `path`, group `gid`, mode 0660, in a folder of mode
/// 0755 that is made if there is none. Never a socket anyone else made: an
/// old one of ours is removed first.
pub fn bind(path: &Path, gid: u32) -> std::io::Result<tokio::net::UnixListener> {
    if let Some(parent) = path.parent() {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(parent)?;
    }
    // Only our own old socket is removed (a failed removal is not fatal: the
    // bind then fails with the reason, and no socket is served).
    let _ = std::fs::remove_file(path);
    let listener = tokio::net::UnixListener::bind(path)?;
    // The mode and the group are set right after the bind, and nothing serves
    // the socket before they are: the file is made with the process's umask
    // (not necessarily 0600) for those few instructions, which the folder's
    // own mode and the peer-uid check cover.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))?;
    std::os::unix::fs::chown(path, None, Some(gid))?;
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\nlampos:x:994:994::/var/lib/lampos:/usr/sbin/nologin\nagent:x:1001:1001::/home/agent:/bin/bash\n";
    const GROUPS: &str = "root:x:0:\nlampos:x:994:\nlamp-agents:x:995:lampos,agent\n";

    /// A stale socket file after the owner's switch is turned off (the lead's run on lamp-75c9, 13 Nov): lamp-control cannot
    /// remove it at its next start (`ProtectSystem=strict`; the folder is writable only through the switch's drop-in, which
    /// the off removes), so the unit's stop does, as root, for the socket's own path and nothing else.
    #[test]
    fn the_units_stop_removes_the_socket_file_the_switch_made() {
        let unit = include_str!("../../../deploy/product/lamp-control.service");
        assert!(
            unit.contains(&format!("\nExecStopPost=-+/usr/bin/rm -f {SOCKET}\n")),
            "the unit's stop removes {SOCKET}"
        );
        // The folder is not in the unit's own writable paths: that is why lamp-control cannot do it itself.
        let folder = std::path::Path::new(SOCKET)
            .parent()
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            !unit
                .lines()
                .any(|line| line.starts_with("ReadWritePaths=") && line.contains(folder)),
            "{folder} is writable only through the switch's drop-in"
        );
        // Only the one file: no glob, no folder.
        let stops: Vec<_> = unit
            .lines()
            .filter(|l| l.starts_with("ExecStopPost="))
            .collect();
        assert_eq!(stops.len(), 1, "{stops:?}");
        assert!(
            !stops[0].contains('*') && !stops[0].contains(" -r"),
            "{stops:?}"
        );
    }

    #[test]
    fn a_user_and_a_group_are_read_by_their_exact_name() {
        assert_eq!(uid_of("agent", PASSWD), Some(1001));
        assert_eq!(uid_of("lampos", PASSWD), Some(994));
        assert_eq!(uid_of("agen", PASSWD), None);
        assert_eq!(uid_of("nobody", PASSWD), None);
        assert_eq!(gid_of("lamp-agents", GROUPS), Some(995));
        assert_eq!(gid_of("lamp", GROUPS), None);
    }

    #[test]
    fn only_a_plain_name_can_be_a_user() {
        assert!(user_name_ok("agent"));
        assert!(user_name_ok("lamp-agent.1"));
        assert!(!user_name_ok(""));
        assert!(!user_name_ok("a:b"));
        assert!(!user_name_ok("../x"));
        assert!(!user_name_ok(&"a".repeat(33)));
    }
}
