//! An app's name, and what is named after it. The name becomes a unit
//! instance and a dynamic user (`lampapp-<name>`, at most 32), so it is held
//! to a small alphabet before it reaches systemd or a path.

/// The most characters in an app's name (nine's FORMAT 2 spec): with
/// `lampapp-` in front, 32, the longest a user name may be.
pub const MOST_NAME: usize = 24;

/// `[a-z0-9-]`, 1 to [`MOST_NAME`] characters, starting with a letter.
pub fn app_name_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MOST_NAME
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// The dynamic user's name, `lampapp-<name>`.
pub fn user_name(name: &str) -> String {
    format!("lampapp-{name}")
}

/// The instance of the app's own unit (no `net`).
pub fn plain_unit(name: &str) -> String {
    format!("lampapp@{name}.service")
}

/// The instance of the unit with a way out, for a manifest with `"net": true`.
pub fn net_unit(name: &str) -> String {
    format!("lampapp-net@{name}.service")
}
