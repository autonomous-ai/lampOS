//! The local agent (`docs/control.md`, "The local agent"): the one user, the
//! one group and the one socket folder the owner's switch turns on. Names are
//! constants, never taken from the app, so no request can name another user
//! into a unit file.

/// The unprivileged user the lamp's coding agents run as; never `lampos`.
pub const USER: &str = "lampagent";
/// The group that lets lamp-control give its socket to that user; the peer's
/// uid, not this group, is the guard.
pub const GROUP: &str = "lamp-agents";
/// The folder the socket is made in (mode 0755, made by the installer's
/// tmpfiles line: lamp-control's own runtime folder is 0700).
pub const SOCKET_DIR: &str = "/run/lampos-agent";
/// The socket.
pub const SOCKET: &str = "/run/lampos-agent/agent.sock";

/// What lamp-control's drop-in holds while the switch is on, exactly: the
/// user it serves, the group it may give the socket to, and the folder the
/// unit's sandbox lets it write.
pub const DROPIN_TEXT: &str = "[Service]\nEnvironment=LAMP_CONTROL_LOCAL_AGENT=lampagent\nSupplementaryGroups=lamp-agents\nReadWritePaths=/run/lampos-agent\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_drop_in_names_the_constants_and_nothing_else() {
        assert_eq!(
            DROPIN_TEXT,
            format!(
                "[Service]\nEnvironment=LAMP_CONTROL_LOCAL_AGENT={USER}\nSupplementaryGroups={GROUP}\nReadWritePaths={SOCKET_DIR}\n"
            )
        );
        assert_eq!(SOCKET, format!("{SOCKET_DIR}/agent.sock"));
        assert_ne!(USER, "lampos");
    }
}
