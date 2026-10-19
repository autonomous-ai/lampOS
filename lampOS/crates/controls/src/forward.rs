//! The controls' files as a newer build may have written them (#1680,
//! `docs/updates.md`, "Every part's saved data, one version ahead"):
//! `controls.json` and the pins file. A field this build doesn't know, at
//! any depth, is read past and named once in the journal, by its path and
//! never its value, so a put-back never stops the controls.
//!
//! Still refused, as before:
//! - a file that doesn't parse;
//! - a known field with a bad value (a new `pull`, a mode it doesn't know);
//! - a field named like a key's, by its name, never its value;
//! - a field about privacy (`privacy`, `slide`, `mute` in its name) this
//!   build doesn't know: the privacy slide is read only as this build knows
//!   it, and a section it can't read is never taken as no slide (Third's
//!   design C on #1722). The `privacy` section itself stays strict.
//!
//! What serde says of a file is told only as its category, line and column:
//! its message can hold a value (the reviewer's P3 on #2061).
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, error::Category};
use std::{io, path::Path};

/// At most this many unread fields are named; the rest are counted.
const MOST_NAMED: usize = 20;
/// A field's name is told in at most this many characters.
const NAME_CHARS: usize = 64;

/// `bytes`, the file at `file`, read as `T`: the value, and the journal's
/// line naming the fields this build does not read, if there are any.
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
        return Err(refused(
            file,
            &format!("{path} is named like a key, and no key is kept in this file"),
        ));
    }
    if let Some((path, _)) = unread.iter().find(|(_, name)| about_privacy(name)) {
        return Err(refused(
            file,
            &format!(
                "{path} is about privacy, which this build reads only as it knows it, never as no slide"
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

/// A file serde could not read as its configuration: which file, and only
/// serde's category, line and column.
pub fn unreadable(file: &Path, error: &serde_json::Error) -> io::Error {
    let category = match error.classify() {
        Category::Io => "io",
        Category::Syntax => "syntax",
        Category::Data => "data",
        Category::Eof => "eof",
    };
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "{}: not read ({category} error at line {}, column {})",
            file.display(),
            error.line(),
            error.column()
        ),
    )
}

fn refused(file: &Path, why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{}: {why}: not started", file.display()),
    )
}

/// Each field in `found`, the file, that `known`, what this build read of
/// it written again, does not have: its path and its name. A field this
/// build writes only when set (an `Option` that is none, a `false`) is not
/// in `known`, so one that is `null` or `false` in the file is not named:
/// an unknown field with such a value is read past unnamed.
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
                    None if matches!(value, Value::Null | Value::Bool(false)) => {}
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

/// Whether a field's name is a key's: no file of the controls holds one.
fn names_a_key(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["key", "token", "secret", "password"]
        .iter()
        .any(|word| name == *word || name.ends_with(&format!("_{word}")))
}

/// Whether a field's name is about the privacy slide.
fn about_privacy(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["privacy", "slide", "mute"]
        .iter()
        .any(|word| name.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct Inner {
        line: u32,
    }

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct Outer {
        chip: String,
        pins: Vec<Inner>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reset: Option<Inner>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        legacy: bool,
    }

    fn file() -> &'static Path {
        Path::new("/etc/lampOS/controls/test.json")
    }

    #[test]
    fn unknown_fields_at_any_depth_are_read_past_and_named_by_path() {
        let (read, line) = read::<Outer>(
            file(),
            br#"{"chip":"c","pins":[{"line":1},{"line":2,"debounce_ms":30}],"reset":{"line":3,"held":true},"sway":{"far":1}}"#,
        )
        .unwrap();
        assert_eq!(read.pins[1], Inner { line: 2 });
        assert_eq!(
            line.as_deref(),
            Some(
                "/etc/lampOS/controls/test.json: fields this build does not read, ignored: pins[1].debounce_ms, reset.held, sway"
            )
        );
    }

    #[test]
    fn a_known_field_at_its_default_is_not_named() {
        let (_, line) = read::<Outer>(
            file(),
            br#"{"chip":"c","pins":[],"reset":null,"legacy":false}"#,
        )
        .unwrap();
        assert_eq!(line, None);
    }

    #[test]
    fn a_key_or_privacy_field_is_refused_by_name_never_its_value() {
        for (field, said) in [
            ("api_key", "api_key is named like a key"),
            ("token", "token is named like a key"),
            ("privacy_mode", "privacy_mode is about privacy"),
            ("no_slide", "no_slide is about privacy"),
        ] {
            let bytes = format!(r#"{{"chip":"c","pins":[],"{field}":"sk-hidden-value"}}"#);
            let error = read::<Outer>(file(), bytes.as_bytes()).unwrap_err();
            let text = error.to_string();
            assert!(text.contains(said), "{text}");
            assert!(!text.contains("sk-hidden"), "{text}");
        }
    }

    #[test]
    fn a_bad_value_or_a_file_that_does_not_parse_is_refused_with_no_value_shown() {
        for (bytes, category) in [
            (
                r#"{"chip":"c","pins":[{"line":"sk-hidden-value"}]}"#,
                "data",
            ),
            (r#"{"chip":"c","pins":[{"line":1}]"#, "eof"),
            (r#"{"chip":sk-hidden-value}"#, "syntax"),
        ] {
            let text = read::<Outer>(file(), bytes.as_bytes())
                .unwrap_err()
                .to_string();
            assert!(
                text.contains(&format!("not read ({category} error at line 1, column")),
                "{text}"
            );
            assert!(!text.contains("sk-hidden"), "{text}");
        }
    }

    #[test]
    fn many_unread_fields_are_counted_past_twenty() {
        let fields: String = (0..25).map(|n| format!(r#","f{n}":1"#)).collect();
        let bytes = format!(r#"{{"chip":"c","pins":[]{fields}}}"#);
        let (_, line) = read::<Outer>(file(), bytes.as_bytes()).unwrap();
        let line = line.unwrap();
        assert!(line.ends_with(", and 5 more"), "{line}");
    }
}
