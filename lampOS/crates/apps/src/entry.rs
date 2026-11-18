//! The entry module: the only text from a manifest that reaches systemd,
//! as the one line of `entry.env` the static unit reads.

/// The most characters in an entry.
pub const MOST_ENTRY: usize = 128;
/// The most parts of a dotted path.
pub const MOST_PARTS: usize = 8;

/// A dotted module path: each part a lowercase identifier (`[a-z_][a-z0-9_]*`),
/// nothing empty, no `..`, no slash, no space.
pub fn entry_ok(entry: &str) -> bool {
    if entry.is_empty() || entry.len() > MOST_ENTRY {
        return false;
    }
    let parts: Vec<&str> = entry.split('.').collect();
    parts.len() <= MOST_PARTS
        && parts.iter().all(|part| {
            let bytes = part.as_bytes();
            !bytes.is_empty()
                && (bytes[0].is_ascii_lowercase() || bytes[0] == b'_')
                && bytes
                    .iter()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
        })
}

/// What `entry.env` holds: exactly `ENTRY=<entry>` and a newline.
pub fn entry_env(entry: &str) -> Result<String, String> {
    if !entry_ok(entry) {
        return Err(format!("not an entry: {entry:?}"));
    }
    Ok(format!("ENTRY={entry}\n"))
}
