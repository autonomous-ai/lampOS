//! The owner picked by face in the app (#5495, `docs/cloud.md`, "The owner
//! by face"). One copy of its rules, read by the eyes, which mint each
//! saved face's id, by `lamp-cloud`, which checks the app's pick, by root's
//! helper (`lamp-setup change`), which alone writes the pick's file, and by
//! the voice, which reads it to know who the owner is.
//!
//! A face id is `f-` and 16 lower-case hex digits, random, minted when a
//! face is first saved and never given again: a face removed and saved
//! later under the same name gets another, so a pick can never come back
//! to a new face by its name.
//!
//! The pick's file holds one JSON line, `{"face": "f-…", "set_at": s}`, or
//! `{"face": null, "set_at": s}` once a pick was cleared: a pick made and
//! cleared leaves the lamp with no owner until the app picks again, never
//! back on the typed name. Only a reset removes the file. Read fail-closed:
//! a file that is there but can't be read is a pick of nobody.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The switch, in every unit that reads it: `on` exactly, else off and the
/// lamp as before (the owner is the typed name).
pub const ENVIRONMENT: &str = "LAMP_OWNER_FACE";

/// Where the pick is kept: beside the person's name, root's, 0600.
pub const FILE: &str = "/etc/lampOS/person/owner-face";

/// The most bytes read of the file: a pick is well under it.
const FILE_MOST: u64 = 1_024;

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).as_deref() == Ok("on")
}

/// The pick's file beside the person's name file `name` (setup's
/// configuration names that one).
pub fn beside_name(name: &Path) -> PathBuf {
    name.with_file_name("owner-face")
}

/// Whether `id` is a face id: `f-` and exactly 16 lower-case hex digits.
pub fn is_face_id(id: &str) -> bool {
    id.len() == 18
        && id.starts_with("f-")
        && id[2..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A face id from 8 random bytes.
pub fn face_id(random: [u8; 8]) -> String {
    let mut id = String::from("f-");
    for byte in random {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}

/// What the file holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kept {
    /// The picked face, or `None` once a pick was cleared.
    pub face: Option<String>,
    /// When it was set or cleared, in Unix seconds.
    pub set_at: u64,
}

/// The file's one line for `face` set or cleared at `set_at`.
pub fn text(face: Option<&str>, set_at: u64) -> String {
    let kept = Kept {
        face: face.map(str::to_owned),
        set_at,
    };
    format!(
        "{}\n",
        serde_json::to_string(&kept).unwrap_or_else(|_| "{\"face\":null,\"set_at\":0}".into())
    )
}

/// The pick as the lamp reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// No face was ever picked: the owner is the typed name, as before.
    Never,
    /// This face is the owner, and nobody else, whatever names say.
    Face(String),
    /// A pick was made and cleared, or the file can't be read: no owner.
    Nobody,
}

impl Pick {
    /// The picked face, if any.
    pub fn face(&self) -> Option<&str> {
        match self {
            Self::Face(face) => Some(face),
            _ => None,
        }
    }

    /// Whether a pick was ever made: the typed name then decides nothing.
    pub fn made(&self) -> bool {
        !matches!(self, Self::Never)
    }
}

/// The pick in `text`, the file's bytes: [`Pick::Nobody`] for anything
/// that isn't one line of [`Kept`] with a face id or `null`.
pub fn pick_of(text: &[u8]) -> Pick {
    let Ok(text) = std::str::from_utf8(text) else {
        return Pick::Nobody;
    };
    match serde_json::from_str::<Kept>(text.trim()) {
        Ok(Kept {
            face: Some(face), ..
        }) if is_face_id(&face) => Pick::Face(face),
        _ => Pick::Nobody,
    }
}

/// The pick kept at `path`: [`Pick::Never`] only when there is no file.
/// Anything else that stops a read (a file too large, not text, not
/// readable) is [`Pick::Nobody`], so a broken pick never hands the owner
/// back to a typed name.
pub fn read(path: &Path) -> Pick {
    use std::io::Read;
    match std::fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            match file.take(FILE_MOST + 1).read_to_end(&mut bytes) {
                Ok(_) if bytes.len() as u64 <= FILE_MOST => pick_of(&bytes),
                _ => Pick::Nobody,
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Pick::Never,
        Err(_) => Pick::Nobody,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_face_id_is_f_and_sixteen_lower_case_hex_digits() {
        assert!(is_face_id("f-3c9a51d07be24e18"));
        assert_eq!(
            face_id([0x3c, 0x9a, 0x51, 0xd0, 0x7b, 0xe2, 0x4e, 0x18]),
            "f-3c9a51d07be24e18"
        );
        for wrong in [
            "",
            "f-",
            "f-3C9A51D07BE24E18",
            "f-3c9a51d07be24e1",
            "f-3c9a51d07be24e180",
            "p-3c9a51d07be24e18",
            "f-3c9a51d07be24e1g",
            "f_3c9a51d07be24e18",
        ] {
            assert!(!is_face_id(wrong), "{wrong}");
        }
    }

    #[test]
    fn the_file_is_read_fail_closed() {
        assert_eq!(
            pick_of(text(Some("f-3c9a51d07be24e18"), 1).as_bytes()),
            Pick::Face("f-3c9a51d07be24e18".into())
        );
        assert_eq!(pick_of(text(None, 1).as_bytes()), Pick::Nobody);
        for broken in [
            &b""[..],
            b"f-3c9a51d07be24e18",
            br#"{"face":"Lan","set_at":1}"#,
            br#"{"face":"f-3c9a51d07be24e18"}"#,
            br#"{"face":"f-3c9a51d07be24e18","set_at":1,"name":"Lan"}"#,
            b"\xff\xfe",
        ] {
            assert_eq!(pick_of(broken), Pick::Nobody);
        }
        let dir = std::env::temp_dir().join(format!("owner-face-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = beside_name(&dir.join("name"));
        assert_eq!(file, dir.join("owner-face"));
        assert_eq!(read(&file), Pick::Never);
        assert!(!read(&file).made());
        std::fs::write(&file, text(Some("f-0000000000000001"), 5)).unwrap();
        assert_eq!(read(&file).face(), Some("f-0000000000000001"));
        std::fs::write(&file, vec![b' '; 2_000]).unwrap();
        assert_eq!(read(&file), Pick::Nobody);
        assert!(read(&file).made());
        let _ = std::fs::remove_dir_all(dir);
    }
}
