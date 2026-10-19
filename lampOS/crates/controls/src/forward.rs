//! The controls' files as a newer build may have written them (#1680,
//! `docs/updates.md`, "Every part's saved data, one version ahead"):
//! `controls.json` and the pins file. A field this build doesn't know, at
//! any depth, is read past and named once in the journal, by its path and
//! never its value, so a put-back never stops the controls.
//!
//! Still refused, as before:
//! - a file that doesn't parse;
//! - a known field with a bad value (a new `pull`, a mode it doesn't know);
//! - a field this build doesn't know that is named like a key's, by its
//!   name, never its value;
//! - one about privacy (`privacy`, `slide`, `mute` in its name): the privacy
//!   slide is read only as this build knows it, and a section it can't read
//!   is never taken as no slide (Third's design C on #1722). The `privacy`
//!   section itself stays strict;
//! - one about the reset button (`reset` in its name): the reset is how a
//!   person wipes their data, so a renamed or misspelt `reset` stops the
//!   controls rather than leaving the lamp with no reset (the reviewer on
//!   #2076).
//!
//! Each of those is refused whatever its value, `null` and `false` too.
//!
//! What serde says of a file is told only as its category, line and column:
//! its message can hold a value (the reviewer's P3 on #2061).
use serde::{
    Serialize,
    de::{self, DeserializeOwned, Visitor},
};
use serde_json::{Value, error::Category};
use std::{io, path::Path};

/// At most this many unread fields are named; the rest are counted.
const MOST_NAMED: usize = 20;
/// A field's name is told in at most this many characters.
const NAME_CHARS: usize = 64;

/// A field of the file this build doesn't read: its path, its name, and
/// whether it is named in the journal (not when it is `null` or `false`).
struct Unread {
    path: String,
    name: String,
    named: bool,
}

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
    unread_fields(&found, &known, "", top_fields::<T>(), &mut unread);
    for (about, why) in [
        (
            names_a_key as fn(&str) -> bool,
            "is named like a key, and no key is kept in this file",
        ),
        (
            about_privacy,
            "is about privacy, which this build reads only as it knows it, never as no slide",
        ),
        (
            about_the_reset,
            "is about the reset button, which this build reads only as `reset`, never as none",
        ),
    ] {
        if let Some(field) = unread.iter().find(|field| about(&field.name)) {
            return Err(refused(file, &format!("{} {why}", field.path)));
        }
    }
    let named: Vec<&str> = unread
        .iter()
        .filter(|field| field.named)
        .map(|field| field.path.as_str())
        .collect();
    if named.is_empty() {
        return Ok((read, None));
    }
    let more = named.len().saturating_sub(MOST_NAMED);
    let mut shown: Vec<&str> = named.into_iter().take(MOST_NAMED).collect();
    let more = (more > 0).then(|| format!("and {more} more"));
    shown.extend(more.as_deref());
    Ok((
        read,
        Some(format!(
            "{}: fields this build does not read, ignored: {}",
            file.display(),
            shown.join(", ")
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

/// Each field in `found`, the file, this build doesn't read. At the top,
/// one not among `top`, the fields `T` reads (a field this build writes
/// only when set, an `Option` that is none or a `false`, is among them
/// though `known` lacks it). Below, one that `known`, what this build read
/// written again, lacks: those types write every field they read.
fn unread_fields(
    found: &Value,
    known: &Value,
    at: &str,
    top: Option<&[&str]>,
    unread: &mut Vec<Unread>,
) {
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
                let read = top.map_or(known.contains_key(name), |top| top.contains(&name.as_str()));
                match (read, known.get(name)) {
                    (true, Some(known)) => unread_fields(value, known, &path, None, unread),
                    (true, None) => {}
                    (false, _) => unread.push(Unread {
                        path,
                        name: name.clone(),
                        named: !matches!(value, Value::Null | Value::Bool(false)),
                    }),
                }
            }
        }
        (Value::Array(found), Value::Array(known)) => {
            for (index, (value, known)) in found.iter().zip(known).enumerate() {
                unread_fields(value, known, &format!("{at}[{index}]"), None, unread);
            }
        }
        _ => {}
    }
}

/// The fields `T`, a struct serde derives, reads at its top: what its
/// derived code tells `deserialize_struct`. None for anything else.
fn top_fields<T: DeserializeOwned>() -> Option<&'static [&'static str]> {
    let mut fields = None;
    let _ = T::deserialize(Fields(&mut fields));
    fields
}

/// A deserializer that only learns a struct's fields, then stops.
struct Fields<'a>(&'a mut Option<&'static [&'static str]>);

impl<'de> de::Deserializer<'de> for Fields<'_> {
    type Error = de::value::Error;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
        Err(de::Error::custom("not a struct"))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, Self::Error> {
        *self.0 = Some(fields);
        Err(de::Error::custom("its fields are learnt"))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map enum identifier ignored_any
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

/// Whether a field's name is about the reset button.
fn about_the_reset(name: &str) -> bool {
    name.to_ascii_lowercase().contains("reset")
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

    /// A field this build writes only when set, at its default, is a field
    /// it reads: neither named nor refused, `reset` included. An unknown
    /// field that is `null` or `false` is read past unnamed.
    #[test]
    fn a_known_field_at_its_default_is_not_named_nor_refused() {
        let (_, line) = read::<Outer>(
            file(),
            br#"{"chip":"c","pins":[],"reset":null,"legacy":false,"later":null,"also":false}"#,
        )
        .unwrap();
        assert_eq!(line, None);
        assert_eq!(
            top_fields::<Outer>(),
            Some(&["chip", "pins", "reset", "legacy"][..])
        );
        assert_eq!(top_fields::<u32>(), None);
    }

    /// The reviewer on #2076: the name checks see every unknown field,
    /// whatever its value, `null` and `false` included.
    #[test]
    fn a_key_privacy_or_reset_field_is_refused_by_name_whatever_its_value() {
        for (field, said) in [
            ("api_key", "api_key is named like a key"),
            ("token", "token is named like a key"),
            ("privacy_mode", "privacy_mode is about privacy"),
            ("no_slide", "no_slide is about privacy"),
            ("mute_on_slide", "mute_on_slide is about privacy"),
            ("reset_pin", "reset_pin is about the reset button"),
        ] {
            for value in [r#""sk-hidden-value""#, "false", "null", "true", "0"] {
                let bytes = format!(r#"{{"chip":"c","pins":[],"{field}":{value}}}"#);
                let error = read::<Outer>(file(), bytes.as_bytes()).unwrap_err();
                let text = error.to_string();
                assert!(text.contains(said), "{field} {value}: {text}");
                assert!(!text.contains("sk-hidden"), "{text}");
            }
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
