//! The two yeses another part reads (#5500, `docs/onboarding.md`, "Face
//! expressions" and "Room sounds"): face expressions for lamp-eyes and, on a
//! lamp with no privacy slide, room sounds for lamp-ears. Each is a file in
//! [`FOLDER`], present only while the yes stands:
//! `{"version":1,KEY:"yes","at_unix_s":N,"by":BY}`, `root:lampos` 0640.
//!
//! The voice's onboarding writes one on a spoken yes (`"by":"voice"`), and
//! root's helper (`lamp-setup change`) on the owner's switch in the app
//! (`"by":"app"`, `consent.set` with `LAMP_APP_CONSENT=on`). One copy of the
//! names and the file's shape, read by `lamp-cloud`, the helper and the
//! voice. The readers (`lamp_eyes::seems`, `lamp_ears::classes`) read only
//! `KEY` and keep their own copy of the rule; a file from before `by` is the
//! voice's.
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The switch on `lamp-cloud`: `on` exactly takes [`KINDS`] from the app.
pub const ENVIRONMENT: &str = "LAMP_APP_CONSENT";

/// The app's kinds: `consent.get` (`{}`) and `consent.set` (one key).
pub const KINDS: [&str; 2] = ["consent.get", "consent.set"];

/// The folder both yeses are kept in (`deploy/product/lampos.conf`: 0750
/// `root:lampos`).
pub const FOLDER: &str = "/var/lib/lampOS/consent";

/// The face expressions yes's key, in the file and on the wire.
pub const FACE_EXPRESSIONS: &str = "face_expressions";
/// The room sounds yes's key, in the file and on the wire.
pub const ROOM_SOUNDS: &str = "room_sounds";

/// The keys, each with its file's name in [`FOLDER`].
pub const FILES: [(&str, &str); 2] = [
    (FACE_EXPRESSIONS, "face-expressions.json"),
    (ROOM_SOUNDS, "room-sounds.json"),
];

/// `by` for a yes given in the app.
pub const BY_APP: &str = "app";
/// `by` for a yes spoken at onboarding.
pub const BY_VOICE: &str = "voice";

/// The largest file read, as the readers' own bound.
pub const MOST_BYTES: u64 = 4096;

/// `key`'s file in `folder`, or none for a key that is neither.
pub fn file_in(folder: &Path, key: &str) -> Option<PathBuf> {
    FILES
        .iter()
        .find(|(known, _)| *known == key)
        .map(|(_, name)| folder.join(name))
}

/// What a kept yes file says, read as the readers read it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    /// `KEY` is exactly `"yes"`.
    pub yes: bool,
    /// Who gave it: `"app"`, `"voice"`, or none (a file from before `by`).
    pub by: Option<String>,
    /// When, in Unix seconds, if said.
    pub at_unix_s: Option<u64>,
}

impl Kept {
    /// Given in the app.
    pub fn by_app(&self) -> bool {
        self.by.as_deref() == Some(BY_APP)
    }
}

/// `path`'s yes for `key`. Missing, unreadable, over [`MOST_BYTES`], not
/// JSON or not an object is no yes, with nothing else said.
pub fn read(path: &Path, key: &str) -> Kept {
    let Ok(file) = std::fs::File::open(path) else {
        return Kept::default();
    };
    let mut text = String::new();
    if file.take(MOST_BYTES + 1).read_to_string(&mut text).is_err()
        || text.len() as u64 > MOST_BYTES
    {
        return Kept::default();
    }
    let Ok(Value::Object(value)) = serde_json::from_str::<Value>(&text) else {
        return Kept::default();
    };
    Kept {
        yes: value.get(key).and_then(Value::as_str) == Some("yes"),
        by: value.get("by").and_then(Value::as_str).map(str::to_owned),
        at_unix_s: value.get("at_unix_s").and_then(Value::as_u64),
    }
}

/// A yes file's bytes, whole: `{"version":1,KEY:"yes","at_unix_s":N,"by":BY}`.
pub fn bytes(key: &str, at_unix_s: u64, by: &str) -> Vec<u8> {
    let mut object = serde_json::Map::new();
    object.insert("version".into(), 1.into());
    object.insert(key.into(), "yes".into());
    object.insert("at_unix_s".into(), at_unix_s.into());
    object.insert("by".into(), by.into());
    serde_json::to_vec(&Value::Object(object)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_yes_is_read_with_who_gave_it() {
        let folder = std::env::temp_dir().join(format!("msgs-consent-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = file_in(&folder, FACE_EXPRESSIONS).unwrap();
        assert_eq!(read(&file, FACE_EXPRESSIONS), Kept::default(), "missing");
        std::fs::write(&file, bytes(FACE_EXPRESSIONS, 7, BY_APP)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            r#"{"at_unix_s":7,"by":"app","face_expressions":"yes","version":1}"#
        );
        let kept = read(&file, FACE_EXPRESSIONS);
        assert!(kept.yes && kept.by_app() && kept.at_unix_s == Some(7));
        // A file from before `by` is a yes nobody named: the voice's.
        std::fs::write(
            &file,
            r#"{"version":1,"face_expressions":"yes","at_unix_s":3}"#,
        )
        .unwrap();
        let kept = read(&file, FACE_EXPRESSIONS);
        assert!(kept.yes && !kept.by_app() && kept.by.is_none());
        // Another key's yes, a "no", JSON that is no object: no yes.
        assert!(!read(&file, ROOM_SOUNDS).yes);
        for text in [r#"{"face_expressions":"no"}"#, "[1]", "yes"] {
            std::fs::write(&file, text).unwrap();
            assert!(!read(&file, FACE_EXPRESSIONS).yes, "{text}");
        }
        std::fs::write(&file, vec![b' '; MOST_BYTES as usize + 1]).unwrap();
        assert!(!read(&file, FACE_EXPRESSIONS).yes, "too long");
        assert_eq!(file_in(&folder, "camera"), None);
        let _ = std::fs::remove_dir_all(folder);
    }
}
