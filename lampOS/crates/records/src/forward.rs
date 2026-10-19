//! `records.json` as a newer build may have written it (#1680,
//! `docs/updates.md`, "Every part's saved data, one version ahead"). A field
//! this build doesn't know, at any depth, is read past and named once in the
//! journal, by its path, never its value, so a put-back never fails the
//! cleanup. A file that doesn't parse, a known field with a bad value (a
//! root `kind` this build doesn't know) and a field named like a key's are
//! still refused. What serde says is told only as its category, line and
//! column: its message can hold a value (the reviewer's P3 on #2061).
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, error::Category};
use std::{io, path::Path};

/// At most this many unread fields are named; the rest are counted.
const MOST_NAMED: usize = 20;
/// A field's name is told in at most this many characters.
const NAME_CHARS: usize = 64;

/// `bytes`, the file at `file`, read as `T`: the value, and the journal's
/// line naming the fields this build does not read, if any. Only for a type
/// that writes every field it reads: one written only when set would be
/// named when the file has it.
pub fn read<T: DeserializeOwned + Serialize>(
    file: &Path,
    bytes: &[u8],
) -> io::Result<(T, Option<String>)> {
    let found: Value = serde_json::from_slice(bytes).map_err(|error| unreadable(file, &error))?;
    let read: T = serde_json::from_slice(bytes).map_err(|error| unreadable(file, &error))?;
    let known = serde_json::to_value(&read).map_err(|error| unreadable(file, &error))?;
    let mut unread = Vec::new();
    unread_fields(&found, &known, "", &mut unread);
    if let Some((path, _)) = unread.iter().find(|(_, name)| names_a_key(name)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{}: {path} is named like a key, and no key is kept in this file",
                file.display()
            ),
        ));
    }
    if unread.is_empty() {
        return Ok((read, None));
    }
    let mut named: Vec<&str> = unread
        .iter()
        .take(MOST_NAMED)
        .map(|(path, _)| path.as_str())
        .collect();
    let more = unread.len().saturating_sub(MOST_NAMED);
    let more = (more > 0).then(|| format!("and {more} more"));
    named.extend(more.as_deref());
    Ok((
        read,
        Some(format!(
            "{}: fields this build does not read, ignored: {}",
            file.display(),
            named.join(", ")
        )),
    ))
}

/// A file serde could not read: which file, and only serde's category,
/// line and column.
pub fn unreadable(file: &Path, error: &serde_json::Error) -> io::Error {
    let category = match error.classify() {
        Category::Io => "io",
        Category::Syntax => "syntax",
        Category::Data => "data",
        Category::Eof => "eof",
    };
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "{}: not read ({category} error at line {}, column {})",
            file.display(),
            error.line(),
            error.column()
        ),
    )
}

/// Each field in `found`, the file, that `known`, what this build read of
/// it written again, does not have: its path and its name.
fn unread_fields(found: &Value, known: &Value, at: &str, unread: &mut Vec<(String, String)>) {
    match (found, known) {
        (Value::Object(found), Value::Object(known)) => {
            for (name, value) in found {
                let shown: String = name
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(NAME_CHARS)
                    .collect();
                let path = if at.is_empty() {
                    shown
                } else {
                    format!("{at}.{shown}")
                };
                match known.get(name) {
                    Some(known) => unread_fields(value, known, &path, unread),
                    None => unread.push((path, name.clone())),
                }
            }
        }
        (Value::Array(found), Value::Array(known)) => {
            for (index, (value, known)) in found.iter().zip(known).enumerate() {
                unread_fields(value, known, &format!("{at}[{index}]"), unread);
            }
        }
        _ => {}
    }
}

/// Whether a field's name is a key's: `records.json` never holds one.
fn names_a_key(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["key", "token", "secret", "password"]
        .iter()
        .any(|word| name == *word || name.ends_with(&format!("_{word}")))
}
